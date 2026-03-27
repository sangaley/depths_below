use bevy::prelude::*;
use crate::components::*;
use crate::resources::*;
use crate::events::*;

// Helper functions to get effective engine stats (CalculatedStats or base Engine)
fn get_engine_thrust(calculated: Option<&CalculatedStats>, engine: &Engine) -> f32 {
    calculated
        .and_then(|c| c.engine.as_ref())
        .map(|e| e.thrust)
        .unwrap_or(engine.thrust)
}


/// Handles submarine input
pub fn submarine_input(
    keyboard: Res<Input<KeyCode>>,
    mut input_state: ResMut<InputState>,
) {
    let mut movement = Vec2::ZERO;
    let mut ballast_input = 0.0;

    // W/S: throttle forward/reverse
    if keyboard.pressed(KeyCode::W) || keyboard.pressed(KeyCode::Up) {
        movement.y = 1.0; // forward thrust
    }
    if keyboard.pressed(KeyCode::S) || keyboard.pressed(KeyCode::Down) {
        movement.y = -1.0; // reverse
    }

    // A/D: rudder left/right (turns the sub)
    if keyboard.pressed(KeyCode::A) || keyboard.pressed(KeyCode::Left) {
        movement.x -= 1.0;
    }
    if keyboard.pressed(KeyCode::D) || keyboard.pressed(KeyCode::Right) {
        movement.x += 1.0;
    }

    // Q/E: ballast fill/empty (sink/rise)
    if keyboard.pressed(KeyCode::Q) {
        ballast_input = -1.0; // empty ballast = rise
    }
    if keyboard.pressed(KeyCode::E) {
        ballast_input = 1.0; // fill ballast = sink
    }

    input_state.movement = movement;
    input_state.ballast_input = ballast_input;
}

/// Applies realistic physics to submarine movement
pub fn submarine_movement(
    time: Res<Time>,
    input_state: Res<InputState>,
    _config: Res<GameConfig>,
    engine_query: Query<(&Engine, &Module, Option<&CalculatedStats>, Option<&ModuleEfficiency>)>,
    ballast_query: Query<(&mut Ballast, &Module)>,
    mut submarine_query: Query<(&mut Transform, &mut Velocity, &mut SubmarinePhysics, &mut Buoyancy), With<Submarine>>,
) {
    let Ok((mut transform, mut velocity, mut physics, mut buoyancy)) = submarine_query.get_single_mut() else {
        return;
    };

    let dt = time.delta_seconds();

    // Calculate total thrust from active engines
    // Uses ModuleEfficiency (damage * staffing) when available, falls back to damage-only
    let total_thrust: f32 = engine_query
        .iter()
        .filter(|(_, module, _, _)| module.is_active)
        .map(|(engine, module, calculated_stats, eff)| {
            let efficiency = effective_efficiency(module, eff);
            get_engine_thrust(calculated_stats, engine) * efficiency
        })
        .sum();

    // --- RUDDER / TURNING ---
    physics.rudder = input_state.movement.x;
    let speed = velocity.0.length();

    // Turning works at any speed - minimum 50% effectiveness when stopped
    let turn_effectiveness = (speed / 80.0).clamp(0.5, 1.0);
    let torque = physics.rudder * 3.0 * turn_effectiveness;
    physics.angular_velocity += torque * dt;
    physics.angular_velocity *= 0.88_f32; // angular drag
    physics.rotation += physics.angular_velocity * dt;

    // --- THROTTLE ---
    let throttle_input = input_state.movement.y;
    physics.throttle = physics.throttle + (throttle_input - physics.throttle) * 3.0 * dt;

    // Direction sub faces
    let facing = Vec2::new(physics.rotation.cos(), physics.rotation.sin());

    // Thrust force
    let thrust_force = facing * total_thrust * physics.throttle;

    // --- DRAG ---
    let water_density = 1025.0 + (transform.translation.y.abs() * 0.001); // slightly denser deep
    let v_sq = velocity.0.length_squared();
    let drag_magnitude = 0.5 * physics.drag_coefficient * water_density * v_sq * physics.frontal_area * 0.0001;
    let drag_force = if v_sq > 0.001 {
        -velocity.0.normalize() * drag_magnitude
    } else {
        Vec2::ZERO
    };

    // --- BUOYANCY / BALLAST ---
    let ballast_effect: f32 = ballast_query
        .iter()
        .filter(|(_, module)| module.is_active)
        .map(|(ballast, _)| (ballast.current_level - 0.5) * 2.0) // -1 to 1
        .sum();

    // Ballast input modifies ballast over time (handled in update_depth)
    let net_buoyancy = -ballast_effect * 50.0 + buoyancy.base_buoyancy * 30.0;
    let buoyancy_force = Vec2::new(0.0, net_buoyancy);
    buoyancy.current = ballast_effect;

    // --- NET FORCE ---
    let net_force = thrust_force + drag_force + buoyancy_force;
    let acceleration = net_force / physics.mass;

    // Update velocity
    velocity.0 += acceleration * dt;

    // Apply velocity to position
    transform.translation.x += velocity.0.x * dt;
    transform.translation.y += velocity.0.y * dt;

    // Flip sprite based on facing direction
    if physics.rotation.cos() < 0.0 {
        transform.scale.x = -transform.scale.x.abs();
    } else {
        transform.scale.x = transform.scale.x.abs();
    }
}

