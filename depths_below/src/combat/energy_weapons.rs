use bevy::prelude::*;
use crate::components::*;
use crate::events::*;
use crate::resources::*;
use crate::celestial::components::{GravityAffected, GravityForce};
use super::targeting::{TargetSelection, FireGroupState, FireGroup};
use super::combat_features::WeakPoint;
use super::*;

// ============================================================================
// ENERGY WEAPONS — Laser, Ion Disruptor, EMP Missile
// ============================================================================

// ============================================================================
// LASER BEAM — instant hit, continuous, cuts through targets
// ============================================================================

/// Component marking an active laser beam
#[derive(Component)]
pub struct LaserBeam {
    pub origin: Vec2,
    pub direction: Vec2,
    pub damage_per_second: f32,
    pub max_range: f32,
    pub power_drain_per_second: f32,
    pub heat_per_second: f32,
}

/// Visual line for the laser beam
#[derive(Component)]
pub struct LaserBeamVisual;

/// System: fire laser weapons — continuous beam while fire group held
pub fn fire_laser_system(
    time: Res<Time>,
    fire_state: Res<FireGroupState>,
    selection: Res<TargetSelection>,
    sub_query: Query<&Transform, With<Submarine>>,
    weapon_query: Query<(
        &Module, &Weapon, &FireGroup, &GlobalTransform,
        Option<&ModuleTemperature>,
    ), Without<DestroyedModule>>,
    target_query: Query<&Transform, Without<Submarine>>,
    mut power_state: ResMut<PowerState>,
    mut creature_query: Query<(&Transform, &mut Creature, Option<&Velocity>, Option<&WeakPoint>), Without<Submarine>>,
    mut commands: Commands,
    existing_beams: Query<Entity, With<LaserBeamVisual>>,
    mut notifications: EventWriter<ShowNotification>,
) {
    let dt = time.delta_seconds();

    // Despawn old beam visuals
    for entity in existing_beams.iter() {
        commands.entity(entity).despawn();
    }

    let Ok(sub_transform) = sub_query.get_single() else { return };

    for (module, weapon, fire_group, global_transform, temp) in weapon_query.iter() {
        if module.module_type != ModuleType::Laser { continue; }
        if !module.is_active { continue; }

        // Check fire group
        let group_firing = fire_state.firing[fire_group.group as usize % 4];
        if !group_firing { continue; }

        // Check if overheated
        if let Some(temp) = temp {
            if temp.current >= temp.max_temp * 0.95 { continue; } // Overheated
        }

        // Check power
        let power_cost = 15.0 * dt; // Power drain per second
        if power_state.power_balance < -power_cost {
            continue; // Not enough power
        }

        let weapon_pos = global_transform.translation().truncate();
        let beam_range = weapon.range;

        // Aim at selected target or straight ahead
        let beam_dir = if let Some(target) = selection.target {
            if let Ok(target_transform) = target_query.get(target) {
                (target_transform.translation.truncate() - weapon_pos).normalize_or_zero()
            } else {
                Vec2::X // Default forward
            }
        } else {
            Vec2::X
        };

        // Trace beam — check all creatures along the line
        let beam_end = weapon_pos + beam_dir * beam_range;
        let mut total_damage_dealt = 0.0;
        let mut hit_count = 0u32;
        let mut blocked = false;

        for (creature_transform, mut creature, velocity, weak_point) in creature_query.iter_mut() {
            if creature.health <= 0.0 { continue; }
            if blocked { break; }

            let creature_pos = creature_transform.translation.truncate();

            // Point-to-line distance check
            let to_creature = creature_pos - weapon_pos;
            let projection = to_creature.dot(beam_dir);

            if projection < 0.0 || projection > beam_range { continue; }

            let closest_point = weapon_pos + beam_dir * projection;
            let perpendicular_dist = creature_pos.distance(closest_point);

            let hit_radius = match creature.creature_type {
                CreatureType::Leviathan => 80.0,
                CreatureType::Stalker => 25.0,
                CreatureType::ParasiteSwarm => 12.0,
                CreatureType::VoidDrifter => 10.0,
            };

            if perpendicular_dist > hit_radius { continue; }

            // HIT — apply continuous damage
            let mut damage = weapon.damage * dt;

            // Weak point check
            if let Some(wp) = weak_point {
                let vel = velocity.map(|v| v.0).unwrap_or(Vec2::ZERO);
                let mult = super::combat_features::check_weak_point_hit(
                    closest_point, creature_pos, vel, wp,
                );
                damage *= mult;
            }

            creature.health -= damage;
            total_damage_dealt += damage;
            hit_count += 1;

            // Beam continues through small targets, blocked by large ones
            match creature.creature_type {
                CreatureType::Leviathan => { blocked = true; }
                _ => {} // Beam cuts through
            }
        }

        // Draw beam visual
        let actual_end = if blocked {
            // Shorten beam to where it was blocked
            weapon_pos + beam_dir * (beam_range * 0.7)
        } else {
            beam_end
        };

        let midpoint = (weapon_pos + actual_end) / 2.0;
        let length = weapon_pos.distance(actual_end);
        let angle = beam_dir.y.atan2(beam_dir.x);

        // Core beam (bright)
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgba(0.3, 0.9, 0.4, 0.8),
                    custom_size: Some(Vec2::new(length, 3.0)),
                    ..default()
                },
                transform: Transform {
                    translation: Vec3::new(midpoint.x, midpoint.y, 0.6),
                    rotation: Quat::from_rotation_z(angle),
                    ..default()
                },
                ..default()
            },
            LaserBeamVisual,
        ));

        // Glow around beam
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgba(0.2, 0.7, 0.3, 0.2),
                    custom_size: Some(Vec2::new(length, 10.0)),
                    ..default()
                },
                transform: Transform {
                    translation: Vec3::new(midpoint.x, midpoint.y, 0.55),
                    rotation: Quat::from_rotation_z(angle),
                    ..default()
                },
                ..default()
            },
            LaserBeamVisual,
        ));

        // Impact flash if hitting something
        if hit_count > 0 {
            spawn_hit_effect(&mut commands, actual_end, Color::rgb(0.4, 1.0, 0.5), 8.0);
        }
    }
}

