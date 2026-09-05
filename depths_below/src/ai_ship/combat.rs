use bevy::prelude::*;

use crate::components::*;
use crate::events::*;
use crate::combat::{spawn_floating_damage, spawn_hit_effect};
use crate::combat::ammo_types::KineticAmmoType;
use super::components::*;

/// AI ships in Engaging state fire weapons at their current AiShipTarget —
/// the player OR another AI ship, whichever ai_brain picked this tick (see
/// AiShipTarget's doc comment). WHO to target is only re-decided every
/// 0.25s (the brain tick); WHERE to aim is re-read from that target's live
/// Transform every single frame this system runs — AiShipTarget.position is
/// a snapshot from the moment it was picked, stale by up to 0.25s, which
/// was enough for an orbiting/strafing ship (standard combat maneuver, see
/// movement.rs's standoff-orbit) to be gone from that point by the time a
/// shot arrived. Live lookup fixes shots consistently whiffing at range.
///
/// Two upgrades layered on top of that live aim, both to make enemies an
/// actual threat: LEAD PREDICTION — shots aim where the target WILL be, via
/// the same calculate_lead the player's own guns use, so a moving ship no
/// longer simply outruns enemy fire; and SUBSYSTEM AIMING — per-faction
/// aim_priority sends rounds at a specific enemy module (your weapons /
/// engines / reactor) instead of the hull centre, cached on
/// AiShipTarget.subsystem and re-picked when it's destroyed. Damage is
/// resolved by impact geometry, so aiming a shot at your engine genuinely
/// knocks out your engine. Empty-doctrine factions keep aiming centre-of-mass.
pub fn ai_weapon_fire_system(
    time: Res<Time>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut ai_ships: Query<(
        Entity,
        &Transform,
        &AiShipType,
        &AiShipBehavior,
        &mut AiShipTarget,
        &Children,
        Option<&super::power::AiPowerState>,
    ), With<AiShip>>,
    mut weapon_query: Query<(
        &mut Weapon,
        &mut WeaponCooldown,
        &Module,
        &AmmoStorage,
        &OwnedByAiShip,
        Option<&ModuleEfficiency>,
        // Faction loadouts have set this since layouts.rs was written
        // (apply_module_extras on spawn); nothing ever read it at fire time,
        // so every AI shot resolved as unspecialised regardless.
        Option<&crate::building::customization::tuning::SelectedAmmo>,
    )>,
    player_query: Query<&Transform, With<Ship>>,
    target_transform_query: Query<&Transform>,
    // Ship hit detection (combat/projectiles.rs) centers on the shield's
    // world_center — the blocks' centroid, not the root, since the root is
    // often at one end of the layout. Aiming at the raw root position was
    // consistent geometry mismatch on any ship with an off-center layout.
    target_shield_query: Query<&crate::combat::shields::ShipShield>,
    // Live world position + health of ANY module on ANY ship (player or AI) —
    // used to aim at, and lazily re-pick, a specific enemy subsystem.
    module_pos_query: Query<(&Module, &GlobalTransform)>,
    children_query: Query<&Children>,
    // Target velocity, for lead prediction. Player and AI ships both carry it.
    velocity_query: Query<&Velocity>,
    mut fired_events: MessageWriter<WeaponFired>,
) {
    // DEPTHS_MOVETEST_ENEMY spawns a target dummy that's shot-free by
    // default (for testing movement/damage-model in isolation). Set
    // DEPTHS_MOVETEST_ENEMY_SHOOTS=1 too to let it fire back.
    if crate::demo::skip_ai_ship_spawn()
        && std::env::var("DEPTHS_MOVETEST_ENEMY_SHOOTS").ok().as_deref() != Some("1")
    {
        return;
    }

    let player_pos = player_query.single().ok().map(|t| t.translation.truncate());

    for (ai_entity, ai_transform, ship_type, behavior, mut ai_target, children, ai_power) in ai_ships.iter_mut() {
        if *behavior != AiShipBehavior::Engaging {
            continue;
        }

        // Power-starved ships hold fire — same hard cutoff the player's own
        // kinetic/missile weapons already use (combat/new_projectiles.rs,
        // combat/missiles.rs). None (graph not computed yet this tick, e.g.
        // the ship just spawned) defaults to permissive so a fresh ship
        // isn't blocked before its first power tick ever runs.
        if ai_power.is_some_and(|p| p.power_balance < 0.0) {
            continue;
        }

        // Centre-of-mass of whoever the brain picked, re-read fresh every
        // frame. Falls back to the last-known snapshot (target despawned
        // mid-frame, say), then to the player, only if the live lookup fails.
        // This is the range reference and the aim fallback for factions with
        // no subsystem doctrine.
        let Some(centroid) = ai_target.entity
            .and_then(|e| target_transform_query.get(e).ok().map(|t| {
                target_shield_query.get(e).ok()
                    .map(|s| s.world_center(t))
                    .unwrap_or_else(|| t.translation.truncate())
            }))
            .or_else(|| Some(ai_target.position).filter(|_| ai_target.entity.is_some()))
            .or(player_pos)
        else { continue };

        let ai_pos = ai_transform.translation.truncate();

        // --- Subsystem aim point (faction doctrine) ---
        // Default to centre-of-mass; only override when this faction targets
        // subsystems AND a matching live module is found on the target.
        let mut aim_base = centroid;
        let priorities = aim_priority(*ship_type);
        if !priorities.is_empty() {
            if let Some(target_entity) = ai_target.entity {
                // Reuse the cached pick while it's still a live module.
                let cached = ai_target.subsystem
                    .and_then(|m| module_pos_query.get(m).ok())
                    .filter(|(module, _)| module.is_active && module.health > 0.0)
                    .map(|(_, gt)| gt.translation().truncate());
                if let Some(p) = cached {
                    aim_base = p;
                } else {
                    // Re-pick: walk the doctrine's category priority; within a
                    // category take the module nearest the shooter (near side
                    // first — the round has to punch through outer blocks to
                    // reach it anyway).
                    let mut chosen: Option<(Entity, Vec2)> = None;
                    if let Ok(kids) = children_query.get(target_entity) {
                        for cat in priorities {
                            let mut best: Option<(Entity, Vec2, f32)> = None;
                            for child in kids.iter() {
                                let Ok((module, gt)) = module_pos_query.get(child) else { continue };
                                if !module.is_active || module.health <= 0.0 { continue; }
                                if module.module_type.category() != *cat { continue; }
                                let wp = gt.translation().truncate();
                                let d = wp.distance(ai_pos);
                                if best.map_or(true, |(_, _, bd)| d < bd) {
                                    best = Some((child, wp, d));
                                }
                            }
                            if let Some((e, wp, _)) = best {
                                chosen = Some((e, wp));
                                break;
                            }
                        }
                    }
                    match chosen {
                        Some((e, wp)) => { ai_target.subsystem = Some(e); aim_base = wp; }
                        None => { ai_target.subsystem = None; } // no live target module; aim centre-of-mass
                    }
                }
            }
        }

        // Target velocity, for lead prediction (player and AI both carry it).
        let target_vel = ai_target.entity
            .and_then(|e| velocity_query.get(e).ok())
            .map(|v| v.0)
            .unwrap_or(Vec2::ZERO);

        // Range is gated on the ship centre, so a subsystem sitting a little
        // farther out than the hull centroid doesn't push the target out of
        // "in range" on its own.
        let dist_to_target = ai_pos.distance(centroid);

        for child in children.iter() {
            let Ok((mut weapon, mut cooldown, module, ammo_storage, _owned, eff, loaded)) =
                weapon_query.get_mut(child)
            else {
                continue;
            };

            if !module.is_active || module.health <= 0.0
                || (!crate::combat::INFINITE_AMMO && weapon.ammo == 0) {
                continue;
            }

            // Unstaffed weapon stations produce nothing — same rule the
            // player's own ship runs under (compute_module_efficiency,
            // crew/mod.rs). Every weapon module is crew_station:true in the
            // registry, so this is a real gate for every AI faction, scaled
            // by crew_fill_fraction per faction (ai_ship::components).
            let efficiency = effective_efficiency(module, eff);
            if efficiency <= 0.0 {
                continue;
            }

            // Only fire if the target is within weapon range
            if dist_to_target > weapon.range {
                continue;
            }

            // Tick cooldown
            cooldown.timer.tick(time.delta());
            if !cooldown.timer.is_finished() {
                continue;
            }

            cooldown.timer.reset();
            if !crate::combat::INFINITE_AMMO {
                weapon.ammo = weapon.ammo.saturating_sub(1);
            }
            fired_events.write(WeaponFired {
                weapon_type: module.module_type,
                position: ai_pos,
                from_player: false,
            });

            // Muzzle speed: the SAME per-weapon base the player's own guns
            // fire at (base_projectile_speed — Railgun 9000, Cannon 6000, …),
            // not the old flat 1800 the AI used for everything. At any real
            // range a 1800 shell took 3-5× as long to arrive as the player's
            // same weapon, so enemy fire crawled in and got walked out of —
            // most of "they don't hit me." Enemies fire at the untuned base
            // speed; the player can still tune theirs faster. (× the loaded
            // ammo's own velocity profile, same as the player.)
            let base_speed =
                crate::building::customization::tuning::base_projectile_speed(module.module_type);
            let proj_speed = base_speed * ammo_storage.ammo_type.speed_mult();

            // Lead the shot: aim where the subsystem/centroid WILL be by the
            // time the round gets there — turns "sprays where you were" into
            // "actually hits a moving ship."
            //
            // SHOOTER VELOCITY IS ZERO here, deliberately. calculate_lead's
            // relative-velocity math assumes the projectile inherits the
            // shooter's own velocity (the player's guns do — see
            // new_projectiles.rs). The legacy AI projectile does NOT: it flies
            // a plain straight line at a fixed speed from muzzle to aim point
            // (projectiles.rs projectile_movement). Feeding the enemy's own
            // velocity in made the aim over-compensate by shooter_vel ×
            // travel_time — a big miss for a ship strafing its standoff orbit.
            // Leading purely on the TARGET's velocity matches how the round
            // actually travels. Enemies aim true (no accuracy spread); the
            // target's own jinking over the flight is the only reason to miss.
            let aim_point = crate::combat::targeting::lead_prediction::calculate_lead(
                ai_pos,
                Vec2::ZERO,
                aim_base,
                target_vel,
                Vec2::ZERO,
                proj_speed,
                crate::combat::targeting::lead_prediction::PredictionTier::BasicLead,
                weapon.range,
            ).aim_point;

            crate::combat::projectiles::spawn_projectile(
                &mut commands,
                &asset_server,
                ai_pos,
                aim_point,
                weapon.damage * efficiency,
                base_speed,
                weapon.range,
                crate::components::ProjectileOwner::AiShip(ai_entity),
                ammo_storage.ammo_type,
                loaded.map(|a| a.0),
            );
        }
    }
}

