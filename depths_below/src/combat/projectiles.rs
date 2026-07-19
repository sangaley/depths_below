use bevy::prelude::*;

use super::*;
use crate::ai_ship::components::AiShip;
use crate::events::AiShipDamaged;

/// Spawn a projectile entity, differentiated by ammo type.
///
/// `range` sets how far the shot can actually travel before it expires —
/// this used to be a fixed per-ammo-type timer (1.5-4s) completely
/// disconnected from the weapon's stated range, so a "6000-range" weapon's
/// bullets (600u/s * 1.5 speed_mult = 900u/s, 1.5s lifetime) physically
/// expired after ~1350 units. Every ship "in range" per that stat was
/// wasting ammo shooting at something its own shots could never reach,
/// which meant nothing could ever actually fight at the ranges the AI
/// standoff distances and weapon stats implied. Lifetime is now derived
/// from range so a shot fired at max range takes exactly as long to arrive
/// as the geometry implies.
pub(crate) fn spawn_projectile(
    commands: &mut Commands,
    asset_server: &AssetServer,
    origin: Vec2,
    target: Vec2,
    damage: f32,
    speed: f32,
    range: f32,
    owner: ProjectileOwner,
    ammo_type: AmmoType,
) {
    let direction = (target - origin).normalize_or_zero();
    let angle = direction.y.atan2(direction.x);

    let texture_path = if owner.is_player() {
        crate::sprite_map::effect_sprite_path("torpedo")
    } else {
        crate::sprite_map::effect_sprite_path("enemy_projectile")
    };

    // Enemy projectiles keep red tint regardless of ammo type
    let final_color = if owner.is_player() { ammo_type.projectile_color() } else { Color::srgb(1.0, 0.2, 0.2) };

    let final_speed = speed * ammo_type.speed_mult();
    let lifetime_secs = (range / final_speed.max(1.0)).max(0.1);

    commands.spawn((
        (Sprite {
                image: asset_server.load(texture_path),
                color: final_color,
                custom_size: Some(ammo_type.projectile_size()),
                ..default()
            }, Transform {
                translation: Vec3::new(origin.x, origin.y, 0.5),
                rotation: Quat::from_rotation_z(angle),
                ..default()
            }),
        Projectile {
            damage,
            speed: final_speed,
            direction,
            lifetime: Timer::from_seconds(lifetime_secs, TimerMode::Once),
            owner,
            ammo_type,
        },
    ));
}

/// Move projectiles and despawn expired ones
pub(super) fn projectile_movement(
    time: Res<Time>,
    mut commands: Commands,
    mut projectile_query: Query<(Entity, &mut Projectile, &mut Transform)>,
) {
    for (entity, mut projectile, mut transform) in projectile_query.iter_mut() {
        // Move
        let delta = projectile.direction * projectile.speed * time.delta_secs();
        transform.translation.x += delta.x;
        transform.translation.y += delta.y;

        // Tick lifetime
        projectile.lifetime.tick(time.delta());
        if projectile.lifetime.is_finished() {
            commands.entity(entity).despawn();
        }
    }
}

