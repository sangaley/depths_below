use bevy::prelude::*;
use rand::Rng;

use crate::components::*;
use crate::events::*;
use crate::resources::*;
use crate::ai_ship::components::AiShipWreck;

// ============================================================================
// EVA SALVAGE — Cosmoteer-style crew looting.
// F near a wreck dispatches a detail of idle crew: they leave the ship,
// fly to the hulk, pry a unit of cargo loose, and ferry it home, trip
// after trip, until the wreck is stripped or they're recalled (F again).
// Loot is only banked when a crew member makes it back aboard — a crew
// death mid-ferry loses whatever they were carrying, and wreck
// explosions (death rattles, cook-offs on hot wrecks) hurt crew working
// the hulk. This replaces the old instant press-F-to-loot flow.
// ============================================================================

/// How close the ship must be to a wreck to dispatch a detail.
const ORDER_RANGE: f32 = 420.0;
/// Ship drifting further than this from the wreck recalls the detail.
const BREAK_RANGE: f32 = 750.0;
/// Crew stranded further than this from the ship emergency-board instantly.
const TELEPORT_RANGE: f32 = 1300.0;
const EVA_SPEED: f32 = 130.0;
const GRAB_SECONDS: f32 = 1.0;
const ARRIVE_WRECK: f32 = 16.0;
const ARRIVE_SHIP: f32 = 60.0;
const DETAIL_SIZE: usize = 3;
/// Blast radius/scale for wreck explosions hitting EVA crew. Suits soak
/// half the blast — a final-boom at point blank hurts badly but is
/// survivable for a healthy crew member.
const BLAST_RADIUS: f32 = 90.0;
const BLAST_SCALE: f32 = 0.5;

const EVA_TINT: Color = Color::srgb(0.75, 0.85, 1.0);

pub enum EvaPhase {
    Outbound,
    Grabbing(Timer),
    Returning,
}

/// A crew member currently on salvage EVA. While this is present the
/// normal in-ship crew systems (needs, room mapping, AI, reparenting)
/// leave the crew member alone — the suit keeps them alive out there.
#[derive(Component)]
pub struct EvaSalvaging {
    pub wreck: Entity,
    /// Block (or wreck root) this crew member is flying at. Re-rolled
    /// every trip; if it despawns mid-flight we fall back to the root.
    pub grab_target: Entity,
    pub phase: EvaPhase,
    pub carrying: Option<ItemType>,
    pub recalled: bool,
    /// Local transform aboard ship, restored when they board.
    pub home_local: Vec3,
    pub base_color: Color,
}

