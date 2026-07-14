use bevy::prelude::*;

use crate::ai_ship::components::WorldSimulation;
use crate::components::{CargoHold, Module, ModuleType, Ship, ZoneType};
use crate::events::*;
use crate::resources::{Currency, DepthState, Inventory};
use crate::world::home_base;
use super::{ContractObjective, ContractState, ContractStatus, FactionReputation};

// ============================================================================
// KILL TRACKING
// ============================================================================

pub fn track_kill_contracts(
    mut state: ResMut<ContractState>,
    mut kills: MessageReader<CreatureKilled>,
) {
    for event in kills.read() {
        for contract in state.active_contracts.iter_mut() {
            if contract.status != ContractStatus::Active { continue; }
            if let ContractObjective::Kill { creature_type, current_count, .. } = &mut contract.objective {
                if *creature_type == event.creature_type {
                    *current_count += 1;
                }
            }
        }
    }
}

// ============================================================================
// DESTROY-SHIP TRACKING
// ============================================================================

pub fn track_destroy_ship_contracts(
    mut state: ResMut<ContractState>,
    mut destroyed: MessageReader<AiShipDestroyed>,
) {
    for event in destroyed.read() {
        let Some(bounty_id) = event.bounty_id else { continue };
        for contract in state.active_contracts.iter_mut() {
            if contract.status != ContractStatus::Active { continue; }
            if let ContractObjective::DestroyShip { target_id, destroyed, .. } = &mut contract.objective {
                if *target_id == bounty_id {
                    *destroyed = true;
                }
            }
        }
    }
}

// ============================================================================
// POI TRACKING
// ============================================================================

pub fn track_poi_contracts(
    mut state: ResMut<ContractState>,
    mut discoveries: MessageReader<PoiDiscovered>,
) {
    for event in discoveries.read() {
        for contract in state.active_contracts.iter_mut() {
            if contract.status != ContractStatus::Active { continue; }
            if let ContractObjective::ExplorePoi { poi_type, discovered } = &mut contract.objective {
                if *poi_type == event.poi_type {
                    *discovered = true;
                }
            }
        }
    }
}

// ============================================================================
// DEPTH TRACKING
// ============================================================================

pub fn track_depth_contracts(
    mut state: ResMut<ContractState>,
    depth: Res<DepthState>,
) {
    for contract in state.active_contracts.iter_mut() {
        if contract.status != ContractStatus::Active { continue; }
        if let ContractObjective::ReachDepth { target_depth, reached } = &mut contract.objective {
            if depth.current_depth >= *target_depth {
                *reached = true;
            }
        }
    }
}

// ============================================================================
// SALVAGE TRACKING
// ============================================================================

pub fn track_salvage_contracts(
    mut state: ResMut<ContractState>,
    inventory: Res<Inventory>,
) {
    for contract in state.active_contracts.iter_mut() {
        if contract.status != ContractStatus::Active { continue; }
        if let ContractObjective::RetrieveSalvage { item_type, current_count, .. } = &mut contract.objective {
            let held = inventory.items.get(item_type).copied().unwrap_or(0);
            *current_count = held;
        }
    }
}

// ============================================================================
// SURVEY TRACKING
// ============================================================================

fn current_zone(depth: f32) -> ZoneType {
    if depth < 200.0 { ZoneType::NearOrbit }
    else if depth < 500.0 { ZoneType::AsteroidBelt }
    else if depth < 1000.0 { ZoneType::DeepSpace }
    else if depth < 2000.0 { ZoneType::Nebula }
    else { ZoneType::BlackHole }
}

pub fn track_survey_contracts(
    mut state: ResMut<ContractState>,
    depth: Res<DepthState>,
    time: Res<Time>,
) {
    let zone = current_zone(depth.current_depth);

    for contract in state.active_contracts.iter_mut() {
        if contract.status != ContractStatus::Active { continue; }
        if let ContractObjective::SurveyZone { zone: target_zone, elapsed_seconds, .. } = &mut contract.objective {
            if zone == *target_zone {
                *elapsed_seconds += time.delta_secs();
            }
        }
    }
}

// ============================================================================
// CAPTURE TRACKING
// ============================================================================

pub fn track_capture_contracts(
    mut state: ResMut<ContractState>,
    containment_query: Query<(&CargoHold, &Module)>,
) {
    // Check if any CreatureContainment module has cargo > 0
    // This is a simplified check — in a full implementation you'd track
    // which creature type is in each containment unit.
    let has_captured_creature = containment_query.iter().any(|(cargo, module)| {
        module.module_type == ModuleType::CreatureContainment && cargo.current_weight > 0.0
    });

    if has_captured_creature {
        for contract in state.active_contracts.iter_mut() {
            if contract.status != ContractStatus::Active { continue; }
            if let ContractObjective::CaptureLive { captured, .. } = &mut contract.objective {
                *captured = true;
            }
        }
    }
}

