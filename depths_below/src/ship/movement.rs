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


/// Handles ship input
pub fn ship_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut input_state: ResMut<InputState>,
) {
    let mut movement = Vec2::ZERO;
    let mut thruster_input = 0.0;

    // W/S: throttle forward/reverse
    if keyboard.pressed(KeyCode::KeyW) || keyboard.pressed(KeyCode::ArrowUp) {
        movement.y = 1.0; // forward thrust
    }
    if keyboard.pressed(KeyCode::KeyS) || keyboard.pressed(KeyCode::ArrowDown) {
        movement.y = -1.0; // reverse
    }

    // A/D: strafe left/right (facing follows the mouse cursor)
    if keyboard.pressed(KeyCode::KeyA) || keyboard.pressed(KeyCode::ArrowLeft) {
        movement.x -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyD) || keyboard.pressed(KeyCode::ArrowRight) {
        movement.x += 1.0;
    }

    // Q/E: vertical thrusters (ascend/descend)
    if keyboard.pressed(KeyCode::KeyQ) {
        thruster_input = 1.0; // thrust up
    }
    if keyboard.pressed(KeyCode::KeyE) {
        thruster_input = -1.0; // thrust down
    }

    input_state.movement = movement;
    input_state.thruster_input = thruster_input;
    // Shift: brake — retro-thrust against whatever direction we're drifting
    input_state.brake = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
}

/// Converts raw engine thrust into usable acceleration. Without this, the
/// starter ship's 1200 thrust / 1200 mass gave 1 unit/s² — one grid cell
/// per second per second, i.e. imperceptible. 180x puts cruising speed a
/// few seconds of burn away while keeping engine count meaningful.
const THRUST_SCALE: f32 = 180.0;

/// Max yaw rate at full deflection (rad/s). ~115°/s.
const MAX_TURN_RATE: f32 = 2.0;
/// How quickly angular velocity reaches the target rate (per second).
/// Higher = snappier stick response, framerate-independent.
const TURN_RESPONSE: f32 = 14.0;
/// Turn rate per radian of aim error. This is what makes SMALL corrections
/// fast — the previous low gain meant the cap above only mattered for huge
/// swings while ordinary cursor-tracking still felt sluggish.
const TURN_GAIN: f32 = 12.0;
/// When coasting (no thrust input, no brake), speed halves every this many
/// seconds. Pure Newtonian drift meant the ship "kept flying forward
/// forever" after any tap of W.
const COAST_HALF_LIFE: f32 = 1.4;
/// Flight assist: while thrusting forward, existing velocity is gently swung
/// toward the ship's facing, so turns become course changes instead of
/// endless sideways drift. 0.0 = pure Newtonian.
const VELOCITY_ALIGN_RATE: f32 = 1.2;

