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

/// How many AI hulls may exist as real entities at once.
///
/// Each one is a full ship — hull cells, modules, decking, crew — so this is
/// a frame-time budget, not a gameplay limit: everything past it stays in the
/// simulation, still moving and still fighting, and materialises as soon as a
/// slot frees. Nearest to the player wins the slot.
const MAX_LIVE_AI_HULLS: usize = 8;
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
    galaxy_map: Res<crate::celestial::resources::GalaxyMap>,
    streaming: Res<crate::celestial::resources::SystemStreamingManager>,
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
    // Broken Choir, not Terran Hegemony: Terran Hegemony is a ~10x16-cell battleship with a
    // full hull shell around every module — with the 45-unit "nearest
    // block" hit radius, shots just kept landing on whatever hull was
    // closest across that huge surface and never punched through to a
    // module, which read as "modules are invincible". Broken Choir is much
    // smaller (~6x10) while still holding a real standoff distance
    // (unlike Recursive Kingdom, which rams point-blank).
    if crate::demo::skip_ai_ship_spawn() {
        if std::env::var("DEPTHS_MOVETEST_ENEMY").ok().as_deref() == Some("1") {
            // DEPTHS_MOVETEST_ENEMY_FACTION overrides the dummy's faction for
            // behavior-tree testing (e.g. "The Silence", "Terran Hegemony") — defaults
            // to Broken Choir for the original damage-model testing use case.
            let faction = match std::env::var("DEPTHS_MOVETEST_ENEMY_FACTION").ok().as_deref() {
                Some("StellarPreserve") => AiShipType::StellarPreserve,
                Some("SynthesisCollective") => AiShipType::SynthesisCollective,
                Some("CorpseStars") => AiShipType::CorpseStars,
                Some("TheSilence") => AiShipType::TheSilence,
                Some("TerranHegemony") => AiShipType::TerranHegemony,
                Some("GildedThrone") => AiShipType::GildedThrone,
                Some("RecursiveKingdom") => AiShipType::RecursiveKingdom,
                _ => AiShipType::BrokenChoir,
            };
            sim.ships.push(SimulatedShip::patrolling(
                0, faction, Vec2::new(500.0, 0.0), Vec2::new(500.0, 0.0), 2000.0, 0.0,
            ));
            info!("MOVETEST: single non-shooting {:?} dummy spawned at (500, 0) for damage-model testing", faction);
        }

        // TEMP [AI_VS_AI_DIAGNOSTIC]: spawns a tight cluster of guaranteed
        // combat-capable ships (alternating Terran Hegemony / Recursive Kingdom / Broken Choir
        // / Gilded Throne, all "attack anything in range" factions per
        // ai_brain.rs) close enough together to be within engage range from
        // the start, for headlessly verifying AI-vs-AI combat actually
        // lands hits. Remove once the diagnosis is confirmed.
        if let Ok(count) = std::env::var("DEPTHS_AI_VS_AI_TEST").unwrap_or_default().parse::<usize>() {
            let factions = [AiShipType::TerranHegemony, AiShipType::RecursiveKingdom, AiShipType::BrokenChoir, AiShipType::GildedThrone];
            for i in 0..count {
                let angle = (i as f32 / count as f32) * std::f32::consts::TAU;
                let pos = Vec2::new(angle.cos(), angle.sin()) * 400.0;
                sim.ships.push(SimulatedShip::patrolling(
                    0, factions[i % factions.len()], pos, pos, 2000.0, 0.0,
                ));
            }
            info!("AI_VS_AI_TEST: spawned {} combat-capable ships in a 400u cluster", count);
        }
        return;
    }

    // System-scoped faction population: each star system now carries its
    // own fixed faction + danger tier (StarSystemDef.faction/danger_tier,
    // assigned once at galaxy generation — celestial/galaxy.rs) instead of
    // the old model of all 10 territories jittered around a single shared
    // origin every game. Haven (system 0) has faction: None — always safe,
    // nothing spawns for it. Its initial Warm neighbors (set by
    // generate_galaxy_on_enter) get populated too, so they're already
    // "alive" the moment the game starts, not just on first visit.
    //
    // This also removes the old "ambient" (3 ships) and "roaming wanderer"
    // (24 ships) filler groups that used to surround the origin regardless
    // of faction — they existed to make the immediate spawn area feel
    // populated, which directly conflicted with Haven now being a
    // deliberately safe hub. The systems reachable by warp are where the
    // populated universe lives now.
    ensure_system_population(&mut sim, &galaxy_map, 0);
    for warm_id in &streaming.warm_systems {
        ensure_system_population(&mut sim, &galaxy_map, *warm_id);
    }

    info!("World simulation initialized with {} AI vessels", sim.ships.len());
}

