use bevy::prelude::*;
use rand::prelude::*;

use crate::components::{CreatureType, PoiType, ZoneType};
use crate::resources::ItemType;
use super::{
    Contract, ContractObjective, ContractState, ContractStatus, ContractType,
    Faction, FactionReputation,
};

// ============================================================================
// FACTION → CONTRACT TYPE WEIGHTS
// ============================================================================

fn weighted_contract_types(faction: &Faction) -> Vec<(ContractType, u32)> {
    match faction {
        Faction::ResearchInstitute => vec![
            (ContractType::CaptureLive, 30),
            (ContractType::ExplorePoi, 25),
            (ContractType::SurveyZone, 25),
            (ContractType::Kill, 20),
        ],
        Faction::Navy => vec![
            (ContractType::Kill, 40),
            (ContractType::ReachDepth, 25),
            (ContractType::SurveyZone, 20),
            (ContractType::ExplorePoi, 15),
        ],
        Faction::SalvageGuild => vec![
            (ContractType::RetrieveSalvage, 40),
            (ContractType::ExplorePoi, 30),
            (ContractType::ReachDepth, 20),
            (ContractType::Kill, 10),
        ],
    }
}

fn pick_weighted(weights: &[(ContractType, u32)], rng: &mut impl Rng) -> ContractType {
    let total: u32 = weights.iter().map(|(_, w)| w).sum();
    let mut roll = rng.gen_range(0..total);
    for (ct, w) in weights {
        if roll < *w {
            return *ct;
        }
        roll -= *w;
    }
    weights.last().map(|(ct, _)| *ct).unwrap_or(ContractType::Kill)
}

// ============================================================================
// STAR → PARAMETERS
// ============================================================================

fn creatures_for_star(star: u8) -> &'static [CreatureType] {
    match star {
        1 => &[CreatureType::Scavenger, CreatureType::Stalker],
        2 => &[CreatureType::Ambusher, CreatureType::ElectricEel],
        3 => &[CreatureType::BlindHunter, CreatureType::LureFish],
        4 => &[CreatureType::SwarmQueen, CreatureType::Parasite],
        _ => &[CreatureType::Leviathan, CreatureType::Watcher],
    }
}

fn zone_for_star(star: u8) -> ZoneType {
    match star {
        1 => ZoneType::Light,
        2 => ZoneType::Twilight,
        3 => ZoneType::Dark,
        4 => ZoneType::Abyss,
        _ => ZoneType::Trench,
    }
}

fn depth_range_for_star(star: u8) -> (f32, f32) {
    match star {
        1 => (50.0, 200.0),
        2 => (200.0, 500.0),
        3 => (500.0, 1000.0),
        4 => (1000.0, 2000.0),
        _ => (2000.0, 3500.0),
    }
}

fn base_reward_range(star: u8) -> (u32, u32) {
    match star {
        1 => (80, 150),
        2 => (150, 300),
        3 => (300, 500),
        4 => (500, 800),
        _ => (800, 1500),
    }
}

fn kill_count_for_star(star: u8) -> u32 {
    match star {
        1 => 3,
        2 => 4,
        3 => 3,
        4 => 2,
        _ => 1,
    }
}

fn salvage_items_for_star(star: u8) -> (ItemType, u32) {
    match star {
        1 => (ItemType::ScrapMetal, 5),
        2 => (ItemType::Crystal, 4),
        3 => (ItemType::BioSample, 6),
        4 => (ItemType::RareAlloy, 3),
        _ => (ItemType::AncientArtifact, 2),
    }
}

fn survey_seconds_for_star(star: u8) -> f32 {
    match star {
        1 => 30.0,
        2 => 45.0,
        3 => 60.0,
        4 => 90.0,
        _ => 120.0,
    }
}

fn poi_types() -> &'static [PoiType] {
    &[PoiType::Wreck, PoiType::Cave, PoiType::Ruins, PoiType::ThermalVent, PoiType::Settlement]
}

// ============================================================================
// CONTRACT GENERATION
// ============================================================================

