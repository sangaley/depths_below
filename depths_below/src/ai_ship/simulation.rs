use bevy::prelude::*;
use rand::Rng;
use crate::components::*;
use crate::building::ModuleRegistry;
use super::components::*;
use super::spawner;

/// Render distance - ships within this range get spawned as real entities.
/// Was 1800 — weapon ranges now reach up to 9,600 (see
/// combat::PROJECTILE_SPEED / spawn_projectile) and AI ships hold standoff
/// distances up to ~8,160 (85% of their longest weapon's range, see
/// ai_ship::movement). 10,000 means a ship materializes as a real entity
/// before the player is even in its weapon range, instead of after.
const RENDER_DISTANCE: f32 = 10_000.0;
/// Distance at which spawned entities get converted back to simulation. Was
/// 3500 — well inside the new ~8,160 max standoff distance, so a ship
/// holding a long-range fight would get yanked back to an abstract
/// simulated point (functionally "despawn") the moment it backed off to
/// actually use its weapon's range. 14,000 clears the max standoff with
/// real margin and stays under camera::cull_range (16,000) so a ship
/// doesn't visually vanish before it's converted back to simulation either.
const DESPAWN_DISTANCE: f32 = 14_000.0;

/// Initialize the world simulation with all factions in their territories
pub fn init_world_simulation(
    mut sim: ResMut<WorldSimulation>,
) {
    if sim.initialized {
        return;
    }
    sim.initialized = true;

    // DEPTHS_MOVETEST=1: bare movement sandbox. Normally zero AI ships, but
    // DEPTHS_MOVETEST_ENEMY=1 adds exactly one non-shooting dummy (see
    // ai_weapon_fire_system) close enough to immediately engage, so the
    // standoff/orbit "keep distance" behavior in ai_ship_movement_system can
    // be watched in isolation without the rest of the world simulation.
    // Drowned, not IronTide: IronTide is a ~10x16-cell battleship with a
    // full hull shell around every module — with the 45-unit "nearest
    // block" hit radius, shots just kept landing on whatever hull was
    // closest across that huge surface and never punched through to a
    // module, which read as "modules are invincible". Drowned is much
    // smaller (~6x10) while still holding a real standoff distance
    // (unlike RustSwarm, which rams point-blank).
    if crate::demo::skip_ai_ship_spawn() {
        if std::env::var("DEPTHS_MOVETEST_ENEMY").ok().as_deref() == Some("1") {
            // DEPTHS_MOVETEST_ENEMY_FACTION overrides the dummy's faction for
            // behavior-tree testing (e.g. "GlassEye", "IronTide") — defaults
            // to Drowned for the original damage-model testing use case.
            let faction = match std::env::var("DEPTHS_MOVETEST_ENEMY_FACTION").ok().as_deref() {
                Some("Leviathan") => AiShipType::Leviathan,
                Some("AbyssalCult") => AiShipType::AbyssalCult,
                Some("PressureKing") => AiShipType::PressureKing,
                Some("GlassEye") => AiShipType::GlassEye,
                Some("IronTide") => AiShipType::IronTide,
                Some("Blackwater") => AiShipType::Blackwater,
                Some("RustSwarm") => AiShipType::RustSwarm,
                _ => AiShipType::Drowned,
            };
            sim.ships.push(SimulatedShip {
                faction,
                position: Vec2::new(500.0, 0.0),
                velocity: Vec2::ZERO,
                health: 1.0,
                fuel: 1.0,
                behavior: SimBehavior::Patrolling,
                home_zone: Vec2::new(500.0, 0.0),
                patrol_radius: 2000.0,
                spawned: false,
                bounty_id: None,
            });
            info!("MOVETEST: single non-shooting {:?} dummy spawned at (500, 0) for damage-model testing", faction);
        }
        return;
    }

    let mut rng = rand::thread_rng();

    for territory in faction_territories() {
        for _ in 0..territory.ship_count {
            let angle = rng.gen_range(0.0..std::f32::consts::TAU);
            let dist = rng.gen_range(0.0..territory.radius * 0.8);
            let pos = territory.center + Vec2::new(angle.cos() * dist, angle.sin() * dist);

            let patrol_angle = rng.gen_range(0.0..std::f32::consts::TAU);
            let vel = Vec2::new(patrol_angle.cos(), patrol_angle.sin()) * 30.0;

            sim.ships.push(SimulatedShip {
                faction: territory.faction,
                position: pos,
                velocity: vel,
                health: 1.0,
                fuel: 1.0,
                behavior: SimBehavior::Patrolling,
                home_zone: territory.center,
                patrol_radius: territory.radius,
                spawned: false,
                bounty_id: None,
            });
        }
    }

    // Distant patrols around the starting area — beyond RENDER_DISTANCE so
    // the immediate spawn area stays calm; they materialize as the player
    // flies out. Factions chosen to be non-committal: Drowned wander
    // erratically, GlassEye never attacks.
    let ambient_patrols = [
        (AiShipType::Drowned,  Vec2::new(2600.0, -1800.0),  Vec2::new(-12.0, -8.0)),
        (AiShipType::GlassEye, Vec2::new(-3100.0, -1200.0), Vec2::new(10.0, -14.0)),
        (AiShipType::Drowned,  Vec2::new(700.0, -3300.0),   Vec2::new(-16.0, 6.0)),
    ];
    for (faction, pos, vel) in ambient_patrols {
        sim.ships.push(SimulatedShip {
            faction,
            position: pos,
            velocity: vel,
            health: 1.0,
            fuel: 1.0,
            behavior: SimBehavior::Patrolling,
            home_zone: pos,
            patrol_radius: 2000.0,
            spawned: false,
            bounty_id: None,
        });
    }

    // Roaming wanderers scattered across the space between and beyond the
    // fixed faction territories (now 25k-345k out, see faction_territories)
    // and out toward the first star system (~492k out, see celestial/mod.rs)
    // so the whole map reads as populated instead of "a cluster near spawn,
    // then empty space, then a star." 24 wanderers spread 20k-420k out.
    for i in 0..24 {
        let angle = (i as f32 / 24.0) * std::f32::consts::TAU + rng.gen_range(-0.2..0.2);
        let dist = rng.gen_range(20_000.0..420_000.0);
        let pos = Vec2::new(angle.cos() * dist, angle.sin() * dist);
        let heading = rng.gen_range(0.0..std::f32::consts::TAU);
        let faction = match i % 4 {
            0 => AiShipType::Drowned,
            1 => AiShipType::GlassEye,
            2 => AiShipType::RustSwarm,
            _ => AiShipType::Blackwater,
        };
        sim.ships.push(SimulatedShip {
            faction,
            position: pos,
            velocity: Vec2::new(heading.cos(), heading.sin()) * rng.gen_range(20.0..45.0),
            health: 1.0,
            fuel: 1.0,
            behavior: SimBehavior::Patrolling,
            home_zone: pos,
            patrol_radius: 18_000.0,
            spawned: false,
            bounty_id: None,
        });
    }

    info!("World simulation initialized with {} AI vessels across 8 factions", sim.ships.len());
}

