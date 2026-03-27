use bevy::prelude::*;
use bevy::sprite::TextureAtlasSprite;
use rand::Rng;
use crate::states::GameState;
use crate::components::*;
use crate::resources::*;
use crate::events::*;
use crate::sprite_map;
use crate::ai_submarine::components::AiSubmarine;

mod behaviors;
pub mod food_chain;
pub mod ecosystem;
pub mod corpse;
pub mod creature_combat;
pub mod noise_trail;
pub mod territory;
pub mod migration;

pub use behaviors::*;

pub struct CreaturePlugin;

impl Plugin for CreaturePlugin {
    fn build(&self, app: &mut App) {
        // Init ecosystem resources
        app.init_resource::<EcosystemState>()
            .init_resource::<EcosystemConfig>()
            .init_resource::<noise_trail::NoiseTrailTimer>();

        app.add_systems(
            Update,
            (
                // Core creature AI pipeline (chained)
                spawn_creatures,
                ecosystem::update_creature_needs,
                ecosystem::ecosystem_ai_decisions,
                migration::follow_migration_path,
                watcher_alert_system,
                creature_movement,
                creature_attack_system,
                creature_combat::creature_vs_creature_combat,
                check_creature_detection,
                electric_eel_shock,
                swarm_queen_spawn,
            )
                .chain()
                .run_if(in_state(GameState::Exploring)),
        )
        .add_systems(
            Update,
            (
                // Ecosystem support systems (independent of main chain)
                ecosystem::feeding_system,
                ecosystem::starvation_system,
                ecosystem::reproduction_system,
                ecosystem::population_balance,
                ecosystem::update_creature_memory,
                corpse::spawn_corpse_on_death,
                corpse::decay_corpses,
                noise_trail::emit_noise_trail,
                noise_trail::decay_noise_trails,
                territory::update_territories,
                migration::check_migration,
                cleanup_distant_creatures,
            )
                .run_if(in_state(GameState::Exploring)),
        )
        .add_systems(
            Update,
            (
                // Ambient life systems (independent, lightweight)
                spawn_ambient_life,
                ambient_movement,
                cleanup_ambient,
                animate_creature_sprites,
            )
                .run_if(in_state(GameState::Exploring)),
        );
    }
}

