use bevy::prelude::*;
use bevy::audio::{AudioSink, AudioSinkPlayback, Volume};
use rand::Rng;
use std::collections::HashMap;

use crate::components::{ModuleType, Ship, ShipPhysics};
use crate::events::*;
use crate::resources::InputState;
use crate::states::GameState;
use crate::celestial::events::WarpJumpStarted;

// ============================================================================
// AUDIO PLUGIN
// Everything is event-driven: gameplay systems write messages (WeaponFired,
// ModuleExploded, HullBreached, ...) and the systems here turn them into
// one-shot AudioPlayer entities or manage the persistent loops (engine,
// ambient drone, alarm).
// ============================================================================

// Per-bus volume scalars. Sounds multiply their own base volume by one of these.
const SFX_VOL: f32 = 0.8;
/// Weapon fire sounds muted at user request (2026-07-14) — the current
/// samples don't fit. Explosions/impacts still play. Flip to re-enable.
const WEAPON_FIRE_SOUNDS: bool = false;
const AMBIENT_VOL: f32 = 0.30;
const UI_VOL: f32 = 0.45;
const ALARM_VOL: f32 = 0.40;
const ENGINE_MAX_VOL: f32 = 0.40;

/// Distance (world units) beyond which off-ship sounds are inaudible.
const AUDIBLE_RANGE: f32 = 9000.0;

pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<AlarmState>()
            .add_systems(Startup, load_audio)
            .add_systems(OnEnter(GameState::Exploring), start_flight_loops)
            .add_systems(OnExit(GameState::Exploring), stop_flight_loops)
            .add_systems(Update, (
                weapon_fired_audio,
                explosion_audio,
                alarm_audio,
                warp_audio,
                engine_loop_volume,
                hull_creak_ticker,
            ).run_if(in_state(GameState::Exploring)))
            .add_systems(Update, (
                ui_click_audio,
                notification_audio,
                build_audio,
                docking_audio,
            ));
    }
}

// ============================================================================
// ASSETS
// ============================================================================

#[derive(Resource)]
pub struct GameAudio {
    lasers: Vec<Handle<AudioSource>>,
    heavy_cannons: Vec<Handle<AudioSource>>,
    shots: Vec<Handle<AudioSource>>,
    rocket: Handle<AudioSource>,
    explosions: Vec<Handle<AudioSource>>,
    explosion_deep: Handle<AudioSource>,
    explosion_large: Handle<AudioSource>,
    engine_loop: Handle<AudioSource>,
    warps: Vec<Handle<AudioSource>>,
    space_drone: Handle<AudioSource>,
    hull_creaks: Vec<Handle<AudioSource>>,
    alarm_loop: Handle<AudioSource>,
    ui_select: Vec<Handle<AudioSource>>,
    ui_terminal: Vec<Handle<AudioSource>>,
    ui_beeps: Vec<Handle<AudioSource>>,
}

fn load_audio(mut commands: Commands, assets: Res<AssetServer>) {
    let load_all = |paths: &[&str]| -> Vec<Handle<AudioSource>> {
        paths.iter().map(|p| assets.load(p.to_string())).collect()
    };

    commands.insert_resource(GameAudio {
        lasers: load_all(&[
            "audio/weapons/laser_1.mp3",
            "audio/weapons/laser_2.mp3",
            "audio/weapons/laser_3.mp3",
            "audio/weapons/laser_4.mp3",
            "audio/weapons/laser_5.mp3",
        ]),
        heavy_cannons: load_all(&[
            "audio/weapons/heavy_cannon_1.mp3",
            "audio/weapons/heavy_cannon_2.mp3",
        ]),
        shots: load_all(&[
            "audio/weapons/shoot_1.ogg",
            "audio/weapons/shoot_2.ogg",
        ]),
        rocket: assets.load("audio/weapons/rocket_1.ogg"),
        explosions: load_all(&[
            "audio/impacts/explosion_1.ogg",
            "audio/impacts/explosion_2.ogg",
        ]),
        explosion_deep: assets.load("audio/impacts/explosion_deep.mp3"),
        explosion_large: assets.load("audio/impacts/explosion_large.mp3"),
        engine_loop: assets.load("audio/engines/engine_medium_loop.mp3"),
        warps: load_all(&[
            "audio/engines/warp_1.mp3",
            "audio/engines/warp_2.mp3",
            "audio/engines/warp_3.mp3",
        ]),
        space_drone: assets.load("audio/ambient/space_drone.mp3"),
        hull_creaks: load_all(&[
            "audio/ambient/hull_creak_1.mp3",
            "audio/ambient/hull_creak_2.mp3",
            "audio/ambient/hull_groan_long_1.mp3",
            "audio/ambient/hull_groan_long_2.mp3",
        ]),
        alarm_loop: assets.load("audio/alarms/alarm_loop_1.mp3"),
        ui_select: load_all(&[
            "audio/ui/select_1.mp3",
            "audio/ui/select_2.mp3",
        ]),
        ui_terminal: load_all(&[
            "audio/ui/terminal_1.ogg",
            "audio/ui/terminal_2.ogg",
            "audio/ui/terminal_3.ogg",
        ]),
        ui_beeps: load_all(&[
            "audio/ui/beep_1.ogg",
            "audio/ui/beep_2.ogg",
        ]),
    });
}