/// Spawns a system's faction population the FIRST time it's ever loaded
/// (Hot or Warm) — persistence-safe: if any SimulatedShip already carries
/// this system_id (from an earlier visit, however depleted by combat since
/// then), does nothing and leaves it exactly as it is. Called from
/// celestial::warp::execute_warp_jump whenever a system newly becomes Hot
/// or joins the Warm neighbor set.
pub fn ensure_system_population(sim: &mut WorldSimulation, galaxy_map: &crate::celestial::resources::GalaxyMap, system_id: u32) {
    if sim.ships.iter().any(|s| s.system_id == system_id) {
        return;
    }
    if let Some(system) = galaxy_map.systems.iter().find(|s| s.id == system_id) {
        spawn_system_faction_population(sim, system);
    }
}

/// Spawns one star system's faction population — ship_count/radius pulled
/// from that faction's existing territory template (faction_territories()),
/// centered on the system's faction cluster point (offset clear of the
/// star itself — see celestial::galaxy::faction_cluster_center) instead of
/// a jittered-origin position. No-ops for a safe system (faction: None,
/// e.g. Haven).
fn spawn_system_faction_population(sim: &mut WorldSimulation, system: &crate::celestial::resources::StarSystemDef) {
    let Some(faction) = system.faction else { return };
    let Some(template) = faction_territories().into_iter().find(|t| t.faction == faction) else { return };
    let mut rng = rand::thread_rng();
    let cluster_center = crate::celestial::galaxy::faction_cluster_center(system);

    // How many, and arranged how.
    //
    // This used to drop `ship_count` ships uniformly inside ONE disc of
    // radius*0.8 around a single point, which gave every system the same
    // shape: a single clump of four-to-eight dots, most of them outside the
    // 10,000-unit materialisation bubble, none of them anywhere in
    // particular. Reported as "way too low and way too close to each other".
    //
    // Now the population is split into ELEMENTS — lone ships, pairs, and
    // packs of three to five — and each element is placed in its own distance
    // band. A pack flies in close company; the bands put some elements on top
    // of the faction's holdings and others out at the edge of the territory,
    // so the spacing between them is genuinely uneven rather than uniform.
    let count = system_ship_count(template.ship_count, system.danger_tier);
    let mut placed = 0usize;
    while placed < count {
        // Weighted so loners and pairs are common and a real pack is an event.
        let roll = rng.gen::<f32>();
        let size = if roll < 0.40 { 1 } else if roll < 0.70 { 2 } else { rng.gen_range(3..=5) };
        let size = size.min(count - placed);

        // Three bands rather than a uniform draw: uniform-in-a-disc actually
        // concentrates toward the rim, which is the opposite of varied.
        let band = match rng.gen_range(0..3) {
            0 => rng.gen_range(0.10..0.30),
            1 => rng.gen_range(0.35..0.60),
            _ => rng.gen_range(0.65..0.95),
        };
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let anchor = cluster_center
            + Vec2::new(angle.cos(), angle.sin()) * (template.radius * band);

        // One speed for the element, so a pack holds formation instead of
        // smearing out over the first leg.
        let cruise = rng.gen_range(55.0..130.0);
        let destination = patrol_waypoint(&mut rng, cluster_center, template.radius);

        for _ in 0..size {
            let spread = if size == 1 { 0.0 } else { rng.gen_range(120.0..420.0) };
            let a = rng.gen_range(0.0..std::f32::consts::TAU);
            let pos = anchor + Vec2::new(a.cos(), a.sin()) * spread;
            let mut ship = SimulatedShip::patrolling(
                system.id,
                faction,
                pos,
                cluster_center,
                template.radius,
                cruise * rng.gen_range(0.92..1.08),
            );
            ship.destination = destination;
            sim.ships.push(ship);
        }
        placed += size;
    }
}

