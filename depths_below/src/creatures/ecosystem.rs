use bevy::prelude::*;
use bevy::sprite::TextureAtlasSprite;

#[allow(unused_imports)]
use crate::components::{
    AttackCooldown, Corpse, Creature, CreatureAI, CreatureAIState,
    CreatureAnimation, CreatureMemory, CreatureNeeds, CreatureType, EcoTarget,
    FoodChainRole, FoodChainTier, NoiseTrailPoint, Reproductive, Submarine, Territory,
    Velocity,
};
use crate::ai_submarine::components::{AiSubmarine, AiSubState};
use crate::events::{
    CascadeType, CreatureAteCorpse, EcosystemCascade, ShowNotification,
};
use crate::resources::{EcosystemConfig, EcosystemState, NoiseState};

use super::food_chain;

/// Tick hunger up and energy down each frame
pub fn update_creature_needs(
    time: Res<Time>,
    mut query: Query<(&mut CreatureNeeds, &CreatureAI)>,
) {
    let dt = time.delta_seconds();
    for (mut needs, ai) in query.iter_mut() {
        needs.hunger = (needs.hunger + needs.hunger_rate * dt).min(100.0);

        if ai.state == CreatureAIState::Idle {
            // Energy recovers during idle
            needs.energy = (needs.energy + 5.0 * dt).min(100.0);
        } else {
            needs.energy = (needs.energy - needs.energy_drain_rate * dt).max(0.0);
        }
    }
}

/// Sensory data built per creature each frame
struct PerceptionData {
    nearest_prey_creature: Option<(Entity, f32, Vec2)>,
    nearest_threat: Option<(Entity, f32, Vec2)>,
    nearest_corpse: Option<(Entity, f32, Vec2)>,
    sub_distance: Option<(Entity, f32, Vec2)>,
    nearest_ai_sub: Option<(Entity, f32, Vec2)>,
    noise_trail: Option<(Vec2, f32)>,
}

