use bevy::prelude::*;

use crate::components::{NoiseTrailPoint, Submarine};
use crate::resources::{EcosystemConfig, NoiseState};

/// Timer for noise trail emission
#[derive(Resource)]
pub struct NoiseTrailTimer {
    pub timer: Timer,
}

impl Default for NoiseTrailTimer {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(0.5, TimerMode::Repeating),
        }
    }
}

/// Every 0.5s, if submarine noise > 10, spawn invisible NoiseTrailPoint at sub position
pub fn emit_noise_trail(
    time: Res<Time>,
    mut trail_timer: ResMut<NoiseTrailTimer>,
    noise_state: Option<Res<NoiseState>>,
    sub_query: Query<&Transform, With<Submarine>>,
    existing_trails: Query<Entity, With<NoiseTrailPoint>>,
    eco_config: Res<EcosystemConfig>,
    mut commands: Commands,
) {
    trail_timer.timer.tick(time.delta());

    if !trail_timer.timer.just_finished() {
        return;
    }

    let noise_level = noise_state.map(|n| n.noise_level).unwrap_or(0.0);
    if noise_level < 10.0 {
        return;
    }

    let sub_transform = match sub_query.iter().next() {
        Some(t) => t,
        None => return,
    };

    // Enforce trail point cap
    let trail_count = existing_trails.iter().count();
    if trail_count >= eco_config.max_trail_points {
        return;
    }

    let pos = sub_transform.translation;
    let intensity = noise_level * 0.5;

    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: Color::rgba(0.0, 0.0, 0.0, 0.0), // invisible
                custom_size: Some(Vec2::new(1.0, 1.0)),
                ..default()
            },
            transform: Transform::from_translation(pos),
            ..default()
        },
        NoiseTrailPoint {
            intensity,
            decay_rate: eco_config.noise_decay_rate,
        },
    ));
}

/// Trail points lose intensity over time and despawn when too weak
pub fn decay_noise_trails(
    time: Res<Time>,
    mut commands: Commands,
    mut trails: Query<(Entity, &mut NoiseTrailPoint)>,
) {
    let dt = time.delta_seconds();
    for (entity, mut trail) in trails.iter_mut() {
        trail.intensity -= trail.decay_rate * dt;
        if trail.intensity < 1.0 {
            commands.entity(entity).despawn_recursive();
        }
    }
}
