use bevy::prelude::*;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::ai_ship::components::{faction_power, AiShipType};
use super::components::StarSystemMember;
use super::resources::{GalaxyMap, StarSystemDef, StarSystemInfo, SystemDiscovery, SystemStreamingManager};

/// How many nearest-neighbor systems stay Warm (real ticking background
/// simulation) around the currently-loaded (Hot) system. Small and fixed
/// regardless of galaxy size — see the plan's coordinate-model writeup.
pub const WARM_NEIGHBOR_COUNT: usize = 3;

/// Passive discovery range: any Unknown system within this distance of
/// wherever you're currently Hot quietly reveals as Located (a dim pip, no
/// details) — no scanning action needed, just proximity, Cosmoteer-style.
pub const SENSOR_RANGE: f32 = 900_000.0;

/// Active scan range: pressing the radar ping (Z) with an active detection
/// module sweeps interstellar space and reveals any Unknown system within this
/// distance as Located — much longer reach than passive proximity, so a scan
/// actually surfaces systems you couldn't see just by sitting still. Half the
/// galaxy radius: a scan lights up a big neighbourhood but never the whole map.
pub const GALAXY_SCAN_RANGE: f32 = 2_500_000.0;

/// How close a blind-warp target has to land to an actual system's
/// galaxy_pos to snap to arriving AT that system (revealing it fully)
/// instead of landing in genuinely empty space nearby. Forgiving on
/// purpose — the map's pips are small and clicking exactly on one of the
/// UNDISCOVERED ones (which aren't even rendered) would otherwise be
/// next to impossible.
pub const SNAP_TOLERANCE: f32 = 150_000.0;

/// Ambient passive depletion rate applied by catch_up_system, expressed as
/// fraction-per-second. ~33 minutes of real elapsed time (Cold or Warm, it
/// doesn't matter) fully exhausts an untouched system's resources — real,
/// permanent, no floor, per the plan's danger-model writeup. Tune by feel.
const AMBIENT_DEPLETION_PER_SECOND: f32 = 0.0005;

/// Non-Haven system count for this pass. 20-40 was the target range;
/// nothing below scales with this as a hardcoded assumption (it's just the
/// loop bound for generation), so raising it later to scale toward
/// "hundreds" doesn't require touching this module's logic.
pub const SYSTEM_COUNT: usize = 30;

/// Abstract galaxy-map radius (NOT a real Transform coordinate — see
/// StarSystemDef::galaxy_pos doc comment).
pub const GALAXY_RADIUS: f32 = 5_000_000.0;

const MIN_SYSTEM_SEPARATION: f32 = 400_000.0;
const MAX_PLACEMENT_ATTEMPTS: u32 = 200;

/// All 10 factions ordered weakest-to-strongest by faction_power. Used only
/// to bias WHICH systems are near vs. far (a placement heuristic for
/// pacing) — danger_tier itself is fixed per system once assigned, not a
/// continuous distance formula (that's exactly what this feature replaces).
fn faction_roster() -> Vec<AiShipType> {
    vec![
        AiShipType::GlassEye,
        AiShipType::RustSwarm,
        AiShipType::Drowned,
        AiShipType::Leviathan,
        AiShipType::AbyssalCult,
        AiShipType::Blackwater,
        AiShipType::PressureKing,
        AiShipType::IronTide,
        AiShipType::Dreadnought,
        AiShipType::VoidTitan,
    ]
}

