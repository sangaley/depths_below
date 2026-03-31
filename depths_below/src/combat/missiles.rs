use bevy::prelude::*;
use crate::components::*;
use crate::events::*;
use crate::celestial::components::{GravityAffected, GravityForce};
use super::targeting::{TargetSelection, FireGroupState, FireGroup};
use super::new_projectiles::MissileProjectile;
use super::*;

// ============================================================================
// MISSILE SYSTEM
// Guided missiles with burn phase + reserve fuel for course corrections.
// Bay chain length determines missile size. Decoyable. Shootable.
// ============================================================================

/// Fire missile weapons when their fire group is active
pub fn fire_missiles_system(
    time: Res<Time>,
    fire_state: Res<FireGroupState>,
    selection: Res<TargetSelection>,
    mut weapon_query: Query<(
        Entity, &Module, &mut Weapon, &mut WeaponCooldown,
        &GlobalTransform, &FireGroup,
    ), Without<DestroyedModule>>,
    target_query: Query<&Transform, Without<Submarine>>,
    machine_stats: Query<&crate::building::multiblock::components::MachineStats>,
    mut fuel_state: ResMut<crate::resources::FuelState>,
    mut commands: Commands,
    mut notifications: EventWriter<ShowNotification>,
) {
    for (entity, module, mut weapon, mut cooldown, global_transform, fire_group) in weapon_query.iter_mut() {
        // Only missile-type weapons
        if !matches!(module.module_type,
            ModuleType::HeavyMissile | ModuleType::GuidedMissile | ModuleType::ClusterRocket
        ) { continue; }

        if !module.is_active { continue; }
        cooldown.timer.tick(time.delta());
        if !cooldown.timer.finished() { continue; }

        let group_firing = fire_state.firing[fire_group.group as usize % 4];
        if !group_firing { continue; }

        let Some(target_entity) = selection.target else { continue; };
        let Ok(target_transform) = target_query.get(target_entity) else { continue; };

        if weapon.ammo <= 0 { continue; }

        let weapon_pos = global_transform.translation().truncate();
        let target_pos = target_transform.translation.truncate();

        // Determine missile properties based on module type and bay chain length
        let bay_count = machine_stats.get(entity)
            .map(|s| s.barrel_count.max(1)) // Barrel count = bay chain length for missiles
            .unwrap_or(1);

        let size_mult = bay_count as f32;

        // Base missile stats scaled by bay count
        let missile_damage = weapon.damage * size_mult;
        let missile_thrust = 400.0 / size_mult.sqrt(); // Bigger = slower acceleration
        let tracking = match module.module_type {
            ModuleType::GuidedMissile => 2.0,     // Good tracking
            ModuleType::HeavyMissile => 1.2,      // Sluggish tracking
            ModuleType::ClusterRocket => 0.0,      // No tracking — dumb fire
            _ => 1.0,
        };
        let blast_radius = 30.0 + size_mult * 20.0;

        // Fuel from ship — bigger missile needs more fuel
        let fuel_cost = 5.0 * size_mult;
        if fuel_state.current_fuel < fuel_cost {
            notifications.send(ShowNotification {
                message: "No fuel for missile launch!".into(),
                notification_type: NotificationType::Warning,
                duration: 2.0,
            });
            continue;
        }
        fuel_state.current_fuel -= fuel_cost;

        // Fuel split: 70% burn, 30% reserve (default — customizable via Tier 3)
        let total_fuel = fuel_cost;
        let burn_fuel = total_fuel * 0.7;
        let reserve_fuel = total_fuel * 0.3;

        // Fire!
        cooldown.timer.reset();
        weapon.ammo = weapon.ammo.saturating_sub(1);

        let direction = (target_pos - weapon_pos).normalize_or_zero();
        let initial_vel = direction * 100.0; // Slow launch

        // Visual size scales with bay count
        let visual_w = 12.0 + size_mult * 4.0;
        let visual_h = 5.0 + size_mult * 2.0;
        let angle = direction.y.atan2(direction.x);

        let volley_count = match module.module_type {
            ModuleType::ClusterRocket => (3.0 * size_mult).min(8.0) as u32, // More bays = more rockets
            _ => 1,
        };

        for i in 0..volley_count {
            let spread = if volley_count > 1 {
                let spread_angle = (i as f32 - volley_count as f32 / 2.0) * 0.15;
                Vec2::new(spread_angle.cos(), spread_angle.sin()) * 20.0
            } else {
                Vec2::ZERO
            };

            let missile_vel = initial_vel + spread;
            let per_missile_damage = if volley_count > 1 {
                missile_damage / volley_count as f32
            } else {
                missile_damage
            };

            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color: Color::rgb(0.8, 0.3, 0.2),
                        custom_size: Some(Vec2::new(visual_w, visual_h)),
                        ..default()
                    },
                    transform: Transform {
                        translation: Vec3::new(weapon_pos.x, weapon_pos.y, 0.5),
                        rotation: Quat::from_rotation_z(angle),
                        ..default()
                    },
                    ..default()
                },
                MissileProjectile {
                    damage: per_missile_damage,
                    target: if tracking > 0.0 { Some(target_entity) } else { None },
                    burn_fuel,
                    reserve_fuel,
                    thrust: missile_thrust,
                    tracking_agility: tracking,
                    armed: false,
                    arm_distance: 80.0,
                    traveled: 0.0,
                    blast_radius,
                    owner: entity,
                },
                Velocity(missile_vel),
                GravityAffected { mass: 2.0 + size_mult },
                GravityForce::default(),
            ));
        }

        // Launch notification for heavy missiles
        if module.module_type == ModuleType::HeavyMissile {
            notifications.send(ShowNotification {
                message: "Heavy missile launched!".into(),
                notification_type: NotificationType::Warning,
                duration: 1.5,
            });
        }
    }
}

