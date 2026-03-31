use bevy::prelude::*;
use crate::components::*;
use crate::events::*;
use crate::celestial::components::{GravityAffected, GravityForce};
use crate::building::multiblock::components::*;

// ============================================================================
// SECTION SEVERANCE
// When the connection between ship sections is destroyed, the detached
// section breaks off with ship velocity + impact force.
// Becomes debris. If it had reactor/ammo = tumbling bomb.
// ============================================================================

/// Marker for a detached ship section — flying debris with momentum
#[derive(Component)]
pub struct DetachedSection {
    pub has_reactor: bool,
    pub has_ammo: bool,
    pub has_fuel: bool,
    pub meltdown_timer: Option<f32>,
    pub mass: f32,
}

/// System: detect when hull segments are destroyed and check for section severance
pub fn check_section_severance(
    mut commands: Commands,
    destroyed_hull: Query<(&HullSegment, &GlobalTransform), Added<crate::components::HullDestroyed>>,
    module_query: Query<(Entity, &Module, &GlobalTransform), Without<DestroyedModule>>,
    sub_query: Query<(&Transform, &Velocity), With<Submarine>>,
    mut notifications: EventWriter<ShowNotification>,
) {
    if destroyed_hull.is_empty() { return; }
    let Ok((sub_transform, sub_velocity)) = sub_query.get_single() else { return; };

    for (hull, hull_gt) in destroyed_hull.iter() {
        let hull_pos = hull_gt.translation().truncate();

        // Check if any modules are now isolated (simplified check)
        // Full implementation would use flood-fill from ship core
        // For now: modules far from center that lost adjacent hull = at risk

        let ship_center = sub_transform.translation.truncate();

        for (module_entity, module, module_gt) in module_query.iter() {
            let module_pos = module_gt.translation().truncate();
            let dist_to_hull = module_pos.distance(hull_pos);

            // Only check modules near the destroyed hull
            if dist_to_hull > 130.0 { continue; }

            let dist_to_center = module_pos.distance(ship_center);
            // Modules far from center + near destroyed hull = potential severance
            if dist_to_center < 200.0 { continue; } // Too close to center to sever

            // Check if this module has any intact hull between it and center
            // Simplified: if distance to destroyed hull < distance to center, likely severed
            if dist_to_hull < dist_to_center * 0.5 {
                // SEVER!
                let has_reactor = matches!(module.module_type,
                    ModuleType::SmallReactor | ModuleType::StandardReactor |
                    ModuleType::LargeReactor | ModuleType::FusionReactor);
                let has_ammo = matches!(module.module_type,
                    ModuleType::AmmoFeedUnit | ModuleType::WarheadBay);
                let has_fuel = module.module_type == ModuleType::FuelTank;

                // Calculate ejection velocity: ship velocity + impact kick
                let impact_dir = (module_pos - hull_pos).normalize_or_zero();
                let kick = impact_dir * 80.0;
                let eject_velocity = sub_velocity.0 + kick;

                // Detach the module from the ship
                commands.entity(module_entity)
                    .remove_parent()
                    .insert(DetachedSection {
                        has_reactor,
                        has_ammo,
                        has_fuel,
                        meltdown_timer: if has_reactor { Some(10.0) } else { None },
                        mass: 50.0,
                    })
                    .insert(Velocity(eject_velocity))
                    .insert(GravityAffected { mass: 50.0 })
                    .insert(GravityForce::default());

                notifications.send(ShowNotification {
                    message: format!("SECTION SEVERED! {} detached!", module.module_type.name()),
                    notification_type: NotificationType::Danger,
                    duration: 4.0,
                });

                if has_reactor {
                    notifications.send(ShowNotification {
                        message: "WARNING: Severed section contains reactor! Meltdown in 10s!".into(),
                        notification_type: NotificationType::Danger,
                        duration: 5.0,
                    });
                }
            }
        }
    }
}

/// Move detached sections — they tumble and drift, affected by gravity
pub fn move_detached_sections(
    time: Res<Time>,
    mut commands: Commands,
    mut section_query: Query<(Entity, &mut DetachedSection, &mut Transform, &Velocity, &GravityForce)>,
    mut notifications: EventWriter<ShowNotification>,
) {
    let dt = time.delta_seconds();

    for (entity, mut section, mut transform, velocity, gravity) in section_query.iter_mut() {
        // Move
        transform.translation.x += (velocity.0.x + gravity.0.x) * dt;
        transform.translation.y += (velocity.0.y + gravity.0.y) * dt;

        // Tumble
        transform.rotation *= Quat::from_rotation_z(1.5 * dt);

        // Reactor meltdown countdown
        if let Some(ref mut timer) = section.meltdown_timer {
            *timer -= dt;
            if *timer <= 0.0 {
                // BOOM
                notifications.send(ShowNotification {
                    message: "Severed reactor EXPLODED!".into(),
                    notification_type: NotificationType::Danger,
                    duration: 3.0,
                });

                let pos = transform.translation.truncate();
                super::spawn_hit_effect(&mut commands, pos, Color::rgb(1.0, 0.5, 0.1), 200.0);

                commands.entity(entity).despawn_recursive();
            }
        }

        // Ammo cook-off (random chance each second)
        if section.has_ammo && rand::random::<f32>() < 0.02 * dt {
            let pos = transform.translation.truncate();
            super::spawn_hit_effect(&mut commands, pos, Color::rgb(1.0, 0.4, 0.1), 80.0);
            notifications.send(ShowNotification {
                message: "Ammo cook-off in debris!".into(),
                notification_type: NotificationType::Warning,
                duration: 2.0,
            });
            commands.entity(entity).despawn_recursive();
        }
    }
}

/// Detached sections can collide with creatures and damage them
pub fn debris_collision(
    mut commands: Commands,
    section_query: Query<(Entity, &DetachedSection, &Transform, &Velocity)>,
    mut creature_query: Query<(Entity, &Transform, &mut Creature), Without<DetachedSection>>,
) {
    for (section_entity, section, section_transform, section_vel) in section_query.iter() {
        let section_pos = section_transform.translation.truncate();
        let speed = section_vel.0.length();

        if speed < 20.0 { continue; } // Not moving fast enough to hurt

        for (_creature_entity, creature_transform, mut creature) in creature_query.iter_mut() {
            if creature.health <= 0.0 { continue; }

            let creature_pos = creature_transform.translation.truncate();
            let dist = section_pos.distance(creature_pos);

            if dist < 60.0 {
                // COLLISION! Damage based on speed and mass
                let impact_damage = speed * section.mass * 0.01;
                creature.health -= impact_damage;

                super::spawn_hit_effect(&mut commands, section_pos, Color::rgb(0.8, 0.6, 0.2), 20.0);
                super::spawn_floating_damage(&mut commands, section_pos, impact_damage, Color::rgb(0.7, 0.5, 0.2));

                // Destroy the debris on impact
                commands.entity(section_entity).despawn_recursive();
                break;
            }
        }
    }
}
