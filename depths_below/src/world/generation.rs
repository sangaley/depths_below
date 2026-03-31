use bevy::prelude::*;
use bevy::sprite::Anchor;
use rand::prelude::*;
use crate::components::*;
use crate::sprite_map;

/// Dark & mysterious narrative log entries placed at POIs throughout the void.
/// Each entry: (title, text, minimum_depth_level)
const LOG_ENTRIES: &[(&str, &str, i32)] = &[
    // --- NEAR ORBIT (depth 0-3) ---
    ("Expedition Log #1",
     "Day 3: We've pushed past the asteroid fields. Radar shows massive structures ahead. Not natural formations.",
     1),
    ("Recovered Note",
     "To whoever finds this: the company lied about what's out here. Turn back. The station has forgotten this sector for good reason.",
     2),
    ("Ship's Log: CSS Meridian",
     "Engine failure at sector 180. Hull compromised. Three crew missing since last night. Nobody heard them leave.",
     2),

    // --- ASTEROID BELT (depth 3-6) ---
    ("Expedition Log #2",
     "Day 7: Found wreckage of a previous expedition. Their hull was breached from the INSIDE. What could do that?",
     3),
    ("Research Note: Acoustics",
     "We've been recording infrasound from deeper in the void. When played back at normal speed, it sounds like breathing.",
     4),
    ("Distress Signal (Decoded)",
     "MAYDAY MAYDAY. Something is following us. It matches our speed exactly. It's been three days. It never gets closer, never falls behind.",
     5),
    ("Research Note: Luminescence",
     "The creatures here don't just glow - they communicate with light. Patterns too complex to be random. Are they... words?",
     5),

    // --- DEEP SPACE (depth 6-10) ---
    ("Expedition Log #3",
     "Day 12: The ruins are older than anything at the station. Carved metal at sector 800. Impossible engineering. The carvings depict... us. Ships. How?",
     6),
    ("Personal Journal: Dr. Vasquez",
     "The symbols match nothing in any database. But I dream about them now. In the dreams, I can read them perfectly. I just can't remember what they say when I wake.",
     7),
    ("Engineering Report",
     "Hull sensors report external contact - something is running along the hull. Like fingers. There's nothing on radar.",
     8),
    ("Audio Transcript #47",
     "RESEARCHER: The artifact we recovered - it's warm to the touch. CAPTAIN: That's impossible in the void. RESEARCHER: I know. And it's getting warmer.",
     9),
    ("Warning Beacon",
     "AUTOMATED MESSAGE: Do not proceed past sector 1000. Repeat: DO NOT proceed. The watchers are not what they seem.",
     9),

    // --- NEBULA (depth 10-16) ---
    ("Expedition Log #4",
     "Day 18: We can hear it now. A low hum from deeper in. The instruments say nothing is there, but we can all hear it. Chen says it's trying to communicate.",
     10),
    ("Recovered Black Box",
     "Last words of the crew of the DSV Orpheus: 'It opened its eyes. Oh god, the whole void opened its eyes.'",
     11),
    ("Research Note: Evolution",
     "These creatures didn't evolve to live here. They evolved somewhere else and were... placed here. Like prisoners. Or guards.",
     12),
    ("Fragment: Ancient Text",
     "Translation (partial): '...and in the deep void we built our prisons, for what slumbers must never dream of the worlds above...'",
     13),
    ("Personal Log: Unknown Author",
     "Day ??? The compass doesn't work anymore. Neither does time. My watch says it's been 3 hours. My body says weeks. I can feel the hum in my teeth.",
     14),
    ("Radio Intercept",
     "Station control, this is Deep Outpost Seven. We are NOT alone out here. I don't mean the creatures. Something is watching through them. Request immediate extraction.",
     15),

    // --- BLACK HOLE PROXIMITY (depth 16+) ---
    ("Final Transmission",
     "They built this place to contain something. The ruins aren't ruins - they're a cage. And it's waking up.",
     16),
    ("Carved Metal (Translated)",
     "WE WHO GUARD THE DEEP VOID WARN YOU: WHAT SLEEPS BEYOND DREAMS OF YOUR WORLDS. DO NOT WAKE IT. DO NOT LISTEN TO ITS SONGS.",
     17),
    ("???",
     "The hum has stopped. That's worse. That's so much worse.",
     18),
    ("Final Entry",
     "We were wrong about everything. The void isn't hostile. It's terrified. Space itself is trying to keep us away from what lies beyond.",
     19),
    ("[UNTITLED]",
     "You found it. The deepest point. The silence is absolute. The void itself seems alive. You understand now - you were always meant to come here. It was always going to be you.",
     20),
];

