use bevy::prelude::*;
use crate::components::*;
use crate::resources::*;
use crate::events::*;

/// Checks for pressure damage at current depth
pub fn check_pressure_damage(
    time: Res<Time>,
    config: Res<GameConfig>,
    submarine_query: Query<&Depth, With<Submarine>>,
    mut hull_query: Query<(Entity, &mut HullSegment)>,
    mut breach_events: EventWriter<HullBreached>,
    mut damage_events: EventWriter<SubmarineDamaged>,
    mut notifications: EventWriter<ShowNotification>,
    mut warned_50: Local<bool>,
    mut warned_30: Local<bool>,
) {
    let Ok(depth) = submarine_query.get_single() else {
        return;
    };

    // Use actual pressure to scale damage (Phase 4.4)
    let current_pressure = depth.0 * config.pressure_per_meter;

    for (entity, mut hull) in hull_query.iter_mut() {
        // Check if depth exceeds hull rating
        if depth.0 > hull.depth_rating {
            let excess = depth.0 - hull.depth_rating;
            // Scale damage by pressure value for more realistic physics
            let pressure_factor = (current_pressure / 100.0).max(1.0);
            let damage = excess * config.pressure_damage_multiplier * pressure_factor * time.delta_seconds();

            hull.health -= damage;

            let health_pct = hull.health / hull.max_health;

            // Warning at 50% health
            if health_pct <= 0.5 && !*warned_50 {
                *warned_50 = true;
                notifications.send(ShowNotification {
                    message: "Hull segment at 50%! Pressure damage increasing!".into(),
                    notification_type: NotificationType::Warning,
                    duration: 3.0,
                });
            }

            // Warning at 30% health
            if health_pct <= 0.3 && !*warned_30 {
                *warned_30 = true;
                notifications.send(ShowNotification {
                    message: "Hull segment critical at 30%! Breach imminent!".into(),
                    notification_type: NotificationType::Danger,
                    duration: 3.0,
                });
            }

            // Check for breach
            if hull.health <= hull.max_health * 0.3 && !hull.is_flooded {
                // Hull is critically damaged, start flooding
                hull.is_flooded = true;
                breach_events.send(HullBreached {
                    segment: entity,
                    severity: 1.0 - (hull.health / hull.max_health),
                });
                // Also fire damage event at breach point (not just at 0%)
                damage_events.send(SubmarineDamaged {
                    source: DamageSource::Pressure,
                    amount: damage,
                    position: None,
                    direction: None,
                });
            }

            if hull.health <= 0.0 {
                hull.health = 0.0;
                damage_events.send(SubmarineDamaged {
                    source: DamageSource::Pressure,
                    amount: 10.0,
                    position: None,
                    direction: None,
                });
            }
        } else {
            // Reset warnings when out of danger
            *warned_50 = false;
            *warned_30 = false;
        }
    }
}
