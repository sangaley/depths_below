use bevy::prelude::*;
use std::collections::{HashSet, VecDeque};
use crate::components::*;
use crate::resources::*;
use crate::events::*;

/// BFS from all power-generating module positions through adjacent module AND inner hull tiles.
/// Builds a set of all grid tiles that have power connectivity.
/// Power flows through: active modules (health > 0) and inner hull segments (walls/bulkheads).
pub fn build_power_graph(
    module_query: Query<&Module>,
    hull_query: Query<&HullSegment>,
    mut power_graph: ResMut<PowerGraph>,
) {
    power_graph.powered_tiles.clear();

    // Collect all tiles that can conduct power
    let mut conductive_tiles: HashSet<IVec2> = HashSet::new();
    let mut power_sources: Vec<IVec2> = Vec::new();

    // Modules conduct power (if alive)
    for module in module_query.iter() {
        if module.health <= 0.0 { continue; }
        conductive_tiles.insert(module.grid_position);
        // Multi-cell modules: insert all occupied cells
        let cells = crate::building::GridOccupancy::cells_for(
            module.grid_position, module.size, module.rotation
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

    // All hull segments conduct power (structural backbone of the submarine)
    for hull in hull_query.iter() {
        conductive_tiles.insert(hull.grid_position);
    }

    // BFS from power sources through adjacent conductive tiles
    let mut visited: HashSet<IVec2> = HashSet::new();
    let mut queue: VecDeque<IVec2> = VecDeque::new();
    for pos in power_sources {
        if visited.insert(pos) { queue.push_back(pos); }
    }
    while let Some(current) = queue.pop_front() {
        power_graph.powered_tiles.insert(current);
        for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
            let neighbor = current + offset;
            if !visited.contains(&neighbor) && conductive_tiles.contains(&neighbor) {
                visited.insert(neighbor);
                queue.push_back(neighbor);
            }
        }
    }
}

/// Updates the power system. Uses PowerGraph for adjacency and ModuleEfficiency for staffing+damage.
pub fn update_power_system(
    module_query: Query<(&Module, Option<&ModuleEfficiency>)>,
    power_graph: Res<PowerGraph>,
    mut power_state: ResMut<PowerState>,
    mut power_events: EventWriter<PowerStateChanged>,
) {
    let mut total_generation = 0.0;
    let mut total_consumption = 0.0;

    for (module, eff) in module_query.iter() {
        if !module.is_active {
            continue;
        }

        let efficiency = effective_efficiency(module, eff);

        // Power generators self-power (always active if health > 0)
        if module.power_generation > 0.0 {
            total_generation += module.power_generation * efficiency;
            continue;
        }

        // Power consumers only active if they have power via the graph
        if power_graph.powered_tiles.contains(&module.grid_position) {
            total_consumption += module.power_consumption * efficiency;
        }
    }

    let new_balance = total_generation - total_consumption;
    let was_critical = power_state.power_balance < 0.0;
    let is_critical = new_balance < 0.0;

    power_state.total_power_generation = total_generation;
    power_state.total_power_consumption = total_consumption;
    power_state.power_balance = new_balance;

    // Fire event if power state changed critically
    if was_critical != is_critical {
        power_events.send(PowerStateChanged {
            new_balance,
            is_critical,
        });
    }
}

/// Manages reactor heat warnings, auto-shutdown, and explosion.
/// Heat generation and cooling are now handled by the heat network (heat.rs).
/// Reactor.heat is synced from ModuleTemperature by heat::sync_reactor_heat.
pub fn update_reactor_heat(
    mut reactor_query: Query<(&mut Reactor, &mut Module)>,
    mut notifications: EventWriter<ShowNotification>,
    mut warned_70: Local<bool>,
    mut warned_90: Local<bool>,
) {
    for (mut reactor, mut module) in reactor_query.iter_mut() {
        if !module.is_active {
            continue;
        }

        let heat_pct = reactor.heat / reactor.max_heat;

        // Warning at 70%
        if heat_pct >= 0.7 && !*warned_70 {
            *warned_70 = true;
            notifications.send(ShowNotification {
                message: "Reactor heat at 70%! Consider reducing power output.".into(),
                notification_type: NotificationType::Warning,
                duration: 3.0,
            });
        }
        if heat_pct < 0.65 {
            *warned_70 = false;
        }

        // Warning at 90%
        if heat_pct >= 0.9 && !*warned_90 {
            *warned_90 = true;
            notifications.send(ShowNotification {
                message: "REACTOR HEAT CRITICAL (90%)! Shutdown imminent!".into(),
                notification_type: NotificationType::Danger,
                duration: 4.0,
            });
        }
        if heat_pct < 0.85 {
            *warned_90 = false;
        }

        // Auto-shutdown at 100% (only notify when transitioning from active to inactive)
        if heat_pct >= 1.0 {
            if module.is_active {
                module.is_active = false;
                notifications.send(ShowNotification {
                    message: "Reactor auto-shutdown! Overheated!".into(),
                    notification_type: NotificationType::Danger,
                    duration: 4.0,
                });
            }
            reactor.heat = reactor.max_heat;
        }

        // Explosion if heat exceeds 110% on explosion-risk reactors
        if reactor.explosion_risk && reactor.heat > reactor.max_heat * 1.1 {
            module.health = 0.0;
            module.is_active = false;
            reactor.heat = 0.0;
        }
    }
}
