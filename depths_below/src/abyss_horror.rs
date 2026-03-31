use bevy::prelude::*;
use rand::Rng;

use crate::camera::{camera_shake_update, update_background_color, CameraState};
use crate::components::*;
use crate::creatures::ecosystem::ecosystem_ai_decisions;
use crate::events::{NotificationType, ShowNotification};
use crate::resources::*;
use crate::sonar::{update_sonar_radar, SonarBlip, SonarRadarDisplay};
use crate::states::GameState;
use crate::ui::{update_hud, update_hud_secondary, DepthText, HullText, NoiseText, OxygenText, PowerText};

// ============================================================================
// RESOURCE
// ============================================================================

#[derive(Resource)]
pub struct AbyssalHorror {
    pub intensity: f32,
    pub phase: u8,
    // Effect timers
    pub instrument_glitch_timer: Timer,
    pub phantom_blip_timer: Timer,
    pub watching_check_timer: Timer,
    pub sync_flee_timer: Timer,
    pub time_glitch_timer: Timer,
    pub notification_timer: Timer,
    pub camera_pulse_timer: Timer,
    // Physics distortion
    pub drift_direction: Vec2,
    pub drift_strength: f32,
    pub buoyancy_distortion: f32,
    // Glitch state
    pub glitch_active: bool,
    pub glitch_remaining: f32,
    // Time glitch state
    pub time_frozen: bool,
}

impl Default for AbyssalHorror {
    fn default() -> Self {
        Self {
            intensity: 0.0,
            phase: 0,
            instrument_glitch_timer: Timer::from_seconds(8.0, TimerMode::Repeating),
            phantom_blip_timer: Timer::from_seconds(12.0, TimerMode::Repeating),
            watching_check_timer: Timer::from_seconds(5.0, TimerMode::Repeating),
            sync_flee_timer: Timer::from_seconds(45.0, TimerMode::Repeating),
            time_glitch_timer: Timer::from_seconds(60.0, TimerMode::Repeating),
            notification_timer: Timer::from_seconds(20.0, TimerMode::Repeating),
            camera_pulse_timer: Timer::from_seconds(3.0, TimerMode::Repeating),
            drift_direction: Vec2::ZERO,
            drift_strength: 0.0,
            buoyancy_distortion: 1.0,
            glitch_active: false,
            glitch_remaining: 0.0,
            time_frozen: false,
        }
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

pub struct AbyssHorrorPlugin;

impl Plugin for AbyssHorrorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AbyssalHorror>()
            .add_systems(
                Update,
                (
                    update_horror_intensity,
                    horror_creature_watching,
                    enforce_abyssal_watching.after(ecosystem_ai_decisions),
                    horror_synchronized_flee,
                    horror_time_glitch,
                    restore_time_glitch,
                    restore_creature_behavior,
                    spawn_phantom_sonar_blips.after(update_sonar_radar),
                    update_phantom_blips,
                    horror_instrument_glitch
                        .after(update_hud)
                        .after(update_hud_secondary),
                    apply_physics_distortion,
                    apply_horror_camera_effects.after(camera_shake_update),
                    apply_horror_background_tint.after(update_background_color),
                    horror_notifications,
                )
                    .run_if(in_state(GameState::Exploring)),
            );
    }
}

// ============================================================================
// SYSTEM 1: UPDATE HORROR INTENSITY
// ============================================================================