/// Generates the persistent galaxy layout. System 0 is always Haven's home
/// system (fixed at galaxy-space origin, safe, pre-visited). The rest are
/// scattered via rejection sampling with a minimum separation so the map
/// doesn't clump, then sorted by distance from Haven so faction assignment
/// can bias weak-near/strong-far for pacing.
pub fn generate_galaxy_map(galaxy_seed: u64) -> GalaxyMap {
    let mut rng = StdRng::seed_from_u64(galaxy_seed);
    let roster = faction_roster();

    // Rejection-sampled scatter. O(n^2) worst case (each candidate checked
    // against every already-placed system) — trivial at n=30, still fine at
    // a few hundred; a grid-bucketed spatial index would only be needed far
    // beyond that.
    let mut positions: Vec<Vec2> = vec![Vec2::ZERO]; // Haven reserves the origin
    for _ in 0..SYSTEM_COUNT {
        let mut candidate = Vec2::ZERO;
        for _ in 0..MAX_PLACEMENT_ATTEMPTS {
            let angle = rng.gen_range(0.0..std::f32::consts::TAU);
            // sqrt of a uniform sample gives uniform area density instead of
            // clustering candidates near the center.
            let r = GALAXY_RADIUS * rng.gen_range(0.05f32..1.0).sqrt();
            let p = Vec2::new(angle.cos() * r, angle.sin() * r);
            if positions.iter().all(|existing| existing.distance(p) >= MIN_SYSTEM_SEPARATION) {
                candidate = p;
                break;
            }
        }
        positions.push(candidate);
    }

    let mut non_haven: Vec<Vec2> = positions[1..].to_vec();
    non_haven.sort_by(|a, b| a.length().partial_cmp(&b.length()).unwrap());

    let mut systems = Vec::with_capacity(SYSTEM_COUNT + 1);
    systems.push(StarSystemDef {
        id: 0,
        name: "Haven".to_string(),
        galaxy_pos: Vec2::ZERO,
        // Matches today's spawn_initial_system center exactly.
        local_center: Vec2::new(200_000.0, -450_000.0),
        seed: 42,
        faction: None,
        danger_tier: 0.0,
        discovery: SystemDiscovery::Visited,
        last_updated: 0.0,
        resource_fraction_remaining: 1.0,
    });

    // Every system's local_center lives in the SAME shared local coordinate
    // space (only ever one system is physically "Hot" at a time, so this
    // never causes a visual clash) — but the render-distance spawn check
    // that promotes a Warm system's abstract SimulatedShip into a real
    // entity compares raw positions in that same space. Two systems'
    // local_centers landing within render distance of each other by pure
    // chance would let a Warm neighbor's ships erroneously materialize
    // before the player ever actually travels there. Rejection-sample with
    // a wide separation margin (well beyond RENDER_DISTANCE/DESPAWN_DISTANCE,
    // ai_ship/simulation.rs) to rule that out.
    let mut local_centers: Vec<Vec2> = vec![Vec2::new(200_000.0, -450_000.0)]; // Haven's
    const LOCAL_MIN_SEPARATION: f32 = 200_000.0;
    const LOCAL_RANGE: f32 = 1_500_000.0;

    for (rank, pos) in non_haven.into_iter().enumerate() {
        let id = (rank + 1) as u32;
        let faction = roster[(rank * roster.len()) / SYSTEM_COUNT];

        let mut local_center = Vec2::ZERO;
        for _ in 0..MAX_PLACEMENT_ATTEMPTS {
            let candidate = Vec2::new(
                rng.gen_range(-LOCAL_RANGE..LOCAL_RANGE),
                rng.gen_range(-LOCAL_RANGE..0.0),
            );
            if local_centers.iter().all(|existing| existing.distance(candidate) >= LOCAL_MIN_SEPARATION) {
                local_center = candidate;
                break;
            }
        }
        local_centers.push(local_center);

        systems.push(StarSystemDef {
            id,
            name: format!("System-{:02}", id),
            galaxy_pos: pos,
            local_center,
            seed: rng.gen::<u64>(),
            faction: Some(faction),
            danger_tier: faction_power(faction),
            discovery: SystemDiscovery::Unknown,
            last_updated: 0.0,
            resource_fraction_remaining: 1.0,
        });
    }

    GalaxyMap { systems, galaxy_seed }
}

/// Generates the galaxy once per session, same guard pattern as today's
/// spawn_initial_system (celestial/mod.rs). Also seeds the streaming
/// manager: Haven (system 0) starts Hot, its nearest neighbors start Warm.
pub fn generate_galaxy_on_enter(
    mut galaxy_map: ResMut<GalaxyMap>,
    mut streaming: ResMut<SystemStreamingManager>,
) {
    if !galaxy_map.systems.is_empty() {
        return;
    }
    let seed = rand::random::<u64>();
    *galaxy_map = generate_galaxy_map(seed);
    streaming.loaded_system = Some(0);
    streaming.current_galaxy_pos = Vec2::ZERO;
    streaming.warm_systems = nearest_neighbors(&galaxy_map, 0, WARM_NEIGHBOR_COUNT);
    info!("Galaxy generated: {} systems, seed={}", galaxy_map.systems.len(), galaxy_map.galaxy_seed);
}