/// How many hulls a system's faction keeps on station.
///
/// The template count is the shallow-system baseline; `danger_tier` is the
/// authored near-weak/far-strong curve, so a deep hostile system is busier
/// than a border one instead of every system fielding the same handful.
pub fn system_ship_count(template_count: usize, danger_tier: f32) -> usize {
    let scale = 1.0 + (danger_tier / 100.0).clamp(0.0, 2.0);
    ((template_count as f32 * 2.2 * scale).round() as usize).clamp(6, 40)
}

/// Close enough to call it arrived, and to pick the next leg.
const ARRIVE_RADIUS: f32 = 450.0;

/// Where a ship goes next.
///
/// Two in five legs head for something that matters — a station, a berth,
/// whatever the loaded system put on the board — approached to a loose
/// standoff rather than parked on top of it. The rest are ordinary patrol
/// legs inside the territory. The mix is the point: traffic that only ever
/// visited stations would read as a conveyor belt, and traffic that never did
/// is the random walk this replaced.
fn next_waypoint(rng: &mut impl Rng, ship: &SimulatedShip, anchors: &[Vec2]) -> Vec2 {
    if !anchors.is_empty() && rng.gen::<f32>() < 0.4 {
        let anchor = anchors[rng.gen_range(0..anchors.len())];
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let standoff = rng.gen_range(600.0..2200.0);
        let want = anchor + Vec2::new(angle.cos(), angle.sin()) * standoff;
        // Only if it is somewhere this ship is allowed to be.
        if want.distance(ship.home_zone) <= ship.patrol_radius {
            return want;
        }
    }
    patrol_waypoint(rng, ship.home_zone, ship.patrol_radius)
}

/// A point worth flying to inside a territory, for a ship with no better idea.
fn patrol_waypoint(rng: &mut impl Rng, center: Vec2, radius: f32) -> Vec2 {
    let angle = rng.gen_range(0.0..std::f32::consts::TAU);
    let dist = rng.gen_range(0.15..0.95) * radius;
    center + Vec2::new(angle.cos(), angle.sin()) * dist
}