fn update_horror_intensity(
    time: Res<Time>,
    depth_state: Res<DepthState>,
    mut horror: ResMut<AbyssalHorror>,
) {
    let depth = depth_state.current_depth;

    // Calculate phase and intensity from depth
    let (phase, intensity) = if depth < 500.0 {
        (0, 0.0)
    } else if depth < 1000.0 {
        (1, (depth - 500.0) / 500.0 * 0.25)
    } else if depth < 1500.0 {
        (2, 0.25 + (depth - 1000.0) / 500.0 * 0.25)
    } else if depth < 2000.0 {
        (3, 0.5 + (depth - 1500.0) / 500.0 * 0.25)
    } else {
        (4, (0.75 + (depth - 2000.0) / 1000.0 * 0.25).min(1.0))
    };

    horror.intensity = intensity;
    horror.phase = phase;

    // Adjust timer intervals based on intensity (more intense = more frequent)
    if phase >= 1 {
        let glitch_interval = 8.0 - intensity * 6.0; // 8s → 2s
        horror.instrument_glitch_timer.set_duration(std::time::Duration::from_secs_f32(glitch_interval.max(1.5)));

        let blip_interval = 12.0 - intensity * 9.0; // 12s → 3s
        horror.phantom_blip_timer.set_duration(std::time::Duration::from_secs_f32(blip_interval.max(2.0)));

        let notif_interval = 25.0 - intensity * 15.0; // 25s → 10s
        horror.notification_timer.set_duration(std::time::Duration::from_secs_f32(notif_interval.max(8.0)));
    }

    // Update drift direction slowly (rotating sine)
    let t = time.elapsed_seconds();
    horror.drift_direction = Vec2::new(
        (t * 0.1).sin(),
        (t * 0.07).cos(),
    ).normalize_or_zero();

    // Update drift strength based on phase
    horror.drift_strength = match phase {
        0 | 1 => 0.0,
        2 => 3.0 + intensity * 8.0,
        3 => 8.0 + intensity * 8.0,
        _ => 12.0 + intensity * 6.0,
    };

    // Buoyancy distortion at phase 4
    horror.buoyancy_distortion = if phase >= 4 {
        let osc = (t * 0.3).sin();
        // Oscillates between 0.7 and -0.3
        0.2 + osc * 0.5
    } else if phase >= 3 {
        0.85 + (t * 0.2).sin() * 0.15 // subtle wobble
    } else {
        1.0
    };

    // Tick glitch remaining time
    if horror.glitch_active {
        horror.glitch_remaining -= time.delta_seconds();
        if horror.glitch_remaining <= 0.0 {
            horror.glitch_active = false;
        }
    }
}

// ============================================================================
// SYSTEM 2: HORROR CREATURE WATCHING
// ============================================================================

fn horror_creature_watching(
    time: Res<Time>,
    mut horror: ResMut<AbyssalHorror>,
    sub_query: Query<Entity, With<Submarine>>,
    mut creatures: Query<(Entity, &Creature, &mut CreatureAI, Option<&AbyssalInfluence>)>,
    mut commands: Commands,
) {
    if horror.phase < 2 { return; }

    horror.watching_check_timer.tick(time.delta());
    if !horror.watching_check_timer.just_finished() { return; }

    let Ok(sub_entity) = sub_query.get_single() else { return; };
    let mut rng = rand::thread_rng();

    for (entity, creature, mut ai, influence) in creatures.iter_mut() {
        // Skip if already watching
        if influence.is_some() { continue; }

        // Leviathans only at phase 3+
        if creature.creature_type == CreatureType::Leviathan && horror.phase < 3 {
            continue;
        }

        // Probability scales with intensity
        let prob = horror.intensity * 0.8;
        if rng.gen::<f32>() > prob { continue; }

        // Save original state and switch to watching
        let original_state = ai.state;
        ai.state = CreatureAIState::Observing;
        ai.target = Some(EcoTarget::Submarine(sub_entity));

        commands.entity(entity).insert(AbyssalInfluence {
            watching: true,
            original_state,
        });
    }
}

// ============================================================================
// SYSTEM 3: ENFORCE ABYSSAL WATCHING
// ============================================================================

fn enforce_abyssal_watching(
    horror: Res<AbyssalHorror>,
    sub_query: Query<Entity, With<Submarine>>,
    mut creatures: Query<(&mut CreatureAI, &AbyssalInfluence)>,
) {
    if horror.phase < 2 { return; }

    let Ok(sub_entity) = sub_query.get_single() else { return; };

    for (mut ai, influence) in creatures.iter_mut() {
        if influence.watching {
            ai.state = CreatureAIState::Observing;
            ai.target = Some(EcoTarget::Submarine(sub_entity));
        }
    }
}

// ============================================================================
// SYSTEM 4: SYNCHRONIZED FLEE
// ============================================================================