// ============================================================================
// HELPERS
// ============================================================================

fn play_oneshot(commands: &mut Commands, handle: Handle<AudioSource>, volume: f32) {
    commands.spawn((
        AudioPlayer(handle),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume)),
    ));
}

fn pick<'a>(rng: &mut impl Rng, v: &'a [Handle<AudioSource>]) -> &'a Handle<AudioSource> {
    &v[rng.gen_range(0..v.len())]
}

/// Quadratic falloff with distance from the player ship. Player-originated
/// sounds pass distance 0.0 and get full volume.
fn attenuate(volume: f32, distance: f32) -> f32 {
    let t = (1.0 - (distance / AUDIBLE_RANGE).clamp(0.0, 1.0)).powi(2);
    volume * t
}

fn player_pos(ship_query: &Query<&Transform, With<Ship>>) -> Vec2 {
    ship_query.single().map(|t| t.translation.truncate()).unwrap_or(Vec2::ZERO)
}

// ============================================================================
// WEAPONS
// ============================================================================

/// Minimum seconds between two sounds of the same weapon type. Keeps the
/// Gatling from stacking 10 overlapping samples and the laser (which writes
/// WeaponFired every frame while beaming) down to a periodic re-trigger.
fn min_interval(weapon: ModuleType) -> f64 {
    match weapon {
        ModuleType::Gatling => 0.12,
        ModuleType::Laser => 1.4,
        ModuleType::PlasmaCaster | ModuleType::IonDisruptor => 0.5,
        _ => 0.05,
    }
}

fn weapon_fired_audio(
    mut events: MessageReader<WeaponFired>,
    audio: Option<Res<GameAudio>>,
    ship_query: Query<&Transform, With<Ship>>,
    time: Res<Time>,
    mut last_played: Local<HashMap<ModuleType, f64>>,
    mut commands: Commands,
) {
    if !WEAPON_FIRE_SOUNDS {
        events.clear();
        return;
    }
    let Some(audio) = audio else { return };
    let mut rng = rand::thread_rng();
    let now = time.elapsed_secs_f64();
    let ppos = player_pos(&ship_query);

    for ev in events.read() {
        let last = last_played.get(&ev.weapon_type).copied().unwrap_or(f64::MIN);
        if now - last < min_interval(ev.weapon_type) {
            continue;
        }

        let (handle, base_vol) = match ev.weapon_type {
            ModuleType::Cannon | ModuleType::Railgun =>
                (pick(&mut rng, &audio.heavy_cannons).clone(), 0.75),
            ModuleType::Coilgun | ModuleType::Gatling =>
                (pick(&mut rng, &audio.shots).clone(), 0.5),
            ModuleType::Laser | ModuleType::PlasmaCaster | ModuleType::IonDisruptor | ModuleType::EMPPulse =>
                (pick(&mut rng, &audio.lasers).clone(), 0.55),
            ModuleType::HeavyMissile | ModuleType::GuidedMissile | ModuleType::ClusterRocket =>
                (audio.rocket.clone(), 0.65),
            _ => continue,
        };

        let dist = if ev.from_player { 0.0 } else { ev.position.distance(ppos) };
        let vol = attenuate(base_vol * SFX_VOL, dist);
        if vol < 0.01 { continue; }

        last_played.insert(ev.weapon_type, now);
        play_oneshot(&mut commands, handle, vol);
    }
}

