use bevy::prelude::*;
use bevy::input::mouse::MouseWheel;
use crate::components::Ship;
use crate::resources::DepthState;
use crate::events::ShipDamaged;
use crate::states::GameState;

/// Marker for the main camera
#[derive(Component)]
pub struct MainCamera;

/// Camera zoom and shake state
#[derive(Resource)]
pub struct CameraState {
    pub zoom: f32,
    pub min_zoom: f32,
    pub max_zoom: f32,
    pub shake_intensity: f32,
    pub shake_decay: f32,
    pub shake_offset: Vec2,
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            zoom: 1.8,
            min_zoom: 0.5,
            // Was 8.0 — at scale 8 on a 1280x720 window the visible area is
            // only ~10,240x5,760 world units, so an enemy holding an
            // 8,000-9,600 unit standoff (now that shots can actually reach
            // that far, see combat::PROJECTILE_SPEED) sat right at the
            // screen edge even fully zoomed out. 20.0 gives ~25,600x14,400
            // visible, comfortably framing a long-range fight.
            max_zoom: 20.0,
            shake_intensity: 0.0,
            shake_decay: 5.0,
            shake_offset: Vec2::ZERO,
        }
    }
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<CameraState>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                (
                    camera_zoom_input,
                    camera_shake_on_damage,
                    camera_shake_update,
                    camera_follow_ship,
                    update_background_color,
                    update_depth_vignette,
                )
                    .chain()
                    .run_if(in_state(GameState::Exploring)
                        .or_else(in_state(GameState::StationDocked))),
            );
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        MainCamera,
    ));
}

/// Scroll wheel + keyboard (+/-) zoom
fn camera_zoom_input(
    mut scroll_events: MessageReader<MouseWheel>,
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut camera_state: ResMut<CameraState>,
) {
    for event in scroll_events.read() {
        let zoom_delta = -event.y * 0.1;
        camera_state.zoom = (camera_state.zoom + zoom_delta)
            .clamp(camera_state.min_zoom, camera_state.max_zoom);
    }

    // Keyboard zoom: +/= zooms in, -/_ zooms out (plus numpad variants)
    let kb_speed = 1.5 * time.delta_secs();
    if keyboard.pressed(KeyCode::Equal) || keyboard.pressed(KeyCode::NumpadAdd) {
        camera_state.zoom = (camera_state.zoom - kb_speed)
            .clamp(camera_state.min_zoom, camera_state.max_zoom);
    }
    if keyboard.pressed(KeyCode::Minus) || keyboard.pressed(KeyCode::NumpadSubtract) {
        camera_state.zoom = (camera_state.zoom + kb_speed)
            .clamp(camera_state.min_zoom, camera_state.max_zoom);
    }
}

/// Trigger shake on ship damage.
/// Was `(amount * 0.3).min(15.0)` stacking up to a cap of 20, oscillating at
/// ~6-8Hz — with weapon damage now going up to 80, that pegged near max on
/// nearly every hit and stayed there under sustained fire. Removed.
fn camera_shake_on_damage(
    mut damage_events: MessageReader<ShipDamaged>,
    mut _camera_state: ResMut<CameraState>,
) {
    for _event in damage_events.read() {}
}

/// Decay shake over time and compute offset
pub fn camera_shake_update(
    time: Res<Time>,
    mut camera_state: ResMut<CameraState>,
) {
    if camera_state.shake_intensity > 0.1 {
        let t = time.elapsed_secs();
        // Pseudo-random shake using sin waves at different frequencies
        camera_state.shake_offset = Vec2::new(
            (t * 37.0).sin() * camera_state.shake_intensity,
            (t * 53.0).cos() * camera_state.shake_intensity,
        );
        camera_state.shake_intensity -= camera_state.shake_decay * time.delta_secs();
        if camera_state.shake_intensity < 0.1 {
            camera_state.shake_intensity = 0.0;
            camera_state.shake_offset = Vec2::ZERO;
        }
    } else {
        camera_state.shake_offset = Vec2::ZERO;
    }
}

/// Smoothly follows the ship with the camera, applies zoom and shake
fn camera_follow_ship(
    time: Res<Time>,
    camera_state: Res<CameraState>,
    ship_query: Query<&Transform, (With<Ship>, Without<MainCamera>)>,
    mut camera_query: Query<(&mut Transform, &mut Projection), (With<MainCamera>, Without<Ship>)>,
) {
    let Ok(ship_transform) = ship_query.single() else { return };
    let Ok((mut cam_transform, mut projection)) = camera_query.single_mut() else { return };
    let Projection::Orthographic(ref mut projection) = *projection else { return };

    // Target position with shake offset
    let target = Vec3::new(
        ship_transform.translation.x + camera_state.shake_offset.x,
        ship_transform.translation.y + camera_state.shake_offset.y,
        cam_transform.translation.z,
    );

    // Snap camera to ship position (instant follow, no lag)
    cam_transform.translation = target;

    // Smooth zoom
    let target_scale = camera_state.zoom;
    projection.scale = projection.scale + (target_scale - projection.scale) * 5.0 * time.delta_secs();
}

