use bevy::prelude::*;
use std::collections::{HashSet, VecDeque};

use crate::combat::shields::ShipShield;
use crate::components::*;

use super::components::{AiShip, OwnedByAiShip};

/// Per-ship power balance for AI ships — the AI-side mirror of the player's
/// PowerGraph/PowerState (ship/power.rs), computed independently per AI
/// ship root. Kept as a separate component rather than folding into a
/// shared refactor of the player's PowerState/PowerGraph: those are a
/// global Resource consumed by ~10 player-only combat/UI systems, and a
/// single shared instance can only ever represent one ship (that's the
/// documented reason build_power_graph/update_power_system are hard player-
/// scoped in the first place) — reworking them into per-entity data would
/// have touched every one of those consumers for a feature that only needs
/// AI ships to gain power simulation, not the player's to change shape.
#[derive(Component, Default)]
pub struct AiPowerState {
    pub power_balance: f32,
}

/// Rebuilds every AI ship's power graph and balance each frame: same BFS-
/// from-generators-through-conductive-tiles algorithm as build_power_graph
/// (ship/power.rs), looped per AI root instead of hard-coded to the
/// player's singleton Ship. Grid positions are ship-local, so each ship's
/// BFS must stay scoped to its own modules/hull — mixing them would corrupt
/// every ship's graph, exactly why the player's version is scoped too.
pub fn update_ai_power(
    mut commands: Commands,
    ai_ships: Query<(Entity, Option<&ShipShield>), With<AiShip>>,
    module_query: Query<(&Module, &OwnedByAiShip, Option<&ModuleEfficiency>)>,
    hull_query: Query<(&HullSegment, &OwnedByAiShip)>,
) {
    for (ai_root, shield) in ai_ships.iter() {
        let mut conductive_tiles: HashSet<IVec2> = HashSet::new();
        let mut power_sources: Vec<IVec2> = Vec::new();

        for (module, owned, _eff) in module_query.iter() {
            if owned.root != ai_root || module.health <= 0.0 {
                continue;
            }
            let footprint = crate::building::footprints::footprint_override(module.module_type);
            let cells = crate::building::GridOccupancy::cells_for(
                module.grid_position, module.size, module.rotation, footprint,
            );
            for cell in &cells {
                conductive_tiles.insert(*cell);
            }
            if module.power_generation > 0.0 {
                for cell in cells {
                    power_sources.push(cell);
                }
            }
        }
        for (hull, owned) in hull_query.iter() {
            if owned.root == ai_root {
                conductive_tiles.insert(hull.grid_position);
            }
        }

        let mut visited: HashSet<IVec2> = HashSet::new();
        let mut queue: VecDeque<IVec2> = VecDeque::new();
        for pos in power_sources {
            if visited.insert(pos) {
                queue.push_back(pos);
            }
        }
        let mut powered_tiles: HashSet<IVec2> = HashSet::new();
        while let Some(current) = queue.pop_front() {
            powered_tiles.insert(current);
            for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                let neighbor = current + offset;
                if !visited.contains(&neighbor) && conductive_tiles.contains(&neighbor) {
                    visited.insert(neighbor);
                    queue.push_back(neighbor);
                }
            }
        }

        let mut total_generation = 0.0;
        let mut total_consumption = 0.0;
        // Shield upkeep — AI shields are always enabled (ShipShield's own
        // doc comment), so this is a flat tax whenever a shield exists.
        if shield.is_some_and(|s| s.enabled) {
            total_consumption += crate::combat::shields::SHIELD_UPKEEP_POWER;
        }
        for (module, owned, eff) in module_query.iter() {
            if owned.root != ai_root || !module.is_active {
                continue;
            }
            let efficiency = effective_efficiency(module, eff);
            if module.power_generation > 0.0 {
                total_generation += module.power_generation * efficiency;
                continue;
            }
            if powered_tiles.contains(&module.grid_position) {
                total_consumption += module.power_consumption * efficiency;
            }
        }

        commands.entity(ai_root).try_insert(AiPowerState {
            power_balance: total_generation - total_consumption,
        });
    }
}
