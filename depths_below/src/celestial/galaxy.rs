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

/// Haven's local-space center. A constant because world::home_base needs it
/// before the galaxy has been generated: the game opens docked at Haven, a
/// couple of frames before OnEnter(Exploring) rolls the galaxy, and Haven's
/// station has to exist for that whole time.
/// Where Haven's star sits, in the shared local frame.
///
/// 75,000 units from `world::home_base::STATION_POS`, the fixed spot the ship
/// spawns beside. The old value was (200,000, -450,000) -- 492,000 from the
/// station -- so the player began half a million units OUTSIDE their own
/// solar system.
///
/// The window is narrow and both walls are solid. Haven's star is a dwarf of
/// radius 40,000 (seed 42), and the nearest a planet's inner EDGE can come
/// is `star_radius + ORBIT_GAP_MIN` = 120,000, whatever its size. So the
/// spawn has to sit inside (40,000, 120,000); 75,000 is near the middle of
/// it. An earlier pass had this at 95,000, which was fine until planets
/// doubled in size -- then the nearest possible edge moved inward to 85,000
/// and the station was sitting ON an orbit a solid body sweeps along.
///
/// This is the one hand-placed system in the galaxy, which is appropriate --
/// it is home, and the tutorial runs here.
pub const HAVEN_LOCAL_CENTER: Vec2 = Vec2::new(31_000.0, -68_400.0);

/// Abstract galaxy-map radius (NOT a real Transform coordinate — see
/// StarSystemDef::galaxy_pos doc comment).
pub const GALAXY_RADIUS: f32 = 5_000_000.0;

/// Closest two systems may sit on the galaxy map. Must stay above twice
/// `SNAP_TOLERANCE` (150,000), or one blind-warp click falls inside two
/// systems at once and which one you arrive at stops being predictable.
/// Tighter than the old 400,000 so a constellation reads as a group rather
/// than as more evenly-spread dots.
const MIN_SYSTEM_SEPARATION: f32 = 320_000.0;

/// How many constellations the systems are grouped into. Thirty systems over
/// seven groups averages four or five each — enough to read as a cluster,
/// few enough that the gaps between them are the dominant feature.
const CONSTELLATION_COUNT: usize = 7;

/// Smallest distance between two constellation centres. Comfortably more
/// than twice `CONSTELLATION_SPREAD` so neighbouring groups stay visibly
/// separate instead of smearing into one band.
const CONSTELLATION_SEPARATION: f32 = 1_500_000.0;

/// How far from its constellation's centre a system may sit.
const CONSTELLATION_SPREAD: f32 = 520_000.0;

