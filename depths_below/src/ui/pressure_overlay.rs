//! Where the air is, and where it is going.
//!
//! A flow field cannot be tuned by looking at it sideways, so this is as much
//! a development instrument as a player readout: tiles are tinted by pressure
//! and each one draws a short line the way its air is moving.
//!
//! Built alongside `damage_overlay`, and deliberately exclusive with it --
//! both tint the same blocks.

use bevy::prelude::*;

use crate::building::{grid_to_local, rooms::RoomMap, GRID_SIZE};
use crate::components::*;
use crate::events::*;
use crate::ship::air::AirField;

/// One tinted cell of the overlay, parented to the ship.
#[derive(Component)]
pub struct PressureTile(pub IVec2);

/// Marker for the legend UI node.
#[derive(Component)]
pub(crate) struct PressureOverlayLegend;

/// Pale blue where there is air, near-black where there is none.
fn pressure_color(pressure: f32) -> Color {
    let p = pressure.clamp(0.0, 1.0);
    Color::srgba(
        0.06 + 0.39 * p,
        0.07 + 0.65 * p,
        0.11 + 0.84 * p,
        0.42 - 0.12 * p,
    )
}

/// Q toggles the pressure overlay, turning the damage overlay off if it is up.
pub fn toggle_pressure_overlay(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    ship_query: Query<(Entity, Option<&PressureOverlayVisible>), With<Ship>>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    if !keyboard.just_pressed(KeyCode::KeyQ) {
        return;
    }
    let Ok((entity, overlay)) = ship_query.single() else { return };

    if overlay.is_some() {
        commands.entity(entity).remove::<PressureOverlayVisible>();
        notifications.write(ShowNotification {
            message: "Pressure overlay OFF".into(),
            notification_type: NotificationType::Info,
            duration: 1.0,
        });
    } else {
        commands
            .entity(entity)
            .insert(PressureOverlayVisible)
            .remove::<DamageOverlayVisible>();
        notifications.write(ShowNotification {
            message: "Pressure overlay ON - blue=pressurized, black=vacuum, lines show airflow".into(),
            notification_type: NotificationType::Info,
            duration: 2.5,
        });
    }
}

/// Keeps one tinted sprite per interior tile while the overlay is up.
pub fn update_pressure_overlay(
    mut commands: Commands,
    ship_query: Query<(Entity, Has<PressureOverlayVisible>), With<Ship>>,
    room_map: Res<RoomMap>,
    air: Res<AirField>,
    mut tiles: Query<(Entity, &PressureTile, &mut Sprite)>,
) {
    let Ok((ship, visible)) = ship_query.single() else { return };

    if !visible {
        for (entity, _, _) in tiles.iter() {
            commands.entity(entity).try_despawn();
        }
        return;
    }

    let mut seen: std::collections::HashSet<IVec2> = std::collections::HashSet::new();
    for (entity, tile, mut sprite) in tiles.iter_mut() {
        // A tile that was shot away takes its swatch with it.
        if !room_map.tile_to_room.contains_key(&tile.0) {
            commands.entity(entity).try_despawn();
            continue;
        }
        seen.insert(tile.0);
        sprite.color = pressure_color(air.pressure.get(&tile.0).copied().unwrap_or(1.0));
    }

    for &cell in room_map.tile_to_room.keys() {
        if seen.contains(&cell) {
            continue;
        }
        let pressure = air.pressure.get(&cell).copied().unwrap_or(1.0);
        let swatch = commands
            .spawn((
                Sprite {
                    color: pressure_color(pressure),
                    custom_size: Some(Vec2::splat(GRID_SIZE - 6.0)),
                    ..default()
                },
                // Under the crew (0.6) so people stay readable on top of it,
                // over the blocks it describes.
                Transform::from_translation(grid_to_local(cell).extend(0.45)),
                PressureTile(cell),
            ))
            .id();
        commands.entity(swatch).insert(ChildOf(ship));
    }
}

/// Seeds per tile, spread on a 3x3 sub-grid. One line per cell reads as a
/// field of unrelated darts; nine tells you where the air is going.
const SEEDS_PER_AXIS: i32 = 3;

