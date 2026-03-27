use bevy::prelude::*;
use bevy::sprite::TextureAtlasSprite;
use rand::Rng;
use crate::components::*;
use crate::resources::*;
use crate::sprite_map;

/// Maintains a pool of ambient creatures around the submarine.
/// Spawns depth-appropriate life, despawns when too far.
/// Uses a warmup ramp to avoid a burst of creatures on first entering Exploring.
pub fn spawn_ambient_life(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlases: ResMut<Assets<TextureAtlas>>,
    time: Res<Time>,
    sub_state: Res<DepthState>,
    sub_query: Query<&Transform, With<Submarine>>,
    ambient_query: Query<&AmbientCreature>,
    mut spawn_timer: Local<f32>,
    mut warmup_elapsed: Local<f32>,
) {
    let dt = time.delta_seconds();

    // Track how long we've been in Exploring (resets when Local resets on state change)
    *warmup_elapsed += dt;
    let warmup_duration = 15.0; // seconds to ramp up to full spawn rate
    let warmup_factor = (*warmup_elapsed / warmup_duration).min(1.0);

    // During warmup, spawn less frequently (2.0s at start, ramping down to 0.5s)
    let spawn_interval = 0.5 + 1.5 * (1.0 - warmup_factor);

    *spawn_timer += dt;
    if *spawn_timer < spawn_interval {
        return;
    }
    *spawn_timer = 0.0;

    let Ok(sub_transform) = sub_query.get_single() else { return };
    let sub_pos = sub_transform.translation.truncate();
    let depth = sub_state.current_depth;

    let mut rng = rand::thread_rng();

    // Count current ambient creatures
    let current_count = ambient_query.iter().count();

    // Target counts by depth zone (kept moderate for performance)
    let (target_fish, target_jelly, target_school, target_deep) = match depth {
        d if d < 200.0 => (15, 4, 6, 0),         // Light: lively but not overwhelming
        d if d < 500.0 => (10, 5, 4, 0),          // Twilight: less fish, more jelly
        d if d < 1000.0 => (6, 4, 2, 3),          // Dark: sparse, deep fish appear
        d if d < 2000.0 => (2, 2, 0, 5),          // Abyss: almost empty, eerie deep fish
        _ => (0, 2, 0, 4),                         // Trench: alien, minimal
    };

    let target_total = target_fish + target_jelly + target_school + target_deep;

    // Spawn a few per tick until we reach target (don't spawn all at once)
    if current_count >= target_total {
        return;
    }

    // During warmup, limit batch size (1 at start, ramping to 4)
    let max_batch = (1.0 + 3.0 * warmup_factor).round() as usize;
    let batch = (target_total - current_count).min(max_batch);

    for _ in 0..batch {
        // Pick what to spawn based on remaining need
        let roll = rng.gen_range(0..target_total.max(1));
        let kind = if roll < target_fish {
            AmbientKind::SmallFish
        } else if roll < target_fish + target_jelly {
            AmbientKind::Jellyfish
        } else if roll < target_fish + target_jelly + target_school {
            AmbientKind::SchoolFish
        } else {
            AmbientKind::DeepFish
        };

        // Spawn at edge of visibility (500-800 units away, never close to sub)
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let dist = rng.gen_range(500.0..800.0);
        let pos = sub_pos + Vec2::new(angle.cos() * dist, angle.sin() * dist);

        spawn_ambient(&mut commands, &asset_server, &mut texture_atlases, kind, pos, &mut rng);

        // SchoolFish spawn in clusters of 3-5
        if kind == AmbientKind::SchoolFish {
            let group_size = rng.gen_range(2..5);
            for _ in 0..group_size {
                let offset = Vec2::new(rng.gen_range(-15.0..15.0), rng.gen_range(-10.0..10.0));
                spawn_ambient(&mut commands, &asset_server, &mut texture_atlases, AmbientKind::SchoolFish, pos + offset, &mut rng);
            }
        }
    }

    // Rare spawns: Giant Squid (depth 5+) and Whale (depth 2+)
    if depth >= 500.0 && rng.gen::<f32>() < 0.002 {
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let rare_dist = rng.gen_range(700.0..1000.0);
        let pos = sub_pos + Vec2::new(angle.cos() * rare_dist, angle.sin() * rare_dist);
        spawn_ambient(&mut commands, &asset_server, &mut texture_atlases, AmbientKind::GiantSquid, pos, &mut rng);
    }
    if depth >= 100.0 && rng.gen::<f32>() < 0.003 {
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let rare_dist = rng.gen_range(700.0..1000.0);
        let pos = sub_pos + Vec2::new(angle.cos() * rare_dist, angle.sin() * rare_dist);
        spawn_ambient(&mut commands, &asset_server, &mut texture_atlases, AmbientKind::Whale, pos, &mut rng);
    }
}

