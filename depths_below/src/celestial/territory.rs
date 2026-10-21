//! Who holds which systems.
//!
//! Systems used to be dealt to factions by distance from Haven, about three
//! each, with nothing tying a faction's systems together: no capital, no
//! borders, nowhere that was "their space". Now each faction that holds
//! ground has a capital and a cluster of systems grown out from it, and
//! where two clusters meet is a border.
//!
//! - **Territorial factions** (the eight combat factions that hold ground,
//!   plus the two rival trading factions) each get a capital and grow a
//!   cluster round it. Capitals are dealt weakest-nearest, so the walk out
//!   from Haven still gets harder the further you go.
//! - **The bosses** (the Eternal Hegemony and the Shepherd) hold nothing.
//!   They haunt the deepest systems instead: a haunted system keeps its
//!   owner, but the boss is what makes it dangerous, and its danger is the
//!   boss's.
//! - **Danger** is per system: its owner's power, or its haunt's. The story
//!   follows the danger of the system you're in (narrative::logs).

use bevy::prelude::*;

use super::resources::{GalaxyMap, StarSystemDef};
use crate::ai_ship::components::{faction_power, AiShipType};

/// The factions that hold ground, weakest first: the order capitals are
/// dealt outward from Haven.
pub const TERRITORIAL: [AiShipType; 10] = [
    AiShipType::TheSilence,
    AiShipType::MerchantPrinces,
    AiShipType::FreeHaulersCompact,
    AiShipType::RecursiveKingdom,
    AiShipType::BrokenChoir,
    AiShipType::StellarPreserve,
    AiShipType::SynthesisCollective,
    AiShipType::GildedThrone,
    AiShipType::CorpseStars,
    AiShipType::TerranHegemony,
];

/// The bosses, which hold nothing and haunt the deep systems.
pub const ROAMING: [AiShipType; 2] = [AiShipType::EternalHegemony, AiShipType::Shepherd];

/// How many of the deepest systems each boss haunts (Shepherd the deepest).
const HAUNTS: [(AiShipType, usize); 2] = [(AiShipType::Shepherd, 1), (AiShipType::EternalHegemony, 2)];

/// How many systems count as a system's neighbours, for borders.
pub const NEIGHBOURS: usize = 3;

/// A system's danger: its haunting boss's power, or its owner's.
pub fn danger_of(def: &StarSystemDef) -> f32 {
    def.haunt.or(def.faction).map_or(0.0, faction_power)
}

/// Deal the generated systems out. `systems[0]` is Haven and stays nobody's;
/// the rest are sorted nearest-first from Haven.
pub fn assign_territory(systems: &mut [StarSystemDef]) {
    let rest: Vec<usize> = (1..systems.len()).collect();
    if rest.is_empty() {
        return;
    }
    let n = rest.len();
    let per = n.div_ceil(TERRITORIAL.len());

    // Capitals, weakest nearest: faction i takes a system from its own band
    // of distance ranks, the one farthest from the capitals already placed,
    // so capitals spread across the map instead of lining up.
    let mut owner: Vec<Option<usize>> = vec![None; systems.len()];
    let mut capitals: Vec<usize> = Vec::new();
    for (f, _) in TERRITORIAL.iter().enumerate() {
        let lo = (f * n) / TERRITORIAL.len();
        let hi = (((f + 1) * n) / TERRITORIAL.len()).max(lo + 1).min(n);
        let pick = (lo..hi)
            .map(|rank| rest[rank])
            .filter(|&i| owner[i].is_none())
            .max_by(|&a, &b| {
                let spread = |i: usize| {
                    capitals
                        .iter()
                        .map(|&c| systems[c].galaxy_pos.distance(systems[i].galaxy_pos))
                        .fold(f32::MAX, f32::min)
                };
                spread(a).total_cmp(&spread(b))
            });
        let Some(capital) = pick.or_else(|| rest.iter().copied().find(|&i| owner[i].is_none())) else { break };
        owner[capital] = Some(f);
        capitals.push(capital);
    }

    // Grow each cluster out from its capital: the closest unclaimed system
    // to any faction still under its share goes to that faction, again and
    // again, so every cluster is a contiguous patch of the map.
    let mut held: Vec<usize> = vec![1; TERRITORIAL.len()];
    loop {
        let mut best: Option<(f32, usize, usize)> = None;
        for &s in &rest {
            if owner[s].is_some() {
                continue;
            }
            for f in 0..TERRITORIAL.len() {
                if held[f] >= per {
                    continue;
                }
                let d = rest
                    .iter()
                    .filter(|&&o| owner[o] == Some(f))
                    .map(|&o| systems[o].galaxy_pos.distance(systems[s].galaxy_pos))
                    .fold(f32::MAX, f32::min);
                if best.map_or(true, |(bd, ..)| d < bd) {
                    best = Some((d, s, f));
                }
            }
        }
        let Some((_, s, f)) = best else { break };
        owner[s] = Some(f);
        held[f] += 1;
    }

    // Haunts: the deepest systems, by distance rank from Haven.
    let mut deepest = rest.iter().rev().copied();
    let mut haunt: Vec<Option<AiShipType>> = vec![None; systems.len()];
    for (boss, count) in HAUNTS {
        for _ in 0..count {
            if let Some(i) = deepest.next() {
                haunt[i] = Some(boss);
            }
        }
    }

    for &i in &rest {
        let def = &mut systems[i];
        def.faction = owner[i].map(|f| TERRITORIAL[f]);
        def.capital = capitals.contains(&i);
        def.haunt = haunt[i];
        def.danger_tier = danger_of(def);
    }
}

