use bevy::prelude::*;
use rand::Rng;
use crate::camera::MainCamera;

// ============================================================================
// PARALLAX STARFIELD
// Three layers of stars wrap around the camera in a large box. Each layer
// follows the camera by a different fraction (parallax factor) — distant
// stars track the camera closely (appear far away, drift slowly), near
// stars lag behind (rush past) — giving a strong sense of motion in what
// is otherwise a featureless void.
// ============================================================================

/// One star in the wrapping starfield. `parallax` is how much of the camera's
/// movement the star inherits: 0.9 = distant (barely moves past), 0.5 = close.
#[derive(Component)]
pub struct StarfieldStar {
    pub parallax: f32,
}

/// Wrap box around the camera — must exceed the max visible area at the
/// widest zoom so stars never visibly pop in or out at the edges.
const FIELD_W: f32 = 6000.0;
const FIELD_H: f32 = 4000.0;

/// Spawn the starfield once, centered on the camera (guarded by query).
pub fn spawn_starfield(
    mut commands: Commands,
    existing: Query<(), With<StarfieldStar>>,
    camera_query: Query<&Transform, With<MainCamera>>,
) {
    if !existing.is_empty() {
        return;
    }
    let Ok(cam) = camera_query.single() else { return };
    let center = cam.translation.truncate();
    let mut rng = rand::thread_rng();

    // (count, parallax, size range, alpha range, tint)
    let layers: [(usize, f32, (f32, f32), (f32, f32), Color); 3] = [
        (140, 0.92, (1.0, 2.2), (0.25, 0.45), Color::srgb(0.85, 0.88, 1.0)),
        (90,  0.78, (1.8, 3.2), (0.40, 0.65), Color::srgb(0.95, 0.95, 1.0)),
        (50,  0.58, (2.6, 4.5), (0.60, 0.95), Color::srgb(1.0, 1.0, 1.0)),
    ];

    for (count, parallax, size, alpha, tint) in layers {
        for _ in 0..count {
            let pos = center + Vec2::new(
                rng.gen_range(-FIELD_W / 2.0..FIELD_W / 2.0),
                rng.gen_range(-FIELD_H / 2.0..FIELD_H / 2.0),
            );
            let s = rng.gen_range(size.0..size.1);
            let a = rng.gen_range(alpha.0..alpha.1);
            let c = tint.to_srgba();
            commands.spawn((
                Sprite {
                    color: Color::srgba(c.red, c.green, c.blue, a),
                    custom_size: Some(Vec2::splat(s)),
                    ..default()
                },
                Transform::from_xyz(pos.x, pos.y, -500.0),
                StarfieldStar { parallax },
            ));
        }
    }
}

/// Apply per-layer parallax and wrap stars that drift out of the field box.
pub fn update_starfield(
    camera_query: Query<&Transform, (With<MainCamera>, Without<StarfieldStar>)>,
    mut star_query: Query<(&StarfieldStar, &mut Transform), Without<MainCamera>>,
    mut last_cam: Local<Option<Vec2>>,
) {
    let Ok(cam) = camera_query.single() else { return };
    let cam_pos = cam.translation.truncate();

    let delta = match *last_cam {
        Some(prev) => cam_pos - prev,
        None => Vec2::ZERO,
    };
    *last_cam = Some(cam_pos);

    if delta == Vec2::ZERO {
        return;
    }

    for (star, mut transform) in star_query.iter_mut() {
        transform.translation.x += delta.x * star.parallax;
        transform.translation.y += delta.y * star.parallax;

        // Wrap into the box around the camera
        let dx = transform.translation.x - cam_pos.x;
        let dy = transform.translation.y - cam_pos.y;
        if dx > FIELD_W / 2.0 {
            transform.translation.x -= FIELD_W;
        } else if dx < -FIELD_W / 2.0 {
            transform.translation.x += FIELD_W;
        }
        if dy > FIELD_H / 2.0 {
            transform.translation.y -= FIELD_H;
        } else if dy < -FIELD_H / 2.0 {
            transform.translation.y += FIELD_H;
        }
    }
}