fn spawn_ambient(
    commands: &mut Commands,
    asset_server: &AssetServer,
    texture_atlases: &mut Assets<TextureAtlas>,
    kind: AmbientKind,
    pos: Vec2,
    rng: &mut impl Rng,
) {
    // Random size variation per individual (0.6x to 1.5x)
    let size_scale = rng.gen_range(0.6..1.5_f32);

    let (color, base_w, base_h, speed) = match kind {
        AmbientKind::SmallFish => {
            // Silver / light gray
            let brightness = rng.gen_range(0.7..0.9);
            (Color::rgba(brightness, brightness, brightness + 0.05, 0.85),
             18.0, 9.0, rng.gen_range(20.0..45.0))
        }
        AmbientKind::Jellyfish => {
            // Translucent cyan / pink
            let variant = rng.gen_range(0.0..1.0_f32);
            let (r, g, b) = if variant < 0.5 {
                // Cyan variant
                (rng.gen_range(0.3..0.5), rng.gen_range(0.7..0.9), rng.gen_range(0.8..1.0))
            } else {
                // Pink variant
                (rng.gen_range(0.8..1.0), rng.gen_range(0.3..0.5), rng.gen_range(0.6..0.85))
            };
            (Color::rgba(r, g, b, 0.4),
             30.0, 39.0, rng.gen_range(3.0..8.0))
        }
        AmbientKind::SchoolFish => {
            // Golden / yellow
            let gold = rng.gen_range(0.8..1.0);
            (Color::rgba(gold, rng.gen_range(0.65..0.8), rng.gen_range(0.1..0.3), 0.85),
             27.0, 13.5, rng.gen_range(25.0..40.0))
        }
        AmbientKind::DeepFish => {
            // Pale bioluminescent blue
            let glow = rng.gen_range(0.5..0.8);
            (Color::rgba(0.4, 0.6 + glow * 0.2, 0.9 + glow * 0.1, 0.7),
             45.0, 22.5, rng.gen_range(8.0..18.0))
        }
        AmbientKind::GiantSquid => {
            // Dark red
            let r = rng.gen_range(0.5..0.7);
            (Color::rgba(r, rng.gen_range(0.08..0.15), rng.gen_range(0.08..0.12), 0.55),
             330.0, 90.0, rng.gen_range(10.0..20.0))
        }
        AmbientKind::Whale => {
            // Dark blue-gray
            let base = rng.gen_range(0.25..0.35);
            (Color::rgba(base, base + 0.02, base + 0.1, 0.6),
             480.0, 150.0, rng.gen_range(12.0..22.0))
        }
    };

    let w = base_w * size_scale;
    let h = base_h * size_scale;
    // Bigger creatures are slower
    let speed = speed / size_scale.sqrt();

    // Sprite sheet info per ambient type
    let (frame_w, frame_h, num_frames) = match kind {
        AmbientKind::SmallFish =>   (32, 32, 4),
        AmbientKind::Jellyfish =>   (64, 64, 4),
        AmbientKind::SchoolFish =>  (64, 64, 4),
        AmbientKind::DeepFish =>    (64, 64, 4),
        AmbientKind::GiantSquid =>  (96, 64, 4),
        AmbientKind::Whale =>       (128, 64, 4),
    };

    let texture: Handle<Image> = asset_server.load(sprite_map::ambient_sprite_path(kind));
    let atlas = TextureAtlas::from_grid(
        texture,
        Vec2::new(frame_w as f32, frame_h as f32),
        num_frames, 1, None, None,
    );

    let angle = rng.gen_range(0.0..std::f32::consts::TAU);
    let base_vel = Vec2::new(angle.cos() * speed, angle.sin() * speed);
    let base_vel = if kind == AmbientKind::Jellyfish {
        Vec2::new(rng.gen_range(-3.0..3.0), -speed)
    } else {
        base_vel
    };

    let anim_speed = match kind {
        AmbientKind::Whale => 0.25,
        AmbientKind::GiantSquid => 0.2,
        AmbientKind::Jellyfish => 0.3,
        _ => 0.15,
    };

    commands.spawn((
        SpriteSheetBundle {
            texture_atlas: texture_atlases.add(atlas),
            sprite: TextureAtlasSprite {
                index: 0,
                color,
                custom_size: Some(Vec2::new(w, h)),
                ..default()
            },
            transform: Transform {
                translation: Vec3::new(pos.x, pos.y, -0.15),
                rotation: if kind != AmbientKind::Jellyfish {
                    Quat::from_rotation_z(base_vel.y.atan2(base_vel.x))
                } else {
                    Quat::IDENTITY
                },
                ..default()
            },
            ..default()
        },
        AmbientCreature {
            kind,
            health: match kind {
                AmbientKind::SmallFish => 5.0 * size_scale,
                AmbientKind::Jellyfish => 3.0 * size_scale,
                AmbientKind::SchoolFish => 4.0 * size_scale,
                AmbientKind::DeepFish => 8.0 * size_scale,
                AmbientKind::GiantSquid => 50.0 * size_scale,
                AmbientKind::Whale => 200.0 * size_scale,
            },
            food_value: match kind {
                AmbientKind::SmallFish => 5.0 * size_scale,
                AmbientKind::Jellyfish => 3.0 * size_scale,
                AmbientKind::SchoolFish => 4.0 * size_scale,
                AmbientKind::DeepFish => 8.0 * size_scale,
                AmbientKind::GiantSquid => 40.0 * size_scale,
                AmbientKind::Whale => 80.0 * size_scale,
            },
        },
        CreatureAnimation {
            timer: Timer::from_seconds(anim_speed, TimerMode::Repeating),
            swim_frames: num_frames,
            attack_frames: 0, // ambient creatures don't attack
            total_frames: num_frames,
            current_frame: 0,
        },
        Velocity(base_vel),
        ScatterBehavior {
            scatter_timer: 0.0,
            base_velocity: base_vel,
        },
    ));
}