/// Generates a chunk at the given position
pub fn generate_chunk(
    commands: &mut Commands,
    asset_server: &AssetServer,
    chunk_pos: IVec2,
    seed: u64,
) -> Entity {
    let mut rng = StdRng::seed_from_u64(seed ^ (chunk_pos.x as u64) ^ ((chunk_pos.y as u64) << 32));

    // Position chunk entity at its world-space location so children are offset correctly
    let chunk_world_x = chunk_pos.x as f32 * 512.0;
    let chunk_world_y = chunk_pos.y as f32 * 512.0;

    let chunk = commands.spawn((
        SpatialBundle {
            transform: Transform::from_xyz(chunk_world_x, chunk_world_y, 0.0),
            ..default()
        },
        Chunk {
            position: chunk_pos,
            is_explored: false,
        },
    )).id();

    let depth_level = -chunk_pos.y; // Lower Y = deeper

    // Skip chunks above the surface or below max depth (500m = depth_level ~10)
    if depth_level < 0 || depth_level > 11 {
        return chunk;
    }

    // The asteroid field floor is at depth_level 9-10 (roughly 450-500 units)
    // Only those chunks get terrain. Everything above is open void.
    let terrain_level = 9;
    let is_terrain_chunk = depth_level >= terrain_level;

    // --- Settlements at fixed depth intervals ---
    if depth_level > 0 && depth_level % 4 == 0 && (chunk_pos.x.abs() % 3 == 0) {
        spawn_poi(commands, asset_server, chunk, PoiType::Settlement, depth_level, &mut rng);
    }

    // --- POIs scattered in open void ---
    if depth_level > 0 && depth_level < terrain_level {
        let poi_chance = 0.20 + (depth_level as f32 * 0.02).min(0.2);
        if rng.gen::<f32>() < poi_chance {
            let poi_type = match depth_level {
                d if d < 3 => PoiType::Wreck,
                d if d < 6 => {
                    let roll = rng.gen::<f32>();
                    if roll < 0.4 { PoiType::Wreck }
                    else if roll < 0.7 { PoiType::Cave }
                    else { PoiType::ThermalVent }
                }
                _ => {
                    let roll = rng.gen::<f32>();
                    if roll < 0.3 { PoiType::Ruins }
                    else if roll < 0.6 { PoiType::Cave }
                    else if roll < 0.8 { PoiType::ThermalVent }
                    else { PoiType::Wreck }
                }
            };
            spawn_poi(commands, asset_server, chunk, poi_type, depth_level, &mut rng);
        }
    }

    // --- POIs on the terrain ---
    if is_terrain_chunk {
        if rng.gen::<f32>() < 0.4 {
            let poi_type = if rng.gen::<f32>() < 0.5 { PoiType::Ruins } else { PoiType::ThermalVent };
            spawn_poi(commands, asset_server, chunk, poi_type, depth_level, &mut rng);
        }
    }

    // Only generate terrain on deep chunks
    if is_terrain_chunk {
        spawn_decorations(commands, asset_server, chunk, depth_level, chunk_pos, seed, &mut rng);
    } else if depth_level > 0 {
        // Open void chunks get floating particles for visual reference
        spawn_void_particles(commands, chunk, depth_level, &mut rng);
    }

    chunk
}