/// Check projectile collisions — ammo-type aware.
/// Torpedo/Bullet: single target. Charge: AoE hits all creatures in radius.
pub(super) fn projectile_collision(
    mut commands: Commands,
    projectile_query: Query<(Entity, &Projectile, &Transform)>,
    mut creature_query: Query<(Entity, &Transform, &mut Creature), Without<Ship>>,
    // Without<AiShip>: this system also reads AI ships' ShipShield
    // (immutably) via ai_ship_query below. Bevy's conflict checker can't
    // infer From With<Ship>/With<AiShip> alone that these two queries are
    // disjoint — same missing-canceling-pair issue documented on the
    // laser/ion systems in energy_weapons.rs.
    mut ship_query: Query<(Entity, &Transform, Option<&mut crate::combat::shields::ShipShield>), (With<Ship>, Without<AiShip>)>,
    // Every AI ship gets a ShipShield on spawn (attach_ai_shields) sized to
    // its actual hull extent — SUBMARINE_RADIUS below is only the fallback
    // for the brief window before that attaches. Without this, hit
    // detection used a flat 60-unit circle around the ship ROOT regardless
    // of actual size, so a shot aimed dead-center at a large ship (most of
    // the roster — Iron Tide, Dreadnought, Void Titan...) could sail
    // straight through its visible hull without ever registering as a hit.
    mut ai_ship_query: Query<(Entity, &Transform, Option<&mut crate::combat::shields::ShipShield>), With<AiShip>>,
    mut damage_events: MessageWriter<ShipDamaged>,
    mut ai_damage_events: MessageWriter<AiShipDamaged>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for (proj_entity, projectile, proj_transform) in projectile_query.iter() {
        let proj_pos = proj_transform.translation.truncate();

        // Stage 2 of the ownership rework: an AI shot that whiffs past the
        // player now still resolves against every OTHER ai ship it's near
        // (never its own — see the `owner_ai_root` filter below). Player
        // shots and creature shots keep their stage-1 behavior; brains
        // still only aim at the player (that's slice 5), so this mostly
        // catches stray fire in crowded fights — but a shot flying through
        // a ship no longer does structurally nothing.
        let owner_ai_root = match projectile.owner {
            ProjectileOwner::AiShip(root) => Some(root),
            _ => None,
        };

        if projectile.owner.is_player() {
            let effective_radius = PROJECTILE_RADIUS * projectile.ammo_type.hit_radius_mult() + CREATURE_RADIUS;
            let is_aoe = projectile.ammo_type.is_aoe();
            let mut hit_any = false;

            let hit_color = if is_aoe { Color::srgb(0.5, 0.7, 1.0) } else { Color::srgb(1.0, 1.0, 0.5) };
            let hit_size = if is_aoe { 28.0 } else { 16.0 };

            for (_c_entity, c_transform, mut creature) in creature_query.iter_mut() {
                let c_pos = c_transform.translation.truncate();
                let dist = proj_pos.distance(c_pos);

                if dist < effective_radius {
                    creature.health -= projectile.damage;
                    hit_any = true;

                    spawn_hit_effect(&mut commands, c_pos, hit_color, hit_size);
                    spawn_floating_damage(&mut commands, c_pos, projectile.damage, Color::srgb(1.0, 1.0, 0.3));

                    if !is_aoe {
                        break;
                    }
                }
            }

            // Check AI ships if no creature was hit (single-target) or always for AoE
            if !hit_any || is_aoe {
                for (ai_entity, ai_transform, mut shield) in ai_ship_query.iter_mut() {
                    let ai_pos = shield.as_ref().map(|s| s.world_center(ai_transform))
                        .unwrap_or_else(|| ai_transform.translation.truncate());
                    let hit_radius = shield.as_ref().map(|s| s.radius).unwrap_or(SUBMARINE_RADIUS);
                    let dist = proj_pos.distance(ai_pos);

                    if dist < PROJECTILE_RADIUS + hit_radius {
                        // Shield absorbs first — this used to skip straight
                        // to hull/module damage regardless of shield state.
                        if let Some(shield) = shield.as_deref_mut() {
                            if shield.is_up() {
                                shield.absorb(projectile.damage);
                                hit_any = true;
                                spawn_hit_effect(&mut commands, proj_pos, Color::srgb(0.5, 0.8, 1.0), 16.0);
                                if !is_aoe {
                                    break;
                                }
                                continue;
                            }
                        }

                        ai_damage_events.write(AiShipDamaged {
                            target: ai_entity,
                            source: DamageSource::Explosion,
                            amount: projectile.damage,
                            position: Some(proj_pos),
                            direction: Some(projectile.direction),
                            attacker: ship_query.single().ok().map(|(e, _, _)| e),
                        });
                        hit_any = true;

                        spawn_hit_effect(&mut commands, ai_pos, Color::srgb(1.0, 0.5, 0.2), hit_size);
                        spawn_floating_damage(&mut commands, ai_pos, projectile.damage, Color::srgb(1.0, 0.8, 0.3));

                        if !is_aoe {
                            break;
                        }
                    }
                }
            }

            if hit_any {
                commands.entity(proj_entity).despawn();
            }
        } else {
            // Non-player projectile (AI ship or creature) -> player shield
            // first, then the hull. Tracks whether the player was actually
            // hit, so an AI-owned shot that whiffs can fall through to the
            // stage-2 AI-vs-AI check below instead of flying on forever.
            let mut hit_player = false;

            if let Ok((_, ship_transform, shield)) = ship_query.single_mut() {
                let ship_pos = ship_transform.translation.truncate();
                let mut dist = proj_pos.distance(ship_pos);

                // Hull hit bound follows the ship's real extent (the shield
                // radius is computed from it) — the old fixed radius let most
                // shots sail through the outer hull blocks.
                let mut hull_hit_radius = PROJECTILE_RADIUS + SUBMARINE_RADIUS;

                if let Some(mut shield) = shield {
                    // Bubble is centered on the blocks' centroid, not the root
                    dist = proj_pos.distance(shield.world_center(ship_transform));
                    if shield.is_up() && dist < shield.radius {
                        shield.absorb(projectile.damage);
                        spawn_hit_effect(&mut commands, proj_pos, Color::srgb(0.5, 0.8, 1.0), 16.0);
                        commands.entity(proj_entity).despawn();
                        continue;
                    }
                    hull_hit_radius = hull_hit_radius.max(shield.radius);
                }

                if dist < hull_hit_radius {
                    damage_events.write(ShipDamaged {
                        source: DamageSource::Creature(Entity::PLACEHOLDER),
                        amount: projectile.damage,
                        position: Some(proj_pos),
                        // process_ship_damage's outermost-first penetration
                        // sort assumes `direction` points from the ship
                        // TOWARD the attacker (every other ShipDamaged
                        // writer uses (attacker_pos - ship_pos)). This
                        // passed the projectile's own direction of travel —
                        // attacker THROUGH the ship, the opposite sign — so
                        // damage was applied outermost-first along the
                        // wrong axis: a shot into the bow could destroy
                        // blocks at the stern first instead of the bow
                        // blocks it actually hit.
                        direction: Some(-projectile.direction),
                    });

                    notifications.write(ShowNotification {
                        message: format!("Hull hit! -{:.0} damage", projectile.damage),
                        notification_type: NotificationType::Danger,
                        duration: 2.0,
                    });

                    commands.entity(proj_entity).despawn();
                    hit_player = true;
                }
            }

            // Stage 2: an AI-owned shot that missed the player is still
            // live — check it against every OTHER ai ship (never its own;
            // firing-arc/adjacency already keeps a ship from hitting
            // itself, this is belt-and-suspenders). Creature-owned shots
            // don't get this arm — creatures don't fight AI ships here.
            if !hit_player {
                if let Some(owner_root) = owner_ai_root {
                    for (ai_entity, ai_transform, mut shield) in ai_ship_query.iter_mut() {
                        if ai_entity == owner_root { continue; }
                        let ai_pos = shield.as_ref().map(|s| s.world_center(ai_transform))
                            .unwrap_or_else(|| ai_transform.translation.truncate());
                        let hit_radius = shield.as_ref().map(|s| s.radius).unwrap_or(SUBMARINE_RADIUS);
                        let dist = proj_pos.distance(ai_pos);

                        if dist < PROJECTILE_RADIUS + hit_radius {
                            // Shield absorbs first, same as the player's own
                            // hit path above — this arm used to skip straight
                            // to hull/module damage, so an AI ship's shield
                            // never visibly took a hit (or blocked anything)
                            // even though the hull underneath WAS being
                            // damaged correctly.
                            if let Some(shield) = shield.as_deref_mut() {
                                if shield.is_up() {
                                    shield.absorb(projectile.damage);
                                    spawn_hit_effect(&mut commands, proj_pos, Color::srgb(0.5, 0.8, 1.0), 16.0);
                                    commands.entity(proj_entity).despawn();
                                    break;
                                }
                            }

                            ai_damage_events.write(AiShipDamaged {
                                target: ai_entity,
                                source: DamageSource::Explosion,
                                amount: projectile.damage,
                                position: Some(proj_pos),
                                direction: Some(projectile.direction),
                                attacker: Some(owner_root),
                            });

                            spawn_hit_effect(&mut commands, ai_pos, Color::srgb(1.0, 0.5, 0.2), 16.0);
                            spawn_floating_damage(&mut commands, ai_pos, projectile.damage, Color::srgb(1.0, 0.8, 0.3));

                            commands.entity(proj_entity).despawn();
                            break;
                        }
                    }
                }
            }
        }
    }
}