// ============================================================================
// ION DISRUPTOR — slow projectile, loses energy over distance, disables
// ============================================================================

/// Ion pulse projectile
#[derive(Component)]
pub struct IonPulse {
    pub initial_energy: f32,
    pub current_energy: f32,
    pub decay_rate: f32,       // Energy lost per second
    pub disable_duration: f32, // How long targets stay disabled
    pub owner: Entity,
}

/// Marks a module as temporarily disabled by ion
#[derive(Component)]
pub struct IonDisabled {
    pub timer: f32,
}

/// System: fire ion disruptor — slow pulse projectile
pub fn fire_ion_system(
    time: Res<Time>,
    fire_state: Res<FireGroupState>,
    selection: Res<TargetSelection>,
    mut weapon_query: Query<(
        &Module, &mut Weapon, &mut WeaponCooldown,
        &GlobalTransform, &FireGroup,
    ), Without<DestroyedModule>>,
    target_query: Query<&Transform, Without<Submarine>>,
    mut commands: Commands,
) {
    for (module, mut weapon, mut cooldown, global_transform, fire_group) in weapon_query.iter_mut() {
        if module.module_type != ModuleType::IonDisruptor { continue; }
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
        let direction = (target_pos - weapon_pos).normalize_or_zero();

        cooldown.timer.reset();
        weapon.ammo = weapon.ammo.saturating_sub(1);

        let speed = 250.0; // Slow
        let angle = direction.y.atan2(direction.x);

        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgba(0.5, 0.3, 0.9, 0.8),
                    custom_size: Some(Vec2::splat(14.0)),
                    ..default()
                },
                transform: Transform {
                    translation: Vec3::new(weapon_pos.x, weapon_pos.y, 0.5),
                    rotation: Quat::from_rotation_z(angle),
                    ..default()
                },
                ..default()
            },
            IonPulse {
                initial_energy: 100.0,
                current_energy: 100.0,
                decay_rate: 15.0, // Loses 15 energy per second in flight
                disable_duration: 5.0,
                owner: Entity::PLACEHOLDER,
            },
            Velocity(direction * speed),
            GravityAffected { mass: 0.2 },
            GravityForce::default(),
        ));

        // Muzzle flash
        spawn_hit_effect(&mut commands, weapon_pos + direction * 20.0, Color::rgb(0.5, 0.3, 0.9), 10.0);
    }
}

