//! Turrets: a small deck of their own that turns over the hull.
//!
//! A Turret Ring (1×1, 3×3 or 5×5) sits on the hull deck. Above it stands a
//! TurretBody: an entity of its own, a child of the ship, that rotates. The
//! turret's floor is plating (hull segments within the ring's reach) and the
//! guns and parts on it are ordinary modules parented to the body, laid out
//! on the turret's own little grid with (0, 0) at the ring's centre.
//!
//! Hanging the deck off its own entity keeps it out of everything that reads
//! the hull deck -- rooms, walk maps, power, break-off -- which only ever see
//! the ship's direct children. Bevy's transforms turn it with the ship.
//!
//! How fast a turret turns is its Traverse Motors (on the hull deck, touching
//! the ring) against everything standing on it (combat::gun_math).

use bevy::prelude::*;

use crate::components::{
    DestroyedModule, HullLayer, HullMaterial, HullSegment, Module, ModuleType, Ship,
};

/// Where the player is aiming this frame, in world space: a locked block, else
/// the selected target, else the cursor (or a scripted run's DEPTHS_AIM_AT).
/// Turrets turn toward it.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct PlayerAimPoint(pub Option<Vec2>);

pub fn update_player_aim_point(
    mut aim: ResMut<PlayerAimPoint>,
    lock: Res<crate::combat::targeting::AimLock>,
    selection: Res<crate::combat::targeting::TargetSelection>,
    targets: Query<&GlobalTransform>,
    windows: Query<&Window>,
    camera: Query<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    ship: Query<&GlobalTransform, With<Ship>>,
) {
    let cursor = windows
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .and_then(|c| camera.single().ok().and_then(|(cam, gt)| cam.viewport_to_world_2d(gt, c).ok()));
    let scripted = crate::combat::new_projectiles::aim_at_override()
        .and_then(|p| ship.single().ok().map(|s| s.transform_point(p.extend(0.0)).truncate()));
    aim.0 = lock
        .aim_point()
        .or_else(|| selection.target.and_then(|t| targets.get(t).ok()).map(|g| g.translation().truncate()))
        .or(cursor)
        .or(scripted);
}

/// The rotating deck above a turret ring.
#[derive(Component, Debug, Clone)]
pub struct TurretBody {
    /// The Turret Ring module it stands on.
    pub ring: Entity,
    /// That ring's grid cell on the hull deck: how designs and saves say
    /// which turret a block belongs to.
    pub ring_cell: IVec2,
    /// 1, 3 or 5.
    pub size: i32,
    /// Ship-local heading of the turret's +x axis, radians.
    pub angle: f32,
    /// Degrees a second it can turn right now.
    pub traverse_deg_s: f32,
    pub mass_t: f32,
    pub motors: u32,
}

/// On a Turret Ring: the body standing on it.
#[derive(Component, Debug, Clone, Copy)]
pub struct HasTurretBody(pub Entity);

/// A block standing on a turret deck. Its Module.grid_position is in the
/// turret's own grid.
#[derive(Component, Debug, Clone, Copy)]
pub struct OnTurret {
    pub ring_cell: IVec2,
}

/// A turret's floor plating. Made from the ring, so it is never saved.
#[derive(Component, Debug, Clone, Copy)]
pub struct TurretPlate;

/// A block that belongs on the turret standing on the ring at this hull
/// cell, waiting for that turret to exist (designs and saves spawn every
/// block in one go; the turret body comes a frame later).
#[derive(Component, Debug, Clone, Copy)]
pub struct PendingTurretMount(pub IVec2);

/// The ship something on a ship belongs to: its parent, or for a block on a
/// turret deck, the ship the turret stands on.
pub fn ship_of(
    entity: Entity,
    parents: &Query<&ChildOf>,
    bodies: &Query<(), With<TurretBody>>,
) -> Option<Entity> {
    let p = parents.get(entity).ok()?.parent();
    if bodies.contains(p) {
        parents.get(p).ok().map(|g| g.parent())
    } else {
        Some(p)
    }
}

pub fn is_turret_ring(t: ModuleType) -> bool {
    matches!(t, ModuleType::TurretRingSmall | ModuleType::TurretRing | ModuleType::TurretRingLarge)
}

pub fn ring_size(t: ModuleType) -> i32 {
    match t {
        ModuleType::TurretRingSmall => 1,
        ModuleType::TurretRingLarge => 5,
        _ => 3,
    }
}

/// How far from the ring's centre a turret's deck reaches, in cells.
pub fn deck_radius(size: i32) -> f32 {
    size as f32 / 2.0 + 1.05
}

