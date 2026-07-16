use bevy::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};
use crate::components::Module;
use super::components::*;

// ============================================================================
// CONNECTION DETECTION SYSTEM
// Two-pass approach to avoid borrow conflicts.
// Pass 1: BFS to determine connections (read-only positions).
// Pass 2: Apply connection data (mutable writes).
// ============================================================================

/// Connection data computed during BFS
struct ConnectionResult {
    entity: Entity,
    core: Entity,
    distance: u32,
    prev: Option<Entity>,
    next: Option<Entity>,
    role: BlockRole,
}

/// Rebuild all machine connections each frame.
///
/// The position lookup is keyed by (SHIP ROOT, grid cell), not grid cell
/// alone — grid coordinates are ship-local, and every ship/wreck reuses
/// the same small numbers. A global map chained the player's machine
/// cores into machine blocks on wrecks and AI ships that happened to be
/// grid-adjacent in their OWN coordinates: kilometers-long connection
/// lines on screen, and nearby wrecks silently changing the player's
/// composed weapon stats.
pub fn rebuild_machine_connections(
    mut commands: Commands,
    mut machine_query: Query<(Entity, &Module, &mut MachineBlock, &ChildOf)>,
    mut core_stats: Query<&mut MachineStats>,
) {
    // Build (ship, position) → entity lookup
    let mut pos_to_entity: HashMap<(Entity, IVec2), Entity> = HashMap::new();
    let mut entity_to_pos: HashMap<Entity, IVec2> = HashMap::new();
    let mut entity_roles: HashMap<Entity, BlockRole> = HashMap::new();
    let mut cores: Vec<(Entity, IVec2, Entity)> = Vec::new();
    let mut all_machine_entities: HashSet<Entity> = HashSet::new();

    for (entity, module, block, parent) in machine_query.iter_mut() {
        let ship = parent.parent();
        pos_to_entity.insert((ship, module.grid_position), entity);
        entity_to_pos.insert(entity, module.grid_position);
        entity_roles.insert(entity, block.role);
        all_machine_entities.insert(entity);

        if block.role == BlockRole::Core {
            cores.push((entity, module.grid_position, ship));
        }
    }

    // === PASS 1: BFS to compute connections ===
    let mut connections: Vec<ConnectionResult> = Vec::new();
    let mut connected_this_frame: HashSet<Entity> = HashSet::new();
    let mut core_counts: HashMap<Entity, MachineStats> = HashMap::new();

    for (core_entity, core_pos, core_ship) in &cores {
        connected_this_frame.insert(*core_entity);
        core_counts.insert(*core_entity, MachineStats::default());

        // Connections for this core's self
        connections.push(ConnectionResult {
            entity: *core_entity,
            core: *core_entity,
            distance: 0,
            prev: None,
            next: None,
            role: BlockRole::Core,
        });

        let mut queue: VecDeque<(Entity, IVec2, u32, Entity)> = VecDeque::new();
        let mut visited: HashSet<Entity> = HashSet::new();
        visited.insert(*core_entity);

        // Seed with adjacent blocks (same ship only)
        for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
            let adj_pos = *core_pos + offset;
            if let Some(&adj_entity) = pos_to_entity.get(&(*core_ship, adj_pos)) {
                if adj_entity != *core_entity && all_machine_entities.contains(&adj_entity) {
                    queue.push_back((adj_entity, adj_pos, 1, *core_entity));
                }
            }
        }

        while let Some((entity, pos, distance, prev_entity)) = queue.pop_front() {
            if visited.contains(&entity) { continue; }
            visited.insert(entity);
            connected_this_frame.insert(entity);

            let role = entity_roles.get(&entity).copied().unwrap_or(BlockRole::Core);

            connections.push(ConnectionResult {
                entity,
                core: *core_entity,
                distance,
                prev: Some(prev_entity),
                next: None, // Will be set in pass 2
                role,
            });

            // Count toward core stats
            if let Some(stats) = core_counts.get_mut(core_entity) {
                match role {
                    BlockRole::Barrel => stats.barrel_count += 1,
                    BlockRole::AmmoFeed => stats.feed_count += 1,
                    BlockRole::Cooling => stats.cooling_count += 1,
                    BlockRole::FuelRod => stats.fuel_rod_count += 1,
                    BlockRole::Nozzle => stats.nozzle_count += 1,
                    BlockRole::ShieldEmitter => stats.emitter_count += 1,
                    BlockRole::Core => {}
                }
            }

            // Continue BFS for chainable blocks (same ship only)
            if role.can_chain() {
                for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                    let adj_pos = pos + offset;
                    if let Some(&adj_entity) = pos_to_entity.get(&(*core_ship, adj_pos)) {
                        if !visited.contains(&adj_entity) && all_machine_entities.contains(&adj_entity) {
                            queue.push_back((adj_entity, adj_pos, distance + 1, entity));
                        }
                    }
                }
            }
        }
    }

    // Build prev→next mapping
    let mut prev_to_next: HashMap<Entity, Entity> = HashMap::new();
    for conn in &connections {
        if let Some(prev) = conn.prev {
            prev_to_next.insert(prev, conn.entity);
        }
    }

    // === PASS 2: Apply connection data ===
    for conn in &connections {
        if let Ok((_, _, mut block, _)) = machine_query.get_mut(conn.entity) {
            block.connected_core = Some(conn.core);
            block.chain_distance = conn.distance;
            block.prev_in_chain = conn.prev;
            block.next_in_chain = prev_to_next.get(&conn.entity).copied();
        }
    }

    // Apply core stats
    for (core_entity, stats) in &core_counts {
        if let Ok(mut core_stats) = core_stats.get_mut(*core_entity) {
            *core_stats = stats.clone();
        }
    }

    // Mark disconnected blocks
    for entity in &all_machine_entities {
        if !connected_this_frame.contains(entity) {
            if let Ok((_, _, mut block, _)) = machine_query.get_mut(*entity) {
                block.connected_core = None;
                block.chain_distance = 0;
                block.next_in_chain = None;
                block.prev_in_chain = None;
            }
            commands.entity(*entity).insert(Disconnected);
        } else {
            commands.entity(*entity).remove::<Disconnected>();
        }
    }
}

/// Calculate barrel stress — blocks closer to core bear more load
pub fn calculate_barrel_stress(
    block_query: Query<(Entity, &MachineBlock)>,
    mut stress_query: Query<&mut BarrelStress>,
    cascade_query: Query<&CascadeRisk>,
) {
    // Group barrels by core, find max distance
    let mut core_max_distance: HashMap<Entity, u32> = HashMap::new();
    for (_, block) in block_query.iter() {
        if block.role == BlockRole::Barrel {
            if let Some(core) = block.connected_core {
                let max = core_max_distance.entry(core).or_insert(0);
                *max = (*max).max(block.chain_distance);
            }
        }
    }

    for (entity, block) in block_query.iter() {
        if block.role != BlockRole::Barrel || block.connected_core.is_none() { continue; }

        let Ok(mut stress) = stress_query.get_mut(entity) else { continue };
        let Some(core_entity) = block.connected_core else { continue; };
        let max_dist = core_max_distance.get(&core_entity).copied().unwrap_or(1);

        stress.load = max_dist.saturating_sub(block.chain_distance) + 1;

        let base_chance = cascade_query.get(entity)
            .map(|c| c.cascade_chance)
            .unwrap_or(0.15);

        let stress_mult = 1.0 + (stress.load as f32 - 1.0) * 0.5;
        stress.effective_cascade_chance = (base_chance * stress_mult).min(0.8);
    }
}
