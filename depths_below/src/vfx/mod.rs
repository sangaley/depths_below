pub mod celestial_visuals;
pub mod particles;
pub mod screen_effects;
pub mod block_visuals;

use bevy::prelude::*;
use crate::states::GameState;

pub struct VfxPlugin;

impl Plugin for VfxPlugin {
    fn build(&self, app: &mut App) {
        app
            .add_systems(
                Update,
                (
                    celestial_visuals::animate_star_glow,
                    celestial_visuals::animate_star_flare_buildup,
                    celestial_visuals::animate_black_hole_disk,
                    celestial_visuals::animate_planet_atmosphere,
                    particles::spawn_engine_particles,
                    particles::spawn_breach_particles,
                    particles::update_particles,
                    screen_effects::update_screen_effects,
                ).run_if(in_state(GameState::Exploring)),
            )
            // Spawn visual layers when celestial bodies are created
            .add_systems(
                Update,
                (
                    celestial_visuals::attach_star_visuals,
                    celestial_visuals::attach_planet_visuals,
                    celestial_visuals::attach_black_hole_visuals,
                ).run_if(in_state(GameState::Exploring)),
            )
            // Block visuals — attach unique look to every placed module
            .add_systems(
                Update,
                block_visuals::attach_block_visuals,
            );
    }
}