/// Applies space physics to ship movement (no drag, inertial flight)
pub fn ship_movement(
    time: Res<Time>,
    input_state: Res<InputState>,
    _config: Res<GameConfig>,
    camera_state: Res<crate::camera::CameraState>,
    // Without<OwnedByAiShip>: AI ships now carry real Engine/ModuleEfficiency
    // data too (see ai_ship::crew) — unscoped, this sum would let nearby AI
    // ships' staffed engines add thrust to the PLAYER's own ship the moment
    // any AI ship has crew (same class of leak the projectile-ownership and
    // staffing-HUD work already had to guard against elsewhere).
    engine_query: Query<(&Engine, &Module, Option<&CalculatedStats>, Option<&ModuleEfficiency>), Without<crate::ai_ship::components::OwnedByAiShip>>,
    thruster_query: Query<(&mut Thruster, &Module)>,
    mut ship_query: Query<(&mut Transform, &mut Velocity, &mut ShipPhysics, &mut ThrusterState), With<Ship>>,
    windows_query: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
) {
    let Ok((mut transform, mut velocity, mut physics, mut thruster_state)) = ship_query.single_mut() else {
        return;
    };

    let dt = time.delta_secs();

    // Calculate total thrust from active engines
    let total_thrust: f32 = engine_query
        .iter()
        .filter(|(_, module, _, _)| module.is_active)
        .map(|(engine, module, calculated_stats, eff)| {
            let efficiency = effective_efficiency(module, eff);
            get_engine_thrust(calculated_stats, engine) * efficiency
        })
        .sum();

    // --- FACING: nose follows the aim source ---
    // Controller right stick when it has aim (see InputState.gamepad_aim),
    // the mouse cursor otherwise. Proportional controller: turn rate scales
    // with how far off-target the nose is, capped at MAX_TURN_RATE, so the
    // ship settles on the target smoothly instead of oscillating past it.
    physics.rudder = input_state.movement.x;
    // While free-looking (holding T), the cursor is being used to pan the
    // camera, not aim — freezing the turn here stops the ship spinning to
    // face wherever the player happens to be looking.
    if !camera_state.free_look_active {
        let target_angle = if let Some(aim) = input_state.gamepad_aim {
            Some(aim.y.atan2(aim.x))
        } else if let (Ok(window), Ok((camera, cam_gt))) =
            (windows_query.single(), camera_query.single())
        {
            window.cursor_position()
                .and_then(|cursor| camera.viewport_to_world_2d(cam_gt, cursor).ok())
                .map(|cursor_world| cursor_world - transform.translation.truncate())
                .filter(|to_cursor| to_cursor.length_squared() > 4.0)
                .map(|to_cursor| to_cursor.y.atan2(to_cursor.x))
        } else {
            None
        };

        if let Some(target_angle) = target_angle {
            let mut diff = target_angle - physics.rotation;
            while diff > std::f32::consts::PI { diff -= std::f32::consts::TAU; }
            while diff < -std::f32::consts::PI { diff += std::f32::consts::TAU; }

            let target_rate = (diff * TURN_GAIN).clamp(-MAX_TURN_RATE, MAX_TURN_RATE);
            let blend = (TURN_RESPONSE * dt).min(1.0);
            physics.angular_velocity += (target_rate - physics.angular_velocity) * blend;
        }
    } else {
        // Decay any leftover turn rate instead of leaving it frozen —
        // otherwise the ship keeps coasting on whatever angular velocity
        // it had the instant free-look was pressed.
        let blend = (TURN_RESPONSE * dt).min(1.0);
        physics.angular_velocity -= physics.angular_velocity * blend;
    }
    physics.rotation += physics.angular_velocity * dt;

    // The ship root actually rotates — previously facing only existed in the
    // physics math and the hull visual just mirrored left/right (side-view
    // submarine holdover). Rotating the root carries all hull/module children.
    transform.rotation = Quat::from_rotation_z(physics.rotation);

    // --- THROTTLE ---
    let throttle_input = input_state.movement.y;
    physics.throttle = physics.throttle + (throttle_input - physics.throttle) * 3.0 * dt;

    // Direction ship faces
    let facing = Vec2::new(physics.rotation.cos(), physics.rotation.sin());

    // Thrust force: forward/reverse along facing plus lateral strafe (A/D).
    // Strafe runs at 50% main thrust — maneuvering jets, not the main drive.
    let right = Vec2::new(facing.y, -facing.x);
    let thrust_force = facing * total_thrust * physics.throttle * THRUST_SCALE
        + right * total_thrust * input_state.movement.x * 0.5 * THRUST_SCALE;

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

    // Thruster input directly applies vertical force (scaled to match main thrust)
    let vertical_thrust = input_state.thruster_input * 400.0 * THRUST_SCALE / 4.0
        + thruster_effect * thruster_state.base_drift * 30.0;
    let thruster_force = Vec2::new(0.0, vertical_thrust);
    thruster_state.current = input_state.thruster_input;

    // --- NET FORCE ---
    let net_force = thrust_force + drag_force + thruster_force;
    let acceleration = net_force / physics.mass;

    // Update velocity
    velocity.0 += acceleration * dt;

    // Brake (Shift): retro-thrust straight against the drift. In a dragless
    // void this is the only way to actually stop.
    if input_state.brake {
        let speed = velocity.0.length();
        if speed > 1.0 {
            let decel = (total_thrust * THRUST_SCALE / physics.mass) * dt;
            let new_speed = (speed - decel).max(0.0);
            velocity.0 = velocity.0 * (new_speed / speed);
        } else {
            velocity.0 = Vec2::ZERO;
        }
    }

    // Flight assist: under forward thrust, swing existing momentum toward
    // the ship's facing so a turn actually changes course (see const doc).
    if !input_state.brake && physics.throttle > 0.1 && VELOCITY_ALIGN_RATE > 0.0 {
        let speed = velocity.0.length();
        if speed > 1.0 {
            let t = (VELOCITY_ALIGN_RATE * physics.throttle * dt).min(1.0);
            velocity.0 = velocity.0.lerp(facing * speed, t);
        }
    }

    // Coast damping: with no thrust input and no brake, bleed speed off
    // automatically (arcade handling — release W and the ship settles).
    if !input_state.brake && input_state.movement.y.abs() < 0.05 && input_state.movement.x.abs() < 0.05 {
        let decay = (0.5_f32).powf(dt / COAST_HALF_LIFE);
        velocity.0 *= decay;
        if velocity.0.length_squared() < 4.0 {
            velocity.0 = Vec2::ZERO;
        }
    }

    // Apply velocity to position
    transform.translation.x += velocity.0.x * dt;
    transform.translation.y += velocity.0.y * dt;

    // (The old left/right sprite mirror is gone — the root now truly rotates.)
    transform.scale.x = transform.scale.x.abs();
}