/// Bevy system wrapper for passive_proximity_discovery — runs continuously
/// while Exploring, not gated behind any player action.
pub fn passive_proximity_discovery_system(
    mut galaxy_map: ResMut<GalaxyMap>,
    streaming: Res<SystemStreamingManager>,
) {
    passive_proximity_discovery(&mut galaxy_map, streaming.current_galaxy_pos);
}

/// Distance a system's faction population cluster sits away from the star
/// itself. Stars run 40k-150k radius (StarSizeClass::radius) but several
/// faction territories are smaller than that (RustSwarm's is only 15k) — if
/// the cluster were centered ON the star like the star's own position, the
/// player (and the ships) would routinely be inside or right on top of the
/// star. Offsetting the cluster clear of any star size decouples the two
/// entirely.
pub const FACTION_CLUSTER_OFFSET: f32 = 220_000.0;

/// Where a system's faction population is actually centered — offset from
/// the star (system.local_center) so it never overlaps it (see
/// FACTION_CLUSTER_OFFSET doc comment). Deterministic per system (derived
/// from its id, not randomized per call) so it's stable every time the
/// system is loaded, and spreads the offset DIRECTION across systems so
/// they don't all put their cluster in the exact same relative spot.
pub fn faction_cluster_center(system: &StarSystemDef) -> Vec2 {
    let angle = (system.id as f32 * 2.399963).rem_euclid(std::f32::consts::TAU);
    system.local_center + Vec2::new(angle.cos(), angle.sin()) * FACTION_CLUSTER_OFFSET
}

/// Nearest `count` other systems to a raw galaxy-space position — the
/// general form behind both `nearest_neighbors` (Warm-tier selection around
/// a loaded system) and blind-warp arrival (nothing to exclude by id).
/// O(n log n) in galaxy size, trivial at n=30, still fine at a few hundred.
pub fn nearest_neighbors_to_pos(galaxy_map: &GalaxyMap, pos: Vec2, exclude_id: Option<u32>, count: usize) -> Vec<u32> {
    let mut others: Vec<(u32, f32)> = galaxy_map.systems.iter()
        .filter(|s| Some(s.id) != exclude_id)
        .map(|s| (s.id, s.galaxy_pos.distance(pos)))
        .collect();
    others.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    others.into_iter().take(count).map(|(id, _)| id).collect()
}

/// Nearest `count` other systems to `system_id` by galaxy_pos distance — the
/// Warm tier (see SystemStreamingManager doc comment).
pub fn nearest_neighbors(galaxy_map: &GalaxyMap, system_id: u32, count: usize) -> Vec<u32> {
    let Some(origin) = galaxy_map.systems.iter().find(|s| s.id == system_id) else {
        return Vec::new();
    };
    nearest_neighbors_to_pos(galaxy_map, origin.galaxy_pos, Some(system_id), count)
}

/// If a blind-warp target lands within SNAP_TOLERANCE of an actual system
/// (Unknown or otherwise), returns its id — the jump snaps to arriving AT
/// that system instead of at the raw empty-space point.
pub fn system_within_snap_tolerance(galaxy_map: &GalaxyMap, pos: Vec2) -> Option<u32> {
    galaxy_map.systems.iter()
        .filter(|s| s.galaxy_pos.distance(pos) <= SNAP_TOLERANCE)
        .min_by(|a, b| a.galaxy_pos.distance(pos).partial_cmp(&b.galaxy_pos.distance(pos)).unwrap())
        .map(|s| s.id)
}

/// Passive proximity discovery: any Unknown system within SENSOR_RANGE of
/// wherever the player currently is (Hot system, or a blind-space position —
/// see SystemStreamingManager.loaded_system) reveals as Located. Runs
/// continuously while Exploring, not gated behind any scan action —
/// Cosmoteer-style ambient fog-of-war peel-back.
pub fn passive_proximity_discovery(galaxy_map: &mut GalaxyMap, current_pos: Vec2) {
    for system in galaxy_map.systems.iter_mut() {
        if system.discovery == SystemDiscovery::Unknown && system.galaxy_pos.distance(current_pos) <= SENSOR_RANGE {
            system.discovery = SystemDiscovery::Located;
        }
    }
}

