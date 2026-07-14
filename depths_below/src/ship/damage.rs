use bevy::prelude::*;
use rand::Rng;
use crate::components::*;
use crate::events::*;
use crate::building::rooms::RoomMap;
use crate::building::GridOccupancy;
use super::hull::{mix_color, DAMAGE_TINT_TARGET};

/// Internal enum for sorting damage targets along an attack ray
enum DamageTarget {
    Hull { entity: Entity, grid_pos: IVec2, projection: f32 },
    Module { entity: Entity, _grid_pos: IVec2, projection: f32 },
}

/// Consumes ShipDamaged events and applies damage using directional kinetic penetration.
///
/// If direction is available: collect all hull/module tiles, project onto attack ray,
/// walk outermost-first applying penetration damage.
/// If no direction (radiation, explosion): fall back to random hull segment.
/// PLAYER SHIP ONLY: hull/module queries are filtered to the player's own
/// children. Unscoped, the "outermost block first" damage walk collected the
/// ATTACKER's blocks too (they sit right on the attack ray) — enemy fire was
/// being absorbed by the enemy's own ship and never reached the player.
pub fn process_ship_damage(
    mut damage_events: MessageReader<ShipDamaged>,
    mut hull_query: Query<(Entity, &mut HullSegment, &GlobalTransform, &ChildOf)>,
    mut module_query: Query<(Entity, &mut Module, &GlobalTransform, &ChildOf), Without<DestroyedModule>>,
    ship_query: Query<(Entity, &GlobalTransform), With<Ship>>,
    room_map: Res<RoomMap>,
    time: Res<Time>,
    mut death_cause: ResMut<crate::resources::DeathCause>,
    mut breach_events: MessageWriter<HullBreached>,
    mut room_depressurize_events: MessageWriter<RoomDepressurized>,
    mut notifications: MessageWriter<ShowNotification>,
    mut commands: Commands,
) {
    let mut rng = rand::thread_rng();

    let Ok((player_ship, player_gt)) = ship_query.single() else { return };
    let ship_center = player_gt.translation().truncate();

    for event in damage_events.read() {
        // Skip radiation damage — it's handled directly in check_radiation_damage
        if matches!(event.source, DamageSource::Radiation) {
            continue;
        }

        // Remember what hit us — the death screen uses this to attribute
        // hull/crew deaths to their actual source.
        let source_desc = match event.source {
            DamageSource::Creature(_) => "creature attack",
            DamageSource::Collision => "collision",
            DamageSource::Explosion => "explosion",
            DamageSource::Fire => "fire",
            DamageSource::Radiation => unreachable!(),
        };
        death_cause.last_damage = Some((source_desc.to_string(), time.elapsed_secs_f64()));

        if hull_query.is_empty() {
            continue;
        }

        // Determine attack direction
        let direction = event.direction.or_else(|| {
            event.position.map(|pos| (pos - ship_center).normalize_or_zero())
        });

        if let Some(dir) = direction {
            // === DIRECTIONAL DAMAGE WITH PENETRATION ===
            let mut targets: Vec<DamageTarget> = Vec::new();

            // Collect hull targets (player's own hull only)
            for (entity, hull, gt, parent) in hull_query.iter() {
                if parent.parent() != player_ship { continue; }
                let pos = gt.translation().truncate();
                let to_tile = pos - ship_center;
                let projection = to_tile.dot(dir);
                // Filter: only tiles near the attack ray (within 1.5 grid cells perpendicular)
                let perp_dist = (to_tile - dir * projection).length();
                if perp_dist < 1.5 * 66.0 {
                    targets.push(DamageTarget::Hull {
                        entity,
                        grid_pos: hull.grid_position,
                        projection,
                    });
                }
            }

            // Collect module targets (player's own modules only)
            for (entity, module, gt, parent) in module_query.iter() {
                if parent.parent() != player_ship { continue; }
                let pos = gt.translation().truncate();
                let to_tile = pos - ship_center;
                let projection = to_tile.dot(dir);
                let perp_dist = (to_tile - dir * projection).length();
                if perp_dist < 1.5 * 66.0 {
                    targets.push(DamageTarget::Module {
                        entity,
                        _grid_pos: module.grid_position,
                        projection,
                    });
                }
            }

            // Sort outermost-first (highest projection = closest to attacker)
            targets.sort_by(|a, b| {
                let pa = match a { DamageTarget::Hull { projection, .. } | DamageTarget::Module { projection, .. } => *projection };
                let pb = match b { DamageTarget::Hull { projection, .. } | DamageTarget::Module { projection, .. } => *projection };
                pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
            });

            let mut remaining_damage = event.amount;

            for target in &targets {
                if remaining_damage <= 0.0 {
                    break;
                }

                match target {
                    DamageTarget::Hull { entity, grid_pos, .. } => {
                        if let Ok((_, mut hull, _, _)) = hull_query.get_mut(*entity) {
                            let absorbed = hull.material.damage_absorption().min(remaining_damage);
                            hull.health = (hull.health - absorbed).max(0.0);
                            remaining_damage -= absorbed;

                            let health_pct = if hull.max_health > 0.0 {
                                hull.health / hull.max_health
                            } else {
                                0.0
                            };

                            // Breach if health drops below 30%
                            if health_pct < 0.3 && !hull.is_depressurized {
                                hull.is_depressurized = true;
                                breach_events.write(HullBreached {
                                    segment: *entity,
                                    severity: 1.0 - health_pct,
                                });

                                // Send RoomDepressurized if this tile is in a room
                                if let Some(&room_id) = room_map.tile_to_room.get(grid_pos) {
                                    room_depressurize_events.write(RoomDepressurized {
                                        room_id,
                                        severity: 1.0 - health_pct,
                                    });
                                }

                                notifications.write(ShowNotification {
                                    message: "Hull breach! Decompression in progress!".into(),
                                    notification_type: NotificationType::Danger,
                                    duration: 3.0,
                                });
                            }
                        }
                    }
                    DamageTarget::Module { entity, .. } => {
                        if let Ok((_, mut module, _, _)) = module_query.get_mut(*entity) {
                            // Modules take 70% of remaining damage as HP damage
                            let module_damage = remaining_damage * 0.7;
                            module.health = (module.health - module_damage).max(0.0);
                            // Absorb 50% of remaining damage
                            remaining_damage *= 0.5;
                        }
                    }
                }
            }
        } else {
            // === NON-DIRECTIONAL FALLBACK (radiation, explosion, etc.) ===
            // Player hull only — the random pick used to land on AI ships.
            let player_hulls: Vec<Entity> = hull_query.iter()
                .filter(|(_, _, _, parent)| parent.parent() == player_ship)
                .map(|(e, _, _, _)| e)
                .collect();
            if player_hulls.is_empty() { continue; }
            let target = player_hulls[rng.gen_range(0..player_hulls.len())];

            if let Ok((_, mut hull, _, _)) = hull_query.get_mut(target) {
                hull.health = (hull.health - event.amount).max(0.0);

                let health_pct = if hull.max_health > 0.0 {
                    hull.health / hull.max_health
                } else {
                    0.0
                };

                if health_pct < 0.3 && !hull.is_depressurized {
                    hull.is_depressurized = true;
                    breach_events.write(HullBreached {
                        segment: target,
                        severity: 1.0 - health_pct,
                    });

                    if let Some(&room_id) = room_map.tile_to_room.get(&hull.grid_position) {
                        room_depressurize_events.write(RoomDepressurized {
                            room_id,
                            severity: 1.0 - health_pct,
                        });
                    }

                    notifications.write(ShowNotification {
                        message: "Hull breach! Decompression in progress!".into(),
                        notification_type: NotificationType::Danger,
                        duration: 3.0,
                    });
                }
            }

            // 30% chance to also damage the nearest module (legacy behavior for non-directional)
            let hit_pos = event.position.unwrap_or(Vec2::ZERO);
            if event.position.is_some() && rng.gen::<f32>() < 0.3 {
                let closest_module = module_query
                    .iter_mut()
                    .filter(|(_, _, t, parent)| {
                        parent.parent() == player_ship
                            && t.translation().truncate().distance(hit_pos) < 80.0
                    })
                    .min_by(|(_, _, ta, _), (_, _, tb, _)| {
                        let da = ta.translation().truncate().distance(hit_pos);
                        let db = tb.translation().truncate().distance(hit_pos);
                        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                    });

                if let Some((_, mut module, _, _)) = closest_module {
                    let module_damage = event.amount * 0.5;
                    module.health = (module.health - module_damage).max(0.0);
                }
            }
        }

        // Spawn hit spark at damage position
        if let Some(pos) = event.position {
            commands.spawn((
                (Sprite {
                        color: Color::srgba(1.0, 0.4, 0.1, 0.9),
                        custom_size: Some(Vec2::splat(24.0)),
                        ..default()
                    }, Transform::from_xyz(pos.x, pos.y, 0.7)),
                HitEffect {
                    timer: Timer::from_seconds(0.3, TimerMode::Once),
                },
            ));
        }
    }
}

