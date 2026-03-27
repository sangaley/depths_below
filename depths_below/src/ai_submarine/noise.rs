use bevy::prelude::*;

use crate::components::*;
use super::components::*;

/// Sums child Engine noise into AiSubState.noise_level.
/// Inserts/removes AiSubSonarContact based on threshold.
pub fn ai_sub_noise_system(
    mut commands: Commands,
    mut ai_subs: Query<(Entity, &mut AiSubState, &Children), With<AiSubmarine>>,
    engine_query: Query<(&Engine, &Module, &OwnedByAiSub)>,
) {
    for (entity, mut state, children) in ai_subs.iter_mut() {
        if state.is_destroyed {
            continue;
        }

        let mut total_noise = 0.0_f32;
        for &child in children.iter() {
            if let Ok((engine, module, _)) = engine_query.get(child) {
                if module.is_active && module.health > 0.0 {
                    let efficiency = module.health / module.max_health;
                    total_noise += engine.noise_level * efficiency;
                }
            }
        }

        state.noise_level = total_noise;

        // Insert/remove sonar contact based on noise threshold
        if total_noise > 20.0 {
            commands.entity(entity).insert(AiSubSonarContact {
                noise_signature: total_noise,
                revealed_timer: Timer::from_seconds(3.0, TimerMode::Once),
            });
        }
    }
}

/// Spawns NoiseTrailPoint entities at AI submarine positions when noisy enough.
/// Creatures with trail detection (BlindHunter, Scavenger) follow these automatically.
pub fn ai_sub_noise_trail_system(
    time: Res<Time>,
    ai_subs: Query<(&Transform, &AiSubState), With<AiSubmarine>>,
    mut commands: Commands,
    mut trail_timer: Local<f32>,
) {
    *trail_timer += time.delta_seconds();
    if *trail_timer < 0.5 {
        return;
    }
    *trail_timer = 0.0;

    for (transform, state) in ai_subs.iter() {
        if state.is_destroyed || state.noise_level < 5.0 {
            continue;
        }

        let pos = transform.translation.truncate();
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgba(0.0, 0.0, 0.0, 0.0), // Invisible
                    custom_size: Some(Vec2::splat(1.0)),
                    ..default()
                },
                transform: Transform::from_xyz(pos.x, pos.y, -0.2),
                ..default()
            },
            NoiseTrailPoint {
                intensity: state.noise_level * 0.5,
                decay_rate: 2.0,
            },
        ));
    }
}

/// Decay sonar contact timers on AI subs
pub fn ai_sub_sonar_contact_decay(
    time: Res<Time>,
    mut commands: Commands,
    mut contacts: Query<(Entity, &mut AiSubSonarContact)>,
) {
    for (entity, mut contact) in contacts.iter_mut() {
        contact.revealed_timer.tick(time.delta());
        if contact.revealed_timer.finished() {
            commands.entity(entity).remove::<AiSubSonarContact>();
        }
    }
}