/// Dynamic background based on celestial proximity.
/// Near stars: warm glow. Near black holes: deep red/dark. Open void: dark blue/black.
pub fn update_background_color(
    _depth_state: Res<DepthState>,
    ship_query: Query<&Transform, With<Ship>>,
    star_query: Query<(&Transform, &crate::celestial::components::Star, &crate::celestial::components::CelestialBody)>,
    bh_query: Query<(&Transform, &crate::celestial::components::BlackHole, &crate::celestial::components::CelestialBody)>,
    mut clear_color: ResMut<ClearColor>,
) {
    let ship_pos = ship_query
        .single()
        .map(|t| t.translation.truncate())
        .unwrap_or(Vec2::ZERO);

    // Base color: deep void (dark blue-black)
    let void_color = Vec3::new(0.01, 0.02, 0.06);

    // Star influence: warm glow when close
    let mut star_influence = Vec3::ZERO;
    let mut star_proximity = 0.0_f32;
    for (star_transform, star, body) in star_query.iter() {
        let dist = ship_pos.distance(star_transform.translation.truncate());
        let influence_range = body.radius * 4.0;
        if dist < influence_range {
            let t = 1.0 - (dist / influence_range).clamp(0.0, 1.0);
            let warmth = match star.size_class {
                crate::celestial::components::StarSizeClass::Dwarf => Vec3::new(0.15, 0.08, 0.03),
                crate::celestial::components::StarSizeClass::Main => Vec3::new(0.15, 0.12, 0.06),
                crate::celestial::components::StarSizeClass::Giant => Vec3::new(0.20, 0.10, 0.03),
                crate::celestial::components::StarSizeClass::Supergiant => Vec3::new(0.10, 0.12, 0.20),
            };
            star_influence += warmth * t * t;
            star_proximity = star_proximity.max(t);
        }
    }

    // Black hole influence: dark red pull
    let mut bh_influence = Vec3::ZERO;
    for (bh_transform, bh, _body) in bh_query.iter() {
        let dist = ship_pos.distance(bh_transform.translation.truncate());
        let influence_range = bh.accretion_disk_radius * 3.0;
        if dist < influence_range {
            let t = 1.0 - (dist / influence_range).clamp(0.0, 1.0);
            bh_influence += Vec3::new(0.08 * t, 0.0, 0.02 * t); // Dark red
            // Also darken overall as black hole sucks light
            bh_influence -= Vec3::splat(0.03 * t);
        }
    }

    let c = void_color + star_influence + bh_influence;
    let c = c.clamp(Vec3::ZERO, Vec3::ONE);
    clear_color.0 = Color::srgb(c.x, c.y, c.z);
}

/// Applies a vignette-like darkening effect to sprites based on depth
/// Makes distant sprites darker at greater depths (simulates light falloff)
/// Also hides entities beyond cull range for performance (GTA-style)
fn update_depth_vignette(
    depth_state: Res<DepthState>,
    ship_query: Query<&Transform, With<Ship>>,
    mut sprite_query: Query<
        (&mut Sprite, &GlobalTransform, &mut Visibility, Option<&ChildOf>),
        (
            Without<Ship>,
            Without<MainCamera>,
            Without<crate::vfx::starfield::StarfieldStar>,
            // Shield bubbles manage their own alpha entirely (0 when down,
            // proportional to charge when up) in shields::update_shields.
            // This system's "restore to at least 0.3 near the player" floor
            // was stomping that back to visible every frame for AI ships
            // (whose bubbles aren't covered by the player-child exclusion
            // below) — the bubble never actually looked like it dropped.
            Without<crate::combat::shields::ShieldBubble>,
        ),
    >,
    ship_entity: Query<Entity, With<Ship>>,
) {
    let depth = depth_state.current_depth;
    let Ok(ship_transform) = ship_query.single() else { return };
    let ship_pos = ship_transform.translation.truncate();
    let ship_entity = ship_entity.single().ok();

    // Constant visibility: space has no water column swallowing your
    // floodlights. (The old code shrank "light range" to 120 units past
    // depth 1000 and CULLED everything beyond 3x that — stars, stations,
    // and enemy ships all vanished a screen-width out.) The starfield is
    // excluded from this system entirely — the sky is not a world object.
    //
    // Was 2500 — weapon ranges now reach up to 9,600 (see
    // combat::PROJECTILE_SPEED) and AI ships hold standoff distances up to
    // 8,000-9,600 of their own, so a target at typical long-range engagement
    // distance was already faded to the 15% floor before a shot even landed.
    // 6000 keeps a target bright/near-bright through most real engagement
    // ranges instead of undermining the range fix with "can hit it, can't
    // see it."
    let light_range = 6000.0;

    // Cull distance — entities beyond this are hidden purely for performance.
    // Was light_range * 4 (10,000 total) — now sized explicitly so it clears
    // the longest weapon range (9,600) with real margin for ships
    // patrolling/closing in before they're actually in range.
    let cull_range = 16_000.0;

    for (mut sprite, global_transform, mut visibility, parent) in sprite_query.iter_mut() {
        // Never cull ship's own children (hull, modules, crew)
        if let Some(ship_e) = ship_entity {
            if let Some(p) = parent {
                if p.parent() == ship_e {
                    *visibility = Visibility::Inherited;
                    continue;
                }
            }
        }

        let dist = global_transform.translation().truncate().distance(ship_pos);

        // GTA-style: completely hide entities far from the player
        if dist > cull_range {
            *visibility = Visibility::Hidden;
            continue;
        }

        // Make sure nearby entities are visible
        *visibility = Visibility::Inherited;

        // Skip alpha processing at shallow depths for performance
        if depth < 100.0 {
            continue;
        }

        // Use alpha-based fading to avoid permanently destroying sprite colors
        let base_alpha = sprite.color.alpha().min(1.0);
        if dist > light_range * 2.0 {
            sprite.color.set_alpha((base_alpha * 0.15).max(0.05));
        } else if dist > light_range {
            let fade = 1.0 - ((dist - light_range) / light_range).clamp(0.0, 1.0);
            sprite.color.set_alpha((base_alpha.min(0.9) * fade).max(0.05));
        } else {
            // Within light range: restore alpha to original (max 1.0)
            sprite.color.set_alpha(base_alpha.min(1.0).max(0.3));
        }
    }
}