/// Gradual damage tint for modules (player or AI): darkens continuously as
/// health drops from max toward 0. Same reasoning as tint_damaged_hull —
/// blends from the stable spawn-time BaseSpriteColor, never from the live
/// (already-tinted) sprite.color.
pub fn tint_damaged_modules(
    mut module_query: Query<(&Module, &BaseSpriteColor, &mut Sprite), Without<DestroyedModule>>,
) {
    for (module, base, mut sprite) in module_query.iter_mut() {
        if module.max_health <= 0.0 { continue; }
        let damage_frac = 1.0 - (module.health / module.max_health).clamp(0.0, 1.0);
        sprite.color = mix_color(base.0, DAMAGE_TINT_TARGET, damage_frac);
    }
}

/// Processes module destruction — marks destroyed modules with DestroyedModule component.
/// Applies to every ship (AI blocks visibly "break" — dark grey — too), but
/// events/notifications only fire for the player's own modules.
pub fn process_module_destruction(
    mut commands: Commands,
    mut module_query: Query<(Entity, &mut Module, &mut Sprite, &ChildOf), Without<DestroyedModule>>,
    ship_query: Query<Entity, With<Ship>>,
    mut destroy_events: MessageWriter<ModuleDestroyed>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let player_ship = ship_query.single().ok();
    for (entity, mut module, mut sprite, parent) in module_query.iter_mut() {
        if module.health <= 0.0 && module.is_active {
            module.is_active = false;
            module.health = 0.0;
            // try_insert: applies to every ship including AI ships. If this
            // module's ship also lost its reactor this same frame,
            // ai_ship_death_system recursively despawns the whole ship —
            // plain insert() panics if that despawn flushes first.
            commands.entity(entity).try_insert(DestroyedModule {
                original_type: module.module_type,
            });
            sprite.color = DAMAGE_TINT_TARGET;
            if Some(parent.parent()) == player_ship {
                destroy_events.write(ModuleDestroyed { module: entity });
                notifications.write(ShowNotification {
                    message: format!("{} destroyed!", module.module_type.name()),
                    notification_type: NotificationType::Danger,
                    duration: 3.0,
                });
            }
        }
    }
}

