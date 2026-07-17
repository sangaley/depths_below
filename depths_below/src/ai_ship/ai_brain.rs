use bevy::prelude::*;

use crate::components::*;
use crate::ai_ship::components::*;
use crate::spatial::CreatureGrid;

/// Priority-scorer AI decision system for AI ships.
/// Each faction has its own unique decision tree.
/// Runs on a 0.25s tick.
///
/// TARGET SELECTION: alongside each faction's own trigger logic (unchanged),
/// every ship also gets a faction-agnostic `best_target` computed once per
/// tick — the highest (size+firepower)/distance score among the player and
/// every other living AI ship within this ship's own engage_range. This is
/// what lets a ship pull off a nearby tiny raider onto a much bigger,
/// farther threat instead of just fixating on whatever's closest.
///
/// Only the "attack anything/anyone in range" arms — the ones already
/// worded that way in each faction's own flavor text (Drowned, Iron Tide's
/// generic engage, Blackwater's tactical engage, Rust Swarm, Dreadnought,
/// Void Titan) — actually USE best_target for their fire-at-this position.
/// Player-specific narrative behaviors (Abyssal Cult protecting creatures
/// FROM the player, Pressure King ramming intruders out of their depth
/// zone, Glass Eye's player-shadowing, Leviathan's flee-only) keep
/// targeting the player exactly as before — generalizing those would blur
/// what makes each faction distinct. "Under fire → retaliate" arms also
/// stay player-only everywhere: there's no per-hit attacker tracking yet
/// to know WHO actually shot at a ship, only that IT WAS hit.
pub fn ai_brain_system(
    time: Res<Time>,
    mut ai_ships: Query<(
        Entity,
        &Transform,
        &AiShipType,
        &mut AiShipState,
        &mut AiShipBehavior,
        &mut AiShipNav,
        &mut AiShipDecisionTimer,
        &mut AiShipTarget,
        &Children,
    )>,
    player_ship: Query<(Entity, &Transform, &Children), With<Ship>>,
    player_weapon_query: Query<(&Weapon, &Module), Without<OwnedByAiShip>>,
    creature_query: Query<(Entity, &Transform, &Creature), Without<Ship>>,
    wreck_query: Query<(Entity, &Transform, &AiShipWreck)>,
    weapon_query: Query<(&Weapon, &Module, &OwnedByAiShip), Without<Engine>>,
    creature_grid: Res<CreatureGrid>,
) {
    // --- Pre-pass: snapshot every living ship's position + "threat value"
    // (block count + 2x active weapon damage — cheap proxies for size and
    // firepower) BEFORE the mutable per-ship loop below. Two sequential
    // .iter() calls on the same query is fine in Bevy; it's the same
    // pattern as any read-then-mutate pass, just split across two loops.
    fn threat_value(children: &Children, weapon_dmg: f32) -> f32 {
        children.iter().count() as f32 + weapon_dmg * 2.0
    }

    let mut ai_snapshot: Vec<(Entity, Vec2, f32)> = Vec::new();
    for (entity, transform, _, state, _, _, _, _, children) in ai_ships.iter() {
        if state.is_destroyed { continue; }
        let weapon_dmg: f32 = children.iter()
            .filter_map(|c| weapon_query.get(c).ok())
            .filter(|(_, module, _)| module.is_active && module.health > 0.0)
            .map(|(w, _, _)| w.damage)
            .sum();
        ai_snapshot.push((entity, transform.translation.truncate(), threat_value(children, weapon_dmg)));
    }

    let player_snapshot: Option<(Entity, Vec2, f32)> = player_ship.iter().next().map(|(e, t, children)| {
        let weapon_dmg: f32 = children.iter()
            .filter_map(|c| player_weapon_query.get(c).ok())
            .filter(|(_, module)| module.is_active && module.health > 0.0)
            .map(|(w, _)| w.damage)
            .sum();
        (e, t.translation.truncate(), threat_value(children, weapon_dmg))
    });

    for (entity, transform, ship_type, mut state, mut behavior, mut nav, mut timer, mut ai_target, children) in ai_ships.iter_mut() {
        timer.timer.tick(time.delta());
        if !timer.timer.just_finished() {
            continue;
        }

        state.last_hit_timer += 0.25;

        if state.is_destroyed {
            *behavior = AiShipBehavior::Dead;
            continue;
        }

        let pos = transform.translation.truncate();
        let under_fire = state.last_hit_timer < 5.0;
        let hull_pct = state.hull_integrity;
        let fuel_pct = state.fuel / state.max_fuel.max(1.0);
        let depth = (-pos.y / 10.0).max(0.0);

        // Detection/engagement range for the "should I start fighting" checks
        // below — must stay comfortably above movement.rs's standoff distance
        // (now also weapon-based, at 0.85x max range) or a long-range-armed
        // ship would want to hold a standoff its own brain never lets it
        // reach: ai_brain decides "not in range, don't engage" using a
        // smaller number than movement.rs's "hold at my weapon's range" once
        // it IS engaging, so the ship would never start fighting from its
        // actual weapon range in the first place. 1.05x keeps a safety
        // margin above the 0.85x standoff so the two systems don't fight
        // each other into an engage/disengage oscillation.
        let max_weapon_range = children.iter()
            .filter_map(|c| weapon_query.get(c).ok())
            .filter(|(_, module, _)| module.is_active && module.health > 0.0)
            .map(|(weapon, _, _)| weapon.range)
            .fold(0.0_f32, f32::max);
        let engage_range = if max_weapon_range > 0.0 { max_weapon_range * 1.05 } else { 4400.0 };

        // Perception
        let player_info = player_snapshot.map(|(e, p, _)| (e, p, pos.distance(p)));

        // Faction-agnostic target pick: highest value/distance among the
        // player + every OTHER living AI ship within THIS ship's own
        // engage_range. Only consulted by the "attack anything in range"
        // arms below — see the function doc comment for which ones.
        //
        // STICKY: the target this ship already has (ai_target.entity, from
        // last tick) gets its score boosted before comparing. Recomputing
        // a fresh "best" from scratch every 0.25s meant that in any cluster
        // of 3+ ships, near-tied scores flipped the winner tick to tick —
        // nav.destination jumped to a totally different ship's position
        // each time, and the standoff-orbit logic in movement.rs whipped
        // the heading around chasing it (the "moving all over the place"
        // playtest report). A held target now stays locked until something
        // clearly outclasses it (40%+ better score) or it dies/leaves range.
        const TARGET_STICKINESS: f32 = 1.4;
        let held_target = ai_target.entity;
        let best_target: Option<(Entity, Vec2)> = player_snapshot.iter()
            .map(|&(e, p, v)| (e, p, v))
            .chain(ai_snapshot.iter().filter(|(e, _, _)| *e != entity).map(|&(e, p, v)| (e, p, v)))
            .filter(|(_, p, _)| pos.distance(*p) < engage_range)
            .map(|(e, p, v)| (e, p, if Some(e) == held_target { v * TARGET_STICKINESS } else { v }))
            .max_by(|(_, pa, va), (_, pb, vb)| {
                let score_a = va / pos.distance(*pa).max(200.0);
                let score_b = vb / pos.distance(*pb).max(200.0);
                score_a.partial_cmp(&score_b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(e, p, _)| (e, p));

        // Spatial-grid narrowed: only the creature(s) in nearby cells are distance-checked,
        // instead of every creature in the world, every 0.25s, for every AI ship.
        let nearest_creature = creature_grid.0.nearest(pos, 600.0, None)
            .and_then(|(e, d)| creature_query.get(e).ok().map(|(_, t, c)| {
                (e, d, t.translation.truncate(), c.creature_type)
            }));

        // Only the Rust Swarm scorer consumes this — wide range so
        // scavenger waves spawned at the edge of the area actually smell
        // the carcass and burn inward; picked-clean wrecks don't count.
        let nearest_wreck = wreck_query.iter()
            .filter(|(_, _, aw)| aw.loot_remaining > 0)
            .map(|(e, t, _)| {
                let d = pos.distance(t.translation.truncate());
                (e, d, t.translation.truncate())
            })
            .filter(|(_, d, _)| *d < 3000.0)
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        struct ScoredAction {
            score: f32,
            behavior: AiShipBehavior,
            destination: Option<Vec2>,
            /// Who/where to actually FIRE at — distinct from `destination`,
            /// which is sometimes an offset (Blackwater's flank point,
            /// Pressure King's ram-from-above point), not the target itself.
            target: Option<(Entity, Vec2)>,
        }

        let mut actions: Vec<ScoredAction> = Vec::with_capacity(12);

        // ====================================================================
        // Per-faction unique decision trees
        // ====================================================================
        match *ship_type {
            // ----------------------------------------------------------------
            // LEVIATHAN RIDERS: Hunt creatures, avoid combat, flee when hurt
            // Prioritize: find creatures > capture > flee > patrol
            // ----------------------------------------------------------------
            AiShipType::Leviathan => {
                // Critical flee
                if hull_pct < 0.25 || fuel_pct < 0.1 {
                    actions.push(ScoredAction {
                        score: 100.0,
                        behavior: AiShipBehavior::Fleeing,
                        destination: Some(pos + Vec2::new(0.0, 600.0)),
                        target: None,
                    });
                }

                // Flee from player if under fire (not a fighter)
                if under_fire {
                    if let Some((_, p_pos, _)) = player_info {
                        let away = (pos - p_pos).normalize_or_zero();
                        actions.push(ScoredAction {
                            score: 85.0,
                            behavior: AiShipBehavior::Fleeing,
                            destination: Some(pos + away * 500.0),
                            target: None,
                        });
                    }
                }

                // Chase creatures to capture them (primary purpose)
                if let Some((_, dist, c_pos, _)) = nearest_creature {
                    if dist < 500.0 {
                        actions.push(ScoredAction {
                            score: 75.0,
                            behavior: AiShipBehavior::Salvaging, // "salvaging" = capturing
                            destination: Some(c_pos),
                            target: None,
                        });
                    }
                }

                // Patrol hunting grounds
                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 40.0,
                        behavior: AiShipBehavior::Patrolling,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                        target: None,
                    });
                }
            }

            // ----------------------------------------------------------------
            // ABYSSAL CULT: Protect creatures, attack creature-killers,
            // kamikaze ram when below 20% HP, patrol sacred waters
            // ----------------------------------------------------------------
            AiShipType::AbyssalCult => {
                // KAMIKAZE when critically damaged - ram nearest target
                // Detection range must exceed the standoff (movement.rs) or
                // the ship engages, immediately backs off past the trigger
                // distance, drops out of Engaging, drifts back in, and
                // repeats — a fast oscillation that looks like glitching.
                // engage_range is weapon-based now (1.05x max weapon range)
                // so this holds regardless of loadout.
                if hull_pct < 0.20 {
                    if let Some((_, p_pos, dist)) = player_info {
                        if dist < engage_range {
                            actions.push(ScoredAction {
                                score: 100.0,
                                behavior: AiShipBehavior::Engaging,
                                destination: Some(p_pos), // ram into player
                                target: player_info.map(|(pe, pp, _)| (pe, pp)),
                            });
                        }
                    }
                }

                // Attack player if they're near creatures (protecting sea life)
                if let Some((_, c_dist, _c_pos, _)) = nearest_creature {
                    if let Some((_, p_pos, p_dist)) = player_info {
                        // Player near a creature = threat to sacred life
                        if p_dist < engage_range && c_dist < 400.0 {
                            actions.push(ScoredAction {
                                score: 88.0,
                                behavior: AiShipBehavior::Engaging,
                                destination: Some(p_pos),
                                target: player_info.map(|(pe, pp, _)| (pe, pp)),
                            });
                        }
                    }
                }

                // Under fire → fight back (fanatics don't flee easily)
                if under_fire {
                    if let Some((_, p_pos, _)) = player_info {
                        actions.push(ScoredAction {
                            score: 82.0,
                            behavior: AiShipBehavior::Engaging,
                            destination: Some(p_pos),
                            target: player_info.map(|(pe, pp, _)| (pe, pp)),
                        });
                    }
                }

                // Escort creatures (follow them around)
                if let Some((_, _, c_pos, _)) = nearest_creature {
                    actions.push(ScoredAction {
                        score: 60.0,
                        behavior: AiShipBehavior::FollowingTradeRoute,
                        destination: Some(c_pos + Vec2::new(100.0, 0.0)),
                        target: None,
                    });
                }

                // Patrol sacred waters
                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 35.0,
                        behavior: AiShipBehavior::Patrolling,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                        target: None,
                    });
                }
            }

            // ----------------------------------------------------------------
            // THE DROWNED: Mindless ghost ships. Attack everything in range.
            // No fleeing, no self-preservation. Erratic movement.
            // ----------------------------------------------------------------
            AiShipType::Drowned => {
                // Never flee - already dead, can't die again (narratively)

                // Attack anything nearby (detection must exceed the
                // standoff distance — see AbyssalCult kamikaze comment).
                // "Anything" is now literal: best_target, not just player.
                if let Some((_, t_pos)) = best_target {
                    if pos.distance(t_pos) < engage_range {
                        actions.push(ScoredAction {
                            score: 80.0,
                            behavior: AiShipBehavior::Engaging,
                            destination: Some(t_pos),
                            target: best_target,
                        });
                    }
                }

                // Attack creatures too (mindless aggression). No ship-vs-
                // creature targeting system exists yet, so this still
                // fires at the player if one's around — same as before.
                if let Some((_, dist, c_pos, _)) = nearest_creature {
                    if dist < 300.0 {
                        actions.push(ScoredAction {
                            score: 70.0,
                            behavior: AiShipBehavior::Engaging,
                            destination: Some(c_pos),
                            target: player_info.map(|(pe, pp, _)| (pe, pp)),
                        });
                    }
                }

                // Wander aimlessly - repeat old patrol routes (ghost behavior)
                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 50.0,
                        behavior: AiShipBehavior::Patrolling,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                        target: None,
                    });
                }

                // Just drift if nothing else
                actions.push(ScoredAction {
                    score: 20.0,
                    behavior: AiShipBehavior::Idle,
                    destination: Some(pos + Vec2::new(50.0, -30.0)),
                    target: None,
                });
            }

            // ----------------------------------------------------------------
            // PRESSURE KINGS: Deep-zone gatekeepers.
            // Attack anyone above 800m in their territory.
            // Ram intruders upward. Don't flee. Ignore shallow threats.
            // ----------------------------------------------------------------
            AiShipType::PressureKing => {
                // Only active in deep void - idle near station
                if depth < 600.0 {
                    actions.push(ScoredAction {
                        score: 90.0,
                        behavior: AiShipBehavior::Patrolling,
                        destination: Some(pos + Vec2::new(0.0, -500.0)), // go deeper
                        target: None,
                    });
                } else {
                    // RAM player upward - primary behavior (detection must
                    // exceed the standoff — see AbyssalCult comment)
                    if let Some((_, p_pos, dist)) = player_info {
                        if dist < engage_range {
                            // Position above the player to push them up
                            let ram_pos = Vec2::new(p_pos.x, p_pos.y + 150.0);
                            actions.push(ScoredAction {
                                score: 90.0,
                                behavior: AiShipBehavior::Engaging,
                                destination: Some(ram_pos),
                                target: player_info.map(|(pe, pp, _)| (pe, pp)),
                            });
                        }
                    }

                    // Attack anyone under fire (never retreat in deep zone)
                    if under_fire {
                        if let Some((_, p_pos, _)) = player_info {
                            actions.push(ScoredAction {
                                score: 95.0,
                                behavior: AiShipBehavior::Engaging,
                                destination: Some(p_pos),
                                target: player_info.map(|(pe, pp, _)| (pe, pp)),
                            });
                        }
                    }

                    // Only retreat if almost destroyed
                    if hull_pct < 0.10 {
                        actions.push(ScoredAction {
                            score: 98.0,
                            behavior: AiShipBehavior::Fleeing,
                            destination: Some(pos + Vec2::new(0.0, -800.0)), // flee DEEPER
                            target: None,
                        });
                    }
                }

                // Deep patrol
                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 40.0,
                        behavior: AiShipBehavior::Patrolling,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                        target: None,
                    });
                }
            }

            // ----------------------------------------------------------------
            // GLASS EYE: Silent watchers. NEVER attack. Always flee.
            // Follow player from safe distance. Fastest flee speed.
            // ----------------------------------------------------------------
            AiShipType::GlassEye => {
                // If under fire, flee at maximum speed
                if under_fire {
                    let flee_dest = if let Some((_, p_pos, _)) = player_info {
                        let away = (pos - p_pos).normalize_or_zero();
                        pos + away * 1000.0 // flee very far
                    } else {
                        pos + Vec2::new(0.0, 800.0)
                    };
                    actions.push(ScoredAction {
                        score: 100.0,
                        behavior: AiShipBehavior::Fleeing,
                        destination: Some(flee_dest),
                        target: None,
                    });
                }

                // Shadow the player from 500-800u distance (surveillance)
                if let Some((_, p_pos, dist)) = player_info {
                    if dist < 400.0 {
                        // Too close - back off
                        let away = (pos - p_pos).normalize_or_zero();
                        actions.push(ScoredAction {
                            score: 80.0,
                            behavior: AiShipBehavior::Fleeing,
                            destination: Some(pos + away * 300.0),
                            target: None,
                        });
                    } else if dist < 900.0 {
                        // Good shadowing distance - maintain
                        let toward = (p_pos - pos).normalize_or_zero();
                        let shadow_pos = p_pos - toward * 650.0;
                        actions.push(ScoredAction {
                            score: 70.0,
                            behavior: AiShipBehavior::FollowingTradeRoute,
                            destination: Some(shadow_pos),
                            target: None,
                        });
                    }
                }

                // Roam scanning routes
                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 45.0,
                        behavior: AiShipBehavior::FollowingTradeRoute,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                        target: None,
                    });
                }
            }

            // ----------------------------------------------------------------
            // IRON TIDE: Heavy battleship. Engage everything hostile in range.
            // Slow to maneuver. Never flees unless nearly destroyed.
            // Maximum aggression at close range.
            // ----------------------------------------------------------------
            AiShipType::IronTide => {
                // Only retreat at extreme damage
                if hull_pct < 0.10 && fuel_pct < 0.15 {
                    actions.push(ScoredAction {
                        score: 100.0,
                        behavior: AiShipBehavior::Fleeing,
                        destination: Some(pos + Vec2::new(0.0, 500.0)),
                        target: None,
                    });
                }

                // Under fire → immediately engage (battle-hardened). No
                // attacker tracking exists, so retaliate against the
                // biggest threat in range rather than assuming it's the
                // player — otherwise this outscores the generic engage arm
                // below every time the PLAYER is the one shooting, and a
                // battleship never gets the chance to turn on something
                // else even when it's a much bigger threat.
                if under_fire {
                    if let Some((_, t_pos)) = best_target {
                        actions.push(ScoredAction {
                            score: 95.0,
                            behavior: AiShipBehavior::Engaging,
                            destination: Some(t_pos),
                            target: best_target,
                        });
                    }
                }

                // Engage ANY target in weapon range (long range weapons) —
                // best_target makes "any" literal: a farther battleship can
                // now outweigh a closer but trivial target.
                if let Some((_, t_pos)) = best_target {
                    if pos.distance(t_pos) < engage_range {
                        actions.push(ScoredAction {
                            score: 85.0,
                            behavior: AiShipBehavior::Engaging,
                            destination: Some(t_pos),
                            target: best_target,
                        });
                    }
                }

                // Engage large creatures too (shows dominance). No ship-vs-
                // creature targeting system exists yet, so this still fires
                // at the player if one's around — same as before.
                if let Some((_, dist, c_pos, c_type)) = nearest_creature {
                    let is_large = matches!(c_type, CreatureType::Leviathan);
                    if dist < 400.0 && is_large {
                        actions.push(ScoredAction {
                            score: 75.0,
                            behavior: AiShipBehavior::Engaging,
                            destination: Some(c_pos),
                            target: player_info.map(|(pe, pp, _)| (pe, pp)),
                        });
                    }
                }

                // Slow patrol - battleship doesn't rush
                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 40.0,
                        behavior: AiShipBehavior::Patrolling,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                        target: None,
                    });
                }
            }

            // ----------------------------------------------------------------
            // BLACKWATER PMC: Tactical mercs. Hunt bounties (player if hostile).
            // Flank targets. Coordinate. Disengage when outmatched.
            // ----------------------------------------------------------------
            AiShipType::Blackwater => {
                // Tactical retreat when damaged (live to fight another day)
                if hull_pct < 0.30 {
                    let flee_dest = if let Some((_, p_pos, _)) = player_info {
                        let away = (pos - p_pos).normalize_or_zero();
                        pos + away * 600.0
                    } else {
                        pos + Vec2::new(0.0, 400.0)
                    };
                    actions.push(ScoredAction {
                        score: 90.0,
                        behavior: AiShipBehavior::Fleeing,
                        destination: Some(flee_dest),
                        target: None,
                    });
                }

                // Under fire → fight back. No attacker tracking exists, so
                // flank whatever's actually the biggest threat in range —
                // see Iron Tide's identical reasoning.
                if under_fire {
                    if let Some((t_entity, t_pos)) = best_target {
                        // FLANK - don't go straight at target, offset to the side
                        let to_target = (t_pos - pos).normalize_or_zero();
                        let flank = Vec2::new(-to_target.y, to_target.x); // perpendicular
                        let flank_pos = t_pos + flank * 200.0;
                        actions.push(ScoredAction {
                            score: 85.0,
                            behavior: AiShipBehavior::Engaging,
                            destination: Some(flank_pos),
                            target: Some((t_entity, t_pos)),
                        });
                    }
                }

                // Engage the best target at medium range (tactical
                // distance) — mercs hunt whatever bounty is worth the
                // most, not just whoever's player-shaped and nearby.
                // Flank math is unchanged, just parameterized on t_pos.
                if let Some((t_entity, t_pos)) = best_target {
                    if pos.distance(t_pos) < engage_range {
                        let to_target = (t_pos - pos).normalize_or_zero();
                        let flank = Vec2::new(-to_target.y, to_target.x);
                        let offset = if pos.x > t_pos.x { 1.0 } else { -1.0 };
                        let flank_pos = t_pos + flank * 180.0 * offset;
                        actions.push(ScoredAction {
                            score: 78.0,
                            behavior: AiShipBehavior::Engaging,
                            destination: Some(flank_pos),
                            target: Some((t_entity, t_pos)),
                        });
                    }
                }

                // Patrol routes
                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 45.0,
                        behavior: AiShipBehavior::Patrolling,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                        target: None,
                    });
                }
            }

            // ----------------------------------------------------------------
            // RUST SWARM: Aggressive junk. Attack anything nearby.
            // No self-preservation. Kamikaze when critical. Mine everything.
            // ----------------------------------------------------------------
            AiShipType::RustSwarm => {
                // KAMIKAZE when critical - charge at nearest/biggest target
                // (best_target — genuinely "nearest target" now, not just
                // whichever one happens to be the player)
                if hull_pct < 0.25 {
                    if let Some((_, t_pos)) = best_target {
                        if pos.distance(t_pos) < 600.0 {
                            actions.push(ScoredAction {
                                score: 100.0,
                                behavior: AiShipBehavior::Engaging,
                                destination: Some(t_pos), // death charge
                                target: best_target,
                            });
                        }
                    }
                }

                // Attack aggressively — swarm mentality, whatever's closest
                // and worth swarming, not specifically the player.
                if let Some((_, t_pos)) = best_target {
                    if pos.distance(t_pos) < 500.0 {
                        actions.push(ScoredAction {
                            score: 80.0,
                            behavior: AiShipBehavior::Engaging,
                            destination: Some(t_pos),
                            target: best_target,
                        });
                    }
                }

                // Also chase wrecks for scrap
                if let Some((_, _, w_pos)) = nearest_wreck {
                    actions.push(ScoredAction {
                        score: 65.0,
                        behavior: AiShipBehavior::Salvaging,
                        destination: Some(w_pos),
                        target: None,
                    });
                }

                // Erratic patrol (random, twitchy movement)
                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 35.0,
                        behavior: AiShipBehavior::Patrolling,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                        target: None,
                    });
                }

                // Idle drift
                actions.push(ScoredAction {
                    score: 15.0,
                    behavior: AiShipBehavior::Idle,
                    destination: Some(pos + Vec2::new(30.0, -20.0)),
                    target: None,
                });
            }

            // ----------------------------------------------------------------
            // DREADNOUGHT: Mega-battleship. Never retreats — no flee action
            // at all. Engages anything in an enormous detection range and
            // grinds it down with sheer weapon coverage.
            // ----------------------------------------------------------------
            AiShipType::Dreadnought => {
                // No attacker tracking exists, so retaliate against the
                // biggest threat in range — see Iron Tide's reasoning.
                if under_fire {
                    if let Some((_, t_pos)) = best_target {
                        actions.push(ScoredAction {
                            score: 95.0,
                            behavior: AiShipBehavior::Engaging,
                            destination: Some(t_pos),
                            target: best_target,
                        });
                    }
                }

                // "Engages anything in an enormous detection range" — now
                // actually anything, via best_target.
                if let Some((_, t_pos)) = best_target {
                    if pos.distance(t_pos) < 9000.0 {
                        actions.push(ScoredAction {
                            score: 90.0,
                            behavior: AiShipBehavior::Engaging,
                            destination: Some(t_pos),
                            target: best_target,
                        });
                    }
                }

                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 40.0,
                        behavior: AiShipBehavior::Patrolling,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                        target: None,
                    });
                }
            }

            // ----------------------------------------------------------------
            // VOID TITAN: The apex threat. Never flees, never hesitates — if
            // you're in range, it's already coming for you.
            // ----------------------------------------------------------------
            AiShipType::VoidTitan => {
                // "If you're in range, it's already coming for you" —
                // whoever "you" is, via best_target.
                if let Some((_, t_pos)) = best_target {
                    if pos.distance(t_pos) < 12000.0 {
                        actions.push(ScoredAction {
                            score: 100.0,
                            behavior: AiShipBehavior::Engaging,
                            destination: Some(t_pos),
                            target: best_target,
                        });
                    }
                }

                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 40.0,
                        behavior: AiShipBehavior::Patrolling,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                        target: None,
                    });
                }
            }
        }

        // Default fallback for all factions
        actions.push(ScoredAction {
            score: 5.0,
            behavior: AiShipBehavior::Patrolling,
            destination: nav.waypoints.get(nav.current_waypoint).copied(),
            target: None,
        });

        // Pick highest score
        if let Some(best) = actions.iter().max_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal)) {
            *behavior = best.behavior;
            if let Some(dest) = best.destination {
                nav.destination = Some(dest);
            }
            // Combat target for ai_weapon_fire_system — separate from
            // nav.destination (see AiShipTarget's doc comment for why).
            // Non-Engaging actions carry target: None, which correctly
            // clears any stale target once the ship disengages.
            ai_target.entity = best.target.map(|(e, _)| e);
            ai_target.position = best.target.map(|(_, p)| p).unwrap_or(pos);
        }

        // Advance waypoints when near current one
        if let Some(dest) = nav.destination {
            if pos.distance(dest) < 80.0 && !nav.waypoints.is_empty() {
                nav.current_waypoint = (nav.current_waypoint + 1) % nav.waypoints.len();
                nav.destination = nav.waypoints.get(nav.current_waypoint).copied();
            }
        }
    }
}