/// Spawns creatures based on depth and biome, using ecosystem population caps.
fn spawn_creatures(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlases: ResMut<Assets<TextureAtlas>>,
    time: Res<Time>,
    config: Res<GameConfig>,
    sub_state: Res<DepthState>,
    world_state: Res<WorldState>,
    submarine_query: Query<&Transform, With<Submarine>>,
    creature_query: Query<&Creature>,
    eco_state: Res<EcosystemState>,
    eco_config: Res<EcosystemConfig>,
    mut spawn_timer: Local<f32>,
    session_timer: Res<ExploringSessionTimer>,
) {
    // Don't spawn any hostile creatures in the first 30 seconds of Exploring
    let hostile_warmup = 30.0;
    if session_timer.elapsed < hostile_warmup {
        return;
    }

    *spawn_timer += time.delta_seconds();

    // Use ecosystem total cap
    let total_creatures: u32 = eco_state.population_counts.values().sum();
    if total_creatures >= eco_config.max_total_creatures {
        return;
    }

    // Also check old depth-based max as a safety valve
    let depth = sub_state.current_depth.abs();
    let max_creatures = (5 + (depth / 400.0) as usize).min(eco_config.max_total_creatures as usize);
    if creature_query.iter().count() >= max_creatures {
        return;
    }

    // Spawn rate scales with depth (faster spawns deeper)
    let spawn_interval = (2.5 - (depth / 1500.0).min(1.0)) / config.creature_spawn_rate;
    if *spawn_timer >= spawn_interval {
        *spawn_timer = 0.0;

        // Use biome creature weights for weighted random selection
        let weights = crate::world::biome_creature_weights(world_state.current_biome);
        let creature_type = if !weights.is_empty() {
            let total: f32 = weights.iter().map(|(_, w)| w).sum();
            let mut roll = rand::thread_rng().gen::<f32>() * total;
            let mut selected = weights[0].0;
            for (name, weight) in &weights {
                roll -= weight;
                if roll <= 0.0 {
                    selected = name;
                    break;
                }
            }
            match selected {
                "scavenger" => CreatureType::Scavenger,
                "stalker" => CreatureType::Stalker,
                "ambusher" => CreatureType::Ambusher,
                "electric_eel" => CreatureType::ElectricEel,
                "blind_hunter" => CreatureType::BlindHunter,
                "lure_fish" => CreatureType::LureFish,
                "swarm_queen" => CreatureType::SwarmQueen,
                "leviathan" => CreatureType::Leviathan,
                "parasite" => CreatureType::Parasite,
                "watcher" => CreatureType::Watcher,
                _ => CreatureType::Scavenger,
            }
        } else {
            // Fallback: depth-based
            match sub_state.current_depth {
                d if d < 200.0 => CreatureType::Scavenger,
                d if d < 500.0 => CreatureType::Stalker,
                d if d < 1000.0 => CreatureType::BlindHunter,
                _ => CreatureType::Watcher,
            }
        };

        // Leviathans only spawn in deep water (1500m+)
        let creature_type = if creature_type == CreatureType::Leviathan && depth < 1500.0 {
            CreatureType::BlindHunter
        } else {
            creature_type
        };

        // Check per-type population cap
        let current_count = eco_state
            .population_counts
            .get(&creature_type)
            .copied()
            .unwrap_or(0);
        let type_cap = eco_config
            .per_type_caps
            .get(&creature_type)
            .copied()
            .unwrap_or(5);
        if current_count >= type_cap {
            return;
        }

        // Get submarine's actual position
        let sub_pos = submarine_query
            .get_single()
            .map(|t| t.translation.truncate())
            .unwrap_or(Vec2::ZERO);

        // Randomize spawn position around the submarine (full 360 degrees, 600-1000 units away)
        let mut rng = rand::thread_rng();
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let dist = rng.gen_range(600.0..1000.0);

        let offset = Vec2::new(angle.cos() * dist, angle.sin() * dist);
        let spawn_pos = sub_pos + offset;

        spawn_creature(&mut commands, &asset_server, &mut texture_atlases, creature_type, spawn_pos);
    }
}

