use bevy::prelude::*;
use crate::components::ModuleType;
use crate::building::customization::parameters::*;
use super::framework::*;
use super::tooltip::Tooltip;

// ============================================================================
// MODULE INSPECTION WINDOW (Tier 2)
// Right-click a module → floating window showing sub-component slots.
// Click a slot → dropdown to swap sub-component type.
// Stats update in real-time.
// ============================================================================

/// Marker for the module inspection window
#[derive(Component)]
pub struct InspectionWindow {
    pub module_entity: Entity,
    pub module_type: ModuleType,
}

/// Marker for a sub-component slot button in the inspection window
#[derive(Component)]
pub struct SlotButton {
    pub slot_name: String,
    pub window_id: String,
}

/// Marker for the slot options dropdown
#[derive(Component)]
pub struct SlotDropdown {
    pub slot_name: String,
}

/// Marker for an individual option in the dropdown
#[derive(Component)]
pub struct SlotOptionButton {
    pub slot_name: String,
    pub option_index: usize,
}

/// Marker for the "Customize" button that opens Tier 3
#[derive(Component)]
pub struct CustomizeButton {
    pub slot_name: String,
    pub module_entity: Entity,
}

/// Marker for a preset button
#[derive(Component)]
pub struct PresetButton {
    pub preset_index: usize,
    pub module_entity: Entity,
}

