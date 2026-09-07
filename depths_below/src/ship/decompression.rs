use bevy::prelude::*;
use crate::components::*;
use crate::resources::*;
use crate::events::*;
use crate::building::rooms::RoomMap;

/// Updates decompression on a per-room basis.
/// RoomDepressurized events mark rooms as breached, air escapes from breached rooms,
/// hull segments sync to their room's depressurization level via tile_to_room.
pub fn update_decompression(
    time: Res<Time>,
    ship_query: Query<Entity, With<Ship>>,
    mut hull_query: Query<(&mut HullSegment, &Transform, &ChildOf)>,
    room_map: Res<RoomMap>,
    mut oxygen_state: ResMut<OxygenState>,
    mut player_body: Query<(&GlobalTransform, &mut Velocity, &mut ShipPhysics), With<Ship>>,
) {
    // Player ship only: hull_query spans every ship in the world (this
    // component is shared with AI ships). Unscoped, an AI ship whose local
    // grid_position happened to numerically match one of the player's
    // breached tiles got flagged depressurized too, and its
    // depressurization_level was then summed into the player's oxygen
    // drain below — the player could lose all their air from damage
    // happening on a ship they've never even seen.
    let Ok(player_ship) = ship_query.single() else { return };

    // Air level and is_breached are owned by ship::air, which runs just
    // before this and derives both from the per-tile pressure field. What is
    // left here is everything downstream of them: the hull sync, the oxygen
    // drain, and vent thrust.
    let dt = time.delta_secs();

    // Build a lookup: tile -> air_level from rooms
    let mut tile_air: std::collections::HashMap<IVec2, f32> = std::collections::HashMap::new();
    for room in room_map.rooms.iter() {
        if room.air_level < 1.0 {
            for tile in &room.tiles {
                tile_air.insert(*tile, room.air_level);
            }
        }
    }

    // Sync hull segments with their room's depressurization level
    let mut total_air_loss = 0.0;
    for (mut hull, _transform, parent) in hull_query.iter_mut() {
        if parent.parent() != player_ship { continue; }
        if let Some(&air) = tile_air.get(&hull.grid_position) {
            hull.depressurization_level = 1.0 - air;  // 0 air = fully depressurized
            hull.is_depressurized = air < 1.0;
        }
        // Also progress existing hull-level decompression (segments breached directly)
        if hull.is_depressurized && hull.depressurization_level < 1.0 && !tile_air.contains_key(&hull.grid_position) {
            hull.depressurization_level = (hull.depressurization_level + 0.15 * dt).min(1.0);
        }
        total_air_loss += hull.depressurization_level;
    }

    // Decompression drains oxygen — air escaping into the void
    for room in room_map.rooms.iter() {
        if room.is_breached {
            total_air_loss += (1.0 - room.air_level) * room.tiles.len() as f32;
        }
    }

    // Each unit of air loss drains oxygen at 3.0 per second — breach without bulkheads = crew dies
    let oxygen_drain = total_air_loss * 3.0 * dt;
    oxygen_state.current_oxygen = (oxygen_state.current_oxygen - oxygen_drain).max(0.0);

    // VENT THRUST — air jetting out of a breach is reaction mass. While a
    // room is actively draining, each breached hull tile bordering it pushes
    // the ship away from the hole (jet goes out, ship goes the other way)
    // and, being off-center, slowly yaws it. Subtle by design: a full 7s
    // vent adds roughly a hundred u/s of drift, not a spin-out — but it
    // makes WHERE you got holed matter.
    let mut vent_accel_local = Vec2::ZERO;
    let mut vent_torque = 0.0_f32;
    let mut vent_tiles = 0u32;
    for (hull, _t, parent) in hull_query.iter() {
        if parent.parent() != player_ship { continue; }
        if !hull.is_depressurized { continue; }
        // Air still escaping from this hole? Either an adjacent detected
        // room is actively draining, or the segment's own direct
        // depressurization is still in progress (ships without enclosed
        // rooms never populate tile_to_room, which made venting silently
        // impossible on open layouts).
        let room_draining = [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y].iter().any(|off| {
            room_map.tile_to_room.get(&(hull.grid_position + *off))
                .and_then(|id| room_map.rooms.get(*id))
                .map(|room| room.is_breached && room.air_level > 0.0)
                .unwrap_or(false)
        });
        let segment_draining = hull.depressurization_level < 1.0;
        if !room_draining && !segment_draining { continue; }
        vent_tiles += 1;

        // Ship-local tile offset from the ship origin (grid_y*66-33 layout)
        let tile_local = Vec2::new(
            hull.grid_position.x as f32 * 66.0,
            hull.grid_position.y as f32 * 66.0 - 33.0,
        );
        if let Some(inward) = (-tile_local).try_normalize() {
            vent_accel_local += inward;
            // Off-center hole = slight yaw (2D cross of position × force)
            vent_torque += tile_local.x * inward.y - tile_local.y * inward.x;
        }
    }

    if vent_tiles > 0 {
        if let Ok((gt, mut velocity, mut physics)) = player_body.single_mut() {
            // Per-tile force flattens out past a few holes — a colander
            // doesn't vent harder than a puncture, it just empties faster.
            let strength = 45.0 * (vent_tiles as f32).sqrt() / vent_tiles as f32;
            let world_accel = gt.rotation() * (vent_accel_local * strength).extend(0.0);
            velocity.0 += world_accel.truncate() * dt;
            physics.angular_velocity += (vent_torque * 0.00008).clamp(-0.25, 0.25) * dt;

            // Throttled trace so playtests can confirm venting actually
            // fires (it silently never triggered on room-less layouts).
            if (time.elapsed_secs() % 1.0) < dt {
                info!("[VENT] {} hole(s) venting, accel {:.1} u/s²", vent_tiles, (vent_accel_local * strength).length());
            }
        }
    }
}

