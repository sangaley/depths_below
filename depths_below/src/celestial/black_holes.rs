use bevy::prelude::*;
use crate::components::Submarine;
use crate::events::{SubmarineDamaged, DamageSource, ShowNotification, NotificationType};
use super::components::*;
use super::resources::*;
use super::events::*;

/// Check if any entity crossed the event horizon — begin consumption.
/// Ship gets gradual crush damage instead of instant death.
pub fn check_event_horizon(
    mut commands: Commands,
    bh_query: Query<(Entity, &Transform, &BlackHole)>,
    body_query: Query<(Entity, &Transform, &CelestialBody), (Without<BlackHole>, Without<BeingConsumed>)>,
    sub_query: Query<&Transform, With<Submarine>>,
    mut damage_events: EventWriter<SubmarineDamaged>,
    mut notifications: EventWriter<ShowNotification>,
    config: Res<CelestialConfig>,
    time: Res<Time>,
) {
    for (bh_entity, bh_transform, bh) in bh_query.iter() {
        let bh_pos = bh_transform.translation.truncate();

        // Check celestial bodies crossing event horizon
        for (body_entity, body_transform, body) in body_query.iter() {
            let body_pos = body_transform.translation.truncate();
            let dist = body_pos.distance(bh_pos);

            if dist < bh.event_horizon_radius + body.radius * 0.5 {
                // Begin consumption
                commands.entity(body_entity).insert(BeingConsumed {
                    by_black_hole: bh_entity,
                    progress: 0.0,
                });

                notifications.send(ShowNotification {
                    message: format!("{} crossing event horizon!", body.name),
                    notification_type: NotificationType::Danger,
                    duration: 4.0,
                });
            }
        }

        // Check ship — gradual crush damage, not instant death
        if let Ok(sub_transform) = sub_query.get_single() {
            let sub_pos = sub_transform.translation.truncate();
            let dist = sub_pos.distance(bh_pos);

            // Crush damage in accretion disk zone
            if dist < bh.accretion_disk_radius {
                let proximity = 1.0 - (dist / bh.accretion_disk_radius).clamp(0.0, 1.0);
                let damage = config.black_hole_crush_damage_rate
                    * proximity * proximity  // Quadratic ramp
                    * bh.tidal_force_multiplier
                    * time.delta_seconds();

                if damage > 0.1 {
                    damage_events.send(SubmarineDamaged {
                        source: DamageSource::Radiation,
                        amount: damage,
                        position: None,
                        direction: Some((bh_pos - sub_pos).normalize_or_zero()),
                    });
                }
            }

            // Critical warning at event horizon
            if dist < bh.event_horizon_radius * 1.5 {
                notifications.send(ShowNotification {
                    message: "EVENT HORIZON! ESCAPE NOW OR BE CONSUMED!".into(),
                    notification_type: NotificationType::Danger,
                    duration: 2.0,
                });
            }
        }
    }
}

/// Animate/progress consumption (visual spiral-in, then despawn).
pub fn process_consumption(
    time: Res<Time>,
    mut commands: Commands,
    config: Res<CelestialConfig>,
    mut consuming_query: Query<(Entity, &mut BeingConsumed, &mut Transform, &CelestialBody)>,
    bh_query: Query<(&Transform, &mut BlackHole, &mut GravityWell), Without<BeingConsumed>>,
    mut consumed_events: EventWriter<BodyConsumed>,
    mut planet_consumed_events: EventWriter<PlanetConsumed>,
    planet_query: Query<&Planet>,
    mut notifications: EventWriter<ShowNotification>,
) {
    let dt = time.delta_seconds();

    for (entity, mut consuming, mut transform, body) in consuming_query.iter_mut() {
        consuming.progress += config.black_hole_consume_speed * dt;

        // Spiral-in visual: shrink and rotate toward black hole
        let scale_factor = (1.0 - consuming.progress).max(0.01);
        transform.scale = Vec3::splat(scale_factor);
        transform.rotation *= Quat::from_rotation_z(dt * 3.0); // Spin

        // Move toward black hole
        if let Ok((bh_transform, _, _)) = bh_query.get(consuming.by_black_hole) {
            let bh_pos = bh_transform.translation.truncate();
            let pos = transform.translation.truncate();
            let direction = (bh_pos - pos).normalize_or_zero();
            let pull_speed = 200.0 * consuming.progress;
            transform.translation.x += direction.x * pull_speed * dt;
            transform.translation.y += direction.y * pull_speed * dt;
        }

        // Consumption complete
        if consuming.progress >= 1.0 {
            let mass = body.mass;

            // Check if it's a planet
            if let Ok(planet) = planet_query.get(entity) {
                planet_consumed_events.send(PlanetConsumed {
                    planet: entity,
                    black_hole: consuming.by_black_hole,
                    planet_type: planet.planet_type,
                });
                notifications.send(ShowNotification {
                    message: format!("{} consumed by black hole!", body.name),
                    notification_type: NotificationType::Danger,
                    duration: 5.0,
                });
            }

            consumed_events.send(BodyConsumed {
                entity,
                black_hole: consuming.by_black_hole,
                mass_gained: mass,
            });

            commands.entity(entity).despawn_recursive();
        }
    }
}

/// Black holes grow stronger as they consume mass.
pub fn grow_black_hole(
    mut consumed_events: EventReader<BodyConsumed>,
    mut bh_query: Query<(&mut BlackHole, &mut GravityWell, &mut CelestialBody)>,
    mut notifications: EventWriter<ShowNotification>,
) {
    for event in consumed_events.iter() {
        if let Ok((mut bh, mut well, mut body)) = bh_query.get_mut(event.black_hole) {
            bh.consumed_mass += event.mass_gained;
            body.mass += event.mass_gained;

            // Grow event horizon and gravity proportional to consumed mass
            let growth_factor = (event.mass_gained / 1000.0).clamp(0.01, 0.5);
            bh.event_horizon_radius *= 1.0 + growth_factor * 0.1;
            bh.accretion_disk_radius *= 1.0 + growth_factor * 0.05;
            well.strength *= 1.0 + growth_factor * 0.15;
            well.influence_radius *= 1.0 + growth_factor * 0.05;

            notifications.send(ShowNotification {
                message: "Black hole growing stronger...".into(),
                notification_type: crate::events::NotificationType::Warning,
                duration: 3.0,
            });
        }
    }
}