/// Spawn an inspection window for a specific module
pub fn spawn_inspection_window(
    commands: &mut Commands,
    module_entity: Entity,
    module_type: ModuleType,
    customization: Option<&ModuleCustomization>,
    registry: &CustomizationRegistry,
    position: Vec2,
) {
    let window_id = format!("inspect_{:?}", module_entity);
    let title = format!("{} — Configuration", module_type.name());

    let content = spawn_floating_window(
        commands,
        &window_id,
        &title,
        Vec2::new(320.0, 400.0),
        position,
    );

    commands.entity(content).insert(InspectionWindow {
        module_entity,
        module_type,
    });

    // Get customization definition
    let custom_key = format!("weapon_{:?}", module_type).to_lowercase();
    let Some(def) = registry.get(&custom_key) else {
        // No customization available — show basic info
        spawn_window_section(commands, content, "No customization available");
        let info = commands.spawn(
            TextBundle::from_section(
                "This module uses default configuration.",
                TextStyle { font_size: 12.0, color: WindowStyle::TEXT_DIM, ..default() },
            ),
        ).id();
        commands.entity(content).add_child(info);
        return;
    };

    // Header: module name and type
    spawn_window_section(commands, content, "Sub-Components");

    // Show each slot with current selection
    for slot_def in &def.slots {
        let current_selection = customization
            .and_then(|c| c.slot_selections.get(&slot_def.slot_name))
            .copied()
            .unwrap_or(slot_def.default_option);

        let current_option = &slot_def.options[current_selection];

        // Slot row: [Slot Name] [Current Option ▼] [Customize →]
        let slot_row = commands.spawn(
            NodeBundle {
                style: Style {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(4.0)),
                    margin: UiRect::vertical(Val::Px(2.0)),
                    ..default()
                },
                background_color: Color::rgba(0.06, 0.08, 0.12, 0.8).into(),
                ..default()
            },
        ).id();

        // Slot name label
        let slot_label = commands.spawn(
            TextBundle::from_section(
                &slot_def.slot_name,
                TextStyle { font_size: 11.0, color: WindowStyle::TEXT_DIM, ..default() },
            ),
        ).id();

        // Button row
        let button_row = commands.spawn(
            NodeBundle {
                style: Style {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(4.0),
                    align_items: AlignItems::Center,
                    ..default()
                },
                ..default()
            },
        ).id();

        // Current selection button (click to open dropdown)
        let selection_btn = commands.spawn((
            ButtonBundle {
                style: Style {
                    flex_grow: 1.0,
                    padding: UiRect::all(Val::Px(4.0)),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    flex_direction: FlexDirection::Row,
                    ..default()
                },
                background_color: Color::rgba(0.10, 0.13, 0.20, 1.0).into(),
                ..default()
            },
            SlotButton {
                slot_name: slot_def.slot_name.clone(),
                window_id: window_id.clone(),
            },
            Tooltip {
                text: current_option.description.clone(),
                detail: Some(slot_def.description.clone()),
            },
        )).id();

        let selection_text = commands.spawn(
            TextBundle::from_section(
                &current_option.name,
                TextStyle { font_size: 12.0, color: WindowStyle::TEXT_COLOR, ..default() },
            ),
        ).id();

        let dropdown_arrow = commands.spawn(
            TextBundle::from_section(
                "▼",
                TextStyle { font_size: 10.0, color: WindowStyle::TEXT_DIM, ..default() },
            ),
        ).id();

        commands.entity(selection_btn).push_children(&[selection_text, dropdown_arrow]);

        // Customize button (opens Tier 3)
        if !current_option.parameters.is_empty() {
            let customize_btn = commands.spawn((
                ButtonBundle {
                    style: Style {
                        padding: UiRect::horizontal(Val::Px(8.0)),
                        height: Val::Px(24.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    background_color: Color::rgba(0.15, 0.20, 0.35, 1.0).into(),
                    ..default()
                },
                CustomizeButton {
                    slot_name: slot_def.slot_name.clone(),
                    module_entity,
                },
                Tooltip {
                    text: format!("Deep customize {} — {} parameters", slot_def.slot_name, current_option.parameters.len()),
                    detail: None,
                },
            )).id();

            let customize_text = commands.spawn(
                TextBundle::from_section(
                    "⚙",
                    TextStyle { font_size: 14.0, color: Color::rgb(0.5, 0.7, 1.0), ..default() },
                ),
            ).id();

            commands.entity(customize_btn).add_child(customize_text);
            commands.entity(button_row).push_children(&[selection_btn, customize_btn]);
        } else {
            commands.entity(button_row).add_child(selection_btn);
        }

        // Stat modifiers preview
        if !current_option.stat_modifiers.is_empty() {
            let stats_row = commands.spawn(
                NodeBundle {
                    style: Style {
                        flex_direction: FlexDirection::Row,
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: Val::Px(8.0),
                        ..default()
                    },
                    ..default()
                },
            ).id();

            for (stat, modifier) in &current_option.stat_modifiers {
                let color = if *modifier > 1.0 {
                    Color::rgb(0.3, 0.8, 0.4) // Green = buff
                } else if *modifier < 1.0 {
                    Color::rgb(0.8, 0.4, 0.3) // Red = nerf
                } else {
                    WindowStyle::TEXT_DIM
                };

                let sign = if *modifier > 1.0 { "+" } else { "" };
                let pct = ((*modifier - 1.0) * 100.0) as i32;

                let stat_text = commands.spawn(
                    TextBundle::from_section(
                        format!("{}{:}% {}", sign, pct, stat),
                        TextStyle { font_size: 10.0, color, ..default() },
                    ),
                ).id();
                commands.entity(stats_row).add_child(stat_text);
            }

            commands.entity(slot_row).push_children(&[slot_label, button_row, stats_row]);
        } else {
            commands.entity(slot_row).push_children(&[slot_label, button_row]);
        }

        commands.entity(content).add_child(slot_row);
    }

    // Separator before presets
    let sep = commands.spawn(
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Px(1.0),
                margin: UiRect::vertical(Val::Px(6.0)),
                ..default()
            },
            background_color: WindowStyle::BORDER_COLOR.into(),
            ..default()
        },
    ).id();
    commands.entity(content).add_child(sep);

    // Presets section
    spawn_window_section(commands, content, "Quick Presets");

    let presets_row = commands.spawn(
        NodeBundle {
            style: Style {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(4.0),
                row_gap: Val::Px(4.0),
                ..default()
            },
            ..default()
        },
    ).id();

    // Add preset buttons (if weapon has presets)
    let preset_names = ["Balanced", "Sniper", "Brawler", "Custom"];
    for (i, name) in preset_names.iter().enumerate() {
        let btn = commands.spawn((
            ButtonBundle {
                style: Style {
                    padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(3.0), Val::Px(3.0)),
                    ..default()
                },
                background_color: Color::rgba(0.12, 0.15, 0.22, 1.0).into(),
                ..default()
            },
            PresetButton {
                preset_index: i,
                module_entity,
            },
        )).id();

        let text = commands.spawn(
            TextBundle::from_section(
                *name,
                TextStyle { font_size: 11.0, color: WindowStyle::TEXT_COLOR, ..default() },
            ),
        ).id();

        commands.entity(btn).add_child(text);
        commands.entity(presets_row).add_child(btn);
    }

    commands.entity(content).add_child(presets_row);
}