// ============================================================================
// EXPLOSIONS / DESTRUCTION
// ============================================================================

fn explosion_audio(
    mut module_exploded: MessageReader<ModuleExploded>,
    mut ai_destroyed: MessageReader<AiShipDestroyed>,
    mut hull_destroyed: MessageReader<HullSegmentDestroyed>,
    audio: Option<Res<GameAudio>>,
    ship_query: Query<&Transform, With<Ship>>,
    time: Res<Time>,
    mut last_hull_crunch: Local<f64>,
    mut commands: Commands,
) {
    let Some(audio) = audio else { return };
    let mut rng = rand::thread_rng();
    let ppos = player_pos(&ship_query);

    // Module detonations happen on the player's own ship — full volume.
    for _ in module_exploded.read() {
        play_oneshot(&mut commands, pick(&mut rng, &audio.explosions).clone(), 0.8 * SFX_VOL);
        play_oneshot(&mut commands, audio.explosion_deep.clone(), 0.7 * SFX_VOL);
    }

    // A ship dying is the big payoff — layered boom + low rumble, attenuated.
    for ev in ai_destroyed.read() {
        let dist = ev.position.distance(ppos);
        play_oneshot(&mut commands, audio.explosion_large.clone(), attenuate(0.9 * SFX_VOL, dist));
        play_oneshot(&mut commands, audio.explosion_deep.clone(), attenuate(0.8 * SFX_VOL, dist));
    }

    // Individual hull blocks popping — quiet crunch, rate-limited so a
    // volley chewing through armor doesn't stack 15 copies in one frame.
    let now = time.elapsed_secs_f64();
    for _ in hull_destroyed.read() {
        if now - *last_hull_crunch < 0.3 { continue; }
        *last_hull_crunch = now;
        play_oneshot(&mut commands, pick(&mut rng, &audio.explosions).clone(), 0.35 * SFX_VOL);
    }
}

// ============================================================================
// ALARM (hull breach)
// ============================================================================

#[derive(Resource, Default)]
struct AlarmState {
    /// Seconds of alarm remaining; each new breach tops it back up.
    remaining: f32,
}

#[derive(Component)]
struct AlarmLoopAudio;

fn alarm_audio(
    mut breaches: MessageReader<HullBreached>,
    mut state: ResMut<AlarmState>,
    time: Res<Time>,
    audio: Option<Res<GameAudio>>,
    alarm_query: Query<Entity, With<AlarmLoopAudio>>,
    mut commands: Commands,
) {
    let Some(audio) = audio else { return };

    for _ in breaches.read() {
        state.remaining = 8.0;
    }

    if state.remaining > 0.0 {
        state.remaining -= time.delta_secs();
        if alarm_query.is_empty() {
            commands.spawn((
                AudioPlayer(audio.alarm_loop.clone()),
                PlaybackSettings::LOOP.with_volume(Volume::Linear(ALARM_VOL)),
                AlarmLoopAudio,
            ));
        }
    } else if let Ok(entity) = alarm_query.single() {
        commands.entity(entity).despawn();
    }
}

// ============================================================================
// WARP
// ============================================================================

fn warp_audio(
    mut jumps: MessageReader<WarpJumpStarted>,
    audio: Option<Res<GameAudio>>,
    mut commands: Commands,
) {
    let Some(audio) = audio else { return };
    let mut rng = rand::thread_rng();
    for _ in jumps.read() {
        play_oneshot(&mut commands, pick(&mut rng, &audio.warps).clone(), 0.7 * SFX_VOL);
    }
}

// ============================================================================
// PERSISTENT LOOPS (engine + ambient drone)
// ============================================================================

#[derive(Component)]
struct EngineLoopAudio;

#[derive(Component)]
struct AmbientLoopAudio;

fn start_flight_loops(audio: Option<Res<GameAudio>>, mut commands: Commands) {
    let Some(audio) = audio else { return };
    // Engine starts silent; engine_loop_volume drives it from throttle.
    commands.spawn((
        AudioPlayer(audio.engine_loop.clone()),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)),
        EngineLoopAudio,
    ));
    commands.spawn((
        AudioPlayer(audio.space_drone.clone()),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(AMBIENT_VOL)),
        AmbientLoopAudio,
    ));
}

