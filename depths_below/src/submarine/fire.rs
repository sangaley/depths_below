use bevy::prelude::*;
use rand::Rng;
use crate::components::*;
use crate::events::*;
use crate::building::GridOccupancy;
use crate::building::rooms::RoomMap;

/// Reads FireStarted events and ignites modules that aren't already on fire.
pub fn apply_fire_ignition(
    mut commands: Commands,
    mut fire_events: EventReader<FireStarted>,
    module_query: Query<(Entity, &Module), (Without<DestroyedModule>, Without<OnFire>)>,
    room_map: Res<RoomMap>,
    mut notifications: EventWriter<ShowNotification>,
) {
    for event in fire_events.iter() {
        // Check if module exists, is not destroyed, and not already on fire
        let Ok((entity, module)) = module_query.get(event.module) else {
            continue;
        };

        // Can't ignite in heavily flooded rooms
        if let Some(&room_id) = room_map.tile_to_room.get(&event.grid_position) {
            if let Some(room) = room_map.rooms.get(room_id) {
                if room.water_level > 0.3 {
                    continue;
                }
            }
        }

        let intensity = event.intensity.clamp(0.05, 1.0);
        commands.entity(entity).insert(OnFire {
            intensity,
            damage_per_second: 8.0 * intensity,
            spread_timer: Timer::from_seconds(3.0, TimerMode::Repeating),
            duration: Timer::from_seconds(15.0, TimerMode::Once),
        });

        notifications.send(ShowNotification {
            message: format!("Fire in {}!", module.module_type.name()),
            notification_type: NotificationType::Danger,
            duration: 3.0,
        });
    }
}

/// Updates all burning modules: flood suppression, burnout, DoT, visual tint, and spread.
pub fn update_fire(
    mut commands: Commands,
    time: Res<Time>,
    mut fire_query: Query<(Entity, &mut OnFire, &mut Module, &mut Sprite), Without<DestroyedModule>>,
    room_map: Res<RoomMap>,
    occupancy: Res<GridOccupancy>,
    alive_modules: Query<Entity, (With<Module>, Without<DestroyedModule>, Without<OnFire>)>,
    sealed_query: Query<(&HullSegment, &Transform), With<BulkheadSealed>>,
    firebreak_query: Query<&Module, (With<FirebreakMarker>, Without<DestroyedModule>, Without<OnFire>)>,
    mut fire_events: EventWriter<FireStarted>,
    mut extinguish_events: EventWriter<FireExtinguished>,
) {
    let dt = time.delta_seconds();
    let mut rng = rand::thread_rng();

    // Build set of sealed bulkhead positions to block fire spread
    let mut sealed_positions: std::collections::HashSet<IVec2> = sealed_query
        .iter()
        .map(|(_, transform)| crate::building::rooms::transform_to_grid(transform))
        .collect();

    // FirebreakWall always blocks fire spread (no seal needed)
    for fb_module in firebreak_query.iter() {
        sealed_positions.insert(fb_module.grid_position);
    }

    for (entity, mut fire, mut module, mut sprite) in fire_query.iter_mut() {
        // Flood suppression
        if let Some(&room_id) = room_map.tile_to_room.get(&module.grid_position) {
            if let Some(room) = room_map.rooms.get(room_id) {
                if room.water_level > 0.5 {
                    // Fully extinguish
                    commands.entity(entity).remove::<OnFire>();
                    sprite.color = Color::rgb(0.2, 0.2, 0.2); // Burnt/destroyed tint
                    extinguish_events.send(FireExtinguished {
                        module: entity,
                        cause: FireExtinguishCause::Flooding,
                    });
                    continue;
                } else if room.water_level > 0.1 {
                    // Reduce intensity based on water
                    fire.intensity = (fire.intensity - room.water_level * 0.5 * dt).max(0.0);
                    fire.damage_per_second = 8.0 * fire.intensity;
                }
            }
        }

        // Tick timers
        fire.duration.tick(time.delta());
        fire.spread_timer.tick(time.delta());

        // Burnout check
        if fire.duration.finished() || fire.intensity < 0.05 {
            commands.entity(entity).remove::<OnFire>();
            extinguish_events.send(FireExtinguished {
                module: entity,
                cause: FireExtinguishCause::BurnedOut,
            });
            continue;
        }

        // DoT
        module.health -= fire.damage_per_second * dt;
        if module.health < 0.0 {
            module.health = 0.0;
        }

        // Visual: tint sprite orange based on intensity
        let r = 0.2 + fire.intensity * 0.8; // 0.2 (no fire) to 1.0 (full fire)
        let g = 0.2 + fire.intensity * 0.3; // slight orange
        let b = 0.2 * (1.0 - fire.intensity);
        sprite.color = Color::rgb(r, g, b);

        // Spread on timer tick
        if fire.spread_timer.just_finished() {
            let spread_chance = 0.3 * fire.intensity;
            for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                let adj_pos = module.grid_position + offset;
                // Block fire spread across sealed bulkheads
                if sealed_positions.contains(&adj_pos) {
                    continue;
                }
                if rng.gen::<f32>() < spread_chance {
                    if let Some(&adj_entity) = occupancy.cells.get(&adj_pos) {
                        // Only spread to alive, non-burning modules
                        if alive_modules.get(adj_entity).is_ok() {
                            fire_events.send(FireStarted {
                                module: adj_entity,
                                grid_position: adj_pos,
                                intensity: fire.intensity * 0.7,
                            });
                        }
                    }
                }
            }
        }
    }
}

/// Emergency bulkheads auto-seal when adjacent rooms flood and unseal when water drains.
pub fn emergency_bulkhead_system(
    mut commands: Commands,
    bulkhead_query: Query<(Entity, &Module, Option<&BulkheadSealed>), Without<DestroyedModule>>,
    room_map: Res<RoomMap>,
    mut notifications: EventWriter<ShowNotification>,
) {
    for (entity, module, sealed) in bulkhead_query.iter() {
        if module.module_type != ModuleType::EmergencyBulkhead { continue; }
        if !module.is_active { continue; }

        // Check adjacent tiles for flooding
        let mut adjacent_flooded = false;
        for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
            let adj_pos = module.grid_position + offset;
            if let Some(&room_id) = room_map.tile_to_room.get(&adj_pos) {
                if let Some(room) = room_map.rooms.get(room_id) {
                    if room.water_level > 0.3 {
                        adjacent_flooded = true;
                        break;
                    }
                }
            }
        }

        if adjacent_flooded && sealed.is_none() {
            commands.entity(entity).insert(BulkheadSealed);
            notifications.send(ShowNotification {
                message: "Emergency bulkhead auto-sealed!".into(),
                notification_type: NotificationType::Warning,
                duration: 3.0,
            });
        } else if !adjacent_flooded && sealed.is_some() {
            commands.entity(entity).remove::<BulkheadSealed>();
        }
    }
}