/// Queues a freshly-destroyed module for removal. Doesn't despawn directly
/// here — see `PendingRemoval`'s doc comment for why.
pub fn queue_module_removal(
    mut commands: Commands,
    fresh: Query<Entity, Added<DestroyedModule>>,
) {
    for entity in fresh.iter() {
        commands.entity(entity).try_insert(PendingRemoval {
            timer: Timer::from_seconds(0.5, TimerMode::Once),
        });
    }
}

/// Ticks the destruction-to-removal delay and despawns blocks (hull or
/// module, player or AI ship) once it expires — leaving an actual gap
/// instead of an inert dark husk. try_despawn: the block's whole ship may
/// already be gone by now (e.g. a reactor kill recursively despawning
/// everything), in which case this is just a no-op.
pub fn tick_pending_removal(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut PendingRemoval)>,
) {
    for (entity, mut pending) in query.iter_mut() {
        pending.timer.tick(time.delta());
        if pending.timer.is_finished() {
            commands.entity(entity).try_despawn();
        }
    }
}

/// Hit effect that auto-despawns after timer expires.
/// Used for both ship damage sparks and creature hit flashes.
#[derive(Component)]
pub struct HitEffect {
    pub timer: Timer,
}

/// Cleanup system for hit effects
pub fn cleanup_hit_effects(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut HitEffect)>,
) {
    for (entity, mut effect) in query.iter_mut() {
        effect.timer.tick(time.delta());
        if effect.timer.is_finished() {
            commands.entity(entity).despawn();
        }
    }
}