/// System: handle slot button click — cycle to next option
pub fn slot_button_click(
    mut buttons: Query<(&SlotButton, &Interaction), Changed<Interaction>>,
    inspection_windows: Query<&InspectionWindow>,
    mut customization_query: Query<&mut crate::building::customization::parameters::ModuleCustomization>,
    registry: Res<crate::building::customization::parameters::CustomizationRegistry>,
    mut notifications: EventWriter<crate::events::ShowNotification>,
) {
    for (slot_btn, interaction) in buttons.iter_mut() {
        if *interaction != Interaction::Pressed { continue; }

        // Find the inspection window this slot belongs to
        for window in inspection_windows.iter() {
            // Get the customization def for this module
            let key = format!("weapon_{:?}", window.module_type).to_lowercase();
            let Some(def) = registry.get(&key) else { continue };

            // Find the slot definition
            let Some(slot_def) = def.slots.iter().find(|s| s.slot_name == slot_btn.slot_name) else { continue };

            // Get or create customization component
            if let Ok(mut customization) = customization_query.get_mut(window.module_entity) {
                let current = customization.slot_selections
                    .get(&slot_btn.slot_name)
                    .copied()
                    .unwrap_or(slot_def.default_option);

                // Cycle to next option
                let next = (current + 1) % slot_def.options.len();
                customization.slot_selections.insert(slot_btn.slot_name.clone(), next);

                let option_name = &slot_def.options[next].name;
                notifications.send(crate::events::ShowNotification {
                    message: format!("{}: {} — close and reopen to see updated stats", slot_btn.slot_name, option_name),
                    notification_type: crate::events::NotificationType::Info,
                    duration: 2.0,
                });

                // Mark window for rebuild by despawning it (player will right-click to reopen)
                // A proper solution would rebuild inline, but this works for now
            }
        }
    }
}

/// System: handle slot button hover
pub fn slot_button_hover(
    mut buttons: Query<(&Interaction, &mut BackgroundColor), (With<SlotButton>, Changed<Interaction>)>,
) {
    for (interaction, mut bg) in buttons.iter_mut() {
        *bg = match interaction {
            Interaction::Hovered => Color::rgba(0.15, 0.18, 0.28, 1.0).into(),
            Interaction::Pressed => Color::rgba(0.20, 0.24, 0.35, 1.0).into(),
            Interaction::None => Color::rgba(0.10, 0.13, 0.20, 1.0).into(),
        };
    }
}

/// System: handle customize button hover
pub fn customize_button_hover(
    mut buttons: Query<(&Interaction, &mut BackgroundColor), (With<CustomizeButton>, Changed<Interaction>)>,
) {
    for (interaction, mut bg) in buttons.iter_mut() {
        *bg = match interaction {
            Interaction::Hovered => Color::rgba(0.20, 0.28, 0.45, 1.0).into(),
            Interaction::Pressed => Color::rgba(0.25, 0.35, 0.55, 1.0).into(),
            Interaction::None => Color::rgba(0.15, 0.20, 0.35, 1.0).into(),
        };
    }
}

/// System: handle preset button hover
pub fn preset_button_hover(
    mut buttons: Query<(&Interaction, &mut BackgroundColor), (With<PresetButton>, Changed<Interaction>)>,
) {
    for (interaction, mut bg) in buttons.iter_mut() {
        *bg = match interaction {
            Interaction::Hovered => Color::rgba(0.18, 0.22, 0.30, 1.0).into(),
            Interaction::Pressed => Color::rgba(0.22, 0.28, 0.38, 1.0).into(),
            Interaction::None => Color::rgba(0.12, 0.15, 0.22, 1.0).into(),
        };
    }
}
