use bevy::prelude::*;
use rand::prelude::*;

use crate::ai_ship::components::{faction_display_name, faction_power, AiShipType, WorldSimulation};
use crate::components::{CreatureType, PoiType, ZoneType};
use crate::resources::ItemType;
use super::{
    Contract, ContractObjective, ContractState, ContractStatus, ContractType,
    Faction, FactionReputation,
};

// ============================================================================
// FACTION → CONTRACT TYPE WEIGHTS
// ============================================================================

/// No Kill and no CaptureLive: both target creatures, and creatures no longer
/// exist in this build. The board was issuing jobs that could never be
/// completed -- a playthrough accepted "Kill 4 VoidDrifters" and then flew
/// around for seven minutes with nothing to kill. Their weight went to the
/// objectives that actually resolve.
///
/// The ContractType variants themselves are left in place. Kill is the obvious
/// home for a future hunt-a-ship objective and costs nothing sitting unused;
/// generating it is what was broken, not having it.
fn weighted_contract_types(faction: &Faction) -> Vec<(ContractType, u32)> {
    match faction {
        Faction::ResearchInstitute => vec![
            (ContractType::SurveyZone, 40),
            (ContractType::ExplorePoi, 30),
            (ContractType::DestroyShip, 30),
        ],
        Faction::Navy => vec![
            (ContractType::DestroyShip, 50),
            (ContractType::ReachDepth, 25),
            (ContractType::SurveyZone, 25),
        ],
        Faction::SalvageGuild => vec![
            (ContractType::RetrieveSalvage, 45),
            (ContractType::ExplorePoi, 20),
            (ContractType::ReachDepth, 15),
            (ContractType::DestroyShip, 20),
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
        1 => &[CreatureType::VoidDrifter],
        2 => &[CreatureType::VoidDrifter, CreatureType::Stalker],
        3 => &[CreatureType::Stalker, CreatureType::ParasiteSwarm],
        4 => &[CreatureType::ParasiteSwarm, CreatureType::Leviathan],
        _ => &[CreatureType::Leviathan],
    }
}

fn zone_for_star(star: u8) -> ZoneType {
    match star {
        1 => ZoneType::NearOrbit,
        2 => ZoneType::AsteroidBelt,
        3 => ZoneType::DeepSpace,
        4 => ZoneType::Nebula,
        _ => ZoneType::BlackHole,
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

/// Only kinds that exist somewhere other than Haven.
///
/// Cave and ThermalVent are chunk-layer only, and the chunk layer generates in
/// a narrow band around the world origin -- so a contract asking for either was
/// uncompletable the moment the player warped anywhere. Wreck, Ruins and
/// Settlement all have celestial equivalents in every system (see
/// SpacePoiType::as_poi_type).
fn poi_types() -> &'static [PoiType] {
    &[PoiType::Wreck, PoiType::Ruins, PoiType::Settlement]
}

// ============================================================================
// DESTROY-SHIP BOUNTIES — one specific tagged ship, reward scales with how
// far it actually is from Haven Station and how tough its faction is.
// ============================================================================

/// Hostile factions available as bounty targets at each star rating, roughly
/// ordered by how far their territory sits from Haven Station (see
/// `ai_ship::components::faction_territories`) so higher-star contracts (which
/// need more faction rep to unlock) point at farther, tougher targets.
fn ship_factions_for_star(star: u8) -> &'static [AiShipType] {
    match star {
        1 => &[AiShipType::RecursiveKingdom],
        2 => &[AiShipType::RecursiveKingdom, AiShipType::StellarPreserve, AiShipType::BrokenChoir],
        3 => &[AiShipType::SynthesisCollective, AiShipType::TheSilence, AiShipType::GildedThrone],
        4 => &[AiShipType::GildedThrone, AiShipType::TerranHegemony],
        // Bosses only show up at the top star tier (max faction rep) — rare,
        // legendary jackpot bounties, not a routine offering.
        _ => &[AiShipType::TerranHegemony, AiShipType::CorpseStars, AiShipType::EternalHegemony, AiShipType::Shepherd],
    }
}

/// Reward for a ship bounty: base star reward, scaled up by how far the
/// *actual tagged ship* currently is from spawn and by its faction's combat
/// power rating — the two factors the player asked for ("farther and more
/// difficult"). The distance multiplier is uncapped enough that the bosses
/// (580k-850k out, versus ~350k for the farthest normal faction) land in a
/// different bracket entirely rather than just tying the luckiest normal
/// bounty.
fn destroy_ship_reward(star: u8, ship_type: AiShipType, distance: f32, rng: &mut impl Rng) -> u32 {
    let (lo, hi) = base_reward_range(star);
    let base = rng.gen_range(lo..=hi) as f32;

    // 1x near spawn, up to 3x at ~350,000 out (farthest normal faction),
    // up to 5x at ~800,000+ out (boss territory).
    let distance_mult = 1.0 + (distance / 175_000.0).min(4.0);

    // 0.6x for the weakest faction (The Silence) up to 3.8x for The Shepherd.
    let power_mult = 0.6 + faction_power(ship_type) * 0.4;

    (base * distance_mult * power_mult).round() as u32
}

/// Tries each candidate faction for this star tier (in random order) until
/// one still has an untagged living ship to mark as the bounty target.
/// Returns None if every candidate faction is fully claimed/dead right now.
fn tag_destroy_ship_target(star: u8, sim: &mut WorldSimulation, active_systems: &[u32], rng: &mut impl Rng) -> Option<(AiShipType, u32, f32)> {
    let mut factions: Vec<AiShipType> = ship_factions_for_star(star).to_vec();
    factions.shuffle(rng);
    for faction in factions {
        if let Some((bounty_id, distance)) = sim.tag_bounty_target(faction, active_systems, rng) {
            return Some((faction, bounty_id, distance));
        }
    }
    None
}

// ============================================================================
// CONTRACT GENERATION
// ============================================================================

/// Builds one contract. Returns None only for DestroyShip when no eligible
/// ship is currently available to tag (every candidate faction fully claimed
/// or dead) — the caller should just skip that board slot.
fn generate_single_contract(
    faction: Faction,
    star: u8,
    contract_type: ContractType,
    id: u32,
    sim: &mut WorldSimulation,
    active_systems: &[u32],
    rng: &mut impl Rng,
) -> Option<Contract> {
    let zone = zone_for_star(star);
    let (reward_lo, reward_hi) = base_reward_range(star);
    let mut reward = rng.gen_range(reward_lo..=reward_hi);

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
            // Round to 100 units (0.1 km) so the km figure reads cleanly.
            let target = (target / 100.0).round().max(1.0) * 100.0;
            (
                format!("Reach {} out", crate::ui::format_range_km(target)),
                format!("Travel at least {} out from Haven Station.", crate::ui::format_range_km(target)),
                ContractObjective::ReachDepth { target_depth: target, reached: false },
            )
        }
        ContractType::RetrieveSalvage => {
            let (item, count) = salvage_items_for_star(star);
            (
                format!("Retrieve {} {}", count, item.name()),
                format!("Collect {} units of {} and bring them to station.", count, item.name()),
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
        ContractType::DestroyShip => {
            let Some((ship_type, target_id, distance)) = tag_destroy_ship_target(star, sim, active_systems, rng) else {
                return None;
            };
            reward = destroy_ship_reward(star, ship_type, distance, rng);
            let name = faction_display_name(ship_type);
            let is_boss = matches!(ship_type, AiShipType::EternalHegemony | AiShipType::Shepherd);
            let (title, desc) = if is_boss {
                (
                    format!("JACKPOT BOUNTY: {}", name),
                    format!("A {} has been marked on your map, far past the edge of charted space. This is the single biggest bounty available - and the single hardest kill.", name),
                )
            } else {
                (
                    format!("Bounty: {} vessel", name),
                    format!("A {} vessel has been marked on your map - hunt it down and destroy it. Higher-value bounty for a distant, dangerous target.", name),
                )
            };
            (title, desc, ContractObjective::DestroyShip { ship_type, target_id, destroyed: false })
        }
    };

    let deposit = if star >= 3 { reward / 4 } else { 0 };

    Some(Contract {
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
    })
}

/// Generates 2-3 contracts per faction (6-9 total) for one station's board.
/// DestroyShip slots that fail to find an eligible ship to tag are simply
/// skipped, so the board may come back slightly smaller than requested.
pub fn generate_station_board(
    rep: &FactionReputation,
    next_id: &mut u32,
    sim: &mut WorldSimulation,
    active_systems: &[u32],
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
            if let Some(contract) = generate_single_contract(*faction, star, contract_type, *next_id, sim, active_systems, &mut rng) {
                *next_id += 1;
                contracts.push(contract);
            }
        }
    }

    contracts
}

