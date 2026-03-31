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

/// System: read 1-4 keys, set fire group state
pub fn fire_group_input(
    keyboard: Res<Input<KeyCode>>,
    mut state: ResMut<FireGroupState>,
) {
    state.firing[0] = keyboard.pressed(KeyCode::Key1);
    state.firing[1] = keyboard.pressed(KeyCode::Key2);
    state.firing[2] = keyboard.pressed(KeyCode::Key3);
    state.firing[3] = keyboard.pressed(KeyCode::Key4);
}

/// System: assign fire groups during build mode with Ctrl+1-4
pub fn assign_fire_group(
    keyboard: Res<Input<KeyCode>>,
    mouse: Res<Input<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    occupancy: Res<crate::building::GridOccupancy>,
    mut weapon_query: Query<(Entity, &Module, &mut FireGroup), With<Weapon>>,
    mut notifications: EventWriter<crate::events::ShowNotification>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !ctrl { return; }

    let group = if keyboard.just_pressed(KeyCode::Key1) { Some(0) }
        else if keyboard.just_pressed(KeyCode::Key2) { Some(1) }
        else if keyboard.just_pressed(KeyCode::Key3) { Some(2) }
        else if keyboard.just_pressed(KeyCode::Key4) { Some(3) }
        else { None };

    let Some(group) = group else { return };

    // Find weapon under cursor
    let Ok(window) = windows.get_single() else { return };
    let Ok((camera, cam_transform)) = camera_query.get_single() else { return };
    let Some(cursor) = window.cursor_position()
        .and_then(|p| camera.viewport_to_world_2d(cam_transform, p))
    else { return };

    let grid_pos = IVec2::new(
        (cursor.x / 66.0).round() as i32,
        ((cursor.y + 33.0) / 66.0).round() as i32,
    );

    if let Some(&entity) = occupancy.cells.get(&grid_pos) {
        if let Ok((_, module, mut fire_group)) = weapon_query.get_mut(entity) {
            fire_group.group = group;
            notifications.send(crate::events::ShowNotification {
                message: format!("{} assigned to Fire Group {}", module.module_type.name(), group + 1),
                notification_type: crate::events::NotificationType::Info,
                duration: 2.0,
            });
        }
    }
}