fn horror_synchronized_flee(
    time: Res<Time>,
    mut horror: ResMut<AbyssalHorror>,
    mut creatures: Query<(Entity, &Transform, &mut CreatureAI, Option<&AbyssalInfluence>)>,
    mut commands: Commands,
    mut camera_state: ResMut<CameraState>,
    mut notifications: EventWriter<ShowNotification>,
) {
    if horror.phase < 3 { return; }

    horror.sync_flee_timer.tick(time.delta());
    if !horror.sync_flee_timer.just_finished() { return; }

    let mut rng = rand::thread_rng();

    // Flee direction: mostly upward and away from "something below"
    let flee_dir = Vec2::new(
        rng.gen_range(-0.3..0.3),
        rng.gen_range(0.5..1.0),
    ).normalize_or_zero();

    let duration = rng.gen_range(5.0..8.0);

    let mut affected = 0;
    for (entity, transform, mut ai, influence) in creatures.iter_mut() {
        // Remove watching state temporarily
        if influence.is_some() {
            commands.entity(entity).remove::<AbyssalInfluence>();
        }

        let pos = transform.translation.truncate();
        let flee_target = pos + flee_dir * 600.0;

        ai.state = CreatureAIState::Fleeing;
        ai.target = Some(EcoTarget::Position(flee_target));

        commands.entity(entity).insert(SynchronizedFlee {
            flee_direction: flee_dir,
            duration: Timer::from_seconds(duration, TimerMode::Once),
        });

        affected += 1;
    }

    if affected > 0 {
        camera_state.shake_intensity = (camera_state.shake_intensity + 3.0).min(20.0);

        let messages = [
            "Something moved in the deep...",
            "The creatures flee as one...",
            "They sense something below...",
            "A presence stirs beneath you...",
        ];
        notifications.send(ShowNotification {
            message: messages[rng.gen_range(0..messages.len())].to_string(),
            notification_type: NotificationType::Danger,
            duration: 4.0,
        });
    }
}

// ============================================================================
// SYSTEM 5: TIME GLITCH (Phase 4)
// ============================================================================

fn horror_time_glitch(
    time: Res<Time>,
    mut horror: ResMut<AbyssalHorror>,
    mut creatures: Query<(Entity, &mut Velocity), (With<Creature>, Without<Submarine>)>,
    mut sub_query: Query<(Entity, &mut Velocity), With<Submarine>>,
    mut commands: Commands,
    mut camera_state: ResMut<CameraState>,
    mut notifications: EventWriter<ShowNotification>,
) {
    if horror.phase < 4 { return; }
    if horror.time_frozen { return; }

    horror.time_glitch_timer.tick(time.delta());
    if !horror.time_glitch_timer.just_finished() { return; }

    let mut rng = rand::thread_rng();
    let freeze_duration = rng.gen_range(0.5..2.0);

    // Freeze all creatures
    for (entity, mut velocity) in creatures.iter_mut() {
        let saved = velocity.0;
        velocity.0 = Vec2::ZERO;
        commands.entity(entity).insert(TimeGlitchFrozen {
            duration: Timer::from_seconds(freeze_duration, TimerMode::Once),
            saved_velocity: saved,
        });
    }

    // Freeze submarine
    if let Ok((entity, mut velocity)) = sub_query.get_single_mut() {
        let saved = velocity.0;
        velocity.0 = Vec2::ZERO;
        commands.entity(entity).insert(TimeGlitchFrozen {
            duration: Timer::from_seconds(freeze_duration, TimerMode::Once),
            saved_velocity: saved,
        });
    }

    horror.time_frozen = true;

    // Camera burst
    camera_state.shake_intensity = (camera_state.shake_intensity + 5.0).min(20.0);

    let messages = [
        "Time... skipped.",
        "A moment was lost.",
        "Reality stuttered.",
        "The deep holds time still.",
    ];
    notifications.send(ShowNotification {
        message: messages[rng.gen_range(0..messages.len())].to_string(),
        notification_type: NotificationType::Danger,
        duration: 3.0,
    });
}