/// Which enemy subsystems a faction tries to shoot out, in priority order. An
/// empty list means "no doctrine" — aim centre-of-mass, the historical
/// behaviour. This is what gives factions distinct combat personalities:
/// professionals defang you, gatekeepers strand you, zealots and bosses go for
/// the core, dumb swarms just hammer the middle.
fn aim_priority(faction: AiShipType) -> &'static [crate::components::ModuleCategory] {
    use crate::components::ModuleCategory::*;
    use AiShipType::*;
    match faction {
        // Elite mercs: disable your guns, then your drive — a clean takedown.
        Blackwater => &[Weapons, Propulsion],
        // Battleship doctrine: silence the guns, then crack the reactor.
        IronTide => &[Weapons, Power],
        // Deep-zone gatekeepers: kill your engines, strand you in the dark.
        PressureKing => &[Propulsion, Weapons],
        // Bosses: methodical — guns first, then the reactor for the kill.
        Dreadnought => &[Weapons, Power],
        VoidTitan => &[Power, Weapons],
        // Zealots: fixate on the reactor, a holy execution.
        AbyssalCult => &[Power],
        // Mindless ghosts / dumb swarm / everything else: centre of mass.
        _ => &[],
    }
}

// ============================================================================
// DISTRESS CALLS — reinforcement aggro WITHOUT spawning ships from nowhere
// (spawn_raider_waves was cut for exactly that reason — see ai_ship/mod.rs).
// A ship in a real fight broadcasts; nearby SAME-FACTION fighters already in
// the world converge on and engage whoever the caller is fighting.
// ============================================================================