/// System: move ion pulses, decay energy, check hits
pub fn update_ion_pulses(
    time: Res<Time>,
    mut commands: Commands,
    mut pulse_query: Query<(Entity, &mut IonPulse, &mut Transform, &mut Velocity, &mut Sprite, &GravityForce)>,
    mut creature_query: Query<(Entity, &Transform, &mut Creature), Without<IonPulse>>,
    mut notifications: EventWriter<ShowNotification>,
) {
    let dt = time.delta_seconds();

    for (entity, mut pulse, mut transform, mut velocity, mut sprite, gravity) in pulse_query.iter_mut() {
        // Decay energy over time
        pulse.current_energy -= pulse.decay_rate * dt;

        // Visual: pulse dims as energy drops
        let energy_ratio = (pulse.current_energy / pulse.initial_energy).clamp(0.0, 1.0);
        sprite.color.set_a(energy_ratio * 0.8);

        // Shrink as energy drops
        let size = 14.0 * energy_ratio.max(0.3);
        sprite.custom_size = Some(Vec2::splat(size));

        // Dead pulse
        if pulse.current_energy <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        // Apply gravity
        velocity.0 += gravity.0 * dt;

        // Move
        transform.translation.x += velocity.0.x * dt;
        transform.translation.y += velocity.0.y * dt;

        // Check hits
        let pulse_pos = transform.translation.truncate();

        for (creature_entity, creature_transform, mut creature) in creature_query.iter_mut() {
            if creature.health <= 0.0 { continue; }

            let dist = pulse_pos.distance(creature_transform.translation.truncate());
            if dist > 30.0 { continue; }

            // HIT — disable based on remaining energy
            let disable_strength = energy_ratio; // Full energy = full disable, half energy = half duration
            let disable_time = pulse.disable_duration * disable_strength;

            // Small damage
            creature.health -= 5.0 * energy_ratio;

            // Stun: reduce creature speed temporarily (via notification for now)
            notifications.send(ShowNotification {
                message: format!("Ion hit! Target disrupted for {:.1}s", disable_time),
                notification_type: NotificationType::Info,
                duration: 2.0,
            });

            // Visual
            spawn_hit_effect(&mut commands, pulse_pos, Color::rgb(0.6, 0.4, 1.0), 16.0);
            spawn_floating_damage(&mut commands, pulse_pos, 5.0 * energy_ratio, Color::rgb(0.5, 0.3, 0.9));

            commands.entity(entity).despawn();
            break;
        }
    }
}

/// System: tick ion disabled timers, re-enable modules
pub fn update_ion_disabled(
    mut commands: Commands,
    time: Res<Time>,
    mut disabled_query: Query<(Entity, &mut IonDisabled, &mut Module)>,
) {
    let dt = time.delta_seconds();

    for (entity, mut disabled, mut module) in disabled_query.iter_mut() {
        disabled.timer -= dt;
        module.is_active = false; // Keep disabled

        if disabled.timer <= 0.0 {
            module.is_active = true; // Re-enable
            commands.entity(entity).remove::<IonDisabled>();
        }
    }
}

// ============================================================================
// EMP MISSILE — disables everything in blast radius including friendly
// ============================================================================

/// EMP missile warhead data
#[derive(Component)]
pub struct EmpWarhead {
    pub emp_radius: f32,
    pub disable_duration: f32,
    pub affects_friendly: bool,
}