/// Updates ship position tracking based on Y position
pub fn update_depth(
    time: Res<Time>,
    input_state: Res<InputState>,
    mut thruster_query: Query<(&mut Thruster, &Module)>,
    mut ship_query: Query<(&Transform, &mut Depth), With<Ship>>,
) {
    let Ok((transform, mut depth)) = ship_query.single_mut() else {
        return;
    };

    // Update thrusters based on Q/E input
    for (mut thruster, module) in thruster_query.iter_mut() {
        if module.is_active {
            let response_rate = 0.3 * time.delta_secs();
            thruster.current_output = (thruster.current_output + input_state.thruster_input * response_rate).clamp(0.0, 1.0);
        }
    }

    // Distance from home (origin) drives zone/danger progression — radial,
    // so danger grows in every direction. The old code clamped the ship
    // between y=0 and y=-5000: the ocean surface and the seafloor. In space
    // there is no surface — both invisible walls are gone.
    depth.0 = transform.translation.truncate().length();
}

/// Consumes fuel from engines and deactivates them when fuel runs out.
/// PLAYER ENGINES ONLY: AI ships reuse the same Engine components, and an
/// unscoped query made every spawned AI ship's engines drain the player's
/// fuel tank (a raider wave emptied it in seconds).
pub fn update_fuel_consumption(
    time: Res<Time>,
    mut fuel_state: ResMut<FuelState>,
    mut engine_query: Query<(&Engine, &mut Module, &ChildOf)>,
    ship_query: Query<Entity, With<Ship>>,
    mut notifications: MessageWriter<ShowNotification>,
    mut warned_25: Local<bool>,
    mut warned_10: Local<bool>,
) {
    let Ok(player_ship) = ship_query.single() else { return };
    let dt = time.delta_secs();
    let mut total_consumption = 0.0;

    // Calculate fuel consumption from the player's active engines
    for (engine, module, parent) in engine_query.iter() {
        if parent.parent() != player_ship { continue; }
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
        notifications.write(ShowNotification {
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
        notifications.write(ShowNotification {
            message: "FUEL CRITICAL (10%)! Engines will shut down soon!".into(),
            notification_type: NotificationType::Danger,
            duration: 4.0,
        });
    }
    if fuel_pct > 0.15 {
        *warned_10 = false;
    }

    // Deactivate the player's engines when fuel runs out
    if fuel_state.current_fuel <= 0.0 {
        for (_engine, mut module, parent) in engine_query.iter_mut() {
            if parent.parent() != player_ship { continue; }
            if module.is_active {
                module.is_active = false;
                notifications.write(ShowNotification {
                    message: "Engine shut down! No fuel remaining!".into(),
                    notification_type: NotificationType::Danger,
                    duration: 4.0,
                });
            }
        }
    }
}