/// Scan nearby entities to build per-creature sensory data, then run priority scorer
pub fn ecosystem_ai_decisions(
    mut creatures: Query<(
        Entity,
        &Transform,
        &Creature,
        &mut CreatureAI,
        &CreatureNeeds,
        &FoodChainRole,
        Option<&Territory>,
    )>,
    other_creatures: Query<(Entity, &Transform, &Creature), Without<Submarine>>,
    corpse_query: Query<(Entity, &Transform, &Corpse)>,
    sub_query: Query<(Entity, &Transform), With<Submarine>>,
    ai_sub_query: Query<(Entity, &Transform, &AiSubState), With<AiSubmarine>>,
    trail_query: Query<(&Transform, &NoiseTrailPoint)>,
    noise_state: Option<Res<NoiseState>>,
    eco_config: Res<EcosystemConfig>,
) {
    let sub_info = sub_query.iter().next();
    let noise_level = noise_state.map(|n| n.noise_level).unwrap_or(0.0);

    // Collect creature data to avoid borrow conflicts
    let creature_data: Vec<(Entity, Vec2, f32, CreatureType, f32, f32)> = creatures
        .iter()
        .map(|(e, t, c, _ai, needs, _, _)| {
            (e, t.translation.truncate(), c.detection_range, c.creature_type, c.health / c.max_health, needs.hunger)
        })
        .collect();

    // For each creature, build perception and decide
    for (entity, transform, creature, mut ai, needs, role, territory) in creatures.iter_mut() {
        let pos = transform.translation.truncate();
        let range = creature.detection_range;

        // Build perception
        let mut perception = PerceptionData {
            nearest_prey_creature: None,
            nearest_threat: None,
            nearest_corpse: None,
            sub_distance: None,
            nearest_ai_sub: None,
            noise_trail: None,
        };

        // Check other creatures
        for &(other_entity, other_pos, _, other_type, _, _) in &creature_data {
            if other_entity == entity {
                continue;
            }
            let dist = pos.distance(other_pos);
            if dist > range * 1.5 {
                continue;
            }

            // Is this a threat?
            if role.threat_types.contains(&other_type) {
                if perception.nearest_threat.map_or(true, |(_, d, _)| dist < d) {
                    perception.nearest_threat = Some((other_entity, dist, other_pos));
                }
            }

            // Is this prey?
            if role.prey_types.contains(&other_type) {
                if perception.nearest_prey_creature.map_or(true, |(_, d, _)| dist < d) {
                    perception.nearest_prey_creature = Some((other_entity, dist, other_pos));
                }
            }
        }

        // Check corpses
        {
            for (corpse_entity, corpse_transform, _corpse) in corpse_query.iter() {
                let corpse_pos = corpse_transform.translation.truncate();
                let dist = pos.distance(corpse_pos);
                if dist < range * 1.5 {
                    if perception.nearest_corpse.map_or(true, |(_, d, _)| dist < d) {
                        perception.nearest_corpse = Some((corpse_entity, dist, corpse_pos));
                    }
                }
            }
        }

        // Check submarine
        if role.attacks_submarine {
            if let Some((sub_entity, sub_transform)) = sub_info {
                let sub_pos = sub_transform.translation.truncate();
                let dist = pos.distance(sub_pos);

                // Territory bonus: +50% detection range if sub is in territory
                let effective_range = if let Some(terr) = territory {
                    if sub_pos.distance(terr.center) < terr.radius {
                        range * 1.5
                    } else {
                        range
                    }
                } else {
                    range
                };

                if dist < effective_range {
                    perception.sub_distance = Some((sub_entity, dist, sub_pos));
                }
            }
        }

        // Check AI submarines
        if role.attacks_submarine {
            for (ai_entity, ai_transform, ai_state) in ai_sub_query.iter() {
                if ai_state.is_destroyed {
                    continue;
                }
                let ai_pos = ai_transform.translation.truncate();
                let dist = pos.distance(ai_pos);
                if dist < range {
                    if perception.nearest_ai_sub.map_or(true, |(_, d, _)| dist < d) {
                        perception.nearest_ai_sub = Some((ai_entity, dist, ai_pos));
                    }
                }
            }
        }

        // Check noise trail
        let trail_range = match creature.creature_type {
            CreatureType::Stalker => range * 1.5,
            _ => range,
        };
        let mut best_trail: Option<(Vec2, f32)> = None;
        for (trail_transform, trail_point) in trail_query.iter() {
            let trail_pos = trail_transform.translation.truncate();
            let dist = pos.distance(trail_pos);
            if dist < trail_range {
                if best_trail.map_or(true, |(_, intensity)| trail_point.intensity > intensity) {
                    best_trail = Some((trail_pos, trail_point.intensity));
                }
            }
        }
        perception.noise_trail = best_trail;

        // Priority scorer
        let health_pct = creature.health / creature.max_health;
        let hunger_pct = needs.hunger / 100.0;

        struct ScoredAction {
            score: f32,
            state: CreatureAIState,
            target: Option<EcoTarget>,
        }

        let mut actions: Vec<ScoredAction> = Vec::with_capacity(8);

        // 1. Flee (wounded) — except Leviathan
        if health_pct < 0.25
            && creature.creature_type != CreatureType::Leviathan
        {
            let flee_target = perception
                .nearest_threat
                .or(perception.sub_distance)
                .map(|(_, _, p)| EcoTarget::Position(pos + (pos - p).normalize_or_zero() * 300.0));
            actions.push(ScoredAction {
                score: 100.0,
                state: CreatureAIState::Fleeing,
                target: flee_target,
            });
        }

        // 2. Flee (threat nearby)
        if let Some((threat_e, threat_dist, _)) = perception.nearest_threat {
            if threat_dist < range {
                actions.push(ScoredAction {
                    score: 90.0,
                    state: CreatureAIState::Fleeing,
                    target: Some(EcoTarget::Creature(threat_e)),
                });
            }
        }

        // 3. Attack — already in melee range of current target
        if let Some(current_target) = &ai.target {
            let in_range = match current_target {
                EcoTarget::Creature(e) => {
                    other_creatures.get(*e).ok().map(|(_, t, _)| {
                        pos.distance(t.translation.truncate()) < 80.0
                    }).unwrap_or(false)
                }
                EcoTarget::Submarine(e) => {
                    sub_query.get(*e).ok().map(|(_, t)| {
                        pos.distance(t.translation.truncate()) < 100.0
                    }).unwrap_or(false)
                }
                _ => false,
            };
            if in_range {
                actions.push(ScoredAction {
                    score: 85.0,
                    state: CreatureAIState::Attacking,
                    target: Some(*current_target),
                });
            }
        }

        // 4. Feed on corpse
        if let Some((corpse_e, corpse_dist, _)) = perception.nearest_corpse {
            if needs.hunger > eco_config.hunt_hunger_threshold && corpse_dist < 80.0 {
                actions.push(ScoredAction {
                    score: 80.0 * hunger_pct,
                    state: CreatureAIState::Feeding,
                    target: Some(EcoTarget::Corpse(corpse_e)),
                });
            } else if needs.hunger > eco_config.hunt_hunger_threshold {
                actions.push(ScoredAction {
                    score: 70.0 * hunger_pct,
                    state: CreatureAIState::Hunting,
                    target: Some(EcoTarget::Corpse(corpse_e)),
                });
            }
        }

        // 5. Hunt prey creature
        if let Some((prey_e, _, _)) = perception.nearest_prey_creature {
            if needs.hunger > eco_config.hunt_hunger_threshold {
                actions.push(ScoredAction {
                    score: 70.0 * hunger_pct,
                    state: CreatureAIState::Hunting,
                    target: Some(EcoTarget::Creature(prey_e)),
                });
            }
        }

        // 6. Hunt submarine
        if let Some((sub_e, _, _)) = perception.sub_distance {
            if needs.hunger > eco_config.hunt_hunger_threshold && role.attacks_submarine {
                let noise_factor = (noise_level / 100.0).clamp(0.1, 1.0);
                actions.push(ScoredAction {
                    score: 60.0 * hunger_pct * noise_factor,
                    state: CreatureAIState::Hunting,
                    target: Some(EcoTarget::Submarine(sub_e)),
                });
            }
        }

        // 6b. Hunt AI submarine (slightly lower priority than player sub)
        if let Some((ai_sub_e, _, _)) = perception.nearest_ai_sub {
            if needs.hunger > eco_config.hunt_hunger_threshold && role.attacks_submarine {
                actions.push(ScoredAction {
                    score: 55.0 * hunger_pct,
                    state: CreatureAIState::Hunting,
                    target: Some(EcoTarget::AiSubmarine(ai_sub_e)),
                });
            }
        }

        // 7. Investigate noise trail
        if let Some((trail_pos, intensity)) = perception.noise_trail {
            if intensity > 5.0 {
                let normalized = (intensity / 100.0).clamp(0.0, 1.0);
                actions.push(ScoredAction {
                    score: 50.0 * normalized,
                    state: CreatureAIState::Investigating,
                    target: Some(EcoTarget::Position(trail_pos)),
                });
            }
        }

        // 8. Defend territory
        if let Some(terr) = territory {
            // Check for intruders
            for &(other_entity, other_pos, _, other_type, _, _) in &creature_data {
                if other_entity == entity {
                    continue;
                }
                if other_pos.distance(terr.center) < terr.radius {
                    // Same or lower tier intruder
                    let other_role = food_chain::food_chain_role(other_type);
                    if other_role.tier as u8 >= role.tier as u8 || other_role.tier == FoodChainTier::Prey {
                        actions.push(ScoredAction {
                            score: 45.0 * terr.aggression,
                            state: CreatureAIState::Hunting,
                            target: Some(EcoTarget::Creature(other_entity)),
                        });
                        break;
                    }
                }
            }

            // Sub in territory
            if let Some((sub_e, _, sub_pos)) = perception.sub_distance {
                if sub_pos.distance(terr.center) < terr.radius {
                    actions.push(ScoredAction {
                        score: 45.0 * terr.aggression,
                        state: CreatureAIState::Hunting,
                        target: Some(EcoTarget::Submarine(sub_e)),
                    });
                }
            }
        }

        // 9. Patrol territory
        if territory.is_some() {
            actions.push(ScoredAction {
                score: 30.0,
                state: CreatureAIState::Patrolling,
                target: None,
            });
        }

        // 10. Migrate (if has migration path)
        // (handled by migration system adding MigrationPath component)

        // 11. Wander (default)
        actions.push(ScoredAction {
            score: 10.0,
            state: CreatureAIState::Wandering,
            target: None,
        });

        // Pick highest score
        if let Some(best) = actions.iter().max_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal)) {
            ai.state = best.state;
            if best.target.is_some() {
                ai.target = best.target;
            } else if best.state == CreatureAIState::Wandering || best.state == CreatureAIState::Patrolling {
                // Keep existing target for patrol/wander or clear it
                ai.target = None;
            }
        }
    }
}

