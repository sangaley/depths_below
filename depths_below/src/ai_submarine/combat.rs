use bevy::prelude::*;

use crate::components::*;
use crate::events::*;
use crate::combat::{spawn_floating_damage, spawn_hit_effect};
use super::components::*;

/// AI submarines in Engaging state fire weapons at the player
pub fn ai_weapon_fire_system(
    time: Res<Time>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    ai_subs: Query<(Entity, &Transform, &AiSubBehavior, &Children), With<AiSubmarine>>,
    mut weapon_query: Query<(
        &mut Weapon,
        &mut WeaponCooldown,
        &Module,
        &AmmoStorage,
        &OwnedByAiSub,
    )>,
    player_query: Query<&Transform, With<Submarine>>,
) {
    let Ok(player_transform) = player_query.get_single() else { return };
    let player_pos = player_transform.translation.truncate();

    for (_ai_entity, ai_transform, behavior, children) in ai_subs.iter() {
        if *behavior != AiSubBehavior::Engaging {
            continue;
        }

        let ai_pos = ai_transform.translation.truncate();
        let dist_to_player = ai_pos.distance(player_pos);

        for &child in children.iter() {
            let Ok((mut weapon, mut cooldown, module, ammo_storage, _owned)) =
                weapon_query.get_mut(child)
            else {
                continue;
            };

            if !module.is_active || module.health <= 0.0 || weapon.ammo == 0 {
                continue;
            }

            // Only fire if player is within weapon range
            if dist_to_player > weapon.range {
                continue;
            }

            // Tick cooldown
            cooldown.timer.tick(time.delta());
            if !cooldown.timer.finished() {
                continue;
            }

            cooldown.timer.reset();
            weapon.ammo = weapon.ammo.saturating_sub(1);

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

/// Process damage to AI submarines — per-module penetration
pub fn process_ai_sub_damage_system(
    mut damage_events: EventReader<AiSubDamaged>,
    mut ai_subs: Query<(&mut AiSubState, &Children), With<AiSubmarine>>,
    mut hull_query: Query<(&mut HullSegment, &Transform, &OwnedByAiSub), Without<Module>>,
    mut module_query: Query<(&mut Module, &Transform, &OwnedByAiSub), Without<HullSegment>>,
    mut destroyed_events: EventWriter<AiSubDestroyed>,
    ai_sub_query: Query<(&Transform, &AiSubType), With<AiSubmarine>>,
    mut commands: Commands,
) {
    for event in damage_events.iter() {
        let Ok((mut state, children)) = ai_subs.get_mut(event.target) else {
            continue;
        };

        state.last_hit_timer = 0.0;

        let impact_pos = event.position.unwrap_or(Vec2::ZERO);
        let mut remaining_damage = event.amount;

        // Collect child hull segments sorted by distance from impact
        let mut hull_hits: Vec<(Entity, f32)> = Vec::new();
        for &child in children.iter() {
            if let Ok((_, hull_transform, owned)) = hull_query.get(child) {
                if owned.root == event.target {
                    let dist = hull_transform.translation.truncate().distance(impact_pos);
                    hull_hits.push((child, dist));
                }
            }
        }
        hull_hits.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

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
                    Color::rgb(1.0, 0.3, 0.3),
                );
                spawn_hit_effect(
                    &mut commands,
                    hull_transform.translation.truncate(),
                    Color::rgb(1.0, 0.5, 0.2),
                    16.0,
                );
            }
        }

        // If damage penetrates hull, hit nearest modules
        if remaining_damage > 0.0 {
            let mut module_hits: Vec<(Entity, f32)> = Vec::new();
            for &child in children.iter() {
                if let Ok((_, mod_transform, owned)) = module_query.get(child) {
                    if owned.root == event.target {
                        let dist = mod_transform.translation.truncate().distance(impact_pos);
                        module_hits.push((child, dist));
                    }
                }
            }
            module_hits.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

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
                        Color::rgb(1.0, 0.6, 0.2),
                    );
                }
            }
        }

        // Recalculate hull integrity
        let mut total_hull_hp = 0.0_f32;
        let mut max_hull_hp = 0.0_f32;
        for &child in children.iter() {
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
            if let Ok((ai_transform, ai_sub_type)) = ai_sub_query.get(event.target) {
                destroyed_events.send(AiSubDestroyed {
                    entity: event.target,
                    sub_type: *ai_sub_type,
                    position: ai_transform.translation.truncate(),
                });
            }
        }
    }
}
