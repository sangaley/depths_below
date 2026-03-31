use bevy::prelude::*;

use crate::components::*;
use crate::ai_submarine::components::*;

/// Priority-scorer AI decision system for AI submarines.
/// Each faction has its own unique decision tree.
/// Runs on a 0.25s tick.
pub fn ai_brain_system(
    time: Res<Time>,
    mut ai_subs: Query<(
        Entity,
        &Transform,
        &AiSubType,
        &mut AiSubState,
        &mut AiSubBehavior,
        &mut AiSubNav,
        &mut AiSubDecisionTimer,
    )>,
    player_sub: Query<(Entity, &Transform), With<Submarine>>,
    creature_query: Query<(Entity, &Transform, &Creature), Without<Submarine>>,
    wreck_query: Query<(Entity, &Transform), With<AiSubWreck>>,
) {
    for (_entity, transform, sub_type, mut state, mut behavior, mut nav, mut timer) in ai_subs.iter_mut() {
        timer.timer.tick(time.delta());
        if !timer.timer.just_finished() {
            continue;
        }

        state.last_hit_timer += 0.25;

        if state.is_destroyed {
            *behavior = AiSubBehavior::Dead;
            continue;
        }

        let pos = transform.translation.truncate();
        let under_fire = state.last_hit_timer < 5.0;
        let hull_pct = state.hull_integrity;
        let fuel_pct = state.fuel / state.max_fuel.max(1.0);
        let depth = (-pos.y / 10.0).max(0.0);

        // Perception
        let player_info = player_sub.iter().next().map(|(e, t)| {
            let p = t.translation.truncate();
            (e, p, pos.distance(p))
        });

        let nearest_creature = creature_query.iter()
            .map(|(e, t, c)| {
                let d = pos.distance(t.translation.truncate());
                (e, d, t.translation.truncate(), c.creature_type)
            })
            .filter(|(_, d, _, _)| *d < 600.0)
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let nearest_wreck = wreck_query.iter()
            .map(|(e, t)| {
                let d = pos.distance(t.translation.truncate());
                (e, d, t.translation.truncate())
            })
            .filter(|(_, d, _)| *d < 600.0)
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        struct ScoredAction {
            score: f32,
            behavior: AiSubBehavior,
            destination: Option<Vec2>,
        }

        let mut actions: Vec<ScoredAction> = Vec::with_capacity(12);

        // ====================================================================
        // Per-faction unique decision trees
        // ====================================================================
        match *sub_type {
            // ----------------------------------------------------------------
            // LEVIATHAN RIDERS: Hunt creatures, avoid combat, flee when hurt
            // Prioritize: find creatures > capture > flee > patrol
            // ----------------------------------------------------------------
            AiSubType::Leviathan => {
                // Critical flee
                if hull_pct < 0.25 || fuel_pct < 0.1 {
                    actions.push(ScoredAction {
                        score: 100.0,
                        behavior: AiSubBehavior::Fleeing,
                        destination: Some(pos + Vec2::new(0.0, 600.0)),
                    });
                }

                // Flee from player if under fire (not a fighter)
                if under_fire {
                    if let Some((_, p_pos, _)) = player_info {
                        let away = (pos - p_pos).normalize_or_zero();
                        actions.push(ScoredAction {
                            score: 85.0,
                            behavior: AiSubBehavior::Fleeing,
                            destination: Some(pos + away * 500.0),
                        });
                    }
                }

                // Chase creatures to capture them (primary purpose)
                if let Some((_, dist, c_pos, _)) = nearest_creature {
                    if dist < 500.0 {
                        actions.push(ScoredAction {
                            score: 75.0,
                            behavior: AiSubBehavior::Salvaging, // "salvaging" = capturing
                            destination: Some(c_pos),
                        });
                    }
                }

                // Patrol hunting grounds
                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 40.0,
                        behavior: AiSubBehavior::Patrolling,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                    });
                }
            }

            // ----------------------------------------------------------------
            // ABYSSAL CULT: Protect creatures, attack creature-killers,
            // kamikaze ram when below 20% HP, patrol sacred waters
            // ----------------------------------------------------------------
            AiSubType::AbyssalCult => {
                // KAMIKAZE when critically damaged - ram nearest target
                if hull_pct < 0.20 {
                    if let Some((_, p_pos, dist)) = player_info {
                        if dist < 800.0 {
                            actions.push(ScoredAction {
                                score: 100.0,
                                behavior: AiSubBehavior::Engaging,
                                destination: Some(p_pos), // ram into player
                            });
                        }
                    }
                }

                // Attack player if they're near creatures (protecting sea life)
                if let Some((_, c_dist, c_pos, _)) = nearest_creature {
                    if let Some((_, p_pos, p_dist)) = player_info {
                        // Player near a creature = threat to sacred life
                        if p_dist < 600.0 && c_dist < 400.0 {
                            actions.push(ScoredAction {
                                score: 88.0,
                                behavior: AiSubBehavior::Engaging,
                                destination: Some(p_pos),
                            });
                        }
                    }
                }

                // Under fire → fight back (fanatics don't flee easily)
                if under_fire {
                    if let Some((_, p_pos, _)) = player_info {
                        actions.push(ScoredAction {
                            score: 82.0,
                            behavior: AiSubBehavior::Engaging,
                            destination: Some(p_pos),
                        });
                    }
                }

                // Escort creatures (follow them around)
                if let Some((_, _, c_pos, _)) = nearest_creature {
                    actions.push(ScoredAction {
                        score: 60.0,
                        behavior: AiSubBehavior::FollowingTradeRoute,
                        destination: Some(c_pos + Vec2::new(100.0, 0.0)),
                    });
                }

                // Patrol sacred waters
                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 35.0,
                        behavior: AiSubBehavior::Patrolling,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                    });
                }
            }

            // ----------------------------------------------------------------
            // THE DROWNED: Mindless ghost ships. Attack everything in range.
            // No fleeing, no self-preservation. Erratic movement.
            // ----------------------------------------------------------------
            AiSubType::Drowned => {
                // Never flee - already dead, can't die again (narratively)

                // Attack anything nearby - player
                if let Some((_, p_pos, dist)) = player_info {
                    if dist < 500.0 {
                        actions.push(ScoredAction {
                            score: 80.0,
                            behavior: AiSubBehavior::Engaging,
                            destination: Some(p_pos),
                        });
                    }
                }

                // Attack creatures too (mindless aggression)
                if let Some((_, dist, c_pos, _)) = nearest_creature {
                    if dist < 300.0 {
                        actions.push(ScoredAction {
                            score: 70.0,
                            behavior: AiSubBehavior::Engaging,
                            destination: Some(c_pos),
                        });
                    }
                }

                // Wander aimlessly - repeat old patrol routes (ghost behavior)
                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 50.0,
                        behavior: AiSubBehavior::Patrolling,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                    });
                }

                // Just drift if nothing else
                actions.push(ScoredAction {
                    score: 20.0,
                    behavior: AiSubBehavior::Idle,
                    destination: Some(pos + Vec2::new(50.0, -30.0)),
                });
            }

            // ----------------------------------------------------------------
            // PRESSURE KINGS: Deep-zone gatekeepers.
            // Attack anyone above 800m in their territory.
            // Ram intruders upward. Don't flee. Ignore shallow threats.
            // ----------------------------------------------------------------
            AiSubType::PressureKing => {
                // Only active in deep void - idle near station
                if depth < 600.0 {
                    actions.push(ScoredAction {
                        score: 90.0,
                        behavior: AiSubBehavior::Patrolling,
                        destination: Some(pos + Vec2::new(0.0, -500.0)), // go deeper
                    });
                } else {
                    // RAM player upward - primary behavior
                    if let Some((_, p_pos, dist)) = player_info {
                        if dist < 500.0 {
                            // Position above the player to push them up
                            let ram_pos = Vec2::new(p_pos.x, p_pos.y + 150.0);
                            actions.push(ScoredAction {
                                score: 90.0,
                                behavior: AiSubBehavior::Engaging,
                                destination: Some(ram_pos),
                            });
                        }
                    }

                    // Attack anyone under fire (never retreat in deep zone)
                    if under_fire {
                        if let Some((_, p_pos, _)) = player_info {
                            actions.push(ScoredAction {
                                score: 95.0,
                                behavior: AiSubBehavior::Engaging,
                                destination: Some(p_pos),
                            });
                        }
                    }

                    // Only retreat if almost destroyed
                    if hull_pct < 0.10 {
                        actions.push(ScoredAction {
                            score: 98.0,
                            behavior: AiSubBehavior::Fleeing,
                            destination: Some(pos + Vec2::new(0.0, -800.0)), // flee DEEPER
                        });
                    }
                }

                // Deep patrol
                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 40.0,
                        behavior: AiSubBehavior::Patrolling,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                    });
                }
            }

            // ----------------------------------------------------------------
            // GLASS EYE: Silent watchers. NEVER attack. Always flee.
            // Follow player from safe distance. Fastest flee speed.
            // ----------------------------------------------------------------
            AiSubType::GlassEye => {
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
                        behavior: AiSubBehavior::Fleeing,
                        destination: Some(flee_dest),
                    });
                }

                // Shadow the player from 500-800u distance (surveillance)
                if let Some((_, p_pos, dist)) = player_info {
                    if dist < 400.0 {
                        // Too close - back off
                        let away = (pos - p_pos).normalize_or_zero();
                        actions.push(ScoredAction {
                            score: 80.0,
                            behavior: AiSubBehavior::Fleeing,
                            destination: Some(pos + away * 300.0),
                        });
                    } else if dist < 900.0 {
                        // Good shadowing distance - maintain
                        let toward = (p_pos - pos).normalize_or_zero();
                        let shadow_pos = p_pos - toward * 650.0;
                        actions.push(ScoredAction {
                            score: 70.0,
                            behavior: AiSubBehavior::FollowingTradeRoute,
                            destination: Some(shadow_pos),
                        });
                    }
                }

                // Roam scanning routes
                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 45.0,
                        behavior: AiSubBehavior::FollowingTradeRoute,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                    });
                }
            }

            // ----------------------------------------------------------------
            // IRON TIDE: Heavy battleship. Engage everything hostile in range.
            // Slow to maneuver. Never flees unless nearly destroyed.
            // Maximum aggression at close range.
            // ----------------------------------------------------------------
            AiSubType::IronTide => {
                // Only retreat at extreme damage
                if hull_pct < 0.10 && fuel_pct < 0.15 {
                    actions.push(ScoredAction {
                        score: 100.0,
                        behavior: AiSubBehavior::Fleeing,
                        destination: Some(pos + Vec2::new(0.0, 500.0)),
                    });
                }

                // Under fire → immediately engage (battle-hardened)
                if under_fire {
                    if let Some((_, p_pos, _)) = player_info {
                        actions.push(ScoredAction {
                            score: 95.0,
                            behavior: AiSubBehavior::Engaging,
                            destination: Some(p_pos),
                        });
                    }
                }

                // Engage any target in weapon range (long range weapons)
                if let Some((_, p_pos, dist)) = player_info {
                    if dist < 600.0 {
                        actions.push(ScoredAction {
                            score: 85.0,
                            behavior: AiSubBehavior::Engaging,
                            destination: Some(p_pos),
                        });
                    }
                }

                // Engage large creatures too (shows dominance)
                if let Some((_, dist, c_pos, c_type)) = nearest_creature {
                    let is_large = matches!(c_type, CreatureType::Leviathan);
                    if dist < 400.0 && is_large {
                        actions.push(ScoredAction {
                            score: 75.0,
                            behavior: AiSubBehavior::Engaging,
                            destination: Some(c_pos),
                        });
                    }
                }

                // Slow patrol - battleship doesn't rush
                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 40.0,
                        behavior: AiSubBehavior::Patrolling,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                    });
                }
            }

            // ----------------------------------------------------------------
            // BLACKWATER PMC: Tactical mercs. Hunt bounties (player if hostile).
            // Flank targets. Coordinate. Disengage when outmatched.
            // ----------------------------------------------------------------
            AiSubType::Blackwater => {
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
                        behavior: AiSubBehavior::Fleeing,
                        destination: Some(flee_dest),
                    });
                }

                // Under fire → fight back
                if under_fire {
                    if let Some((_, p_pos, _)) = player_info {
                        // FLANK - don't go straight at target, offset to the side
                        let to_target = (p_pos - pos).normalize_or_zero();
                        let flank = Vec2::new(-to_target.y, to_target.x); // perpendicular
                        let flank_pos = p_pos + flank * 200.0;
                        actions.push(ScoredAction {
                            score: 85.0,
                            behavior: AiSubBehavior::Engaging,
                            destination: Some(flank_pos),
                        });
                    }
                }

                // Engage player at medium range (tactical distance)
                if let Some((_, p_pos, dist)) = player_info {
                    if dist < 450.0 {
                        let to_target = (p_pos - pos).normalize_or_zero();
                        let flank = Vec2::new(-to_target.y, to_target.x);
                        let offset = if pos.x > p_pos.x { 1.0 } else { -1.0 };
                        let flank_pos = p_pos + flank * 180.0 * offset;
                        actions.push(ScoredAction {
                            score: 78.0,
                            behavior: AiSubBehavior::Engaging,
                            destination: Some(flank_pos),
                        });
                    }
                }

                // Patrol routes
                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 45.0,
                        behavior: AiSubBehavior::Patrolling,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                    });
                }
            }

            // ----------------------------------------------------------------
            // RUST SWARM: Aggressive junk. Attack anything nearby.
            // No self-preservation. Kamikaze when critical. Mine everything.
            // ----------------------------------------------------------------
            AiSubType::RustSwarm => {
                // KAMIKAZE when critical - charge at nearest target
                if hull_pct < 0.25 {
                    if let Some((_, p_pos, dist)) = player_info {
                        if dist < 600.0 {
                            actions.push(ScoredAction {
                                score: 100.0,
                                behavior: AiSubBehavior::Engaging,
                                destination: Some(p_pos), // death charge
                            });
                        }
                    }
                }

                // Attack player aggressively (swarm mentality)
                if let Some((_, p_pos, dist)) = player_info {
                    if dist < 500.0 {
                        actions.push(ScoredAction {
                            score: 80.0,
                            behavior: AiSubBehavior::Engaging,
                            destination: Some(p_pos),
                        });
                    }
                }

                // Also chase wrecks for scrap
                if let Some((_, _, w_pos)) = nearest_wreck {
                    actions.push(ScoredAction {
                        score: 65.0,
                        behavior: AiSubBehavior::Salvaging,
                        destination: Some(w_pos),
                    });
                }

                // Erratic patrol (random, twitchy movement)
                if !nav.waypoints.is_empty() {
                    actions.push(ScoredAction {
                        score: 35.0,
                        behavior: AiSubBehavior::Patrolling,
                        destination: nav.waypoints.get(nav.current_waypoint).copied(),
                    });
                }

                // Idle drift
                actions.push(ScoredAction {
                    score: 15.0,
                    behavior: AiSubBehavior::Idle,
                    destination: Some(pos + Vec2::new(30.0, -20.0)),
                });
            }
        }

        // Default fallback for all factions
        actions.push(ScoredAction {
            score: 5.0,
            behavior: AiSubBehavior::Patrolling,
            destination: nav.waypoints.get(nav.current_waypoint).copied(),
        });

        // Pick highest score
        if let Some(best) = actions.iter().max_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal)) {
            *behavior = best.behavior;
            if let Some(dest) = best.destination {
                nav.destination = Some(dest);
            }
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