/// System: fire EMP missiles (uses missile bay system)
pub fn fire_emp_missiles(
    time: Res<Time>,
    fire_state: Res<FireGroupState>,
    selection: Res<TargetSelection>,
    mut weapon_query: Query<(
        Entity, &Module, &mut Weapon, &mut WeaponCooldown,
        &GlobalTransform, &FireGroup,
    ), Without<DestroyedModule>>,
    target_query: Query<&Transform, Without<Submarine>>,
    machine_stats: Query<&crate::building::multiblock::components::MachineStats>,
    mut fuel_state: ResMut<FuelState>,
    mut commands: Commands,
) {
    for (entity, module, mut weapon, mut cooldown, global_transform, fire_group) in weapon_query.iter_mut() {
        if module.module_type != ModuleType::EMPPulse { continue; }
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
        let direction = (target_pos - weapon_pos).normalize_or_zero();

        // Bay chain length determines EMP radius
        let bay_count = machine_stats.get(entity)
            .map(|s| s.barrel_count.max(1))
            .unwrap_or(1) as f32;

        let emp_radius = 100.0 + bay_count * 60.0;

        // Fuel cost
        let fuel_cost = 8.0 * bay_count;
        if fuel_state.current_fuel < fuel_cost { continue; }
        fuel_state.current_fuel -= fuel_cost;

        cooldown.timer.reset();
        weapon.ammo = weapon.ammo.saturating_sub(1);

        let speed = 300.0;
        let angle = direction.y.atan2(direction.x);

        // Spawn EMP missile
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgb(0.4, 0.3, 0.8),
                    custom_size: Some(Vec2::new(14.0, 8.0)),
                    ..default()
                },
                transform: Transform {
                    translation: Vec3::new(weapon_pos.x, weapon_pos.y, 0.5),
                    rotation: Quat::from_rotation_z(angle),
                    ..default()
                },
                ..default()
            },
            super::new_projectiles::MissileProjectile {
                damage: 5.0, // Low physical damage
                target: Some(target_entity),
                burn_fuel: fuel_cost * 0.7,
                reserve_fuel: fuel_cost * 0.3,
                thrust: 350.0,
                tracking_agility: 1.5,
                armed: false,
                arm_distance: 100.0,
                traveled: 0.0,
                blast_radius: emp_radius,
                owner: entity,
            },
            EmpWarhead {
                emp_radius,
                disable_duration: 6.0,
                affects_friendly: true,
            },
            Velocity(direction * speed),
            GravityAffected { mass: 3.0 },
            GravityForce::default(),
        ));
    }
}

/// System: when EMP missile hits, disable everything in radius
pub fn emp_detonation(
    mut commands: Commands,
    missile_query: Query<(Entity, &Transform, &EmpWarhead, &super::new_projectiles::MissileProjectile)>,
    mut module_query: Query<(Entity, &Module, &GlobalTransform), Without<DestroyedModule>>,
    creature_query: Query<(Entity, &Transform, &Creature)>,
    sub_query: Query<&Transform, With<Submarine>>,
    mut notifications: EventWriter<ShowNotification>,
) {
    // Check if any EMP missiles have hit their target (handled by missile collision system)
    // This system handles the EMP effect AFTER detonation
    // For now, detect armed EMP missiles near targets and detonate

    let Ok(sub_transform) = sub_query.get_single() else { return };
    let sub_pos = sub_transform.translation.truncate();

    for (missile_entity, missile_transform, emp, missile) in missile_query.iter() {
        if !missile.armed { continue; }

        let missile_pos = missile_transform.translation.truncate();

        // Check if near any creature
        let mut detonated = false;
        for (_creature_entity, creature_transform, creature) in creature_query.iter() {
            if creature.health <= 0.0 { continue; }
            let dist = missile_pos.distance(creature_transform.translation.truncate());
            if dist < 40.0 {
                detonated = true;
                break;
            }
        }

        if !detonated { continue; }

        // EMP DETONATION!
        // Visual: purple-blue expanding ring
        spawn_hit_effect(&mut commands, missile_pos, Color::rgba(0.4, 0.3, 0.9, 0.6), emp.emp_radius);

        // Check if player ship is in blast radius
        let dist_to_player = missile_pos.distance(sub_pos);
        if dist_to_player < emp.emp_radius && emp.affects_friendly {
            notifications.send(ShowNotification {
                message: "EMP BLAST! Your systems are disrupted!".into(),
                notification_type: NotificationType::Danger,
                duration: 3.0,
            });

            // Disable player modules in radius
            for (module_entity, module, module_gt) in module_query.iter_mut() {
                let module_pos = module_gt.translation().truncate();
                if module_pos.distance(missile_pos) < emp.emp_radius {
                    commands.entity(module_entity).insert(IonDisabled {
                        timer: emp.disable_duration * 0.5, // Half duration for friendly
                    });
                }
            }
        }

        notifications.send(ShowNotification {
            message: format!("EMP detonated! {:.0}m radius disruption!", emp.emp_radius),
            notification_type: NotificationType::Warning,
            duration: 3.0,
        });

        commands.entity(missile_entity).despawn();
    }
}