// ============================================================================
// SYSTEM 5b: RESTORE FROM TIME GLITCH
// ============================================================================

fn restore_time_glitch(
    time: Res<Time>,
    mut horror: ResMut<AbyssalHorror>,
    mut frozen_query: Query<(Entity, &mut Velocity, &mut TimeGlitchFrozen)>,
    mut commands: Commands,
) {
    if !horror.time_frozen { return; }

    let mut any_frozen = false;
    for (entity, mut velocity, mut frozen) in frozen_query.iter_mut() {
        frozen.duration.tick(time.delta());
        if frozen.duration.finished() {
            velocity.0 = frozen.saved_velocity;
            commands.entity(entity).remove::<TimeGlitchFrozen>();
        } else {
            any_frozen = true;
        }
    }

    if !any_frozen {
        horror.time_frozen = false;
    }
}

// ============================================================================
// SYSTEM 6: RESTORE CREATURE BEHAVIOR (on ascending)
// ============================================================================

fn restore_creature_behavior(
    time: Res<Time>,
    horror: Res<AbyssalHorror>,
    mut creatures: Query<(Entity, &mut CreatureAI, &AbyssalInfluence)>,
    mut sync_creatures: Query<(Entity, &mut SynchronizedFlee, &mut CreatureAI), Without<AbyssalInfluence>>,
    mut commands: Commands,
) {
    // Restore watched creatures when ascending above phase 2
    if horror.phase < 2 {
        for (entity, mut ai, influence) in creatures.iter_mut() {
            ai.state = influence.original_state;
            commands.entity(entity).remove::<AbyssalInfluence>();
        }
    }

    // Tick synchronized flee durations and restore
    for (entity, mut sync, mut ai) in sync_creatures.iter_mut() {
        sync.duration.tick(time.delta());
        if sync.duration.finished() {
            ai.state = CreatureAIState::Wandering;
            ai.target = None;
            commands.entity(entity).remove::<SynchronizedFlee>();
        }
    }
}

// ============================================================================
// SYSTEM 7: SPAWN PHANTOM SONAR BLIPS
// ============================================================================

fn spawn_phantom_sonar_blips(
    time: Res<Time>,
    mut horror: ResMut<AbyssalHorror>,
    radar_query: Query<Entity, With<SonarRadarDisplay>>,
    mut commands: Commands,
) {
    if horror.phase < 1 { return; }

    horror.phantom_blip_timer.tick(time.delta());
    if !horror.phantom_blip_timer.just_finished() { return; }

    let Ok(radar_entity) = radar_query.get_single() else { return; };

    let mut rng = rand::thread_rng();

    // Number of phantom blips scales with phase
    let blip_count = match horror.phase {
        1 => 1,
        2 => rng.gen_range(1..=3),
        3 => rng.gen_range(2..=5),
        _ => rng.gen_range(3..=7),
    };

    let radar_size = 80.0; // half the 160px radar display

    for i in 0..blip_count {
        // Position on radar
        let (x, y) = if horror.phase >= 3 && i > 0 {
            // Geometric patterns at phase 3+: ring or triangle
            let angle = std::f32::consts::TAU * (i as f32) / (blip_count as f32);
            let radius = rng.gen_range(30.0..70.0);
            (angle.cos() * radius, angle.sin() * radius)
        } else {
            // Random positions, biased toward edges at phase 1
            let radius = if horror.phase == 1 {
                rng.gen_range(55.0..radar_size)
            } else {
                rng.gen_range(20.0..radar_size)
            };
            let angle = rng.gen_range(0.0..std::f32::consts::TAU);
            (angle.cos() * radius, angle.sin() * radius)
        };

        // Color based on phase — phase 2+ can show fake red (leviathan-sized) blips
        let (color, size) = if horror.phase >= 2 && rng.gen::<f32>() < 0.3 {
            (Color::rgba(1.0, 0.2, 0.2, 0.8), 7.0) // fake leviathan blip
        } else {
            (Color::rgba(1.0, 1.0, 0.3, 0.7), 4.0) // normal-looking blip
        };

        let lifetime = rng.gen_range(3.0..6.0);
        let drift = Vec2::new(
            rng.gen_range(-5.0..5.0),
            rng.gen_range(-5.0..5.0),
        );

        let blip_entity = commands
            .spawn((
                NodeBundle {
                    style: Style {
                        position_type: PositionType::Absolute,
                        left: Val::Px(radar_size + x - size / 2.0),
                        top: Val::Px(radar_size - y - size / 2.0),
                        width: Val::Px(size),
                        height: Val::Px(size),
                        ..default()
                    },
                    background_color: color.into(),
                    ..default()
                },
                PhantomBlip {
                    lifetime: Timer::from_seconds(lifetime, TimerMode::Once),
                    drift,
                },
                SonarBlip {
                    lifetime: Timer::from_seconds(lifetime, TimerMode::Once),
                },
            ))
            .id();

        commands.entity(radar_entity).add_child(blip_entity);
    }
}

