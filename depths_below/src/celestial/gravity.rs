use bevy::prelude::*;
use crate::components::{Submarine, Velocity, SubmarinePhysics};
use crate::events::ShowNotification;
use super::components::*;
use super::resources::*;
use super::events::GravityWarning;

/// Accumulate gravity forces on all GravityAffected entities from all GravityWells.
/// Uses influence_radius to skip distant wells.
pub fn accumulate_gravity(
    config: Res<CelestialConfig>,
    well_query: Query<(&Transform, &GravityWell), Without<BeingConsumed>>,
    mut affected_query: Query<(&Transform, &GravityAffected, &mut GravityForce)>,
) {
    for (affected_transform, _affected, mut gravity_force) in affected_query.iter_mut() {
        let pos = affected_transform.translation.truncate();
        let mut total_force = Vec2::ZERO;

        for (well_transform, well) in well_query.iter() {
            let well_pos = well_transform.translation.truncate();
            let delta = well_pos - pos;
            let distance = delta.length();

            // Skip if outside influence radius
            if distance > well.influence_radius || distance < 1.0 {
                continue;
            }

            let direction = delta / distance;

            let force_magnitude = match well.falloff {
                GravityFalloff::InverseSquare => {
                    well.strength / (distance * distance)
                }
                GravityFalloff::InverseLinear => {
                    well.strength / distance
                }
                GravityFalloff::BlackHole => {
                    // Dramatic ramp: gentle at distance, extreme near event horizon
                    let normalized = (distance / well.influence_radius).clamp(0.01, 1.0);
                    well.strength / (normalized * normalized * distance)
                }
            };

            let clamped = force_magnitude.min(config.max_gravity_force);
            total_force += direction * clamped;
        }

        gravity_force.0 = total_force;
    }
}

/// Apply accumulated gravity to velocity for all gravity-affected entities (creatures, debris)
pub fn apply_gravity_to_velocity(
    time: Res<Time>,
    mut query: Query<(&GravityForce, &GravityAffected, &mut Velocity), Without<Submarine>>,
) {
    let dt = time.delta_seconds();
    for (gravity_force, affected, mut velocity) in query.iter_mut() {
        let acceleration = gravity_force.0 / affected.mass.max(1.0);
        velocity.0 += acceleration * dt;
    }
}

/// Apply gravity to the ship — integrates with existing movement system.
/// Also fires GravityWarning events when pull is significant.
pub fn apply_gravity_to_submarine(
    time: Res<Time>,
    mut sub_query: Query<(&GravityForce, &mut Velocity, &SubmarinePhysics), With<Submarine>>,
    mut warnings: EventWriter<GravityWarning>,
    mut notifications: EventWriter<ShowNotification>,
    well_query: Query<(Entity, &Transform, &GravityWell)>,
    sub_transform_query: Query<&Transform, With<Submarine>>,
    mut warned_light: Local<bool>,
    mut warned_heavy: Local<bool>,
) {
    let dt = time.delta_seconds();

    let Ok((gravity_force, mut velocity, physics)) = sub_query.get_single_mut() else {
        return;
    };

    let force_magnitude = gravity_force.0.length();

    // Apply gravity as acceleration (F = ma, a = F/m)
    let acceleration = gravity_force.0 / physics.mass.max(1.0);
    velocity.0 += acceleration * dt;

    // Warn player when gravity is pulling them
    if force_magnitude > 100.0 && !*warned_light {
        *warned_light = true;
        notifications.send(ShowNotification {
            message: "Gravitational pull detected — watch your trajectory!".into(),
            notification_type: crate::events::NotificationType::Warning,
            duration: 3.0,
        });
    }
    if force_magnitude > 400.0 && !*warned_heavy {
        *warned_heavy = true;
        notifications.send(ShowNotification {
            message: "EXTREME GRAVITY! Full thrust required to escape!".into(),
            notification_type: crate::events::NotificationType::Danger,
            duration: 4.0,
        });
    }

    // Reset warnings when force drops
    if force_magnitude < 50.0 {
        *warned_light = false;
        *warned_heavy = false;
    }

    // Fire warning events for UI
    if force_magnitude > 50.0 {
        if let Ok(sub_transform) = sub_transform_query.get_single() {
            let sub_pos = sub_transform.translation.truncate();
            // Find the strongest gravity source
            if let Some((entity, _, _)) = well_query.iter()
                .min_by(|(_, ta, _), (_, tb, _)| {
                    let da = ta.translation.truncate().distance(sub_pos);
                    let db = tb.translation.truncate().distance(sub_pos);
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
            {
                warnings.send(GravityWarning {
                    source: entity,
                    pull_strength: force_magnitude,
                });
            }
        }
    }
}