/// Spawn a creature entity with animated sprite sheet and random size variation
fn spawn_creature(
    commands: &mut Commands,
    asset_server: &AssetServer,
    texture_atlases: &mut Assets<TextureAtlas>,
    creature_type: CreatureType,
    position: Vec2,
) {
    let mut rng = rand::thread_rng();

    // Use Y position as proxy for depth (negative Y = deeper)
    let depth = position.y.abs();
    let depth_factor = 1.0 + (depth / 1000.0).min(2.0);
    let speed_factor = 1.0 + (depth / 2000.0).min(0.5);
    let range_factor = 1.0 + (depth / 3000.0).min(0.3);

    // Base stats: (sprite_frame_w, sprite_frame_h, display_w, display_h, health, damage, speed, detection_range)
    // sprite_frame_w/h = per-frame size in the sprite sheet
    // display_w/h = world-space render size (before random scaling)
    let (frame_w, frame_h, base_w, base_h, health, damage, speed, range) = match creature_type {
        CreatureType::Scavenger =>    ( 64,  64,  84.0,  42.0,  20.0,  5.0,  50.0, 200.0),
        CreatureType::Stalker =>      ( 64,  64, 150.0,  54.0,  50.0, 15.0,  80.0, 350.0),
        CreatureType::Ambusher =>     ( 64,  64, 120.0,  48.0,  40.0, 20.0, 120.0, 180.0),
        CreatureType::ElectricEel =>  ( 64,  64, 165.0,  30.0,  25.0, 12.0,  90.0, 300.0),
        CreatureType::BlindHunter =>  ( 64,  64, 165.0, 105.0,  80.0, 25.0, 100.0, 150.0),
        CreatureType::LureFish =>     ( 64,  64,  66.0,  54.0,  30.0,  8.0,  30.0, 250.0),
        CreatureType::SwarmQueen =>   ( 64,  64, 180.0, 150.0,  60.0, 10.0,  40.0, 400.0),
        CreatureType::Leviathan =>    (128,  64, 660.0, 180.0, 500.0,100.0,  40.0, 500.0),
        CreatureType::Parasite =>     ( 64,  64,  36.0,  24.0,  15.0,  3.0,  70.0, 120.0),
        CreatureType::Watcher =>      ( 64,  64,  90.0,  90.0,  30.0,  0.0,  20.0, 600.0),
    };

    // Random size variation: 0.75x to 1.35x (bigger = more health, slower)
    let size_scale = rng.gen_range(0.75..1.35_f32);
    let w = base_w * size_scale;
    let h = base_h * size_scale;
    let health_scale = size_scale.powf(1.5); // bigger creatures are tougher
    let speed_scale = 1.0 / size_scale.sqrt(); // bigger creatures are slower

    let attack_time = match creature_type {
        CreatureType::Leviathan => 3.0,
        CreatureType::Scavenger => 2.0,
        CreatureType::Parasite => 0.5,
        _ => 1.5,
    };

    // Apply depth + size scaling to creature stats
    let scaled_health = health * depth_factor * health_scale;
    let scaled_damage = damage * depth_factor * size_scale;
    let scaled_speed = speed * speed_factor * speed_scale;
    let scaled_range = range * range_factor;

    // Get ecosystem data
    let eco_stats = food_chain::creature_ecosystem_stats(creature_type);
    let role = food_chain::food_chain_role(creature_type);

    // Animation: hostile creatures have 6 frames (4 swim + 2 attack) in a horizontal strip
    let swim_frames = 4;
    let attack_frames = 2;
    let total_frames = swim_frames + attack_frames;
    let anim_speed = match creature_type {
        CreatureType::Parasite => 0.1,
        CreatureType::Leviathan => 0.25,
        CreatureType::SwarmQueen => 0.2,
        _ => 0.15,
    };

    let texture: Handle<Image> = asset_server.load(sprite_map::creature_sprite_path(creature_type));
    let atlas = TextureAtlas::from_grid(
        texture,
        Vec2::new(frame_w as f32, frame_h as f32),
        total_frames, // columns
        1,            // rows
        None,
        None,
    );

    let mut entity_commands = commands.spawn((
        SpriteSheetBundle {
            texture_atlas: texture_atlases.add(atlas),
            sprite: TextureAtlasSprite {
                index: 0,
                custom_size: Some(Vec2::new(w, h)),
                ..default()
            },
            transform: Transform::from_xyz(position.x, position.y, 0.0),
            ..default()
        },
        Creature {
            creature_type,
            health: scaled_health,
            max_health: scaled_health,
            damage: scaled_damage,
            speed: scaled_speed,
            detection_range: scaled_range,
            attack_cooldown: 0.0,
            food_value: eco_stats.food_value * size_scale,
        },
        CreatureAI {
            state: CreatureAIState::Wandering,
            target: None,
            home_position: position,
            wander_radius: 200.0,
        },
        AttackCooldown {
            timer: Timer::from_seconds(attack_time, TimerMode::Once),
        },
        Velocity(Vec2::ZERO),
        CreatureAnimation {
            timer: Timer::from_seconds(anim_speed, TimerMode::Repeating),
            swim_frames,
            attack_frames,
            total_frames,
            current_frame: 0,
        },
        // Ecosystem components
        CreatureNeeds {
            hunger: 20.0 + rng.gen_range(0.0..30.0),
            energy: 80.0 + rng.gen_range(0.0..20.0),
            hunger_rate: eco_stats.hunger_rate,
            energy_drain_rate: eco_stats.energy_drain_rate,
        },
        role,
        CreatureMemory::default(),
    ));

    // Add territory for territorial creatures
    if eco_stats.is_territorial {
        entity_commands.insert(Territory {
            center: position,
            radius: eco_stats.territory_radius,
            aggression: eco_stats.territory_aggression,
        });
    }

    // Add reproduction for creatures that can breed
    if eco_stats.can_reproduce {
        entity_commands.insert(Reproductive {
            gestation_timer: 0.0,
            gestation_duration: eco_stats.gestation_duration,
            offspring_count: eco_stats.offspring_count,
            satiation_threshold: eco_stats.satiation_threshold,
        });
    }
}

