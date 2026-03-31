use bevy::prelude::*;
use super::components::PlanetType;

/// A star emits a radiation flare — random timing, intensity scales with star size
#[derive(Event)]
pub struct RadiationFlare {
    pub star: Entity,
    pub intensity: f32,
    pub position: Vec2,
    pub radius: f32,
}

/// A star has been destroyed — triggers supernova, planets go flying
#[derive(Event)]
pub struct StarDestroyed {
    pub star: Entity,
    pub position: Vec2,
    pub supernova_radius: f32,
    pub freed_planets: Vec<Entity>,
}

/// A planet was consumed by a black hole
#[derive(Event)]
pub struct PlanetConsumed {
    pub planet: Entity,
    pub black_hole: Entity,
    pub planet_type: PlanetType,
}

/// Any body consumed by a black hole (planets, asteroids, debris)
#[derive(Event)]
pub struct BodyConsumed {
    pub entity: Entity,
    pub black_hole: Entity,
    pub mass_gained: f32,
}

/// Warning when gravity pull becomes significant on the ship
#[derive(Event)]
pub struct GravityWarning {
    pub source: Entity,
    pub pull_strength: f32,
}

/// Supernova shockwave expanding outward
#[derive(Event)]
pub struct SupernovaShockwave {
    pub origin: Vec2,
    pub damage: f32,
    pub radius: f32,
}

/// Warp jump initiated to another star system
#[derive(Event)]
pub struct WarpJumpStarted {
    pub target_system: u32,
}

/// Warp jump completed — arrived at new system
#[derive(Event)]
pub struct WarpJumpCompleted {
    pub system_id: u32,
}