/// How far a distress call reaches. Tuned to a patrol/nest cluster, not the
/// whole map — territories sit tens of thousands of units apart, so a call
/// only ever wakes a faction's own neighbours, never a distant faction.
const DISTRESS_RADIUS: f32 = 4500.0;
/// How long a summoned ship stays aggro'd on the caller's target before it
/// drifts back to its own patrol.
const ALERT_DURATION: f32 = 25.0;
/// Minimum gap between one ship's distress broadcasts.
const DISTRESS_COOLDOWN: f32 = 12.0;

/// Ships mid-fight call for backup; nearby same-faction fighters answer.
///
/// Two passes because both the caller (to stamp its re-broadcast cooldown)
/// and every responder (to receive the alert) need `&mut AiShipState`, and a
/// single mutable iteration can't cross-reference other rows. Pass 1 reads out
/// the fresh broadcasts; pass 2 ticks every ship's timers and applies them.
/// The brain then acts on `alert_target`/`alert_timer` (see its distress-
/// response arm). Bevy serialises this against the brain/damage systems
/// automatically (all want `&mut AiShipState`), so there's no ordering to fix.
pub fn ai_distress_system(
    time: Res<Time>,
    mut ai_ships: Query<
        (Entity, &Transform, &AiShipType, &AiShipBehavior, &AiShipTarget, &mut AiShipState),
        With<AiShip>,
    >,
    player_query: Query<Entity, With<Ship>>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let dt = time.delta_secs();
    let player = player_query.single().ok();

    // PASS 1 — collect fresh broadcasts: a fighting ship, off cooldown, that's
    // actually in trouble (recently hit or hull hurt) and has a target.
    let mut broadcasts: Vec<(Entity, AiShipType, Vec2, Entity)> = Vec::new();
    for (e, tf, ftype, behavior, target, state) in ai_ships.iter() {
        if state.is_destroyed { continue; }
        if *behavior != AiShipBehavior::Engaging { continue; }
        if state.distress_cooldown > 0.0 { continue; }
        if !faction_fights(*ftype) { continue; }
        let Some(tgt) = target.entity else { continue };
        let in_trouble = state.last_hit_timer < 5.0 || state.hull_integrity < 0.9;
        if !in_trouble { continue; }
        broadcasts.push((e, *ftype, tf.translation.truncate(), tgt));
    }

    // PASS 2 — tick every ship's timers, stamp caller cooldowns, and alert
    // nearby same-faction fighters onto the caller's target.
    let mut player_backup_called = false;
    for (e, tf, ftype, _behavior, _target, mut state) in ai_ships.iter_mut() {
        state.alert_timer = (state.alert_timer - dt).max(0.0);
        state.distress_cooldown = (state.distress_cooldown - dt).max(0.0);
        if state.is_destroyed { continue; }

        if broadcasts.iter().any(|(be, _, _, _)| *be == e) {
            state.distress_cooldown = DISTRESS_COOLDOWN;
        }

        if !faction_fights(*ftype) { continue; }
        let pos = tf.translation.truncate();
        // Whether this ship was already answering a call before this frame —
        // used so the "reinforcements!" warning fires once per NEW recruit,
        // not every time an already-committed ship's alert is refreshed.
        let was_alerted = state.alert_timer > 0.0;

        // Answer the nearest same-faction call within earshot (never one's own).
        let mut nearest: Option<(f32, Entity)> = None;
        for (be, bfaction, bpos, btgt) in &broadcasts {
            if *be == e || *bfaction != *ftype { continue; }
            let d = pos.distance(*bpos);
            if d > DISTRESS_RADIUS { continue; }
            if nearest.map_or(true, |(nd, _)| d < nd) {
                nearest = Some((d, *btgt));
            }
        }
        if let Some((_, btgt)) = nearest {
            state.alert_target = Some(btgt);
            state.alert_timer = state.alert_timer.max(ALERT_DURATION);
            if !was_alerted && Some(btgt) == player {
                player_backup_called = true;
            }
        }
    }

    if player_backup_called {
        notifications.write(ShowNotification {
            message: "Enemy is calling in reinforcements!".into(),
            notification_type: NotificationType::Warning,
            duration: 3.0,
        });
    }
}