/// Moves creatures based on AI state, resolving EcoTarget for direction
fn creature_movement(
    time: Res<Time>,
    submarine_query: Query<&Transform, With<Submarine>>,
    mut creature_query: Query<(Entity, &mut Transform, &Creature, &CreatureAI, &mut Velocity), Without<Submarine>>,
    corpse_query: Query<&Transform, (With<Corpse>, Without<Creature>, Without<Submarine>)>,
    ai_sub_positions: Query<&Transform, (With<AiSubmarine>, Without<Creature>, Without<Submarine>)>,
) {
    let sub_pos = submarine_query
        .get_single()
        .map(|t| t.translation.truncate())
        .unwrap_or(Vec2::ZERO);

    // Pre-collect creature positions to avoid conflicting borrows
    let creature_pos_map: std::collections::HashMap<Entity, Vec2> = creature_query
        .iter()
        .map(|(e, t, _, _, _)| (e, t.translation.truncate()))
        .collect();

    for (_entity, mut transform, creature, ai, mut velocity) in creature_query.iter_mut() {
        let pos = transform.translation.truncate();

        // Resolve target position from EcoTarget
        let target_pos = if let Some(ref eco_target) = ai.target {
            match eco_target {
                EcoTarget::Submarine(_) => sub_pos,
                EcoTarget::AiSubmarine(e) => {
                    ai_sub_positions.get(*e).ok()
                        .map(|t| t.translation.truncate())
                        .unwrap_or(sub_pos)
                }
                EcoTarget::Creature(e) => {
                    creature_pos_map.get(e).copied().unwrap_or(sub_pos)
                }
                EcoTarget::Corpse(e) => {
                    corpse_query.get(*e).ok()
                        .map(|t| t.translation.truncate())
                        .unwrap_or(sub_pos)
                }
                EcoTarget::Position(p) => *p,
            }
        } else {
            ai.home_position
        };

        // Wounded rage: creatures move faster when hurt (except passive types)
        let health_pct = if creature.max_health > 0.0 { creature.health / creature.max_health } else { 1.0 };
        let rage_mult = if health_pct < 0.4 && !matches!(creature.creature_type,
            CreatureType::Watcher | CreatureType::LureFish | CreatureType::Parasite) {
            1.5
        } else {
            1.0
        };

        let target_velocity = match ai.state {
            CreatureAIState::Hunting => {
                let direction = (target_pos - pos).normalize_or_zero();
                match creature.creature_type {
                    // Stalker: weaves side to side while approaching
                    CreatureType::Stalker => {
                        let perp = Vec2::new(-direction.y, direction.x);
                        let weave = (time.elapsed_seconds() * 3.0).sin() * 0.4;
                        (direction + perp * weave).normalize_or_zero() * creature.speed
                    }
                    // SwarmQueen: slow but relentless
                    CreatureType::SwarmQueen => direction * creature.speed * 0.6,
                    // Leviathan: phases based on health
                    CreatureType::Leviathan => {
                        if health_pct < 0.5 {
                            direction * creature.speed * 1.8 * rage_mult
                        } else {
                            let perp = Vec2::new(-direction.y, direction.x);
                            let sweep = (time.elapsed_seconds() * 1.2).sin() * 0.3;
                            (direction + perp * sweep).normalize_or_zero() * creature.speed * rage_mult
                        }
                    }
                    _ => direction * creature.speed,
                }
            }
            CreatureAIState::Attacking => {
                let direction = (target_pos - pos).normalize_or_zero();
                match creature.creature_type {
                    CreatureType::Ambusher => direction * creature.speed * 2.5,
                    CreatureType::Parasite => direction * creature.speed * 1.3,
                    CreatureType::Leviathan => {
                        if health_pct < 0.25 {
                            direction * creature.speed * 2.5 * rage_mult
                        } else {
                            direction * creature.speed * 1.8 * rage_mult
                        }
                    }
                    _ => direction * creature.speed,
                }
            }
            CreatureAIState::Fleeing => {
                // Flee away from the threat (target position)
                let flee_from = target_pos;
                let direction = (pos - flee_from).normalize_or_zero();
                direction * creature.speed * 1.5
            }
            CreatureAIState::Observing => {
                let to_target = target_pos - pos;
                let dist = to_target.length();
                match creature.creature_type {
                    CreatureType::LureFish => {
                        if dist > 100.0 {
                            to_target.normalize_or_zero() * creature.speed * 0.3
                        } else {
                            let perp = Vec2::new(-to_target.y, to_target.x).normalize_or_zero();
                            perp * creature.speed * 0.15
                        }
                    }
                    CreatureType::Watcher => {
                        let perp = Vec2::new(-to_target.y, to_target.x).normalize_or_zero();
                        let radial = if dist < 150.0 {
                            (pos - target_pos).normalize_or_zero() * creature.speed * 0.3
                        } else {
                            Vec2::ZERO
                        };
                        perp * creature.speed * 0.5 + radial
                    }
                    _ => {
                        let perp = Vec2::new(-to_target.y, to_target.x).normalize_or_zero();
                        perp * creature.speed * 0.5
                    }
                }
            }
            CreatureAIState::Feeding => {
                // Slow approach to corpse/food
                let direction = (target_pos - pos).normalize_or_zero();
                let dist = pos.distance(target_pos);
                if dist > 30.0 {
                    direction * creature.speed * 0.3
                } else {
                    Vec2::ZERO // Stay at food
                }
            }
            CreatureAIState::Patrolling => {
                // Circle around territory center (home_position)
                let to_home = ai.home_position - pos;
                let dist = to_home.length();
                let perp = Vec2::new(-to_home.y, to_home.x).normalize_or_zero();
                if dist > 200.0 {
                    // Drift back toward center
                    (to_home.normalize_or_zero() * 0.5 + perp * 0.5).normalize_or_zero() * creature.speed * 0.4
                } else {
                    perp * creature.speed * 0.4
                }
            }
            CreatureAIState::Migrating => {
                // Move toward migration waypoint
                let direction = (target_pos - pos).normalize_or_zero();
                direction * creature.speed * 0.7
            }
            CreatureAIState::Investigating => {
                // Follow noise trail toward highest intensity point
                let direction = (target_pos - pos).normalize_or_zero();
                direction * creature.speed * 0.6
            }
            CreatureAIState::Wandering => {
                let direction = (ai.home_position - pos).normalize_or_zero();
                direction * creature.speed * 0.3
            }
            CreatureAIState::Idle => Vec2::ZERO,
        };

        // Lerp speed varies: ambush lunge is snappy, leviathan is heavy
        let lerp_factor = match creature.creature_type {
            CreatureType::Ambusher if matches!(ai.state, CreatureAIState::Attacking) => 8.0,
            CreatureType::Leviathan => 0.8,
            _ => 2.0,
        };

        velocity.0 = velocity.0.lerp(target_velocity, lerp_factor * time.delta_seconds());
        transform.translation.x += velocity.0.x * time.delta_seconds();
        transform.translation.y += velocity.0.y * time.delta_seconds();

        // Rotate sprite to face movement direction
        if velocity.0.length_squared() > 1.0 {
            let angle = velocity.0.y.atan2(velocity.0.x);
            transform.rotation = Quat::from_rotation_z(angle);
        }
    }
}

