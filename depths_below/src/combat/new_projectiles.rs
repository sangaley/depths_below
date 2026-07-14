use bevy::prelude::*;
use crate::celestial::components::{GravityAffected, GravityForce};
use super::targeting::{TargetSelection, FireGroupState, FireGroup, lead_prediction::*};
use super::*;

// ============================================================================
// NEW PROJECTILE SYSTEM
// Every projectile is a real entity: position, velocity, gravity-affected.
// Kinetic rounds have penetration. Angled hits ricochet.
// ============================================================================

/// A projectile entity
#[derive(Component)]
pub struct Projectile {
    pub damage: f32,
    pub speed: f32,
    pub lifetime: f32,
    pub max_lifetime: f32,
    pub owner: Entity,
    pub damage_type: ProjectileDamageType,
    pub penetration: f32,     // How much armor it can go through
    pub has_penetrated: bool, // Already went through one layer
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProjectileDamageType {
    Kinetic,        // AP rounds — penetrates
    Explosive,      // HE rounds — area on impact
    Incendiary,     // Sets fires
    EmpRound,       // Disables modules
}

/// Missile entity — has guidance and fuel
#[derive(Component)]
pub struct MissileProjectile {
    pub damage: f32,
    pub target: Option<Entity>,
    pub burn_fuel: f32,        // Fuel for main engine
    pub reserve_fuel: f32,     // Fuel for course corrections
    pub thrust: f32,
    pub tracking_agility: f32, // How fast it can turn (rad/s)
    pub armed: bool,           // Needs to travel min distance before arming
    pub arm_distance: f32,
    pub traveled: f32,
    pub blast_radius: f32,
    pub owner: Entity,
}

/// Visual trail marker for projectiles
#[derive(Component)]
pub struct ProjectileTrail {
    pub color: Color,
    pub width: f32,
    pub fade_time: f32,
}

// ============================================================================
// WEAPON FIRING SYSTEM — uses fire groups + lead prediction
// ============================================================================

/// Main weapon firing system: reads fire groups, aims with lead prediction, spawns projectiles
pub fn fire_weapons_system(
    time: Res<Time>,
    fire_state: Res<FireGroupState>,
    power_state: Res<crate::resources::PowerState>,
    selection: Res<TargetSelection>,
    ship_query: Query<(Entity, &Transform, &ShipPhysics, &Velocity), With<Ship>>,
    mut weapon_query: Query<(
        Entity, &Module, &mut Weapon, &mut WeaponCooldown,
        &GlobalTransform, &FireGroup, &WeaponMount, &ChildOf,
        Option<&crate::building::customization::parameters::ModuleCustomization>,
        Option<&crate::building::customization::tuning::WeaponTuning>,
        Option<&crate::building::customization::tuning::SelectedAmmo>,
    ), Without<DestroyedModule>>,
    target_transform_query: Query<&Transform, Without<Ship>>,
    target_velocity_query: Query<&Velocity, Without<Ship>>,
    targeting_computer_query: Query<&Module, Without<DestroyedModule>>,
    windows_query: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    mut fired_events: MessageWriter<crate::events::WeaponFired>,
    mut commands: Commands,
) {
    let Ok((player_ship, _ship_transform, ship_physics, ship_velocity)) = ship_query.single() else { return };
    let _dt = time.delta_secs();

    // Weapons need power: a grid in deficit (e.g. shield surging under fire)
    // silences the guns until the balance recovers.
    if power_state.power_balance < 0.0 {
        return;
    }

    // Cursor world position — dumb-fire fallback when no target is selected.
    let cursor_world: Option<Vec2> = windows_query.single().ok()
        .and_then(|w| w.cursor_position())
        .and_then(|c| {
            camera_query.single().ok()
                .and_then(|(cam, gt)| cam.viewport_to_world_2d(gt, c).ok())
        });

    // Build module position list for adjacency checks
    let all_modules: Vec<(IVec2, ModuleType, bool)> = targeting_computer_query.iter()
        .map(|m| (m.grid_position, m.module_type, m.is_active))
        .collect();

    for (entity, module, mut weapon, mut cooldown, global_transform, fire_group, mount, parent, customization, tuning, selected_ammo) in weapon_query.iter_mut() {
        // Player ship only: this query has no ownership filter on its own, and
        // AI ships carry the exact same Weapon/FireGroup/WeaponMount
        // components (shared spawn_module path). Unscoped, holding Space
        // also fired every AI ship's default-group weapon at the player's
        // current target/cursor — including an AI ship shooting itself when
        // the player had it targeted.
        if parent.parent() != player_ship { continue; }
        // True kinetic weapons only. This loop has no type match arms below
        // for anything else — every OTHER weapon type (Laser, PlasmaCaster,
        // IonDisruptor, EMPPulse, the three missile types, MiningDrill,
        // TractorBeam) fell through the `_ =>` wildcard arms and phantom-fired
        // a generic small yellow bullet from this system IN ADDITION to
        // whatever their own dedicated firing system did (or, for
        // PlasmaCaster, didn't — it has no dedicated system at all). That
        // stray shot is what made the laser look broken: the real beam
        // can't hit ships (separate bug), but this phantom bullet could,
        // muddying what was actually happening.
        if !matches!(module.module_type,
            ModuleType::Cannon | ModuleType::Railgun | ModuleType::Coilgun | ModuleType::Gatling
        ) {
            continue;
        }
        if !module.is_active { continue; }

        // Tick cooldown
        cooldown.timer.tick(time.delta());
        if !cooldown.timer.is_finished() { continue; }

        // Check if this weapon's fire group is active
        let group_firing = fire_state.firing[fire_group.group as usize % 4];
        if !group_firing { continue; }

        // Check ammo
        if !crate::combat::INFINITE_AMMO && weapon.ammo <= 0 { continue; }

        let weapon_pos = global_transform.translation().truncate();

        // Aim at the selected target if there is one; otherwise dumb-fire at
        // the cursor. Never silently skip on range — an out-of-range target
        // just means the shot is capped at max range and falls short, which
        // is visible feedback instead of a gun that refuses to fire.
        let (target_pos, target_vel) = if let Some(target_entity) = selection.target {
            let Ok(target_transform) = target_transform_query.get(target_entity) else { continue };
            let mut target_pos = target_transform.translation.truncate();
            let to_target = target_pos - weapon_pos;
            if to_target.length() > weapon.range {
                target_pos = weapon_pos + to_target.normalize_or_zero() * weapon.range;
            }
            let vel = target_velocity_query.get(target_entity)
                .map(|v| v.0)
                .unwrap_or(Vec2::ZERO);
            (target_pos, vel)
        } else if let Some(cursor) = cursor_world {
            // Cap the aim point to weapon range along the cursor direction
            let to_cursor = cursor - weapon_pos;
            let aim = if to_cursor.length() > weapon.range {
                weapon_pos + to_cursor.normalize_or_zero() * weapon.range
            } else {
                cursor
            };
            (aim, Vec2::ZERO)
        } else {
            continue;
        };

        // Fixed/broadside mounts can't swivel outside their arc — without this
        // check a forward-facing cannon would fire at a target behind the
        // ship, sending the shot straight through the hull it's mounted on.
        if !is_in_firing_arc(ship_physics.rotation, &module.rotation, mount, target_pos - weapon_pos) {
            continue;
        }

        // Determine prediction tier — Targeting Computer must be ADJACENT to this weapon
        let has_adjacent_tc = crate::combat::targeting::lead_prediction::check_adjacent_targeting_computer(
            module.grid_position, &all_modules,
        );
        let tier = get_weapon_prediction_tier(module, customization, has_adjacent_tc);

        // Muzzle speed: per-type base (see tuning.rs — shared with the tuning
        // window's live readout) scaled by the velocity slider and the loaded
        // ammo's own velocity profile (APFSDS darts fly, HEAT crawls).
        let tuning_vel = tuning.map(|t| t.velocity).unwrap_or(1.0);
        let ammo_vel = selected_ammo.map(|a| a.0.velocity_mult()).unwrap_or(1.0);
        let proj_speed =
            crate::building::customization::tuning::base_projectile_speed(module.module_type)
            * tuning_vel * ammo_vel;

        // Get shooter velocity for relative prediction
        let shooter_vel = ship_velocity.0;

        // Calculate lead — accounts for shooter velocity, degrades with distance
        let prediction = calculate_lead(
            weapon_pos,
            shooter_vel,
            target_pos,
            target_vel,
            Vec2::ZERO,
            proj_speed,
            tier,
            weapon.range,
        );

        // Apply accuracy spread — worse at longer range
        let aim_point = apply_accuracy_spread(
            weapon_pos,
            prediction.aim_point,
            prediction.distance_accuracy * 0.85, // Distance degrades accuracy
            10.0, // Max spread degrees at worst accuracy
        );

        // Fire!
        let direction = (aim_point - weapon_pos).normalize_or_zero();
        cooldown.timer.reset();
        if !crate::combat::INFINITE_AMMO {
            weapon.ammo = weapon.ammo.saturating_sub(1);
        }
        fired_events.write(crate::events::WeaponFired {
            weapon_type: module.module_type,
            position: weapon_pos,
            from_player: true,
        });

        // Loaded ammo drives damage type, per-round damage, and penetration.
        // Without a SelectedAmmo (AI ships, pre-tuning saves) everything
        // falls back to the old per-weapon-type behavior.
        use crate::combat::ammo_types::KineticAmmoType;
        let (damage_type, ammo_damage_mult, penetration) = match selected_ammo.map(|a| a.0) {
            Some(ammo) => (
                match ammo {
                    KineticAmmoType::Incendiary => ProjectileDamageType::Incendiary,
                    KineticAmmoType::EMPShell => ProjectileDamageType::EmpRound,
                    KineticAmmoType::APHE | KineticAmmoType::HEFrag | KineticAmmoType::Flak =>
                        ProjectileDamageType::Explosive,
                    _ => ProjectileDamageType::Kinetic,
                },
                ammo.damage_mult(),
                ammo.penetration(),
            ),
            None => (
                ProjectileDamageType::Kinetic,
                1.0,
                match module.module_type {
                    ModuleType::Railgun => 80.0,   // Goes through almost anything
                    ModuleType::Cannon => 40.0,    // Decent penetration
                    ModuleType::Coilgun => 25.0,   // Light penetration
                    ModuleType::Gatling => 10.0,   // Barely penetrates
                    _ => 20.0,
                },
            ),
        };

        // Spawn projectile(s)
        let burst_count = match module.module_type {
            ModuleType::Coilgun => 3,  // 3-round burst
            ModuleType::Gatling => 1,  // Continuous stream (high fire rate handles it)
            _ => 1,
        };

        for _i in 0..burst_count {
            let spread_offset = if burst_count > 1 {
                Vec2::new(
                    (rand::random::<f32>() - 0.5) * 3.0,
                    (rand::random::<f32>() - 0.5) * 3.0,
                )
            } else {
                Vec2::ZERO
            };

            // Projectiles inherit the ship's own velocity — calculate_lead's
            // aim point assumes this (it solves using target_vel - shooter_vel
            // as the relative velocity, which only converges to a correct
            // intercept if the shot's world-frame speed is proj_speed +
            // shooter_vel). Without this, aiming while moving fast dragged
            // the computed aim point back toward the ship itself, collapsing
            // the shot's direction to near-zero — kinetic rounds barely
            // moved while sustained-thrusting toward a target.
            let vel = direction * proj_speed + shooter_vel + spread_offset;

            // Visual size/color based on weapon — sized and colored to read
            // clearly at gameplay zoom instead of every shot looking like
            // the same small yellow sliver.
            let (size, base_color) = match module.module_type {
                ModuleType::Railgun => (Vec2::new(50.0, 4.0), Color::srgb(0.2, 0.5, 1.0)),   // long blue streak
                ModuleType::Cannon => (Vec2::new(20.0, 12.0), Color::srgb(1.0, 0.45, 0.05)), // big orange shell
                ModuleType::Coilgun => (Vec2::new(12.0, 5.0), Color::srgb(0.6, 0.8, 1.0)),
                ModuleType::Gatling => (Vec2::new(8.0, 3.0), Color::srgb(1.0, 0.85, 0.2)),
                _ => (Vec2::new(8.0, 3.0), Color::srgb(0.8, 0.8, 0.4)),
            };
            // Loaded ammo recolors the round (AP brass, EMP blue, ...) so a
            // mixed loadout reads at a glance; size stays per-weapon.
            let color = selected_ammo.map(|a| a.0.color()).unwrap_or(base_color);

            let angle = vel.y.atan2(vel.x);

            commands.spawn((
                (Sprite {
                        color,
                        custom_size: Some(size),
                        ..default()
                    }, Transform {
                        translation: Vec3::new(weapon_pos.x, weapon_pos.y, 0.5),
                        rotation: Quat::from_rotation_z(angle),
                        ..default()
                    }),
                Projectile {
                    damage: weapon.damage * ammo_damage_mult,
                    speed: proj_speed,
                    lifetime: 4.0,
                    max_lifetime: 4.0,
                    owner: entity,
                    damage_type,
                    penetration,
                    has_penetrated: false,
                },
                Velocity(vel),
                GravityAffected { mass: 0.5 }, // Projectiles affected by gravity (slightly)
                GravityForce::default(),
            ));
        }

        // Muzzle flash
        let flash_color = match module.module_type {
            ModuleType::Railgun => Color::srgb(0.2, 0.5, 1.0),
            ModuleType::Cannon => Color::srgb(1.0, 0.45, 0.05),
            ModuleType::Coilgun => Color::srgb(0.6, 0.8, 1.0),
            ModuleType::Gatling => Color::srgb(1.0, 0.85, 0.2),
            _ => Color::srgb(0.8, 0.8, 0.4),
        };
        spawn_hit_effect(&mut commands, weapon_pos + direction * 30.0, flash_color, 12.0);
    }
}

// ============================================================================
// PROJECTILE MOVEMENT — gravity-affected, lifetime-limited
// ============================================================================

/// Move projectiles, apply gravity, tick lifetime, despawn expired
pub fn move_projectiles(
    time: Res<Time>,
    mut commands: Commands,
    mut proj_query: Query<(Entity, &mut Projectile, &mut Transform, &mut Velocity, &GravityForce)>,
) {
    let dt = time.delta_secs();

    for (entity, mut proj, mut transform, mut velocity, gravity) in proj_query.iter_mut() {
        // Apply gravity to velocity
        velocity.0 += gravity.0 * dt * 0.5; // Projectiles resist gravity more than ships

        // Move
        transform.translation.x += velocity.0.x * dt;
        transform.translation.y += velocity.0.y * dt;

        // Rotate to face movement direction
        if velocity.0.length_squared() > 1.0 {
            let angle = velocity.0.y.atan2(velocity.0.x);
            transform.rotation = Quat::from_rotation_z(angle);
        }

        // Age
        proj.lifetime -= dt;
        if proj.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

// ============================================================================
// PROJECTILE COLLISION — damage on hit, penetration, ricochet
// ============================================================================

/// Largest creature hit radius (Leviathan) — used to size the spatial grid query margin.
const MAX_CREATURE_HIT_RADIUS: f32 = 90.0;

/// Check projectile collisions with creatures and ships.
/// Uses the creature spatial grid to only distance-check creatures near each
/// projectile instead of every creature in the world.
pub fn check_projectile_hits(
    mut commands: Commands,
    proj_query: Query<(Entity, &Projectile, &Transform, &Velocity)>,
    mut creature_query: Query<(&Transform, &mut Creature), Without<Ship>>,
    creature_grid: Res<crate::spatial::CreatureGrid>,
    mut ai_ship_query: Query<
        (Entity, &Transform, &Children, &mut crate::combat::shields::ShipShield),
        With<crate::ai_ship::components::AiShip>,
    >,
    mut ai_module_query: Query<(&mut Module, &GlobalTransform), Without<DestroyedModule>>,
    mut ai_hull_query: Query<(&mut HullSegment, &GlobalTransform), Without<crate::components::HullDestroyed>>,
    owner_parent_query: Query<&ChildOf>,
    mut ai_damage_events: MessageWriter<crate::events::AiShipDamaged>,
    _notifications: MessageWriter<ShowNotification>,
) {
    'projectiles: for (proj_entity, proj, proj_transform, _proj_vel) in proj_query.iter() {
        let proj_pos = proj_transform.translation.truncate();
        // A weapon's own ship is never a valid target for its own shot,
        // regardless of aim — belt-and-suspenders on top of firing-arc and
        // per-ship query scoping.
        let owner_ship = owner_parent_query.get(proj.owner).ok().map(|p| p.parent());

        // === AI SHIPS: shield first, then block-by-block hull damage ===
        for (ai_entity, ai_transform, children, mut shield) in ai_ship_query.iter_mut() {
            if Some(ai_entity) == owner_ship { continue; }
            // Bubble is centered on the blocks' centroid, not the root
            let center = shield.world_center(ai_transform);
            let dist_to_ship = proj_pos.distance(center);

            // Shield bubble intercepts anything crossing its radius
            if shield.is_up() && dist_to_ship < shield.radius {
                shield.absorb(proj.damage);
                spawn_hit_effect(&mut commands, proj_pos, Color::srgb(0.5, 0.8, 1.0), 14.0);
                commands.entity(proj_entity).despawn();
                continue 'projectiles;
            }

            // Shield down: hit the nearest block within impact range — hull
            // segment or module, whichever is actually closer to the impact.
            // This is the single authoritative hit resolution for AI ships;
            // process_ai_ship_damage_system used to redo its own separate
            // "nearest hull segment on the whole ship, no radius limit"
            // search off the same event, which routinely landed on some
            // unrelated block far from the actual shot — the block you aimed
            // at wasn't the one that took damage. It now only recalculates
            // aggregate integrity from whatever this system already did.
            // Scan bound follows the ship's real extent (shield radius is
            // computed from it) — a fixed bound left long hulls unhittable.
            if dist_to_ship < shield.radius + 60.0 {
                let mut best_module: Option<(Entity, f32)> = None;
                for child in children.iter() {
                    if let Ok((_, gt)) = ai_module_query.get(child) {
                        let d = proj_pos.distance(gt.translation().truncate());
                        if d < 45.0 && best_module.map(|(_, bd)| d < bd).unwrap_or(true) {
                            best_module = Some((child, d));
                        }
                    }
                }
                let mut best_hull: Option<(Entity, f32)> = None;
                for child in children.iter() {
                    if let Ok((_, gt)) = ai_hull_query.get(child) {
                        let d = proj_pos.distance(gt.translation().truncate());
                        if d < 45.0 && best_hull.map(|(_, bd)| d < bd).unwrap_or(true) {
                            best_hull = Some((child, d));
                        }
                    }
                }

                let hit_module = matches!((best_module, best_hull), (Some((_, md)), Some((_, hd))) if md <= hd)
                    || (best_module.is_some() && best_hull.is_none());

                if hit_module {
                    if let Ok((mut module, gt)) = ai_module_query.get_mut(best_module.unwrap().0) {
                        module.health = (module.health - proj.damage).max(0.0);
                        let hit_pos = gt.translation().truncate();
                        spawn_hit_effect(&mut commands, hit_pos, Color::srgb(1.0, 0.6, 0.2), 12.0);
                        spawn_floating_damage(&mut commands, hit_pos, proj.damage, Color::srgb(1.0, 0.8, 0.3));
                        ai_damage_events.write(crate::events::AiShipDamaged {
                            target: ai_entity,
                            source: crate::events::DamageSource::Explosion,
                            amount: 0.0, // damage already applied directly above — this is bookkeeping only
                            position: Some(hit_pos),
                            direction: None,
                        });
                        commands.entity(proj_entity).despawn();
                        continue 'projectiles;
                    }
                } else if let Some((hull_entity, _)) = best_hull {
                    if let Ok((mut hull, gt)) = ai_hull_query.get_mut(hull_entity) {
                        hull.health = (hull.health - proj.damage).max(0.0);
                        let hit_pos = gt.translation().truncate();
                        spawn_hit_effect(&mut commands, hit_pos, Color::srgb(1.0, 0.5, 0.2), 16.0);
                        spawn_floating_damage(&mut commands, hit_pos, proj.damage, Color::srgb(1.0, 0.3, 0.3));
                        ai_damage_events.write(crate::events::AiShipDamaged {
                            target: ai_entity,
                            source: crate::events::DamageSource::Explosion,
                            amount: 0.0,
                            position: Some(hit_pos),
                            direction: None,
                        });
                        commands.entity(proj_entity).despawn();
                        continue 'projectiles;
                    }
                }
            }
        }

        for (creature_entity, _) in creature_grid.0.nearby(proj_pos, MAX_CREATURE_HIT_RADIUS) {
            let Ok((creature_transform, mut creature)) = creature_query.get_mut(creature_entity) else { continue };
            if creature.health <= 0.0 { continue; }

            let creature_pos = creature_transform.translation.truncate();
            let hit_radius = match creature.creature_type {
                CreatureType::Leviathan => 90.0,
                CreatureType::Stalker => 30.0,
                CreatureType::ParasiteSwarm => 15.0,
                CreatureType::VoidDrifter => 12.0,
            };

            let dist = proj_pos.distance(creature_pos);
            if dist > hit_radius { continue; }

            // HIT!
            creature.health -= proj.damage;

            // Impact spark
            spawn_hit_effect(&mut commands, proj_pos, Color::srgb(1.0, 0.8, 0.3), 8.0);
            spawn_floating_damage(&mut commands, proj_pos, proj.damage, Color::srgb(1.0, 0.4, 0.2));

            // Despawn projectile (unless it penetrates)
            if proj.penetration < 30.0 || proj.has_penetrated {
                commands.entity(proj_entity).despawn();
            }
            // High penetration projectiles continue through

            break; // One hit per frame per projectile
        }
    }
}