/// When an explosive module is freshly destroyed, queue a detonation with a short fuse delay.
/// Warning notifications only fire for the player's own ship (via ChildOf) — an
/// AI ship's reactor cooking off is not the player's emergency.
pub fn queue_detonation(
    mut commands: Commands,
    query: Query<(Entity, &Module, &Explosive, &ChildOf), Added<DestroyedModule>>,
    ship_query: Query<Entity, With<Ship>>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let player_ship = ship_query.single().ok();
    for (entity, module, explosive, parent) in query.iter() {
        let is_player_ship = Some(parent.parent()) == player_ship;

        // AI ship modules never queue detonations: their grid_position is in
        // THEIR ship's local coordinates, but detonation AoE resolves against
        // the player's GridOccupancy — an AI reactor at its (1,0) would blow
        // up the player's module at (1,0). AI ships have their own
        // hull-integrity damage model.
        if !is_player_ship {
            continue;
        }
        let fuse_secs = match explosive.explosive_type {
            ExplosiveType::Reactor => 0.15,
            ExplosiveType::Ammo => 0.05,
            ExplosiveType::Fuel => 0.2,
            ExplosiveType::Battery => 0.1,
        };

        commands.entity(entity).insert(PendingDetonation {
            timer: Timer::from_seconds(fuse_secs, TimerMode::Once),
            blast_radius: explosive.blast_radius,
            blast_damage: explosive.blast_damage,
            explosive_type: explosive.explosive_type,
            grid_position: module.grid_position,
        });

        let warning = match explosive.explosive_type {
            ExplosiveType::Reactor => "Reactor critical! Explosion imminent!",
            ExplosiveType::Ammo => "Ammo cooking off!",
            ExplosiveType::Fuel => "Fuel tank rupture! Fire risk!",
            ExplosiveType::Battery => "Battery overload!",
        };
        notifications.write(ShowNotification {
            message: warning.into(),
            notification_type: NotificationType::Danger,
            duration: 3.0,
        });
    }
}

