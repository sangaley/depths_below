use bevy::prelude::*;
use crate::components::*;

// ============================================================================
// FIRE GROUP SYSTEM
// 4 groups assigned in build mode. Keys 1-4 fire each group.
// Hold for sustained fire. Unassigned weapons default to group 1.
// ============================================================================

/// Component on weapon modules — which fire group they belong to
#[derive(Component, Default)]
pub struct FireGroup {
    pub group: u8, // 0-3 (displayed as 1-4)
}

/// Component on weapon modules — whether this weapon is in intercept mode
#[derive(Component)]
pub struct InterceptMode;

/// Component on weapon modules — whether this weapon auto-fires at selected target
#[derive(Component)]
pub struct AutoFireMode;

/// Resource tracking which fire groups are currently firing
#[derive(Resource, Default)]
pub struct FireGroupState {
    pub firing: [bool; 4],
}

/// System: read fire inputs, set fire group state.
/// Space or left-click fires everything (matching the "Space: Fire" HUD
/// hint); 1-4 fire individual groups for players who assign them.
pub fn fire_group_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<FireGroupState>,
) {
    let fire_all = keyboard.pressed(KeyCode::Space) || mouse.pressed(MouseButton::Left);
    state.firing[0] = keyboard.pressed(KeyCode::Digit1) || fire_all;
    state.firing[1] = keyboard.pressed(KeyCode::Digit2) || fire_all;
    state.firing[2] = keyboard.pressed(KeyCode::Digit3) || fire_all;
    state.firing[3] = keyboard.pressed(KeyCode::Digit4) || fire_all;
}

/// System: assign fire groups during build mode with Ctrl+1-4
pub fn assign_fire_group(
    keyboard: Res<ButtonInput<KeyCode>>,
    _mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    occupancy: Res<crate::building::GridOccupancy>,
    mut weapon_query: Query<(Entity, &Module, &mut FireGroup), With<Weapon>>,
    mut notifications: MessageWriter<crate::events::ShowNotification>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !ctrl { return; }

    let group = if keyboard.just_pressed(KeyCode::Digit1) { Some(0) }
        else if keyboard.just_pressed(KeyCode::Digit2) { Some(1) }
        else if keyboard.just_pressed(KeyCode::Digit3) { Some(2) }
        else if keyboard.just_pressed(KeyCode::Digit4) { Some(3) }
        else { None };

    let Some(group) = group else { return };

    // Find weapon under cursor
    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_transform)) = camera_query.single() else { return };
    let Some(cursor) = window.cursor_position()
        .and_then(|p| camera.viewport_to_world_2d(cam_transform, p).ok())
    else { return };

    let grid_pos = IVec2::new(
        (cursor.x / 66.0).round() as i32,
        ((cursor.y + 33.0) / 66.0).round() as i32,
    );

    if let Some(&entity) = occupancy.cells.get(&grid_pos) {
        if let Ok((_, module, mut fire_group)) = weapon_query.get_mut(entity) {
            fire_group.group = group;
            notifications.write(crate::events::ShowNotification {
                message: format!("{} assigned to Fire Group {}", module.module_type.name(), group + 1),
                notification_type: crate::events::NotificationType::Info,
                duration: 2.0,
            });
        }
    }
}