/// Deterministic local-space arrival point for a blind warp that lands in
/// genuinely empty space (no system within snap tolerance) — doesn't need
/// the same separation guarantees real systems' local_centers do, since
/// nothing else ever spawns there; just needs to vary with where in the
/// galaxy the blind point actually was instead of always landing in one
/// fixed void.
pub fn blind_point_local_center(galaxy_pos: Vec2) -> Vec2 {
    Vec2::new(
        (galaxy_pos.x * 0.15).rem_euclid(1_000_000.0) - 500_000.0,
        -((galaxy_pos.y.abs() * 0.15).rem_euclid(700_000.0)) - 200_000.0,
    )
}

/// Brings a system's ambient resource depletion up to date for however long
/// it's been since it was last checked — the Cold-tier "life happens in the
/// background" math (see the plan's coordinate-model writeup). Real and
/// permanent: no floor keeping it above zero. Called whenever a system
/// needs a genuine answer (loading it, or unloading it to stamp the
/// timestamp for next time), never on a per-frame timer.
pub fn catch_up_system(def: &mut StarSystemDef, now: f64) {
    let elapsed = (now - def.last_updated).max(0.0) as f32;
    def.resource_fraction_remaining = (def.resource_fraction_remaining - elapsed * AMBIENT_DEPLETION_PER_SECOND).max(0.0);
    def.last_updated = now;
}

/// Spawns a system's full contents (star, planets, asteroids, POIs)
/// deterministically from its seed — one continuous RNG stream shared
/// across all three spawn calls (see spawning.rs's seeding-fix doc
/// comments), scaling asteroid resource amounts by the system's current
/// depletion (catch_up_system should be called first by the caller so this
/// reflects up-to-date depletion, not a stale snapshot).
pub fn spawn_system_contents(
    commands: &mut Commands,
    asset_server: &AssetServer,
    textures: &crate::vfx::procedural_textures::CelestialTextures,
    def: &StarSystemDef,
) -> StarSystemInfo {
    let mut rng = StdRng::seed_from_u64(def.seed);

    let system_info = super::spawning::spawn_star_system(
        commands, asset_server, def.local_center, def.id, &mut rng, textures,
    );

    super::spawning::spawn_asteroid_field(
        commands, asset_server,
        def.local_center + Vec2::new(50_000.0, 0.0),
        20, 30_000.0, def.id, &mut rng,
        def.resource_fraction_remaining,
    );

    let planet_positions: Vec<Vec2> = system_info.planet_entities.iter()
        .map(|_| def.local_center + Vec2::new(rng.gen_range(-30_000.0..30_000.0), rng.gen_range(-30_000.0..30_000.0)))
        .collect();
    super::poi::spawn_system_pois(commands, def.local_center, def.id, &planet_positions, &mut rng);

    system_info
}

/// Despawns every entity tagged for `system_id` and brings its depletion
/// math up to date (stamping last_updated) before it goes Cold.
pub fn unload_system(
    commands: &mut Commands,
    member_query: &Query<(Entity, &StarSystemMember)>,
    galaxy_map: &mut GalaxyMap,
    system_id: u32,
    now: f64,
) {
    for (entity, member) in member_query.iter() {
        if member.system_id == system_id {
            commands.entity(entity).despawn();
        }
    }
    if let Some(def) = galaxy_map.systems.iter_mut().find(|s| s.id == system_id) {
        catch_up_system(def, now);
    }
}

/// Brings a system's depletion up to date, then spawns its contents. The
/// combination `unload_system` (old) + `load_system` (new) is what
/// celestial::warp::execute_warp_jump now does on a completed jump, instead
/// of despawning everything and rolling a brand-new random system.
pub fn load_system(
    commands: &mut Commands,
    asset_server: &AssetServer,
    textures: &crate::vfx::procedural_textures::CelestialTextures,
    galaxy_map: &mut GalaxyMap,
    system_id: u32,
    now: f64,
) -> Option<StarSystemInfo> {
    let def = galaxy_map.systems.iter_mut().find(|s| s.id == system_id)?;
    catch_up_system(def, now);
    def.discovery = SystemDiscovery::Visited;
    let def = galaxy_map.systems.iter().find(|s| s.id == system_id)?;
    Some(spawn_system_contents(commands, asset_server, textures, def))
}
