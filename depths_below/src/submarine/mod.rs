use bevy::prelude::*;

mod movement;
mod systems;
mod power;
mod oxygen;
mod pressure;
mod hull;
mod spawner;
pub mod flooding;
pub mod damage;
pub mod fire;
mod atmosphere;
mod subsystems;
mod heat;
mod logistics;

pub use movement::*;
pub use systems::*;
pub use power::*;
pub use oxygen::*;
pub use pressure::*;
pub use hull::*;
pub use spawner::*;
pub use flooding::*;
#[allow(unused_imports)]
pub use damage::*;
#[allow(unused_imports)]
pub use fire::*;
pub use atmosphere::*;

use crate::states::{GameState, SubmarineSet};
use crate::resources::{DepthState, PowerState, OxygenState, HullState, NoiseState, GameConfig, FuelState, VictoryState, ExploringSessionTimer, PowerGraph, HeatNetworkState, ResearchState, AutopilotState, TargetingBonus};
use crate::crew::spawn_starter_crew;

pub struct SubmarinePlugin;

impl Plugin for SubmarinePlugin {
    fn build(&self, app: &mut App) {
        app
            // Resources
            .init_resource::<DepthState>()
            .init_resource::<PowerState>()
            .init_resource::<OxygenState>()
            .init_resource::<HullState>()
            .init_resource::<NoiseState>()
            .init_resource::<GameConfig>()
            .init_resource::<FuelState>()
            .init_resource::<VictoryState>()
            .init_resource::<ExploringSessionTimer>()
            .init_resource::<PowerGraph>()
            .init_resource::<HeatNetworkState>()
            .init_resource::<ResearchState>()
            .init_resource::<AutopilotState>()
            .init_resource::<TargetingBonus>()
            .init_resource::<AtmosphereState>()

            // Configure set ordering
            .configure_set(Update, SubmarineSet::Input.run_if(in_state(GameState::Exploring)))
            .configure_set(Update, SubmarineSet::Movement.after(SubmarineSet::Input).run_if(in_state(GameState::Exploring)))
            .configure_set(Update, SubmarineSet::Physics.after(SubmarineSet::Movement).run_if(in_state(GameState::Exploring)))
            .configure_set(Update, SubmarineSet::Power.after(SubmarineSet::Physics).run_if(in_state(GameState::Exploring)))
            .configure_set(Update, SubmarineSet::Heat.after(SubmarineSet::Power).run_if(in_state(GameState::Exploring)))
            .configure_set(Update, SubmarineSet::Oxygen.after(SubmarineSet::Heat).run_if(in_state(GameState::Exploring)))
            .configure_set(Update, SubmarineSet::Hull.after(SubmarineSet::Oxygen).run_if(in_state(GameState::Exploring)))
            .configure_set(Update, SubmarineSet::State.after(SubmarineSet::Hull).run_if(in_state(GameState::Exploring)))

            // Startup - spawn submarine, flush commands, then spawn crew (crew needs submarine entity)
            .add_systems(OnEnter(GameState::StationDocked), (
                (spawn_starter_submarine, reset_victory_state),
                apply_deferred,
                spawn_starter_crew,
            ).chain())

            // Update systems that run while exploring
            .add_systems(
                Update,
                (
                    submarine_input.in_set(SubmarineSet::Input),
                    submarine_movement.in_set(SubmarineSet::Movement),
                    update_depth.in_set(SubmarineSet::Movement),
                    check_radiation_damage.in_set(SubmarineSet::Physics),
                    build_power_graph.in_set(SubmarineSet::Power),
                    update_power_system.after(build_power_graph).in_set(SubmarineSet::Power),
                    update_reactor_heat.after(update_power_system).in_set(SubmarineSet::Power),
                    update_fuel_consumption.after(update_power_system).in_set(SubmarineSet::Power),
                    update_oxygen_system.in_set(SubmarineSet::Oxygen),
                    update_submarine_state.in_set(SubmarineSet::State),
                    check_game_over.in_set(SubmarineSet::State),
                    check_victory.in_set(SubmarineSet::State),
                    update_inventory_capacity.in_set(SubmarineSet::State),
                    update_statistics.in_set(SubmarineSet::State),
                    tick_session_timer.in_set(SubmarineSet::Input),
                ),
            )

            // Hull damage chain: damage → destruction → detonation → fire → cascade → integrity
            .add_systems(
                Update,
                (
                    damage::process_submarine_damage,
                    damage::process_module_destruction.after(damage::process_submarine_damage),
                    damage::queue_detonation.after(damage::process_module_destruction),
                    damage::process_detonations.after(damage::queue_detonation),
                    fire::apply_fire_ignition.after(damage::process_detonations),
                    fire::update_fire.after(fire::apply_fire_ignition),
                    hull::process_hull_cascade.after(damage::process_detonations),
                    update_hull_integrity.after(hull::process_hull_cascade).after(fire::update_fire),
                    update_decompression.after(hull::process_hull_cascade),
                    seal_breach_system.after(update_decompression),
                    fire::emergency_bulkhead_system.after(update_decompression),
                    handle_bulkhead_toggle,
                    bulkhead_seal_input,
                ).in_set(SubmarineSet::Hull),
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
                ).in_set(SubmarineSet::Heat),
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
                ).in_set(SubmarineSet::State),
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
