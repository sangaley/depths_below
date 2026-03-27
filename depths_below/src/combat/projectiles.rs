use bevy::prelude::*;

use super::*;
use crate::ai_submarine::components::AiSubmarine;
use crate::events::AiSubDamaged;

/// Spawn a projectile entity, differentiated by ammo type
pub(crate) fn spawn_projectile(
    commands: &mut Commands,
    asset_server: &AssetServer,
    origin: Vec2,
    target: Vec2,
    damage: f32,
    speed: f32,
    from_player: bool,
    ammo_type: AmmoType,
) {
    let direction = (target - origin).normalize_or_zero();
    let angle = direction.y.atan2(direction.x);

    let texture_path = if from_player {
        crate::sprite_map::effect_sprite_path("torpedo")
    } else {
        crate::sprite_map::effect_sprite_path("enemy_projectile")
    };

    // Enemy projectiles keep red tint regardless of ammo type
    let final_color = if from_player { ammo_type.projectile_color() } else { Color::rgb(1.0, 0.2, 0.2) };

    commands.spawn((
        SpriteBundle {
            texture: asset_server.load(texture_path),
            sprite: Sprite {
                color: final_color,
                custom_size: Some(ammo_type.projectile_size()),
                ..default()
            },
            transform: Transform {
                translation: Vec3::new(origin.x, origin.y, 0.5),
                rotation: Quat::from_rotation_z(angle),
                ..default()
            },
            ..default()
        },
        Projectile {
            damage,
            speed: speed * ammo_type.speed_mult(),
            direction,
            lifetime: Timer::from_seconds(ammo_type.lifetime_secs(), TimerMode::Once),
            from_player,
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
        let delta = projectile.direction * projectile.speed * time.delta_seconds();
        transform.translation.x += delta.x;
        transform.translation.y += delta.y;

        // Tick lifetime
        projectile.lifetime.tick(time.delta());
        if projectile.lifetime.finished() {
            commands.entity(entity).despawn_recursive();
        }
    }
}

/// Check projectile collisions — ammo-type aware.
/// Torpedo/Bullet: single target. Charge: AoE hits all creatures in radius.
pub(super) fn projectile_collision(
    mut commands: Commands,
    projectile_query: Query<(Entity, &Projectile, &Transform)>,
    mut creature_query: Query<(Entity, &Transform, &mut Creature), Without<Submarine>>,
    sub_query: Query<&Transform, With<Submarine>>,
    ai_sub_query: Query<(Entity, &Transform), With<AiSubmarine>>,
    mut damage_events: EventWriter<SubmarineDamaged>,
    mut ai_damage_events: EventWriter<AiSubDamaged>,
    mut notifications: EventWriter<ShowNotification>,
) {
    for (proj_entity, projectile, proj_transform) in projectile_query.iter() {
        let proj_pos = proj_transform.translation.truncate();

        if projectile.from_player {
            let effective_radius = PROJECTILE_RADIUS * projectile.ammo_type.hit_radius_mult() + CREATURE_RADIUS;
            let is_aoe = projectile.ammo_type.is_aoe();
            let mut hit_any = false;

            let hit_color = if is_aoe { Color::rgb(0.5, 0.7, 1.0) } else { Color::rgb(1.0, 1.0, 0.5) };
            let hit_size = if is_aoe { 28.0 } else { 16.0 };

            for (_c_entity, c_transform, mut creature) in creature_query.iter_mut() {
                let c_pos = c_transform.translation.truncate();
                let dist = proj_pos.distance(c_pos);

                if dist < effective_radius {
                    creature.health -= projectile.damage;
                    hit_any = true;

                    spawn_hit_effect(&mut commands, c_pos, hit_color, hit_size);
                    spawn_floating_damage(&mut commands, c_pos, projectile.damage, Color::rgb(1.0, 1.0, 0.3));

                    if !is_aoe {
                        break;
                    }
                }
            }

            // Check AI submarines if no creature was hit (single-target) or always for AoE
            if !hit_any || is_aoe {
                for (ai_entity, ai_transform) in ai_sub_query.iter() {
                    let ai_pos = ai_transform.translation.truncate();
                    let dist = proj_pos.distance(ai_pos);

                    if dist < PROJECTILE_RADIUS + SUBMARINE_RADIUS {
                        ai_damage_events.send(AiSubDamaged {
                            target: ai_entity,
                            source: DamageSource::Explosion,
                            amount: projectile.damage,
                            position: Some(proj_pos),
                            direction: Some(projectile.direction),
                        });
                        hit_any = true;

                        spawn_hit_effect(&mut commands, ai_pos, Color::rgb(1.0, 0.5, 0.2), hit_size);
                        spawn_floating_damage(&mut commands, ai_pos, projectile.damage, Color::rgb(1.0, 0.8, 0.3));

                        if !is_aoe {
                            break;
                        }
                    }
                }
            }

            if hit_any {
                commands.entity(proj_entity).despawn_recursive();
            }
        } else {
            // Enemy projectile -> check against submarine
            if let Ok(sub_transform) = sub_query.get_single() {
                let sub_pos = sub_transform.translation.truncate();
                let dist = proj_pos.distance(sub_pos);

                if dist < PROJECTILE_RADIUS + SUBMARINE_RADIUS {
                    damage_events.send(SubmarineDamaged {
                        source: DamageSource::Creature(Entity::PLACEHOLDER),
                        amount: projectile.damage,
                        position: Some(proj_pos),
                        direction: Some(projectile.direction),
                    });

                    notifications.send(ShowNotification {
                        message: format!("Hull hit! -{:.0} damage", projectile.damage),
                        notification_type: NotificationType::Danger,
                        duration: 2.0,
                    });

                    commands.entity(proj_entity).despawn_recursive();
                }
            }
        }
    }
}
