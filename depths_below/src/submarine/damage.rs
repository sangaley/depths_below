use bevy::prelude::*;
use rand::Rng;
use crate::components::*;
use crate::events::*;
use crate::building::rooms::RoomMap;
use crate::building::GridOccupancy;

/// Internal enum for sorting damage targets along an attack ray
enum DamageTarget {
    Hull { entity: Entity, grid_pos: IVec2, projection: f32 },
    Module { entity: Entity, _grid_pos: IVec2, projection: f32 },
}

/// Consumes SubmarineDamaged events and applies damage using directional kinetic penetration.
///
/// If direction is available: collect all hull/module tiles, project onto attack ray,
/// walk outermost-first applying penetration damage.
/// If no direction (radiation, explosion): fall back to random hull segment.
pub fn process_submarine_damage(
    mut damage_events: EventReader<SubmarineDamaged>,
    mut hull_query: Query<(Entity, &mut HullSegment, &GlobalTransform)>,
    mut module_query: Query<(Entity, &mut Module, &GlobalTransform), Without<DestroyedModule>>,
    sub_query: Query<&GlobalTransform, With<Submarine>>,
    room_map: Res<RoomMap>,
    mut breach_events: EventWriter<HullBreached>,
    mut room_depressurize_events: EventWriter<RoomDepressurized>,
    mut notifications: EventWriter<ShowNotification>,
    mut commands: Commands,
) {
    let mut rng = rand::thread_rng();

    let sub_center = sub_query
        .get_single()
        .map(|gt| gt.translation().truncate())
        .unwrap_or(Vec2::ZERO);

    for event in damage_events.iter() {
        // Skip radiation damage — it's handled directly in check_radiation_damage
        if matches!(event.source, DamageSource::Radiation) {
            continue;
        }

        if hull_query.is_empty() {
            continue;
        }

        // Determine attack direction
        let direction = event.direction.or_else(|| {
            event.position.map(|pos| (pos - sub_center).normalize_or_zero())
        });

        if let Some(dir) = direction {
            // === DIRECTIONAL DAMAGE WITH PENETRATION ===
            let mut targets: Vec<DamageTarget> = Vec::new();

            // Collect hull targets
            for (entity, hull, gt) in hull_query.iter() {
                let pos = gt.translation().truncate();
                let to_tile = pos - sub_center;
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

            // Collect module targets
            for (entity, module, gt) in module_query.iter() {
                let pos = gt.translation().truncate();
                let to_tile = pos - sub_center;
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
                        if let Ok((_, mut hull, _)) = hull_query.get_mut(*entity) {
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
                                breach_events.send(HullBreached {
                                    segment: *entity,
                                    severity: 1.0 - health_pct,
                                });

                                // Send RoomDepressurized if this tile is in a room
                                if let Some(&room_id) = room_map.tile_to_room.get(grid_pos) {
                                    room_depressurize_events.send(RoomDepressurized {
                                        room_id,
                                        severity: 1.0 - health_pct,
                                    });
                                }

                                notifications.send(ShowNotification {
                                    message: "Hull breach! Decompression in progress!".into(),
                                    notification_type: NotificationType::Danger,
                                    duration: 3.0,
                                });
                            }
                        }
                    }
                    DamageTarget::Module { entity, .. } => {
                        if let Ok((_, mut module, _)) = module_query.get_mut(*entity) {
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
            let count = hull_query.iter().count();
            if count == 0 { continue; }
            let idx = rng.gen_range(0..count);
            let target_entity = hull_query.iter().nth(idx).map(|(e, _, _)| e);

            let Some(target) = target_entity else { continue };

            if let Ok((_, mut hull, _)) = hull_query.get_mut(target) {
                hull.health = (hull.health - event.amount).max(0.0);

                let health_pct = if hull.max_health > 0.0 {
                    hull.health / hull.max_health
                } else {
                    0.0
                };

                if health_pct < 0.3 && !hull.is_depressurized {
                    hull.is_depressurized = true;
                    breach_events.send(HullBreached {
                        segment: target,
                        severity: 1.0 - health_pct,
                    });

                    if let Some(&room_id) = room_map.tile_to_room.get(&hull.grid_position) {
                        room_depressurize_events.send(RoomDepressurized {
                            room_id,
                            severity: 1.0 - health_pct,
                        });
                    }

                    notifications.send(ShowNotification {
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
                    .filter(|(_, _, t)| t.translation().truncate().distance(hit_pos) < 80.0)
                    .min_by(|(_, _, ta), (_, _, tb)| {
                        let da = ta.translation().truncate().distance(hit_pos);
                        let db = tb.translation().truncate().distance(hit_pos);
                        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                    });

                if let Some((_, mut module, _)) = closest_module {
                    let module_damage = event.amount * 0.5;
                    module.health = (module.health - module_damage).max(0.0);
                }
            }
        }

        // Spawn hit spark at damage position
        if let Some(pos) = event.position {
            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color: Color::rgba(1.0, 0.4, 0.1, 0.9),
                        custom_size: Some(Vec2::splat(24.0)),
                        ..default()
                    },
                    transform: Transform::from_xyz(pos.x, pos.y, 0.7),
                    ..default()
                },
                HitEffect {
                    timer: Timer::from_seconds(0.3, TimerMode::Once),
                },
            ));
        }
    }
}

/// Processes module destruction — marks destroyed modules with DestroyedModule component.
pub fn process_module_destruction(
    mut commands: Commands,
    mut module_query: Query<(Entity, &mut Module, &mut Sprite), Without<DestroyedModule>>,
    mut destroy_events: EventWriter<ModuleDestroyed>,
    mut notifications: EventWriter<ShowNotification>,
) {
    for (entity, mut module, mut sprite) in module_query.iter_mut() {
        if module.health <= 0.0 && module.is_active {
            module.is_active = false;
            module.health = 0.0;
            commands.entity(entity).insert(DestroyedModule {
                original_type: module.module_type,
            });
            sprite.color = Color::rgb(0.2, 0.2, 0.2);
            destroy_events.send(ModuleDestroyed { module: entity });
            notifications.send(ShowNotification {
                message: format!("{} destroyed!", module.module_type.name()),
                notification_type: NotificationType::Danger,
                duration: 3.0,
            });
        }
    }
}

/// Hit effect that auto-despawns after timer expires.
/// Used for both submarine damage sparks and creature hit flashes.
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
        if effect.timer.finished() {
            commands.entity(entity).despawn_recursive();
        }
    }
}

