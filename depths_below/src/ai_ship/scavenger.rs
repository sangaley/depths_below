use bevy::prelude::*;
use rand::Rng;

use crate::components::*;
use crate::events::*;
use super::components::*;

// ============================================================================
// SCAVENGER COMPETITION — wrecks are contested, not patient.
// A fresh kill rings the dinner bell: some time after a ship dies, a
// Rust Swarm wing spawns at the edge of the area and burns straight for
// the hulk (their brain already prioritizes wrecks — see ai_brain's
// RustSwarm tree). Every swarm ship parked on a wreck EATS its loot,
// unit by unit. The player's choice: strip the wreck fast, drive the
// vultures off (they're brittle kamikaze junk), or cede the carcass.
// ============================================================================

/// Chance a kill attracts a wave at all.
const WAVE_CHANCE: f32 = 0.65;
/// How long after the kill the vultures arrive (randomized per wave).
const WAVE_DELAY_MIN: f32 = 25.0;
const WAVE_DELAY_MAX: f32 = 60.0;
/// How far out the wave spawns from the wreck.
const SPAWN_DIST_MIN: f32 = 1400.0;
const SPAWN_DIST_MAX: f32 = 2000.0;
/// A swarm ship must be this close to a wreck to feed on it.
const FEED_RANGE: f32 = 260.0;
/// Seconds per loot unit eaten, per feeding ship.
const FEED_SECONDS: f32 = 4.0;

struct PendingWave {
    wreck: Entity,
    position: Vec2,
    timer: Timer,
    ships: u32,
}

#[derive(Resource, Default)]
pub struct ScavengerWaves {
    pending: Vec<PendingWave>,
}

/// A kill advertises itself — schedule the vultures.
pub fn schedule_scavenger_waves(
    mut waves: ResMut<ScavengerWaves>,
    mut destroyed_events: MessageReader<AiShipDestroyed>,
) {
    let mut rng = rand::thread_rng();
    for event in destroyed_events.read() {
        // The scavengers don't mourn their own.
        if event.ship_type == AiShipType::RustSwarm {
            continue;
        }
        if rng.gen::<f32>() > WAVE_CHANCE {
            continue;
        }
        // Bigger carcasses draw bigger flocks.
        let ships = match event.ship_type {
            AiShipType::Dreadnought | AiShipType::VoidTitan => rng.gen_range(4..=5),
            AiShipType::IronTide | AiShipType::PressureKing => rng.gen_range(3..=4),
            _ => rng.gen_range(2..=3),
        };
        waves.pending.push(PendingWave {
            wreck: event.entity,
            position: event.position,
            timer: Timer::from_seconds(rng.gen_range(WAVE_DELAY_MIN..WAVE_DELAY_MAX), TimerMode::Once),
            ships,
        });
    }
}

/// When a wave's clock runs out and the carcass still has meat on it,
/// the vultures arrive at the edge and burn inward.
pub fn spawn_scavenger_waves(
    time: Res<Time>,
    mut commands: Commands,
    mut waves: ResMut<ScavengerWaves>,
    wreck_query: Query<&Wreck>,
    registry: Res<crate::building::ModuleRegistry>,
    asset_server: Res<AssetServer>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let mut rng = rand::thread_rng();
    waves.pending.retain_mut(|wave| {
        wave.timer.tick(time.delta());
        if !wave.timer.is_finished() {
            return true;
        }
        // Wreck already gone or picked clean — nothing to come for.
        if !wreck_query.get(wave.wreck).is_ok_and(|w| w.loot_remaining > 0) {
            return false;
        }
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let base = wave.position
            + Vec2::new(angle.cos(), angle.sin()) * rng.gen_range(SPAWN_DIST_MIN..SPAWN_DIST_MAX);
        for i in 0..wave.ships {
            let jitter = Vec2::new(rng.gen_range(-120.0..120.0), rng.gen_range(-120.0..120.0));
            // Spawned "at" the wreck position so their patrol waypoints
            // ring the carcass; the wreck-chasing brain does the rest.
            super::spawner::spawn_ai_ship(
                AiShipType::RustSwarm,
                base + jitter + Vec2::Y * (i as f32 * 40.0),
                &mut commands,
                &registry,
                &asset_server,
            );
        }
        notifications.write(ShowNotification {
            message: "Scavenger swarm inbound — they smell the wreck!".into(),
            notification_type: NotificationType::Warning,
            duration: 4.0,
        });
        false
    });
}

/// Swarm ships parked on a wreck eat its loot, one unit per ship every
/// few seconds — the visible smoke puffs are your cargo leaving.
pub fn scavengers_feed(
    time: Res<Time>,
    mut commands: Commands,
    swarm_query: Query<(&Transform, &AiShipType, &AiShipState)>,
    mut wreck_query: Query<(Entity, &GlobalTransform, &mut Wreck, &mut AiShipWreck, &Children)>,
    block_pos_query: Query<&GlobalTransform, Or<(With<Module>, With<HullSegment>)>>,
    mut notifications: MessageWriter<ShowNotification>,
    mut tick: Local<f32>,
    mut warned: Local<std::collections::HashSet<Entity>>,
) {
    *tick += time.delta_secs();
    if *tick < FEED_SECONDS {
        return;
    }
    *tick = 0.0;

    let mut rng = rand::thread_rng();
    for (wreck_entity, wreck_gt, mut wreck, mut ai_wreck, children) in wreck_query.iter_mut() {
        if wreck.loot_remaining == 0 {
            continue;
        }
        let wreck_pos = wreck_gt.translation().truncate();
        let feeders = swarm_query
            .iter()
            .filter(|(t, ship_type, state)| {
                **ship_type == AiShipType::RustSwarm
                    && !state.is_destroyed
                    && t.translation.truncate().distance(wreck_pos) < FEED_RANGE
            })
            .count() as u32;
        if feeders == 0 {
            continue;
        }

        let eaten = feeders.min(wreck.loot_remaining);
        wreck.loot_remaining -= eaten;
        ai_wreck.loot_remaining = ai_wreck.loot_remaining.saturating_sub(eaten);

        // Smoke puff over a random block per unit eaten — loss reads visually
        let blocks: Vec<Vec2> = children
            .iter()
            .filter_map(|c| block_pos_query.get(c).ok())
            .map(|gt| gt.translation().truncate())
            .collect();
        for _ in 0..eaten.min(3) {
            let at = if blocks.is_empty() {
                wreck_pos
            } else {
                blocks[rng.gen_range(0..blocks.len())]
            };
            crate::combat::spawn_hit_effect(&mut commands, at, Color::srgba(0.55, 0.45, 0.3, 0.7), 34.0);
        }

        if warned.insert(wreck_entity) {
            notifications.write(ShowNotification {
                message: "Scavengers are stripping the wreck!".into(),
                notification_type: NotificationType::Warning,
                duration: 4.0,
            });
        }
    }
}
