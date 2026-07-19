use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use super::components::*;
use super::resources::*;
use super::poi::{SpacePoi, SpacePoiType, MineableResource, ResourceNodeType};
use crate::vfx::procedural_textures::CelestialTextures;

/// Generate a star system at a given center position.
/// Returns the StarSystemInfo for tracking.
/// Hand-picked real planet art (Kenney "Planets" pack, CC0 — see
/// assets/sprites/celestial/CREDITS.txt) grouped loosely by PlanetType so a
/// Lava world reads as fiery, an Ice world as pale/blue, etc. Exact match
/// isn't precise (all 10 are decent for any type) but this keeps the flavor
/// roughly honest.
fn planet_sprite_path(planet_type: PlanetType, rng: &mut impl Rng) -> String {
    let variants: &[u32] = match planet_type {
        PlanetType::Rocky => &[1, 4, 6],
        PlanetType::Gas => &[2, 9],
        PlanetType::Ice => &[0, 3],
        PlanetType::Lava => &[5, 7, 8],
        PlanetType::Shattered => &[4, 6],
    };
    let idx = variants[rng.gen_range(0..variants.len())];
    format!("sprites/celestial/planets/planet{:02}.png", idx)
}

pub fn spawn_star_system(
    commands: &mut Commands,
    asset_server: &AssetServer,
    center: Vec2,
    system_id: u32,
    seed: u64,
    textures: &CelestialTextures,
) -> StarSystemInfo {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

    // Pick star class based on seed
    let star_class = match rng.gen_range(0..10) {
        0..=3 => StarSizeClass::Dwarf,
        4..=7 => StarSizeClass::Main,
        8 => StarSizeClass::Giant,
        _ => StarSizeClass::Supergiant,
    };

    let star_radius = star_class.radius();
    let star_mass = star_class.mass();

    // Spawn star
    let star_entity = commands.spawn((
        (Sprite {
                image: textures.solid.clone(),
                color: match star_class {
                    StarSizeClass::Dwarf => Color::srgb(1.0, 0.6, 0.3),
                    StarSizeClass::Main => Color::srgb(1.0, 0.95, 0.8),
                    StarSizeClass::Giant => Color::srgb(1.0, 0.8, 0.4),
                    StarSizeClass::Supergiant => Color::srgb(0.7, 0.8, 1.0),
                },
                custom_size: Some(Vec2::splat(star_radius * 2.0)),
                ..default()
            }, Transform::from_xyz(center.x, center.y, -1.0)),
        CelestialBody {
            body_type: CelestialBodyType::Star,
            mass: star_mass,
            radius: star_radius,
            name: format!("Star-{}", system_id),
        },
        Star {
            luminosity: star_class.radiation_multiplier(),
            radiation_output: star_class.radiation_multiplier() * 10.0,
            size_class: star_class,
            flare_buildup: 0.0,
            flare_threshold: rng.gen_range(0.7..0.95),
            is_dying: false,
            death_timer: 10.0, // 10 second countdown when dying starts
        },
        GravityWell {
            strength: star_mass * 500.0,
            influence_radius: star_radius * 4.0,
            falloff: GravityFalloff::InverseSquare,
        },
        StarSystemMember { system_id },
    )).id();

    // Generate 2-6 planets
    let planet_count = rng.gen_range(2..=6);
    let mut planet_entities = Vec::new();

    let planet_types = [PlanetType::Rocky, PlanetType::Gas, PlanetType::Ice, PlanetType::Lava];

    for i in 0..planet_count {
        let planet_type = planet_types[rng.gen_range(0..planet_types.len())];
        let (r_min, r_max) = planet_type.radius_range();
        let (m_min, m_max) = planet_type.mass_range();

        let planet_radius = rng.gen_range(r_min..r_max);
        let planet_mass = rng.gen_range(m_min..m_max);

        // Orbit distance increases with planet index
        let orbit_distance = star_radius * 2.0 + (i as f32 + 1.0) * rng.gen_range(25_000.0..45_000.0);
        let orbit_period = rng.gen_range(60.0..300.0); // 1-5 minutes per orbit
        let eccentricity = rng.gen_range(0.0..0.3);
        let phase = rng.gen_range(0.0..std::f32::consts::TAU);
        let clockwise = rng.gen_bool(0.5);

        // Initial position on orbit
        let initial_x = center.x + orbit_distance * phase.cos();
        let initial_y = center.y + orbit_distance * phase.sin();

        let planet_entity = commands.spawn((
            (Sprite {
                    image: asset_server.load(planet_sprite_path(planet_type, &mut rng)),
                    custom_size: Some(Vec2::splat(planet_radius * 2.0)),
                    ..default()
                }, Transform::from_xyz(initial_x, initial_y, -0.9)),
            CelestialBody {
                body_type: CelestialBodyType::Planet,
                mass: planet_mass,
                radius: planet_radius,
                name: format!("Planet-{}-{}", system_id, i + 1),
            },
            Planet {
                planet_type,
                has_atmosphere: matches!(planet_type, PlanetType::Gas | PlanetType::Rocky) && rng.gen_bool(0.4),
                resource_richness: rng.gen_range(0.1..1.0),
            },
            OrbitalPath {
                parent: star_entity,
                semi_major_axis: orbit_distance,
                eccentricity,
                phase,
                period: orbit_period,
                clockwise,
            },
            GravityWell {
                strength: planet_mass * 100.0,
                influence_radius: planet_radius * 3.0,
                falloff: GravityFalloff::InverseSquare,
            },
            StarSystemMember { system_id },
        )).id();

        planet_entities.push(planet_entity);
    }

    StarSystemInfo {
        id: system_id,
        star_entity: Some(star_entity),
        planet_entities,
        center,
        is_alive: true,
    }
}