/// Updates submarine depth based on Y position
pub fn update_depth(
    time: Res<Time>,
    input_state: Res<InputState>,
    mut ballast_query: Query<(&mut Ballast, &Module)>,
    mut submarine_query: Query<(&mut Transform, &mut Depth), With<Submarine>>,
) {
    let Ok((mut transform, mut depth)) = submarine_query.get_single_mut() else {
        return;
    };

    // Update ballast tanks based on Q/E input
    for (mut ballast, module) in ballast_query.iter_mut() {
        if module.is_active {
            let fill_rate = 0.3 * time.delta_seconds();
            ballast.current_level = (ballast.current_level + input_state.ballast_input * fill_rate).clamp(0.0, 1.0);
        }
    }

    // Depth is derived from Y position (negative Y = deeper)
    // Clamp to surface (y=0) and max depth 500m (y=-5000)
    if transform.translation.y > 0.0 {
        transform.translation.y = 0.0;
    }
    if transform.translation.y < -5000.0 {
        transform.translation.y = -5000.0;
    }

    depth.0 = (-transform.translation.y).max(0.0);
}

/// Consumes fuel from engines and deactivates them when fuel runs out (Phase 3.3)
pub fn update_fuel_consumption(
    time: Res<Time>,
    mut fuel_state: ResMut<FuelState>,
    mut engine_query: Query<(&Engine, &mut Module)>,
    mut notifications: EventWriter<ShowNotification>,
    mut warned_25: Local<bool>,
    mut warned_10: Local<bool>,
) {
    let dt = time.delta_seconds();
    let mut total_consumption = 0.0;

    // Calculate fuel consumption from active engines
    for (engine, module) in engine_query.iter() {
        if module.is_active {
            total_consumption += engine.fuel_consumption * fuel_state.fuel_consumption_rate * dt;
        }
    }

    if total_consumption > 0.0 {
        fuel_state.current_fuel = (fuel_state.current_fuel - total_consumption).max(0.0);
    }

    let fuel_pct = if fuel_state.max_fuel > 0.0 {
        fuel_state.current_fuel / fuel_state.max_fuel
    } else {
        1.0
    };

    // Warning at 25%
    if fuel_pct <= 0.25 && fuel_pct > 0.10 && !*warned_25 {
        *warned_25 = true;
        notifications.send(ShowNotification {
            message: "Fuel at 25%! Consider conserving engine power.".into(),
            notification_type: NotificationType::Warning,
            duration: 3.0,
        });
    }
    if fuel_pct > 0.30 {
        *warned_25 = false;
    }

    // Warning at 10%
    if fuel_pct <= 0.10 && fuel_pct > 0.0 && !*warned_10 {
        *warned_10 = true;
        notifications.send(ShowNotification {
            message: "FUEL CRITICAL (10%)! Engines will shut down soon!".into(),
            notification_type: NotificationType::Danger,
            duration: 4.0,
        });
    }
    if fuel_pct > 0.15 {
        *warned_10 = false;
    }

    // Deactivate engines when fuel runs out
    if fuel_state.current_fuel <= 0.0 {
        for (_engine, mut module) in engine_query.iter_mut() {
            if module.is_active {
                module.is_active = false;
                notifications.send(ShowNotification {
                    message: "Engine shut down! No fuel remaining!".into(),
                    notification_type: NotificationType::Danger,
                    duration: 4.0,
                });
            }
        }
    }
}
