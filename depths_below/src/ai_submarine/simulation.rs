use bevy::prelude::*;
use rand::Rng;
use crate::components::*;
use crate::building::ModuleRegistry;
use super::components::*;
use super::spawner;

/// Render distance - subs within this range get spawned as real entities
const RENDER_DISTANCE: f32 = 1800.0;
/// Distance at which spawned entities get converted back to simulation
const DESPAWN_DISTANCE: f32 = 3500.0;

/// Initialize the world simulation with all factions in their territories
pub fn init_world_simulation(
    mut sim: ResMut<WorldSimulation>,
) {
    if sim.initialized {
        return;
    }
    sim.initialized = true;

    let mut rng = rand::thread_rng();

    for territory in faction_territories() {
        for _ in 0..territory.sub_count {
            let angle = rng.gen_range(0.0..std::f32::consts::TAU);
            let dist = rng.gen_range(0.0..territory.radius * 0.8);
            let pos = territory.center + Vec2::new(angle.cos() * dist, angle.sin() * dist);

            let patrol_angle = rng.gen_range(0.0..std::f32::consts::TAU);
            let vel = Vec2::new(patrol_angle.cos(), patrol_angle.sin()) * 30.0;

            sim.subs.push(SimulatedSub {
                faction: territory.faction,
                position: pos,
                velocity: vel,
                health: 1.0,
                fuel: 1.0,
                behavior: SimBehavior::Patrolling,
                home_zone: territory.center,
                spawned: false,
            });
        }
    }

    info!("World simulation initialized with {} AI vessels across 8 factions", sim.subs.len());
}

/// Tick the off-screen simulation: move subs, resolve encounters
pub fn tick_world_simulation(
    time: Res<Time>,
    mut sim: ResMut<WorldSimulation>,
) {
    sim.tick_timer.tick(time.delta());
    if !sim.tick_timer.just_finished() {
        return;
    }

    let dt = sim.tick_timer.duration().as_secs_f32();
    let mut rng = rand::thread_rng();

    // Collect positions for interaction checks
    let positions: Vec<(usize, AiSubType, Vec2, f32)> = sim.subs.iter().enumerate()
        .filter(|(_, s)| !s.spawned && s.behavior != SimBehavior::Dead)
        .map(|(i, s)| (i, s.faction, s.position, s.health))
        .collect();

    // Phase 1: Move off-screen subs
    for sub in sim.subs.iter_mut() {
        if sub.spawned || sub.behavior == SimBehavior::Dead {
            continue;
        }

        // Move
        sub.position += sub.velocity * dt;
        sub.fuel = (sub.fuel - 0.001 * dt).max(0.0);

        // Drift back toward home zone if too far
        let home_dist = sub.position.distance(sub.home_zone);
        let territory_radius = 2500.0;
        if home_dist > territory_radius {
            let toward_home = (sub.home_zone - sub.position).normalize_or_zero();
            sub.velocity = sub.velocity * 0.95 + toward_home * 20.0;
        }

        // Random course changes
        if rng.gen::<f32>() < 0.1 {
            let turn = rng.gen_range(-0.5..0.5);
            let speed = sub.velocity.length().max(20.0);
            let angle = sub.velocity.y.atan2(sub.velocity.x) + turn;
            sub.velocity = Vec2::new(angle.cos(), angle.sin()) * speed;
        }

        // Fuel exhaustion
        if sub.fuel <= 0.0 {
            sub.velocity *= 0.5;
        }
    }

    // Phase 2: Off-screen encounters between factions
    for i in 0..positions.len() {
        for j in (i + 1)..positions.len() {
            let (idx_a, faction_a, pos_a, _health_a) = positions[i];
            let (idx_b, faction_b, pos_b, _health_b) = positions[j];

            let dist = pos_a.distance(pos_b);
            if dist > 500.0 { continue; }

            // Check hostility
            if factions_hostile(faction_a, faction_b) {
                // Simulated combat: both take damage proportional to opponent strength
                let dmg_a = faction_power(faction_b) * 0.05 * dt;
                let dmg_b = faction_power(faction_a) * 0.05 * dt;

                if let Some(sub_a) = sim.subs.get_mut(idx_a) {
                    if !sub_a.spawned {
                        sub_a.health = (sub_a.health - dmg_a).max(0.0);
                        sub_a.behavior = SimBehavior::Fighting(idx_b);
                        if sub_a.health <= 0.0 {
                            sub_a.behavior = SimBehavior::Dead;
                        }
                    }
                }
                if let Some(sub_b) = sim.subs.get_mut(idx_b) {
                    if !sub_b.spawned {
                        sub_b.health = (sub_b.health - dmg_b).max(0.0);
                        sub_b.behavior = SimBehavior::Fighting(idx_a);
                        if sub_b.health <= 0.0 {
                            sub_b.behavior = SimBehavior::Dead;
                        }
                    }
                }
            }
        }
    }

    // Phase 3: Respawn dead subs after a delay (represented by health recovery)
    for sub in sim.subs.iter_mut() {
        if sub.behavior == SimBehavior::Dead {
            // After "death", reset after some time (simulated by fuel as timer)
            sub.fuel -= 0.01;
            if sub.fuel <= -0.5 {
                // Respawn at home zone
                let angle = rng.gen_range(0.0..std::f32::consts::TAU);
                let dist = rng.gen_range(0.0..1500.0);
                sub.position = sub.home_zone + Vec2::new(angle.cos() * dist, angle.sin() * dist);
                sub.health = 1.0;
                sub.fuel = 1.0;
                sub.behavior = SimBehavior::Patrolling;
                sub.spawned = false;
            }
        }
    }
}