/// The factions holding the systems next to this one, other than its own
/// owner: who it borders. Empty for a system deep in its owner's space.
pub fn border_factions(map: &GalaxyMap, system_id: u32) -> Vec<AiShipType> {
    let Some(def) = map.systems.iter().find(|s| s.id == system_id) else { return Vec::new() };
    let mut out: Vec<AiShipType> = Vec::new();
    for id in super::galaxy::nearest_neighbors(map, system_id, NEIGHBOURS) {
        let Some(other) = map.systems.iter().find(|s| s.id == id).and_then(|s| s.faction) else { continue };
        if Some(other) != def.faction && !out.contains(&other) {
            out.push(other);
        }
    }
    out
}

/// The system a faction rules from, if it holds one.
pub fn capital_of(map: &GalaxyMap, faction: AiShipType) -> Option<u32> {
    map.systems.iter().find(|s| s.capital && s.faction == Some(faction)).map(|s| s.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::celestial::galaxy::generate_galaxy_map;

    #[test]
    fn every_territorial_faction_holds_a_capital_and_its_share() {
        for seed in [1, 7, 42, 99, 1234] {
            let map = generate_galaxy_map(seed);
            for f in TERRITORIAL {
                let held = map.systems.iter().filter(|s| s.faction == Some(f)).count();
                assert!((2..=4).contains(&held), "seed {seed}: {f:?} holds {held}");
                assert!(capital_of(&map, f).is_some(), "seed {seed}: {f:?} has no capital");
            }
            for f in ROAMING {
                assert!(map.systems.iter().all(|s| s.faction != Some(f)), "seed {seed}: boss {f:?} holds ground");
                assert!(map.systems.iter().any(|s| s.haunt == Some(f)), "seed {seed}: {f:?} haunts nothing");
            }
            assert!(map.systems[0].faction.is_none() && map.systems[0].haunt.is_none(), "Haven is nobody's");
        }
    }

    #[test]
    fn danger_still_grows_outward() {
        // Mean danger of the nearest third is below the farthest third.
        let map = generate_galaxy_map(42);
        let mut rest: Vec<_> = map.systems[1..].iter().collect();
        rest.sort_by(|a, b| a.galaxy_pos.length().total_cmp(&b.galaxy_pos.length()));
        let third = rest.len() / 3;
        let mean = |s: &[&StarSystemDef]| s.iter().map(|d| d.danger_tier).sum::<f32>() / s.len() as f32;
        assert!(mean(&rest[..third]) < mean(&rest[rest.len() - third..]));
        // The deepest system is the Shepherd's.
        assert_eq!(rest.last().unwrap().haunt, Some(AiShipType::Shepherd));
    }

    #[test]
    fn some_systems_are_borders() {
        let map = generate_galaxy_map(42);
        let borders = map.systems[1..].iter().filter(|s| !border_factions(&map, s.id).is_empty()).count();
        assert!(borders >= 5, "only {borders} border systems");
        assert!(borders < map.systems.len() - 1, "every system is a border");
    }
}