/// Tick the off-screen simulation: move ships, resolve encounters
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
    let positions: Vec<(usize, AiShipType, Vec2, f32)> = sim.ships.iter().enumerate()
        .filter(|(_, s)| !s.spawned && s.behavior != SimBehavior::Dead)
        .map(|(i, s)| (i, s.faction, s.position, s.health))
        .collect();

    // Phase 1: Move off-screen ships
    for ship in sim.ships.iter_mut() {
        if ship.spawned || ship.behavior == SimBehavior::Dead {
            continue;
        }

        // Move
        ship.position += ship.velocity * dt;
        ship.fuel = (ship.fuel - 0.001 * dt).max(0.0);

        // Drift back toward home zone if too far. Was a flat 2500.0 for
        // every ship regardless of its actual territory size — see
        // SimulatedShip::patrol_radius doc comment.
        let home_dist = ship.position.distance(ship.home_zone);
        if home_dist > ship.patrol_radius {
            let toward_home = (ship.home_zone - ship.position).normalize_or_zero();
            ship.velocity = ship.velocity * 0.95 + toward_home * 20.0;
        }

        // Random course changes
        if rng.gen::<f32>() < 0.1 {
            let turn = rng.gen_range(-0.5..0.5);
            let speed = ship.velocity.length().max(20.0);
            let angle = ship.velocity.y.atan2(ship.velocity.x) + turn;
            ship.velocity = Vec2::new(angle.cos(), angle.sin()) * speed;
        }

        // Fuel exhaustion
        if ship.fuel <= 0.0 {
            ship.velocity *= 0.5;
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

                // Bounty targets never die to off-screen faction combat — a
                // health floor above zero instead of the normal 0.0 clamp.
                // Without this a tagged ship could be quietly killed by a
                // rival faction before the player ever reaches it, leaving
                // an active bounty contract permanently unfinishable.
                if let Some(ship_a) = sim.ships.get_mut(idx_a) {
                    if !ship_a.spawned {
                        let floor = if ship_a.bounty_id.is_some() { 0.05 } else { 0.0 };
                        ship_a.health = (ship_a.health - dmg_a).max(floor);
                        ship_a.behavior = SimBehavior::Fighting(idx_b);
                        if ship_a.health <= floor && floor == 0.0 {
                            ship_a.behavior = SimBehavior::Dead;
                        }
                    }
                }
                if let Some(ship_b) = sim.ships.get_mut(idx_b) {
                    if !ship_b.spawned {
                        let floor = if ship_b.bounty_id.is_some() { 0.05 } else { 0.0 };
                        ship_b.health = (ship_b.health - dmg_b).max(floor);
                        ship_b.behavior = SimBehavior::Fighting(idx_a);
                        if ship_b.health <= floor && floor == 0.0 {
                            ship_b.behavior = SimBehavior::Dead;
                        }
                    }
                }
            }
        }
    }

    // Phase 3: Respawn dead ships after a delay (represented by health recovery)
    for ship in sim.ships.iter_mut() {
        if ship.behavior == SimBehavior::Dead {
            // After "death", reset after some time (simulated by fuel as timer)
            ship.fuel -= 0.01;
            if ship.fuel <= -0.5 {
                // Respawn at home zone
                let angle = rng.gen_range(0.0..std::f32::consts::TAU);
                let dist = rng.gen_range(0.0..1500.0);
                ship.position = ship.home_zone + Vec2::new(angle.cos() * dist, angle.sin() * dist);
                ship.health = 1.0;
                ship.fuel = 1.0;
                ship.behavior = SimBehavior::Patrolling;
                ship.spawned = false;
                // This is a fresh respawn, not the same hull a completed
                // bounty was hunting — clear the old tag so the faction's
                // ship pool is eligible for new bounties again. Without
                // this, a single-ship faction (the bosses) could only ever
                // be offered as a bounty once, permanently.
                ship.bounty_id = None;
            }
        }
    }
}