/// Process damage to AI ships — per-module penetration
pub fn process_ai_ship_damage_system(
    mut damage_events: MessageReader<AiShipDamaged>,
    mut ai_ships: Query<(&mut AiShipState, &Children), With<AiShip>>,
    mut hull_query: Query<(&mut HullSegment, &Transform, &OwnedByAiShip), Without<Module>>,
    mut module_query: Query<(&mut Module, &Transform, &OwnedByAiShip), Without<HullSegment>>,
    mut destroyed_events: MessageWriter<AiShipDestroyed>,
    ai_ship_query: Query<(&Transform, &AiShipType, Option<&BountyTarget>), With<AiShip>>,
    mut commands: Commands,
) {
    for event in damage_events.read() {
        let Ok((mut state, children)) = ai_ships.get_mut(event.target) else {
            continue;
        };

        state.last_hit_timer = 0.0;
        // Preserve the last known attributable attacker across non-
        // attributable damage ticks (fire DoT, self-detonation) rather
        // than clearing it — a ship mid-burn from an earlier shot should
        // still remember who fired it.
        if event.attacker.is_some() {
            state.last_attacker = event.attacker;
        }

        let impact_pos = event.position.unwrap_or(Vec2::ZERO);
        let mut remaining_damage = event.amount;

        // Collect child hull segments sorted by distance from impact
        let mut hull_hits: Vec<(Entity, f32)> = Vec::new();
        for child in children.iter() {
            if let Ok((_, hull_transform, owned)) = hull_query.get(child) {
                if owned.root == event.target {
                    let dist = hull_transform.translation.truncate().distance(impact_pos);
                    hull_hits.push((child, dist));
                }
            }
        }
        hull_hits.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Apply damage to nearest hull segments
        for (hull_entity, _dist) in &hull_hits {
            if remaining_damage <= 0.0 {
                break;
            }
            if let Ok((mut hull, hull_transform, _)) = hull_query.get_mut(*hull_entity) {
                let damage_to_apply = remaining_damage.min(hull.health);
                hull.health -= damage_to_apply;
                remaining_damage -= damage_to_apply;

                spawn_floating_damage(
                    &mut commands,
                    hull_transform.translation.truncate(),
                    damage_to_apply,
                    Color::srgb(1.0, 0.3, 0.3),
                );
                spawn_hit_effect(
                    &mut commands,
                    hull_transform.translation.truncate(),
                    Color::srgb(1.0, 0.5, 0.2),
                    16.0,
                );
            }
        }

        // If damage penetrates hull, hit nearest modules
        if remaining_damage > 0.0 {
            let mut module_hits: Vec<(Entity, f32)> = Vec::new();
            for child in children.iter() {
                if let Ok((_, mod_transform, owned)) = module_query.get(child) {
                    if owned.root == event.target {
                        let dist = mod_transform.translation.truncate().distance(impact_pos);
                        module_hits.push((child, dist));
                    }
                }
            }
            module_hits.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            for (mod_entity, _dist) in &module_hits {
                if remaining_damage <= 0.0 {
                    break;
                }
                if let Ok((mut module, mod_transform, _)) = module_query.get_mut(*mod_entity) {
                    let damage_to_apply = remaining_damage.min(module.health);
                    module.health -= damage_to_apply;
                    remaining_damage -= damage_to_apply;

                    if module.health <= 0.0 {
                        module.is_active = false;
                    }

                    spawn_floating_damage(
                        &mut commands,
                        mod_transform.translation.truncate(),
                        damage_to_apply,
                        Color::srgb(1.0, 0.6, 0.2),
                    );
                }
            }
        }

        // Recalculate hull integrity
        let mut total_hull_hp = 0.0_f32;
        let mut max_hull_hp = 0.0_f32;
        for child in children.iter() {
            if let Ok((hull, _, owned)) = hull_query.get(child) {
                if owned.root == event.target {
                    total_hull_hp += hull.health;
                    max_hull_hp += hull.max_health;
                }
            }
        }
        state.hull_integrity = if max_hull_hp > 0.0 {
            total_hull_hp / max_hull_hp
        } else {
            0.0
        };

        // Check destruction
        if state.hull_integrity <= 0.0 && !state.is_destroyed {
            state.is_destroyed = true;
            if let Ok((ai_transform, ai_ship_type, bounty)) = ai_ship_query.get(event.target) {
                destroyed_events.write(AiShipDestroyed {
                    entity: event.target,
                    ship_type: *ai_ship_type,
                    position: ai_transform.translation.truncate(),
                    bounty_id: bounty.map(|b| b.0),
                    cause: ShipDeathCause::Gutted,
                });
            }
        }
    }
}