fn stop_flight_loops(
    loops: Query<Entity, Or<(With<EngineLoopAudio>, With<AmbientLoopAudio>, With<AlarmLoopAudio>)>>,
    mut state: ResMut<AlarmState>,
    mut commands: Commands,
) {
    state.remaining = 0.0;
    for entity in loops.iter() {
        commands.entity(entity).despawn();
    }
}

fn engine_loop_volume(
    input: Res<InputState>,
    ship_query: Query<&ShipPhysics, With<Ship>>,
    mut sink_query: Query<&mut AudioSink, With<EngineLoopAudio>>,
    time: Res<Time>,
    mut current: Local<f32>,
) {
    let Ok(mut sink) = sink_query.single_mut() else { return };
    let throttle = ship_query.single().map(|p| p.throttle.abs()).unwrap_or(0.0);
    let intensity = throttle
        .max(input.movement.x.abs() * 0.5)
        .max(input.thruster_input.abs() * 0.5)
        .max(if input.brake { 0.4 } else { 0.0 });

    // Ease toward the target so thrust taps don't click the loop on/off.
    let target = intensity.clamp(0.0, 1.0) * ENGINE_MAX_VOL;
    let rate = if target > *current { 6.0 } else { 2.5 };
    *current += (target - *current) * (rate * time.delta_secs()).min(1.0);
    sink.set_volume(Volume::Linear(*current));
}

// ============================================================================
// HULL CREAKS — sparse, quiet, unsettling
// ============================================================================

fn hull_creak_ticker(
    time: Res<Time>,
    audio: Option<Res<GameAudio>>,
    mut timer: Local<Option<Timer>>,
    mut commands: Commands,
) {
    let Some(audio) = audio else { return };
    let mut rng = rand::thread_rng();

    let t = timer.get_or_insert_with(|| {
        Timer::from_seconds(rng.gen_range(25.0..70.0), TimerMode::Once)
    });
    t.tick(time.delta());
    if t.is_finished() {
        play_oneshot(&mut commands, pick(&mut rng, &audio.hull_creaks).clone(), 0.22);
        *timer = Some(Timer::from_seconds(rng.gen_range(25.0..70.0), TimerMode::Once));
    }
}

// ============================================================================
// UI
// ============================================================================

fn ui_click_audio(
    interactions: Query<&Interaction, (Changed<Interaction>, With<Button>)>,
    audio: Option<Res<GameAudio>>,
    mut commands: Commands,
) {
    let Some(audio) = audio else { return };
    let mut rng = rand::thread_rng();
    for interaction in interactions.iter() {
        if *interaction == Interaction::Pressed {
            play_oneshot(&mut commands, pick(&mut rng, &audio.ui_select).clone(), UI_VOL);
        }
    }
}

fn notification_audio(
    mut notifications: MessageReader<ShowNotification>,
    audio: Option<Res<GameAudio>>,
    time: Res<Time>,
    mut last: Local<f64>,
    mut commands: Commands,
) {
    let Some(audio) = audio else { return };
    let now = time.elapsed_secs_f64();
    for ev in notifications.read() {
        if now - *last < 0.25 { continue; }
        *last = now;
        let handle = match ev.notification_type {
            NotificationType::Info => audio.ui_terminal[0].clone(),
            NotificationType::Success => audio.ui_terminal[1].clone(),
            NotificationType::Warning => audio.ui_beeps[0].clone(),
            NotificationType::Danger => audio.ui_beeps[1].clone(),
        };
        play_oneshot(&mut commands, handle, UI_VOL);
    }
}

fn build_audio(
    mut placed: MessageReader<ModulePlaced>,
    mut removed: MessageReader<ModuleRemoved>,
    audio: Option<Res<GameAudio>>,
    mut commands: Commands,
) {
    let Some(audio) = audio else { return };
    for _ in placed.read() {
        play_oneshot(&mut commands, audio.ui_terminal[2].clone(), UI_VOL);
    }
    for _ in removed.read() {
        play_oneshot(&mut commands, audio.ui_beeps[0].clone(), UI_VOL * 0.8);
    }
}

fn docking_audio(
    mut docked: MessageReader<DockingCompleted>,
    audio: Option<Res<GameAudio>>,
    mut commands: Commands,
) {
    let Some(audio) = audio else { return };
    for _ in docked.read() {
        play_oneshot(&mut commands, audio.ui_terminal[1].clone(), UI_VOL);
    }
}
