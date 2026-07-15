use bevy::prelude::*;

mod movement;
mod systems;
mod power;
mod oxygen;
mod radiation;
mod hull;
mod spawner;
pub mod decompression;
pub mod damage;
pub mod fire;
mod atmosphere;
mod subsystems;
mod heat;
mod logistics;
pub mod drill;

pub use movement::*;
pub use systems::*;
pub use power::*;
pub use oxygen::*;
pub use radiation::*;
pub use hull::*;
pub use spawner::*;
pub use decompression::*;
#[allow(unused_imports)]
pub use damage::*;
#[allow(unused_imports)]
pub use fire::*;
pub use atmosphere::*;

use crate::states::{GameState, ShipSet};
use crate::resources::{DepthState, PowerState, OxygenState, HullState, NoiseState, GameConfig, FuelState, VictoryState, DeathCause, ExploringSessionTimer, PowerGraph, HeatNetworkState, ResearchState, AutopilotState, TargetingBonus};
use crate::crew::spawn_starter_crew;

pub struct ShipPlugin;

impl Plugin for ShipPlugin {
    fn build(&self, app: &mut App) {
        app
            // Breaker Drill: contact wreck salvage (see drill.rs)
            .add_systems(Update, drill::wreck_drill_system.run_if(in_state(GameState::Exploring)))
            // Resources
            .init_resource::<DepthState>()
            .init_resource::<PowerState>()
            .init_resource::<OxygenState>()
            .init_resource::<HullState>()
            .init_resource::<NoiseState>()
            .init_resource::<GameConfig>()
            .init_resource::<FuelState>()
            .init_resource::<VictoryState>()
            .init_resource::<DeathCause>()
            .init_resource::<ExploringSessionTimer>()
            .init_resource::<PowerGraph>()
            .init_resource::<HeatNetworkState>()
            .init_resource::<ResearchState>()
            .init_resource::<AutopilotState>()
            .init_resource::<TargetingBonus>()
            .init_resource::<AtmosphereState>()

            // Configure set ordering
            .configure_sets(Update, ShipSet::Input.run_if(in_state(GameState::Exploring)))
            .configure_sets(Update, ShipSet::Movement.after(ShipSet::Input).run_if(in_state(GameState::Exploring)))
            .configure_sets(Update, ShipSet::Physics.after(ShipSet::Movement).run_if(in_state(GameState::Exploring)))
            .configure_sets(Update, ShipSet::Power.after(ShipSet::Physics).run_if(in_state(GameState::Exploring)))
            .configure_sets(Update, ShipSet::Heat.after(ShipSet::Power).run_if(in_state(GameState::Exploring)))
            .configure_sets(Update, ShipSet::Oxygen.after(ShipSet::Heat).run_if(in_state(GameState::Exploring)))
            .configure_sets(Update, ShipSet::Hull.after(ShipSet::Oxygen).run_if(in_state(GameState::Exploring)))
            .configure_sets(Update, ShipSet::State.after(ShipSet::Hull).run_if(in_state(GameState::Exploring)))

            // Startup - spawn ship, flush commands, then spawn crew (crew needs ship entity)
            .add_systems(OnEnter(GameState::StationDocked), (
                (spawn_starter_ship, reset_victory_state),
                ApplyDeferred,
                spawn_starter_crew,
            ).chain())

            // Update systems that run while exploring
            .add_systems(
                Update,
                (
                    ship_input.in_set(ShipSet::Input),
                    ship_movement.in_set(ShipSet::Movement),
                    update_depth.in_set(ShipSet::Movement),
                    // check_radiation_damage removed — radiation mechanic disabled per request.
                    build_power_graph.in_set(ShipSet::Power),
                    update_power_system.after(build_power_graph).in_set(ShipSet::Power),
                    update_reactor_heat.after(update_power_system).in_set(ShipSet::Power),
                    update_fuel_consumption.after(update_power_system).in_set(ShipSet::Power),
                    update_oxygen_system.in_set(ShipSet::Oxygen),
                    update_ship_state.in_set(ShipSet::State),
                    check_game_over.in_set(ShipSet::State),
                    check_victory.in_set(ShipSet::State),
                    update_inventory_capacity.in_set(ShipSet::State),
                    update_statistics.in_set(ShipSet::State),
                    tick_session_timer.in_set(ShipSet::Input),
                ),
            )

            // Hull damage chain: damage → destruction → detonation → fire → cascade → integrity
            .add_systems(
                Update,
                (
                    damage::process_ship_damage,
                    hull::tint_damaged_hull.after(damage::process_ship_damage),
                    hull::tint_destroyed_hull.after(hull::tint_damaged_hull),
                    damage::tint_damaged_modules.after(damage::process_ship_damage),
                    damage::process_module_destruction.after(damage::tint_damaged_modules),
                    damage::queue_detonation.after(damage::process_module_destruction),
                    damage::process_detonations.after(damage::queue_detonation),
                    // AI-ship blasts resolve in world space against the ship's
                    // own blocks (GridOccupancy only knows the player's grid)
                    damage::queue_ai_detonation.after(damage::process_module_destruction),
                    damage::process_ai_detonations.after(damage::queue_ai_detonation),
                    damage::explosion_shockwaves.after(damage::process_ai_detonations),
                    fire::apply_fire_ignition.after(damage::process_detonations),
                    fire::update_fire.after(fire::apply_fire_ignition),
                    hull::process_hull_cascade.after(damage::process_detonations),
                    update_hull_integrity.after(hull::process_hull_cascade).after(fire::update_fire),
                    update_decompression.after(hull::process_hull_cascade),
                    seal_breach_system.after(update_decompression),
                    fire::emergency_bulkhead_system.after(update_decompression),
                    handle_bulkhead_toggle,
                    bulkhead_seal_input,
                ).in_set(ShipSet::Hull),
            )

            // Destroyed-block removal: a short delay after tint_destroyed_hull /
            // process_module_destruction gives every other Added<HullDestroyed>/
            // Added<DestroyedModule> reader across plugins (severance, chain
            // reactions, reactor meltdown, detonation queueing) a full frame to
            // react before the block actually disappears.
            .add_systems(
                Update,
                (
                    hull::queue_hull_removal.after(hull::tint_destroyed_hull),
                    damage::queue_module_removal.after(damage::process_module_destruction),
                    damage::tick_pending_removal,
                ).in_set(ShipSet::Hull),
            )

            // Heat network (7 systems, chained)
            .add_systems(
                Update,
                (
                    heat::sync_module_temperatures,
                    heat::generate_heat.after(heat::sync_module_temperatures),
                    heat::diffuse_heat.after(heat::generate_heat),
                    heat::apply_cooling.after(heat::diffuse_heat),
                    heat::apply_heat_damage.after(heat::apply_cooling),
                    heat::sync_reactor_heat.after(heat::apply_heat_damage),
                    heat::sync_temperatures_back.after(heat::sync_reactor_heat),
                ).in_set(ShipSet::Heat),
            )

            // Subsystems (capacitors, fire suppression, radiation shielding, drones, torpedo loader, Phase B)
            .add_systems(
                Update,
                (
                    subsystems::update_capacitors,
                    subsystems::update_fire_suppression,
                    subsystems::update_radiation_shielding,
                    subsystems::update_drone_bays,
                    subsystems::apply_torpedo_loader_bonus,
                    subsystems::apply_targeting_computer_bonus,
                    subsystems::update_ai_combat_core,
                    subsystems::update_research_lab,
                    subsystems::maintenance_locker_boost,
                    logistics::update_conveyor_tubes,
                    logistics::update_fuel_processor,
                ).in_set(ShipSet::State),
            )

            // Atmospheric events (ambient immersion)
            .add_systems(Update, atmospheric_event_system.run_if(in_state(GameState::Exploring)))

            // Reset session timer when launching into void
            .add_systems(OnEnter(GameState::Exploring), reset_session_timer)

            // Cleanup when returning to main menu (after game over / restart)
            .add_systems(OnEnter(GameState::MainMenu), cleanup_game_entities)

            ;
    }
}