/// Fraction of a subsystem class still alive below which it counts as shot
/// out. Not zero: the last gun on a battleship shouldn't keep a 160-tile
/// hulk "in the fight" for another three minutes of grinding.
const CRIPPLE_THRESHOLD: f32 = 0.25;

/// Seconds between a reactor breach and detonation. The ship is at its most
/// dangerous here — see tick_reactor_meltdown.
pub const MELTDOWN_SECONDS: f32 = 8.0;

/// Counts a class of modules on one AI ship as (alive, total). Destroyed
/// modules keep their entity (they're marked, not despawned), so the totals
/// are the ship's original loadout without storing anything at spawn.
fn subsystem_tally(
    root: Entity,
    children: &Children,
    module_query: &Query<(&Module, &OwnedByAiShip)>,
    matches_class: impl Fn(&Module) -> bool,
) -> (u32, u32) {
    let mut alive = 0;
    let mut total = 0;
    for child in children.iter() {
        let Ok((module, owned)) = module_query.get(child) else { continue };
        if owned.root != root || !matches_class(module) { continue; }
        total += 1;
        if module.health > 0.0 {
            alive += 1;
        }
    }
    (alive, total)
}

fn is_weapon_module(module: &Module) -> bool {
    ModuleCategory::Weapons.module_types().contains(&module.module_type)
}

fn is_engine_module(module: &Module) -> bool {
    ModuleCategory::Propulsion.module_types().contains(&module.module_type)
}

/// A ship is DEFEATED when it can no longer fight, not when its last hull
/// tile is ground off. Shoot out the guns and the engines and the crew
/// strikes colors: the ship stops fighting and drifts as an intact derelict.
///
/// This is the anti-grind valve. An Iron Tide is 160 tiles x 500 HP — eighty
/// thousand hull HP, minutes of held fire — so before this the only kill
/// anyone ever went for was sniping the reactor, and everything in between
/// was a slog. Now a fight ends when you've taken the ship apart in the
/// places that matter, and the wreck you're left with is the best salvage in
/// the game (wreck.rs scores loot by how intact the hull still is).
pub fn check_ai_cripple(
    mut ai_ships: Query<
        (Entity, &Transform, &AiShipType, &Children, &mut AiShipState, Option<&BountyTarget>),
        (With<AiShip>, Without<ReactorMeltdown>),
    >,
    module_query: Query<(&Module, &OwnedByAiShip)>,
    mut destroyed_events: MessageWriter<AiShipDestroyed>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for (entity, transform, ship_type, children, mut state, bounty) in ai_ships.iter_mut() {
        if state.is_destroyed { continue; }

        let (guns_alive, guns_total) = subsystem_tally(entity, children, &module_query, is_weapon_module);
        let (engines_alive, engines_total) = subsystem_tally(entity, children, &module_query, is_engine_module);

        // An unarmed hull (GlassEye) is toothless by construction; an
        // engineless one is already adrift. Either way the ratio for a class
        // it never had must not read as "still fine".
        let toothless = guns_total == 0
            || (guns_alive as f32 / guns_total as f32) <= CRIPPLE_THRESHOLD;
        let immobile = engines_total == 0
            || (engines_alive as f32 / engines_total as f32) <= CRIPPLE_THRESHOLD;
        if !(toothless && immobile) { continue; }

        state.is_destroyed = true;
        let pos = transform.translation.truncate();
        notifications.write(ShowNotification {
            message: format!("{:?} strikes colors — derelict adrift, ripe for salvage.", ship_type),
            notification_type: NotificationType::Success,
            duration: 4.0,
        });
        destroyed_events.write(AiShipDestroyed {
            entity,
            ship_type: *ship_type,
            position: pos,
            bounty_id: bounty.map(|b| b.0),
            cause: ShipDeathCause::Struck,
        });
    }
}

