use bevy::prelude::*;
use crate::components::Ship;
use crate::celestial::components::*;

// ============================================================================
// SCREEN EFFECTS
// Full-screen overlay effects that respond to game state.
// Radiation tint, gravity distortion, flare flash, low oxygen warning.
// ============================================================================

/// Screen overlay entity — spawned once, updated each frame
#[derive(Component)]
pub struct ScreenOverlay;

/// Marker for radiation warning overlay
#[derive(Component)]
pub struct RadiationOverlay;

/// Marker for gravity warning overlay
#[derive(Component)]
pub struct GravityOverlay;

/// Marker for low oxygen warning overlay
#[derive(Component)]
pub struct OxygenWarningOverlay;

/// Master system: updates all screen overlays based on proximity to hazards
pub fn update_screen_effects(
    mut commands: Commands,
    ship_query: Query<&Transform, With<Ship>>,
    star_query: Query<(&Transform, &Star, &CelestialBody), Without<Ship>>,
    _bh_query: Query<(&Transform, &BlackHole, &CelestialBody), Without<Ship>>,
    gravity_force: Query<&crate::celestial::components::GravityForce, With<Ship>>,
    oxygen_state: Res<crate::resources::OxygenState>,
    existing_rad: Query<Entity, With<RadiationOverlay>>,
    existing_grav: Query<Entity, With<GravityOverlay>>,
    existing_o2: Query<Entity, With<OxygenWarningOverlay>>,
    time: Res<Time>,
) {
    let Ok(ship_transform) = ship_query.single() else { return };
    let ship_pos = ship_transform.translation.truncate();
    let _ = &star_query; // kept as a system param; radiation mechanic disabled per request

    // Radiation overlay removed along with check_radiation_damage — clean up
    // any leftover overlay entity from before this change (e.g. a loaded save).
    for entity in existing_rad.iter() {
        commands.entity(entity).despawn();
    }

    // === GRAVITY PROXIMITY OVERLAY ===
    let gravity_strength = gravity_force.single()
        .map(|gf| gf.0.length())
        .unwrap_or(0.0);

    let gravity_intensity = (gravity_strength / 300.0).clamp(0.0, 1.0);

    if gravity_intensity > 0.1 {
        if existing_grav.is_empty() {
            commands.spawn((
                (Node {
                        position_type: PositionType::Absolute,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)), ZIndex(4)),
                GravityOverlay,
            ));
        }
    }
    for entity in existing_grav.iter() {
        if gravity_intensity < 0.05 {
            commands.entity(entity).despawn();
        } else {
            // Dark purple-red vignette for gravity
            let alpha = gravity_intensity * 0.20;
            commands.entity(entity).insert(
                BackgroundColor(Color::srgba(0.3, 0.05, 0.1, alpha))
            );
        }
    }

    // === LOW OXYGEN WARNING OVERLAY ===
    let o2_pct = if oxygen_state.max_oxygen > 0.0 {
        oxygen_state.current_oxygen / oxygen_state.max_oxygen
    } else {
        1.0
    };

    if o2_pct < 0.25 {
        if existing_o2.is_empty() {
            commands.spawn((
                (Node {
                        position_type: PositionType::Absolute,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)), ZIndex(6)),
                OxygenWarningOverlay,
            ));
        }
    }
    for entity in existing_o2.iter() {
        if o2_pct > 0.30 {
            commands.entity(entity).despawn();
        } else {
            // Pulsing red-black vignette when suffocating
            let pulse = (time.elapsed_secs() * 3.0).sin() * 0.5 + 0.5;
            let severity = 1.0 - (o2_pct / 0.25).clamp(0.0, 1.0);
            let alpha = severity * 0.3 * pulse;
            commands.entity(entity).insert(
                BackgroundColor(Color::srgba(0.5, 0.0, 0.0, alpha))
            );
        }
    }
}