/// Segments per streamline. The curve comes from re-sampling the field at each
/// step, so more steps means a longer, more sharply bent line.
const TRACE_STEPS: usize = 14;

/// Ship-local units per segment.
const STEP_LEN: f32 = 9.0;

/// Below this the air is not really moving and the line would be noise.
const FLOW_FLOOR: f32 = 0.03;

/// Air speed that draws at full brightness.
const FULL_BRIGHT: f32 = 1.2;

/// Flow at an arbitrary ship-local point, blended from the four tile centres
/// around it.
///
/// Sampling the containing tile alone gives every line inside one cell an
/// identical heading, and they come out as straight parallel darts that snap
/// direction at the tile border. Blending is what lets a line bend.
fn sample_flow(air: &AirField, local: Vec2) -> Vec2 {
    // Tile centres sit at grid_to_local; work in that space and take the
    // fractional position between the four nearest.
    let gx = local.x / GRID_SIZE;
    let gy = (local.y + 33.0) / GRID_SIZE;
    let (x0, y0) = (gx.floor(), gy.floor());
    let (fx, fy) = (gx - x0, gy - y0);

    let at = |dx: i32, dy: i32| -> Vec2 {
        let cell = IVec2::new(x0 as i32 + dx, y0 as i32 + dy);
        air.flow.get(&cell).copied().unwrap_or(Vec2::ZERO)
    };

    let bottom = at(0, 0) * (1.0 - fx) + at(1, 0) * fx;
    let top = at(0, 1) * (1.0 - fx) + at(1, 1) * fx;
    bottom * (1.0 - fy) + top * fy
}

/// Traces the airflow as curving streamlines, weather-map fashion.
///
/// Each line starts at a seed and walks the field one short step at a time,
/// re-reading the direction as it goes, so it bends around corners and bunches
/// where the air is being funnelled — which is the part that tells you a hole
/// is pulling from three compartments away. Brightness is air speed, so a
/// still room draws almost nothing and a vent draws a bright fan into it.
pub fn draw_pressure_flow(
    mut gizmos: Gizmos,
    ship_query: Query<(&GlobalTransform, Has<PressureOverlayVisible>), With<Ship>>,
    room_map: Res<RoomMap>,
    air: Res<AirField>,
) {
    let Ok((ship_gt, visible)) = ship_query.single() else { return };
    if !visible {
        return;
    }

    let to_world = |p: Vec2| ship_gt.transform_point(p.extend(0.5)).truncate();
    let spacing = GRID_SIZE / SEEDS_PER_AXIS as f32;

    for &cell in room_map.tile_to_room.keys() {
        let centre = grid_to_local(cell);
        for sy in 0..SEEDS_PER_AXIS {
            for sx in 0..SEEDS_PER_AXIS {
                // Offset from the tile's own corner so seeds tile evenly
                // across the ship rather than clustering at cell centres.
                let offset = Vec2::new(
                    (sx as f32 + 0.5) * spacing - GRID_SIZE * 0.5,
                    (sy as f32 + 0.5) * spacing - GRID_SIZE * 0.5,
                );
                let mut point = centre + offset;

                for step in 0..TRACE_STEPS {
                    let flow = sample_flow(&air, point);
                    let speed = flow.length();
                    if speed < FLOW_FLOOR {
                        break;
                    }
                    let next = point + (flow / speed) * STEP_LEN;

                    // Fade along the trail so the bright end is the one the
                    // air is heading towards — direction without arrowheads.
                    let along = step as f32 / TRACE_STEPS as f32;
                    let alpha =
                        (speed / FULL_BRIGHT).clamp(0.05, 1.0) * (0.25 + 0.75 * along) * 0.9;
                    gizmos.line_2d(
                        to_world(point),
                        to_world(next),
                        Color::srgba(1.0, 1.0, 1.0, alpha),
                    );
                    point = next;
                }
            }
        }
    }
}

/// Drops the overlay's sprites when the run ends.
pub fn cleanup_pressure_overlay_on_exit(
    mut commands: Commands,
    tiles: Query<Entity, With<PressureTile>>,
    legend: Query<Entity, With<PressureOverlayLegend>>,
) {
    for entity in tiles.iter().chain(legend.iter()) {
        commands.entity(entity).try_despawn();
    }
}
