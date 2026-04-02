use bevy::prelude::*;
use crate::components::*;
use crate::resources::*;
use crate::events::*;
use crate::states::BuildState;
use crate::building::{GridOccupancy, ModuleRegistry};
use crate::building::multiblock::components::*;

// ============================================================================
// COPY/PASTE SELECTION SYSTEM
// Ctrl+click to select modules. Ctrl+C copies. Ctrl+V pastes.
// Handles multi-block weapon chains as a unit.
// ============================================================================

/// A copied module definition
#[derive(Clone, Debug)]
pub struct ClipboardEntry {
    pub module_type: ModuleType,
    pub offset: IVec2,  // Relative to selection origin
    pub rotation: Rotation,
    pub has_machine_block: bool,
    pub machine_role: Option<BlockRole>,
}

/// Resource tracking clipboard and selection
#[derive(Resource, Default)]
pub struct BuildClipboard {
    /// Currently selected module entities
    pub selected: Vec<Entity>,
    /// Copied module data (ready to paste)
    pub copied: Vec<ClipboardEntry>,
    /// Whether we're in paste mode
    pub paste_mode: bool,
    /// Origin point of the copy (for offset calculation)
    pub copy_origin: IVec2,
}

/// Marker for selected module visual highlight
#[derive(Component)]
pub struct SelectionHighlight;

