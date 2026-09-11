use bevy::prelude::*;
use rand::Rng;
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
    rng: &mut impl Rng,
    textures: &CelestialTextures,
) -> StarSystemInfo {
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
                    image: asset_server.load(planet_sprite_path(planet_type, rng)),
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
/// Shape variants per (size class, ore type). See tools/art/pixel/asteroids.py.
const ASTEROID_VARIANTS: usize = 3;

/// Sprite for one rock.
///
/// The art is procedural pixel work (tools/art/pixel/asteroids.py): granular
/// grain-by-grain rock with ore running through it as thin seams. Resolution
/// tracks size class so a PIXEL is ~4 world units at every size -- a big rock
/// is more grains, not bigger grains.
///
/// Ore is baked into the sprite, so what a rock is worth mining for is
/// legible from the rock itself instead of from a flat colour wash.
fn asteroid_sprite(size: f32, resource: ResourceNodeType, variant: usize) -> String {
    let class = if size < 350.0 {
        "small"
    } else if size < 550.0 {
        "medium"
    } else {
        "large"
    };
    let ore = match resource {
        ResourceNodeType::MetalOre => "metal",
        ResourceNodeType::RareCrystal => "crystal",
        ResourceNodeType::FuelDeposit => "fuel",
        ResourceNodeType::ExoticMatter => "exotic",
    };
    format!(
        "sprites/celestial/asteroids/ast_{}_{}_{}.png",
        class, ore, variant % ASTEROID_VARIANTS
    )
}

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
    rng: &mut impl Rng,
    // 1.0 = untouched, scales down toward 0.0 as the system's ambient
    // depletion (StarSystemDef.resource_fraction_remaining, see
    // celestial::galaxy::catch_up_system) progresses — applied post-roll so
    // it doesn't disturb the deterministic RNG sequence (same seed still
    // rolls the same asteroid count/size/type/position every time).
    depletion_mult: f32,
) {
    // Rocks are solid now (see ship::collision) — nudge overlapping rolls
    // apart so a field doesn't generate asteroids fused into each other.
    // Still deterministic: same seed, same draw sequence, same layout.
    let mut placed: Vec<(Vec2, f32)> = Vec::new();
    for i in 0..count {
        let size = rng.gen_range(200.0..800.0);
        let mass = size * 0.5;
        let radius = size * 0.5;

        let mut pos = center;
        for _attempt in 0..8 {
            let angle = rng.gen_range(0.0..std::f32::consts::TAU);
            let dist = rng.gen_range(0.0..spread);
            pos = center + Vec2::new(angle.cos() * dist, angle.sin() * dist);
            if placed.iter().all(|(p, r)| pos.distance(*p) > (radius + r) * 1.1 + 40.0) {
                break;
            }
        }
        placed.push((pos, radius));

        let resource_type = match rng.gen_range(0..4) {
            0 => ResourceNodeType::MetalOre,
            1 => ResourceNodeType::RareCrystal,
            2 => ResourceNodeType::FuelDeposit,
            _ => ResourceNodeType::ExoticMatter,
        };
        // Gameplay rolls first, cosmetics last, so art changes can never
        // disturb what a system actually contains.
        let resource_amount = size * rng.gen_range(0.5..1.2);

        // ONE fixed-width draw for everything cosmetic. gen::<u32>() consumes
        // exactly one word; gen_range uses rejection sampling, so its
        // consumption depends on the range -- which would mean that changing
        // the variant count later silently reshuffled every existing system's
        // asteroid layout. Deriving all the jitter from the bits of a single
        // word makes the art independent of the gameplay stream for good.
        let art: u32 = rng.gen();
        let variant = (art % ASTEROID_VARIANTS as u32) as usize;
        let flip_x = (art >> 8) & 1 == 1;
        // The old flat per-resource wash is gone: the sprite carries its own
        // ore seams now, and a hue tint multiplied over pixel art just muds
        // the ore ladder. A neutral brightness wobble is all that is left, so
        // a field doesn't read as one rock stamped twenty times.
        let shade = 0.88 + ((art >> 16) & 0xff) as f32 / 255.0 * 0.12;
        let color = Color::srgb(shade, shade, shade);
        let sprite_path = asteroid_sprite(size, resource_type, variant);

        commands.spawn((
            (Sprite {
                    // NEAREST, not the project default. Sprites are sampled
                    // linear globally (main.rs never sets
                    // ImagePlugin::default_nearest), and these textures are
                    // blown up ~4x from texel to world size -- bilinear would
                    // smear the grain into mush and throw away the entire
                    // point of the pixel art. Set per-asset rather than
                    // globally so the ~100 smooth module/weapon renders keep
                    // their filtering.
                    image: asset_server
                        .load_builder()
                        .with_settings(|s: &mut bevy::image::ImageLoaderSettings| {
                            s.sampler = bevy::image::ImageSampler::nearest();
                        })
                        .load(sprite_path),
                    color,
                    custom_size: Some(Vec2::splat(size)),
                    // Free second silhouette per variant, and safe here: the
                    // baked light is straight-down, so a mirror leaves the
                    // lighting consistent across the field.
                    flip_x,
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
                resource_remaining: resource_amount * depletion_mult,
                resource_type,
                extraction_rate: 5.0,
            },
            StarSystemMember { system_id },
        ));
    }
}

#[cfg(test)]
mod asteroid_art_tests {
    use super::*;

    /// Every (size class x ore type x variant) combination must resolve to a
    /// file that actually exists.
    ///
    /// `asteroid_sprite` builds paths with `format!` rather than indexing a
    /// const table, so a typo or a missing render fails silently in game as an
    /// invisible asteroid -- and 36 files is far too many to check by eye.
    #[test]
    fn every_asteroid_sprite_exists() {
        let sizes = [250.0_f32, 450.0, 700.0];
        let ores = [
            ResourceNodeType::MetalOre,
            ResourceNodeType::RareCrystal,
            ResourceNodeType::FuelDeposit,
            ResourceNodeType::ExoticMatter,
        ];
        let mut checked = 0;
        for size in sizes {
            for ore in ores {
                for variant in 0..ASTEROID_VARIANTS {
                    let rel = asteroid_sprite(size, ore, variant);
                    let path = std::path::Path::new("assets").join(&rel);
                    assert!(path.exists(), "missing asteroid sprite: {}", rel);
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, sizes.len() * ores.len() * ASTEROID_VARIANTS);
    }
}