// ============================================================================
// COMPLETION CHECK
// ============================================================================

pub fn check_contract_completion(
    mut state: ResMut<ContractState>,
    mut completed_events: MessageWriter<ContractCompleted>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for contract in state.active_contracts.iter_mut() {
        if contract.status != ContractStatus::Active { continue; }
        if contract.is_objective_complete() {
            contract.status = ContractStatus::Completed;
            completed_events.write(ContractCompleted { contract_id: contract.id });
            notifications.write(ShowNotification {
                message: format!("Contract complete: {}! Claim it at any station (F).", contract.title),
                notification_type: NotificationType::Success,
                duration: 5.0,
            });
        }
    }
}

// ============================================================================
// TURN IN — claimable at any station, not just the one that offered it
// ============================================================================

/// Pays out every completed active contract. Shared by the automatic
/// Haven-docking turn-in and the proximity-based turn-in at any station.
fn turn_in_active(
    state: &mut ContractState,
    rep: &mut FactionReputation,
    currency: &mut Currency,
    turned_in_events: &mut MessageWriter<ContractTurnedIn>,
    notifications: &mut MessageWriter<ShowNotification>,
) {
    let mut completed_count = 0u32;

    for contract in state.active_contracts.iter_mut() {
        if contract.status != ContractStatus::Completed { continue; }

        currency.credits += contract.reward;
        let rep_gain = contract.star_rating as f32 * 2.0;
        rep.add(&contract.faction, rep_gain);
        contract.status = ContractStatus::TurnedIn;

        turned_in_events.write(ContractTurnedIn {
            contract_id: contract.id,
            reward: contract.reward,
            faction: contract.faction,
        });
        notifications.write(ShowNotification {
            message: format!(
                "{} contract turned in! +{}c, +{:.0} {} rep",
                contract.title, contract.reward, rep_gain, contract.faction.name()
            ),
            notification_type: NotificationType::Success,
            duration: 5.0,
        });

        completed_count += 1;
    }

    state.contracts_completed_total += completed_count;
    state.active_contracts.retain(|c| c.status != ContractStatus::TurnedIn);
}

/// Docking at Haven always settles up any completed bounties.
pub fn turn_in_contracts(
    mut state: ResMut<ContractState>,
    mut rep: ResMut<FactionReputation>,
    mut currency: ResMut<Currency>,
    mut turned_in_events: MessageWriter<ContractTurnedIn>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    turn_in_active(&mut state, &mut rep, &mut currency, &mut turned_in_events, &mut notifications);
}

/// Flying near any station (Haven or an outpost) and pressing F claims
/// completed contracts too — you don't need to fly all the way back to
/// wherever a bounty happened to be posted to collect it.
pub fn turn_in_at_station_proximity(
    keyboard: Res<ButtonInput<KeyCode>>,
    ship_query: Query<&Transform, With<Ship>>,
    mut state: ResMut<ContractState>,
    mut rep: ResMut<FactionReputation>,
    mut currency: ResMut<Currency>,
    mut turned_in_events: MessageWriter<ContractTurnedIn>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    if !keyboard.just_pressed(KeyCode::KeyF) { return; }
    if !state.active_contracts.iter().any(|c| c.status == ContractStatus::Completed) { return; }

    let Ok(transform) = ship_query.single() else { return };
    let pos = transform.translation.truncate();
    if home_base::nearest_station_index(pos).is_none() { return; }

    turn_in_active(&mut state, &mut rep, &mut currency, &mut turned_in_events, &mut notifications);
}

// ============================================================================
// FAILURE (on entering GameOver)
// ============================================================================

pub fn handle_contract_failure(
    mut state: ResMut<ContractState>,
    mut currency: ResMut<Currency>,
    mut sim: ResMut<WorldSimulation>,
    mut failed_events: MessageWriter<ContractFailed>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let mut failed_count = 0u32;

    for contract in state.active_contracts.iter_mut() {
        if contract.status == ContractStatus::TurnedIn { continue; }

        if let ContractObjective::DestroyShip { target_id, .. } = &contract.objective {
            sim.untag_bounty(*target_id);
        }

        contract.status = ContractStatus::Failed;
        failed_count += 1;

        if contract.deposit > 0 {
            currency.credits = currency.credits.saturating_sub(contract.deposit);
            notifications.write(ShowNotification {
                message: format!("Contract failed: {}. Lost {}c deposit.", contract.title, contract.deposit),
                notification_type: NotificationType::Danger,
                duration: 5.0,
            });
        } else {
            notifications.write(ShowNotification {
                message: format!("Contract failed: {}.", contract.title),
                notification_type: NotificationType::Warning,
                duration: 3.0,
            });
        }

        failed_events.write(ContractFailed { contract_id: contract.id });
    }

    state.contracts_failed_total += failed_count;
    state.active_contracts.clear();
}
