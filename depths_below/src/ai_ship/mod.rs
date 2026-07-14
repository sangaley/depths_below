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

use crate::states::{GameState, SpatialSet};

use components::*;

pub struct AiShipPlugin;

impl Plugin for AiShipPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<WorldSimulation>()
            .add_systems(OnEnter(GameState::Exploring), simulation::init_world_simulation)
            .add_systems(
                Update,
                (
                    simulation::tick_world_simulation,
                    simulation::sync_simulation_entities,
                    // spawn_raider_waves removed — was periodically teleporting a
                    // hostile wing in right next to the player regardless of
                    // location, which was most of what made fights feel
                    // constant/unavoidable. Territories + roaming wanderers are
                    // the encounter source now; ships stay on their own patrols.
                    ai_brain::ai_brain_system,
                    movement::ai_ship_movement_system,
                    movement::ai_thruster_system,
                    movement::ai_fuel_system,
                    combat::ai_weapon_fire_system,
                    combat::process_ai_ship_damage_system,
                    combat::check_ai_reactor_destruction,
                    noise::ai_ship_noise_system,
                    noise::ai_ship_noise_trail_system,
                    noise::ai_ship_radar_contact_decay,
                    wreck::ai_ship_death_system,
                )
                    .after(SpatialSet::Update)
                    .run_if(in_state(GameState::Exploring)),
            );
    }
}