// ============================================================================
// SYSTEM 8: UPDATE PHANTOM BLIPS
// ============================================================================

fn update_phantom_blips(
    time: Res<Time>,
    horror: Res<AbyssalHorror>,
    mut blips: Query<(Entity, &mut PhantomBlip, &mut Style)>,
    mut commands: Commands,
) {
    for (entity, mut phantom, mut style) in blips.iter_mut() {
        phantom.lifetime.tick(time.delta());

        if phantom.lifetime.finished() {
            commands.entity(entity).despawn_recursive();
            continue;
        }

        // Apply drift
        let dt = time.delta_seconds();
        if let Val::Px(ref mut left) = style.left {
            *left += phantom.drift.x * dt;
        }
        if let Val::Px(ref mut top) = style.top {
            *top -= phantom.drift.y * dt;
        }

        // Phase 3+ pulse effect (scale oscillation via width/height)
        if horror.phase >= 3 {
            let t = time.elapsed_seconds();
            let pulse = 1.0 + (t * 4.0).sin() * 0.3;
            let base_size = 4.0;
            style.width = Val::Px(base_size * pulse);
            style.height = Val::Px(base_size * pulse);
        }
    }
}

// ============================================================================
// SYSTEM 9: INSTRUMENT GLITCH
// ============================================================================

