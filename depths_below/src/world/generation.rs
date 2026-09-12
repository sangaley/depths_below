use bevy::prelude::*;
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
        Transform::from_xyz(chunk_world_x, chunk_world_y, 0.0),
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

    // Depth 9+ used to be "seafloor" and got generated terrain; there is no
    // terrain any more (rock is celestial-layer asteroids now), but the band
    // is still a useful divider for WHICH points of interest appear: deep
    // chunks favour ruins and vents, shallow ones wrecks and caves.
    let deep_level = 9;
    let is_deep_chunk = depth_level >= deep_level;

    // --- Settlements at fixed depth intervals ---
    if depth_level > 0 && depth_level % 4 == 0 && (chunk_pos.x.abs() % 3 == 0) {
        spawn_poi(commands, asset_server, chunk, PoiType::Settlement, depth_level, &mut rng);
    }

    // --- POIs scattered in open void ---
    if depth_level > 0 && depth_level < deep_level {
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

    // --- POIs in the deep band ---
    if is_deep_chunk {
        if rng.gen::<f32>() < 0.4 {
            let poi_type = if rng.gen::<f32>() < 0.5 { PoiType::Ruins } else { PoiType::ThermalVent };
            spawn_poi(commands, asset_server, chunk, poi_type, depth_level, &mut rng);
        }
    }

    // Drifting particulate, for a sense of motion between the big rocks.
    // The old seafloor branch (terrain columns, cliffs, cave mouths, spore
    // stalks anchored to a ground line) was submarine-era code generating a
    // seabed in open space; asteroids now come from the celestial layer
    // (celestial::spawning::spawn_asteroid_field), which already carries
    // collision, mining and per-system streaming.
    if depth_level > 0 {
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
            (Sprite {
                    color: Color::srgba(brightness, brightness + 0.05, brightness + 0.1, alpha),
                    custom_size: Some(Vec2::new(size, size)),
                    ..default()
                }, Transform::from_xyz(x, y, -0.1)),
            WorldDecoration { decoration_type: DecorationType::Dust },
        )).insert(ChildOf(parent));
    }

    // Occasional larger debris/silt clouds
    if rng.gen::<f32>() < 0.3 {
        let x = rng.gen_range(-200.0..200.0_f32);
        let y = rng.gen_range(-200.0..200.0_f32);
        let w = rng.gen_range(20.0..60.0_f32);
        let h = rng.gen_range(10.0..30.0_f32);
        let brightness = (0.25 - depth_level as f32 * 0.02).max(0.05);

        commands.spawn((
            (Sprite {
                    color: Color::srgba(brightness, brightness, brightness + 0.03, 0.15),
                    custom_size: Some(Vec2::new(w, h)),
                    ..default()
                }, Transform {
                    translation: Vec3::new(x, y, -0.1),
                    rotation: Quat::from_rotation_z(rng.gen_range(0.0..std::f32::consts::TAU)),
                    ..default()
                }),
            WorldDecoration { decoration_type: DecorationType::Dust },
        )).insert(ChildOf(parent));
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
        PoiType::Wreck => (Color::srgb(0.5, 0.4, 0.3), Vec2::new(400.0, 180.0)),
        PoiType::Cave => (Color::srgb(0.15, 0.15, 0.18), Vec2::new(350.0, 280.0)),
        PoiType::Ruins => (Color::srgb(0.35, 0.35, 0.45), Vec2::new(450.0, 300.0)),
        PoiType::ThermalVent => (Color::srgb(0.8, 0.3, 0.1), Vec2::new(200.0, 320.0)),
        PoiType::Settlement => (Color::srgb(0.3, 0.7, 0.4), Vec2::new(500.0, 350.0)),
    };

    let texture = asset_server.load(sprite_map::poi_sprite_path(poi_type));

    let mut entity_commands = commands.spawn((
        (Sprite {
                image: texture,
                color,
                custom_size: Some(size),
                ..default()
            }, Transform::from_xyz(offset.x, offset.y, -0.1)),
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

    entity_commands.insert(ChildOf(parent));
}
