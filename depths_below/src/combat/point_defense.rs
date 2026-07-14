use bevy::prelude::*;
use super::targeting::fire_groups::InterceptMode;
use super::new_projectiles::{MissileProjectile, Projectile};
use super::*;

// ============================================================================
// POINT DEFENSE INTERCEPT MODE
// Any weapon flagged with InterceptMode auto-shoots incoming missiles.
// Player assigns intercept mode with I key + click on weapon.
// ============================================================================

/// Toggle intercept mode on a weapon during build mode
pub fn toggle_intercept_mode(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    occupancy: Res<crate::building::GridOccupancy>,
    mut commands: Commands,
    weapon_query: Query<(Entity, &Module, Option<&InterceptMode>), With<Weapon>>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    if !keyboard.pressed(KeyCode::KeyI) || !mouse.just_pressed(MouseButton::Left) { return; }

    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_transform)) = camera_query.single() else { return };
    let Some(cursor) = window.cursor_position()
        .and_then(|p| camera.viewport_to_world_2d(cam_transform, p).ok())
    else { return };

    let grid_pos = IVec2::new(
        (cursor.x / 66.0).round() as i32,
        ((cursor.y + 33.0) / 66.0).round() as i32,
    );

    if let Some(&entity) = occupancy.cells.get(&grid_pos) {
        if let Ok((_, module, intercept)) = weapon_query.get(entity) {
            if intercept.is_some() {
                commands.entity(entity).remove::<InterceptMode>();
                notifications.write(ShowNotification {
                    message: format!("{}: Intercept mode OFF", module.module_type.name()),
                    notification_type: NotificationType::Info,
                    duration: 2.0,
                });
            } else {
                commands.entity(entity).insert(InterceptMode);
                notifications.write(ShowNotification {
                    message: format!("{}: Intercept mode ON — will target incoming missiles", module.module_type.name()),
                    notification_type: NotificationType::Warning,
                    duration: 2.0,
                });
            }
        }
    }
}

/// Intercept system: weapons in intercept mode auto-fire at incoming missiles
pub fn intercept_missiles(
    time: Res<Time>,
    ship_query: Query<&Transform, With<Ship>>,
    mut intercept_weapons: Query<(
        &Module, &mut Weapon, &mut WeaponCooldown, &GlobalTransform,
    ), (With<InterceptMode>, Without<DestroyedModule>)>,
    missile_query: Query<(Entity, &Transform, &Velocity), With<MissileProjectile>>,
    mut commands: Commands,
) {
    let Ok(ship_transform) = ship_query.single() else { return };
    let ship_pos = ship_transform.translation.truncate();

    // Find incoming missiles (heading toward the ship)
    let mut threats: Vec<(Entity, Vec2, f32)> = Vec::new();
    for (entity, transform, velocity) in missile_query.iter() {
        let missile_pos = transform.translation.truncate();
        let to_ship = ship_pos - missile_pos;
        let dist = to_ship.length();

        // Only track missiles heading roughly toward us within 800 units
        if dist > 800.0 { continue; }
        let heading_toward = to_ship.normalize_or_zero().dot(velocity.0.normalize_or_zero());
        if heading_toward < 0.3 { continue; } // Must be heading somewhat toward us

        threats.push((entity, missile_pos, dist));
    }

    if threats.is_empty() { return; }

    // Sort by distance (closest first)
    threats.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    // Each intercept weapon targets the closest threat
    for (module, mut weapon, mut cooldown, global_transform) in intercept_weapons.iter_mut() {
        if !module.is_active { continue; }
        cooldown.timer.tick(time.delta());
        if !cooldown.timer.is_finished() { continue; }
        if weapon.ammo <= 0 { continue; }

        let weapon_pos = global_transform.translation().truncate();

        // Find closest threat in range
        let Some((_threat_entity, threat_pos, _)) = threats.iter()
            .find(|(_, pos, _)| weapon_pos.distance(*pos) < weapon.range)
        else { continue; };

        // Fire at the missile
        let direction = (*threat_pos - weapon_pos).normalize_or_zero();
        let proj_speed = 600.0;

        cooldown.timer.reset();
        weapon.ammo = weapon.ammo.saturating_sub(1);

        let angle = direction.y.atan2(direction.x);

        commands.spawn((
            (Sprite {
                    color: Color::srgb(0.3, 1.0, 0.5), 
                    custom_size: Some(Vec2::new(6.0, 2.0)),
                    ..default()
                }, Transform {
                    translation: Vec3::new(weapon_pos.x, weapon_pos.y, 0.5),
                    rotation: Quat::from_rotation_z(angle),
                    ..default()
                }),
            Projectile {
                damage: weapon.damage * 0.5, // PD rounds do less damage but only need to hit missiles
                speed: proj_speed,
                lifetime: 1.5,
                max_lifetime: 1.5,
                owner: Entity::PLACEHOLDER,
                damage_type: super::new_projectiles::ProjectileDamageType::Kinetic,
                penetration: 5.0,
                has_penetrated: false,
                ammo: None, // PD rounds have no special on-hit behavior
                caliber: 0.3,
                last_hit: None,
            },
            Velocity(direction * proj_speed),
        ));

        // Muzzle flash
        spawn_hit_effect(&mut commands, weapon_pos + direction * 20.0, Color::srgb(0.3, 0.9, 0.4), 6.0);
    }
}

/// Check if PD projectiles hit incoming missiles — destroy them.
/// Uses the missile spatial grid to only distance-check missiles near each PD shot.
pub fn pd_missile_collision(
    mut commands: Commands,
    proj_query: Query<(Entity, &Projectile, &Transform)>,
    missile_query: Query<&Transform, With<MissileProjectile>>,
    missile_grid: Res<crate::spatial::MissileGrid>,
) {
    for (proj_entity, _proj, proj_transform) in proj_query.iter() {
        let proj_pos = proj_transform.translation.truncate();

        for (missile_entity, _) in missile_grid.0.nearby(proj_pos, 20.0) {
            let Ok(missile_transform) = missile_query.get(missile_entity) else { continue };
            let missile_pos = missile_transform.translation.truncate();
            let dist = proj_pos.distance(missile_pos);

            if dist < 20.0 {
                // Missile destroyed by point defense!
                commands.entity(missile_entity).despawn();
                commands.entity(proj_entity).despawn();

                // Small explosion
                spawn_hit_effect(&mut commands, missile_pos, Color::srgb(1.0, 0.6, 0.2), 15.0);
                break;
            }
        }
    }
}