/// Ensures the given station's board is populated, generating one if it's
/// currently empty (fresh start, or every contract on it was already taken).
/// Called lazily whenever the mission board is opened near a station — see
/// ui::toggle_mission_board.
pub fn ensure_station_board(
    station: usize,
    state: &mut ContractState,
    rep: &FactionReputation,
    sim: &mut WorldSimulation,
    active_systems: &[u32],
) {
    if !state.board_mut(station).is_empty() {
        return;
    }
    let contracts = generate_station_board(rep, &mut state.next_id, sim, active_systems);
    *state.board_mut(station) = contracts;
}

#[cfg(test)]
mod poi_target_tests {
    use super::*;
    use crate::celestial::poi::SpacePoiType;

    /// Every kind an Explore contract can ask for must exist somewhere other
    /// than Haven.
    ///
    /// The two point-of-interest systems grew up apart: contracts speak
    /// PoiType, which only ever existed on the chunk layer near the world
    /// origin, while SpacePoi is what actually populates all thirty-one
    /// systems. Asking for a Cave was uncompletable the moment the player
    /// warped anywhere -- and the expedition trail sends them exactly there.
    #[test]
    fn every_explore_target_exists_outside_haven() {
        let reachable: Vec<PoiType> = [
            SpacePoiType::DerelictShip,
            SpacePoiType::DebrisField,
            SpacePoiType::Anomaly,
            SpacePoiType::SignalSource,
            SpacePoiType::SpaceStation,
            SpacePoiType::AsteroidNode,
        ]
        .iter()
        .filter_map(|s| s.as_poi_type())
        .collect();

        for wanted in poi_types() {
            assert!(
                reachable.contains(wanted),
                "Explore contracts can ask for {wanted:?}, which has no celestial \
                 equivalent and so cannot be found outside Haven"
            );
        }
    }

    /// And the list must not be empty, or Explore silently stops generating.
    #[test]
    fn there_are_targets_to_ask_for() {
        assert!(!poi_types().is_empty());
    }
}