/// Periodically send a small hostile wing after the player. They seed just
/// outside render distance, headed inward — the existing per-faction AI takes
/// over once they materialize. First wave holds off long enough for the
/// player to learn the controls.
pub fn spawn_raider_waves(
    time: Res<Time>,
    mut sim: ResMut<WorldSimulation>,
    ship_query: Query<&Transform, With<Ship>>,
    mut notifications: MessageWriter<crate::events::ShowNotification>,
    mut next_wave_at: Local<f32>,
    mut elapsed: Local<f32>,
) {
    let Ok(player_transform) = ship_query.single() else { return };
    *elapsed += time.delta_secs();

    if *next_wave_at == 0.0 {
        *next_wave_at = 180.0; // first raid: 3 minutes in
    }
    if *elapsed < *next_wave_at {
        return;
    }
    *next_wave_at = *elapsed + 150.0 + rand::random::<f32>() * 90.0;

    let player_pos = player_transform.translation.truncate();
    let mut rng = rand::thread_rng();

    let faction = match rng.gen_range(0..3) {
        0 => AiShipType::RustSwarm,   // swarm of junk ships
        1 => AiShipType::Blackwater,  // tactical mercs
        _ => AiShipType::Drowned,     // erratic ghost ships
    };
    let count = match faction {
        AiShipType::RustSwarm => rng.gen_range(3..=5),
        _ => rng.gen_range(2..=3),
    };

    let approach_angle = rng.gen_range(0.0..std::f32::consts::TAU);
    for i in 0..count {
        // Wide spacing: each ship comes in on its own bearing and range so
        // the wave arrives as a loose pincer, not a clump.
        let jitter = (i as f32 - count as f32 / 2.0) * 0.45 + rng.gen_range(-0.1..0.1);
        let angle = approach_angle + jitter;
        let dist = rng.gen_range(2200.0..3200.0);
        let pos = player_pos + Vec2::new(angle.cos(), angle.sin()) * dist;
        let toward_player = (player_pos - pos).normalize_or_zero() * 60.0;
        sim.ships.push(SimulatedShip {
            faction,
            position: pos,
            velocity: toward_player,
            health: 1.0,
            fuel: 1.0,
            behavior: SimBehavior::Patrolling,
            home_zone: player_pos,
            patrol_radius: 5000.0,
            spawned: false,
            bounty_id: None,
        });
    }

    notifications.write(crate::events::ShowNotification {
        message: "Hostile contacts inbound on radar!".into(),
        notification_type: crate::events::NotificationType::Danger,
        duration: 4.0,
    });
}

