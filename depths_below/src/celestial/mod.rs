pub mod components;
pub mod resources;
pub mod events;
pub mod gravity;
pub mod orbits;
pub mod stars;
pub mod black_holes;
pub mod spawning;
pub mod warp;
pub mod poi;

use bevy::prelude::*;
use crate::states::GameState;

pub struct CelestialPlugin;

/// System set for ordering celestial mechanics
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum CelestialSet {
    Orbits,
    Gravity,
    Forces,
    StarLogic,
    BlackHoles,
    Cleanup,
}

impl Plugin for CelestialPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<resources::GalaxyState>()
            .init_resource::<resources::CelestialConfig>()
            // Events
            .add_event::<events::RadiationFlare>()
            .add_event::<events::StarDestroyed>()
            .add_event::<events::PlanetConsumed>()
            .add_event::<events::BodyConsumed>()
            .add_event::<events::GravityWarning>()
            .add_event::<events::SupernovaShockwave>()
            .add_event::<events::WarpJumpStarted>()
            .add_event::<events::WarpJumpCompleted>()
            // System set ordering
            .configure_set(Update, CelestialSet::Orbits.run_if(in_state(GameState::Exploring)))
            .configure_set(Update, CelestialSet::Gravity.after(CelestialSet::Orbits).run_if(in_state(GameState::Exploring)))
            .configure_set(Update, CelestialSet::Forces.after(CelestialSet::Gravity).run_if(in_state(GameState::Exploring)))
            .configure_set(Update, CelestialSet::StarLogic.after(CelestialSet::Forces).run_if(in_state(GameState::Exploring)))
            .configure_set(Update, CelestialSet::BlackHoles.after(CelestialSet::StarLogic).run_if(in_state(GameState::Exploring)))
            .configure_set(Update, CelestialSet::Cleanup.after(CelestialSet::BlackHoles).run_if(in_state(GameState::Exploring)))
            // Orbital mechanics
            .add_systems(Update, (
                orbits::update_orbital_positions,
                orbits::update_free_flight,
            ).in_set(CelestialSet::Orbits))
            // Gravity accumulation
            .add_systems(Update,
                gravity::accumulate_gravity.in_set(CelestialSet::Gravity),
            )
            // Force application
            .add_systems(Update, (
                gravity::apply_gravity_to_velocity,
                gravity::apply_gravity_to_submarine,
            ).in_set(CelestialSet::Forces))
            // Star logic
            .add_systems(Update, (
                stars::star_flare_buildup,
                stars::apply_flare_radiation.after(stars::star_flare_buildup),
                stars::star_death_check,
                stars::apply_supernova_damage,
                orbits::destabilize_orbits.after(stars::star_death_check),
            ).in_set(CelestialSet::StarLogic))
            // Black hole logic
            .add_systems(Update, (
                black_holes::check_event_horizon,
                black_holes::process_consumption.after(black_holes::check_event_horizon),
                black_holes::grow_black_hole.after(black_holes::process_consumption),
            ).in_set(CelestialSet::BlackHoles))
            // Warp system (runs during exploring)
            .add_systems(Update, (
                warp::warp_input_system,
                warp::execute_warp_jump.after(warp::warp_input_system),
                warp::on_warp_complete.after(warp::execute_warp_jump),
                poi::mining_system,
                poi::loot_derelict_system,
            ).run_if(in_state(GameState::Exploring)))
            // Spawn initial star system on entering Exploring
            .add_systems(OnEnter(GameState::Exploring), spawn_initial_system)
        ;
    }
}

/// Spawn the first star system when the player starts exploring
fn spawn_initial_system(
    mut commands: Commands,
    mut galaxy: ResMut<resources::GalaxyState>,
) {
    // Only spawn if no systems exist yet
    if !galaxy.systems.is_empty() {
        return;
    }

    let center = Vec2::new(0.0, -5000.0); // Below the station start area
    let system_id = galaxy.next_system_id;
    galaxy.next_system_id += 1;

    let system_info = spawning::spawn_star_system(
        &mut commands,
        center,
        system_id,
        42, // Seed for first system
    );

    // Also spawn some asteroids
    spawning::spawn_asteroid_field(
        &mut commands,
        center + Vec2::new(50_000.0, 0.0),
        20,
        30_000.0,
        system_id,
    );

    // Spawn POIs
    let planet_positions: Vec<Vec2> = system_info.planet_entities.iter()
        .map(|_| center + Vec2::new(rand::random::<f32>() * 60_000.0 - 30_000.0, rand::random::<f32>() * 60_000.0 - 30_000.0))
        .collect();
    poi::spawn_system_pois(&mut commands, center, system_id, &planet_positions);

    galaxy.systems.push(system_info);
    galaxy.total_bodies = 1;
}