/// F key: dispatch a salvage detail at the nearest lootable wreck in
/// range, or recall the detail that's already out.
pub fn order_salvage_detail(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    ship_query: Query<&GlobalTransform, With<Ship>>,
    wreck_query: Query<(Entity, &GlobalTransform, &Wreck, &PointOfInterest)>,
    mut active_eva: Query<&mut EvaSalvaging>,
    mut crew_query: Query<
        (Entity, &mut CrewMember, &mut Transform, &GlobalTransform, &mut Sprite),
        Without<EvaSalvaging>,
    >,
    mut station_query: Query<(Entity, &mut CrewStation)>,
    children_query: Query<&Children>,
    block_filter: Query<(), Or<(With<Module>, With<HullSegment>)>>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    if !keyboard.just_pressed(KeyCode::KeyF) {
        return;
    }

    // A detail is already out — F recalls it.
    if !active_eva.is_empty() {
        for mut eva in active_eva.iter_mut() {
            eva.recalled = true;
        }
        notifications.write(ShowNotification {
            message: "Salvage detail recalled.".into(),
            notification_type: NotificationType::Info,
            duration: 2.0,
        });
        return;
    }

    let Ok(ship_gt) = ship_query.single() else { return };
    let ship_pos = ship_gt.translation().truncate();

    // Nearest wreck with loot left, in dispatch range
    let mut best: Option<(Entity, f32)> = None;
    for (entity, gt, wreck, poi) in wreck_query.iter() {
        if poi.poi_type != PoiType::Wreck || wreck.loot_remaining == 0 {
            continue;
        }
        let dist = ship_pos.distance(gt.translation().truncate());
        if dist < ORDER_RANGE && best.map_or(true, |(_, d)| dist < d) {
            best = Some((entity, dist));
        }
    }
    let Some((wreck_entity, _)) = best else { return };

    // Manually-assigned crew hold their posts. Auto-assigned crew are fair
    // game — the auto-assigner staffs every free hand within seconds, so
    // "unassigned idler" is an empty set on any crewed ship. Pull them the
    // way emergency dispatch does; auto-assign restaffs once they board.
    let mut manual: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    let mut auto_assigned: std::collections::HashMap<Entity, Entity> = std::collections::HashMap::new();
    for (station_entity, station) in station_query.iter() {
        if let Some(crew_entity) = station.assigned_crew {
            if station.manually_assigned {
                manual.insert(crew_entity);
            } else {
                auto_assigned.insert(crew_entity, station_entity);
            }
        }
    }

    let surviving_blocks: Vec<Entity> = children_query
        .get(wreck_entity)
        .map(|children| {
            children
                .iter()
                .filter(|c| block_filter.get(*c).is_ok())
                .collect()
        })
        .unwrap_or_default();

    let mut rng = rand::thread_rng();
    let mut dispatched = 0usize;
    for (entity, mut crew, mut transform, gt, mut sprite) in crew_query.iter_mut() {
        if dispatched >= DETAIL_SIZE {
            break;
        }
        if crew.health <= 0.0 || crew.state != CrewState::Idle || manual.contains(&entity) {
            continue;
        }
        if let Some(&station_entity) = auto_assigned.get(&entity) {
            if let Ok((_, mut station)) = station_query.get_mut(station_entity) {
                station.assigned_crew = None;
            }
        }

        let home_local = transform.translation;
        let world = gt.translation();
        transform.translation = Vec3::new(world.x, world.y, world.z + 1.0);
        transform.rotation = Quat::IDENTITY;
        crew.state = CrewState::Salvaging;
        let base_color = sprite.color;
        sprite.color = EVA_TINT;

        let grab_target = if surviving_blocks.is_empty() {
            wreck_entity
        } else {
            surviving_blocks[rng.gen_range(0..surviving_blocks.len())]
        };

        commands
            .entity(entity)
            .remove::<ChildOf>()
            .remove::<CrewRoomLocation>()
            .insert(EvaSalvaging {
                wreck: wreck_entity,
                grab_target,
                phase: EvaPhase::Outbound,
                carrying: None,
                recalled: false,
                home_local,
                base_color,
            });
        dispatched += 1;
    }

    if dispatched > 0 {
        notifications.write(ShowNotification {
            message: format!("Salvage detail EVA: {} crew (F to recall)", dispatched),
            notification_type: NotificationType::Info,
            duration: 2.5,
        });
    } else {
        notifications.write(ShowNotification {
            message: "No idle crew for salvage duty.".into(),
            notification_type: NotificationType::Warning,
            duration: 2.5,
        });
    }
}

