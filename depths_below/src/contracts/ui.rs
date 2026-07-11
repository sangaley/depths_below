use bevy::prelude::*;

use crate::ai_ship::components::WorldSimulation;
use crate::components::Ship;
use crate::events::*;
use crate::resources::Currency;
use crate::states::GameState;
use crate::world::home_base;
use super::generation;
use super::{
    ContractState, ContractStatus, Faction, FactionReputation, MissionBoardOpen, ViewingStation,
};

/// Display name for a station index (0 = Haven, 1..=N = outposts).
fn station_name(station: usize) -> String {
    if station == 0 { "Haven Station".to_string() } else { format!("Outpost {}", station) }
}

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
    keyboard: Res<ButtonInput<KeyCode>>,
    mut board_open: ResMut<MissionBoardOpen>,
    existing: Query<Entity, With<MissionBoardPanel>>,
    game_state: Res<State<GameState>>,
    ship_query: Query<&Transform, With<Ship>>,
    mut viewing: ResMut<ViewingStation>,
    mut contract_state: ResMut<ContractState>,
    rep: Res<FactionReputation>,
    mut sim: ResMut<WorldSimulation>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    if !keyboard.just_pressed(KeyCode::KeyJ) { return; }

    if board_open.0 {
        board_open.0 = false;
        for entity in existing.iter() {
            commands.entity(entity).despawn();
        }
        return;
    }

    // Docked at Haven, you're always at station 0. Otherwise (flying near an
    // outpost, or near Haven before actually docking) the board shown is
    // whichever station the ship is currently in range of — every station
    // offers its own bounties.
    let station = if *game_state.get() == GameState::StationDocked {
        Some(0)
    } else {
        ship_query.single().ok()
            .and_then(|t| home_base::nearest_station_index(t.translation.truncate()))
    };

    let Some(station) = station else {
        notifications.write(ShowNotification {
            message: "No station in range — fly closer to view its bounty board.".into(),
            notification_type: NotificationType::Warning,
            duration: 3.0,
        });
        return;
    };

    viewing.0 = station;
    generation::ensure_station_board(station, &mut contract_state, &rep, &mut sim);

    board_open.0 = true;
    commands.init_resource::<MissionBoardSelection>();
    // Spawn board root
    commands.spawn((
        (Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(10.0),
                top: Val::Percent(5.0),
                width: Val::Percent(80.0),
                height: Val::Percent(85.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(16.0)),
                ..default()
            }, BackgroundColor(Color::srgba(0.02, 0.05, 0.15, 0.95)), ZIndex(100)),
        MissionBoardPanel,
    )).with_children(|parent| {
        // Title
        parent.spawn((Text::new(format!("EXPEDITION CONTRACTS — {}", station_name(station))), TextFont { font_size: FontSize::Px(28.0), ..default() }, TextColor(Color::WHITE), Node { margin: UiRect::bottom(Val::Px(12.0)),
            ..default() }));

        // Scrollable content area
        parent.spawn((
            (Node {
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::clip(),
                    flex_grow: 1.0,
                    ..default()
                }),
            MissionBoardContent,
        ));

        // Controls hint
        parent.spawn((Text::new("Up/Down: Select | Enter: Accept | Tab: Abandon Active | J: Close"), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::srgb(0.5, 0.5, 0.5)), Node { margin: UiRect::top(Val::Px(8.0)),
            ..default() }));
    });
}

// ============================================================================
// MISSION BOARD: INPUT
// ============================================================================