/// A breached reactor no longer kills the ship on the spot. Popping the core
/// used to be an instant win, which made every fight a race to dig one hole
/// — now it starts a countdown, and the dying ship spends it berserk (see
/// tick_reactor_meltdown). Ships with a second live reactor just lose that
/// one: redundancy is why a Dreadnought takes longer than a raider, instead
/// of raw hit points.
pub fn check_ai_reactor_destruction(
    mut commands: Commands,
    destroyed_reactors: Query<(&Module, &OwnedByAiShip), Added<DestroyedModule>>,
    ai_ships: Query<(&AiShipState, &Children, Option<&ReactorMeltdown>)>,
    module_query: Query<(&Module, &OwnedByAiShip)>,
    mut shield_query: Query<&mut crate::combat::shields::ShipShield>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for (module, owned) in destroyed_reactors.iter() {
        if !is_reactor(module.module_type) { continue; }

        let Ok((state, children, melting)) = ai_ships.get(owned.root) else { continue };
        if state.is_destroyed || melting.is_some() { continue; }

        // Any other reactor still running? Then the ship just browns out a
        // little and fights on.
        let (reactors_alive, _) = subsystem_tally(
            owned.root, children, &module_query, |m| is_reactor(m.module_type),
        );
        if reactors_alive > 0 { continue; }

        // Containment is gone: the bubble drops now, the ship goes later.
        if let Ok(mut shield) = shield_query.get_mut(owned.root) {
            shield.enabled = false;
            shield.current = 0.0;
        }
        commands.entity(owned.root).try_insert(ReactorMeltdown { remaining: MELTDOWN_SECONDS });
        notifications.write(ShowNotification {
            message: format!("REACTOR BREACH — detonation in {:.0}s. Get clear.", MELTDOWN_SECONDS),
            notification_type: NotificationType::Danger,
            duration: 4.0,
        });
    }
}

fn is_reactor(module_type: ModuleType) -> bool {
    matches!(module_type,
        ModuleType::SmallReactor | ModuleType::StandardReactor
        | ModuleType::LargeReactor | ModuleType::FusionReactor
    )
}

/// Runs the breach countdown. The ship keeps its shield down and burns what's
/// left in the capacitors: it fights to the last second rather than sitting
/// there waiting to be a kill notification, so the payoff for cracking a core
/// is a scramble to get clear, not a free win.
pub fn tick_reactor_meltdown(
    mut commands: Commands,
    time: Res<Time>,
    mut ai_ships: Query<(
        Entity,
        &Transform,
        &AiShipType,
        &mut AiShipState,
        &mut ReactorMeltdown,
        Option<&BountyTarget>,
    )>,
    mut shield_query: Query<&mut crate::combat::shields::ShipShield>,
    mut destroyed_events: MessageWriter<AiShipDestroyed>,
) {
    let dt = time.delta_secs();
    for (entity, transform, ship_type, mut state, mut meltdown, bounty) in ai_ships.iter_mut() {
        if state.is_destroyed {
            commands.entity(entity).remove::<ReactorMeltdown>();
            continue;
        }

        // Hold the bubble down for the whole countdown — update_shields would
        // otherwise recharge it after a few quiet seconds.
        if let Ok(mut shield) = shield_query.get_mut(entity) {
            shield.enabled = false;
            shield.current = 0.0;
        }

        let was = meltdown.remaining;
        meltdown.remaining -= dt;
        let pos = transform.translation.truncate();

        // One flare per remaining second, brightening as it goes.
        if was.ceil() != meltdown.remaining.ceil() && meltdown.remaining > 0.0 {
            let heat = 1.0 - (meltdown.remaining / MELTDOWN_SECONDS);
            spawn_hit_effect(
                &mut commands,
                pos,
                Color::srgb(1.0, 0.6 - 0.4 * heat, 0.1),
                40.0 + 60.0 * heat,
            );
        }

        if meltdown.remaining > 0.0 { continue; }

        state.is_destroyed = true;
        state.hull_integrity = 0.0;
        commands.entity(entity).remove::<ReactorMeltdown>();
        spawn_hit_effect(&mut commands, pos, Color::srgb(1.0, 0.85, 0.4), 260.0);
        spawn_hit_effect(&mut commands, pos, Color::srgb(1.0, 0.5, 0.1), 180.0);
        destroyed_events.write(AiShipDestroyed {
            entity,
            ship_type: *ship_type,
            position: pos,
            bounty_id: bounty.map(|b| b.0),
            cause: ShipDeathCause::Meltdown,
        });
    }
}

/// Blast radius of a cooking-off magazine or fuel bunker, in world units
/// (blocks are 66 across, so this reaches the ring of blocks around it).
const COOKOFF_RADIUS: f32 = 100.0;