/// Creature attack system - damages submarine only when targeting submarine
fn creature_attack_system(
    time: Res<Time>,
    sub_query: Query<(Entity, &Transform), With<Submarine>>,
    mut creature_query: Query<(Entity, &Transform, &Creature, &CreatureAI, &mut AttackCooldown), Without<Submarine>>,
    ai_sub_query: Query<(Entity, &Transform), (With<AiSubmarine>, Without<Submarine>)>,
    mut damage_events: EventWriter<SubmarineDamaged>,
    mut ai_damage_events: EventWriter<AiSubDamaged>,
    mut attacking_events: EventWriter<CreatureAttacking>,
    mut notifications: EventWriter<ShowNotification>,
) {
    let Ok((sub_entity, sub_transform)) = sub_query.get_single() else { return };
    let sub_pos = sub_transform.translation.truncate();

    for (creature_entity, transform, creature, ai, mut cooldown) in creature_query.iter_mut() {
        if !matches!(ai.state, CreatureAIState::Attacking) {
            continue;
        }

        cooldown.timer.tick(time.delta());
        if !cooldown.timer.finished() {
            continue;
        }

        let creature_pos = transform.translation.truncate();

        // Wounded creatures deal +25% damage
        let health_pct = if creature.max_health > 0.0 { creature.health / creature.max_health } else { 1.0 };
        let rage_damage = if health_pct < 0.4 { creature.damage * 1.25 } else { creature.damage };

        // Check if targeting player submarine
        if matches!(ai.target, Some(EcoTarget::Submarine(_))) {
            let dist = creature_pos.distance(sub_pos);
            if dist > 100.0 {
                continue;
            }

            cooldown.timer.reset();

            attacking_events.send(CreatureAttacking {
                creature: creature_entity,
                target: sub_entity,
            });

            damage_events.send(SubmarineDamaged {
                source: DamageSource::Creature(creature_entity),
                amount: rage_damage,
                position: Some(creature_pos),
                direction: Some((creature_pos - sub_pos).normalize_or_zero()),
            });

            notifications.send(ShowNotification {
                message: format!("{:?} attacks! ({:.0} damage)", creature.creature_type, rage_damage),
                notification_type: NotificationType::Danger,
                duration: 2.0,
            });
        }
        // Check if targeting an AI submarine
        else if let Some(EcoTarget::AiSubmarine(ai_sub_entity)) = ai.target {
            if let Ok((ai_e, ai_transform)) = ai_sub_query.get(ai_sub_entity) {
                let ai_pos = ai_transform.translation.truncate();
                let dist = creature_pos.distance(ai_pos);
                if dist > 100.0 {
                    continue;
                }

                cooldown.timer.reset();

                ai_damage_events.send(AiSubDamaged {
                    target: ai_e,
                    source: DamageSource::Creature(Entity::PLACEHOLDER),
                    amount: rage_damage,
                    position: Some(creature_pos),
                    direction: Some((creature_pos - ai_pos).normalize_or_zero()),
                });
            }
        }
    }
}