/// The turret deck's cells: everything within its radius.
pub fn deck_cells(size: i32) -> Vec<IVec2> {
    let r = deck_radius(size);
    let span = r.ceil() as i32;
    let mut cells = Vec::new();
    for x in -span..=span {
        for y in -span..=span {
            if Vec2::new(x as f32, y as f32).length() <= r {
                cells.push(IVec2::new(x, y));
            }
        }
    }
    cells
}

/// Tonnes, for working out how fast a turret turns. The Gun Deck preview's
/// table.
pub fn part_mass_t(t: ModuleType) -> f32 {
    match t {
        ModuleType::FiringPiece => 4.0,
        ModuleType::GaugeIncreaser => 1.5,
        ModuleType::Loader => 1.2,
        ModuleType::ClipRack => 0.8,
        ModuleType::ShellFeeder => 0.5,
        ModuleType::BreechCooler => 1.5,
        ModuleType::RecoilAbsorber => 2.0,
        ModuleType::GunBarrel => 0.9,
        ModuleType::MuzzleBrake => 1.0,
        ModuleType::Mantlet => 3.0,
        ModuleType::RailMagnet => 1.5,
        _ => 1.0,
    }
}
pub const PLATE_MASS_T: f32 = 0.5;

/// The hull cells a ring covers. From its real footprint: a square block
/// laid with a turn still grows from its origin cell in the turned
/// direction, so origin + (half, half) is only right facing North.
pub fn ring_cells(ring: &Module) -> Vec<IVec2> {
    crate::building::GridOccupancy::cells_for(
        ring.grid_position,
        ring.size,
        ring.rotation,
        crate::building::footprints::footprint_override(ring.module_type),
    )
    .into_iter()
    .collect()
}

/// Where a turret body stands, ship-local: the centre of its ring.
fn ring_centre(ring: &Module) -> (IVec2, Vec2) {
    let cells = ring_cells(ring);
    let sum = cells.iter().fold(IVec2::ZERO, |a, c| a + *c);
    let centre = sum / cells.len().max(1) as i32;
    (centre, crate::building::grid_to_local(centre))
}

/// Give every ring a turret body, and take the body away when its ring goes.
pub fn ensure_turret_bodies(
    mut commands: Commands,
    // A load marks the old ship's blocks to go at the end of the frame; a
    // ring on its way out is no ring.
    rings: Query<(Entity, &Module, &ChildOf, Option<&HasTurretBody>), (Without<DestroyedModule>, Without<crate::meta::DespawnAtFrameEnd>)>,
    bodies: Query<(Entity, &TurretBody)>,
    asset_server: Res<AssetServer>,
) {
    for (ring, module, parent, has) in rings.iter() {
        if !is_turret_ring(module.module_type) || has.is_some() {
            continue;
        }
        let size = ring_size(module.module_type);
        let (_, at) = ring_centre(module);
        let body = commands
            .spawn((
                TurretBody {
                    ring,
                    ring_cell: module.grid_position,
                    size,
                    angle: 0.0,
                    traverse_deg_s: 0.0,
                    mass_t: 0.0,
                    motors: 0,
                },
                // Above the hull (0.1) and its modules (0.2-0.4): the turret
                // stands on the deck. Its own blocks layer within this.
                Transform::from_translation(at.extend(1.2)),
                Visibility::default(),
                ChildOf(parent.parent()),
                Name::new("Turret"),
            ))
            .id();
        let texture = asset_server.load("sprites/gun_parts/turret_plating.png");
        let material = HullMaterial::Steel;
        let health = 100.0 * material.health_multiplier();
        for cell in deck_cells(size) {
            commands.spawn((
                Sprite { image: texture.clone(), custom_size: Some(Vec2::splat(66.0)), ..default() },
                Transform::from_xyz(cell.x as f32 * 66.0, cell.y as f32 * 66.0, 0.0),
                HullSegment {
                    hull_layer: HullLayer::Outer,
                    material,
                    radiation_shielding: material.radiation_shielding(),
                    health,
                    max_health: health,
                    grid_position: cell,
                    ..default()
                },
                crate::components::BaseSpriteColor(Color::WHITE),
                crate::building::Block::hull(cell, material),
                TurretPlate,
                ChildOf(body),
            ));
        }
        commands.entity(ring).try_insert(HasTurretBody(body));
    }
    // A body whose ring has gone (shot off, removed, a load replaced the
    // ship) goes with it. Shooting the ring should jam the turret instead;
    // that comes with turret damage.
    for (body, t) in bodies.iter() {
        if rings.get(t.ring).is_err() {
            commands.entity(body).try_despawn();
        }
    }
}