/// AMMO COOK-OFF / FUEL FIRE on enemy ships. Player ships have had this since
/// chain_reactions.rs, but AI ships never did — so the one shot that should
/// end a fight outright, straight into the magazine, did exactly as much as a
/// shot into a corridor. Now a destroyed ammo bay or fuel tank takes its
/// neighbours with it, which is what makes a lucky (or aimed) hit on a
/// magazine the fastest kill in the game.
pub fn ai_chain_reactions(
    mut commands: Commands,
    // ParamSet because the "what just blew up" query and the "damage the
    // neighbours" query both touch Module — one shared, one mutable.
    mut modules: ParamSet<(
        Query<(&Module, &GlobalTransform, &OwnedByAiShip), Added<DestroyedModule>>,
        Query<(&mut Module, &GlobalTransform, &OwnedByAiShip), Without<HullSegment>>,
    )>,
    mut hull_query: Query<(&mut HullSegment, &GlobalTransform, &OwnedByAiShip), Without<Module>>,
    mut damage_events: MessageWriter<AiShipDamaged>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let blasts: Vec<(Entity, Vec2, f32)> = modules.p0().iter()
        .filter_map(|(module, gt, owned)| {
            let blast = match module.module_type {
                ModuleType::WarheadBay => 90.0,
                ModuleType::AmmoFeedUnit | ModuleType::AmmoBay => 55.0,
                ModuleType::FuelTank | ModuleType::FuelProcessor => 40.0,
                _ => return None,
            };
            Some((owned.root, gt.translation().truncate(), blast))
        })
        .collect();

    for (root, pos, blast) in blasts {
        for (mut hull, hull_gt, hull_owned) in hull_query.iter_mut() {
            if hull_owned.root != root { continue; }
            if hull_gt.translation().truncate().distance(pos) > COOKOFF_RADIUS { continue; }
            hull.health = (hull.health - blast).max(0.0);
        }
        for (mut neighbour, mod_gt, mod_owned) in modules.p1().iter_mut() {
            if mod_owned.root != root { continue; }
            if neighbour.health <= 0.0 { continue; }
            if mod_gt.translation().truncate().distance(pos) > COOKOFF_RADIUS { continue; }
            neighbour.health = (neighbour.health - blast).max(0.0);
        }

        spawn_hit_effect(&mut commands, pos, Color::srgb(1.0, 0.7, 0.2), 140.0);
        notifications.write(ShowNotification {
            message: "Magazine cook-off aboard the target!".into(),
            notification_type: NotificationType::Success,
            duration: 2.5,
        });

        // Bookkeeping only — the damage above is already applied; this makes
        // the ship recompute its hull integrity (and remember it was hit).
        damage_events.write(AiShipDamaged {
            target: root,
            source: DamageSource::Explosion,
            amount: 0.0,
            position: Some(pos),
            direction: None,
            attacker: None,
        });
    }
}

/// Crews that keep watching their rounds skip off you eventually stop firing
/// the round that skips.
///
/// This is the counterplay to sloped armour. Without it, a player who learns
/// to angle becomes steadily harder to hurt with no answer from the other
/// side — fights get EASIER the better you understand the system, which is
/// backwards. An enemy that changes what it loads is also the most legible
/// form of AI thinking available here: you see the tracers change colour and
/// then start biting.
///
/// The replacement is picked to defeat geometry rather than to hit harder.
/// HESH spalls through armour it never breaches; the exotics ignore impact
/// angle outright. Which one a faction reaches for is a characterisation:
/// scrappers improvise a squash head, deep-zone lords have something worse.
pub fn ai_adapt_ammo(
    // Entity comes from the SAME query. Zipping a separate Query<Entity> against
    // this one would assume both iterate in the same archetype order, which
    // Bevy does not promise — and getting it wrong silently attributes one
    // ship's gunnery record to another.
    mut ships: Query<(Entity, &AiShipType, &AiShipTarget, &Children, &mut AiGunneryLog)>,
    mut weapons: Query<&mut crate::building::customization::tuning::SelectedAmmo, With<Weapon>>,
    mut notifications: MessageWriter<ShowNotification>,
    mut last_target: Local<std::collections::HashMap<Entity, Option<Entity>>>,
) {
    for (entity, faction, target, children, mut log) in ships.iter_mut() {
        // Studying a new ship starts the lesson over — what worked against the
        // last one says nothing about this one's armour.
        let previous = last_target.entry(entity).or_insert(None);
        if *previous != target.entity {
            *previous = target.entity;
            log.ricochets = 0;
            log.switched = false;
        }

        if log.switched || log.ricochets < RICOCHETS_BEFORE_SWITCH {
            continue;
        }
        log.switched = true;

        let answer = angle_proof_round(*faction);
        let mut changed = 0;
        for child in children.iter() {
            if let Ok(mut loaded) = weapons.get_mut(child) {
                loaded.0 = answer;
                changed += 1;
            }
        }
        if changed > 0 {
            notifications.write(ShowNotification {
                message: format!("Enemy switching ammunition — {}", answer.name()),
                notification_type: NotificationType::Warning,
                duration: 3.0,
            });
        }
    }
}

