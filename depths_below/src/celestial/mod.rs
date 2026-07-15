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
            .add_message::<events::RadiationFlare>()
            .add_message::<events::StarDestroyed>()
            .add_message::<events::PlanetConsumed>()
            .add_message::<events::BodyConsumed>()
            .add_message::<events::GravityWarning>()
            .add_message::<events::SupernovaShockwave>()
            .add_message::<events::WarpJumpStarted>()
            .add_message::<events::WarpJumpCompleted>()
            // System set ordering
            .configure_sets(Update, CelestialSet::Orbits.run_if(in_state(GameState::Exploring)))
            .configure_sets(Update, CelestialSet::Gravity.after(CelestialSet::Orbits).run_if(in_state(GameState::Exploring)))
            .configure_sets(Update, CelestialSet::Forces.after(CelestialSet::Gravity).run_if(in_state(GameState::Exploring)))
            .configure_sets(Update, CelestialSet::StarLogic.after(CelestialSet::Forces).run_if(in_state(GameState::Exploring)))
            .configure_sets(Update, CelestialSet::BlackHoles.after(CelestialSet::StarLogic).run_if(in_state(GameState::Exploring)))
            .configure_sets(Update, CelestialSet::Cleanup.after(CelestialSet::BlackHoles).run_if(in_state(GameState::Exploring)))
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
                gravity::apply_gravity_to_ship,
            ).in_set(CelestialSet::Forces))
            // Star logic
            // stars::star_flare_buildup / apply_flare_radiation removed —
            // radiation mechanic disabled per request. (Their damage events
            // were already routed to a dead end: process_ship_damage skips
            // any DamageSource::Radiation event, having been written for the
            // old check_radiation_damage's direct-application model — so
            // this only ever cost a misleading "radiation spike" warning.)
            .add_systems(Update, (
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
    textures: Res<crate::vfx::procedural_textures::CelestialTextures>,
    asset_server: Res<AssetServer>,
) {
    // Only spawn if no systems exist yet
    if !galaxy.systems.is_empty() {
        return;
    }

    // Far below-right of the station start area. Star radii run 40k-150k and
    // their color/gravity influence extends to 4x that — the old center of
    // (0, -5000) put the starting station INSIDE the star's body, washing
    // the whole starting area in the star's warm tint (and rendering the
    // station on top of the star sprite). Distance is progression: the sun
    // is a destination, not a spawn point.
    let center = Vec2::new(200_000.0, -450_000.0);
    let system_id = galaxy.next_system_id;
    galaxy.next_system_id += 1;

    let system_info = spawning::spawn_star_system(
        &mut commands,
        &asset_server,
        center,
        system_id,
        42, // Seed for first system
        &textures,
    );

    // Also spawn some asteroids
    spawning::spawn_asteroid_field(
        &mut commands,
        &asset_server,
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
