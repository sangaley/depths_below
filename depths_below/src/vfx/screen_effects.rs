use bevy::prelude::*;
use crate::components::Submarine;
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
    sub_query: Query<&Transform, With<Submarine>>,
    star_query: Query<(&Transform, &Star, &CelestialBody), Without<Submarine>>,
    _bh_query: Query<(&Transform, &BlackHole, &CelestialBody), Without<Submarine>>,
    gravity_force: Query<&crate::celestial::components::GravityForce, With<Submarine>>,
    oxygen_state: Res<crate::resources::OxygenState>,
    existing_rad: Query<Entity, With<RadiationOverlay>>,
    existing_grav: Query<Entity, With<GravityOverlay>>,
    existing_o2: Query<Entity, With<OxygenWarningOverlay>>,
    time: Res<Time>,
) {
    let Ok(sub_transform) = sub_query.get_single() else { return };
    let sub_pos = sub_transform.translation.truncate();

    // === RADIATION PROXIMITY OVERLAY ===
    let mut max_radiation_proximity = 0.0_f32;
    for (star_transform, _star, body) in star_query.iter() {
        let dist = sub_pos.distance(star_transform.translation.truncate());
        let danger_range = body.radius * 3.0;
        if dist < danger_range {
            let proximity = 1.0 - (dist / danger_range).clamp(0.0, 1.0);
            max_radiation_proximity = max_radiation_proximity.max(proximity);
        }
    }

    // Spawn or update radiation overlay
    if max_radiation_proximity > 0.05 {
        if existing_rad.is_empty() {
            commands.spawn((
                NodeBundle {
                    style: Style {
                        position_type: PositionType::Absolute,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    background_color: Color::rgba(0.0, 0.0, 0.0, 0.0).into(),
                    z_index: ZIndex::Global(5),
                    ..default()
                },
                RadiationOverlay,
            ));
        }
    }
    // Update radiation overlay alpha
    for entity in existing_rad.iter() {
        if max_radiation_proximity < 0.02 {
            commands.entity(entity).despawn();
        } else {
            // Yellow-orange tint at edges, intensity scales with proximity
            let alpha = max_radiation_proximity * 0.25;
            commands.entity(entity).insert(
                BackgroundColor(Color::rgba(0.8, 0.4, 0.1, alpha))
            );
        }
    }

    // === GRAVITY PROXIMITY OVERLAY ===
    let gravity_strength = gravity_force.get_single()
        .map(|gf| gf.0.length())
        .unwrap_or(0.0);

    let gravity_intensity = (gravity_strength / 300.0).clamp(0.0, 1.0);

    if gravity_intensity > 0.1 {
        if existing_grav.is_empty() {
            commands.spawn((
                NodeBundle {
                    style: Style {
                        position_type: PositionType::Absolute,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    background_color: Color::rgba(0.0, 0.0, 0.0, 0.0).into(),
                    z_index: ZIndex::Global(4),
                    ..default()
                },
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
                BackgroundColor(Color::rgba(0.3, 0.05, 0.1, alpha))
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
                NodeBundle {
                    style: Style {
                        position_type: PositionType::Absolute,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    background_color: Color::rgba(0.0, 0.0, 0.0, 0.0).into(),
                    z_index: ZIndex::Global(6),
                    ..default()
                },
                OxygenWarningOverlay,
            ));
        }
    }
    for entity in existing_o2.iter() {
        if o2_pct > 0.30 {
            commands.entity(entity).despawn();
        } else {
            // Pulsing red-black vignette when suffocating
            let pulse = (time.elapsed_seconds() * 3.0).sin() * 0.5 + 0.5;
            let severity = 1.0 - (o2_pct / 0.25).clamp(0.0, 1.0);
            let alpha = severity * 0.3 * pulse;
            commands.entity(entity).insert(
                BackgroundColor(Color::rgba(0.5, 0.0, 0.0, alpha))
            );
        }
    }
}
