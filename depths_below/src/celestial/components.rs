use bevy::prelude::*;

// ============================================================================
// CELESTIAL BODY COMPONENTS
// ============================================================================

/// Core component for any celestial body (star, planet, asteroid, black hole)
#[derive(Component)]
pub struct CelestialBody {
    pub body_type: CelestialBodyType,
    pub mass: f32,
    pub radius: f32,
    pub name: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CelestialBodyType {
    Star,
    Planet,
    Asteroid,
    BlackHole,
    Debris,
}

// ============================================================================
// GRAVITY
// ============================================================================

/// Anything with this component exerts gravitational pull
#[derive(Component)]
pub struct GravityWell {
    /// Pre-computed GM constant (tuned for gameplay, not physics)
    pub strength: f32,
    /// Beyond this distance, gravity is zero (performance optimization)
    pub influence_radius: f32,
    pub falloff: GravityFalloff,
}

#[derive(Clone, Copy, Debug)]
pub enum GravityFalloff {
    /// F = strength / r^2 — realistic, used for stars and planets
    InverseSquare,
    /// F = strength / r — gentler, used for smaller bodies
    InverseLinear,
    /// Custom dramatic ramp near event horizon
    BlackHole,
}

/// Anything with this component is AFFECTED by gravity wells
#[derive(Component)]
pub struct GravityAffected {
    pub mass: f32,
}

/// Accumulated gravity force this frame — written by gravity system, read by movement
#[derive(Component, Default)]
pub struct GravityForce(pub Vec2);

// ============================================================================
// ORBITS
// ============================================================================

/// Stable Keplerian orbit around a parent body. Position evaluated analytically.
#[derive(Component)]
pub struct OrbitalPath {
    pub parent: Entity,
    pub semi_major_axis: f32,
    pub eccentricity: f32,
    pub phase: f32,
    pub period: f32,
    pub clockwise: bool,
}

/// Replaces OrbitalPath when a star dies — body flies off on a tangent
#[derive(Component)]
pub struct FreeFlight {
    pub velocity: Vec2,
}

// ============================================================================
// STARS
// ============================================================================

#[derive(Component)]
pub struct Star {
    pub luminosity: f32,
    pub radiation_output: f32,
    pub size_class: StarSizeClass,
    /// Builds up randomly over time. When it crosses flare_threshold, a flare fires.
    pub flare_buildup: f32,
    /// Randomized per-star (0.7 to 0.95) — unpredictable flare timing
    pub flare_threshold: f32,
    pub is_dying: bool,
    pub death_timer: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StarSizeClass {
    Dwarf,
    Main,
    Giant,
    Supergiant,
}

impl StarSizeClass {
    pub fn radiation_multiplier(&self) -> f32 {
        match self {
            Self::Dwarf => 0.5,
            Self::Main => 1.0,
            Self::Giant => 2.5,
            Self::Supergiant => 5.0,
        }
    }

    pub fn flare_intensity_multiplier(&self) -> f32 {
        match self {
            Self::Dwarf => 0.3,
            Self::Main => 1.0,
            Self::Giant => 3.0,
            Self::Supergiant => 8.0,
        }
    }

    pub fn radius(&self) -> f32 {
        match self {
            Self::Dwarf => 40_000.0,
            Self::Main => 80_000.0,
            Self::Giant => 120_000.0,
            Self::Supergiant => 150_000.0,
        }
    }

    pub fn mass(&self) -> f32 {
        match self {
            Self::Dwarf => 5_000.0,
            Self::Main => 20_000.0,
            Self::Giant => 80_000.0,
            Self::Supergiant => 200_000.0,
        }
    }
}

// ============================================================================
// BLACK HOLES
// ============================================================================

#[derive(Component)]
pub struct BlackHole {
    pub event_horizon_radius: f32,
    pub accretion_disk_radius: f32,
    /// Grows as it consumes mass — makes it progressively more dangerous
    pub consumed_mass: f32,
    pub tidal_force_multiplier: f32,
}

/// Marks an entity being consumed — visual spiral-in before despawn
#[derive(Component)]
pub struct BeingConsumed {
    pub by_black_hole: Entity,
    pub progress: f32,
}

// ============================================================================
// PLANETS
// ============================================================================

#[derive(Component)]
pub struct Planet {
    pub planet_type: PlanetType,
    pub has_atmosphere: bool,
    pub resource_richness: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlanetType {
    Rocky,
    Gas,
    Ice,
    Lava,
    Shattered,
}

impl PlanetType {
    pub fn radius_range(&self) -> (f32, f32) {
        match self {
            Self::Rocky => (10_000.0, 15_000.0),
            Self::Gas => (20_000.0, 30_000.0),
            Self::Ice => (8_000.0, 12_000.0),
            Self::Lava => (10_000.0, 14_000.0),
            Self::Shattered => (5_000.0, 10_000.0),
        }
    }

    pub fn mass_range(&self) -> (f32, f32) {
        match self {
            Self::Rocky => (800.0, 2_000.0),
            Self::Gas => (5_000.0, 15_000.0),
            Self::Ice => (600.0, 1_500.0),
            Self::Lava => (1_000.0, 2_500.0),
            Self::Shattered => (200.0, 800.0),
        }
    }
}

// ============================================================================
// STAR SYSTEM
// ============================================================================

/// Tags an entity as belonging to a specific star system
#[derive(Component)]
pub struct StarSystemMember {
    pub system_id: u32,
}

// ============================================================================
// WARP / JUMP
// ============================================================================

/// Marks the ship as currently charging a warp jump
#[derive(Component)]
pub struct WarpCharging {
    pub target_system: u32,
    pub charge_timer: Timer,
}
