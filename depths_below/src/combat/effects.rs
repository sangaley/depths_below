use bevy::prelude::*;

use super::*;

/// Some creatures can fire projectiles back at the ship
pub(super) fn creature_ranged_attack(
    time: Res<Time>,
    mut creature_query: Query<(&Transform, &mut Creature, &CreatureAI), Without<Ship>>,
    ship_query: Query<&Transform, With<Ship>>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    let Ok(ship_transform) = ship_query.single() else { return };
    let ship_pos = ship_transform.translation.truncate();

    for (c_transform, mut creature, ai) in creature_query.iter_mut() {
        // Only fire when actively attacking and targeting the ship
        if !matches!(ai.state, CreatureAIState::Attacking) {
            continue;
        }
        if !matches!(ai.target, Some(EcoTarget::Ship(_))) {
            continue;
        }

        // Only some creature types shoot back
        let (shoot_range, shoot_damage, shoot_cooldown) = match creature.creature_type {
            CreatureType::Stalker => (300.0, 12.0, 4.0),
            _ => continue,
        };

        let c_pos = c_transform.translation.truncate();
        let dist = c_pos.distance(ship_pos);

        if dist > shoot_range {
            continue;
        }

        // Use attack_cooldown for ranged timing
        creature.attack_cooldown -= time.delta_secs();
        if creature.attack_cooldown > 0.0 {
            continue;
        }
        creature.attack_cooldown = shoot_cooldown;

        // Fire projectile at ship
        projectiles::spawn_projectile(
            &mut commands,
            &asset_server,
            c_pos,
            ship_pos,
            shoot_damage,
            PROJECTILE_SPEED * 0.6,
            shoot_range,
            ProjectileOwner::Creature,
            AmmoType::Charge,
        );
    }
}

/// Animates floating damage numbers: move upward, fade out, despawn
pub(super) fn animate_floating_damage(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut FloatingDamage, &mut Transform, &mut TextColor)>,
) {
    for (entity, mut dmg, mut transform, mut text_color) in query.iter_mut() {
        dmg.timer.tick(time.delta());
        transform.translation.y += dmg.velocity * time.delta_secs();

        // Fade out alpha
        let alpha = 1.0 - dmg.timer.fraction();
        text_color.0.set_alpha(alpha);

        if dmg.timer.is_finished() {
            commands.entity(entity).despawn();
        }
    }
}

/// Despawn creatures with health <= 0, send events, drop loot, record in ecosystem
pub(super) fn despawn_dead_creatures(
    mut commands: Commands,
    creature_query: Query<(Entity, &Transform, &Creature)>,
    mut killed_events: MessageWriter<CreatureKilled>,
    mut inventory: ResMut<Inventory>,
    mut statistics: ResMut<Statistics>,
    mut notifications: MessageWriter<ShowNotification>,
    mut eco_state: ResMut<EcosystemState>,
) {
    for (entity, transform, creature) in creature_query.iter() {
        if creature.health <= 0.0 {
            // Determine loot
            let loot = match creature.creature_type {
                CreatureType::VoidDrifter => vec![ItemType::ScrapMetal],
                CreatureType::Stalker => vec![ItemType::BioSample],
                CreatureType::Leviathan => vec![ItemType::RareAlloy, ItemType::AncientArtifact],
                CreatureType::ParasiteSwarm => vec![ItemType::ScrapMetal],
            };

            // Add loot to inventory
            for &item in &loot {
                inventory.add_item(item, 1);
            }

            statistics.creatures_killed += 1;

            // Record kill in ecosystem state
            let pos = transform.translation.truncate();
            let elapsed = eco_state.total_elapsed;
            eco_state.recent_kills.push(crate::resources::EcoKillRecord {
                killer_type: None, // Could be player or another creature
                victim_type: creature.creature_type,
                position: pos,
                time: elapsed,
                by_player: true, // Assume player kill (projectile kills)
            });

            // Decrement population count
            if let Some(count) = eco_state.population_counts.get_mut(&creature.creature_type) {
                *count = count.saturating_sub(1);
            }

            killed_events.write(CreatureKilled {
                creature: entity,
                creature_type: creature.creature_type,
                loot: loot.clone(),
            });

            let loot_names: Vec<&str> = loot.iter().map(|i| i.name()).collect();
            notifications.write(ShowNotification {
                message: format!("{:?} killed! Loot: {}", creature.creature_type, loot_names.join(", ")),
                notification_type: NotificationType::Success,
                duration: 3.0,
            });

            commands.entity(entity).despawn();
        }
    }
}
