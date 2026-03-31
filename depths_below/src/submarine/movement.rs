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
    let mut thruster_input = 0.0;

    // W/S: throttle forward/reverse
    if keyboard.pressed(KeyCode::W) || keyboard.pressed(KeyCode::Up) {
        movement.y = 1.0; // forward thrust
    }
    if keyboard.pressed(KeyCode::S) || keyboard.pressed(KeyCode::Down) {
        movement.y = -1.0; // reverse
    }

    // A/D: yaw left/right (turns the ship)
    if keyboard.pressed(KeyCode::A) || keyboard.pressed(KeyCode::Left) {
        movement.x -= 1.0;
    }
    if keyboard.pressed(KeyCode::D) || keyboard.pressed(KeyCode::Right) {
        movement.x += 1.0;
    }

    // Q/E: vertical thrusters (ascend/descend)
    if keyboard.pressed(KeyCode::Q) {
        thruster_input = 1.0; // thrust up
    }
    if keyboard.pressed(KeyCode::E) {
        thruster_input = -1.0; // thrust down
    }

    input_state.movement = movement;
    input_state.thruster_input = thruster_input;
}

/// Applies space physics to ship movement (no drag, inertial flight)
pub fn submarine_movement(
    time: Res<Time>,
    input_state: Res<InputState>,
    _config: Res<GameConfig>,
    engine_query: Query<(&Engine, &Module, Option<&CalculatedStats>, Option<&ModuleEfficiency>)>,
    thruster_query: Query<(&mut Thruster, &Module)>,
    mut submarine_query: Query<(&mut Transform, &mut Velocity, &mut SubmarinePhysics, &mut ThrusterState), With<Submarine>>,
) {
    let Ok((mut transform, mut velocity, mut physics, mut thruster_state)) = submarine_query.get_single_mut() else {
        return;
    };

    let dt = time.delta_seconds();

    // Calculate total thrust from active engines
    let total_thrust: f32 = engine_query
        .iter()
        .filter(|(_, module, _, _)| module.is_active)
        .map(|(engine, module, calculated_stats, eff)| {
            let efficiency = effective_efficiency(module, eff);
            get_engine_thrust(calculated_stats, engine) * efficiency
        })
        .sum();

    // --- YAW / TURNING ---
    physics.rudder = input_state.movement.x;
    let speed = velocity.0.length();

    // In space, RCS thrusters allow turning at any speed
    let turn_effectiveness = (speed / 80.0).clamp(0.5, 1.0);
    let torque = physics.rudder * 3.0 * turn_effectiveness;
    physics.angular_velocity += torque * dt;
    physics.angular_velocity *= 0.95_f32; // RCS damping
    physics.rotation += physics.angular_velocity * dt;

    // --- THROTTLE ---
    let throttle_input = input_state.movement.y;
    physics.throttle = physics.throttle + (throttle_input - physics.throttle) * 3.0 * dt;

    // Direction ship faces
    let facing = Vec2::new(physics.rotation.cos(), physics.rotation.sin());

    // Thrust force
    let thrust_force = facing * total_thrust * physics.throttle;

    // --- SPACE DRAG (minimal — just light dampening for gameplay) ---
    let v_sq = velocity.0.length_squared();
    let drag_magnitude = 0.5 * physics.drag_coefficient * v_sq * physics.frontal_area * 0.00002;
    let drag_force = if v_sq > 0.001 {
        -velocity.0.normalize() * drag_magnitude
    } else {
        Vec2::ZERO
    };

    // --- VERTICAL THRUSTERS ---
    let thruster_effect: f32 = thruster_query
        .iter()
        .filter(|(_, module)| module.is_active)
        .map(|(thruster, _)| thruster.current_output * thruster.thrust_power)
        .sum();

    // Thruster input directly applies vertical force
    let vertical_thrust = input_state.thruster_input * 50.0 + thruster_effect * thruster_state.base_drift * 30.0;
    let thruster_force = Vec2::new(0.0, vertical_thrust);
    thruster_state.current = input_state.thruster_input;

    // --- NET FORCE ---
    let net_force = thrust_force + drag_force + thruster_force;
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

/// Updates ship position tracking based on Y position
pub fn update_depth(
    time: Res<Time>,
    input_state: Res<InputState>,
    mut thruster_query: Query<(&mut Thruster, &Module)>,
    mut submarine_query: Query<(&mut Transform, &mut Depth), With<Submarine>>,
) {
    let Ok((mut transform, mut depth)) = submarine_query.get_single_mut() else {
        return;
    };

    // Update thrusters based on Q/E input
    for (mut thruster, module) in thruster_query.iter_mut() {
        if module.is_active {
            let response_rate = 0.3 * time.delta_seconds();
            thruster.current_output = (thruster.current_output + input_state.thruster_input * response_rate).clamp(0.0, 1.0);
        }
    }

    // Position is derived from Y (negative Y = further from safe zone)
    if transform.translation.y > 0.0 {
        transform.translation.y = 0.0;
    }
    if transform.translation.y < -5000.0 {
        transform.translation.y = -5000.0;
    }

    depth.0 = (-transform.translation.y).max(0.0);
}

/// Consumes fuel from engines and deactivates them when fuel runs out
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
