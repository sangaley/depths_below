use bevy::prelude::*;
use crate::components::*;
use crate::ui::windows::inspection::{InspectionWindow, spawn_inspection_window, CustomizeButton};
use crate::ui::windows::customization::spawn_deep_customization_window;
use super::customization::parameters::{CustomizationRegistry, ModuleCustomization};
use super::GridOccupancy;

/// Right-click a placed module → open inspection floating window
pub fn right_click_inspect(
    mouse: Res<Input<MouseButton>>,
    windows_query: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    module_query: Query<(Entity, &Module, Option<&ModuleCustomization>)>,
    occupancy: Res<GridOccupancy>,
    registry: Res<CustomizationRegistry>,
    existing_inspections: Query<&InspectionWindow>,
    mut commands: Commands,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }

    let Ok(window) = windows_query.get_single() else { return };
    let Ok((camera, camera_transform)) = camera_query.get_single() else { return };

    let Some(cursor_pos) = window.cursor_position() else { return };
    let Some(world_pos) = camera.viewport_to_world_2d(camera_transform, cursor_pos) else { return };

    // Convert world pos to grid pos
    let grid_x = (world_pos.x / 66.0).round() as i32;
    let grid_y = ((world_pos.y + 33.0) / 66.0).round() as i32;
    let grid_pos = IVec2::new(grid_x, grid_y);

    // Check if there's a module at this grid position
    let Some(&module_entity) = occupancy.cells.get(&grid_pos) else { return };
    let Ok((entity, module, customization)) = module_query.get(module_entity) else { return };

    // Don't open duplicate inspection windows for the same module
    for existing in existing_inspections.iter() {
        if existing.module_entity == entity {
            return;
        }
    }

    // Spawn inspection window at cursor position (offset so it doesn't cover the module)
    spawn_inspection_window(
        &mut commands,
        entity,
        module.module_type,
        customization,
        &registry,
        Vec2::new(cursor_pos.x + 20.0, cursor_pos.y.max(50.0)),
    );
}

/// When a CustomizeButton is clicked, open the Tier 3 deep customization window
pub fn handle_customize_click(
    buttons: Query<(&CustomizeButton, &Interaction), Changed<Interaction>>,
    module_query: Query<(&Module, Option<&ModuleCustomization>)>,
    registry: Res<CustomizationRegistry>,
    mut commands: Commands,
) {
    for (btn, interaction) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }

        let Ok((module, customization)) = module_query.get(btn.module_entity) else { continue };

        let custom_key = format!("weapon_{:?}", module.module_type).to_lowercase();
        let Some(def) = registry.get(&custom_key) else { continue };

        // Find the slot and current option
        let Some(slot_def) = def.slots.iter().find(|s| s.slot_name == btn.slot_name) else { continue };
        let selection = customization
            .and_then(|c| c.slot_selections.get(&btn.slot_name))
            .copied()
            .unwrap_or(slot_def.default_option);
        let option = &slot_def.options[selection];

        spawn_deep_customization_window(
            &mut commands,
            btn.module_entity,
            &btn.slot_name,
            option,
            customization,
            Vec2::new(400.0, 100.0),
        );
    }
}