/// Fraction of the galaxy radius the constellation centres are allowed to
/// reach, leaving the rim as genuine emptiness rather than a ring of systems
/// pressed against the edge of the map.
const CONSTELLATION_REACH: f32 = 0.82;
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

    // Constellations, not an even scatter. The old layout sampled uniform
    // area density across the whole disc with a flat 400,000 separation,
    // which produces the one thing a starmap must not be: evenly spaced. No
    // region read as anywhere in particular, so there was nothing to
    // recognise and nothing to aim at.
    //
    // Systems are grouped instead. Haven anchors the core, the other
    // constellation centres scatter core-dense, and each system is placed
    // near one of them -- so the map has crowded neighbourhoods with real
    // emptiness between, and warping "to the next constellation over" is a
    // thing you can see before you do it.
    let mut cluster_centers: Vec<Vec2> = vec![Vec2::ZERO]; // Haven anchors the core
    for _ in 1..CONSTELLATION_COUNT {
        let mut candidate = None;
        for _ in 0..MAX_PLACEMENT_ATTEMPTS {
            let angle = rng.gen_range(0.0..std::f32::consts::TAU);
            // `powf(0.9)` rather than the old `sqrt` (0.5, uniform area):
            // weights the draw toward small radii, so the middle of the
            // galaxy is denser than the rim the way a populated core is.
            let r = GALAXY_RADIUS * CONSTELLATION_REACH * rng.gen_range(0.2f32..1.0).powf(0.9);
            let p = Vec2::new(angle.cos() * r, angle.sin() * r);
            if cluster_centers.iter().all(|c| c.distance(p) >= CONSTELLATION_SEPARATION) {
                candidate = Some(p);
                break;
            }
        }
        // A centre that cannot be placed is dropped rather than defaulted.
        // The previous loop defaulted an unplaceable system to Vec2::ZERO,
        // which silently stacked it on top of Haven at the origin.
        if let Some(p) = candidate {
            cluster_centers.push(p);
        }
    }

    // O(n^2) worst case (each candidate checked against every already-placed
    // system) — trivial at n=30, still fine at a few hundred.
    let mut positions: Vec<Vec2> = vec![Vec2::ZERO]; // Haven reserves the origin
    for i in 0..SYSTEM_COUNT {
        let center = cluster_centers[i % cluster_centers.len()];
        let mut candidate = None;
        let mut last = center;
        for _ in 0..MAX_PLACEMENT_ATTEMPTS {
            let angle = rng.gen_range(0.0..std::f32::consts::TAU);
            let r = CONSTELLATION_SPREAD * rng.gen_range(0.15f32..1.0).sqrt();
            let p = center + Vec2::new(angle.cos() * r, angle.sin() * r);
            last = p;
            if positions.iter().all(|existing| existing.distance(p) >= MIN_SYSTEM_SEPARATION) {
                candidate = Some(p);
                break;
            }
        }
        // Fall back to the last candidate tried, never to the origin.
        positions.push(candidate.unwrap_or(last));
    }

    let mut non_haven: Vec<Vec2> = positions[1..].to_vec();
    non_haven.sort_by(|a, b| a.length().partial_cmp(&b.length()).unwrap());

    let mut systems = Vec::with_capacity(SYSTEM_COUNT + 1);
    systems.push(StarSystemDef {
        id: 0,
        name: "Haven".to_string(),
        galaxy_pos: Vec2::ZERO,
        // Matches today's spawn_initial_system center exactly.
        local_center: HAVEN_LOCAL_CENTER,
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
    let mut local_centers: Vec<Vec2> = vec![HAVEN_LOCAL_CENTER];
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

/// Radius of the asteroid field, unchanged from the original generator.
const FIELD_SPREAD: f32 = 30_000.0;

/// Rocks per field. Up from the original twenty so that the five packs the
/// field breaks into hold six rocks each rather than four -- a pack of four
/// at pack spacing still reads as scattered singles.
const FIELD_ROCKS: u32 = 30;

/// How much room to leave around the station a player arrives at. Rocks are
/// solid, and the field is aimed at that station now, so without a bubble one
/// could spawn on top of the ship. Wide enough that the nearest rock is a
/// short burn (~180 cells) rather than a collision.
const STATION_KEEP_CLEAR: f32 = 12_000.0;

/// This system's primary station, which is where the player turns up.
fn station_pos(def: &StarSystemDef) -> Vec2 {
    crate::world::home_base::station_sites(def.id, def.local_center)
        .first()
        .map(|s| s.pos)
        .unwrap_or(def.local_center)
}

/// Bearing from a system's star to its primary station, so the asteroid
/// field can be put on the side of the star the player arrives on.
///
/// Falls back to due east only if the station resolves exactly onto the star,
/// which `station_sites` never produces.
fn station_field_bearing(def: &StarSystemDef) -> f32 {
    let to_station = station_pos(def) - def.local_center;
    if to_station.length_squared() < 1.0 {
        0.0
    } else {
        to_station.to_angle()
    }
}

/// Where to put this system's asteroid field, as an offset from the star.
///
/// The original generator hardcoded `(50_000, 0)`, which was simply wrong:
/// stars are solid -- `Collider::circle(radius, INFINITY)` -- and run 40,000
/// in radius for a dwarf up to 150,000 for a supergiant. A 30,000-radius
/// field centred 50,000 out therefore spanned 20,000-80,000 from the star,
/// so for anything Main-class or larger -- about 60% of systems, by the class
/// roll in `spawn_star_system` -- every rock in it spawned inside the star
/// and could never be reached. That is why asteroids appeared not to exist.
///
/// Still one field, not a belt: this keeps the original shape and rock count
/// and only moves it somewhere it can exist. It goes in the middle of the
/// widest gap between the star's surface and the planet orbits, at `angle`
/// around the star, so it clears the star and both neighbouring orbits
/// without being pinned to one side of every system in the galaxy.
fn asteroid_field_offset(star_radius: f32, planet_bands: &[(f32, f32)], angle: f32) -> Vec2 {
    // Free radial intervals: from the star's surface outward, the stretches
    // no planet occupies. Measured between planet EDGES, not orbit centres —
    // a planet 60,000 in radius swallows the midpoint of the gap its centre
    // sits in, so centre-based placement can put the field inside a world.
    let mut intervals: Vec<(f32, f32)> = Vec::new();
    let mut cursor = star_radius;
    for &(orbit, radius) in planet_bands {
        let inner = orbit - radius;
        if inner > cursor {
            intervals.push((cursor, inner));
        }
        cursor = cursor.max(orbit + radius);
    }
    // Outside the outermost planet there is always room, but this is a
    // FALLBACK, not a candidate. Offered alongside the real gaps it tends to
    // win on width -- it is unbounded -- and drags the field out past every
    // planet, which is exactly where nobody flies.
    if intervals.is_empty() {
        intervals.push((cursor, cursor + FIELD_SPREAD * 3.0));
    }

    let (lo, hi) = intervals
        .into_iter()
        .max_by(|a, b| (a.1 - a.0).partial_cmp(&(b.1 - b.0)).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or((star_radius, star_radius + FIELD_SPREAD * 3.0));

    let distance = (lo + hi) * 0.5;
    Vec2::new(angle.cos(), angle.sin()) * distance
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

    // Toward this system's primary station, not a free roll. A field at a
    // random bearing is a field you have to go hunting for: you arrive at a
    // station, and the rocks are as likely to be on the far side of the star
    // as near you. Aiming it at the station you dock at means every system
    // has visible rocks from where you actually turn up, without the field
    // being any denser or larger.
    let field_angle = station_field_bearing(def);
    super::spawning::spawn_asteroid_field(
        commands, asset_server,
        def.local_center + asteroid_field_offset(
            system_info.star_radius, &system_info.planet_bands, field_angle,
        ),
        FIELD_ROCKS, FIELD_SPREAD,
        Some((station_pos(def), STATION_KEEP_CLEAR)),
        def.id, &mut rng,
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

#[cfg(test)]
mod asteroid_placement_tests {
    use super::*;
    use crate::celestial::components::StarSizeClass;
    use crate::celestial::spawning::ORBIT_GAP_MIN;

    const CLASSES: [StarSizeClass; 4] = [
        StarSizeClass::Dwarf,
        StarSizeClass::Main,
        StarSizeClass::Giant,
        StarSizeClass::Supergiant,
    ];

    /// Planets the way `spawn_star_system` walks them outward: each orbit
    /// clears the previous body's radius, its own, and a gap. Returned as
    /// `(orbit, radius)` because the radius is what the field placer needs —
    /// a 60,000-radius giant swallows the midpoint of the gap its centre is
    /// in, so centres alone are not enough to place anything safely.
    fn bands(star_radius: f32, radii: &[f32], gap: f32) -> Vec<(f32, f32)> {
        let mut frontier = star_radius;
        let mut prev = 0.0;
        radii
            .iter()
            .map(|&r| {
                frontier += prev + r + gap;
                prev = r;
                (frontier, r)
            })
            .collect()
    }

    /// The original fault: the field's offset was hardcoded to (50,000, 0)
    /// with a 30,000 spread, so every rock sat 20,000-80,000 from the star
    /// while a Main-class star is 80,000 in radius and solid. In about 60% of
    /// systems the whole field spawned inside the sun.
    #[test]
    fn the_field_never_spawns_inside_the_star() {
        for class in CLASSES {
            let star = class.radius();
            for radii in [vec![], vec![20_000.0], vec![60_000.0, 20_000.0, 40_000.0]] {
                for gap in [ORBIT_GAP_MIN, 140_000.0] {
                    let b = bands(star, &radii, gap);
                    let inner = asteroid_field_offset(star, &b, 0.7).length() - FIELD_SPREAD;
                    assert!(
                        inner >= star,
                        "{class:?} with {} planets: field reaches {inner}, inside the star",
                        radii.len()
                    );
                }
            }
        }
    }

    /// Planets are solid too, and now large enough that this is the binding
    /// constraint rather than a formality.
    #[test]
    fn the_field_clears_every_planet() {
        for class in CLASSES {
            let star = class.radius();
            for radii in [vec![20_000.0], vec![60_000.0, 20_000.0], vec![10_000.0, 60_000.0, 30_000.0]] {
                for gap in [ORBIT_GAP_MIN, 140_000.0] {
                    let b = bands(star, &radii, gap);
                    let d = asteroid_field_offset(star, &b, 2.1).length();
                    let (lo, hi) = (d - FIELD_SPREAD, d + FIELD_SPREAD);
                    for (orbit, radius) in &b {
                        let (p_lo, p_hi) = (orbit - radius, orbit + radius);
                        assert!(
                            hi <= p_lo || lo >= p_hi,
                            "{class:?}: field spans {lo}..{hi}, overlapping a planet \
                             occupying {p_lo}..{p_hi}"
                        );
                    }
                }
            }
        }
    }

    /// The field is no longer pinned to the same side of every system in the
    /// galaxy, which is what `(50_000, 0)` did for all 31 of them.
    #[test]
    fn the_field_is_not_always_in_the_same_direction() {
        let star = StarSizeClass::Main.radius();
        let b = bands(star, &[30_000.0, 20_000.0], ORBIT_GAP_MIN);
        let a = asteroid_field_offset(star, &b, 0.0);
        let c = asteroid_field_offset(star, &b, 2.4);
        assert!(a.distance(c) > FIELD_SPREAD, "two angles put the field in the same place");
        assert!(
            (a.length() - c.length()).abs() < 1.0,
            "angle changed the distance from the star, not just the direction"
        );
    }
}

#[cfg(test)]
mod galaxy_layout_tests {
    use super::*;

    const SEEDS: [u64; 6] = [1, 42, 7_777, 123_456, u64::MAX / 3, 9_999_999];

    /// Two systems closer than twice the snap tolerance means one click on the
    /// map lands inside both, and which one you warp to stops being
    /// predictable. This is a static relationship between two constants, so it
    /// holds for every seed and is the first thing to check after retuning
    /// either -- MIN_SYSTEM_SEPARATION was tightened from 400,000 to 320,000
    /// to let constellations read as groups.
    #[test]
    fn systems_stay_far_enough_apart_to_click() {
        assert!(
            MIN_SYSTEM_SEPARATION > SNAP_TOLERANCE * 2.0,
            "separation {MIN_SYSTEM_SEPARATION} is within two snap tolerances ({}), \
             so one click can resolve to two systems",
            SNAP_TOLERANCE * 2.0
        );

        for seed in SEEDS {
            let map = generate_galaxy_map(seed);
            for (i, a) in map.systems.iter().enumerate() {
                for b in map.systems.iter().skip(i + 1) {
                    let d = a.galaxy_pos.distance(b.galaxy_pos);
                    assert!(
                        d >= MIN_SYSTEM_SEPARATION,
                        "seed {seed}: systems {} and {} are {d} apart",
                        a.id, b.id
                    );
                }
            }
        }
    }

    /// Only Haven sits at the origin. The previous placer defaulted an
    /// unplaceable system to `Vec2::ZERO`, which stacked it silently on top of
    /// Haven -- invisible on the map, and two systems one click could not tell
    /// apart.
    #[test]
    fn nothing_is_stacked_on_haven() {
        for seed in SEEDS {
            let map = generate_galaxy_map(seed);
            assert_eq!(map.systems[0].galaxy_pos, Vec2::ZERO, "Haven left the origin");
            for sys in map.systems.iter().skip(1) {
                assert_ne!(
                    sys.galaxy_pos, Vec2::ZERO,
                    "seed {seed}: system {} defaulted onto the origin", sys.id
                );
            }
        }
    }

    #[test]
    fn every_system_fits_on_the_map() {
        for seed in SEEDS {
            let map = generate_galaxy_map(seed);
            assert_eq!(map.systems.len(), SYSTEM_COUNT + 1, "seed {seed}: wrong system count");
            for sys in &map.systems {
                assert!(
                    sys.galaxy_pos.length() <= GALAXY_RADIUS,
                    "seed {seed}: system {} is at {}, off the edge of the map",
                    sys.id, sys.galaxy_pos.length()
                );
            }
        }
    }

    /// The point of the change: systems are grouped, not evenly spread.
    ///
    /// Measured rather than asserted by construction -- each system's nearest
    /// neighbour should be much closer than the average system, which is true
    /// of clusters and false of an even scatter.
    ///
    /// Both numbers here were measured, not guessed. The old uniform-area
    /// sampler scores 0.230; constellations score 0.114, a clean factor of
    /// two. The threshold sits between them rather than just under the old
    /// value, so this fails if the grouping is lost AND if it is quietly
    /// weakened partway back toward an even spread.
    #[test]
    fn systems_are_grouped_into_constellations() {
        for seed in SEEDS {
            let map = generate_galaxy_map(seed);
            let pts: Vec<Vec2> = map.systems.iter().map(|s| s.galaxy_pos).collect();

            let mut nearest_sum = 0.0;
            let mut all_sum = 0.0;
            let mut all_count = 0.0;
            for (i, a) in pts.iter().enumerate() {
                let mut nearest = f32::MAX;
                for (j, b) in pts.iter().enumerate() {
                    if i == j { continue; }
                    let d = a.distance(*b);
                    nearest = nearest.min(d);
                    all_sum += d;
                    all_count += 1.0;
                }
                nearest_sum += nearest;
            }
            let ratio = (nearest_sum / pts.len() as f32) / (all_sum / all_count);
            assert!(
                ratio < 0.17,
                "seed {seed}: nearest-neighbour/mean ratio {ratio:.3} -- systems are \
                 spread evenly rather than gathered into constellations"
            );
        }
    }
}

#[cfg(test)]
mod haven_spawn_tests {
    use super::*;
    use crate::celestial::components::StarSizeClass;
    use crate::world::home_base::STATION_POS;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    /// Haven's star class, the way `spawn_star_system` rolls it: the first
    /// draw off a stream seeded with the system's own seed, which for Haven is
    /// the fixed 42. Derived rather than hardcoded so this keeps telling the
    /// truth if that roll is ever retuned.
    fn havens_star_radius() -> f32 {
        let mut rng = StdRng::seed_from_u64(42);
        let class = match rng.gen_range(0..10) {
            0..=3 => StarSizeClass::Dwarf,
            4..=7 => StarSizeClass::Main,
            8 => StarSizeClass::Giant,
            _ => StarSizeClass::Supergiant,
        };
        class.radius()
    }

    fn spawn_to_star() -> f32 {
        STATION_POS.distance(HAVEN_LOCAL_CENTER)
    }

    /// The closest a planet's inner edge can come to the star, whatever its
    /// size. The orbit walk clears the previous body, this body's own radius
    /// and a gap, so on the first planet the radius cancels and the floor is
    /// exactly `star_radius + ORBIT_GAP_MIN`.
    fn nearest_possible_planet_edge(star_radius: f32) -> f32 {
        star_radius + crate::celestial::spawning::ORBIT_GAP_MIN
    }

    /// The ship must not spawn inside its own sun. Stars are solid and
    /// infinitely massive, so this is not a cosmetic concern.
    #[test]
    fn the_spawn_is_outside_havens_star() {
        let radius = havens_star_radius();
        let distance = spawn_to_star();
        assert!(
            distance > radius * 1.2,
            "spawn is {distance} from a star of radius {radius} -- inside it, or too close to it"
        );
    }

    /// ...and inside the planet orbits, which is the point of moving it --
    /// without ever sitting ON one, because planets are solid and sweep
    /// their whole orbit every few minutes.
    #[test]
    fn the_spawn_is_inside_the_planet_orbits() {
        let radius = havens_star_radius();
        let nearest_edge = nearest_possible_planet_edge(radius);
        let distance = spawn_to_star();
        assert!(
            distance < nearest_edge,
            "spawn is {distance} out; a planet's inner edge can reach {nearest_edge}, \
             so a solid world can sweep through the station"
        );
    }

    /// The fault this fixes, stated as a number. The old centre put the spawn
    /// 492,000 units from Haven's star, so every planet and every rock in the
    /// home system was unreachable and the only planet anyone ever saw was the
    /// parallax one glued to the camera.
    #[test]
    fn the_spawn_is_not_half_a_million_units_from_home() {
        let distance = spawn_to_star();
        assert!(
            distance < 150_000.0,
            "spawn is {distance} from Haven's star; the old value was 492,000 and that \
             is what made the home system unvisitable"
        );
    }

    /// Asteroids land on the side of the star the player arrives on, within
    /// a short flight rather than somewhere around the far limb.
    #[test]
    fn havens_asteroids_are_near_the_spawn() {
        let radius = havens_star_radius();
        let def = StarSystemDef {
            id: 0,
            name: "Haven".into(),
            galaxy_pos: Vec2::ZERO,
            local_center: HAVEN_LOCAL_CENTER,
            seed: 42,
            faction: None,
            danger_tier: 0.0,
            discovery: SystemDiscovery::Visited,
            last_updated: 0.0,
            resource_fraction_remaining: 1.0,
        };

        // Across the range of first-planet sizes and gaps the walk can roll.
        for first_radius in [10_000.0f32, 30_000.0, 60_000.0] {
            for gap in [crate::celestial::spawning::ORBIT_GAP_MIN, 140_000.0] {
                let orbit = radius + first_radius + gap;
                let bands = [(orbit, first_radius)];
                let field = HAVEN_LOCAL_CENTER
                    + asteroid_field_offset(radius, &bands, station_field_bearing(&def));
                let from_spawn = field.distance(STATION_POS);
                assert!(
                    from_spawn < FIELD_SPREAD * 2.0,
                    "first planet r={first_radius} gap={gap}: field centre is {from_spawn} \
                     from the spawn, too far to see rocks from where the player starts"
                );
            }
        }
    }

    /// The field must clear the bodies on both sides of it. This is why
    /// ORBIT_GAP_MIN is tied to FIELD_SPREAD rather than chosen by feel.
    #[test]
    fn the_field_never_touches_a_planet_or_the_star() {
        let star = havens_star_radius();
        for first_radius in [10_000.0f32, 30_000.0, 60_000.0] {
            for gap in [crate::celestial::spawning::ORBIT_GAP_MIN, 140_000.0] {
                let orbit = star + first_radius + gap;
                let bands = [(orbit, first_radius)];
                let d = asteroid_field_offset(star, &bands, 0.0).length();
                assert!(
                    d - FIELD_SPREAD > star,
                    "field reaches {} from the centre, inside a star of {star}",
                    d - FIELD_SPREAD
                );
                let (lo, hi) = (d - FIELD_SPREAD, d + FIELD_SPREAD);
                let (p_lo, p_hi) = (orbit - first_radius, orbit + first_radius);
                assert!(
                    hi <= p_lo || lo >= p_hi,
                    "field spans {lo}..{hi}, overlapping the planet at {p_lo}..{p_hi}"
                );
            }
        }
    }
}
