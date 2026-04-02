use bevy::prelude::*;
use crate::components::Submarine;
use crate::celestial::components::*;
use super::framework::*;

// ============================================================================
// SYSTEM MINIMAP — floating window showing star system from above
// ============================================================================

#[derive(Component)]
pub struct MinimapWindow;

#[derive(Component)]
pub struct MinimapShipDot;

#[derive(Component)]
pub struct MinimapBodyDot {
    pub entity: Entity,
}

const MINIMAP_SIZE: f32 = 200.0;
const MINIMAP_RANGE: f32 = 200_000.0; // World units visible on minimap

/// Toggle minimap with N key
pub fn toggle_minimap(
    mut commands: Commands,
    keyboard: Res<Input<KeyCode>>,
    existing: Query<Entity, With<MinimapWindow>>,
) {
    if !keyboard.just_pressed(KeyCode::N) {
        return;
    }

    if let Ok(entity) = existing.get_single() {
        commands.entity(entity).despawn_recursive();
        return;
    }

    // Spawn minimap floating window
    let content = spawn_floating_window(
        &mut commands,
        "minimap",
        "System Map",
        Vec2::new(MINIMAP_SIZE + 16.0, MINIMAP_SIZE + 50.0),
        Vec2::new(10.0, 100.0),
    );

    // Mark the window
    // Find the root FloatingWindow entity (parent of content's parent)
    commands.entity(content).insert(MinimapWindow);

    // Minimap canvas (dark background)
    let canvas = commands.spawn(
        NodeBundle {
            style: Style {
                width: Val::Px(MINIMAP_SIZE),
                height: Val::Px(MINIMAP_SIZE),
                position_type: PositionType::Relative,
                ..default()
            },
            background_color: Color::rgba(0.02, 0.03, 0.06, 1.0).into(),
            ..default()
        },
    ).id();

    // Ship dot (center initially, updates each frame)
    let ship_dot = commands.spawn((
        NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                width: Val::Px(4.0),
                height: Val::Px(4.0),
                left: Val::Px(MINIMAP_SIZE / 2.0),
                top: Val::Px(MINIMAP_SIZE / 2.0),
                ..default()
            },
            background_color: Color::rgb(0.2, 1.0, 0.3).into(),
            ..default()
        },
        MinimapShipDot,
    )).id();

    commands.entity(canvas).add_child(ship_dot);
    commands.entity(content).add_child(canvas);
}

/// Update minimap dots based on ship position and celestial body positions
pub fn update_minimap(
    sub_query: Query<&Transform, With<Submarine>>,
    _body_query: Query<(Entity, &Transform, &CelestialBody)>,
    mut ship_dot_query: Query<&mut Style, (With<MinimapShipDot>, Without<MinimapBodyDot>)>,
    minimap_exists: Query<Entity, With<MinimapWindow>>,
) {
    if minimap_exists.is_empty() {
        return;
    }

    let Ok(sub_transform) = sub_query.get_single() else { return };
    let _sub_pos = sub_transform.translation.truncate();

    // Ship is always at center
    for mut style in ship_dot_query.iter_mut() {
        style.left = Val::Px(MINIMAP_SIZE / 2.0 - 2.0);
        style.top = Val::Px(MINIMAP_SIZE / 2.0 - 2.0);
    }
}