/// Watcher scout mechanic: when a Watcher enters Observing state, it alerts nearby creatures
fn watcher_alert_system(
    mut creature_query: Query<(Entity, &Transform, &Creature, &mut CreatureAI)>,
    submarine_query: Query<Entity, With<Submarine>>,
    mut notifications: EventWriter<ShowNotification>,
    mut alerted_this_dive: Local<bool>,
) {
    let Ok(_sub_entity) = submarine_query.get_single() else { return };

    // Reset alert flag when no creatures exist (new dive after cleanup)
    if creature_query.is_empty() {
        *alerted_this_dive = false;
        return;
    }

    // Collect watcher positions and targets first
    let watchers: Vec<(Vec2, EcoTarget)> = creature_query
        .iter()
        .filter(|(_, _, c, ai)| {
            c.creature_type == CreatureType::Watcher
                && matches!(ai.state, CreatureAIState::Observing)
        })
        .filter_map(|(_, t, _, ai)| ai.target.map(|tgt| (t.translation.truncate(), tgt)))
        .collect();

    if watchers.is_empty() {
        return;
    }

    // Alert nearby creatures for each watcher
    for (watcher_pos, target) in &watchers {
        for (_, transform, creature, mut ai) in creature_query.iter_mut() {
            if creature.creature_type == CreatureType::Watcher {
                continue;
            }
            if !matches!(ai.state, CreatureAIState::Wandering | CreatureAIState::Idle) {
                continue;
            }
            let dist = transform.translation.truncate().distance(*watcher_pos);
            if dist < 400.0 {
                ai.state = CreatureAIState::Hunting;
                ai.target = Some(*target);
            }
        }
    }

    // First-time notification
    if !*alerted_this_dive {
        *alerted_this_dive = true;
        notifications.send(ShowNotification {
            message: "Something is watching you...".into(),
            notification_type: NotificationType::Warning,
            duration: 3.0,
        });
    }
}

