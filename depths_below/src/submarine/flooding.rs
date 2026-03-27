use bevy::prelude::*;
use crate::components::*;
use crate::resources::*;
use crate::events::*;
use crate::building::rooms::RoomMap;

/// Updates flooding on a per-room basis.
/// RoomFlooded events mark rooms as breached, water progresses in breached rooms,
/// hull segments sync to their room's water level via tile_to_room.
pub fn update_flooding(
    time: Res<Time>,
    mut hull_query: Query<(&mut HullSegment, &Transform)>,
    mut room_map: ResMut<RoomMap>,
    mut hull_state: ResMut<HullState>,
    mut room_flood_events: EventReader<RoomFlooded>,
) {
    // Read RoomFlooded events → mark rooms as breached
    for event in room_flood_events.iter() {
        if let Some(room) = room_map.rooms.get_mut(event.room_id) {
            room.is_breached = true;
        }
    }

    // Progress water_level in breached rooms
    let dt = time.delta_seconds();
    for room in room_map.rooms.iter_mut() {
        if room.is_breached && room.water_level < 1.0 {
            room.water_level = (room.water_level + 0.1 * dt).min(1.0);
        }
    }

    // Build a lookup: tile -> water_level from rooms
    let mut tile_water: std::collections::HashMap<IVec2, f32> = std::collections::HashMap::new();
    for room in room_map.rooms.iter() {
        if room.water_level > 0.0 {
            for tile in &room.tiles {
                tile_water.insert(*tile, room.water_level);
            }
        }
    }

    // Sync hull segments with their room's water level
    let mut flood_weight = 0.0;
    for (mut hull, _transform) in hull_query.iter_mut() {
        if let Some(&water) = tile_water.get(&hull.grid_position) {
            hull.flood_level = water;
            hull.is_flooded = water > 0.0;
        }
        // Also progress existing hull-level floods (segments breached directly)
        if hull.is_flooded && hull.flood_level < 1.0 && !tile_water.contains_key(&hull.grid_position) {
            hull.flood_level = (hull.flood_level + 0.1 * dt).min(1.0);
        }
        flood_weight += hull.flood_level * 500.0;
    }

    // Add flood weight from room water levels
    for room in room_map.rooms.iter() {
        flood_weight += room.water_level * room.tiles.len() as f32 * 200.0;
    }

    hull_state.total_weight = 5000.0 + flood_weight;
}

/// Crew in Repairing state pump water from their room.
/// RepairBay modules in the same room multiply pump power.
/// WaterPump modules provide automated pumping independent of crew.
pub fn pump_water_system(
    time: Res<Time>,
    crew_query: Query<(&CrewMember, &CrewRoomLocation)>,
    repair_bays: Query<(&Module, &RepairSystem), Without<DestroyedModule>>,
    water_pumps: Query<(&WaterPumpComp, &Module), Without<DestroyedModule>>,
    mut room_map: ResMut<RoomMap>,
) {
    let dt = time.delta_seconds();

    // Build per-room pump power from repairing crew
    let mut room_pump_power: std::collections::HashMap<usize, f32> = std::collections::HashMap::new();
    for (crew, location) in crew_query.iter() {
        if crew.state != CrewState::Repairing || crew.health <= 0.0 {
            continue;
        }
        if let Some(room_id) = location.room_id {
            *room_pump_power.entry(room_id).or_insert(0.0) += 0.03;
        }
    }

    // Automated WaterPump modules add pump power to their room
    for (pump, module) in water_pumps.iter() {
        if !module.is_active { continue; }
        if let Some(&room_id) = room_map.tile_to_room.get(&module.grid_position) {
            *room_pump_power.entry(room_id).or_insert(0.0) += pump.pump_rate;
        }
    }

    if room_pump_power.is_empty() {
        return;
    }

    // Build per-room RepairBay boost
    let mut room_repair_boost: std::collections::HashMap<usize, f32> = std::collections::HashMap::new();
    for (module, repair_sys) in repair_bays.iter() {
        if let Some(&room_id) = room_map.tile_to_room.get(&module.grid_position) {
            let boost = room_repair_boost.entry(room_id).or_insert(1.0);
            *boost += repair_sys.repair_rate * 0.1;
        }
    }

    // Apply pumping to each room
    for (room_id, pump_power) in room_pump_power.iter() {
        if let Some(room) = room_map.rooms.get_mut(*room_id) {
            if room.water_level <= 0.0 {
                continue;
            }
            let boost = room_repair_boost.get(room_id).copied().unwrap_or(1.0);
            let drain = pump_power * boost * dt;
            room.water_level = (room.water_level - drain).max(0.0);
            if room.water_level <= 0.0 {
                room.is_breached = false;
            }
        }
    }
}

/// Reads ToggleBulkhead events and seals/unseals bulkhead doors.
pub fn handle_bulkhead_toggle(
    mut commands: Commands,
    mut events: EventReader<ToggleBulkhead>,
    mut hull_query: Query<(&HullSegment, &mut Sprite)>,
    mut notifications: EventWriter<ShowNotification>,
) {
    for event in events.iter() {
        let Ok((hull, mut sprite)) = hull_query.get_mut(event.segment) else {
            continue;
        };
        if hull.hull_layer != HullLayer::BulkheadDoor {
            continue;
        }
        if event.seal {
            commands.entity(event.segment).insert(BulkheadSealed);
            sprite.color = Color::rgb(0.8, 0.2, 0.2); // Red = sealed
            notifications.send(ShowNotification {
                message: "Bulkhead sealed!".into(),
                notification_type: NotificationType::Warning,
                duration: 2.0,
            });
        } else {
            commands.entity(event.segment).remove::<BulkheadSealed>();
            sprite.color = Color::rgb(0.9, 0.8, 0.7); // Default bulkhead color
            notifications.send(ShowNotification {
                message: "Bulkhead opened!".into(),
                notification_type: NotificationType::Info,
                duration: 2.0,
            });
        }
    }
}

/// F key + left click = toggle nearest bulkhead door within 50 world units.
pub fn bulkhead_seal_input(
    keyboard: Res<Input<KeyCode>>,
    mouse: Res<Input<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    hull_query: Query<(Entity, &HullSegment, &Transform, Option<&BulkheadSealed>)>,
    mut toggle_events: EventWriter<ToggleBulkhead>,
) {
    if !keyboard.pressed(KeyCode::F) || !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(window) = windows.get_single() else { return };
    let Ok((camera, camera_transform)) = camera_query.get_single() else { return };

    let Some(cursor_world) = window.cursor_position()
        .and_then(|p| camera.viewport_to_world_2d(camera_transform, p))
    else {
        return;
    };

    // Find nearest bulkhead door within 50 world units
    let mut nearest: Option<(Entity, f32, bool)> = None;
    for (entity, hull, transform, sealed) in hull_query.iter() {
        if hull.hull_layer != HullLayer::BulkheadDoor {
            continue;
        }
        let dist = transform.translation.truncate().distance(cursor_world);
        if dist < 50.0 {
            if nearest.map_or(true, |(_, d, _)| dist < d) {
                nearest = Some((entity, dist, sealed.is_some()));
            }
        }
    }

    if let Some((entity, _, is_sealed)) = nearest {
        toggle_events.send(ToggleBulkhead {
            segment: entity,
            seal: !is_sealed,
        });
    }
}