fn generate_single_contract(
    faction: Faction,
    star: u8,
    contract_type: ContractType,
    id: u32,
    rng: &mut impl Rng,
) -> Contract {
    let zone = zone_for_star(star);
    let (reward_lo, reward_hi) = base_reward_range(star);
    let reward = rng.gen_range(reward_lo..=reward_hi);
    let deposit = if star >= 3 { reward / 4 } else { 0 };

    let (title, description, objective) = match contract_type {
        ContractType::Kill => {
            let creatures = creatures_for_star(star);
            let ct = creatures[rng.gen_range(0..creatures.len())];
            let count = kill_count_for_star(star);
            (
                format!("Kill {} {:?}s", count, ct),
                format!("Eliminate {} {:?} creatures in the {:?} zone.", count, ct, zone),
                ContractObjective::Kill { creature_type: ct, target_count: count, current_count: 0 },
            )
        }
        ContractType::ExplorePoi => {
            let pois = poi_types();
            let pt = pois[rng.gen_range(0..pois.len())];
            (
                format!("Explore {:?}", pt),
                format!("Discover a {:?} point of interest.", pt),
                ContractObjective::ExplorePoi { poi_type: pt, discovered: false },
            )
        }
        ContractType::ReachDepth => {
            let (lo, hi) = depth_range_for_star(star);
            let target = rng.gen_range(lo..=hi);
            let target = (target / 50.0).round() * 50.0; // round to nearest 50
            (
                format!("Reach {}m depth", target as u32),
                format!("Descend to at least {}m depth.", target as u32),
                ContractObjective::ReachDepth { target_depth: target, reached: false },
            )
        }
        ContractType::RetrieveSalvage => {
            let (item, count) = salvage_items_for_star(star);
            (
                format!("Retrieve {} {}", count, item.name()),
                format!("Collect {} units of {} and bring them to surface.", count, item.name()),
                ContractObjective::RetrieveSalvage { item_type: item, target_count: count, current_count: 0 },
            )
        }
        ContractType::CaptureLive => {
            let creatures = creatures_for_star(star);
            let ct = creatures[rng.gen_range(0..creatures.len())];
            (
                format!("Capture {:?} alive", ct),
                format!("Capture a living {:?} specimen using Creature Containment.", ct),
                ContractObjective::CaptureLive { creature_type: ct, captured: false },
            )
        }
        ContractType::SurveyZone => {
            let seconds = survey_seconds_for_star(star);
            (
                format!("Survey {:?} Zone", zone),
                format!("Spend {:.0} seconds in the {:?} zone to complete the survey.", seconds, zone),
                ContractObjective::SurveyZone { zone, required_seconds: seconds, elapsed_seconds: 0.0 },
            )
        }
    };

    Contract {
        id,
        faction,
        contract_type,
        title,
        description,
        objective,
        reward,
        deposit,
        star_rating: star,
        depth_zone: zone,
        status: ContractStatus::Available,
    }
}

/// Generates 2-3 contracts per faction (6-9 total) and fills the board.
pub fn generate_contract_board(
    rep: &FactionReputation,
    next_id: &mut u32,
) -> Vec<Contract> {
    let mut rng = rand::thread_rng();
    let mut contracts = Vec::new();

    for faction in Faction::all() {
        let max_star = rep.max_star(faction);
        let count = rng.gen_range(2..=3);

        for _ in 0..count {
            let star = rng.gen_range(1..=max_star);
            let weights = weighted_contract_types(faction);
            let contract_type = pick_weighted(&weights, &mut rng);
            let contract = generate_single_contract(*faction, star, contract_type, *next_id, &mut rng);
            *next_id += 1;
            contracts.push(contract);
        }
    }

    contracts
}

// ============================================================================
// SYSTEM: generate board on entering SurfaceBase
// ============================================================================

pub fn generate_initial_board(
    mut state: ResMut<ContractState>,
    rep: Res<FactionReputation>,
) {
    // Only regenerate if the available board is empty (fresh start or all taken).
    // Keep available contracts if some remain so the player can re-visit the board.
    if !state.available_contracts.is_empty() {
        return;
    }

    let contracts = generate_contract_board(&rep, &mut state.next_id);
    state.available_contracts = contracts;
}