/// Move blocks waiting for a turret onto it, once it exists.
pub fn mount_pending_turret_blocks(
    mut commands: Commands,
    mut pending: Query<(Entity, &PendingTurretMount, &ChildOf, &mut Transform)>,
    bodies: Query<(Entity, &TurretBody, &ChildOf)>,
    live_rings: Query<(), (With<Module>, Without<DestroyedModule>, Without<crate::meta::DespawnAtFrameEnd>)>,
) {
    for (block, want, parent, mut transform) in pending.iter_mut() {
        let ship = parent.parent();
        // Only a turret whose ring is alive: right after a load the old
        // ship's turret is still there for a frame, on a ring that's going,
        // and anything mounted on it went with it.
        let Some((body, _, _)) = bodies
            .iter()
            .find(|(_, t, p)| p.parent() == ship && t.ring_cell == want.0 && live_rings.contains(t.ring))
        else {
            continue;
        };
        // spawn_module laid it out in the ship's frame, where cell (x, y)
        // stands at (66x, 66y - 33). The turret's frame has no half-cell
        // offset: (0, 0) is the ring's centre.
        transform.translation.y += 33.0;
        commands
            .entity(block)
            .remove::<PendingTurretMount>()
            .insert((OnTurret { ring_cell: want.0 }, ChildOf(body)));
    }
}

/// Weigh each turret, count its motors, and turn it toward what it should
/// point at -- the player's aim for the player's ship -- no faster than it
/// can.
pub fn traverse_turrets(
    time: Res<Time>,
    mut bodies: Query<(&mut TurretBody, &mut Transform, &ChildOf, Option<&Children>)>,
    parts: Query<&Module, Without<DestroyedModule>>,
    plates: Query<(), (With<TurretPlate>, Without<crate::components::HullDestroyed>)>,
    ships: Query<(&GlobalTransform, Option<&Children>), With<Ship>>,
    hull_modules: Query<&Module, Without<DestroyedModule>>,
    rings: Query<&Module>,
    aim: Res<PlayerAimPoint>,
) {
    let dt = time.delta_secs();
    for (mut body, mut transform, parent, children) in bodies.iter_mut() {
        let mut mass = 0.0;
        for c in children.into_iter().flatten() {
            if let Ok(m) = parts.get(*c) {
                mass += part_mass_t(m.module_type);
            } else if plates.contains(*c) {
                mass += PLATE_MASS_T;
            }
        }
        body.mass_t = mass;
        // Motors: Traverse Motor blocks on the hull deck touching the ring.
        let ring_cells: Vec<IVec2> = rings.get(body.ring).map(ring_cells).unwrap_or_default();
        let Ok((ship_gt, ship_children)) = ships.get(parent.parent()) else {
            // Not the player's ship: AI turrets aim with AI weapons later.
            continue;
        };
        body.motors = ship_children
            .into_iter()
            .flatten()
            .filter_map(|c| hull_modules.get(*c).ok())
            .filter(|m| m.module_type == ModuleType::TraverseMotor)
            .filter(|m| ring_cells.iter().any(|r| (m.grid_position - *r).abs().max_element() <= 1 && !ring_cells.contains(&m.grid_position)))
            .count() as u32;
        body.traverse_deg_s = if body.motors == 0 { 0.0 } else { super::gun_math::traverse_deg_per_s(body.motors, body.mass_t) };

        let Some(target) = aim.0 else { continue };
        // Heading to the aim point, in the ship's frame.
        let centre = ship_gt.affine().transform_point3(transform.translation.with_z(0.0)).truncate();
        let world = (target - centre).to_angle();
        let ship_rot = ship_gt.to_scale_rotation_translation().1.to_euler(EulerRot::ZYX).0;
        let want = world - ship_rot;
        let mut diff = (want - body.angle).rem_euclid(std::f32::consts::TAU);
        if diff > std::f32::consts::PI {
            diff -= std::f32::consts::TAU;
        }
        let max = body.traverse_deg_s.to_radians() * dt;
        body.angle += diff.clamp(-max, max);
        transform.rotation = Quat::from_rotation_z(body.angle);
    }
}

#[cfg(test)]
mod turret_deck_tests {
    use super::*;

    #[test]
    fn a_bigger_ring_carries_a_bigger_deck() {
        let small = deck_cells(1).len();
        let medium = deck_cells(3).len();
        let large = deck_cells(5).len();
        assert!(small < medium && medium < large, "{small} {medium} {large}");
        assert!(deck_cells(3).contains(&IVec2::new(2, 1)));
        assert!(!deck_cells(3).contains(&IVec2::new(3, 0)));
    }

    #[test]
    fn rings_are_turret_rings() {
        assert!(is_turret_ring(ModuleType::TurretRing));
        assert!(!is_turret_ring(ModuleType::FiringPiece));
        assert_eq!(ring_size(ModuleType::TurretRingLarge), 5);
    }
}