fn horror_instrument_glitch(
    time: Res<Time>,
    mut horror: ResMut<AbyssalHorror>,
    mut depth_query: Query<&mut Text, (With<DepthText>, Without<PowerText>, Without<OxygenText>, Without<HullText>, Without<NoiseText>)>,
    mut hull_query: Query<&mut Text, (With<HullText>, Without<DepthText>, Without<PowerText>, Without<OxygenText>, Without<NoiseText>)>,
    mut oxygen_query: Query<&mut Text, (With<OxygenText>, Without<DepthText>, Without<PowerText>, Without<HullText>, Without<NoiseText>)>,
    mut power_query: Query<&mut Text, (With<PowerText>, Without<DepthText>, Without<OxygenText>, Without<HullText>, Without<NoiseText>)>,
    mut noise_query: Query<&mut Text, (With<NoiseText>, Without<DepthText>, Without<PowerText>, Without<OxygenText>, Without<HullText>)>,
) {
    if horror.phase < 1 { return; }

    // If a glitch is currently active, keep corrupting
    if horror.glitch_active {
        let mut rng = rand::thread_rng();

        match horror.phase {
            1 => {
                // Subtle: depth flickers
                if let Ok(mut text) = depth_query.get_single_mut() {
                    let fake_depth = rng.gen_range(100.0..3000.0);
                    text.sections[0].value = format!("Depth: {:.0}m", fake_depth);
                }
            }
            2 => {
                // Moderate: oxygen false alarm, depth jumps
                if let Ok(mut text) = oxygen_query.get_single_mut() {
                    if rng.gen::<f32>() < 0.5 {
                        text.sections[0].value = "O2: 0%".to_string();
                        text.sections[0].style.color = Color::RED;
                    }
                }
                if let Ok(mut text) = depth_query.get_single_mut() {
                    let fake = rng.gen_range(0.0..5000.0);
                    text.sections[0].value = format!("Depth: {:.0}m", fake);
                }
                if let Ok(mut text) = noise_query.get_single_mut() {
                    text.sections[0].value = "Noise: MAX".to_string();
                    text.sections[0].style.color = Color::RED;
                }
            }
            _ => {
                // Severe: all instruments scramble
                let glitch_strings = ["???", "ERR", "---", "NaN", "∞", "0.0̸̡"];
                if let Ok(mut text) = depth_query.get_single_mut() {
                    text.sections[0].value = format!("Depth: {}", glitch_strings[rng.gen_range(0..glitch_strings.len())]);
                    text.sections[0].style.color = Color::rgba(1.0, 0.3, 0.3, 0.8);
                }
                if let Ok(mut text) = hull_query.get_single_mut() {
                    text.sections[0].value = format!("Hull: {}", glitch_strings[rng.gen_range(0..glitch_strings.len())]);
                    text.sections[0].style.color = Color::rgba(1.0, 0.3, 0.3, 0.8);
                }
                if let Ok(mut text) = oxygen_query.get_single_mut() {
                    text.sections[0].value = format!("O2: {}", glitch_strings[rng.gen_range(0..glitch_strings.len())]);
                    text.sections[0].style.color = Color::rgba(1.0, 0.3, 0.3, 0.8);
                }
                if let Ok(mut text) = power_query.get_single_mut() {
                    text.sections[0].value = format!("Power: {}", glitch_strings[rng.gen_range(0..glitch_strings.len())]);
                    text.sections[0].style.color = Color::rgba(1.0, 0.3, 0.3, 0.8);
                }
                if let Ok(mut text) = noise_query.get_single_mut() {
                    text.sections[0].value = format!("Noise: {}", glitch_strings[rng.gen_range(0..glitch_strings.len())]);
                    text.sections[0].style.color = Color::rgba(1.0, 0.3, 0.3, 0.8);
                }
            }
        }
        return;
    }

    // Check if it's time to trigger a new glitch
    horror.instrument_glitch_timer.tick(time.delta());
    if !horror.instrument_glitch_timer.just_finished() { return; }

    let mut rng = rand::thread_rng();

    // Glitch duration scales with phase
    let duration = match horror.phase {
        1 => rng.gen_range(0.1..0.3),    // single-frame flicker
        2 => rng.gen_range(0.3..0.8),    // noticeable
        3 => rng.gen_range(0.8..1.5),    // alarming
        _ => rng.gen_range(1.0..2.5),    // prolonged
    };

    horror.glitch_active = true;
    horror.glitch_remaining = duration;
}

// ============================================================================
// SYSTEM 10: PHYSICS DISTORTION
// ============================================================================

fn apply_physics_distortion(
    time: Res<Time>,
    horror: Res<AbyssalHorror>,
    mut sub_query: Query<&mut Velocity, With<Submarine>>,
) {
    if horror.phase < 2 { return; }

    let Ok(mut velocity) = sub_query.get_single_mut() else { return; };
    let dt = time.delta_seconds();

    // Apply phantom drift
    velocity.0 += horror.drift_direction * horror.drift_strength * dt;
}

// ============================================================================
// SYSTEM 11: HORROR CAMERA EFFECTS
// ============================================================================

