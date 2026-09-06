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
    /// Opacity at birth.
    ///
    /// Fading multiplies THIS by the remaining life ratio. Reading the
    /// sprite's current alpha instead compounded the fade every frame — the
    /// product of sixty ratios a second collapses almost immediately, so a
    /// particle was invisible long before its lifetime ran out and anything
    /// meant to linger simply could not.
    pub base_alpha: f32,
}

impl Default for Particle {
    fn default() -> Self {
        Self {
            velocity: Vec2::ZERO,
            lifetime: 0.5,
            max_lifetime: 0.5,
            fade: true,
            shrink: true,
            base_alpha: 1.0,
        }
    }
}

impl Particle {
    /// A fully opaque particle that fades and shrinks over `life` seconds.
    pub fn new(velocity: Vec2, life: f32) -> Self {
        Self { velocity, lifetime: life, max_lifetime: life, ..default() }
    }

    /// As `new`, but starting at `alpha` and optionally holding its size.
    pub fn wisp(velocity: Vec2, life: f32, alpha: f32, shrink: bool) -> Self {
        Self { velocity, lifetime: life, max_lifetime: life, base_alpha: alpha, shrink, ..default() }
    }
}

/// Spawn engine exhaust particles behind active engines when thrusting
pub fn spawn_engine_particles(
    time: Res<Time>,
    engine_query: Query<(&Engine, &Module, &GlobalTransform), Without<DestroyedModule>>,
    ship_physics: Query<&ShipPhysics, With<Ship>>,
    mut commands: Commands,
    mut spawn_timer: Local<f32>,
) {
    *spawn_timer += time.delta_secs();
    if *spawn_timer < 0.05 { return; } // 20 particles/sec max per engine
    *spawn_timer = 0.0;

    let Ok(physics) = ship_physics.single() else { return };
    if physics.throttle.abs() < 0.1 { return; } // Not thrusting

    let mut rng = rand::thread_rng();

    for (_engine, module, global_transform) in engine_query.iter() {
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
            let color = Color::srgba(
                0.5 + heat * 0.5,
                0.3 + heat * 0.4,
                0.8 * (1.0 - heat * 0.5),
                0.8,
            );

            commands.spawn((
                (Sprite {
                        color,
                        custom_size: Some(Vec2::splat(size)),
                        ..default()
                    }, Transform::from_xyz(pos.x, pos.y, 0.5)),
                Particle::wisp(vel, lifetime, 0.8, true),
            ));
        }
    }
}

/// Spawn air particles escaping from hull breaches
pub fn spawn_breach_particles(
    time: Res<Time>,
    fx: Res<crate::vfx::effect_textures::EffectTextures>,
    hull_query: Query<(&HullSegment, &GlobalTransform)>,
    mut commands: Commands,
    mut spawn_timer: Local<f32>,
) {
    *spawn_timer += time.delta_secs();
    if *spawn_timer < 0.15 { return; }
    *spawn_timer = 0.0;

    let mut rng = rand::thread_rng();

    for (hull, global_transform) in hull_query.iter() {
        if !hull.is_depressurized || hull.depressurization_level < 0.1 { continue; }

        let pos = global_transform.translation().truncate();
        let intensity = hull.depressurization_level;
        let particle_count = (intensity * 4.0) as u32;

        // A breach vents OUTWARD, and which way it points is the information:
        // it tells you which side of the hull is holed. This used to pick a
        // random angle over the full circle, which reads as a puff sitting on
        // the ship rather than as air leaving it.
        //
        // The segment's grid position is ship-local, so its direction from the
        // origin is roughly "away from the middle"; rotating that through the
        // segment's own transform puts it in world space and keeps the jet
        // pointing correctly as the ship turns.
        let local_out = Vec2::new(
            hull.grid_position.x as f32,
            hull.grid_position.y as f32,
        );
        let out = global_transform
            .affine()
            .transform_vector3(local_out.extend(0.0))
            .truncate()
            .normalize_or_zero();

        for _ in 0..particle_count.min(3) {
            // Fall back to a full-circle puff for a block sitting on the ship's
            // own centreline, where "outward" genuinely has no answer.
            let dir = if out == Vec2::ZERO {
                let a = rng.gen_range(0.0..std::f32::consts::TAU);
                Vec2::new(a.cos(), a.sin())
            } else {
                Vec2::from_angle(rng.gen_range(-0.45..0.45)).rotate(out)
            };
            let speed = rng.gen_range(70.0..150.0) * intensity;
            let lifetime = rng.gen_range(0.5..1.1);
            let size = rng.gen_range(8.0..16.0);

            // Real decompression reads as a condensation fog, not the ice
            // glitter the films use -- so: pale, soft, and thinning fast.
            commands.spawn((
                (Sprite {
                        image: fx.puff(),
                        color: Color::srgba(0.78, 0.85, 1.0, 0.42 * intensity),
                        custom_size: Some(Vec2::splat(size)),
                        ..default()
                    }, Transform::from_xyz(pos.x, pos.y, 0.6)),
                Particle::wisp(dir * speed, lifetime, 0.42 * intensity, false),
            ));
        }
    }
}

/// An expanding, fading blast sphere.
///
/// `HitEffect` is a fixed-size square that sits still for 0.2s, which is
/// adequate for a bullet strike and reads as nothing at all for a warhead.
/// This grows and cools instead, so the size of a detonation is something you
/// can actually see.
#[derive(Component)]
pub struct Blast {
    pub elapsed: f32,
    pub duration: f32,
    pub from: f32,
    pub to: f32,
    pub hot: Color,
    pub cool: Color,
}

/// Grow, cool and fade every blast, then despawn it.
pub fn update_blasts(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut Blast, &mut Transform, &mut Sprite)>,
) {
    let dt = time.delta_secs();

    for (entity, mut blast, mut transform, mut sprite) in query.iter_mut() {
        blast.elapsed += dt;
        let t = (blast.elapsed / blast.duration).clamp(0.0, 1.0);

        if t >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }

        // Ease out: a detonation is fastest at the instant it goes off and
        // coasts to its full width, rather than growing at a constant rate.
        let eased = 1.0 - (1.0 - t).powi(3);
        let size = blast.from + (blast.to - blast.from) * eased;
        transform.scale = Vec3::splat(size);

        let hot = blast.hot.to_srgba();
        let cool = blast.cool.to_srgba();
        sprite.color = Color::srgba(
            hot.red + (cool.red - hot.red) * t,
            hot.green + (cool.green - hot.green) * t,
            hot.blue + (cool.blue - hot.blue) * t,
            // Hold opacity through the first half, then fall away — a
            // fireball does not start fading the moment it appears.
            (1.0 - t).powi(2) * (hot.alpha + (cool.alpha - hot.alpha) * t),
        );
    }
}

/// Update all particles: move, age, fade, shrink, despawn
pub fn update_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particle_query: Query<(Entity, &mut Particle, &mut Transform, &mut Sprite)>,
) {
    let dt = time.delta_secs();

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
            sprite.color.set_alpha(particle.base_alpha * life_ratio.clamp(0.0, 1.0));
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