/// System: Ctrl+click to toggle select modules, Ctrl+C to copy, Ctrl+V to paste
pub fn clipboard_input(
    keyboard: Res<Input<KeyCode>>,
    mouse: Res<Input<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    mut clipboard: ResMut<BuildClipboard>,
    occupancy: Res<GridOccupancy>,
    module_query: Query<(Entity, &Module, Option<&MachineBlock>)>,
    mut commands: Commands,
    existing_highlights: Query<Entity, With<SelectionHighlight>>,
    mut notifications: EventWriter<ShowNotification>,
    current_state: Res<State<BuildState>>,
    _registry: Res<ModuleRegistry>,
) {
    if *current_state.get() == BuildState::Inactive { return; }

    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);

    // === CTRL+CLICK: Toggle select ===
    if ctrl && mouse.just_pressed(MouseButton::Left) && !clipboard.paste_mode {
        let Ok(window) = windows.get_single() else { return };
        let Ok((camera, camera_transform)) = camera_query.get_single() else { return };
        let Some(cursor) = window.cursor_position()
            .and_then(|p| camera.viewport_to_world_2d(camera_transform, p))
        else { return };

        let grid_pos = IVec2::new(
            (cursor.x / 66.0).round() as i32,
            ((cursor.y + 33.0) / 66.0).round() as i32,
        );

        if let Some(&entity) = occupancy.cells.get(&grid_pos) {
            if clipboard.selected.contains(&entity) {
                clipboard.selected.retain(|&e| e != entity);
            } else {
                clipboard.selected.push(entity);

                // Auto-select entire multi-block chain
                if let Ok((_, _module, Some(machine_block))) = module_query.get(entity) {
                    if machine_block.role == BlockRole::Core {
                        // Select all blocks connected to this core
                        for (other_entity, _, other_block) in module_query.iter() {
                            if let Some(other_block) = other_block {
                                if other_block.connected_core == Some(entity)
                                    && !clipboard.selected.contains(&other_entity)
                                {
                                    clipboard.selected.push(other_entity);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Update highlights
        for entity in existing_highlights.iter() {
            commands.entity(entity).despawn();
        }
        for &entity in &clipboard.selected {
            if let Ok((_, module, _)) = module_query.get(entity) {
                let pos = module.grid_position;
                commands.spawn((
                    SpriteBundle {
                        sprite: Sprite {
                            color: Color::rgba(0.3, 0.6, 1.0, 0.3),
                            custom_size: Some(Vec2::splat(64.0)),
                            ..default()
                        },
                        transform: Transform::from_xyz(
                            pos.x as f32 * 66.0,
                            pos.y as f32 * 66.0 - 33.0,
                            0.9,
                        ),
                        ..default()
                    },
                    SelectionHighlight,
                ));
            }
        }
        return;
    }

    // === CTRL+C: Copy selection ===
    if ctrl && keyboard.just_pressed(KeyCode::C) && !clipboard.selected.is_empty() {
        // Collect data first to avoid borrow conflicts
        let selected_entities: Vec<Entity> = clipboard.selected.clone();

        let mut entries: Vec<ClipboardEntry> = Vec::new();
        let mut min_pos = IVec2::new(i32::MAX, i32::MAX);

        for &entity in &selected_entities {
            if let Ok((_, module, _)) = module_query.get(entity) {
                min_pos.x = min_pos.x.min(module.grid_position.x);
                min_pos.y = min_pos.y.min(module.grid_position.y);
            }
        }

        for &entity in &selected_entities {
            if let Ok((_, module, machine_block)) = module_query.get(entity) {
                entries.push(ClipboardEntry {
                    module_type: module.module_type,
                    offset: module.grid_position - min_pos,
                    rotation: module.rotation,
                    has_machine_block: machine_block.is_some(),
                    machine_role: machine_block.map(|mb| mb.role),
                });
            }
        }

        let count = entries.len();
        clipboard.copied = entries;
        clipboard.copy_origin = min_pos;

        notifications.send(ShowNotification {
            message: format!("Copied {} modules", count),
            notification_type: NotificationType::Success,
            duration: 2.0,
        });
    }

    // === CTRL+V: Enter paste mode ===
    if ctrl && keyboard.just_pressed(KeyCode::V) && !clipboard.copied.is_empty() {
        clipboard.paste_mode = true;
        notifications.send(ShowNotification {
            message: "Paste mode — click to place, Escape to cancel".into(),
            notification_type: NotificationType::Info,
            duration: 3.0,
        });
    }

    // === ESCAPE: Cancel selection or paste ===
    if keyboard.just_pressed(KeyCode::Escape) {
        if clipboard.paste_mode {
            clipboard.paste_mode = false;
        } else {
            clipboard.selected.clear();
            for entity in existing_highlights.iter() {
                commands.entity(entity).despawn();
            }
        }
    }

    // === CTRL+A: Select all ===
    if ctrl && keyboard.just_pressed(KeyCode::A) {
        clipboard.selected.clear();
        for (entity, _, _) in module_query.iter() {
            clipboard.selected.push(entity);
        }
        notifications.send(ShowNotification {
            message: format!("Selected all {} modules", clipboard.selected.len()),
            notification_type: NotificationType::Info,
            duration: 2.0,
        });
    }
}

/// System: execute paste when clicking during paste mode
pub fn clipboard_paste(
    mouse: Res<Input<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    mut clipboard: ResMut<BuildClipboard>,
    occupancy: Res<GridOccupancy>,
    registry: Res<ModuleRegistry>,
    currency: ResMut<Currency>,
    mut place_events: EventWriter<PlaceModuleRequest>,
    mut notifications: EventWriter<ShowNotification>,
) {
    if !clipboard.paste_mode { return; }
    if !mouse.just_pressed(MouseButton::Left) { return; }

    let Ok(window) = windows.get_single() else { return };
    let Ok((camera, camera_transform)) = camera_query.get_single() else { return };
    let Some(cursor) = window.cursor_position()
        .and_then(|p| camera.viewport_to_world_2d(camera_transform, p))
    else { return };

    let grid_pos = IVec2::new(
        (cursor.x / 66.0).round() as i32,
        ((cursor.y + 33.0) / 66.0).round() as i32,
    );

    // Check total cost
    let total_cost: u32 = clipboard.copied.iter()
        .map(|entry| registry.get(entry.module_type).cost)
        .sum();

    if currency.credits < total_cost {
        notifications.send(ShowNotification {
            message: format!("Not enough credits! Need {}c, have {}c", total_cost, currency.credits),
            notification_type: NotificationType::Warning,
            duration: 3.0,
        });
        return;
    }

    // Check all positions are free
    let all_free = clipboard.copied.iter().all(|entry| {
        let target = grid_pos + entry.offset;
        !occupancy.cells.contains_key(&target)
    });

    if !all_free {
        notifications.send(ShowNotification {
            message: "Cannot paste — some positions are occupied".into(),
            notification_type: NotificationType::Warning,
            duration: 2.0,
        });
        return;
    }

    // Place all modules
    let mut placed = 0u32;
    for entry in &clipboard.copied {
        let target = grid_pos + entry.offset;
        place_events.send(PlaceModuleRequest {
            module_type: entry.module_type,
            grid_position: target,
            rotation: entry.rotation,
            custom_name: None,
            subcomponents: None,
            free: false,
        });
        placed += 1;
    }

    clipboard.paste_mode = false;
    notifications.send(ShowNotification {
        message: format!("Pasted {} modules (-{}c)", placed, total_cost),
        notification_type: NotificationType::Success,
        duration: 2.0,
    });
}

/// Marker for paste ghost preview sprites
#[derive(Component)]
pub struct PasteGhostBlock;

/// Show ghost preview during paste mode
pub fn paste_ghost_preview(
    mut commands: Commands,
    clipboard: Res<BuildClipboard>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    occupancy: Res<GridOccupancy>,
    existing_ghosts: Query<Entity, With<PasteGhostBlock>>,
) {
    // Despawn old ghosts
    for entity in existing_ghosts.iter() {
        commands.entity(entity).despawn();
    }

    if !clipboard.paste_mode || clipboard.copied.is_empty() { return; }

    let Ok(window) = windows.get_single() else { return };
    let Ok((camera, camera_transform)) = camera_query.get_single() else { return };
    let Some(cursor) = window.cursor_position()
        .and_then(|p| camera.viewport_to_world_2d(camera_transform, p))
    else { return };

    let grid_pos = IVec2::new(
        (cursor.x / 66.0).round() as i32,
        ((cursor.y + 33.0) / 66.0).round() as i32,
    );

    for entry in &clipboard.copied {
        let pos = grid_pos + entry.offset;
        let world_x = pos.x as f32 * 66.0;
        let world_y = pos.y as f32 * 66.0 - 33.0;

        let color = if occupancy.cells.contains_key(&pos) {
            Color::rgba(0.8, 0.2, 0.2, 0.3)
        } else {
            Color::rgba(0.3, 0.7, 1.0, 0.3)
        };

        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color,
                    custom_size: Some(Vec2::splat(60.0)),
                    ..default()
                },
                transform: Transform::from_xyz(world_x, world_y, 0.95),
                ..default()
            },
            PasteGhostBlock,
        ));
    }
}