/// Creatures in Feeding state drain corpse food and reduce hunger
pub fn feeding_system(
    time: Res<Time>,
    mut creatures: Query<(Entity, &CreatureAI, &mut CreatureNeeds, &Creature)>,
    mut corpses: Query<(Entity, &mut Corpse)>,
    mut commands: Commands,
    mut ate_corpse_events: EventWriter<CreatureAteCorpse>,
) {
    let dt = time.delta_seconds();
    for (creature_entity, ai, mut needs, creature) in creatures.iter_mut() {
        if ai.state != CreatureAIState::Feeding {
            continue;
        }
        if let Some(EcoTarget::Corpse(corpse_entity)) = ai.target {
            if let Ok((_e, mut corpse)) = corpses.get_mut(corpse_entity) {
                let eat_amount = 15.0 * dt;
                let actual = eat_amount.min(corpse.food_remaining);
                corpse.food_remaining -= actual;
                needs.hunger = (needs.hunger - actual * 2.0).max(0.0);

                if corpse.food_remaining <= 0.0 {
                    commands.entity(corpse_entity).despawn_recursive();
                    ate_corpse_events.send(CreatureAteCorpse {
                        creature: creature_entity,
                        creature_type: creature.creature_type,
                        corpse_type: corpse.creature_type,
                    });
                }
            }
        }
    }
}

