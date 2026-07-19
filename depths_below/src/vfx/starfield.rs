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

/// Marks the sparse decorative background planets (as opposed to star
/// sparkles, which also carry `StarfieldStar` for the shared parallax/wrap
/// system) — read by camera::update_depth_vignette to give them a gentler
/// darkness treatment than the rest of the world.
#[derive(Component)]
pub struct BackgroundPlanet;

/// Wrap box around the camera — must exceed the max visible area at the
/// widest zoom so stars never visibly pop in or out at the edges. Was
/// 6000x4000, sized for the old max_zoom of 8.0. Tried bumping this all the
/// way to 30,000x17,000 to match the current max_zoom of 20.0, but that grew
/// the box's *area* ~21x while only 4x'ing star count below — density at
/// normal zoom (the common case, not the rarely-used max tactical zoom-out)
/// dropped from ~35 visible stars to ~6, i.e. it made the typical view look
/// *emptier*, not fuller. 16,000x9,000 comfortably covers everything up to
/// zoom ~10 (most real play) at healthy density; only the rarely-used
/// zoom 10-20 tactical range stays sparser rather than fully dense.
const FIELD_W: f32 = 16_000.0;
const FIELD_H: f32 = 9_000.0;

/// Real 4-point sparkle sprites (Kenney "Space Shooter Redux", CC0 — see
/// assets/sprites/celestial/CREDITS.txt) instead of plain colored square
/// dots — mostly matters for the nearest/biggest layer where a shape is
/// actually visible at a few pixels across.
const SPARKLE_SPRITES: [&str; 3] = [
    "sprites/celestial/starfield/star1.png",
    "sprites/celestial/starfield/star2.png",
    "sprites/celestial/starfield/star3.png",
];

/// Same planet art used for real in-system planets (see
/// celestial::spawning::planet_sprite_path) — reused here as small, distant
/// background scenery. A handful scattered far apart, well behind the
/// stars, moving almost not at all (parallax 0.97) so they read as fixed
/// backdrop rather than something to fly to.
const BACKGROUND_PLANET_SPRITES: [&str; 6] = [
    "sprites/celestial/planets/planet00.png",
    "sprites/celestial/planets/planet02.png",
    "sprites/celestial/planets/planet04.png",
    "sprites/celestial/planets/planet06.png",
    "sprites/celestial/planets/planet08.png",
    "sprites/celestial/planets/planet09.png",
];

/// Spawn the starfield once, centered on the camera (guarded by query).
pub fn spawn_starfield(
    mut commands: Commands,
    existing: Query<(), With<StarfieldStar>>,
    camera_query: Query<&Transform, With<MainCamera>>,
    asset_server: Res<AssetServer>,
) {
    if !existing.is_empty() {
        return;
    }
    let Ok(cam) = camera_query.single() else { return };
    let center = cam.translation.truncate();
    let mut rng = rand::thread_rng();

    let sparkle_handles: Vec<Handle<Image>> = SPARKLE_SPRITES.iter()
        .map(|path| asset_server.load(*path))
        .collect();
    let bg_planet_handles: Vec<Handle<Image>> = BACKGROUND_PLANET_SPRITES.iter()
        .map(|path| asset_server.load(*path))
        .collect();

    // (count, parallax, size range, alpha range, tint)
    // Counts scaled 6x to match the OLD density (280 stars over 6000x4000 =
    // 24,000,000 sq units) applied to the new 16,000x9,000 = 144,000,000 sq
    // unit field (exactly 6x the area) — matching density instead of
    // undershooting it like the previous attempt did. 4th layer is a sparse
    // handful of much bigger, brighter stars so the field isn't uniformly
    // tiny — real size variety instead of everything topping out at 4.5px.
    let layers: [(usize, f32, (f32, f32), (f32, f32), Color); 4] = [
        (840, 0.92, (1.0, 2.2), (0.25, 0.45), Color::srgb(0.85, 0.88, 1.0)),
        (540, 0.78, (1.8, 3.2), (0.40, 0.65), Color::srgb(0.95, 0.95, 1.0)),
        (300, 0.58, (2.6, 4.5), (0.60, 0.95), Color::srgb(1.0, 1.0, 1.0)),
        (35,  0.40, (6.0, 11.0), (0.75, 1.0), Color::srgb(1.0, 0.97, 0.9)),
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
            let image = sparkle_handles[rng.gen_range(0..sparkle_handles.len())].clone();
            commands.spawn((
                Sprite {
                    image,
                    color: Color::srgba(c.red, c.green, c.blue, a),
                    custom_size: Some(Vec2::splat(s)),
                    ..default()
                },
                Transform::from_xyz(pos.x, pos.y, -500.0),
                StarfieldStar { parallax },
            ));
        }
    }

    // Distant background planets — sparse, small, mostly-opaque, barely
    // moving. Same art as real planets, just decorative scenery here rather
    // than an orbiting CelestialBody.
    let bg_planet_count = 5;
    for _ in 0..bg_planet_count {
        let pos = center + Vec2::new(
            rng.gen_range(-FIELD_W / 2.0..FIELD_W / 2.0),
            rng.gen_range(-FIELD_H / 2.0..FIELD_H / 2.0),
        );
        let s = rng.gen_range(150.0..380.0);
        let image = bg_planet_handles[rng.gen_range(0..bg_planet_handles.len())].clone();
        commands.spawn((
            Sprite {
                image,
                custom_size: Some(Vec2::splat(s)),
                ..default()
            },
            Transform::from_xyz(pos.x, pos.y, -600.0),
            StarfieldStar { parallax: 0.97 },
            BackgroundPlanet,
        ));
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

        // Wrap into the box around the camera. Was a single +/-FIELD_W
        // correction — correct for normal per-frame drift, but the camera
        // can also jump tens or hundreds of thousands of units in a single
        // frame (warp dash, system-to-system warp), and one correction can't
        // catch up from a jump that's many multiples of the field size. Every
        // star was left stranded outside the field box after any real warp,
        // making deep space look permanently empty — a modulo wrap is
        // correct regardless of how large the jump was.
        let dx = transform.translation.x - cam_pos.x;
        let dy = transform.translation.y - cam_pos.y;
        transform.translation.x = cam_pos.x + (dx + FIELD_W / 2.0).rem_euclid(FIELD_W) - FIELD_W / 2.0;
        transform.translation.y = cam_pos.y + (dy + FIELD_H / 2.0).rem_euclid(FIELD_H) - FIELD_H / 2.0;
    }
}
