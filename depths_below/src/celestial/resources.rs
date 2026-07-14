use bevy::prelude::*;

/// Lightweight metadata per star system (bookkeeping, not ECS)
#[derive(Clone, Debug)]
pub struct StarSystemInfo {
    pub id: u32,
    pub star_entity: Option<Entity>,
    pub planet_entities: Vec<Entity>,
    pub center: Vec2,
    pub is_alive: bool,
}

/// Global galaxy state
#[derive(Resource)]
pub struct GalaxyState {
    pub systems: Vec<StarSystemInfo>,
    pub current_system: u32,
    pub total_bodies: u32,
    pub galaxy_time: f64,
    pub next_system_id: u32,
}

impl Default for GalaxyState {
    fn default() -> Self {
        Self {
            systems: Vec::new(),
            current_system: 0,
            total_bodies: 0,
            galaxy_time: 0.0,
            next_system_id: 0,
        }
    }
}

/// All tuning constants for celestial mechanics
#[derive(Resource)]
pub struct CelestialConfig {
    /// Gameplay-tuned gravity constant
    pub gravity_constant: f32,
    /// Max force applied to ship (prevents instant death)
    pub max_gravity_force: f32,
    /// How fast black holes consume bodies
    pub black_hole_consume_speed: f32,
    /// Random flare buildup rate range (per second)
    pub flare_buildup_rate_min: f32,
    pub flare_buildup_rate_max: f32,
    /// How much worse flare radiation is vs base stellar radiation
    pub flare_radiation_multiplier: f32,
    /// How long a flare lasts (seconds)
    pub flare_duration: f32,
    /// Supernova blast radius
    pub star_death_supernova_radius: f32,
    /// Supernova damage
    pub supernova_damage: f32,
    /// Speed multiplier for freed planets
    pub freed_planet_speed_multiplier: f32,
    /// Warp charge time (seconds)
    pub warp_charge_time: f32,
    /// Distance between star systems
    pub system_spacing: f32,
    /// Gradual crush damage per second near black hole event horizon
    pub black_hole_crush_damage_rate: f32,
}

impl Default for CelestialConfig {
    fn default() -> Self {
        Self {
            // Gravity — realistic. If your thrust < gravity pull, you're dead.
            gravity_constant: 600_000.0,        // Strong enough that underpowered ships get pulled in
            max_gravity_force: 50_000.0,        // Effectively uncapped — physics decides, not a clamp
            // Black holes — terrifying but survivable if you react
            black_hole_consume_speed: 0.2,      // Slower consumption = more escape time
            black_hole_crush_damage_rate: 30.0,  // You went near a black hole without a gravity compensator. That's on you.
            // Flares — unpredictable but not constant
            flare_buildup_rate_min: 0.005,      // Slower buildup = less frequent flares
            flare_buildup_rate_max: 0.03,       // Reduced max rate
            flare_radiation_multiplier: 8.0,     // Deadly without shielding. Get behind a planet or die.
            flare_duration: 6.0,                // Slightly shorter
            // Supernova — the big event
            star_death_supernova_radius: 60_000.0,  // Bigger radius — harder to avoid
            supernova_damage: 800.0,             // Near-instant kill if you're close. Run or die.
            freed_planet_speed_multiplier: 60.0, // Slower freed planets — more time to dodge
            // Warp — should feel intentional, not instant
            warp_charge_time: 8.0,              // Increased from 5 — commitment required
            system_spacing: 200_000.0,
        }
    }
}