/// Well-fed creatures tick gestation timer and spawn offspring
pub fn reproduction_system(
    time: Res<Time>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlases: ResMut<Assets<TextureAtlas>>,
    mut creatures: Query<(
        &Transform,
        &Creature,
        &CreatureNeeds,
        &mut Reproductive,
    )>,
    eco_state: Res<EcosystemState>,
    eco_config: Res<EcosystemConfig>,
) {
    let dt = time.delta_seconds();
    let total: u32 = eco_state.population_counts.values().sum();

    for (transform, creature, needs, mut repro) in creatures.iter_mut() {
        if needs.hunger > repro.satiation_threshold {
            repro.gestation_timer = 0.0;
            continue;
        }

        if total >= eco_config.max_total_creatures {
            continue;
        }

        let cap = eco_config
            .per_type_caps
            .get(&creature.creature_type)
            .copied()
            .unwrap_or(5);
        let current = eco_state
            .population_counts
            .get(&creature.creature_type)
            .copied()
            .unwrap_or(0);
        if current >= cap {
            continue;
        }

        repro.gestation_timer += dt;
        if repro.gestation_timer >= repro.gestation_duration {
            repro.gestation_timer = 0.0;

            let pos = transform.translation.truncate();
            // Spawn offspring nearby — the actual entity creation is handled by
            // the spawn_creature function in mod.rs. We'll just send a marker.
            // For simplicity, we spawn them directly here with minimal components.
            // The spawn_creatures system in mod.rs will attach full components.
            for i in 0..repro.offspring_count.min(3) {
                let offset = Vec2::new(
                    (i as f32 * 120.0 - 120.0) + 50.0,
                    i as f32 * 80.0 - 80.0,
                );
                let spawn_pos = pos + offset;

                use crate::components::*;
                use crate::sprite_map;
                let eco_stats = super::food_chain::creature_ecosystem_stats(creature.creature_type);
                let role = super::food_chain::food_chain_role(creature.creature_type);

                // Offspring are smaller/weaker (0.5x-0.7x parent size)
                let offspring_health = creature.max_health * 0.5;
                let offspring_damage = creature.damage * 0.5;

                // Frame size per creature type
                let (frame_w, frame_h) = match creature.creature_type {
                    CreatureType::Leviathan => (128, 64),
                    _ => (64, 64),
                };
                let total_frames = 6;

                let texture: Handle<Image> = asset_server.load(
                    sprite_map::creature_sprite_path(creature.creature_type)
                );
                let atlas = TextureAtlas::from_grid(
                    texture,
                    Vec2::new(frame_w as f32, frame_h as f32),
                    total_frames, 1, None, None,
                );

                // Offspring are visually smaller
                let offspring_scale = 0.5;
                let (base_w, base_h) = match creature.creature_type {
                    CreatureType::VoidDrifter =>     (45.0, 22.0),
                    CreatureType::Stalker =>         (150.0, 54.0),
                    CreatureType::Leviathan =>       (660.0, 180.0),
                    CreatureType::ParasiteSwarm =>   (30.0, 20.0),
                };

                let mut entity_commands = commands.spawn((
                    SpriteSheetBundle {
                        texture_atlas: texture_atlases.add(atlas),
                        sprite: TextureAtlasSprite {
                            index: 0,
                            custom_size: Some(Vec2::new(
                                base_w * offspring_scale,
                                base_h * offspring_scale,
                            )),
                            ..default()
                        },
                        transform: Transform::from_translation(spawn_pos.extend(5.0)),
                        ..default()
                    },
                    Creature {
                        creature_type: creature.creature_type,
                        health: offspring_health,
                        max_health: offspring_health,
                        damage: offspring_damage,
                        speed: creature.speed * 0.8,
                        detection_range: creature.detection_range * 0.7,
                        attack_cooldown: creature.attack_cooldown,
                        food_value: eco_stats.food_value * 0.5,
                    },
                    CreatureAI {
                        state: CreatureAIState::Wandering,
                        target: None,
                        home_position: spawn_pos,
                        wander_radius: 200.0,
                    },
                    AttackCooldown {
                        timer: Timer::from_seconds(
                            match creature.creature_type {
                                CreatureType::Leviathan => 3.0,
                                CreatureType::ParasiteSwarm => 0.5,
                                _ => 1.5,
                            },
                            TimerMode::Once,
                        ),
                    },
                    Velocity(Vec2::ZERO),
                    CreatureAnimation {
                        timer: Timer::from_seconds(0.15, TimerMode::Repeating),
                        swim_frames: 4,
                        attack_frames: 2,
                        total_frames: 6,
                        current_frame: 0,
                    },
                    CreatureNeeds {
                        hunger: 10.0,
                        energy: 80.0,
                        hunger_rate: eco_stats.hunger_rate,
                        energy_drain_rate: eco_stats.energy_drain_rate,
                    },
                    role,
                    CreatureMemory::default(),
                ));

                if eco_stats.can_reproduce {
                    entity_commands.insert(Reproductive {
                        gestation_timer: 0.0,
                        gestation_duration: eco_stats.gestation_duration,
                        offspring_count: eco_stats.offspring_count,
                        satiation_threshold: eco_stats.satiation_threshold,
                    });
                }
            }
        }
    }
}

