use bevy::prelude::*;
use crate::components::*;
use crate::events::*;
use crate::celestial::components::{GravityAffected, GravityForce};

// ============================================================================
// SECTION SEVERANCE
// When the connection between ship sections is destroyed, the detached
// section breaks off with ship velocity + impact force.
// Becomes debris. If it had reactor/ammo = tumbling bomb.
// ============================================================================

/// Marker for a detached ship section — flying debris with momentum
#[derive(Component)]
pub struct DetachedSection {
    pub has_reactor: bool,
    pub has_ammo: bool,
    pub has_fuel: bool,
    pub meltdown_timer: Option<f32>,
    pub mass: f32,
}

/// Marks a block that broke away with a severed section. It is still a hull
/// segment or module -- it draws, burns and cooks off like one -- but it is
/// no longer the player's, so saving leaves it out.
#[derive(Component)]
pub struct SeveredBlock;

/// The blocks of the player's ship that were joined to its main body the last
/// time this looked. Only these can break away: a piece a designer left
/// floating on purpose was never attached, so losing a plate elsewhere doesn't
/// fling it off.
#[derive(Default)]
pub struct AttachedBefore {
    ship: Option<Entity>,
    blocks: bevy::platform::collections::HashSet<Entity>,
}

/// A section breaks away when nothing joins it to the ship any more.
///
/// After any of the player's blocks is destroyed, the ship's live blocks are
/// grouped by what touches what -- sides AND corners, because hulls step
/// diagonally and a wedge meets its neighbour at a corner. The biggest group
/// is the ship. Every other group that was joined to it a moment ago breaks
/// off as ONE body: a new root carrying the blocks in their places, with the
/// ship's velocity plus a shove outward, its own collider, and the reactor /
/// ammo / fuel aboard deciding whether it becomes a bomb.
///
/// It replaces a guess -- "a module within 130 units of a hole and further
/// from the centre than from the hole is severed" -- which on a big hull
/// threw off nearly every module beside any hole near an edge: a capital
/// losing ten blocks a second shed thirty modules a second, each as its own
/// spinning chunk.
///
/// Only destruction severs. Removing blocks in build mode never does, and the
/// player's ship only -- AI ships break up through their own death rules.
pub fn check_section_severance(
    mut commands: Commands,
    ship_query: Query<(Entity, &Transform, &Velocity, &crate::building::layout::LayoutVersion, &Children), With<Ship>>,
    fresh_destroyed: Query<&ChildOf, Or<(Added<HullDestroyed>, Added<DestroyedModule>)>>,
    hulls: Query<(&HullSegment, &Transform), Without<HullDestroyed>>,
    modules: Query<(&Module, &Transform), Without<DestroyedModule>>,
    mut seen: Local<Option<crate::building::layout::LayoutVersion>>,
    mut attached: Local<AttachedBefore>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let Ok((ship, ship_transform, ship_velocity, version, children)) = ship_query.single() else { return };
    let destroyed_now = fresh_destroyed.iter().any(|p| p.parent() == ship);
    if attached.ship == Some(ship) && *seen == Some(*version) && !destroyed_now {
        return;
    }
    *seen = Some(*version);

    // Live blocks by cell, and where each block sits in the ship.
    let mut cell_block: bevy::platform::collections::HashMap<IVec2, Entity> = Default::default();
    let mut local_pos: bevy::platform::collections::HashMap<Entity, Vec2> = Default::default();
    for child in children.iter() {
        if let Ok((hull, transform)) = hulls.get(child) {
            cell_block.insert(hull.grid_position, child);
            local_pos.insert(child, transform.translation.truncate());
        } else if let Ok((module, transform)) = modules.get(child) {
            let footprint = crate::building::footprints::footprint_override(module.module_type);
            for cell in crate::building::ShipGrid::cells_for(module.grid_position, module.size, module.rotation, footprint) {
                cell_block.insert(cell, child);
            }
            local_pos.insert(child, transform.translation.truncate());
        }
    }

    let groups = touching_groups(&cell_block);
    let Some(main) = groups.iter().max_by_key(|g| g.len()) else { return };

    // First look at this ship (or a new one): learn what is attached, sever nothing.
    if attached.ship != Some(ship) || !destroyed_now {
        attached.ship = Some(ship);
        attached.blocks = main.clone();
        return;
    }

    for group in &groups {
        if std::ptr::eq(group, main) || !group.iter().any(|e| attached.blocks.contains(e)) {
            continue;
        }
        let blocks: Vec<(Entity, Vec2)> = group.iter().filter_map(|e| local_pos.get(e).map(|p| (*e, *p))).collect();
        let module_types: Vec<ModuleType> = group.iter().filter_map(|e| modules.get(*e).ok().map(|(m, _)| m.module_type)).collect();
        break_off(&mut commands, &mut notifications, ship_transform, ship_velocity, &blocks, &module_types);
    }
    attached.blocks = main.clone();
}