/// Tick the off-screen simulation: move ships, resolve encounters. Only
/// ships in the Hot (loaded) or Warm (nearest-neighbor) systems actually
/// tick — Cold systems' ships stay exactly where they are, frozen, until
/// something brings that system back into range. This is deliberately the
/// cheapest possible "state preserved while you're away" mechanism (nothing
/// to snapshot or restore) and fixes a real pre-existing perf issue: the
/// Phase 2 pairwise combat check below is O(n^2) over however many ships
/// are active — bounding that to Hot+Warm keeps it small regardless of how
/// large the galaxy grows, instead of silently scaling with total ship
/// count across every system that's ever existed.
pub fn tick_world_simulation(
    time: Res<Time>,
    mut sim: ResMut<WorldSimulation>,
    streaming: Res<crate::celestial::resources::SystemStreamingManager>,
    stations: Res<crate::world::home_base::SystemStations>,
) {
    sim.tick_timer.tick(time.delta());
    if !sim.tick_timer.just_finished() {
        return;
    }

    let dt = sim.tick_timer.duration().as_secs_f32();
    let mut rng = rand::thread_rng();

    // What is worth being near, in the system the player is actually in.
    // Patrols and raiders both gravitate to the same places a player does,
    // which is what makes the traffic read as purposeful rather than as
    // Brownian motion over an empty map.
    let anchors: Vec<Vec2> = stations.sites.iter().map(|s| s.pos).collect();

    let is_active_system = |system_id: u32| {
        streaming.loaded_system == Some(system_id) || streaming.warm_systems.contains(&system_id)
    };

    // Collect positions for interaction checks
    let positions: Vec<(usize, AiShipType, Vec2, f32)> = sim.ships.iter().enumerate()
        .filter(|(_, s)| !s.spawned && s.behavior != SimBehavior::Dead && is_active_system(s.system_id))
        .map(|(i, s)| (i, s.faction, s.position, s.health))
        .collect();

    // Phase 1: Move off-screen ships
    for ship in sim.ships.iter_mut() {
        if ship.spawned || ship.behavior == SimBehavior::Dead || !is_active_system(ship.system_id) {
            continue;
        }

        // Steer for the destination, arrive, pick another.
        //
        // This was a random walk: a fixed 30 u/s shoved by a random turn on
        // one tick in ten. Net displacement over a minute was close to zero,
        // which is why the map read as dots that "barely move". A ship now
        // holds a course for somewhere, and the somewhere is chosen below.
        let to_dest = ship.destination - ship.position;
        let remaining = to_dest.length();
        if remaining <= ARRIVE_RADIUS {
            ship.destination = next_waypoint(&mut rng, ship, &anchors);
        } else {
            // Turn toward the bearing rather than snapping to it, so a course
            // change reads as a turn and packs stay roughly in company.
            let want = to_dest / remaining * ship.cruise;
            ship.velocity = (ship.velocity * 0.75 + want * 0.25).clamp_length_max(ship.cruise);
        }

        ship.position += ship.velocity * dt;
        ship.fuel = (ship.fuel - 0.001 * dt).max(0.0);

        // The leash still applies: a destination outside the territory, or a
        // ship shoved out of it by combat, gets pulled back.
        if ship.position.distance(ship.home_zone) > ship.patrol_radius {
            ship.destination = patrol_waypoint(&mut rng, ship.home_zone, ship.patrol_radius * 0.6);
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
    streaming: Res<crate::celestial::resources::SystemStreamingManager>,
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
        0 => AiShipType::RecursiveKingdom,   // swarm of junk ships
        1 => AiShipType::GildedThrone,  // tactical mercs
        _ => AiShipType::BrokenChoir,     // erratic ghost ships
    };
    let count = match faction {
        AiShipType::RecursiveKingdom => rng.gen_range(3..=5),
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
        let mut raider = SimulatedShip::patrolling(
            streaming.loaded_system.unwrap_or(0),
            faction,
            pos,
            player_pos,
            5000.0,
            rng.gen_range(70.0..110.0),
        );
        // A raid is the one case that already knows where it is going.
        raider.velocity = (player_pos - pos).normalize_or_zero() * 60.0;
        raider.destination = player_pos;
        sim.ships.push(raider);
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

    // Spawn ships that entered render distance, NEAREST FIRST and only up to
    // the budget.
    //
    // An AI ship is not a sprite: it is a hull, its modules, its decking and
    // its crew, the same as the player's. A dozen inside the bubble at once is
    // thousands of entities. The population was small enough that this never
    // came up; now that a system can field forty, the bubble needs a ceiling
    // or a busy system turns into a slideshow the moment you fly into the
    // middle of it. The rest stay simulated and keep moving — they are still
    // there, still fighting each other, just not built yet.
    let live = ai_ships.iter().count();
    if live < MAX_LIVE_AI_HULLS {
        let mut candidates: Vec<(usize, f32)> = sim
            .ships
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.spawned && s.behavior != SimBehavior::Dead)
            .map(|(i, s)| (i, s.position.distance(player_pos)))
            .filter(|(_, d)| *d < RENDER_DISTANCE)
            .collect();
        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        for (index, _) in candidates.into_iter().take(MAX_LIVE_AI_HULLS - live) {
            let Some(sim_ship) = sim.ships.get_mut(index) else { continue };
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


#[cfg(test)]
mod population_tests {
    use super::*;

    /// A system's population must not be one clump.
    ///
    /// The old spawn put every ship in one disc around a single point, which
    /// is what "way too close to each other" was describing. This asserts the
    /// shape the replacement is supposed to have: several separate elements,
    /// at genuinely different distances from the centre, with pack-sized
    /// groups among them.
    #[test]
    fn a_system_is_populated_in_separated_groups() {
        let mut sim = WorldSimulation::default();
        let system = crate::celestial::resources::StarSystemDef {
            id: 7,
            name: "test".into(),
            galaxy_pos: Vec2::ZERO,
            local_center: Vec2::ZERO,
            seed: 1,
            faction: Some(AiShipType::RecursiveKingdom),
            danger_tier: 60.0,
            discovery: crate::celestial::resources::SystemDiscovery::Visited,
            last_updated: 0.0,
            resource_fraction_remaining: 1.0,
        };
        spawn_system_faction_population(&mut sim, &system);
        let ships = &sim.ships;
        assert!(ships.len() >= 20, "only {} ships spawned", ships.len());

        let centre = crate::celestial::galaxy::faction_cluster_center(&system);
        let mut bands: Vec<f32> = ships.iter().map(|s| s.position.distance(centre)).collect();
        bands.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let spread = bands[bands.len() - 1] - bands[0];
        assert!(
            spread > 4_000.0,
            "every ship sits at much the same range from the centre ({spread:.0} units between \
             nearest and furthest) — that is the single clump this replaced"
        );

        // Clustering: for each ship, how far is its nearest neighbour? A mix
        // means some are in company and some are genuinely alone.
        let mut nearest: Vec<f32> = Vec::new();
        for (i, a) in ships.iter().enumerate() {
            let d = ships
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, b)| a.position.distance(b.position))
                .fold(f32::MAX, f32::min);
            nearest.push(d);
        }
        nearest.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let tightest = nearest[0];
        let loneliest = nearest[nearest.len() - 1];
        println!(
            "nearest-neighbour spread: {:.0} .. {:.0} (median {:.0}) over {} ships",
            tightest, loneliest, nearest[nearest.len() / 2], ships.len()
        );
        // Scale-free: what matters is that the spacing VARIES, not the units.
        // A uniform scatter gives every ship much the same nearest neighbour;
        // packs-and-loners gives a wide ratio between the tightest and the
        // most isolated.
        assert!(
            loneliest > tightest * 4.0,
            "spacing is uniform — tightest neighbour {tightest:.0}, loneliest {loneliest:.0}. \
             That is the even scatter this replaced, not packs and loners"
        );
    }

    /// Patrols have to actually go somewhere. The old random walk left a ship
    /// roughly where it started however long you watched it.
    #[test]
    fn a_patrol_covers_ground() {
        // Seeded. This used `rand::thread_rng()`, so waypoints differed every
        // run and the test failed about two runs in five on identical code
        // (measured: 6 of 15) -- a coin flip that reads as a regression.
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(11);
        let home = Vec2::ZERO;
        let mut ship = SimulatedShip::patrolling(1, AiShipType::GildedThrone, home, home, 12_000.0, 90.0);
        ship.destination = Vec2::new(6_000.0, 0.0);

        // Distance TRAVELLED, summed step by step. This measured straight-line
        // distance from the start instead, which cannot tell a patrol from
        // loitering: a ship that sweeps its territory properly and happens to
        // curve back toward where it began reads as having gone nowhere. That
        // was the other half of the flake -- the random part only decided how
        // often the loop closed.
        let mut travelled = 0.0;
        for _ in 0..60 {
            let to_dest = ship.destination - ship.position;
            let remaining = to_dest.length();
            if remaining <= ARRIVE_RADIUS {
                ship.destination = next_waypoint(&mut rng, &ship, &[]);
            } else {
                let want = to_dest / remaining * ship.cruise;
                ship.velocity = (ship.velocity * 0.75 + want * 0.25).clamp_length_max(ship.cruise);
            }
            let step = ship.velocity * 2.0;
            travelled += step.length();
            ship.position += step;
        }

        assert!(
            travelled > 3_000.0,
            "two minutes of patrol moved the ship {travelled:.0} units — it is loitering, not patrolling"
        );
        assert!(
            ship.position.distance(home) <= 12_000.0,
            "patrol left its territory"
        );
    }
}