/// Track population counts and detect cascades
pub fn population_balance(
    time: Res<Time>,
    creature_query: Query<&Creature>,
    mut eco_state: ResMut<EcosystemState>,
    eco_config: Res<EcosystemConfig>,
    mut cascade_events: EventWriter<EcosystemCascade>,
    mut notifications: EventWriter<ShowNotification>,
) {
    eco_state.total_elapsed += time.delta_seconds();

    // Rebuild population counts
    eco_state.population_counts.clear();
    for creature in creature_query.iter() {
        *eco_state
            .population_counts
            .entry(creature.creature_type)
            .or_insert(0) += 1;
    }

    // Clean old kill records
    let current_time = eco_state.total_elapsed;
    eco_state
        .recent_kills
        .retain(|k| current_time - k.time < eco_config.cascade_time_window);

    // Check for cascades
    let player_kills_recent: Vec<_> = eco_state
        .recent_kills
        .iter()
        .filter(|k| k.by_player)
        .collect();

    if player_kills_recent.len() >= eco_config.cascade_kill_count as usize {
        // Check if these are predator kills → trigger scavenger swarm
        let predator_kills = player_kills_recent
            .iter()
            .filter(|k| {
                matches!(
                    k.victim_type,
                    CreatureType::Stalker
                        | CreatureType::Leviathan
                )
            })
            .count();

        if predator_kills >= eco_config.cascade_kill_count as usize {
            if let Some(last_kill) = player_kills_recent.last() {
                cascade_events.send(EcosystemCascade {
                    cascade_type: CascadeType::ScavengerSwarm,
                    position: last_kill.position,
                });
                notifications.send(ShowNotification {
                    message: "The scavengers are swarming... your kills have drawn attention!"
                        .to_string(),
                    notification_type: crate::events::NotificationType::Warning,
                    duration: 5.0,
                });
            }
            // Clear recent kills to avoid re-triggering
            eco_state.recent_kills.clear();
        }
    }
}