/// Groups of blocks joined by a side or a corner.
fn touching_groups(
    cell_block: &bevy::platform::collections::HashMap<IVec2, Entity>,
) -> Vec<bevy::platform::collections::HashSet<Entity>> {
    let mut seen_cells: bevy::platform::collections::HashSet<IVec2> = Default::default();
    let mut groups = Vec::new();
    for &start in cell_block.keys() {
        if !seen_cells.insert(start) {
            continue;
        }
        let mut group: bevy::platform::collections::HashSet<Entity> = Default::default();
        let mut stack = vec![start];
        while let Some(cell) = stack.pop() {
            group.insert(cell_block[&cell]);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let next = cell + IVec2::new(dx, dy);
                    if cell_block.contains_key(&next) && seen_cells.insert(next) {
                        stack.push(next);
                    }
                }
            }
        }
        groups.push(group);
    }
    groups
}

/// Re-root one severed group under a new body, in place, and send it off.
fn break_off(
    commands: &mut Commands,
    notifications: &mut MessageWriter<ShowNotification>,
    ship_transform: &Transform,
    ship_velocity: &Velocity,
    blocks: &[(Entity, Vec2)],
    module_types: &[ModuleType],
) {
    if blocks.is_empty() {
        return;
    }
    let centroid = blocks.iter().map(|(_, p)| *p).sum::<Vec2>() / blocks.len() as f32;
    let has_reactor = module_types.iter().any(|t| matches!(t,
        ModuleType::SmallReactor | ModuleType::StandardReactor | ModuleType::LargeReactor | ModuleType::FusionReactor));
    let has_ammo = module_types.iter().any(|t| matches!(t, ModuleType::AmmoFeedUnit | ModuleType::WarheadBay));
    let has_fuel = module_types.contains(&ModuleType::FuelTank);
    let mass = (blocks.len() as f32 * 8.0).max(50.0);

    // The new root sits where the section's centre is, turned like the ship,
    // so the blocks keep their exact places and it tumbles about its middle.
    let world_centre = ship_transform.transform_point(centroid.extend(0.0));
    let outward = (ship_transform.rotation * centroid.extend(0.0)).truncate().normalize_or_zero();
    let velocity = ship_velocity.0 + outward * 60.0;
    let points: Vec<Vec2> = blocks.iter().map(|(_, p)| *p - centroid).collect();
    let root = commands.spawn((
        Name::new("Severed section"),
        Transform { translation: world_centre, rotation: ship_transform.rotation, scale: Vec3::ONE },
        Visibility::default(),
        DetachedSection {
            has_reactor,
            has_ammo,
            has_fuel,
            meltdown_timer: if has_reactor { Some(10.0) } else { None },
            mass,
        },
        Velocity(velocity),
        GravityAffected { mass },
        GravityForce::default(),
        // A real body: it caroms off hulls and rocks instead of ghosting
        // through. Shots still pass through debris (collision::TerrainFilter).
        crate::ship::collision::fit_ship_collider(&points, mass),
    )).id();
    for (block, local) in blocks {
        let at = (*local - centroid).extend(0.0);
        commands.entity(*block).insert((SeveredBlock, ChildOf(root))).entry::<Transform>().and_modify(move |mut t| {
            t.translation.x = at.x;
            t.translation.y = at.y;
        });
    }

    notifications.write(ShowNotification {
        message: format!("SECTION SEVERED! {} blocks broke away", blocks.len()),
        notification_type: NotificationType::Danger,
        duration: 4.0,
    });
    if has_reactor {
        notifications.write(ShowNotification {
            message: "WARNING: Severed section contains reactor! Meltdown in 10s!".into(),
            notification_type: NotificationType::Danger,
            duration: 5.0,
        });
    }
}

