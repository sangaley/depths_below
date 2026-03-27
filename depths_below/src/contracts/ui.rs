use bevy::prelude::*;

use crate::events::*;
use crate::resources::Currency;
use super::{
    ContractState, ContractStatus, Faction, FactionReputation, MissionBoardOpen,
};

// ============================================================================
// MARKER COMPONENTS
// ============================================================================

#[derive(Component)]
pub struct MissionBoardPanel;

#[derive(Component)]
pub struct MissionBoardContent;

/// Tracks which contract index (in the combined available+active list) is selected.
#[derive(Resource, Default)]
pub struct MissionBoardSelection {
    index: usize,
}

#[derive(Component)]
pub struct ContractHudRoot;

#[derive(Component)]
pub struct ContractHudText;

// ============================================================================
// MISSION BOARD: TOGGLE
// ============================================================================

pub fn toggle_mission_board(
    mut commands: Commands,
    keyboard: Res<Input<KeyCode>>,
    mut board_open: ResMut<MissionBoardOpen>,
    existing: Query<Entity, With<MissionBoardPanel>>,
) {
    if !keyboard.just_pressed(KeyCode::J) { return; }

    board_open.0 = !board_open.0;

    if board_open.0 {
        commands.init_resource::<MissionBoardSelection>();
        // Spawn board root
        commands.spawn((
            NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(10.0),
                    top: Val::Percent(5.0),
                    width: Val::Percent(80.0),
                    height: Val::Percent(85.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(16.0)),
                    ..default()
                },
                background_color: Color::rgba(0.02, 0.05, 0.15, 0.95).into(),
                z_index: ZIndex::Global(100),
                ..default()
            },
            MissionBoardPanel,
        )).with_children(|parent| {
            // Title
            parent.spawn(TextBundle::from_section(
                "EXPEDITION CONTRACTS",
                TextStyle { font_size: 28.0, color: Color::WHITE, ..default() },
            ).with_style(Style {
                margin: UiRect::bottom(Val::Px(12.0)),
                ..default()
            }));

            // Scrollable content area
            parent.spawn((
                NodeBundle {
                    style: Style {
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::clip(),
                        flex_grow: 1.0,
                        ..default()
                    },
                    ..default()
                },
                MissionBoardContent,
            ));

            // Controls hint
            parent.spawn(TextBundle::from_section(
                "Up/Down: Select | Enter: Accept | Tab: Abandon Active | J: Close",
                TextStyle { font_size: 14.0, color: Color::GRAY, ..default() },
            ).with_style(Style {
                margin: UiRect::top(Val::Px(8.0)),
                ..default()
            }));
        });
    } else {
        for entity in existing.iter() {
            commands.entity(entity).despawn_recursive();
        }
    }
}

// ============================================================================
// MISSION BOARD: INPUT
// ============================================================================

pub fn mission_board_input(
    keyboard: Res<Input<KeyCode>>,
    board_open: Res<MissionBoardOpen>,
    mut state: ResMut<ContractState>,
    mut selection: ResMut<MissionBoardSelection>,
    currency: Res<Currency>,
    mut accepted_events: EventWriter<ContractAccepted>,
    mut notifications: EventWriter<ShowNotification>,
) {
    if !board_open.0 { return; }

    let total_items = state.available_contracts.len() + state.active_contracts.len();
    if total_items == 0 { return; }

    // Navigate
    if keyboard.just_pressed(KeyCode::Up) {
        if selection.index > 0 {
            selection.index -= 1;
        }
    }
    if keyboard.just_pressed(KeyCode::Down) {
        if selection.index + 1 < total_items {
            selection.index += 1;
        }
    }

    // Accept (Enter)
    if keyboard.just_pressed(KeyCode::Return) {
        let avail_len = state.available_contracts.len();
        if selection.index < avail_len {
            // Trying to accept an available contract
            if state.active_contracts.len() >= ContractState::MAX_ACTIVE {
                notifications.send(ShowNotification {
                    message: "Cannot accept: max 3 active contracts.".into(),
                    notification_type: NotificationType::Warning,
                    duration: 3.0,
                });
                return;
            }

            let deposit = state.available_contracts[selection.index].deposit;
            if deposit > 0 && currency.credits < deposit {
                notifications.send(ShowNotification {
                    message: format!("Not enough credits for {}c deposit.", deposit),
                    notification_type: NotificationType::Warning,
                    duration: 3.0,
                });
                return;
            }

            let mut contract = state.available_contracts.remove(selection.index);
            contract.status = ContractStatus::Active;
            let id = contract.id;
            let title = contract.title.clone();
            state.active_contracts.push(contract);

            accepted_events.send(ContractAccepted { contract_id: id });
            notifications.send(ShowNotification {
                message: format!("Accepted: {}", title),
                notification_type: NotificationType::Success,
                duration: 3.0,
            });

            // Clamp selection
            let new_total = state.available_contracts.len() + state.active_contracts.len();
            if selection.index >= new_total && new_total > 0 {
                selection.index = new_total - 1;
            }
        }
    }

    // Abandon active contract (Tab)
    if keyboard.just_pressed(KeyCode::Tab) {
        let avail_len = state.available_contracts.len();
        let active_idx = selection.index.checked_sub(avail_len);
        if let Some(idx) = active_idx {
            if idx < state.active_contracts.len() {
                let contract = state.active_contracts.remove(idx);
                notifications.send(ShowNotification {
                    message: format!("Abandoned: {}", contract.title),
                    notification_type: NotificationType::Warning,
                    duration: 3.0,
                });
                let new_total = state.available_contracts.len() + state.active_contracts.len();
                if selection.index >= new_total && new_total > 0 {
                    selection.index = new_total - 1;
                }
            }
        }
    }
}