/// Creatures with hunger > 90 take starvation damage
pub fn starvation_system(
    time: Res<Time>,
    mut creatures: Query<(&mut Creature, &CreatureNeeds)>,
    eco_config: Res<EcosystemConfig>,
) {
    let dt = time.delta_seconds();
    for (mut creature, needs) in creatures.iter_mut() {
        if needs.hunger > eco_config.starve_hunger_threshold {
            creature.health -= eco_config.starvation_damage * dt;
        }
    }
}

/// Update creature memory with observed positions
pub fn update_creature_memory(
    time: Res<Time>,
    mut creatures: Query<(&Transform, &CreatureAI, &mut CreatureMemory)>,
    sub_query: Query<&Transform, With<Submarine>>,
) {
    let dt = time.delta_seconds();
    let sub_pos = sub_query.iter().next().map(|t| t.translation.truncate());

    for (transform, ai, mut memory) in creatures.iter_mut() {
        let pos = transform.translation.truncate();

        // Decay memory timestamps
        for zone in memory.danger_zones.iter_mut() {
            zone.1 -= dt;
        }
        memory.danger_zones.retain(|z| z.1 > 0.0);

        for loc in memory.food_locations.iter_mut() {
            loc.1 -= dt;
        }
        memory.food_locations.retain(|l| l.1 > 0.0);

        // Update last seen submarine position
        if let Some(target) = &ai.target {
            if matches!(target, EcoTarget::Submarine(_) | EcoTarget::AiSubmarine(_)) {
                if let Some(sp) = sub_pos {
                    memory.last_seen_sub = Some((sp, 30.0));
                }
            }
        }

        if let Some(ref mut sub_mem) = memory.last_seen_sub {
            sub_mem.1 -= dt;
            if sub_mem.1 <= 0.0 {
                memory.last_seen_sub = None;
            }
        }

        // Record danger zones when fleeing from a threat
        if ai.state == CreatureAIState::Fleeing {
            if let Some(target) = &ai.target {
                let threat_pos = match target {
                    EcoTarget::Creature(_) | EcoTarget::Submarine(_) | EcoTarget::AiSubmarine(_) => Some(pos),
                    EcoTarget::Position(p) => {
                        // Fleeing toward a position means danger is behind us
                        let flee_dir = (*p - pos).normalize_or_zero();
                        Some(pos - flee_dir * 100.0)
                    }
                    _ => None,
                };
                if let Some(danger_pos) = threat_pos {
                    if memory.danger_zones.len() < 5 {
                        memory.danger_zones.push((danger_pos, 90.0));
                    }
                }
            }
        }

        // Record food locations when feeding
        if ai.state == CreatureAIState::Feeding {
            if memory.food_locations.len() < 5 {
                memory.food_locations.push((pos, 60.0));
            }
        }
    }
}
