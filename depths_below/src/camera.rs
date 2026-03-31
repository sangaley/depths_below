use bevy::prelude::*;
use bevy::input::mouse::MouseWheel;
use crate::components::Submarine;
use crate::resources::DepthState;
use crate::events::SubmarineDamaged;
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
            max_zoom: 4.0,
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
                    camera_follow_submarine,
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
        Camera2dBundle::default(),
        MainCamera,
    ));
}

/// Scroll wheel + keyboard (+/-) zoom
fn camera_zoom_input(
    mut scroll_events: EventReader<MouseWheel>,
    keyboard: Res<Input<KeyCode>>,
    time: Res<Time>,
    mut camera_state: ResMut<CameraState>,
) {
    for event in scroll_events.iter() {
        let zoom_delta = -event.y * 0.1;
        camera_state.zoom = (camera_state.zoom + zoom_delta)
            .clamp(camera_state.min_zoom, camera_state.max_zoom);
    }

    // Keyboard zoom: +/= zooms in, -/_ zooms out (plus numpad variants)
    let kb_speed = 1.5 * time.delta_seconds();
    if keyboard.pressed(KeyCode::Equals) || keyboard.pressed(KeyCode::NumpadAdd) {
        camera_state.zoom = (camera_state.zoom - kb_speed)
            .clamp(camera_state.min_zoom, camera_state.max_zoom);
    }
    if keyboard.pressed(KeyCode::Minus) || keyboard.pressed(KeyCode::NumpadSubtract) {
        camera_state.zoom = (camera_state.zoom + kb_speed)
            .clamp(camera_state.min_zoom, camera_state.max_zoom);
    }
}

/// Trigger shake on submarine damage
fn camera_shake_on_damage(
    mut damage_events: EventReader<SubmarineDamaged>,
    mut camera_state: ResMut<CameraState>,
) {
    for event in damage_events.iter() {
        // Shake proportional to damage (capped)
        let intensity = (event.amount * 0.3).min(15.0);
        // Stack shakes but cap total
        camera_state.shake_intensity = (camera_state.shake_intensity + intensity).min(20.0);
    }
}

/// Decay shake over time and compute offset
pub fn camera_shake_update(
    time: Res<Time>,
    mut camera_state: ResMut<CameraState>,
) {
    if camera_state.shake_intensity > 0.1 {
        let t = time.elapsed_seconds();
        // Pseudo-random shake using sin waves at different frequencies
        camera_state.shake_offset = Vec2::new(
            (t * 37.0).sin() * camera_state.shake_intensity,
            (t * 53.0).cos() * camera_state.shake_intensity,
        );
        camera_state.shake_intensity -= camera_state.shake_decay * time.delta_seconds();
        if camera_state.shake_intensity < 0.1 {
            camera_state.shake_intensity = 0.0;
            camera_state.shake_offset = Vec2::ZERO;
        }
    } else {
        camera_state.shake_offset = Vec2::ZERO;
    }
}

/// Smoothly follows the submarine with the camera, applies zoom and shake
fn camera_follow_submarine(
    time: Res<Time>,
    camera_state: Res<CameraState>,
    sub_query: Query<&Transform, (With<Submarine>, Without<MainCamera>)>,
    mut camera_query: Query<(&mut Transform, &mut OrthographicProjection), (With<MainCamera>, Without<Submarine>)>,
) {
    let Ok(sub_transform) = sub_query.get_single() else { return };
    let Ok((mut cam_transform, mut projection)) = camera_query.get_single_mut() else { return };

    // Target position with shake offset
    let target = Vec3::new(
        sub_transform.translation.x + camera_state.shake_offset.x,
        sub_transform.translation.y + camera_state.shake_offset.y,
        cam_transform.translation.z,
    );

    // Snap camera to submarine position (instant follow, no lag)
    cam_transform.translation = target;

    // Smooth zoom
    let target_scale = camera_state.zoom;
    projection.scale = projection.scale + (target_scale - projection.scale) * 5.0 * time.delta_seconds();
}

/// Dynamic background based on celestial proximity.
/// Near stars: warm glow. Near black holes: deep red/dark. Open void: dark blue/black.
pub fn update_background_color(
    depth_state: Res<DepthState>,
    sub_query: Query<&Transform, With<Submarine>>,
    star_query: Query<(&Transform, &crate::celestial::components::Star, &crate::celestial::components::CelestialBody)>,
    bh_query: Query<(&Transform, &crate::celestial::components::BlackHole, &crate::celestial::components::CelestialBody)>,
    mut clear_color: ResMut<ClearColor>,
) {
    let sub_pos = sub_query
        .get_single()
        .map(|t| t.translation.truncate())
        .unwrap_or(Vec2::ZERO);

    // Base color: deep void (dark blue-black)
    let void_color = Vec3::new(0.01, 0.02, 0.06);

    // Star influence: warm glow when close
    let mut star_influence = Vec3::ZERO;
    let mut star_proximity = 0.0_f32;
    for (star_transform, star, body) in star_query.iter() {
        let dist = sub_pos.distance(star_transform.translation.truncate());
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
        let dist = sub_pos.distance(bh_transform.translation.truncate());
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
    clear_color.0 = Color::rgb(c.x, c.y, c.z);
}

/// Applies a vignette-like darkening effect to sprites based on depth
/// Makes distant sprites darker at greater depths (simulates light falloff)
/// Also hides entities beyond cull range for performance (GTA-style)
fn update_depth_vignette(
    depth_state: Res<DepthState>,
    sub_query: Query<&Transform, With<Submarine>>,
    mut sprite_query: Query<(&mut Sprite, &GlobalTransform, &mut Visibility, Option<&Parent>), (Without<Submarine>, Without<MainCamera>)>,
    submarine_entity: Query<Entity, With<Submarine>>,
) {
    let depth = depth_state.current_depth;
    let Ok(sub_transform) = sub_query.get_single() else { return };
    let sub_pos = sub_transform.translation.truncate();
    let sub_entity = submarine_entity.get_single().ok();

    // Light range decreases with depth
    let light_range = if depth < 200.0 {
        800.0
    } else if depth < 500.0 {
        500.0
    } else if depth < 1000.0 {
        250.0
    } else {
        120.0
    };

    // Cull distance - entities beyond this are fully hidden (not rendered at all)
    let cull_range = light_range * 3.0;

    for (mut sprite, global_transform, mut visibility, parent) in sprite_query.iter_mut() {
        // Never cull submarine's own children (hull, modules, crew)
        if let Some(sub_e) = sub_entity {
            if let Some(p) = parent {
                if p.get() == sub_e {
                    *visibility = Visibility::Inherited;
                    continue;
                }
            }
        }

        let dist = global_transform.translation().truncate().distance(sub_pos);

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
        let base_alpha = sprite.color.a().min(1.0);
        if dist > light_range * 2.0 {
            sprite.color.set_a((base_alpha * 0.15).max(0.05));
        } else if dist > light_range {
            let fade = 1.0 - ((dist - light_range) / light_range).clamp(0.0, 1.0);
            sprite.color.set_a((base_alpha.min(0.9) * fade).max(0.05));
        } else {
            // Within light range: restore alpha to original (max 1.0)
            sprite.color.set_a(base_alpha.min(1.0).max(0.3));
        }
    }
}
