use bevy::prelude::*;
use rand::Rng;
use crate::components::*;

// ============================================================================
// PARTICLE SYSTEM
// Lightweight sprite-based particles for engine exhaust, weapon fire,
// hull breaches, and explosions. Each particle is a small sprite entity
// with velocity, lifetime, and fade behavior.
// ============================================================================

/// A single particle entity
#[derive(Component)]
pub struct Particle {
    pub velocity: Vec2,
    pub lifetime: f32,
    pub max_lifetime: f32,
    pub fade: bool,
    pub shrink: bool,
}

/// Spawn engine exhaust particles behind active engines when thrusting
pub fn spawn_engine_particles(
    time: Res<Time>,
    engine_query: Query<(&Engine, &Module, &GlobalTransform), Without<DestroyedModule>>,
    sub_physics: Query<&SubmarinePhysics, With<Submarine>>,
    mut commands: Commands,
    mut spawn_timer: Local<f32>,
) {
    *spawn_timer += time.delta_seconds();
    if *spawn_timer < 0.05 { return; } // 20 particles/sec max per engine
    *spawn_timer = 0.0;

    let Ok(physics) = sub_physics.get_single() else { return };
    if physics.throttle.abs() < 0.1 { return; } // Not thrusting

    let mut rng = rand::thread_rng();

    for (engine, module, global_transform) in engine_query.iter() {
        if !module.is_active { continue; }

        let pos = global_transform.translation().truncate();
        let facing = Vec2::new(physics.rotation.cos(), physics.rotation.sin());
        // Exhaust goes opposite to facing direction
        let exhaust_dir = -facing;

        let intensity = physics.throttle.abs();
        let particle_count = (intensity * 3.0) as u32;

        for _ in 0..particle_count {
            let spread = Vec2::new(
                rng.gen_range(-0.3..0.3),
                rng.gen_range(-0.3..0.3),
            );
            let vel = (exhaust_dir + spread).normalize_or_zero() * rng.gen_range(80.0..200.0);
            let lifetime = rng.gen_range(0.2..0.5);
            let size = rng.gen_range(3.0..8.0);

            // Color: blue-white core fading to orange
            let heat = rng.gen_range(0.5..1.0);
            let color = Color::rgba(
                0.5 + heat * 0.5,
                0.3 + heat * 0.4,
                0.8 * (1.0 - heat * 0.5),
                0.8,
            );

            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color,
                        custom_size: Some(Vec2::splat(size)),
                        ..default()
                    },
                    transform: Transform::from_xyz(pos.x, pos.y, 0.5),
                    ..default()
                },
                Particle {
                    velocity: vel,
                    lifetime,
                    max_lifetime: lifetime,
                    fade: true,
                    shrink: true,
                },
            ));
        }
    }
}

/// Spawn air particles escaping from hull breaches
pub fn spawn_breach_particles(
    time: Res<Time>,
    hull_query: Query<(&HullSegment, &GlobalTransform)>,
    mut commands: Commands,
    mut spawn_timer: Local<f32>,
) {
    *spawn_timer += time.delta_seconds();
    if *spawn_timer < 0.15 { return; }
    *spawn_timer = 0.0;

    let mut rng = rand::thread_rng();

    for (hull, global_transform) in hull_query.iter() {
        if !hull.is_depressurized || hull.depressurization_level < 0.1 { continue; }

        let pos = global_transform.translation().truncate();
        let intensity = hull.depressurization_level;
        let particle_count = (intensity * 4.0) as u32;

        for _ in 0..particle_count.min(3) {
            let angle = rng.gen_range(0.0..std::f32::consts::TAU);
            let speed = rng.gen_range(30.0..80.0) * intensity;
            let vel = Vec2::new(angle.cos() * speed, angle.sin() * speed);
            let lifetime = rng.gen_range(0.5..1.5);
            let size = rng.gen_range(2.0..5.0);

            // White-blue air particles
            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color: Color::rgba(0.7, 0.8, 1.0, 0.5 * intensity),
                        custom_size: Some(Vec2::splat(size)),
                        ..default()
                    },
                    transform: Transform::from_xyz(pos.x, pos.y, 0.6),
                    ..default()
                },
                Particle {
                    velocity: vel,
                    lifetime,
                    max_lifetime: lifetime,
                    fade: true,
                    shrink: true,
                },
            ));
        }
    }
}

/// Update all particles: move, age, fade, shrink, despawn
pub fn update_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particle_query: Query<(Entity, &mut Particle, &mut Transform, &mut Sprite)>,
) {
    let dt = time.delta_seconds();

    for (entity, mut particle, mut transform, mut sprite) in particle_query.iter_mut() {
        // Move
        transform.translation.x += particle.velocity.x * dt;
        transform.translation.y += particle.velocity.y * dt;

        // Age
        particle.lifetime -= dt;

        if particle.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        let life_ratio = particle.lifetime / particle.max_lifetime;

        // Fade
        if particle.fade {
            let current_a = sprite.color.a().min(1.0);
            sprite.color.set_a(life_ratio.clamp(0.0, 1.0) * current_a);
        }

        // Shrink
        if particle.shrink {
            let scale = life_ratio.clamp(0.1, 1.0);
            transform.scale = Vec3::splat(scale);
        }

        // Slow down over time (drag-like for particles only — visual, not physics)
        particle.velocity *= 0.98;
    }
}
