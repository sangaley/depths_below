use bevy::prelude::*;

use crate::components::*;
use super::components::*;

/// Moves AI submarines toward their nav destination, reading child Engine components for thrust.
pub fn ai_sub_movement_system(
    time: Res<Time>,
    mut ai_subs: Query<(
        Entity,
        &mut Transform,
        &mut Velocity,
        &AiSubBehavior,
        &AiSubNav,
        &AiSubState,
        &AiSubType,
        &Children,
    ), With<AiSubmarine>>,
    engine_query: Query<(&Engine, &Module, &OwnedByAiSub)>,
) {
    let dt = time.delta_seconds();

    for (_entity, mut transform, mut velocity, behavior, nav, state, sub_type, children) in ai_subs.iter_mut() {
        if *behavior == AiSubBehavior::Dead || state.is_destroyed {
            velocity.0 = Vec2::ZERO;
            continue;
        }

        // Sum thrust from child engines
        let mut total_thrust = 0.0_f32;
        for &child in children.iter() {
            if let Ok((engine, module, _owned)) = engine_query.get(child) {
                if module.is_active && module.health > 0.0 {
                    let efficiency = module.health / module.max_health;
                    total_thrust += engine.thrust * efficiency;
                }
            }
        }

        // Speed multiplier per behavior
        let behavior_mult = match behavior {
            AiSubBehavior::Patrolling | AiSubBehavior::Idle => 0.6,
            AiSubBehavior::FollowingTradeRoute => 0.8,
            AiSubBehavior::Engaging => 1.0,
            AiSubBehavior::Fleeing | AiSubBehavior::EvadingCreature => 1.3,
            AiSubBehavior::Salvaging => 0.3,
            AiSubBehavior::Dead => 0.0,
        };

        // Faction-specific speed characteristics
        let faction_mult = match sub_type {
            AiSubType::GlassEye => 1.6,      // fastest sub in the game
            AiSubType::RustSwarm => 1.3,      // fast but erratic
            AiSubType::Blackwater => 1.2,     // quick tactical sub
            AiSubType::Leviathan => 0.9,      // creature-towed, moderate
            AiSubType::AbyssalCult => 1.0,    // average
            AiSubType::Drowned => 0.7,        // sluggish, damaged engines
            AiSubType::PressureKing => 0.8,   // heavy but powerful engines
            AiSubType::IronTide => 0.6,       // slow battleship
        };

        let max_speed = total_thrust * behavior_mult * faction_mult;

        if let Some(destination) = nav.destination {
            let pos = transform.translation.truncate();
            let to_dest = destination - pos;
            let dist = to_dest.length();

            if dist > 5.0 {
                let direction = to_dest / dist;

                // Gradually accelerate toward desired velocity
                let desired_vel = direction * max_speed;
                let accel = 80.0 * dt;
                velocity.0 = velocity.0 + (desired_vel - velocity.0) * accel.min(1.0);

                // Update rotation to face movement direction
                let target_angle = direction.y.atan2(direction.x);
                let current_angle = transform.rotation.to_euler(EulerRot::ZYX).0;
                let angle_diff = (target_angle - current_angle + std::f32::consts::PI)
                    .rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
                let new_angle = current_angle + angle_diff * 2.0 * dt;
                transform.rotation = Quat::from_rotation_z(new_angle);
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

/// Updates AI submarine depth from Y position
pub fn ai_ballast_system(
    mut ai_subs: Query<(&Transform, &mut AiSubState), With<AiSubmarine>>,
) {
    for (transform, mut state) in ai_subs.iter_mut() {
        // Depth = negative Y (deeper = more negative Y = higher depth value)
        state.depth = (-transform.translation.y / 10.0).max(0.0);
    }
}

/// Consumes fuel based on engine activity
pub fn ai_fuel_system(
    time: Res<Time>,
    mut ai_subs: Query<(&mut AiSubState, &AiSubBehavior, &Children), With<AiSubmarine>>,
    engine_query: Query<(&Engine, &Module, &OwnedByAiSub)>,
) {
    let dt = time.delta_seconds();

    for (mut state, behavior, children) in ai_subs.iter_mut() {
        if *behavior == AiSubBehavior::Dead {
            continue;
        }

        let mut fuel_consumption = 0.0_f32;
        for &child in children.iter() {
            if let Ok((engine, module, _)) = engine_query.get(child) {
                if module.is_active && module.health > 0.0 {
                    fuel_consumption += engine.fuel_consumption;
                }
            }
        }

        // Reduce consumption when idle/patrolling
        let consumption_mult = match behavior {
            AiSubBehavior::Idle => 0.1,
            AiSubBehavior::Patrolling | AiSubBehavior::Salvaging => 0.5,
            AiSubBehavior::FollowingTradeRoute => 0.7,
            _ => 1.0,
        };

        state.fuel = (state.fuel - fuel_consumption * consumption_mult * dt).max(0.0);
    }
}
