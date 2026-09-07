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

/// Draws each tile's flow as a short line, so direction is visible and not
/// just inferred from which cells went dark first.
pub fn draw_pressure_flow(
    mut gizmos: Gizmos,
    ship_query: Query<(&GlobalTransform, Has<PressureOverlayVisible>), With<Ship>>,
    air: Res<AirField>,
) {
    let Ok((ship_gt, visible)) = ship_query.single() else { return };
    if !visible {
        return;
    }

    for (&cell, &flow) in air.flow.iter() {
        let strength = flow.length();
        if strength < 0.02 {
            continue;
        }
        // Clamped so a violent vent stays inside its own cell rather than
        // drawing a spear across the ship.
        let length = (strength * 26.0).min(GRID_SIZE * 0.45);
        let local = grid_to_local(cell);
        let tip = local + flow.normalize_or_zero() * length;
        let from = ship_gt.transform_point(local.extend(0.5)).truncate();
        let to = ship_gt.transform_point(tip.extend(0.5)).truncate();
        gizmos.line_2d(from, to, Color::srgba(0.75, 0.9, 1.0, 0.85));
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
