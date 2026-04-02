use bevy::prelude::*;
use rand::Rng;
use crate::components::{Submarine, Velocity};
use crate::events::{ShowNotification, NotificationType};
use super::components::*;
use super::resources::*;
use super::events::*;
use super::spawning;

/// V key initiates warp charge. Hold to charge, release to cancel.
/// When charge completes, jump to a new star system.
pub fn warp_input_system(
    mut commands: Commands,
    keyboard: Res<Input<KeyCode>>,
    time: Res<Time>,
    config: Res<CelestialConfig>,
    galaxy: Res<GalaxyState>,
    sub_query: Query<Entity, (With<Submarine>, Without<WarpCharging>)>,
    mut charging_query: Query<(Entity, &mut WarpCharging), With<Submarine>>,
    mut notifications: EventWriter<ShowNotification>,
    mut jump_events: EventWriter<WarpJumpStarted>,
) {
    // Start charging when V is pressed
    if keyboard.just_pressed(KeyCode::V) {
        if let Ok(sub_entity) = sub_query.get_single() {
            let target_system = galaxy.next_system_id;

            commands.entity(sub_entity).insert(WarpCharging {
                target_system,
                charge_timer: Timer::from_seconds(config.warp_charge_time, TimerMode::Once),
            });

            notifications.send(ShowNotification {
                message: "Warp drive charging... hold V to jump!".into(),
                notification_type: NotificationType::Info,
                duration: config.warp_charge_time + 1.0,
            });
        }
    }

    // Cancel if V is released before charge completes
    if keyboard.just_released(KeyCode::V) {
        if let Ok((entity, charging)) = charging_query.get_single() {
            if !charging.charge_timer.finished() {
                commands.entity(entity).remove::<WarpCharging>();
                notifications.send(ShowNotification {
                    message: "Warp cancelled.".into(),
                    notification_type: NotificationType::Info,
                    duration: 2.0,
                });
            }
        }
    }

    // Tick charge timer
    if let Ok((entity, mut charging)) = charging_query.get_single_mut() {
        charging.charge_timer.tick(time.delta());

        // Progress notifications
        let pct = charging.charge_timer.percent();
        if pct > 0.5 && pct < 0.55 {
            notifications.send(ShowNotification {
                message: "Warp drive at 50%...".into(),
                notification_type: NotificationType::Warning,
                duration: 1.5,
            });
        }

        // Jump when charge completes
        if charging.charge_timer.finished() {
            let target = charging.target_system;
            commands.entity(entity).remove::<WarpCharging>();

            jump_events.send(WarpJumpStarted {
                target_system: target,
            });
        }
    }
}

/// Execute the warp jump: despawn old system, spawn new one, move ship
pub fn execute_warp_jump(
    mut commands: Commands,
    mut jump_events: EventReader<WarpJumpStarted>,
    mut galaxy: ResMut<GalaxyState>,
    _config: Res<CelestialConfig>,
    // Despawn old system entities
    celestial_query: Query<(Entity, &StarSystemMember)>,
    mut sub_query: Query<(&mut Transform, &mut Velocity), With<Submarine>>,
    mut notifications: EventWriter<ShowNotification>,
    mut completed_events: EventWriter<WarpJumpCompleted>,
) {
    for event in jump_events.iter() {
        let Ok((mut sub_transform, mut sub_velocity)) = sub_query.get_single_mut() else {
            continue;
        };

        // Despawn all entities from the current system
        let current_system = galaxy.current_system;
        for (entity, member) in celestial_query.iter() {
            if member.system_id == current_system {
                commands.entity(entity).despawn_recursive();
            }
        }

        // Generate new system
        let new_id = event.target_system;
        let mut rng = rand::thread_rng();
        let seed = rng.gen::<u64>();

        // New system center — offset from origin
        let center = Vec2::new(
            rng.gen_range(-20_000.0..20_000.0),
            rng.gen_range(-30_000.0..-5_000.0),
        );

        let system_info = spawning::spawn_star_system(
            &mut commands,
            center,
            new_id,
            seed,
        );

        // Spawn asteroids in the new system
        let asteroid_offset = Vec2::new(
            rng.gen_range(-50_000.0..50_000.0),
            rng.gen_range(-30_000.0..30_000.0),
        );
        spawning::spawn_asteroid_field(
            &mut commands,
            center + asteroid_offset,
            rng.gen_range(15..30),
            rng.gen_range(20_000.0..40_000.0),
            new_id,
        );

        // Move ship to safe distance from the new star
        let safe_distance = system_info.star_entity
            .map(|_| 100_000.0) // Start far from star
            .unwrap_or(50_000.0);

        let arrival_angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let arrival_pos = center + Vec2::new(
            arrival_angle.cos() * safe_distance,
            arrival_angle.sin() * safe_distance,
        );

        sub_transform.translation.x = arrival_pos.x;
        sub_transform.translation.y = arrival_pos.y;
        sub_velocity.0 = Vec2::ZERO; // Kill momentum on arrival

        // Update galaxy state
        galaxy.systems.push(system_info);
        galaxy.current_system = new_id;
        galaxy.next_system_id = new_id + 1;

        completed_events.send(WarpJumpCompleted {
            system_id: new_id,
        });

        notifications.send(ShowNotification {
            message: format!("Warp complete! Arrived in System-{}", new_id),
            notification_type: NotificationType::Success,
            duration: 4.0,
        });
    }
}

/// When warp completes, notify the player about the new system
pub fn on_warp_complete(
    mut completed_events: EventReader<WarpJumpCompleted>,
    galaxy: Res<GalaxyState>,
    mut notifications: EventWriter<ShowNotification>,
) {
    for event in completed_events.iter() {
        if let Some(system) = galaxy.systems.iter().find(|s| s.id == event.system_id) {
            let planet_count = system.planet_entities.len();
            notifications.send(ShowNotification {
                message: format!("System scan: {} planets detected. Proceed with caution.", planet_count),
                notification_type: NotificationType::Info,
                duration: 5.0,
            });
        }
    }
}
