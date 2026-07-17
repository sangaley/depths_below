use bevy::prelude::*;
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
    power_state: Res<crate::resources::PowerState>,
    selection: Res<TargetSelection>,
    ship_query: Query<(Entity, &ShipPhysics, &Transform), With<Ship>>,
    mut weapon_query: Query<(
        Entity, &Module, &mut Weapon, &mut WeaponCooldown,
        &GlobalTransform, &FireGroup, &WeaponMount, &ChildOf,
        Option<&crate::building::customization::tuning::WeaponTuning>,
        Option<&ModuleTemperature>,
    ), Without<DestroyedModule>>,
    target_query: Query<&Transform, Without<Ship>>,
    machine_stats: Query<&crate::building::multiblock::components::MachineStats>,
    mut fuel_state: ResMut<crate::resources::FuelState>,
    windows_query: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    input_state: Res<crate::resources::InputState>,
    mut commands: Commands,
    mut notifications: MessageWriter<ShowNotification>,
    mut fired_events: MessageWriter<crate::events::WeaponFired>,
) {
    let Ok((player_ship, ship_physics, ship_transform)) = ship_query.single() else { return };

    // Weapons need power — grid deficit silences launchers too.
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
    // Controller right-stick aim beats the mouse while it owns aim (see
    // InputState.gamepad_aim).
    let cursor_world = input_state.gamepad_aim
        .map(|dir| ship_transform.translation.truncate() + dir * 2000.0)
        .or(cursor_world);

    for (entity, module, mut weapon, mut cooldown, global_transform, fire_group, mount, parent, tuning, temp) in weapon_query.iter_mut() {
        // Player ship only — see fire_weapons_system for why this matters:
        // AI ships carry identical missile-bay components and would
        // otherwise launch whenever the player fires, homing on the
        // player's own target selection.
        if parent.parent() != player_ship { continue; }
        // Only missile-type weapons
        if !matches!(module.module_type,
            ModuleType::HeavyMissile | ModuleType::GuidedMissile | ModuleType::ClusterRocket
        ) { continue; }

        if !module.is_active { continue; }

        // Tick before the thermal gate — see fire_weapons_system: gating
        // first freezes the timer, which generate_heat reads as "recently
        // fired", locking the launcher hot forever.
        cooldown.timer.tick(time.delta());
        if !cooldown.timer.is_finished() { continue; }

        // Thermal throttle — same gate the laser/kinetics use.
        if let Some(temp) = temp {
            if temp.current >= temp.max_temp * 0.95 { continue; }
        }

        let group_firing = fire_state.firing[fire_group.group as usize % 4];
        if !group_firing { continue; }

        if !crate::combat::INFINITE_AMMO && weapon.ammo <= 0 { continue; }

        let weapon_pos = global_transform.translation().truncate();

        // Selected target = homing; no target = dumb-fire toward the cursor.
        let (target_pos, homing_target) = if let Some(target_entity) = selection.target {
            let Ok(target_transform) = target_query.get(target_entity) else { continue };
            (target_transform.translation.truncate(), Some(target_entity))
        } else if let Some(cursor) = cursor_world {
            (cursor, None)
        } else {
            continue;
        };

        // Fixed-mount launchers can't swivel outside their arc, but they
        // never silently refuse to fire (players read that as "rockets are
        // broken" — aiming at a far cursor put every pod off-cone). Launch
        // direction is clamped to the arc edge instead: at worst the salvo
        // flies visibly off-aim, still never backwards through the hull.
        let aim_dir = (target_pos - weapon_pos).normalize_or_zero();
        let launch_dir = clamp_to_firing_arc(ship_physics.rotation, &module.rotation, mount, aim_dir);

        // Determine missile properties based on module type and bay chain length
        let bay_count = machine_stats.get(entity)
            .map(|s| s.barrel_count.max(1)) // Barrel count = bay chain length for missiles
            .unwrap_or(1);

        let size_mult = bay_count as f32;

        // Base missile stats scaled by bay count. The tuning velocity slider
        // is "thrust" for missiles — hotter engines, faster closing speed.
        let missile_damage = weapon.damage * size_mult;
        let thrust_mult = tuning.map(|t| t.velocity).unwrap_or(1.0);
        let missile_thrust = 400.0 * thrust_mult / size_mult.sqrt(); // Bigger = slower acceleration
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
            notifications.write(ShowNotification {
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
        if !crate::combat::INFINITE_AMMO {
            weapon.ammo = weapon.ammo.saturating_sub(1);
        }
        fired_events.write(crate::events::WeaponFired {
            weapon_type: module.module_type,
            position: weapon_pos,
            from_player: true,
        });

        let direction = launch_dir;
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
                (Sprite {
                        color: Color::srgb(0.8, 0.3, 0.2),
                        custom_size: Some(Vec2::new(visual_w, visual_h)),
                        ..default()
                    }, Transform {
                        translation: Vec3::new(weapon_pos.x, weapon_pos.y, 0.5),
                        rotation: Quat::from_rotation_z(angle),
                        ..default()
                    }),
                MissileProjectile {
                    damage: per_missile_damage,
                    target: if tracking > 0.0 { homing_target } else { None },
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
            notifications.write(ShowNotification {
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
    let dt = time.delta_secs();

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

/// Largest creature hit radius (Leviathan) — used to size the spatial grid query margin.
const MAX_CREATURE_HIT_RADIUS: f32 = 100.0;

/// Check missile hits — armed missiles explode on contact.
/// Uses the creature spatial grid to only distance-check nearby creatures.
pub fn check_missile_hits(
    mut commands: Commands,
    missile_query: Query<(Entity, &MissileProjectile, &Transform)>,
    mut creature_query: Query<(&Transform, &mut Creature), Without<Ship>>,
    creature_grid: Res<crate::spatial::CreatureGrid>,
    mut ai_ship_query: Query<
        (Entity, &Transform, &Children, &mut crate::combat::shields::ShipShield),
        With<crate::ai_ship::components::AiShip>,
    >,
    mut ai_module_query: Query<(&mut Module, &GlobalTransform), Without<DestroyedModule>>,
    owner_parent_query: Query<&ChildOf>,
    mut ai_damage_events: MessageWriter<crate::events::AiShipDamaged>,
    _notifications: MessageWriter<ShowNotification>,
) {
    'missiles: for (missile_entity, missile, missile_transform) in missile_query.iter() {
        if !missile.armed { continue; }

        let missile_pos = missile_transform.translation.truncate();
        // A weapon's own ship is never a valid target for its own missile.
        let owner_ship = owner_parent_query.get(missile.owner).ok().map(|p| p.parent());

        // === AI SHIPS: shield absorbs the warhead; shield down = blast
        // damage to every block inside the blast radius ===
        for (ai_entity, ai_transform, children, mut shield) in ai_ship_query.iter_mut() {
            if Some(ai_entity) == owner_ship { continue; }
            let center = shield.world_center(ai_transform);
            let dist_to_ship = missile_pos.distance(center);

            if shield.is_up() && dist_to_ship < shield.radius {
                shield.absorb(missile.damage);
                spawn_hit_effect(&mut commands, missile_pos, Color::srgb(0.5, 0.8, 1.0), missile.blast_radius);
                commands.entity(missile_entity).despawn();
                continue 'missiles;
            }

            if dist_to_ship < shield.radius + 60.0 {
                // Detonate if any block is inside the blast radius
                let mut total_damage = 0.0;
                let mut hit_any = false;
                for child in children.iter() {
                    if let Ok((mut module, gt)) = ai_module_query.get_mut(child) {
                        let d = missile_pos.distance(gt.translation().truncate());
                        if d < missile.blast_radius.max(50.0) {
                            module.health = (module.health - missile.damage).max(0.0);
                            total_damage += missile.damage;
                            hit_any = true;
                        }
                    }
                }
                if hit_any {
                    spawn_hit_effect(&mut commands, missile_pos, Color::srgb(1.0, 0.5, 0.1), missile.blast_radius);
                    spawn_floating_damage(&mut commands, missile_pos, total_damage, Color::srgb(1.0, 0.4, 0.1));
                    // amount: 0.0 — damage already applied directly above to
                    // every module in the blast radius. process_ai_ship_damage_system
                    // used to re-apply this same total again via its own
                    // distance-sort, double-damaging and often hitting
                    // different blocks than the ones actually in the blast.
                    ai_damage_events.write(crate::events::AiShipDamaged {
                        target: ai_entity,
                        source: crate::events::DamageSource::Explosion,
                        amount: 0.0,
                        position: Some(missile_pos),
                        direction: None,
                        attacker: owner_ship,
                    });
                    commands.entity(missile_entity).despawn();
                    continue 'missiles;
                }
            }
        }

        for (creature_entity, _) in creature_grid.0.nearby(missile_pos, MAX_CREATURE_HIT_RADIUS) {
            let Ok((creature_transform, mut creature)) = creature_query.get_mut(creature_entity) else { continue };
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
            spawn_hit_effect(&mut commands, missile_pos, Color::srgb(1.0, 0.5, 0.1), missile.blast_radius);
            spawn_floating_damage(&mut commands, missile_pos, missile.damage, Color::srgb(1.0, 0.3, 0.1));

            commands.entity(missile_entity).despawn();
            break;
        }
    }
}
