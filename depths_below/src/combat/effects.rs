use bevy::prelude::*;

use super::*;

/// Some creatures can fire projectiles back at the submarine
pub(super) fn creature_ranged_attack(
    time: Res<Time>,
    mut creature_query: Query<(&Transform, &mut Creature, &CreatureAI), Without<Submarine>>,
    sub_query: Query<&Transform, With<Submarine>>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    let Ok(sub_transform) = sub_query.get_single() else { return };
    let sub_pos = sub_transform.translation.truncate();

    for (c_transform, mut creature, ai) in creature_query.iter_mut() {
        // Only fire when actively attacking and targeting the submarine
        if !matches!(ai.state, CreatureAIState::Attacking) {
            continue;
        }
        if !matches!(ai.target, Some(EcoTarget::Submarine(_))) {
            continue;
        }

        // Only some creature types shoot back
        let (shoot_range, shoot_damage, shoot_cooldown) = match creature.creature_type {
            CreatureType::ElectricEel => (200.0, 8.0, 3.0),
            CreatureType::Stalker => (300.0, 12.0, 4.0),
            CreatureType::Watcher => (400.0, 25.0, 2.5),
            _ => continue,
        };

        let c_pos = c_transform.translation.truncate();
        let dist = c_pos.distance(sub_pos);

        if dist > shoot_range {
            continue;
        }

        // Use attack_cooldown for ranged timing
        creature.attack_cooldown -= time.delta_seconds();
        if creature.attack_cooldown > 0.0 {
            continue;
        }
        creature.attack_cooldown = shoot_cooldown;

        // Fire projectile at submarine
        projectiles::spawn_projectile(
            &mut commands,
            &asset_server,
            c_pos,
            sub_pos,
            shoot_damage,
            PROJECTILE_SPEED * 0.6,
            false,
            AmmoType::Charge,
        );
    }
}

/// Animates floating damage numbers: move upward, fade out, despawn
pub(super) fn animate_floating_damage(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut FloatingDamage, &mut Transform, &mut Text)>,
) {
    for (entity, mut dmg, mut transform, mut text) in query.iter_mut() {
        dmg.timer.tick(time.delta());
        transform.translation.y += dmg.velocity * time.delta_seconds();

        // Fade out alpha
        let alpha = 1.0 - dmg.timer.percent();
        for section in text.sections.iter_mut() {
            section.style.color.set_a(alpha);
        }

        if dmg.timer.finished() {
            commands.entity(entity).despawn_recursive();
        }
    }
}

/// Despawn creatures with health <= 0, send events, drop loot, record in ecosystem
pub(super) fn despawn_dead_creatures(
    mut commands: Commands,
    creature_query: Query<(Entity, &Transform, &Creature)>,
    mut killed_events: EventWriter<CreatureKilled>,
    mut inventory: ResMut<Inventory>,
    mut statistics: ResMut<Statistics>,
    mut notifications: EventWriter<ShowNotification>,
    mut eco_state: ResMut<EcosystemState>,
) {
    for (entity, transform, creature) in creature_query.iter() {
        if creature.health <= 0.0 {
            // Determine loot
            let loot = match creature.creature_type {
                CreatureType::Scavenger => vec![ItemType::ScrapMetal],
                CreatureType::Stalker => vec![ItemType::BioSample],
                CreatureType::BlindHunter => vec![ItemType::BioSample, ItemType::RareAlloy],
                CreatureType::Leviathan => vec![ItemType::RareAlloy, ItemType::AncientArtifact],
                CreatureType::ElectricEel => vec![ItemType::FuelCell],
                _ => vec![ItemType::ScrapMetal],
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

            killed_events.send(CreatureKilled {
                creature: entity,
                creature_type: creature.creature_type,
                loot: loot.clone(),
            });

            let loot_names: Vec<&str> = loot.iter().map(|i| i.name()).collect();
            notifications.send(ShowNotification {
                message: format!("{:?} killed! Loot: {}", creature.creature_type, loot_names.join(", ")),
                notification_type: NotificationType::Success,
                duration: 3.0,
            });

            commands.entity(entity).despawn_recursive();
        }
    }
}
