//! Heat network system: generation, diffusion, cooling, and damage.
//!
//! Heat spreads between adjacent modules on the grid. Reactors, engines, and
//! weapons generate heat; CoolingPumps and HeatVents remove it. Overheated
//! modules take damage and may catch fire.
//!
//! PLAYER SHIP ONLY: the heat map is keyed by ship-local grid coordinates,
//! and AI ships reuse the same local coordinates (their reactor also sits
//! near (0,0)). Before scoping, every spawned AI ship's reactors dumped heat
//! into the player's mid-section tiles, silently cooking the player's
//! modules. AI ships use their simplified hull-integrity damage model and
//! don't participate in heat simulation.

use bevy::prelude::*;
use crate::components::*;
use crate::resources::*;
use crate::events::*;

/// Sync ModuleTemperature components → HeatNetworkState HashMap.
/// On first encounter, seed the map from the component; thereafter the map
/// is authoritative (updated by diffusion/cooling) and writes back.
pub fn sync_module_temperatures(
    mut heat_state: ResMut<HeatNetworkState>,
    module_query: Query<(&Module, &ModuleTemperature, &ChildOf)>,
    ship_query: Query<Entity, With<Ship>>,
) {
    let Ok(player_ship) = ship_query.single() else { return };
    // Seed any new modules into the map
    for (module, temp, parent) in module_query.iter() {
        if parent.parent() != player_ship { continue; }
        heat_state.temperatures
            .entry(module.grid_position)
            .or_insert(temp.current);
    }
}

/// Heat generation: reactors, engines (when active), weapons (on cooldown).
pub fn generate_heat(
    time: Res<Time>,
    mut heat_state: ResMut<HeatNetworkState>,
    reactor_query: Query<(&Reactor, &Module, &ChildOf), Without<DestroyedModule>>,
    engine_query: Query<(&Engine, &Module, &ChildOf), Without<DestroyedModule>>,
    weapon_query: Query<(&WeaponCooldown, &Module, &ChildOf), Without<DestroyedModule>>,
    ship_query: Query<Entity, With<Ship>>,
) {
    let Ok(player_ship) = ship_query.single() else { return };
    let dt = time.delta_secs();

    // Reactors generate heat proportional to output. Tuned against a
    // reactor's own ModuleTemperature.max_temp (100, set in spawner.rs) and
    // the 5.0/s flat ambient cooling in apply_cooling: at the old 8.0
    // multiplier a Standard Reactor (output 500) generated ~40 heat/s against
    // only 5/s of passive removal, blowing through max_temp in a few
    // seconds — reactors cooked themselves on startup, combat or not.
    for (reactor, module, parent) in reactor_query.iter() {
        if parent.parent() != player_ship { continue; }
        if !module.is_active { continue; }
        let heat_gain = (reactor.output / 100.0) * 1.5 * dt;
        *heat_state.temperatures.entry(module.grid_position).or_insert(0.0) += heat_gain;
    }

    // Active engines generate some heat
    for (engine, module, parent) in engine_query.iter() {
        if parent.parent() != player_ship { continue; }
        if !module.is_active { continue; }
        let heat_gain = (engine.thrust / 100.0) * 2.0 * dt;
        *heat_state.temperatures.entry(module.grid_position).or_insert(0.0) += heat_gain;
    }

    // Weapons generate heat spike when cooling down (just fired)
    for (cooldown, module, parent) in weapon_query.iter() {
        if parent.parent() != player_ship { continue; }
        if !module.is_active { continue; }
        if !cooldown.timer.is_finished() {
            // Currently cooling = recently fired
            let heat_gain = 3.0 * dt;
            *heat_state.temperatures.entry(module.grid_position).or_insert(0.0) += heat_gain;
        }
    }
}

/// Diffuse heat between adjacent grid tiles. Heat is conserved.
pub fn diffuse_heat(
    time: Res<Time>,
    mut heat_state: ResMut<HeatNetworkState>,
    temp_query: Query<(&Module, &ModuleTemperature, &ChildOf)>,
    ship_query: Query<Entity, With<Ship>>,
) {
    let Ok(player_ship) = ship_query.single() else { return };
    let dt = time.delta_secs();

    // Build conductivity map (player modules only — AI local coords collide)
    let mut conductivity_map: std::collections::HashMap<IVec2, f32> = std::collections::HashMap::new();
    for (module, temp, parent) in temp_query.iter() {
        if parent.parent() != player_ship { continue; }
        conductivity_map.insert(module.grid_position, temp.conductivity);
    }

    // Snapshot current temperatures into prev for reading
    heat_state.prev_temperatures = heat_state.temperatures.clone();

    // Compute deltas into a separate map to avoid borrow conflicts
    let mut deltas: Vec<(IVec2, f32)> = Vec::new();
    let offsets = [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y];

    for (&pos, &temp) in heat_state.prev_temperatures.iter() {
        if temp <= 0.0 { continue; }
        let conductivity = conductivity_map.get(&pos).copied().unwrap_or(0.5);
        let transfer_rate = conductivity * 0.1 * dt;

        for offset in &offsets {
            let neighbor = pos + *offset;
            if let Some(&neighbor_temp) = heat_state.prev_temperatures.get(&neighbor) {
                let delta = (temp - neighbor_temp) * transfer_rate;
                if delta > 0.0 {
                    deltas.push((pos, -delta));
                    deltas.push((neighbor, delta));
                }
            }
        }
    }

    // Apply deltas
    for (pos, delta) in deltas {
        *heat_state.temperatures.entry(pos).or_insert(0.0) += delta;
    }
}