/// Flies each EVA crew member through their trip loop:
/// outbound → grab (1s dwell at the hulk) → return → deposit → repeat.
pub fn run_salvage_detail(
    time: Res<Time>,
    mut commands: Commands,
    ship_query: Query<(Entity, &GlobalTransform), With<Ship>>,
    mut eva_query: Query<(Entity, &mut Transform, &mut CrewMember, &mut EvaSalvaging, &mut Sprite)>,
    mut wreck_query: Query<(&GlobalTransform, &mut Wreck, &mut PointOfInterest, Option<&AiShipWreck>)>,
    target_gt_query: Query<&GlobalTransform>,
    children_query: Query<&Children>,
    block_filter: Query<(), Or<(With<Module>, With<HullSegment>)>>,
    mut inventory: ResMut<Inventory>,
    mut statistics: ResMut<Statistics>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let Ok((ship_entity, ship_gt)) = ship_query.single() else { return };
    let ship_pos = ship_gt.translation().truncate();
    let dt = time.delta_secs();
    let mut rng = rand::thread_rng();

    let generic_loot = [
        ItemType::ScrapMetal,
        ItemType::Crystal,
        ItemType::FuelCell,
        ItemType::RareAlloy,
        ItemType::AmmoCrate,
    ];

    for (entity, mut transform, mut crew, eva, mut sprite) in eva_query.iter_mut() {
        let eva = eva.into_inner();
        if crew.health <= 0.0 {
            continue; // death event handling despawns them
        }

        // Ship drifted too far from the worksite — break off.
        if !eva.recalled {
            if let Ok((wreck_gt, ..)) = wreck_query.get_mut(eva.wreck) {
                if ship_pos.distance(wreck_gt.translation().truncate()) > BREAK_RANGE {
                    eva.recalled = true;
                    notifications.write(ShowNotification {
                        message: "Salvage detail breaking off — out of range.".into(),
                        notification_type: NotificationType::Warning,
                        duration: 2.5,
                    });
                }
            }
        }

        match &mut eva.phase {
            EvaPhase::Outbound => {
                if eva.recalled || wreck_query.get_mut(eva.wreck).is_err() {
                    eva.phase = EvaPhase::Returning;
                    continue;
                }
                // Follow the grab target; fall back to the wreck root if
                // that block burned away or popped since we launched.
                let target = target_gt_query
                    .get(eva.grab_target)
                    .or_else(|_| target_gt_query.get(eva.wreck));
                let Ok(target_gt) = target else {
                    eva.phase = EvaPhase::Returning;
                    continue;
                };
                let target_pos = target_gt.translation().truncate();
                if fly_towards(&mut transform, target_pos, dt) < ARRIVE_WRECK {
                    eva.phase = EvaPhase::Grabbing(Timer::from_seconds(GRAB_SECONDS, TimerMode::Once));
                }
            }
            EvaPhase::Grabbing(timer) => {
                timer.tick(time.delta());
                if !timer.is_finished() {
                    continue;
                }
                if let Ok((_, mut wreck, mut poi, ai_wreck)) = wreck_query.get_mut(eva.wreck) {
                    if wreck.loot_remaining > 0 {
                        wreck.loot_remaining -= 1;
                        eva.carrying = Some(match ai_wreck {
                            Some(aw) => crate::ai_ship::wreck::roll_wreck_loot(
                                aw.ship_type,
                                aw.intact_frac,
                                &mut rng,
                            ),
                            None => generic_loot[rng.gen_range(0..generic_loot.len())],
                        });
                        if wreck.loot_remaining == 0 {
                            poi.discovered = true;
                            wreck.is_explored = true;
                            statistics.wrecks_salvaged += 1;
                            notifications.write(ShowNotification {
                                message: "Wreck stripped — detail returning.".into(),
                                notification_type: NotificationType::Info,
                                duration: 3.0,
                            });
                        }
                    }
                }
                eva.phase = EvaPhase::Returning;
            }
            EvaPhase::Returning => {
                let dist = fly_towards(&mut transform, ship_pos, dt);
                if dist > TELEPORT_RANGE {
                    // Stranded — emergency board, drop nothing (they made it).
                    deposit(eva, &mut inventory, &mut notifications);
                    board_crew(&mut commands, ship_entity, entity, eva, &mut transform, &mut crew, &mut sprite);
                    continue;
                }
                if dist >= ARRIVE_SHIP {
                    continue;
                }

                deposit(eva, &mut inventory, &mut notifications);

                // Another trip, or board?
                let more_work = !eva.recalled
                    && wreck_query
                        .get_mut(eva.wreck)
                        .map(|(gt, wreck, ..)| {
                            wreck.loot_remaining > 0
                                && ship_pos.distance(gt.translation().truncate()) < BREAK_RANGE
                        })
                        .unwrap_or(false);

                if more_work {
                    let blocks: Vec<Entity> = children_query
                        .get(eva.wreck)
                        .map(|children| {
                            children.iter().filter(|c| block_filter.get(*c).is_ok()).collect()
                        })
                        .unwrap_or_default();
                    eva.grab_target = if blocks.is_empty() {
                        eva.wreck
                    } else {
                        blocks[rng.gen_range(0..blocks.len())]
                    };
                    eva.phase = EvaPhase::Outbound;
                } else {
                    board_crew(&mut commands, ship_entity, entity, eva, &mut transform, &mut crew, &mut sprite);
                }
            }
        }
    }
}

