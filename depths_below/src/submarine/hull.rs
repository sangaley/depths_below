use bevy::prelude::*;
use crate::components::*;
use crate::resources::*;
use crate::events::*;
use crate::building::rooms::RoomMap;

/// Updates hull integrity tracking
pub fn update_hull_integrity(
    hull_query: Query<&HullSegment>,
    mut hull_state: ResMut<HullState>,
) {
    let mut total_health = 0.0;
    let mut max_health = 0.0;
    let mut min_depth_rating = f32::MAX;

    for hull in hull_query.iter() {
        total_health += hull.health;
        max_health += hull.max_health;

        if hull.depth_rating < min_depth_rating {
            min_depth_rating = hull.depth_rating;
        }
    }

    if max_health > 0.0 {
        hull_state.hull_integrity = total_health / max_health;
    }
    hull_state.max_depth_rating = if min_depth_rating == f32::MAX {
        200.0
    } else {
        min_depth_rating
    };
}

/// When a hull segment is destroyed (0 HP), apply 15% max_health stress damage to adjacent hull.
/// This can cascade through already-weakened segments but won't one-shot healthy hull.
pub fn process_hull_cascade(
    mut commands: Commands,
    mut hull_destroy_events: EventReader<HullSegmentDestroyed>,
    mut hull_query: Query<(Entity, &mut HullSegment), Without<HullDestroyed>>,
    mut breach_events: EventWriter<HullBreached>,
    room_map: Res<RoomMap>,
    mut room_flood_events: EventWriter<RoomFlooded>,
    mut notifications: EventWriter<ShowNotification>,
) {
    let mut destroyed_positions: Vec<IVec2> = Vec::new();

    for event in hull_destroy_events.iter() {
        // Mark the destroyed hull segment
        commands.entity(event.segment).insert(HullDestroyed);
        destroyed_positions.push(event.grid_position);
    }

    // Apply stress damage to adjacent hull segments
    for destroyed_pos in &destroyed_positions {
        for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
            let adj_pos = *destroyed_pos + offset;

            for (hull_entity, mut hull) in hull_query.iter_mut() {
                if hull.grid_position != adj_pos {
                    continue;
                }

                // 15% of max_health as stress damage
                let stress_damage = hull.max_health * 0.15;
                hull.health = (hull.health - stress_damage).max(0.0);

                let health_pct = if hull.max_health > 0.0 {
                    hull.health / hull.max_health
                } else {
                    0.0
                };

                // Breach if newly below 30%
                if health_pct < 0.3 && !hull.is_flooded {
                    hull.is_flooded = true;
                    breach_events.send(HullBreached {
                        segment: hull_entity,
                        severity: 1.0 - health_pct,
                    });
                    if let Some(&room_id) = room_map.tile_to_room.get(&adj_pos) {
                        room_flood_events.send(RoomFlooded {
                            room_id,
                            severity: 1.0 - health_pct,
                        });
                    }
                    notifications.send(ShowNotification {
                        message: "Hull cascade! Adjacent segment weakened!".into(),
                        notification_type: NotificationType::Warning,
                        duration: 3.0,
                    });
                }
            }
        }
    }
}
