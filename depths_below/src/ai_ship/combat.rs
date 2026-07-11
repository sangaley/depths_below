use bevy::prelude::*;

use crate::components::*;
use crate::events::*;
use crate::combat::{spawn_floating_damage, spawn_hit_effect};
use super::components::*;

/// AI ships in Engaging state fire weapons at the player
pub fn ai_weapon_fire_system(
    time: Res<Time>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    ai_ships: Query<(Entity, &Transform, &AiShipBehavior, &Children), With<AiShip>>,
    mut weapon_query: Query<(
        &mut Weapon,
        &mut WeaponCooldown,
        &Module,
        &AmmoStorage,
        &OwnedByAiShip,
    )>,
    player_query: Query<&Transform, With<Ship>>,
) {
    // DEPTHS_MOVETEST_ENEMY spawns a target dummy that's shot-free by
    // default (for testing movement/damage-model in isolation). Set
    // DEPTHS_MOVETEST_ENEMY_SHOOTS=1 too to let it fire back.
    if crate::demo::skip_ai_ship_spawn()
        && std::env::var("DEPTHS_MOVETEST_ENEMY_SHOOTS").ok().as_deref() != Some("1")
    {
        return;
    }

    let Ok(player_transform) = player_query.single() else { return };
    let player_pos = player_transform.translation.truncate();

    for (_ai_entity, ai_transform, behavior, children) in ai_ships.iter() {
        if *behavior != AiShipBehavior::Engaging {
            continue;
        }

        let ai_pos = ai_transform.translation.truncate();
        let dist_to_player = ai_pos.distance(player_pos);

        for child in children.iter() {
            let Ok((mut weapon, mut cooldown, module, ammo_storage, _owned)) =
                weapon_query.get_mut(child)
            else {
                continue;
            };

            if !module.is_active || module.health <= 0.0
                || (!crate::combat::INFINITE_AMMO && weapon.ammo == 0) {
                continue;
            }

            // Only fire if player is within weapon range
            if dist_to_player > weapon.range {
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

            // Spawn projectile toward player
            crate::combat::projectiles::spawn_projectile(
                &mut commands,
                &asset_server,
                ai_pos,
                player_pos,
                weapon.damage,
                600.0,
                false, // not from player
                ammo_storage.ammo_type,
            );
        }
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
                });
            }
        }
    }
}

/// The reactor is the ship's core: destroying it kills the ship outright via
/// a chain-reaction explosion, rather than requiring every single hull tile
/// on the ship to be ground down first (that took minutes even on the
/// smallest ship in the roster — no real "kill it here" moment).
pub fn check_ai_reactor_destruction(
    mut commands: Commands,
    destroyed_reactors: Query<(&Module, &OwnedByAiShip), Added<DestroyedModule>>,
    mut ai_ships: Query<(&mut AiShipState, &Transform, &AiShipType, Option<&BountyTarget>)>,
    mut destroyed_events: MessageWriter<AiShipDestroyed>,
) {
    for (module, owned) in destroyed_reactors.iter() {
        if !matches!(module.module_type,
            ModuleType::SmallReactor | ModuleType::StandardReactor
            | ModuleType::LargeReactor | ModuleType::FusionReactor
        ) {
            continue;
        }
        let Ok((mut state, transform, ship_type, bounty)) = ai_ships.get_mut(owned.root) else { continue };
        if state.is_destroyed { continue; }

        state.is_destroyed = true;
        state.hull_integrity = 0.0;
        let pos = transform.translation.truncate();
        spawn_hit_effect(&mut commands, pos, Color::srgb(1.0, 0.6, 0.1), 120.0);
        destroyed_events.write(AiShipDestroyed {
            entity: owned.root,
            ship_type: *ship_type,
            position: pos,
            bounty_id: bounty.map(|b| b.0),
        });
    }
}