/// Checks if creatures detect the submarine and fires events
fn check_creature_detection(
    submarine_query: Query<&Transform, With<Submarine>>,
    creature_query: Query<(Entity, &Transform, &Creature, &CreatureAI)>,
    mut spotted_events: EventWriter<CreatureSpotted>,
    mut detected: Local<std::collections::HashSet<Entity>>,
) {
    let Ok(sub_transform) = submarine_query.get_single() else {
        return;
    };

    for (entity, transform, creature, ai) in creature_query.iter() {
        let distance = transform.translation.truncate()
            .distance(sub_transform.translation.truncate());

        if matches!(ai.state, CreatureAIState::Hunting | CreatureAIState::Attacking) && !detected.contains(&entity) {
            detected.insert(entity);
            spotted_events.send(CreatureSpotted {
                creature: entity,
                creature_type: creature.creature_type,
                distance,
            });
        } else if matches!(ai.state, CreatureAIState::Wandering | CreatureAIState::Idle) {
            detected.remove(&entity);
        }
    }
}

/// ElectricEel: AoE shock that damages sub when attacking at close range
fn electric_eel_shock(
    time: Res<Time>,
    eel_query: Query<(&Transform, &Creature, &CreatureAI), Without<Submarine>>,
    sub_query: Query<&Transform, With<Submarine>>,
    mut damage_events: EventWriter<SubmarineDamaged>,
    mut notifications: EventWriter<ShowNotification>,
    mut shock_timer: Local<f32>,
) {
    *shock_timer += time.delta_seconds();
    if *shock_timer < 3.0 {
        return;
    }

    let Ok(sub_transform) = sub_query.get_single() else { return };
    let sub_pos = sub_transform.translation.truncate();

    for (transform, creature, ai) in eel_query.iter() {
        if creature.creature_type != CreatureType::ElectricEel {
            continue;
        }
        if !matches!(ai.state, CreatureAIState::Attacking | CreatureAIState::Hunting) {
            continue;
        }

        // Only shock submarine if targeting it
        if !matches!(ai.target, Some(EcoTarget::Submarine(_))) {
            continue;
        }

        let dist = transform.translation.truncate().distance(sub_pos);
        let shock_range = 150.0;

        if dist < shock_range {
            let shock_damage = creature.damage * 0.5 * (1.0 - dist / shock_range);
            if shock_damage > 0.5 {
                *shock_timer = 0.0;
                damage_events.send(SubmarineDamaged {
                    source: DamageSource::Creature(Entity::PLACEHOLDER),
                    amount: shock_damage,
                    position: Some(transform.translation.truncate()),
                    direction: Some((transform.translation.truncate() - sub_pos).normalize_or_zero()),
                });
                notifications.send(ShowNotification {
                    message: format!("Electric shock! ({:.0} damage)", shock_damage),
                    notification_type: NotificationType::Danger,
                    duration: 2.0,
                });
                return;
            }
        }
    }
}