/// Real rock sprites (Kenney "Space Shooter Redux", CC0 — see
/// assets/sprites/celestial/CREDITS.txt). Picked independent of world size —
/// custom_size scales whatever image lands here to the rolled asteroid size,
/// so a "tiny" sprite blown up to a big asteroid's size looks fine.
const METEOR_SPRITES: &[&str] = &[
    "meteorBrown_big1.png", "meteorBrown_big2.png", "meteorBrown_big3.png", "meteorBrown_big4.png",
    "meteorBrown_med1.png", "meteorBrown_med3.png",
    "meteorBrown_small1.png", "meteorBrown_small2.png",
    "meteorBrown_tiny1.png", "meteorBrown_tiny2.png",
    "meteorGrey_big1.png", "meteorGrey_big2.png", "meteorGrey_big3.png", "meteorGrey_big4.png",
    "meteorGrey_med1.png", "meteorGrey_med2.png",
    "meteorGrey_small1.png", "meteorGrey_small2.png",
    "meteorGrey_tiny1.png", "meteorGrey_tiny2.png",
];

/// Spawn asteroid field at a position (decorative gravity bodies, each
/// mineable — see MineableResource). Every asteroid the player actually
/// flies past is a resource node; there's no separate invisible "node"
/// system to stumble into, and since this is the one function both the
/// initial system and every warp jump call, mining works everywhere for
/// free instead of only in the system you started in.
pub fn spawn_asteroid_field(
    commands: &mut Commands,
    asset_server: &AssetServer,
    center: Vec2,
    count: u32,
    spread: f32,
    system_id: u32,
) {
    let mut rng = rand::thread_rng();

    for i in 0..count {
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let dist = rng.gen_range(0.0..spread);
        let pos = center + Vec2::new(angle.cos() * dist, angle.sin() * dist);

        let size = rng.gen_range(200.0..800.0);
        let mass = size * 0.5;

        let resource_type = match rng.gen_range(0..4) {
            0 => ResourceNodeType::MetalOre,
            1 => ResourceNodeType::RareCrystal,
            2 => ResourceNodeType::FuelDeposit,
            _ => ResourceNodeType::ExoticMatter,
        };
        // Tinted by resource type (was a flat gray) so a glance at an
        // asteroid field hints at what's worth mining.
        let color = match resource_type {
            ResourceNodeType::MetalOre => Color::srgb(0.45, 0.40, 0.35),
            ResourceNodeType::RareCrystal => Color::srgb(0.40, 0.50, 0.70),
            ResourceNodeType::FuelDeposit => Color::srgb(0.50, 0.40, 0.20),
            ResourceNodeType::ExoticMatter => Color::srgb(0.50, 0.35, 0.60),
        };

        let sprite_path = format!(
            "sprites/celestial/asteroids/{}",
            METEOR_SPRITES[rng.gen_range(0..METEOR_SPRITES.len())]
        );

        commands.spawn((
            (Sprite {
                    image: asset_server.load(sprite_path),
                    color,
                    custom_size: Some(Vec2::splat(size)),
                    ..default()
                }, Transform::from_xyz(pos.x, pos.y, -0.5)),
            CelestialBody {
                body_type: CelestialBodyType::Asteroid,
                mass,
                radius: size * 0.5,
                name: "Asteroid".into(),
            },
            SpacePoi {
                poi_type: SpacePoiType::AsteroidNode,
                looted: false,
                name: format!("Asteroid-{}-{}", system_id, i),
                loot_value: 0,
            },
            MineableResource {
                // Bigger rock = more to mine (size ranges 200-800).
                resource_remaining: size * rng.gen_range(0.5..1.2),
                resource_type,
                extraction_rate: 5.0,
            },
            StarSystemMember { system_id },
        ));
    }
}