pub fn mission_board_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    board_open: Res<MissionBoardOpen>,
    viewing: Res<ViewingStation>,
    mut state: ResMut<ContractState>,
    mut selection: ResMut<MissionBoardSelection>,
    currency: Res<Currency>,
    mut accepted_events: MessageWriter<ContractAccepted>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    if !board_open.0 { return; }

    let avail_len = state.board_mut(viewing.0).len();
    let total_items = avail_len + state.active_contracts.len();
    if total_items == 0 { return; }

    // Navigate
    if keyboard.just_pressed(KeyCode::ArrowUp) {
        if selection.index > 0 {
            selection.index -= 1;
        }
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) {
        if selection.index + 1 < total_items {
            selection.index += 1;
        }
    }

    // Accept (Enter) — no cap on active contracts, take as many bounties as you want
    if keyboard.just_pressed(KeyCode::Enter) {
        let avail_len = state.board_mut(viewing.0).len();
        if selection.index < avail_len {
            let deposit = state.board_mut(viewing.0)[selection.index].deposit;
            if deposit > 0 && currency.credits < deposit {
                notifications.write(ShowNotification {
                    message: format!("Not enough credits for {}c deposit.", deposit),
                    notification_type: NotificationType::Warning,
                    duration: 3.0,
                });
                return;
            }

            let mut contract = state.board_mut(viewing.0).remove(selection.index);
            contract.status = ContractStatus::Active;
            let id = contract.id;
            let title = contract.title.clone();
            state.active_contracts.push(contract);

            accepted_events.write(ContractAccepted { contract_id: id });
            notifications.write(ShowNotification {
                message: format!("Accepted: {}", title),
                notification_type: NotificationType::Success,
                duration: 3.0,
            });

            // Clamp selection
            let new_total = state.board_mut(viewing.0).len() + state.active_contracts.len();
            if selection.index >= new_total && new_total > 0 {
                selection.index = new_total - 1;
            }
        }
    }

    // Abandon active contract (Tab)
    if keyboard.just_pressed(KeyCode::Tab) {
        let avail_len = state.board_mut(viewing.0).len();
        let active_idx = selection.index.checked_sub(avail_len);
        if let Some(idx) = active_idx {
            if idx < state.active_contracts.len() {
                let contract = state.active_contracts.remove(idx);
                notifications.write(ShowNotification {
                    message: format!("Abandoned: {}", contract.title),
                    notification_type: NotificationType::Warning,
                    duration: 3.0,
                });
                let new_total = state.board_mut(viewing.0).len() + state.active_contracts.len();
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
    viewing: Res<ViewingStation>,
    rep: Res<FactionReputation>,
    currency: Res<Currency>,
    selection: Option<Res<MissionBoardSelection>>,
    mut content_query: Query<(Entity, Option<&Children>), With<MissionBoardContent>>,
    mut commands: Commands,
) {
    if !board_open.0 { return; }

    let empty: Vec<super::Contract> = Vec::new();
    let board = state.available_by_station.get(viewing.0).unwrap_or(&empty);

    // MissionBoardContent is spawned with zero children (this system is what
    // populates it), so it starts out with no Children component at all —
    // Bevy only attaches one once a child actually exists. A bare `&Children`
    // in the query would never match that first-frame state and this system
    // would silently no-op forever, which is exactly what was happening: the
    // board always opened empty regardless of how many contracts existed.
    let Ok((content_entity, children)) = content_query.single_mut() else { return; };
    let sel_idx = selection.map(|s| s.index).unwrap_or(0);

    // Despawn old children
    if let Some(children) = children {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }

    commands.entity(content_entity).with_children(|parent| {
        // Header: credits + active count (no cap — take as many bounties as you want)
        parent.spawn((Text::new(format!(
                "Credits: {}   Active: {}",
                currency.credits,
                state.active_contracts.len(),
            )), TextFont { font_size: FontSize::Px(18.0), ..default() }, TextColor(Color::srgb(1.0, 0.84314, 0.0)), Node { margin: UiRect::bottom(Val::Px(12.0)),
            ..default() }));

        let mut global_index: usize = 0;

        // Available contracts grouped by faction
        for faction in Faction::all() {
            let faction_contracts: Vec<_> = board.iter()
                .filter(|c| c.faction == *faction)
                .collect();

            if faction_contracts.is_empty() && !state.active_contracts.iter().any(|c| c.faction == *faction) {
                continue;
            }

            // Faction header
            parent.spawn((Text::new(format!(
                    "\n{} (Rep: {:.0}) {}",
                    faction.name(),
                    rep.get(faction),
                    rep.star_string(faction),
                )), TextFont { font_size: FontSize::Px(20.0), ..default() }, TextColor(Color::srgb(0.4, 0.7, 1.0)), Node { margin: UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Px(8.0), Val::Px(4.0)),
                ..default() }));

            // Available contracts for this faction
            for contract in &faction_contracts {
                let is_selected = global_index == sel_idx;
                let prefix = if is_selected { "> " } else { "  " };
                let color = if is_selected { Color::WHITE } else { Color::srgb(0.7, 0.7, 0.7) };

                let deposit_text = if contract.deposit > 0 {
                    format!("  ({}c deposit)", contract.deposit)
                } else {
                    String::new()
                };

                parent.spawn((Text::new(format!(
                        "{}{:<6} {:<35} [{}c]{}",
                        prefix,
                        contract.star_display(),
                        contract.title,
                        contract.reward,
                        deposit_text,
                    )), TextFont { font_size: FontSize::Px(16.0), ..default() }, TextColor(color)));

                global_index += 1;
            }
        }

        // Active contracts section
        if !state.active_contracts.is_empty() {
            parent.spawn((Text::new("\nACTIVE CONTRACTS"), TextFont { font_size: FontSize::Px(20.0), ..default() }, TextColor(Color::srgb(0.3, 1.0, 0.3)), Node { margin: UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Px(12.0), Val::Px(4.0)),
                ..default() }));

            for contract in &state.active_contracts {
                let is_selected = global_index == sel_idx;
                let prefix = if is_selected { "> " } else { "  " };

                let status_color = match contract.status {
                    ContractStatus::Completed => Color::srgb(0.0, 1.0, 0.0),
                    ContractStatus::Active => Color::srgb(1.0, 1.0, 0.0),
                    _ => Color::srgb(0.5, 0.5, 0.5),
                };

                let status_text = match contract.status {
                    ContractStatus::Completed => " [COMPLETED]".to_string(),
                    _ => format!(" [{}]", contract.progress_text()),
                };

                parent.spawn((Text::new(format!(
                        "{}{:<6} {:<35} {}c{}",
                        prefix,
                        contract.star_display(),
                        contract.title,
                        contract.reward,
                        status_text,
                    )), TextFont { font_size: FontSize::Px(16.0), ..default() }, TextColor(status_color)));

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
        (Node {
                position_type: PositionType::Absolute,
                right: Val::Px(12.0),
                top: Val::Px(12.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)), ZIndex(50)),
        ContractHudRoot,
    )).with_children(|parent| {
        parent.spawn((
            (Text::new(""), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::WHITE)),
            ContractHudText,
        ));
    });
}

pub fn despawn_contract_hud(
    mut commands: Commands,
    query: Query<Entity, With<ContractHudRoot>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

pub fn update_contract_hud(
    state: Res<ContractState>,
    mut text_query: Query<&mut Text, With<ContractHudText>>,
) {
    let Ok(mut text) = text_query.single_mut() else { return; };

    if state.active_contracts.is_empty() {
        text.0 = String::new();
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

    text.0 = lines.join("\n");
}