/// Move detached sections — they tumble and drift, affected by gravity
pub fn move_detached_sections(
    time: Res<Time>,
    mut commands: Commands,
    fx: Res<crate::vfx::effect_textures::EffectTextures>,
    mut section_query: Query<(Entity, &mut DetachedSection, &mut Transform, &Velocity)>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let dt = time.delta_secs();

    for (entity, mut section, mut transform, velocity) in section_query.iter_mut() {
        // Move. Gravity is already in Velocity (apply_gravity_to_velocity
        // integrates it for everything GravityAffected); adding the raw
        // acceleration to POSITION here was a second pull in the wrong units.
        transform.translation.x += velocity.0.x * dt;
        transform.translation.y += velocity.0.y * dt;

        // Tumble
        transform.rotation *= Quat::from_rotation_z(1.5 * dt);

        // Reactor meltdown countdown
        if let Some(ref mut timer) = section.meltdown_timer {
            *timer -= dt;
            if *timer <= 0.0 {
                // BOOM
                notifications.write(ShowNotification {
                    message: "Severed reactor EXPLODED!".into(),
                    notification_type: NotificationType::Danger,
                    duration: 3.0,
                });

                let pos = transform.translation.truncate();
                super::spawn_explosion(&mut commands, &fx, pos, 90.0, Color::srgb(1.0, 0.5, 0.1));

                commands.entity(entity).try_despawn();
                // This section is gone; don't fall through to the ammo
                // cook-off below and queue a second despawn for the same
                // entity. A severed reactor section usually has ammo aboard
                // too, so both branches fired on the same debris and the
                // second one errored ("Entity despawned") in the middle of a
                // fight.
                continue;
            }
        }

        // Ammo cook-off (random chance each second)
        if section.has_ammo && rand::random::<f32>() < 0.02 * dt {
            let pos = transform.translation.truncate();
            super::spawn_explosion(&mut commands, &fx, pos, 36.0, Color::srgb(1.0, 0.4, 0.1));
            notifications.write(ShowNotification {
                message: "Ammo cook-off in debris!".into(),
                notification_type: NotificationType::Warning,
                duration: 2.0,
            });
            commands.entity(entity).try_despawn();
        }
    }
}

/// Detached sections can collide with creatures and damage them
pub fn debris_collision(
    mut commands: Commands,
    section_query: Query<(Entity, &DetachedSection, &Transform, &Velocity)>,
    mut creature_query: Query<(Entity, &Transform, &mut Creature), Without<DetachedSection>>,
) {
    for (section_entity, section, section_transform, section_vel) in section_query.iter() {
        let section_pos = section_transform.translation.truncate();
        let speed = section_vel.0.length();

        if speed < 20.0 { continue; } // Not moving fast enough to hurt

        for (_creature_entity, creature_transform, mut creature) in creature_query.iter_mut() {
            if creature.health <= 0.0 { continue; }

            let creature_pos = creature_transform.translation.truncate();
            let dist = section_pos.distance(creature_pos);

            if dist < 60.0 {
                // COLLISION! Damage based on speed and mass
                let impact_damage = speed * section.mass * 0.01;
                creature.health -= impact_damage;

                super::spawn_hit_effect(&mut commands, section_pos, Color::srgb(0.8, 0.6, 0.2), 20.0);
                super::spawn_floating_damage(&mut commands, section_pos, impact_damage, Color::srgb(0.7, 0.5, 0.2));

                // Destroy the debris on impact
                commands.entity(section_entity).try_despawn();
                break;
            }
        }
    }
}

