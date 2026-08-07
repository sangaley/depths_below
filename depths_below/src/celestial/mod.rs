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
pub mod galaxy;

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
            .init_resource::<resources::GalaxyMap>()
            .init_resource::<resources::SystemStreamingManager>()
            .init_resource::<resources::PendingGalaxyWarpTarget>()
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
                galaxy::passive_proximity_discovery_system,
            ).run_if(in_state(GameState::Exploring)))
            // Spawn initial star system on entering Exploring — chained so
            // the galaxy layout always exists before Haven tries to read
            // its assigned StarSystemDef from it.
            .add_systems(OnEnter(GameState::Exploring), (galaxy::generate_galaxy_on_enter, spawn_initial_system).chain())
        ;
    }
}

/// Spawn the first star system (Haven) when the player starts exploring.
/// Delegates the actual generation to galaxy::spawn_system_contents so
/// Haven spawns through the exact same deterministic path every other
/// system does — this used to have its own inline copy of the seeded-RNG
/// spawn sequence, which only invited the two from drifting apart.
fn spawn_initial_system(
    mut commands: Commands,
    mut galaxy: ResMut<resources::GalaxyState>,
    mut galaxy_map: ResMut<resources::GalaxyMap>,
    textures: Res<crate::vfx::procedural_textures::CelestialTextures>,
    asset_server: Res<AssetServer>,
    time: Res<Time>,
) {
    // Only spawn if no systems exist yet
    if !galaxy.systems.is_empty() {
        return;
    }
    if galaxy_map.systems.is_empty() {
        return; // galaxy::generate_galaxy_on_enter hasn't run yet this frame
    }

    galaxy::catch_up_system(&mut galaxy_map.systems[0], time.elapsed_secs_f64());
    let def = galaxy_map.systems[0].clone();
    let system_info = galaxy::spawn_system_contents(&mut commands, &asset_server, &textures, &def);

    galaxy.next_system_id = def.id + 1;
    galaxy.systems.push(system_info);
    galaxy.total_bodies = 1;
}