/// Spawn/despawn real entities based on player proximity
pub fn sync_simulation_entities(
    mut commands: Commands,
    mut sim: ResMut<WorldSimulation>,
    registry: Res<ModuleRegistry>,
    asset_server: Res<AssetServer>,
    sub_query: Query<&Transform, With<Submarine>>,
    ai_subs: Query<(Entity, &Transform, &AiSubType, &AiSubState), With<AiSubmarine>>,
) {
    let Ok(player_transform) = sub_query.get_single() else { return };
    let player_pos = player_transform.translation.truncate();

    // Spawn subs that entered render distance
    for sim_sub in sim.subs.iter_mut() {
        if sim_sub.spawned || sim_sub.behavior == SimBehavior::Dead {
            continue;
        }

        let dist = sim_sub.position.distance(player_pos);
        if dist < RENDER_DISTANCE {
            // Spawn as real entity
            spawner::spawn_ai_submarine(
                sim_sub.faction,
                sim_sub.position,
                &mut commands,
                &registry,
                &asset_server,
            );
            sim_sub.spawned = true;
        }
    }

    // Despawn subs that left render distance, convert back to simulation
    for (entity, transform, sub_type, state) in ai_subs.iter() {
        let pos = transform.translation.truncate();
        let dist = pos.distance(player_pos);

        if dist > DESPAWN_DISTANCE || state.is_destroyed {
            commands.entity(entity).despawn_recursive();

            // Update simulation data
            if !state.is_destroyed {
                if let Some(sim_sub) = sim.subs.iter_mut().find(|s| s.spawned && s.faction == *sub_type) {
                    sim_sub.position = pos;
                    sim_sub.health = state.hull_integrity;
                    sim_sub.spawned = false;
                    sim_sub.behavior = SimBehavior::Patrolling;
                }
            } else {
                // Mark as dead in simulation
                if let Some(sim_sub) = sim.subs.iter_mut().find(|s| s.spawned && s.faction == *sub_type) {
                    sim_sub.behavior = SimBehavior::Dead;
                    sim_sub.health = 0.0;
                    sim_sub.spawned = false;
                }
            }
        }
    }
}

/// Returns combat power rating for a faction (used in off-screen combat)
fn faction_power(faction: AiSubType) -> f32 {
    match faction {
        AiSubType::IronTide => 3.0,      // Battleship - strongest
        AiSubType::Blackwater => 2.0,     // Elite mercs
        AiSubType::PressureKing => 2.5,   // Heavy armor + weapons
        AiSubType::AbyssalCult => 1.5,    // Bio-weapons
        AiSubType::Leviathan => 1.2,      // Creature + some weapons
        AiSubType::Drowned => 1.0,        // Already damaged
        AiSubType::RustSwarm => 0.5,      // Weak individually
        AiSubType::GlassEye => 0.1,       // No weapons
    }
}