// ============================================================================
// MISSION BOARD: DISPLAY UPDATE
// ============================================================================

pub fn update_mission_board_display(
    board_open: Res<MissionBoardOpen>,
    state: Res<ContractState>,
    rep: Res<FactionReputation>,
    currency: Res<Currency>,
    selection: Option<Res<MissionBoardSelection>>,
    mut content_query: Query<(Entity, &Children), With<MissionBoardContent>>,
    mut commands: Commands,
) {
    if !board_open.0 { return; }

    let Ok((content_entity, children)) = content_query.get_single_mut() else { return; };
    let sel_idx = selection.map(|s| s.index).unwrap_or(0);

    // Despawn old children
    for &child in children.iter() {
        commands.entity(child).despawn_recursive();
    }

    commands.entity(content_entity).with_children(|parent| {
        // Header: credits + active count
        parent.spawn(TextBundle::from_section(
            format!(
                "Credits: {}   Active: {}/{}",
                currency.credits,
                state.active_contracts.len(),
                ContractState::MAX_ACTIVE,
            ),
            TextStyle { font_size: 18.0, color: Color::GOLD, ..default() },
        ).with_style(Style {
            margin: UiRect::bottom(Val::Px(12.0)),
            ..default()
        }));

        let mut global_index: usize = 0;

        // Available contracts grouped by faction
        for faction in Faction::all() {
            let faction_contracts: Vec<_> = state.available_contracts.iter()
                .filter(|c| c.faction == *faction)
                .collect();

            if faction_contracts.is_empty() && !state.active_contracts.iter().any(|c| c.faction == *faction) {
                continue;
            }

            // Faction header
            parent.spawn(TextBundle::from_section(
                format!(
                    "\n{} (Rep: {:.0}) {}",
                    faction.name(),
                    rep.get(faction),
                    rep.star_string(faction),
                ),
                TextStyle { font_size: 20.0, color: Color::rgb(0.4, 0.7, 1.0), ..default() },
            ).with_style(Style {
                margin: UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Px(8.0), Val::Px(4.0)),
                ..default()
            }));

            // Available contracts for this faction
            for contract in &faction_contracts {
                let is_selected = global_index == sel_idx;
                let prefix = if is_selected { "> " } else { "  " };
                let color = if is_selected { Color::WHITE } else { Color::rgb(0.7, 0.7, 0.7) };

                let deposit_text = if contract.deposit > 0 {
                    format!("  ({}c deposit)", contract.deposit)
                } else {
                    String::new()
                };

                parent.spawn(TextBundle::from_section(
                    format!(
                        "{}{:<6} {:<35} [{}c]{}",
                        prefix,
                        contract.star_display(),
                        contract.title,
                        contract.reward,
                        deposit_text,
                    ),
                    TextStyle { font_size: 16.0, color, ..default() },
                ));

                global_index += 1;
            }
        }

        // Active contracts section
        if !state.active_contracts.is_empty() {
            parent.spawn(TextBundle::from_section(
                "\nACTIVE CONTRACTS",
                TextStyle { font_size: 20.0, color: Color::rgb(0.3, 1.0, 0.3), ..default() },
            ).with_style(Style {
                margin: UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Px(12.0), Val::Px(4.0)),
                ..default()
            }));

            for contract in &state.active_contracts {
                let is_selected = global_index == sel_idx;
                let prefix = if is_selected { "> " } else { "  " };

                let status_color = match contract.status {
                    ContractStatus::Completed => Color::GREEN,
                    ContractStatus::Active => Color::YELLOW,
                    _ => Color::GRAY,
                };

                let status_text = match contract.status {
                    ContractStatus::Completed => " [COMPLETED]".to_string(),
                    _ => format!(" [{}]", contract.progress_text()),
                };

                parent.spawn(TextBundle::from_section(
                    format!(
                        "{}{:<6} {:<35} {}c{}",
                        prefix,
                        contract.star_display(),
                        contract.title,
                        contract.reward,
                        status_text,
                    ),
                    TextStyle { font_size: 16.0, color: status_color, ..default() },
                ));

                global_index += 1;
            }
        }
    });
}

// ============================================================================
// CONTRACT HUD (top-right during Exploring)
// ============================================================================

pub fn spawn_contract_hud(
    mut commands: Commands,
) {
    commands.spawn((
        NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                right: Val::Px(12.0),
                top: Val::Px(12.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            },
            background_color: Color::rgba(0.0, 0.0, 0.0, 0.6).into(),
            z_index: ZIndex::Global(50),
            ..default()
        },
        ContractHudRoot,
    )).with_children(|parent| {
        parent.spawn((
            TextBundle::from_section(
                "",
                TextStyle { font_size: 14.0, color: Color::WHITE, ..default() },
            ),
            ContractHudText,
        ));
    });
}

pub fn despawn_contract_hud(
    mut commands: Commands,
    query: Query<Entity, With<ContractHudRoot>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

pub fn update_contract_hud(
    state: Res<ContractState>,
    mut text_query: Query<&mut Text, With<ContractHudText>>,
) {
    let Ok(mut text) = text_query.get_single_mut() else { return; };

    if state.active_contracts.is_empty() {
        text.sections[0].value = String::new();
        return;
    }

    let mut lines = Vec::new();
    for contract in &state.active_contracts {
        let status = match contract.status {
            ContractStatus::Completed => "DONE".to_string(),
            _ => contract.progress_text(),
        };
        lines.push(format!(
            "{} {}: {}",
            contract.star_display(),
            contract.title,
            status,
        ));
    }

    text.sections[0].value = lines.join("\n");
}
