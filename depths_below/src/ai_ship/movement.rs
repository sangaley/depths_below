use bevy::prelude::*;

use crate::components::*;
use super::components::*;

/// Moves AI ships toward their nav destination, reading child Engine components for thrust.
pub fn ai_ship_movement_system(
    time: Res<Time>,
    mut ai_ships: Query<(
        Entity,
        &mut Transform,
        &mut Velocity,
        &AiShipBehavior,
        &AiShipNav,
        &AiShipState,
        &AiShipType,
        &Children,
    ), With<AiShip>>,
    engine_query: Query<(&Engine, &Module, &OwnedByAiShip)>,
    weapon_query: Query<(&Weapon, &Module, &OwnedByAiShip), Without<Engine>>,
) {
    let dt = time.delta_secs();

    for (_entity, mut transform, mut velocity, behavior, nav, state, ship_type, children) in ai_ships.iter_mut() {
        if *behavior == AiShipBehavior::Dead || state.is_destroyed {
            velocity.0 = Vec2::ZERO;
            continue;
        }

        // Sum thrust from child engines — tracking the undamaged total too,
        // so we know how crippled the drive section is.
        let mut total_thrust = 0.0_f32;
        let mut max_possible_thrust = 0.0_f32;
        for child in children.iter() {
            if let Ok((engine, module, _owned)) = engine_query.get(child) {
                max_possible_thrust += engine.thrust;
                if module.is_active && module.health > 0.0 {
                    let efficiency = module.health / module.max_health;
                    total_thrust += engine.thrust * efficiency;
                }
            }
        }

        // DEATH SPIRALS — a ship with mangled engines can't hold a heading.
        // Thrust asymmetry shows up as a slow sinusoidal weave injected into
        // its steering; the worse the drive damage, the wider and drunker
        // the wander. Phase from the entity index so a damaged squadron
        // doesn't wobble in lockstep.
        let engine_integrity = if max_possible_thrust > 0.0 {
            (total_thrust / max_possible_thrust).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let wobble_angle = if engine_integrity < 0.85 {
            let hurt = 1.0 - engine_integrity;
            let phase = (_entity.to_bits() % 97) as f32;
            (time.elapsed_secs() * 2.3 + phase).sin() * hurt * hurt * 1.1
        } else {
            0.0
        };

        // Speed multiplier per behavior
        let behavior_mult = match behavior {
            AiShipBehavior::Patrolling | AiShipBehavior::Idle => 0.6,
            AiShipBehavior::FollowingTradeRoute => 0.8,
            AiShipBehavior::Engaging => 1.0,
            AiShipBehavior::Fleeing | AiShipBehavior::EvadingCreature => 1.3,
            AiShipBehavior::Salvaging => 0.3,
            AiShipBehavior::Dead => 0.0,
        };

        // Faction-specific speed characteristics
        let faction_mult = match ship_type {
            AiShipType::GlassEye => 1.6,      // fastest ship in the game
            AiShipType::RustSwarm => 1.3,      // fast but erratic
            AiShipType::Blackwater => 1.2,     // quick tactical ship
            AiShipType::Leviathan => 0.9,      // creature-towed, moderate
            AiShipType::AbyssalCult => 1.0,    // average
            AiShipType::Drowned => 0.7,        // sluggish, damaged engines
            AiShipType::PressureKing => 0.8,   // heavy but powerful engines
            AiShipType::IronTide => 0.6,       // slow battleship
            AiShipType::Dreadnought => 0.4,    // colossal, lumbering
            AiShipType::VoidTitan => 0.35,     // barely moves, but it doesn't need to
        };

        // Global AI pace.
        const AI_SPEED_SCALE: f32 = 0.8;
        let max_speed = total_thrust * behavior_mult * faction_mult * AI_SPEED_SCALE;

        // Frame-rate-independent smoothing fraction: closes this much of the
        // remaining gap (to a target velocity or a target angle) per second.
        // The old `(80.0 * dt).min(1.0)` saturated to a full 1.0 snap at any
        // normal framerate — velocity teleported to its new value every
        // single frame while rotation eased in smoothly, so the ship visibly
        // slid in a direction its nose wasn't pointed at. This converges
        // continuously instead, same shape at 30fps or 300fps.
        let vel_blend = 1.0 - (-6.0_f32 * dt).exp();
        let turn_blend = 1.0 - (-4.5_f32 * dt).exp();

        if let Some(destination) = nav.destination {
            let pos = transform.translation.truncate();
            // Drive damage skews the perceived heading — the ship weaves
            // toward where its broken engines are dragging it.
            let to_dest = Vec2::from_angle(wobble_angle).rotate(destination - pos);
            let dist = to_dest.length();

            // Combat standoff: while engaging, hold a firing distance from the
            // target instead of flying into (and through) it. Below the band:
            // back off. Inside the band: orbit sideways. Beyond it: approach.
            // RustSwarm keeps its point-blank ramming — that IS their faction,
            // regardless of what it's carrying. Everyone else holds at their
            // own longest-range active weapon (85% of its range, so they
            // fight solidly inside their own reach instead of right at the
            // ragged edge) — a ship stripped down to short-range guns closes
            // in, one carrying a sniper weapon hangs back, instead of every
            // ship in a faction using the same fixed distance regardless of
            // loadout. Falls back to the old faction defaults only if a ship
            // somehow has no active weapons (e.g. mid-repair).
            let max_weapon_range = children.iter()
                .filter_map(|c| weapon_query.get(c).ok())
                .filter(|(_, module, _)| module.is_active && module.health > 0.0)
                .map(|(weapon, _, _)| weapon.range)
                .fold(0.0_f32, f32::max);

            let standoff = if *behavior == AiShipBehavior::Engaging {
                match ship_type {
                    AiShipType::RustSwarm => 0.0,
                    _ if max_weapon_range > 0.0 => max_weapon_range * 0.85,
                    AiShipType::VoidTitan | AiShipType::Dreadnought => 8000.0,
                    AiShipType::IronTide | AiShipType::PressureKing => 6000.0,
                    AiShipType::Blackwater => 4400.0,
                    _ => 3600.0,
                }
            } else {
                0.0
            };

            if standoff > 0.0 && dist < standoff * 1.15 && dist > 1.0 {
                let direction = to_dest / dist;
                let tangent = Vec2::new(-direction.y, direction.x);
                let desired_vel = if dist < standoff * 0.85 {
                    // Too close — back away while keeping some lateral motion
                    -direction * max_speed * 0.6 + tangent * max_speed * 0.3
                } else {
                    // In the band — strafe an orbit around the target
                    tangent * max_speed * 0.55
                };
                velocity.0 = velocity.0.lerp(desired_vel, vel_blend);

                // Face the target while holding the ring. Slerp between
                // quaternions directly rather than decomposing to a Z euler
                // angle and reconstructing — the decompose/rebuild round trip
                // is exact for a pure Z rotation, but slerp is the standard,
                // robust way to ease rotation and sidesteps it entirely.
                let target_angle = direction.y.atan2(direction.x);
                let target_rotation = Quat::from_rotation_z(target_angle);
                transform.rotation = transform.rotation.slerp(target_rotation, turn_blend);
            } else if dist > 5.0 {
                let direction = to_dest / dist;

                // Gradually accelerate toward desired velocity
                let desired_vel = direction * max_speed;
                velocity.0 = velocity.0.lerp(desired_vel, vel_blend);

                // Update rotation to face movement direction
                let target_angle = direction.y.atan2(direction.x);
                let target_rotation = Quat::from_rotation_z(target_angle);
                transform.rotation = transform.rotation.slerp(target_rotation, turn_blend);
            } else {
                // Near destination, slow down
                velocity.0 *= (1.0 - 3.0 * dt).max(0.0);
            }
        } else {
            // No destination, drift to stop
            velocity.0 *= (1.0 - 2.0 * dt).max(0.0);
        }

        // Apply drag
        let drag = 0.5 * dt;
        velocity.0 *= 1.0 - drag;

        // Apply velocity to position
        transform.translation.x += velocity.0.x * dt;
        transform.translation.y += velocity.0.y * dt;
    }
}

/// Updates AI ship depth from Y position
pub fn ai_thruster_system(
    mut ai_ships: Query<(&Transform, &mut AiShipState), With<AiShip>>,
) {
    for (transform, mut state) in ai_ships.iter_mut() {
        // Depth = negative Y (deeper = more negative Y = higher depth value)
        state.depth = (-transform.translation.y / 10.0).max(0.0);
    }
}

/// Consumes fuel based on engine activity
pub fn ai_fuel_system(
    time: Res<Time>,
    mut ai_ships: Query<(&mut AiShipState, &AiShipBehavior, &Children), With<AiShip>>,
    engine_query: Query<(&Engine, &Module, &OwnedByAiShip)>,
) {
    let dt = time.delta_secs();

    for (mut state, behavior, children) in ai_ships.iter_mut() {
        if *behavior == AiShipBehavior::Dead {
            continue;
        }

        let mut fuel_consumption = 0.0_f32;
        for child in children.iter() {
            if let Ok((engine, module, _)) = engine_query.get(child) {
                if module.is_active && module.health > 0.0 {
                    fuel_consumption += engine.fuel_consumption;
                }
            }
        }

        // Reduce consumption when idle/patrolling
        let consumption_mult = match behavior {
            AiShipBehavior::Idle => 0.1,
            AiShipBehavior::Patrolling | AiShipBehavior::Salvaging => 0.5,
            AiShipBehavior::FollowingTradeRoute => 0.7,
            _ => 1.0,
        };

        state.fuel = (state.fuel - fuel_consumption * consumption_mult * dt).max(0.0);
    }
}