/// Ultra-simple movement: drift + scatter from sub
pub fn ambient_movement(
    time: Res<Time>,
    sub_query: Query<&Transform, With<Submarine>>,
    mut ambient_query: Query<
        (&mut Transform, &AmbientCreature, &mut Velocity, &mut ScatterBehavior),
        Without<Submarine>,
    >,
) {
    let sub_pos = sub_query
        .get_single()
        .map(|t| t.translation.truncate())
        .unwrap_or(Vec2::ZERO);
    let dt = time.delta_seconds();

    for (mut transform, ambient, mut velocity, mut scatter) in ambient_query.iter_mut() {
        let pos = transform.translation.truncate();
        let to_sub = sub_pos - pos;
        let dist_to_sub = to_sub.length();

        // Scatter range depends on creature type
        let scatter_dist = match ambient.kind {
            AmbientKind::SmallFish | AmbientKind::SchoolFish => 80.0,
            AmbientKind::Jellyfish => 40.0,   // barely reacts
            AmbientKind::DeepFish => 60.0,
            AmbientKind::GiantSquid | AmbientKind::Whale => 0.0, // don't scatter
        };

        if scatter_dist > 0.0 && dist_to_sub < scatter_dist && scatter.scatter_timer <= 0.0 {
            // Scatter! Flee away from sub
            let flee_dir = (pos - sub_pos).normalize_or_zero();
            let scatter_speed = match ambient.kind {
                AmbientKind::SmallFish => 120.0,
                AmbientKind::SchoolFish => 100.0,
                _ => 50.0,
            };
            velocity.0 = flee_dir * scatter_speed;
            scatter.scatter_timer = 1.5; // scatter for 1.5 seconds
        }

        if scatter.scatter_timer > 0.0 {
            scatter.scatter_timer -= dt;
            if scatter.scatter_timer <= 0.0 {
                // Resume normal swimming
                velocity.0 = scatter.base_velocity;
            }
        }

        // Jellyfish: gentle bob
        if ambient.kind == AmbientKind::Jellyfish {
            let bob = (time.elapsed_seconds() * 1.5 + pos.x * 0.01).sin() * 2.0;
            velocity.0.y = scatter.base_velocity.y + bob;
        }

        // Apply velocity
        transform.translation.x += velocity.0.x * dt;
        transform.translation.y += velocity.0.y * dt;

        // Face movement direction (not jellyfish - they face down)
        if ambient.kind != AmbientKind::Jellyfish && velocity.0.length_squared() > 1.0 {
            transform.rotation = Quat::from_rotation_z(velocity.0.y.atan2(velocity.0.x));
        }
    }
}

/// Despawn ambient creatures that are too far from the sub
pub fn cleanup_ambient(
    mut commands: Commands,
    sub_query: Query<&Transform, With<Submarine>>,
    ambient_query: Query<(Entity, &Transform), With<AmbientCreature>>,
) {
    let Ok(sub_transform) = sub_query.get_single() else { return };
    let sub_pos = sub_transform.translation.truncate();

    for (entity, transform) in ambient_query.iter() {
        let dist = transform.translation.truncate().distance(sub_pos);
        if dist > 800.0 {
            commands.entity(entity).despawn_recursive();
        }
    }
}