/// Spawns floating particles/debris in open void chunks for visual reference when moving
fn spawn_void_particles(
    commands: &mut Commands,
    parent: Entity,
    depth_level: i32,
    rng: &mut StdRng,
) {
    let particle_count = rng.gen_range(8..16);
    for _ in 0..particle_count {
        let x = rng.gen_range(-250.0..250.0_f32);
        let y = rng.gen_range(-250.0..250.0_f32);
        let size = rng.gen_range(2.0..6.0_f32);

        // Particles get dimmer with depth
        let brightness = (0.4 - depth_level as f32 * 0.03).max(0.08);
        let alpha = rng.gen_range(0.15..0.4_f32);

        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgba(brightness, brightness + 0.05, brightness + 0.1, alpha),
                    custom_size: Some(Vec2::new(size, size)),
                    ..default()
                },
                transform: Transform::from_xyz(x, y, -0.1),
                ..default()
            },
            WorldDecoration { decoration_type: DecorationType::EnergySpot },
        )).set_parent(parent);
    }

    // Occasional larger debris/silt clouds
    if rng.gen::<f32>() < 0.3 {
        let x = rng.gen_range(-200.0..200.0_f32);
        let y = rng.gen_range(-200.0..200.0_f32);
        let w = rng.gen_range(20.0..60.0_f32);
        let h = rng.gen_range(10.0..30.0_f32);
        let brightness = (0.25 - depth_level as f32 * 0.02).max(0.05);

        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgba(brightness, brightness, brightness + 0.03, 0.15),
                    custom_size: Some(Vec2::new(w, h)),
                    ..default()
                },
                transform: Transform {
                    translation: Vec3::new(x, y, -0.1),
                    rotation: Quat::from_rotation_z(rng.gen_range(0.0..std::f32::consts::TAU)),
                    ..default()
                },
                ..default()
            },
            WorldDecoration { decoration_type: DecorationType::RockDebris },
        )).set_parent(parent);
    }
}

fn spawn_poi(
    commands: &mut Commands,
    asset_server: &AssetServer,
    parent: Entity,
    poi_type: PoiType,
    depth_level: i32,
    rng: &mut StdRng,
) {
    let offset = Vec2::new(
        rng.gen_range(-200.0..200.0),
        rng.gen_range(-200.0..200.0),
    );

    let (color, size) = match poi_type {
        PoiType::Wreck => (Color::rgb(0.5, 0.4, 0.3), Vec2::new(400.0, 180.0)),
        PoiType::Cave => (Color::rgb(0.15, 0.15, 0.18), Vec2::new(350.0, 280.0)),
        PoiType::Ruins => (Color::rgb(0.35, 0.35, 0.45), Vec2::new(450.0, 300.0)),
        PoiType::ThermalVent => (Color::rgb(0.8, 0.3, 0.1), Vec2::new(200.0, 320.0)),
        PoiType::Settlement => (Color::rgb(0.3, 0.7, 0.4), Vec2::new(500.0, 350.0)),
    };

    let texture = asset_server.load(sprite_map::poi_sprite_path(poi_type));

    let mut entity_commands = commands.spawn((
        SpriteBundle {
            texture,
            sprite: Sprite {
                color,
                custom_size: Some(size),
                ..default()
            },
            transform: Transform::from_xyz(offset.x, offset.y, -0.1),
            ..default()
        },
        PointOfInterest {
            poi_type,
            discovered: false,
        },
    ));

    // Wrecks get Wreck component for salvage
    if poi_type == PoiType::Wreck {
        entity_commands.insert(Wreck {
            loot_remaining: rng.gen_range(1..=4),
            is_explored: false,
        });
    }

    // ThermalVents get HazardZone component for damage
    if poi_type == PoiType::ThermalVent {
        entity_commands.insert(HazardZone {
            hazard_type: HazardType::ThermalVent,
            radius: 120.0,
            damage_per_second: 3.0 + depth_level as f32 * 0.5,
        });
    }

    // Attach log entries to Wrecks, Ruins, and Caves
    let can_have_log = matches!(poi_type, PoiType::Wreck | PoiType::Ruins | PoiType::Cave);
    if can_have_log && rng.gen::<f32>() < 0.45 {
        // Find all matching log entries for this depth range
        let matching: Vec<_> = LOG_ENTRIES.iter()
            .filter(|&&(_, _, min_depth)| depth_level >= min_depth && depth_level < min_depth + 4)
            .collect();

        if let Some(&&(title, text, min_depth)) = matching.get(rng.gen_range(0..matching.len().max(1))) {
            entity_commands.insert(LogEntry {
                title: title.to_string(),
                text: text.to_string(),
                depth_hint: min_depth as f32 * 100.0,
            });
        }
    }

    entity_commands.set_parent(parent);
}