/// What a faction reaches for when angles are beating it. Characterisation as
/// much as balance: a scrap crew improvises, a deep-zone lord doesn't have to.
fn angle_proof_round(faction: AiShipType) -> KineticAmmoType {
    use AiShipType::*;
    match faction {
        // Nothing exotic aboard — but a squash head doesn't need to get
        // through, and that's the whole trick.
        RustSwarm | Drowned | Leviathan => KineticAmmoType::HESH,
        // Disciplined gunnery: a dart barely deflects at any angle.
        Blackwater | IronTide => KineticAmmoType::APFSDS,
        // Bio-organic and deep-zone: they have stranger things loaded.
        AbyssalCult | PressureKing | GlassEye => KineticAmmoType::HESH,
        // Gravity does not care what angle you hit at.
        Dreadnought | VoidTitan => KineticAmmoType::Singularity,
    }
}

#[cfg(test)]
mod gunnery_tests {
    use super::*;
    use crate::combat::impact::{obliquity, Obliquity};
    use crate::building::Block;
    use crate::components::HullMaterial;
    use bevy::math::{IVec2, Vec2};

    const ALL_FACTIONS: [AiShipType; 10] = [
        AiShipType::Leviathan, AiShipType::AbyssalCult, AiShipType::Drowned,
        AiShipType::PressureKing, AiShipType::GlassEye, AiShipType::IronTide,
        AiShipType::Blackwater, AiShipType::RustSwarm, AiShipType::Dreadnought,
        AiShipType::VoidTitan,
    ];

    /// The point of switching is to stop skipping. Every faction's answer has
    /// to actually beat the geometry that beat it — otherwise the crew "adapts"
    /// into the same failure and the player still can't be touched.
    #[test]
    fn every_factions_answer_survives_the_angle_that_beat_it() {
        // The plate that caused the problem, hit at a glancing 78 degrees.
        let plate = Block::hull(IVec2::ZERO, HullMaterial::Composite);
        let face = Vec2::NEG_X;
        let incoming = Vec2::from_angle(78f32.to_radians());

        // Baseline: unspecialised fire skips off this, which is why the crew
        // is reconsidering in the first place.
        assert!(obliquity(face, incoming, &plate, None, 1.0).ricochet);

        for faction in ALL_FACTIONS {
            let answer = angle_proof_round(faction);
            let o = obliquity(face, incoming, &plate, Some(answer), 1.0);
            let spall = crate::combat::ammo_types::spall(Some(answer));
            // Either it doesn't deflect, or it doesn't NEED to get through.
            assert!(
                !o.ricochet || spall.through_solid,
                "{faction:?} switches to {answer:?}, which still skips and can't spall through"
            );
        }
    }

    /// A switch has to be a change. Reloading the round that was already
    /// bouncing is the crew learning nothing.
    #[test]
    fn the_answer_is_never_the_default_round() {
        for faction in ALL_FACTIONS {
            assert_ne!(angle_proof_round(faction), KineticAmmoType::AP);
        }
    }

    /// Head-on, the switch must not be a straight downgrade — the crew is
    /// solving an angle problem, not throwing damage away.
    #[test]
    fn the_answer_still_works_square_on() {
        let plate = Block::hull(IVec2::ZERO, HullMaterial::Composite);
        for faction in ALL_FACTIONS {
            let answer = angle_proof_round(faction);
            let o = obliquity(Vec2::NEG_X, Vec2::X, &plate, Some(answer), 1.0);
            assert!(!o.ricochet, "{faction:?}'s {answer:?} shouldn't bounce dead-on");
            assert_eq!(o, Obliquity::HEAD_ON.clone(), "{faction:?}: square-on is square-on");
        }
    }
}

/// Tell the player they won, and how.
///
/// AiShipDestroyed reached the audio system and nothing else, so a kill — the
/// payoff of the whole fight — produced no line, no readout, nothing but the
/// target going quiet. The CAUSE is the interesting part and it was already
/// being computed and thrown away: check_ai_cripple decides whether a crew
/// struck colours, the reactor let go, or you ground the hull to nothing, and
/// each leaves a different wreck to pick over.
pub fn announce_kills(
    mut destroyed: MessageReader<AiShipDestroyed>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for event in destroyed.read() {
        let (line, kind) = match event.cause {
            // The cleanest kill and the best salvage — worth naming as a win,
            // not just as a death.
            ShipDeathCause::Struck => (
                format!("{} struck colors — intact derelict", faction_name(event.ship_type)),
                NotificationType::Success,
            ),
            ShipDeathCause::Meltdown => (
                format!("{} reactor breach — she's gone", faction_name(event.ship_type)),
                NotificationType::Warning,
            ),
            ShipDeathCause::Gutted => (
                format!("{} gutted — little left to salvage", faction_name(event.ship_type)),
                NotificationType::Info,
            ),
        };
        notifications.write(ShowNotification {
            message: line,
            notification_type: kind,
            duration: 3.5,
        });
    }
}

fn faction_name(ship_type: AiShipType) -> &'static str {
    use AiShipType::*;
    match ship_type {
        Leviathan => "Leviathan hauler",
        AbyssalCult => "Cult hybrid",
        Drowned => "Drowned hulk",
        PressureKing => "Pressure King",
        GlassEye => "Glass Eye",
        IronTide => "Iron Tide",
        Blackwater => "Blackwater merc",
        RustSwarm => "Rust Swarm raider",
        Dreadnought => "Dreadnought",
        VoidTitan => "Void Titan",
    }
}