/// SwarmQueen: periodically spawns Parasite minions while hunting
fn swarm_queen_spawn(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlases: ResMut<Assets<TextureAtlas>>,
    time: Res<Time>,
    queen_query: Query<(&Transform, &Creature, &CreatureAI)>,
    creature_count: Query<&Creature>,
    mut spawn_timer: Local<f32>,
) {
    *spawn_timer += time.delta_seconds();
    if *spawn_timer < 8.0 {
        return;
    }

    if creature_count.iter().count() >= 15 {
        return;
    }

    for (transform, creature, ai) in queen_query.iter() {
        if creature.creature_type != CreatureType::SwarmQueen {
            continue;
        }
        if !matches!(ai.state, CreatureAIState::Hunting | CreatureAIState::Attacking) {
            continue;
        }

        *spawn_timer = 0.0;
        let queen_pos = transform.translation.truncate();
        for i in 0..2 {
            let offset = Vec2::new(
                if i == 0 { -30.0 } else { 30.0 },
                rand::thread_rng().gen_range(-20.0..20.0),
            );
            spawn_creature(&mut commands, &asset_server, &mut texture_atlases, CreatureType::Parasite, queen_pos + offset);
        }
        return;
    }
}

/// Despawn creatures that are very far from the submarine.
/// Decrements ecosystem population counts. Doesn't despawn migrating creatures.
fn cleanup_distant_creatures(
    mut commands: Commands,
    sub_query: Query<&Transform, With<Submarine>>,
    creature_query: Query<(Entity, &Transform, &Creature, &CreatureAI)>,
    mut eco_state: ResMut<EcosystemState>,
) {
    let Ok(sub_transform) = sub_query.get_single() else { return };
    let sub_pos = sub_transform.translation.truncate();

    for (entity, transform, creature, ai) in creature_query.iter() {
        let dist = transform.translation.truncate().distance(sub_pos);
        if dist > 2000.0 {
            // Don't despawn migrating creatures
            if ai.state == CreatureAIState::Migrating {
                continue;
            }

            // Decrement population count
            if let Some(count) = eco_state.population_counts.get_mut(&creature.creature_type) {
                *count = count.saturating_sub(1);
            }

            commands.entity(entity).despawn_recursive();
        }
    }
}

/// Animate creature sprite sheets — cycles swim frames normally,
/// switches to attack frames when in Attacking state.
fn animate_creature_sprites(
    time: Res<Time>,
    mut query: Query<(
        &mut CreatureAnimation,
        &mut TextureAtlasSprite,
        Option<&CreatureAI>,
    )>,
) {
    for (mut anim, mut sprite, ai) in query.iter_mut() {
        anim.timer.tick(time.delta());
        if !anim.timer.just_finished() {
            continue;
        }

        let attacking = ai
            .map(|a| matches!(a.state, CreatureAIState::Attacking))
            .unwrap_or(false);

        if attacking && anim.attack_frames > 0 {
            // Cycle through attack frames
            let attack_start = anim.swim_frames;
            let attack_end = anim.swim_frames + anim.attack_frames;
            anim.current_frame = if anim.current_frame >= attack_start && anim.current_frame < attack_end - 1 {
                anim.current_frame + 1
            } else {
                attack_start
            };
        } else {
            // Cycle through swim frames
            anim.current_frame = (anim.current_frame + 1) % anim.swim_frames;
        }

        sprite.index = anim.current_frame;
    }
}