struct DecorationConfig {
    decoration_type: DecorationType,
    base_count: i32,
    depth_multiplier: f32,
    min_depth: i32,
    max_depth: i32, // 0 = no limit
    color: Color,
    width_min: f32,
    width_max: f32,
    height_min: f32,
    height_max: f32,
    can_rotate: bool,
}

const DECORATION_CONFIGS: &[DecorationConfig] = &[
    // --- BOULDERS on terrain surface (sit on ground) ---
    DecorationConfig {
        decoration_type: DecorationType::Rock,
        base_count: 4, depth_multiplier: 0.5, min_depth: 0, max_depth: 0,
        color: Color::rgb(0.38, 0.36, 0.32),
        width_min: 50.0, width_max: 120.0, height_min: 40.0, height_max: 90.0,
        can_rotate: true,
    },
    // --- LARGE ROCK FORMATIONS (cliff-like, on terrain) ---
    DecorationConfig {
        decoration_type: DecorationType::Rock,
        base_count: 1, depth_multiplier: 0.3, min_depth: 1, max_depth: 0,
        color: Color::rgb(0.30, 0.28, 0.25),
        width_min: 120.0, width_max: 250.0, height_min: 100.0, height_max: 200.0,
        can_rotate: false,
    },
    // --- SPORE GROWTH (tall, swaying growths) ---
    DecorationConfig {
        decoration_type: DecorationType::SporeGrowth,
        base_count: 6, depth_multiplier: 0.2, min_depth: 0, max_depth: 4,
        color: Color::rgb(0.15, 0.40, 0.12),
        width_min: 12.0, width_max: 25.0, height_min: 80.0, height_max: 180.0,
        can_rotate: false,
    },
    // --- GIANT SPORE STALK (very tall, rare) ---
    DecorationConfig {
        decoration_type: DecorationType::SporeGrowth,
        base_count: 2, depth_multiplier: 0.1, min_depth: 0, max_depth: 3,
        color: Color::rgb(0.10, 0.35, 0.08),
        width_min: 20.0, width_max: 40.0, height_min: 200.0, height_max: 350.0,
        can_rotate: false,
    },
    // --- CRYSTAL CLUSTERS (colorful, on terrain) ---
    DecorationConfig {
        decoration_type: DecorationType::Crystal,
        base_count: 4, depth_multiplier: 0.2, min_depth: 0, max_depth: 6,
        color: Color::rgb(0.75, 0.35, 0.45),
        width_min: 40.0, width_max: 100.0, height_min: 30.0, height_max: 80.0,
        can_rotate: false,
    },
    // --- LARGE CRYSTAL FORMATION (massive, rare) ---
    DecorationConfig {
        decoration_type: DecorationType::Crystal,
        base_count: 1, depth_multiplier: 0.15, min_depth: 1, max_depth: 5,
        color: Color::rgb(0.8, 0.5, 0.3),
        width_min: 130.0, width_max: 220.0, height_min: 80.0, height_max: 150.0,
        can_rotate: false,
    },
    // --- BIOLUMINESCENT SPOTS (deep zone glow) ---
    DecorationConfig {
        decoration_type: DecorationType::EnergySpot,
        base_count: 0, depth_multiplier: 2.0, min_depth: 5, max_depth: 0,
        color: Color::rgb(0.1, 0.8, 0.9),
        width_min: 20.0, width_max: 60.0, height_min: 20.0, height_max: 60.0,
        can_rotate: false,
    },
    // --- THERMAL VENT COLUMNS (deep zone, rising smoke) ---
    DecorationConfig {
        decoration_type: DecorationType::ThermalVentSmoke,
        base_count: 0, depth_multiplier: 0.4, min_depth: 8, max_depth: 0,
        color: Color::rgb(0.55, 0.3, 0.1),
        width_min: 40.0, width_max: 100.0, height_min: 200.0, height_max: 400.0,
        can_rotate: false,
    },
];

