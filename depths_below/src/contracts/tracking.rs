use bevy::prelude::*;

use crate::components::{CargoHold, Module, ModuleType, ZoneType};
use crate::events::*;
use crate::resources::{Currency, DepthState, Inventory};
use super::{ContractObjective, ContractState, ContractStatus, FactionReputation};

// ============================================================================
// KILL TRACKING
// ============================================================================

pub fn track_kill_contracts(
    mut state: ResMut<ContractState>,
    mut kills: EventReader<CreatureKilled>,
) {
    for event in kills.iter() {
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
// POI TRACKING
// ============================================================================

pub fn track_poi_contracts(
    mut state: ResMut<ContractState>,
    mut discoveries: EventReader<PoiDiscovered>,
) {
    for event in discoveries.iter() {
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
                *elapsed_seconds += time.delta_seconds();
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
    mut completed_events: EventWriter<ContractCompleted>,
    mut notifications: EventWriter<ShowNotification>,
) {
    for contract in state.active_contracts.iter_mut() {
        if contract.status != ContractStatus::Active { continue; }
        if contract.is_objective_complete() {
            contract.status = ContractStatus::Completed;
            completed_events.send(ContractCompleted { contract_id: contract.id });
            notifications.send(ShowNotification {
                message: format!("Contract complete: {}! Return to station.", contract.title),
                notification_type: NotificationType::Success,
                duration: 5.0,
            });
        }
    }
}

// ============================================================================
// TURN IN (on entering StationDocked)
// ============================================================================

pub fn turn_in_contracts(
    mut state: ResMut<ContractState>,
    mut rep: ResMut<FactionReputation>,
    mut currency: ResMut<Currency>,
    mut turned_in_events: EventWriter<ContractTurnedIn>,
    mut notifications: EventWriter<ShowNotification>,
) {
    let mut completed_count = 0u32;

    for contract in state.active_contracts.iter_mut() {
        if contract.status != ContractStatus::Completed { continue; }

        currency.credits += contract.reward;
        let rep_gain = contract.star_rating as f32 * 2.0;
        rep.add(&contract.faction, rep_gain);
        contract.status = ContractStatus::TurnedIn;

        turned_in_events.send(ContractTurnedIn {
            contract_id: contract.id,
            reward: contract.reward,
            faction: contract.faction,
        });
        notifications.send(ShowNotification {
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

// ============================================================================
// FAILURE (on entering GameOver)
// ============================================================================

pub fn handle_contract_failure(
    mut state: ResMut<ContractState>,
    mut currency: ResMut<Currency>,
    mut failed_events: EventWriter<ContractFailed>,
    mut notifications: EventWriter<ShowNotification>,
) {
    let mut failed_count = 0u32;

    for contract in state.active_contracts.iter_mut() {
        if contract.status == ContractStatus::TurnedIn { continue; }

        contract.status = ContractStatus::Failed;
        failed_count += 1;

        if contract.deposit > 0 {
            currency.credits = currency.credits.saturating_sub(contract.deposit);
            notifications.send(ShowNotification {
                message: format!("Contract failed: {}. Lost {}c deposit.", contract.title, contract.deposit),
                notification_type: NotificationType::Danger,
                duration: 5.0,
            });
        } else {
            notifications.send(ShowNotification {
                message: format!("Contract failed: {}.", contract.title),
                notification_type: NotificationType::Warning,
                duration: 3.0,
            });
        }

        failed_events.send(ContractFailed { contract_id: contract.id });
    }

    state.contracts_failed_total += failed_count;
    state.active_contracts.clear();
}