/// Ticks pending detonation timers and applies AoE damage when they finish.
pub fn process_detonations(
    mut commands: Commands,
    time: Res<Time>,
    mut det_query: Query<(Entity, &mut PendingDetonation)>,
    mut module_query: Query<(Entity, &mut Module), Without<DestroyedModule>>,
    mut hull_query: Query<(Entity, &mut HullSegment), Without<HullDestroyed>>,
    occupancy: Res<GridOccupancy>,
    mut explosion_events: MessageWriter<ModuleExploded>,
    mut fire_events: MessageWriter<FireStarted>,
    mut breach_events: MessageWriter<HullBreached>,
    mut hull_destroy_events: MessageWriter<HullSegmentDestroyed>,
    room_map: Res<RoomMap>,
    mut room_depressurize_events: MessageWriter<RoomDepressurized>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    // Collect finished detonations first to avoid borrow issues
    let mut finished: Vec<(Entity, PendingDetonation)> = Vec::new();

    for (entity, mut det) in det_query.iter_mut() {
        det.timer.tick(time.delta());
        if det.timer.is_finished() {
            finished.push((entity, PendingDetonation {
                timer: det.timer.clone(),
                blast_radius: det.blast_radius,
                blast_damage: det.blast_damage,
                explosive_type: det.explosive_type,
                grid_position: det.grid_position,
            }));
        }
    }

    for (det_entity, det) in finished {
        // Remove the PendingDetonation component
        commands.entity(det_entity).remove::<PendingDetonation>();

        let radius_cells = det.blast_radius;
        let radius_i = radius_cells.ceil() as i32;

        // Scan grid cells within blast radius
        for dx in -radius_i..=radius_i {
            for dy in -radius_i..=radius_i {
                let target_pos = det.grid_position + IVec2::new(dx, dy);
                let dist = ((dx * dx + dy * dy) as f32).sqrt();
                if dist > radius_cells {
                    continue;
                }
                // Skip self (center)
                if dx == 0 && dy == 0 {
                    continue;
                }

                // Damage falloff: full damage at center, 30% at edge
                let falloff = 1.0 - (dist / radius_cells) * 0.7;
                let damage = det.blast_damage * falloff;

                // Try to damage a module at this position
                if let Some(&target_entity) = occupancy.cells.get(&target_pos) {
                    if let Ok((_, mut target_module)) = module_query.get_mut(target_entity) {
                        target_module.health = (target_module.health - damage).max(0.0);
                    }
                }

                // Try to damage hull at this position
                for (hull_entity, mut hull) in hull_query.iter_mut() {
                    if hull.grid_position == target_pos {
                        hull.health = (hull.health - damage).max(0.0);

                        let health_pct = if hull.max_health > 0.0 {
                            hull.health / hull.max_health
                        } else {
                            0.0
                        };

                        if hull.health <= 0.0 {
                            hull_destroy_events.write(HullSegmentDestroyed {
                                segment: hull_entity,
                                grid_position: target_pos,
                            });
                        } else if health_pct < 0.3 && !hull.is_depressurized {
                            hull.is_depressurized = true;
                            breach_events.write(HullBreached {
                                segment: hull_entity,
                                severity: 1.0 - health_pct,
                            });
                            if let Some(&room_id) = room_map.tile_to_room.get(&target_pos) {
                                room_depressurize_events.write(RoomDepressurized {
                                    room_id,
                                    severity: 1.0 - health_pct,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Fuel/Battery explosions start fires on 4-adjacent non-destroyed modules
        if matches!(det.explosive_type, ExplosiveType::Fuel | ExplosiveType::Battery | ExplosiveType::Ammo) {
            for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                let adj_pos = det.grid_position + offset;
                if let Some(&adj_entity) = occupancy.cells.get(&adj_pos) {
                    if module_query.get(adj_entity).is_ok() {
                        fire_events.write(FireStarted {
                            module: adj_entity,
                            grid_position: adj_pos,
                            intensity: 0.8,
                        });
                    }
                }
            }
        }

        // Send explosion event
        explosion_events.write(ModuleExploded {
            grid_position: det.grid_position,
            blast_damage: det.blast_damage,
            explosive_type: det.explosive_type,
        });

        // Spawn explosion visual (orange HitEffect, larger and longer)
        let world_pos = Vec3::new(
            det.grid_position.x as f32 * 66.0,
            det.grid_position.y as f32 * 66.0 - 33.0,
            0.8,
        );
        commands.spawn((
            (Sprite {
                    color: Color::srgba(1.0, 0.6, 0.1, 0.95),
                    custom_size: Some(Vec2::splat(det.blast_radius * 66.0)),
                    ..default()
                }, Transform::from_translation(world_pos)),
            HitEffect {
                timer: Timer::from_seconds(0.5, TimerMode::Once),
            },
        ));

        notifications.write(ShowNotification {
            message: format!("EXPLOSION at ({}, {})!", det.grid_position.x, det.grid_position.y),
            notification_type: NotificationType::Danger,
            duration: 3.0,
        });
    }
}

// ============================================================================
// AI SHIP DETONATIONS
// The player-ship path above resolves blasts through GridOccupancy, which
// only knows the PLAYER's grid — that's why queue_detonation skips AI ships.
// This pair resolves against the AI ship's own child blocks in world space
// instead, so shooting out an enemy's cannon cooks off ITS ammo and chains
// into ITS neighbors.
// ============================================================================

/// A queued explosion on an AI ship's module (world-space resolution).
#[derive(Component)]
pub struct AiPendingDetonation {
    pub timer: Timer,
    pub blast_radius_world: f32,
    pub blast_damage: f32,
    pub explosive_type: ExplosiveType,
    pub position: Vec2,
    pub ship: Entity,
}

/// Freshly destroyed explosive module on an AI ship → short fuse.
pub fn queue_ai_detonation(
    mut commands: Commands,
    query: Query<(Entity, &Explosive, &GlobalTransform, &ChildOf), Added<DestroyedModule>>,
    ai_ships: Query<(), With<crate::ai_ship::components::AiShip>>,
) {
    for (entity, explosive, gt, parent) in query.iter() {
        if ai_ships.get(parent.parent()).is_err() {
            continue; // player ship handled by queue_detonation above
        }
        let fuse_secs = match explosive.explosive_type {
            ExplosiveType::Reactor => 0.15,
            ExplosiveType::Ammo => 0.05,
            ExplosiveType::Fuel => 0.2,
            ExplosiveType::Battery => 0.1,
        };
        commands.entity(entity).try_insert(AiPendingDetonation {
            timer: Timer::from_seconds(fuse_secs, TimerMode::Once),
            blast_radius_world: explosive.blast_radius * 66.0,
            blast_damage: explosive.blast_damage,
            explosive_type: explosive.explosive_type,
            position: gt.translation().truncate(),
            ship: parent.parent(),
        });
    }
}

/// Ticks AI detonation fuses; on boom, damages every block of that ship in
/// radius (with falloff) and sets survivors near the center burning.
pub fn process_ai_detonations(
    mut commands: Commands,
    time: Res<Time>,
    mut det_query: Query<(Entity, &mut AiPendingDetonation)>,
    children_query: Query<&Children>,
    mut module_query: Query<
        (&mut Module, &GlobalTransform),
        (Without<DestroyedModule>, With<crate::ai_ship::components::OwnedByAiShip>),
    >,
    mut hull_query: Query<
        (&mut HullSegment, &GlobalTransform),
        (Without<HullDestroyed>, With<crate::ai_ship::components::OwnedByAiShip>),
    >,
    mut ai_damage_events: MessageWriter<AiShipDamaged>,
    mut boom_events: MessageWriter<crate::events::AiModuleExploded>,
) {
    for (det_entity, mut det) in det_query.iter_mut() {
        det.timer.tick(time.delta());
        if !det.timer.is_finished() { continue; }
        commands.entity(det_entity).remove::<AiPendingDetonation>();

        let Ok(children) = children_query.get(det.ship) else { continue };
        // Fires start on surviving blocks close to the blast center.
        let fire_radius = det.blast_radius_world * 0.6;
        let starts_fires = matches!(det.explosive_type,
            ExplosiveType::Ammo | ExplosiveType::Fuel | ExplosiveType::Battery);

        for child in children.iter() {
            if child == det_entity { continue; }
            let (block_pos, dealt) = if let Ok((mut module, gt)) = module_query.get_mut(child) {
                let pos = gt.translation().truncate();
                let dist = det.position.distance(pos);
                if dist > det.blast_radius_world { continue; }
                let falloff = 1.0 - (dist / det.blast_radius_world) * 0.7;
                let damage = det.blast_damage * falloff;
                module.health = (module.health - damage).max(0.0);
                (pos, damage)
            } else if let Ok((mut hull, gt)) = hull_query.get_mut(child) {
                let pos = gt.translation().truncate();
                let dist = det.position.distance(pos);
                if dist > det.blast_radius_world { continue; }
                let falloff = 1.0 - (dist / det.blast_radius_world) * 0.7;
                let damage = det.blast_damage * falloff;
                hull.health = (hull.health - damage).max(0.0);
                (pos, damage)
            } else {
                continue;
            };

            crate::combat::spawn_floating_damage(
                &mut commands, block_pos, dealt, Color::srgb(1.0, 0.5, 0.15),
            );
            if starts_fires && det.position.distance(block_pos) < fire_radius {
                commands.entity(child).try_insert(
                    crate::combat::new_projectiles::BlockBurning {
                        dps: det.blast_damage * 0.1,
                        remaining: 6.0,
                        ship: det.ship,
                    },
                );
            }
        }

        // Explosion visual — reuses HitEffect like the player-side blast
        commands.spawn((
            (Sprite {
                    color: Color::srgba(1.0, 0.55, 0.1, 0.95),
                    custom_size: Some(Vec2::splat(det.blast_radius_world * 2.0)),
                    ..default()
                }, Transform::from_translation(det.position.extend(0.8))),
            HitEffect {
                timer: Timer::from_seconds(0.5, TimerMode::Once),
            },
        ));

        ai_damage_events.write(AiShipDamaged {
            target: det.ship,
            source: DamageSource::Explosion,
            amount: 0.0, // damage already applied block-by-block above
            position: Some(det.position),
            direction: None,
        });
        boom_events.write(crate::events::AiModuleExploded {
            position: det.position,
            blast_damage: det.blast_damage,
        });
    }
}

// ============================================================================
// EXPLOSION SHOCKWAVES
// Real detonations (cook-offs, death-rattle pops, final booms) give nearby
// ships a soft radial shove. Deliberately subtle — it should read as feel,
// never wrestle aim away from the player. Set SHOCKWAVE_SCALE to 0.0 to
// disable outright.
// ============================================================================

const SHOCKWAVE_SCALE: f32 = 1.0;
/// Max velocity kick (world units/s) any single blast can impart.
const SHOCKWAVE_MAX_KICK: f32 = 110.0;

/// Applies radial impulse from AI-ship detonations (and the player's own
/// module cook-offs) to every ship in range, falling off linearly.
pub fn explosion_shockwaves(
    mut ai_booms: MessageReader<crate::events::AiModuleExploded>,
    mut player_booms: MessageReader<ModuleExploded>,
    mut ship_query: Query<
        (&GlobalTransform, &mut Velocity),
        Or<(With<Ship>, With<crate::ai_ship::components::AiShip>)>,
    >,
    player_gt_query: Query<&GlobalTransform, With<Ship>>,
) {
    if SHOCKWAVE_SCALE <= 0.0 {
        ai_booms.clear();
        player_booms.clear();
        return;
    }

    let mut blasts: Vec<(Vec2, f32)> = Vec::new();
    for ev in ai_booms.read() {
        blasts.push((ev.position, ev.blast_damage));
    }
    // Player-side ModuleExploded only carries a ship-local grid position —
    // rotate it into world space through the ship's transform.
    if let Ok(player_gt) = player_gt_query.single() {
        for ev in player_booms.read() {
            let local = Vec3::new(
                ev.grid_position.x as f32 * 66.0,
                ev.grid_position.y as f32 * 66.0 - 33.0,
                0.0,
            );
            let world = player_gt.transform_point(local).truncate();
            blasts.push((world, ev.blast_damage));
        }
    }
    if blasts.is_empty() { return; }

    for (blast_pos, blast_damage) in blasts {
        let shock_radius = 250.0 + blast_damage * 2.0;
        let center_kick = (blast_damage * 1.2 * SHOCKWAVE_SCALE).min(SHOCKWAVE_MAX_KICK);

        for (gt, mut velocity) in ship_query.iter_mut() {
            let ship_pos = gt.translation().truncate();
            let offset = ship_pos - blast_pos;
            let dist = offset.length();
            if dist > shock_radius { continue; }
            let dir = offset.normalize_or_zero();
            if dir == Vec2::ZERO { continue; }
            let falloff = 1.0 - dist / shock_radius;
            velocity.0 += dir * center_kick * falloff;
        }
    }
}