/// Apply cooling: CoolingPumps, HeatVents, and ambient environmental cooling.
pub fn apply_cooling(
    time: Res<Time>,
    depth_state: Res<DepthState>,
    mut heat_state: ResMut<HeatNetworkState>,
    cooling_query: Query<(&CoolingPumpComp, &Module, &ChildOf), Without<DestroyedModule>>,
    vent_query: Query<(&HeatVentComp, &Module, &ChildOf), Without<DestroyedModule>>,
    ship_query: Query<Entity, With<Ship>>,
) {
    let Ok(player_ship) = ship_query.single() else { return };
    let dt = time.delta_secs();
    let offsets = [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y];

    // CoolingPumps: remove heat from adjacent tiles
    for (pump, module, parent) in cooling_query.iter() {
        if parent.parent() != player_ship { continue; }
        if !module.is_active { continue; }
        let cooling_per_neighbor = pump.cooling_rate * dt / 4.0;
        for offset in &offsets {
            let neighbor = module.grid_position + *offset;
            if let Some(temp) = heat_state.temperatures.get_mut(&neighbor) {
                *temp = (*temp - cooling_per_neighbor).max(0.0);
            }
        }
    }

    // HeatVents: dissipate own tile heat, scaled by distance (deeper void = better radiative cooling)
    for (vent, module, parent) in vent_query.iter() {
        if parent.parent() != player_ship { continue; }
        if !module.is_active { continue; }
        let depth_bonus = 1.0 + (depth_state.current_depth / 500.0).min(2.0);
        let dissipation = vent.dissipation_rate * depth_bonus * dt;
        if let Some(temp) = heat_state.temperatures.get_mut(&module.grid_position) {
            *temp = (*temp - dissipation).max(0.0);
        }
    }

    // Ambient environmental cooling: all tiles lose heat passively
    let ambient = 5.0 * dt;
    for temp in heat_state.temperatures.values_mut() {
        *temp = (*temp - ambient).max(0.0);
    }
}

/// Apply heat damage to overheated modules. Fire risk at extreme temps.
pub fn apply_heat_damage(
    time: Res<Time>,
    heat_state: Res<HeatNetworkState>,
    mut module_query: Query<(Entity, &mut Module, &ModuleTemperature, Option<&OnFire>, &ChildOf), Without<DestroyedModule>>,
    ship_query: Query<Entity, With<Ship>>,
    mut commands: Commands,
    mut notifications: MessageWriter<ShowNotification>,
    mut warned: Local<bool>,
) {
    let Ok(player_ship) = ship_query.single() else { return };
    let dt = time.delta_secs();

    for (entity, mut module, temp, on_fire, parent) in module_query.iter_mut() {
        if parent.parent() != player_ship { continue; }
        let current = heat_state.temperatures
            .get(&module.grid_position)
            .copied()
            .unwrap_or(temp.current);

        if current <= temp.max_temp * 0.8 {
            continue;
        }

        if current > temp.max_temp {
            // Overheat damage
            let damage = (current - temp.max_temp) * 0.5 * dt;
            module.health = (module.health - damage).max(0.0);

            if !*warned {
                *warned = true;
                notifications.write(ShowNotification {
                    message: "Module overheating! Deploy cooling systems.".into(),
                    notification_type: NotificationType::Warning,
                    duration: 3.0,
                });
            }
        }

        // Fire risk at 150% max_temp
        if current > temp.max_temp * 1.5 && on_fire.is_none() {
            let fire_chance = 0.1 * dt;
            let hash = (module.grid_position.x.wrapping_mul(73) ^ module.grid_position.y.wrapping_mul(179)) as f32;
            let pseudo_rand = ((hash + current * 100.0) % 1000.0) / 1000.0;
            if pseudo_rand < fire_chance {
                commands.entity(entity).insert(OnFire {
                    intensity: 0.5,
                    damage_per_second: 4.0,
                    spread_timer: Timer::from_seconds(5.0, TimerMode::Repeating),
                    duration: Timer::from_seconds(30.0, TimerMode::Once),
                });
            }
        }

        if current <= temp.max_temp {
            *warned = false;
        }
    }
}

/// Bridge: keep existing Reactor.heat in sync with the heat network.
/// Reactor warnings, shutdown, and explosion logic in power.rs reads reactor.heat,
/// so we write the heat network temperature back to it.
pub fn sync_reactor_heat(
    heat_state: Res<HeatNetworkState>,
    mut reactor_query: Query<(&mut Reactor, &Module, &ChildOf), Without<DestroyedModule>>,
    ship_query: Query<Entity, With<Ship>>,
) {
    let Ok(player_ship) = ship_query.single() else { return };
    for (mut reactor, module, parent) in reactor_query.iter_mut() {
        if parent.parent() != player_ship { continue; }
        if let Some(&temp) = heat_state.temperatures.get(&module.grid_position) {
            reactor.heat = temp;
        }
    }
}

/// Write final heat network temperatures back to ModuleTemperature components.
pub fn sync_temperatures_back(
    heat_state: Res<HeatNetworkState>,
    mut temp_query: Query<(&Module, &mut ModuleTemperature, &ChildOf)>,
    ship_query: Query<Entity, With<Ship>>,
) {
    let Ok(player_ship) = ship_query.single() else { return };
    for (module, mut temp, parent) in temp_query.iter_mut() {
        if parent.parent() != player_ship { continue; }
        if let Some(&t) = heat_state.temperatures.get(&module.grid_position) {
            temp.current = t;
        }
    }
}
