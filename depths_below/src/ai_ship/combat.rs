use bevy::prelude::*;

use crate::components::*;
use crate::events::*;
use crate::combat::{spawn_floating_damage, spawn_hit_effect};
use super::components::*;

/// AI ships in Engaging state fire weapons at their current AiShipTarget —
/// the player OR another AI ship, whichever ai_brain picked this tick (see
/// AiShipTarget's doc comment). WHO to target is only re-decided every
/// 0.25s (the brain tick); WHERE to aim is re-read from that target's live
/// Transform every single frame this system runs — AiShipTarget.position is
/// a snapshot from the moment it was picked, stale by up to 0.25s, which
/// was enough for an orbiting/strafing ship (standard combat maneuver, see
/// movement.rs's standoff-orbit) to be gone from that point by the time a
/// shot arrived. Live lookup fixes shots consistently whiffing at range.
pub fn ai_weapon_fire_system(
    time: Res<Time>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    ai_ships: Query<(Entity, &Transform, &AiShipBehavior, &AiShipTarget, &Children, Option<&super::power::AiPowerState>), With<AiShip>>,
    mut weapon_query: Query<(
        &mut Weapon,
        &mut WeaponCooldown,
        &Module,
        &AmmoStorage,
        &OwnedByAiShip,
        Option<&ModuleEfficiency>,
    )>,
    player_query: Query<&Transform, With<Ship>>,
    target_transform_query: Query<&Transform>,
    // Ship hit detection (combat/projectiles.rs) centers on the shield's
    // world_center — the blocks' centroid, not the root, since the root is
    // often at one end of the layout. Aiming at the raw root position was
    // consistent geometry mismatch on any ship with an off-center layout.
    target_shield_query: Query<&crate::combat::shields::ShipShield>,
    mut fired_events: MessageWriter<WeaponFired>,
) {
    // DEPTHS_MOVETEST_ENEMY spawns a target dummy that's shot-free by
    // default (for testing movement/damage-model in isolation). Set
    // DEPTHS_MOVETEST_ENEMY_SHOOTS=1 too to let it fire back.
    if crate::demo::skip_ai_ship_spawn()
        && std::env::var("DEPTHS_MOVETEST_ENEMY_SHOOTS").ok().as_deref() != Some("1")
    {
        return;
    }

    let player_pos = player_query.single().ok().map(|t| t.translation.truncate());

    for (ai_entity, ai_transform, behavior, ai_target, children, ai_power) in ai_ships.iter() {
        if *behavior != AiShipBehavior::Engaging {
            continue;
        }

        // Power-starved ships hold fire — same hard cutoff the player's own
        // kinetic/missile weapons already use (combat/new_projectiles.rs,
        // combat/missiles.rs). None (graph not computed yet this tick, e.g.
        // the ship just spawned) defaults to permissive so a fresh ship
        // isn't blocked before its first power tick ever runs.
        if ai_power.is_some_and(|p| p.power_balance < 0.0) {
            continue;
        }

        // Live position of whoever the brain picked, re-read fresh every
        // frame. Falls back to the last-known snapshot (target despawned
        // mid-frame, say), then to the player, only if the live lookup
        // fails — see the fn doc comment for why this shouldn't happen.
        let Some(target_pos) = ai_target.entity
            .and_then(|e| target_transform_query.get(e).ok().map(|t| {
                target_shield_query.get(e).ok()
                    .map(|s| s.world_center(t))
                    .unwrap_or_else(|| t.translation.truncate())
            }))
            .or_else(|| Some(ai_target.position).filter(|_| ai_target.entity.is_some()))
            .or(player_pos)
        else { continue };

        let ai_pos = ai_transform.translation.truncate();
        let dist_to_target = ai_pos.distance(target_pos);

        for child in children.iter() {
            let Ok((mut weapon, mut cooldown, module, ammo_storage, _owned, eff)) =
                weapon_query.get_mut(child)
            else {
                continue;
            };

            if !module.is_active || module.health <= 0.0
                || (!crate::combat::INFINITE_AMMO && weapon.ammo == 0) {
                continue;
            }

            // Unstaffed weapon stations produce nothing — same rule the
            // player's own ship runs under (compute_module_efficiency,
            // crew/mod.rs). Every weapon module is crew_station:true in the
            // registry, so this is a real gate for every AI faction, scaled
            // by crew_fill_fraction per faction (ai_ship::components).
            let efficiency = effective_efficiency(module, eff);
            if efficiency <= 0.0 {
                continue;
            }

            // Only fire if the target is within weapon range
            if dist_to_target > weapon.range {
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
            fired_events.write(WeaponFired {
                weapon_type: module.module_type,
                position: ai_pos,
                from_player: false,
            });

            crate::combat::projectiles::spawn_projectile(
                &mut commands,
                &asset_server,
                ai_pos,
                target_pos,
                weapon.damage * efficiency,
                crate::combat::PROJECTILE_SPEED,
                weapon.range,
                crate::components::ProjectileOwner::AiShip(ai_entity),
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
        // Preserve the last known attributable attacker across non-
        // attributable damage ticks (fire DoT, self-detonation) rather
        // than clearing it — a ship mid-burn from an earlier shot should
        // still remember who fired it.
        if event.attacker.is_some() {
            state.last_attacker = event.attacker;
        }

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