fn apply_horror_camera_effects(
    time: Res<Time>,
    horror: Res<AbyssalHorror>,
    mut camera_state: ResMut<CameraState>,
) {
    if horror.phase < 1 { return; }

    let t = time.elapsed_seconds();

    match horror.phase {
        1 => {
            // Occasional micro-shake
            if (t * 0.5).sin() > 0.95 {
                camera_state.shake_intensity = camera_state.shake_intensity.max(0.5);
            }
        }
        2 => {
            // Slow zoom drift
            let zoom_offset = (t * 0.3).sin() * 0.05;
            camera_state.zoom = (camera_state.zoom + zoom_offset * time.delta_seconds()).clamp(
                camera_state.min_zoom,
                camera_state.max_zoom,
            );
            // Occasional shake
            if (t * 0.3).sin() > 0.9 {
                camera_state.shake_intensity = camera_state.shake_intensity.max(1.0);
            }
        }
        3 => {
            // Heartbeat pulse
            horror.camera_pulse_timer.duration(); // just reference to keep consistent
            let pulse = ((t * 1.5).sin() * 0.5 + 0.5).powf(4.0); // sharp pulses
            camera_state.zoom = (camera_state.zoom - pulse * 0.03 * time.delta_seconds()).clamp(
                camera_state.min_zoom,
                camera_state.max_zoom,
            );
            // More frequent shake
            if (t * 0.4).sin() > 0.8 {
                camera_state.shake_intensity = camera_state.shake_intensity.max(1.5);
            }
        }
        _ => {
            // Phase 4: sustained instability
            camera_state.shake_intensity = camera_state.shake_intensity.max(1.0 + horror.intensity);

            let zoom_chaos = (t * 0.7).sin() * 0.03 + (t * 1.3).cos() * 0.02;
            camera_state.zoom = (camera_state.zoom + zoom_chaos * time.delta_seconds()).clamp(
                camera_state.min_zoom,
                camera_state.max_zoom,
            );
        }
    }
}

// ============================================================================
// SYSTEM 12: HORROR BACKGROUND TINT
// ============================================================================

fn apply_horror_background_tint(
    time: Res<Time>,
    horror: Res<AbyssalHorror>,
    mut clear_color: ResMut<ClearColor>,
) {
    if horror.phase < 2 { return; }

    let t = time.elapsed_seconds();
    let color = &mut clear_color.0;

    match horror.phase {
        2 => {
            // Slight sickly green/purple tint
            let r = (color.r() - 0.005).max(0.0);
            let g = color.g();
            let b = (color.b() + 0.008).min(0.15);
            *color = Color::rgb(r, g, b);
        }
        3 => {
            // Pulsing dark red undertone
            let pulse = ((t * 0.5).sin() * 0.5 + 0.5) * 0.02;
            let r = (color.r() + pulse).min(0.08);
            let g = (color.g() - 0.002).max(0.0);
            let b = color.b();
            *color = Color::rgb(r, g, b);
        }
        _ => {
            // Phase 4: occasional crimson flash
            let flash = ((t * 0.2).sin() * 0.5 + 0.5).powf(8.0);
            let r = (color.r() + flash * 0.05).min(0.1);
            let g = (color.g() * 0.98).max(0.0);
            let b = (color.b() * 0.98).max(0.0);
            *color = Color::rgb(r, g, b);
        }
    }
}

// ============================================================================
// SYSTEM 13: HORROR NOTIFICATIONS
// ============================================================================

fn horror_notifications(
    time: Res<Time>,
    mut horror: ResMut<AbyssalHorror>,
    mut notifications: EventWriter<ShowNotification>,
) {
    if horror.phase < 1 { return; }

    horror.notification_timer.tick(time.delta());
    if !horror.notification_timer.just_finished() { return; }

    let mut rng = rand::thread_rng();

    let messages: &[&str] = match horror.phase {
        1 => &[
            "The sonar seems... off.",
            "Was that shadow always there?",
            "A faint echo, from nowhere.",
            "The pressure feels wrong.",
        ],
        2 => &[
            "The creatures have stopped moving.",
            "Something is watching.",
            "Your instruments flicker.",
            "The silence is deafening.",
            "They've noticed you.",
        ],
        3 => &[
            "They're all looking at you.",
            "The void is listening.",
            "Even the predators are afraid.",
            "Nothing attacks. Everything watches.",
            "You feel observed from below.",
        ],
        _ => &[
            "Time feels wrong.",
            "There is something below.",
            "You were not meant to be here.",
            "It knows you're here.",
            "The abyss stares back.",
            "Reality bends around you.",
        ],
    };

    let message = messages[rng.gen_range(0..messages.len())];
    let notif_type = if horror.phase >= 3 {
        NotificationType::Danger
    } else {
        NotificationType::Warning
    };

    notifications.send(ShowNotification {
        message: message.to_string(),
        notification_type: notif_type,
        duration: 4.0,
    });
}