/// Wreck explosions (death rattles, hot-wreck cook-offs) hit crew working
/// the hulk. This is the risk that makes "loot it while it burns" a real
/// decision — the fire isn't just eating cargo, it's shooting at your people.
pub fn eva_blast_damage(
    mut explosions: MessageReader<AiModuleExploded>,
    mut eva_query: Query<(Entity, &Transform, &mut CrewMember), With<EvaSalvaging>>,
    mut damage_events: MessageWriter<CrewDamaged>,
    mut death_events: MessageWriter<CrewDied>,
) {
    for explosion in explosions.read() {
        for (entity, transform, mut crew) in eva_query.iter_mut() {
            if crew.health <= 0.0 {
                continue;
            }
            let dist = transform.translation.truncate().distance(explosion.position);
            if dist > BLAST_RADIUS {
                continue;
            }
            let damage = explosion.blast_damage * (1.0 - dist / BLAST_RADIUS) * BLAST_SCALE;
            if damage <= 0.0 {
                continue;
            }
            crew.health -= damage;
            damage_events.write(CrewDamaged {
                crew: entity,
                amount: damage,
                source: CrewDamageSource::Explosion,
            });
            if crew.health <= 0.0 {
                death_events.write(CrewDied {
                    crew: entity,
                    name: crew.name.clone(),
                    cause: CrewDamageSource::Explosion,
                });
            }
        }
    }
}

/// Leaving Exploring (docking, game over, menu) instantly boards every
/// EVA crew member so nobody is left floating in a state we don't simulate.
pub fn abort_eva_on_exit(
    mut commands: Commands,
    ship_query: Query<Entity, With<Ship>>,
    mut eva_query: Query<(Entity, &mut Transform, &mut CrewMember, &mut EvaSalvaging, &mut Sprite)>,
    mut inventory: ResMut<Inventory>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let Ok(ship) = ship_query.single() else { return };
    for (entity, mut transform, mut crew, mut eva, mut sprite) in eva_query.iter_mut() {
        deposit(&mut eva, &mut inventory, &mut notifications);
        board_crew(&mut commands, ship, entity, &eva, &mut transform, &mut crew, &mut sprite);
    }
}

/// Moves toward `target` at EVA speed, returns remaining distance.
fn fly_towards(transform: &mut Transform, target: Vec2, dt: f32) -> f32 {
    let pos = transform.translation.truncate();
    let delta = target - pos;
    let dist = delta.length();
    if dist > 1.0 {
        let step = (EVA_SPEED * dt).min(dist);
        let dir = delta / dist;
        transform.translation.x += dir.x * step;
        transform.translation.y += dir.y * step;
    }
    dist
}

/// Banks whatever this crew member is carrying into the cargo hold.
fn deposit(
    eva: &mut EvaSalvaging,
    inventory: &mut Inventory,
    notifications: &mut MessageWriter<ShowNotification>,
) {
    let Some(item) = eva.carrying.take() else { return };
    if inventory.add_item(item, 1) {
        notifications.write(ShowNotification {
            message: format!("Crew salvaged {}", item.name()),
            notification_type: NotificationType::Success,
            duration: 2.0,
        });
    } else {
        eva.recalled = true;
        notifications.write(ShowNotification {
            message: format!("Cargo hold full — {} jettisoned!", item.name()),
            notification_type: NotificationType::Warning,
            duration: 3.0,
        });
    }
}

fn board_crew(
    commands: &mut Commands,
    ship: Entity,
    entity: Entity,
    eva: &EvaSalvaging,
    transform: &mut Transform,
    crew: &mut CrewMember,
    sprite: &mut Sprite,
) {
    transform.translation = eva.home_local;
    transform.rotation = Quat::IDENTITY;
    sprite.color = eva.base_color;
    if crew.health > 0.0 {
        crew.state = CrewState::Idle;
    }
    commands
        .entity(entity)
        .insert(ChildOf(ship))
        .remove::<EvaSalvaging>();
}