#[cfg(test)]
mod severance_tests {
    use super::*;

    fn hull_at(x: i32, y: i32) -> (HullSegment, Transform) {
        (
            HullSegment {
                health: 100.0,
                max_health: 100.0,
                radiation_shielding: 0.0,
                is_depressurized: false,
                depressurization_level: 0.0,
                hull_layer: HullLayer::Inner,
                material: HullMaterial::Steel,
                grid_position: IVec2::new(x, y),
            },
            Transform::from_xyz(x as f32 * 66.0, y as f32 * 66.0 - 33.0, 0.0),
        )
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(crate::building::layout::LayoutPlugin);
        app.add_message::<ShowNotification>();
        app.add_systems(Update, check_section_severance);
        app
    }

    /// A ship that is a row of hull at x = 0..len, plus any extra cells.
    fn ship(app: &mut App, len: i32, extra: &[(i32, i32)]) -> (Entity, Vec<Entity>) {
        let ship = app.world_mut().spawn((Ship, Transform::default(), Velocity(Vec2::ZERO))).id();
        let mut blocks: Vec<Entity> = (0..len).map(|x| app.world_mut().spawn((hull_at(x, 0), ChildOf(ship))).id()).collect();
        for &(x, y) in extra {
            blocks.push(app.world_mut().spawn((hull_at(x, y), ChildOf(ship))).id());
        }
        app.update(); // learn the attached body
        (ship, blocks)
    }

    fn sections(app: &mut App) -> Vec<Entity> {
        app.world_mut().query_filtered::<Entity, With<DetachedSection>>().iter(app.world()).collect()
    }

    #[test]
    fn cutting_a_row_in_two_sends_the_smaller_end_off_as_one_piece() {
        let mut app = app();
        let (ship, blocks) = ship(&mut app, 7, &[]);
        app.world_mut().entity_mut(blocks[4]).insert(HullDestroyed);
        app.update();
        let found = sections(&mut app);
        assert_eq!(found.len(), 1, "one severed section, not one per block");
        for &b in &blocks[5..] {
            assert_eq!(app.world().get::<ChildOf>(b).unwrap().parent(), found[0]);
            assert!(app.world().get::<SeveredBlock>(b).is_some());
        }
        for &b in &blocks[..4] {
            assert_eq!(app.world().get::<ChildOf>(b).unwrap().parent(), ship, "the bigger part stays the ship");
        }
    }

    #[test]
    fn blocks_joined_only_at_a_corner_hold_together() {
        let mut app = app();
        // A diagonal step off the end of the row: (5,1) touches (4,0) at a corner.
        let (_, blocks) = ship(&mut app, 5, &[(5, 1), (6, 2)]);
        app.world_mut().entity_mut(blocks[0]).insert(HullDestroyed);
        app.update();
        assert!(sections(&mut app).is_empty());
    }

    #[test]
    fn a_piece_built_apart_on_purpose_is_never_flung_off() {
        let mut app = app();
        // (0,5) never touched the hull.
        let (ship, blocks) = ship(&mut app, 5, &[(0, 5)]);
        app.world_mut().entity_mut(blocks[0]).insert(HullDestroyed);
        app.update();
        assert!(sections(&mut app).is_empty());
        assert_eq!(app.world().get::<ChildOf>(blocks[5]).unwrap().parent(), ship);
    }

    #[test]
    fn removing_a_block_without_destroying_it_severs_nothing() {
        let mut app = app();
        let (_, blocks) = ship(&mut app, 7, &[]);
        app.world_mut().entity_mut(blocks[4]).despawn(); // a build-mode delete
        app.update();
        assert!(sections(&mut app).is_empty());
    }
}
