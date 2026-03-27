pub mod components;
mod layouts;
pub mod spawner;
mod ai_brain;
mod movement;
mod combat;
mod noise;
mod wreck;
pub mod simulation;

use bevy::prelude::*;

use crate::components::*;
use crate::building::registry::ModuleRegistry;
use crate::states::GameState;

use components::*;

pub struct AiSubmarinePlugin;

impl Plugin for AiSubmarinePlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<WorldSimulation>()
            .add_systems(OnEnter(GameState::Exploring), simulation::init_world_simulation)
            .add_systems(
                Update,
                (
                    simulation::tick_world_simulation,
                    simulation::sync_simulation_entities,
                    ai_brain::ai_brain_system,
                    movement::ai_sub_movement_system,
                    movement::ai_ballast_system,
                    movement::ai_fuel_system,
                    combat::ai_weapon_fire_system,
                    combat::process_ai_sub_damage_system,
                    noise::ai_sub_noise_system,
                    noise::ai_sub_noise_trail_system,
                    noise::ai_sub_sonar_contact_decay,
                    wreck::ai_sub_death_system,
                )
                    .run_if(in_state(GameState::Exploring)),
            );
    }
}