/// Move and guide missiles — burn phase then coast/correct phase
pub fn move_missiles(
    time: Res<Time>,
    mut commands: Commands,
    mut missile_query: Query<(Entity, &mut MissileProjectile, &mut Transform, &mut Velocity, &GravityForce)>,
    target_query: Query<&Transform, Without<MissileProjectile>>,
) {
    let dt = time.delta_seconds();

    for (entity, mut missile, mut transform, mut velocity, gravity) in missile_query.iter_mut() {
        let pos = transform.translation.truncate();

        // Track distance traveled
        let move_dist = velocity.0.length() * dt;
        missile.traveled += move_dist;

        // Arm after minimum distance
        if !missile.armed && missile.traveled > missile.arm_distance {
            missile.armed = true;
        }

        // === BURN PHASE: engine is on, accelerating ===
        if missile.burn_fuel > 0.0 {
            let burn_cost = missile.thrust * 0.01 * dt;
            missile.burn_fuel -= burn_cost;

            let forward = velocity.0.normalize_or_zero();
            velocity.0 += forward * missile.thrust * dt;
        }

        // === GUIDANCE: use reserve fuel to correct course ===
        if missile.reserve_fuel > 0.0 && missile.tracking_agility > 0.0 {
            if let Some(target) = missile.target {
                if let Ok(target_transform) = target_query.get(target) {
                    let target_pos = target_transform.translation.truncate();
                    let to_target = (target_pos - pos).normalize_or_zero();
                    let current_dir = velocity.0.normalize_or_zero();

                    // Rotate toward target
                    let cross = current_dir.x * to_target.y - current_dir.y * to_target.x;
                    let turn = cross.clamp(-missile.tracking_agility, missile.tracking_agility) * dt;

                    let speed = velocity.0.length();
                    let new_angle = current_dir.y.atan2(current_dir.x) + turn;
                    velocity.0 = Vec2::new(new_angle.cos(), new_angle.sin()) * speed;

                    // Consume reserve fuel for course corrections
                    missile.reserve_fuel -= turn.abs() * 0.5;
                }
            }
        }

        // Apply gravity
        velocity.0 += gravity.0 * dt;

        // Move
        transform.translation.x += velocity.0.x * dt;
        transform.translation.y += velocity.0.y * dt;

        // Rotate sprite to face movement
        if velocity.0.length_squared() > 1.0 {
            let angle = velocity.0.y.atan2(velocity.0.x);
            transform.rotation = Quat::from_rotation_z(angle);
        }

        // Despawn if out of fuel and far from any target (lost missile)
        if missile.burn_fuel <= 0.0 && missile.reserve_fuel <= 0.0 {
            // Coast for 3 more seconds then despawn
            missile.burn_fuel -= dt; // Hack: use negative burn_fuel as coast timer
            if missile.burn_fuel < -3.0 {
                commands.entity(entity).despawn();
            }
        }
    }
}

/// Check missile hits — armed missiles explode on contact
pub fn check_missile_hits(
    mut commands: Commands,
    missile_query: Query<(Entity, &MissileProjectile, &Transform)>,
    mut creature_query: Query<(Entity, &Transform, &mut Creature), Without<Submarine>>,
    mut notifications: EventWriter<ShowNotification>,
) {
    for (missile_entity, missile, missile_transform) in missile_query.iter() {
        if !missile.armed { continue; }

        let missile_pos = missile_transform.translation.truncate();

        for (creature_entity, creature_transform, mut creature) in creature_query.iter_mut() {
            if creature.health <= 0.0 { continue; }

            let creature_pos = creature_transform.translation.truncate();
            let dist = missile_pos.distance(creature_pos);
            let hit_radius = match creature.creature_type {
                CreatureType::Leviathan => 100.0,
                _ => 40.0,
            };

            if dist > hit_radius { continue; }

            // IMPACT!
            // Direct hit damage
            creature.health -= missile.damage;

            // Blast radius damage to nearby creatures
            // (handled by the explosion effect — could expand later)

            // Explosion visual
            spawn_hit_effect(&mut commands, missile_pos, Color::rgb(1.0, 0.5, 0.1), missile.blast_radius);
            spawn_floating_damage(&mut commands, missile_pos, missile.damage, Color::rgb(1.0, 0.3, 0.1));

            commands.entity(missile_entity).despawn();
            break;
        }
    }
}