// Decoration configs removed - terrain is now fully procedural in spawn_decorations()

fn spawn_decorations(
    commands: &mut Commands,
    _asset_server: &AssetServer,
    parent: Entity,
    depth_level: i32,
    chunk_pos: IVec2,
    seed: u64,
    rng: &mut StdRng,
) {
    // ================================================================
    // PROCEDURAL TERRAIN - seamless height profile using world-space hash
    // Heights are deterministic based on world X position so adjacent
    // chunks share the same edge heights (no visible seams)
    // ================================================================

    let depth_darken = (depth_level as f32 * 0.012).min(0.15);

    // Deterministic height function based on WORLD x position + seed
    // Uses multiple sine waves at different frequencies for natural terrain
    let chunk_world_x = chunk_pos.x as f32 * 512.0;
    let seed_f = (seed % 10000) as f32 * 0.1;

    let terrain_height_at = |world_x: f32| -> f32 {
        let base = 120.0;
        let h1 = (world_x * 0.005 + seed_f).sin() * 50.0;      // large rolling hills
        let h2 = (world_x * 0.013 + seed_f * 1.7).sin() * 25.0; // medium bumps
        let h3 = (world_x * 0.031 + seed_f * 2.3).sin() * 12.0; // small detail
        let h4 = (world_x * 0.067 + seed_f * 3.1).sin() * 6.0;  // fine grain
        (base + h1 + h2 + h3 + h4).max(40.0)
    };

    // --- Render terrain as overlapping columns ---
    // Use 12 wide columns with generous overlap to eliminate gaps
    let num_columns = 12;
    let col_width = 512.0 / num_columns as f32;
    let overlap = 6.0; // pixels of overlap between columns

    // Generate heights at world positions
    let mut heights: Vec<f32> = Vec::with_capacity(num_columns);
    for i in 0..num_columns {
        let world_x = chunk_world_x + (i as f32 + 0.5) * col_width;
        heights.push(terrain_height_at(world_x));
    }

    // Render each column - extends DOWN to chunk bottom, UP to terrain height
    for (i, &height) in heights.iter().enumerate() {
        let local_x = -256.0 + (i as f32 + 0.5) * col_width;

        // Terrain fill - single color per column with slight variation
        let color_seed = ((chunk_world_x + local_x) * 73.1 + seed_f).sin() * 0.5 + 0.5;
        let r = (0.24 + color_seed * 0.06 - depth_darken).max(0.04);
        let g = (0.20 + color_seed * 0.05 - depth_darken).max(0.03);
        let b = (0.14 + color_seed * 0.03 - depth_darken).max(0.02);

        // Column fills from chunk bottom (-256) up to terrain height
        // Does NOT extend below the chunk - prevents stacking
        let col_h = height + 4.0; // slight extra to fill gaps at bottom
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgb(r, g, b),
                    custom_size: Some(Vec2::new(col_width + overlap, col_h)),
                    anchor: Anchor::TopCenter,
                    ..default()
                },
                transform: Transform::from_xyz(local_x, -256.0 + height, -0.45),
                ..default()
            },
            WorldDecoration { decoration_type: DecorationType::RockDebris },
        )).set_parent(parent);

        // Surface highlight - thin lighter strip at the terrain top
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgb(
                        (r + 0.10).min(0.5),
                        (g + 0.08).min(0.4),
                        (b + 0.05).min(0.3),
                    ),
                    custom_size: Some(Vec2::new(col_width + overlap, 4.0)),
                    anchor: Anchor::TopCenter,
                    ..default()
                },
                transform: Transform::from_xyz(local_x, -256.0 + height, -0.44),
                ..default()
            },
            WorldDecoration { decoration_type: DecorationType::RockDebris },
        )).set_parent(parent);
    }

    // --- Step 3: Rock formations sitting on terrain surface ---
    let rock_count = rng.gen_range(2..5);
    for _ in 0..rock_count {
        let local_x = rng.gen_range(-240.0..240.0_f32);
        let world_x = chunk_world_x + local_x + 256.0;
        let ground_y = -256.0 + terrain_height_at(world_x);
        let x = local_x;

        let w = rng.gen_range(30.0..100.0_f32);
        let h = rng.gen_range(25.0..80.0_f32);
        let r = 0.32 + rng.gen_range(-0.08..0.08_f32) - depth_darken;
        let g = 0.30 + rng.gen_range(-0.06..0.06_f32) - depth_darken;
        let b = 0.26 + rng.gen_range(-0.05..0.05_f32) - depth_darken;

        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgb(r.max(0.05), g.max(0.05), b.max(0.05)),
                    custom_size: Some(Vec2::new(w, h)),
                    anchor: Anchor::BottomCenter,
                    ..default()
                },
                transform: Transform {
                    translation: Vec3::new(x, ground_y, -0.3),
                    rotation: Quat::from_rotation_z(rng.gen_range(-0.1..0.1)),
                    ..default()
                },
                ..default()
            },
            WorldDecoration { decoration_type: DecorationType::Rock },
        )).set_parent(parent);
    }

    // --- Step 4: Large cliff/boulder features (rare, impressive) ---
    if rng.gen::<f32>() < 0.4 {
        let local_x = rng.gen_range(-180.0..180.0_f32);
        let world_x = chunk_world_x + local_x + 256.0;
        let ground_y = -256.0 + terrain_height_at(world_x);
        let x = local_x;

        let w = rng.gen_range(80.0..200.0_f32);
        let h = rng.gen_range(100.0..250.0_f32);

        // Dark rock cliff
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgb(
                        (0.20 - depth_darken).max(0.04),
                        (0.18 - depth_darken).max(0.03),
                        (0.16 - depth_darken).max(0.03),
                    ),
                    custom_size: Some(Vec2::new(w, h)),
                    anchor: Anchor::BottomCenter,
                    ..default()
                },
                transform: Transform {
                    translation: Vec3::new(x, ground_y, -0.32),
                    rotation: Quat::from_rotation_z(rng.gen_range(-0.08..0.08)),
                    ..default()
                },
                ..default()
            },
            WorldDecoration { decoration_type: DecorationType::Rock },
        )).set_parent(parent);
    }

    // --- Step 5: Cave openings (dark holes in the terrain) ---
    if rng.gen::<f32>() < 0.25 && depth_level >= 1 {
        let local_x = rng.gen_range(-150.0..150.0_f32);
        let world_x = chunk_world_x + local_x + 256.0;
        let ground_y = -256.0 + terrain_height_at(world_x);
        let x = local_x;

        let w = rng.gen_range(60.0..140.0_f32);
        let h = rng.gen_range(40.0..90.0_f32);

        // Dark cave interior
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgb(0.02, 0.015, 0.025),
                    custom_size: Some(Vec2::new(w, h)),
                    anchor: Anchor::BottomCenter,
                    ..default()
                },
                transform: Transform::from_xyz(x, ground_y - h * 0.3, -0.28),
                ..default()
            },
            WorldDecoration { decoration_type: DecorationType::Rock },
        )).set_parent(parent);

        // Cave arch - rock rim above the opening
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgb(
                        (0.25 - depth_darken).max(0.05),
                        (0.22 - depth_darken).max(0.04),
                        (0.18 - depth_darken).max(0.03),
                    ),
                    custom_size: Some(Vec2::new(w + 40.0, 20.0)),
                    anchor: Anchor::BottomCenter,
                    ..default()
                },
                transform: Transform::from_xyz(x, ground_y + 5.0, -0.27),
                ..default()
            },
            WorldDecoration { decoration_type: DecorationType::Rock },
        )).set_parent(parent);
    }

    // --- Step 6: Vegetation anchored to terrain surface ---

    // Spore growths (near orbit to mid distance)
    if depth_level <= 5 {
        let spore_count = rng.gen_range(3..8).min(8 - depth_level).max(0);
        for _ in 0..spore_count {
            let local_x = rng.gen_range(-240.0..240.0_f32);
            let world_x = chunk_world_x + local_x + 256.0;
            let ground_y = -256.0 + terrain_height_at(world_x);
            let x = local_x;

            let w = rng.gen_range(8.0..20.0_f32);
            let h = rng.gen_range(60.0..200.0_f32);
            let sway = rng.gen_range(-0.15..0.15_f32);

            let green = 0.30 + rng.gen_range(-0.1..0.1_f32);
            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color: Color::rgba(0.10, green, 0.08, 0.8),
                        custom_size: Some(Vec2::new(w, h)),
                        anchor: Anchor::BottomCenter,
                        ..default()
                    },
                    transform: Transform {
                        translation: Vec3::new(x, ground_y, -0.2),
                        rotation: Quat::from_rotation_z(sway),
                        ..default()
                    },
                    ..default()
                },
                WorldDecoration { decoration_type: DecorationType::SporeGrowth },
            )).set_parent(parent);
        }
    }

    // Crystal clusters (near orbit to mid distance)
    if depth_level >= 1 && depth_level <= 6 {
        let crystal_count = rng.gen_range(1..4);
        for _ in 0..crystal_count {
            let local_x = rng.gen_range(-230.0..230.0_f32);
            let world_x = chunk_world_x + local_x + 256.0;
            let ground_y = -256.0 + terrain_height_at(world_x);
            let x = local_x;

            let w = rng.gen_range(30.0..90.0_f32);
            let h = rng.gen_range(20.0..60.0_f32);

            // Crystal colors: pinks, oranges, purples
            let hue = rng.gen_range(0..3);
            let color = match hue {
                0 => Color::rgb(0.75 + rng.gen_range(-0.1..0.1), 0.30, 0.40),
                1 => Color::rgb(0.80, 0.50 + rng.gen_range(-0.1..0.1), 0.25),
                _ => Color::rgb(0.55, 0.30, 0.65 + rng.gen_range(-0.1..0.1)),
            };

            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color,
                        custom_size: Some(Vec2::new(w, h)),
                        anchor: Anchor::BottomCenter,
                        ..default()
                    },
                    transform: Transform::from_xyz(x, ground_y, -0.22),
                    ..default()
                },
                WorldDecoration { decoration_type: DecorationType::Crystal },
            )).set_parent(parent);
        }
    }

    // Bioluminescent spots (deep zones)
    if depth_level >= 5 {
        let glow_count = rng.gen_range(1..5).min((depth_level - 4) * 2);
        for _ in 0..glow_count {
            let x = rng.gen_range(-240.0..240.0_f32);
            let y = rng.gen_range(-240.0..200.0_f32);
            let size = rng.gen_range(15.0..50.0_f32);

            let color = if rng.gen::<f32>() < 0.5 {
                Color::rgba(0.1, 0.7, 0.9, 0.5)  // cyan glow
            } else {
                Color::rgba(0.2, 0.9, 0.4, 0.4)  // green glow
            };

            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color,
                        custom_size: Some(Vec2::new(size, size)),
                        ..default()
                    },
                    transform: Transform::from_xyz(x, y, -0.15),
                    ..default()
                },
                WorldDecoration { decoration_type: DecorationType::EnergySpot },
            )).set_parent(parent);
        }
    }

    // Thermal vent smoke columns (very deep)
    if depth_level >= 8 && rng.gen::<f32>() < 0.4 {
        let local_x = rng.gen_range(-180.0..180.0_f32);
        let world_x = chunk_world_x + local_x + 256.0;
        let ground_y = -256.0 + terrain_height_at(world_x);
        let x = local_x;

        let w = rng.gen_range(30.0..80.0_f32);
        let h = rng.gen_range(150.0..350.0_f32);

        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgba(0.5, 0.25, 0.1, 0.4),
                    custom_size: Some(Vec2::new(w, h)),
                    anchor: Anchor::BottomCenter,
                    ..default()
                },
                transform: Transform::from_xyz(x, ground_y, -0.18),
                ..default()
            },
            WorldDecoration { decoration_type: DecorationType::ThermalVentSmoke },
        )).set_parent(parent);
    }
}
