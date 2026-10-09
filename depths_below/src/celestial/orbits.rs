use bevy::prelude::*;
use crate::events::{ShowNotification, NotificationType};
use super::components::*;
use super::resources::*;
use super::events::StarDestroyed;

/// Evaluates orbital positions at current galaxy_time using Keplerian math.
/// Cheap: just trig per orbiting body per frame.
pub fn update_orbital_positions(
    time: Res<Time>,
    mut galaxy: ResMut<GalaxyState>,
    parent_query: Query<&Transform, (With<CelestialBody>, Without<OrbitalPath>)>,
    mut orbit_query: Query<(&OrbitalPath, &mut Transform), Without<FreeFlight>>,
) {
    galaxy.galaxy_time += time.delta_secs() as f64;
    let t = galaxy.galaxy_time as f32;

    for (orbit, mut transform) in orbit_query.iter_mut() {
        let Ok(parent_transform) = parent_query.get(orbit.parent) else {
            continue;
        };

        let parent_pos = parent_transform.translation.truncate();

        // Simplified Keplerian: treat as elliptical motion
        let angular_speed = std::f32::consts::TAU / orbit.period;
        let direction = if orbit.clockwise { -1.0 } else { 1.0 };
        let angle = orbit.phase + angular_speed * t * direction;

        // Ellipse: r = a(1 - e^2) / (1 + e*cos(theta))
        let r = orbit.semi_major_axis * (1.0 - orbit.eccentricity * orbit.eccentricity)
            / (1.0 + orbit.eccentricity * angle.cos());

        let x = parent_pos.x + r * angle.cos();
        let y = parent_pos.y + r * angle.sin();

        transform.translation.x = x;
        transform.translation.y = y;
    }
}

/// When a star dies, convert OrbitalPath → FreeFlight with tangential velocity.
/// Planets go flying!
pub fn destabilize_orbits(
    mut commands: Commands,
    mut star_destroyed_events: MessageReader<StarDestroyed>,
    orbit_query: Query<(Entity, &OrbitalPath, &Transform)>,
    _galaxy: Res<GalaxyState>,
    config: Res<CelestialConfig>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for event in star_destroyed_events.read() {
        let star_pos = event.position;

        for (entity, orbit, transform) in orbit_query.iter() {
            if orbit.parent != event.star {
                continue;
            }

            // Calculate tangential velocity at current position
            let pos = transform.translation.truncate();
            let radial = (pos - star_pos).normalize_or_zero();
            // Tangent is perpendicular to radial
            let tangent = if orbit.clockwise {
                Vec2::new(radial.y, -radial.x)
            } else {
                Vec2::new(-radial.y, radial.x)
            };

            // Speed from orbital velocity + explosion kick
            let orbital_speed = std::f32::consts::TAU * orbit.semi_major_axis / orbit.period;
            let kick_speed = config.freed_planet_speed_multiplier;
            let velocity = tangent * orbital_speed + radial * kick_speed;

            // Remove orbit, add free flight
            commands.entity(entity)
                .remove::<OrbitalPath>()
                .insert(FreeFlight { velocity });
        }

        notifications.write(ShowNotification {
            message: "STAR DESTROYED! Orbital bodies destabilized!".into(),
            notification_type: NotificationType::Danger,
            duration: 5.0,
        });
    }
}

/// Move free-flight bodies (freed planets, debris) — simple velocity integration
pub fn update_free_flight(
    time: Res<Time>,
    mut query: Query<(&FreeFlight, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (flight, mut transform) in query.iter_mut() {
        transform.translation.x += flight.velocity.x * dt;
        transform.translation.y += flight.velocity.y * dt;
    }
}

/// How many segments one full orbit is sampled into. The path is an ellipse
/// (`r = a(1-e²)/(1+e·cos θ)`), not a circle, so it has to be sampled rather
/// than drawn as a ring.
const ORBIT_SAMPLES: usize = 360;
/// Samples drawn, then samples skipped — this is what makes it dotted.
const ORBIT_DASH: usize = 2;
const ORBIT_GAP: usize = 4;

/// Faint. A navigation aid behind the scene, not part of it: at any moment
/// several of these cross the screen at once, and a bright one would draw the
/// eye away from whatever is actually happening.
const ORBIT_LINE_COLOR: Color = Color::srgba(0.45, 0.62, 0.85, 0.22);

/// Draws each orbiting body's path as a dotted ellipse.
///
/// Sampled from the same `r = a(1-e²)/(1+e·cos θ)` that
/// `update_orbital_positions` integrates, so the line is where the planet
/// will actually be rather than an idealised circle through its current
/// position — with eccentricity up to 0.3 those differ visibly.
pub fn draw_orbit_paths(
    mut gizmos: Gizmos,
    parent_query: Query<&Transform, (With<CelestialBody>, Without<OrbitalPath>)>,
    orbit_query: Query<&OrbitalPath>,
) {
    for orbit in orbit_query.iter() {
        let Ok(parent_transform) = parent_query.get(orbit.parent) else { continue };
        let center = parent_transform.translation.truncate();

        let at = |theta: f32| -> Vec2 {
            let r = orbit.semi_major_axis * (1.0 - orbit.eccentricity * orbit.eccentricity)
                / (1.0 + orbit.eccentricity * theta.cos());
            center + Vec2::new(r * theta.cos(), r * theta.sin())
        };

        let step = std::f32::consts::TAU / ORBIT_SAMPLES as f32;
        for i in 0..ORBIT_SAMPLES {
            // Draw ORBIT_DASH samples out of every DASH+GAP.
            if i % (ORBIT_DASH + ORBIT_GAP) >= ORBIT_DASH {
                continue;
            }
            gizmos.line_2d(at(i as f32 * step), at((i + 1) as f32 * step), ORBIT_LINE_COLOR);
        }
    }
}