/// Spawn/despawn real entities based on player proximity
pub fn sync_simulation_entities(
    mut commands: Commands,
    mut sim: ResMut<WorldSimulation>,
    registry: Res<ModuleRegistry>,
    asset_server: Res<AssetServer>,
    ship_query: Query<&Transform, With<Ship>>,
    ai_ships: Query<(Entity, &Transform, &AiShipType, &AiShipState, Option<&BountyTarget>), With<AiShip>>,
) {
    let Ok(player_transform) = ship_query.single() else { return };
    let player_pos = player_transform.translation.truncate();

    // Spawn ships that entered render distance
    for sim_ship in sim.ships.iter_mut() {
        if sim_ship.spawned || sim_ship.behavior == SimBehavior::Dead {
            continue;
        }

        let dist = sim_ship.position.distance(player_pos);
        if dist < RENDER_DISTANCE {
            // Spawn as real entity
            let root = spawner::spawn_ai_ship(
                sim_ship.faction,
                sim_ship.position,
                &mut commands,
                &registry,
                &asset_server,
            );
            if let Some(id) = sim_ship.bounty_id {
                commands.entity(root).insert(BountyTarget(id));
            }
            sim_ship.spawned = true;
        }
    }

    // Despawn LIVE ships that left render distance, convert back to
    // simulation. Destroyed ships are deliberately excluded from this: they
    // become real, permanent wrecks (see ai_ship::wreck::ai_ship_death_system)
    // instead of being despawned and replaced with an abstract simulated
    // "dead" entry — so the player can fly away and come back later to
    // actually scavenge the hull they shot up, instead of it vanishing the
    // moment they're out of range.
    for (entity, transform, ship_type, state, bounty) in ai_ships.iter() {
        // Bounty-tagged ships are matched back to their exact sim entry by
        // id — the faction-only match below is ambiguous whenever more than
        // one ship of the same faction is spawned at once, which would risk
        // flagging the wrong sim ship (possibly the actual bounty target) as
        // dead/despawned.
        if state.is_destroyed {
            // One-time sim bookkeeping so this faction slot frees up for
            // future spawns — doesn't touch the real (now-wreck) entity.
            let sim_ship = if let Some(bounty) = bounty {
                sim.ships.iter_mut().find(|s| s.bounty_id == Some(bounty.0))
            } else {
                sim.ships.iter_mut().find(|s| s.spawned && s.faction == *ship_type)
            };
            if let Some(sim_ship) = sim_ship {
                sim_ship.behavior = SimBehavior::Dead;
                sim_ship.health = 0.0;
                sim_ship.spawned = false;
            }
            continue;
        }

        let pos = transform.translation.truncate();
        let dist = pos.distance(player_pos);

        if dist > DESPAWN_DISTANCE {
            commands.entity(entity).despawn();

            let sim_ship = if let Some(bounty) = bounty {
                sim.ships.iter_mut().find(|s| s.bounty_id == Some(bounty.0))
            } else {
                sim.ships.iter_mut().find(|s| s.spawned && s.faction == *ship_type)
            };
            if let Some(sim_ship) = sim_ship {
                sim_ship.position = pos;
                sim_ship.health = state.hull_integrity;
                sim_ship.spawned = false;
                sim_ship.behavior = SimBehavior::Patrolling;
            }
        }
    }
}