/// When an explosive module is freshly destroyed, queue a detonation with a short fuse delay.
pub fn queue_detonation(
    mut commands: Commands,
    query: Query<(Entity, &Module, &Explosive), Added<DestroyedModule>>,
    mut notifications: EventWriter<ShowNotification>,
) {
    for (entity, module, explosive) in query.iter() {
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
        notifications.send(ShowNotification {
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
    mut explosion_events: EventWriter<ModuleExploded>,
    mut fire_events: EventWriter<FireStarted>,
    mut breach_events: EventWriter<HullBreached>,
    mut hull_destroy_events: EventWriter<HullSegmentDestroyed>,
    room_map: Res<RoomMap>,
    mut room_depressurize_events: EventWriter<RoomDepressurized>,
    mut notifications: EventWriter<ShowNotification>,
) {
    // Collect finished detonations first to avoid borrow issues
    let mut finished: Vec<(Entity, PendingDetonation)> = Vec::new();

    for (entity, mut det) in det_query.iter_mut() {
        det.timer.tick(time.delta());
        if det.timer.finished() {
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
                            hull_destroy_events.send(HullSegmentDestroyed {
                                segment: hull_entity,
                                grid_position: target_pos,
                            });
                        } else if health_pct < 0.3 && !hull.is_depressurized {
                            hull.is_depressurized = true;
                            breach_events.send(HullBreached {
                                segment: hull_entity,
                                severity: 1.0 - health_pct,
                            });
                            if let Some(&room_id) = room_map.tile_to_room.get(&target_pos) {
                                room_depressurize_events.send(RoomDepressurized {
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
        if matches!(det.explosive_type, ExplosiveType::Fuel | ExplosiveType::Battery) {
            for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                let adj_pos = det.grid_position + offset;
                if let Some(&adj_entity) = occupancy.cells.get(&adj_pos) {
                    if module_query.get(adj_entity).is_ok() {
                        fire_events.send(FireStarted {
                            module: adj_entity,
                            grid_position: adj_pos,
                            intensity: 0.8,
                        });
                    }
                }
            }
        }

        // Send explosion event
        explosion_events.send(ModuleExploded {
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
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgba(1.0, 0.6, 0.1, 0.95),
                    custom_size: Some(Vec2::splat(det.blast_radius * 66.0)),
                    ..default()
                },
                transform: Transform::from_translation(world_pos),
                ..default()
            },
            HitEffect {
                timer: Timer::from_seconds(0.5, TimerMode::Once),
            },
        ));

        notifications.send(ShowNotification {
            message: format!("EXPLOSION at ({}, {})!", det.grid_position.x, det.grid_position.y),
            notification_type: NotificationType::Danger,
            duration: 3.0,
        });
    }
}