/// Crew in Repairing state seal breaches in their room.
/// RepairBay modules in the same room multiply seal power.
/// HullSeal modules provide automated breach sealing independent of crew.
pub fn seal_breach_system(
    time: Res<Time>,
    crew_query: Query<(&CrewMember, &CrewRoomLocation)>,
    repair_bays: Query<(&Module, &RepairSystem), Without<DestroyedModule>>,
    hull_seals: Query<(&HullSealComp, &Module), Without<DestroyedModule>>,
    room_map: Res<RoomMap>,
    mut air: ResMut<crate::ship::air::AirField>,
) {
    let dt = time.delta_secs();

    // Build per-room seal power from repairing crew
    let mut room_seal_power: std::collections::HashMap<usize, f32> = std::collections::HashMap::new();
    for (crew, location) in crew_query.iter() {
        if crew.state != CrewState::Repairing || crew.health <= 0.0 {
            continue;
        }
        if let Some(room_id) = location.room_id {
            *room_seal_power.entry(room_id).or_insert(0.0) += 0.03;
        }
    }

    // Automated HullSeal modules add seal power to their room
    for (seal, module) in hull_seals.iter() {
        if !module.is_active { continue; }
        if let Some(&room_id) = room_map.tile_to_room.get(&module.grid_position) {
            *room_seal_power.entry(room_id).or_insert(0.0) += seal.seal_rate;
        }
    }

    if room_seal_power.is_empty() {
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

    // Apply sealing to each room, restoring air to its tiles. This has to
    // write the pressure field rather than room.air_level: air::sync_room_air
    // republishes air_level from the tiles every frame, so a write here would
    // be gone before anything read it.
    for (room_id, seal_power) in room_seal_power.iter() {
        if let Some(room) = room_map.rooms.get(*room_id) {
            let boost = room_repair_boost.get(room_id).copied().unwrap_or(1.0);
            let restore = seal_power * boost * dt;
            for tile in &room.tiles {
                let pressure = air.pressure.entry(*tile).or_insert(1.0);
                *pressure = (*pressure + restore).min(1.0);
            }
        }
    }
}

/// Reads ToggleBulkhead events and seals/unseals bulkhead doors.
pub fn handle_bulkhead_toggle(
    mut commands: Commands,
    mut events: MessageReader<ToggleBulkhead>,
    mut hull_query: Query<(&HullSegment, &mut Sprite)>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for event in events.read() {
        let Ok((hull, mut sprite)) = hull_query.get_mut(event.segment) else {
            continue;
        };
        if hull.hull_layer != HullLayer::BulkheadDoor {
            continue;
        }
        if event.seal {
            commands.entity(event.segment).insert(BulkheadSealed);
            sprite.color = Color::srgb(0.8, 0.2, 0.2); // Red = sealed
            notifications.write(ShowNotification {
                message: "Bulkhead sealed — section airtight!".into(),
                notification_type: NotificationType::Warning,
                duration: 2.0,
            });
        } else {
            commands.entity(event.segment).remove::<BulkheadSealed>();
            sprite.color = Color::srgb(0.9, 0.8, 0.7); // Default bulkhead color
            notifications.write(ShowNotification {
                message: "Bulkhead opened!".into(),
                notification_type: NotificationType::Info,
                duration: 2.0,
            });
        }
    }
}

/// F key + left click = toggle nearest bulkhead door within 50 world units.
pub fn bulkhead_seal_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    hull_query: Query<(Entity, &HullSegment, &Transform, Option<&BulkheadSealed>)>,
    mut toggle_events: MessageWriter<ToggleBulkhead>,
) {
    if !keyboard.pressed(KeyCode::KeyF) || !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Ok((camera, camera_transform)) = camera_query.single() else { return };

    let Some(cursor_world) = window.cursor_position()
        .and_then(|p| camera.viewport_to_world_2d(camera_transform, p).ok())
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
        toggle_events.write(ToggleBulkhead {
            segment: entity,
            seal: !is_sealed,
        });
    }
}
