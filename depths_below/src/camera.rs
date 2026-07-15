use bevy::prelude::*;
use bevy::input::mouse::MouseWheel;
use bevy::sprite::Anchor;
use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use crate::components::{Ship, ShipPhysics, ShipLight, Module};
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
    /// Camera offset from the ship while free-look (hold T) is active.
    /// Eases toward a target each frame rather than snapping, and eases
    /// back to zero on release — see `free_look_input`.
    pub free_look_offset: Vec2,
    /// True while the free-look key is held. Read by `ship::movement` to
    /// freeze the mouse-aim turret so looking around doesn't spin the ship
    /// to face wherever the cursor happens to be.
    pub free_look_active: bool,
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
            free_look_offset: Vec2::ZERO,
            free_look_active: false,
        }
    }
}

/// Camera can pan up to this far from the ship while free-looking.
const FREE_LOOK_MAX_PAN: f32 = 4500.0;
/// Lerp rate (per second) for easing the pan offset toward/away from target.
const FREE_LOOK_EASE: f32 = 6.0;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<CameraState>()
            .add_systems(Startup, (spawn_camera, spawn_light_cone_visual))
            .add_systems(
                Update,
                (
                    camera_zoom_input,
                    camera_shake_on_damage,
                    camera_shake_update,
                    free_look_input,
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

/// Marks the single sprite that renders the flashlight beam itself. Without
/// this, the whole vision-cone system only worked by dimming *other* sprites
/// that happened to be inside/outside an invisible cone — over empty space
/// (nothing to dim) there was nothing to see at all, so the "cone" never
/// actually read as a beam of light, just as things popping in and out.
#[derive(Component)]
struct LightConeVisual;

/// Texture width used for the cone image — the apex sits at local x=0 (left
/// edge) and radius 1.0 (i.e. the eventual `cone_range` after scaling) sits
/// at local x=width. Height is derived from CONE_HALF_ANGLE so the full
/// wedge fits without the square-canvas edges clipping its corners.
const CONE_TEX_WIDTH: u32 = 256;

/// Soft, faint haze rather than a literal flashlight beam: bright near the
/// apex (ship position), fading gradually across the whole wedge instead of
/// staying flat-bright until a hard edge — was peak alpha 0.45 with a warm
/// torch-yellow tint and only the last 30% of the angle softened, which read
/// as a distinct "flashlight cone" graphic. Much lower peak, paler/near-
/// neutral color, and a softening zone starting near the center instead.
fn cone_image() -> Image {
    let width = CONE_TEX_WIDTH;
    let height = (2.0 * width as f32 * CONE_HALF_ANGLE.tan()).ceil() as u32;
    let mut data = vec![0u8; (width * height * 4) as usize];
    let apex_y = height as f32 / 2.0;

    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 + 0.5;
            let dy = (y as f32 + 0.5) - apex_y;
            let radius_norm = ((dx * dx + dy * dy).sqrt() / width as f32).clamp(0.0, 1.0);
            let angle = dy.atan2(dx).abs();

            let alpha = if angle > CONE_HALF_ANGLE {
                0.0
            } else {
                let angular_soft = 1.0 - ((angle - CONE_HALF_ANGLE * 0.35) / (CONE_HALF_ANGLE * 0.65)).clamp(0.0, 1.0);
                let radial_soft = (1.0 - radius_norm).powf(1.7);
                (angular_soft * radial_soft * 0.13).clamp(0.0, 1.0)
            };

            let idx = ((y * width + x) * 4) as usize;
            data[idx] = 235;
            data[idx + 1] = 236;
            data[idx + 2] = 232;
            data[idx + 3] = (alpha * 255.0).round() as u8;
        }
    }

    Image::new(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Spawns the beam sprite at a fixed 1-world-unit reference size (custom_size
/// bakes in the texture's aspect ratio) — update_depth_vignette rescales it
/// to the real cone_range every frame via Transform.scale, since cone_range
/// itself varies with equipped ShipLight modules.
fn spawn_light_cone_visual(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let width = CONE_TEX_WIDTH;
    let height = (2.0 * width as f32 * CONE_HALF_ANGLE.tan()).ceil() as u32;
    let aspect = height as f32 / width as f32;
    let handle = images.add(cone_image());

    commands.spawn((
        Sprite {
            image: handle,
            custom_size: Some(Vec2::new(1.0, aspect)),
            ..default()
        },
        Anchor::CENTER_LEFT,
        Transform::from_xyz(0.0, 0.0, -100.0),
        Visibility::Hidden,
        LightConeVisual,
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

/// Hold T to look around without turning the ship: the camera pans toward
/// the cursor, up to FREE_LOOK_MAX_PAN away, and eases back to centered on
/// release. Ship mouse-aim is frozen for the duration (see ship::movement)
/// so the ship doesn't spin to face wherever you're just looking.
fn free_look_input(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    game_state: Res<State<GameState>>,
    mut camera_state: ResMut<CameraState>,
    ship_query: Query<&Transform, (With<Ship>, Without<MainCamera>)>,
    windows_query: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
) {
    // Exploring-only: while docked, T cycles build templates (see
    // building/templates.rs) — forcing the target to zero here (rather than
    // skipping the system outright) still lets any in-progress pan ease
    // back to centered instead of getting stuck if the player docks mid-hold.
    let held = *game_state.get() == GameState::Exploring && keyboard.pressed(KeyCode::KeyT);
    camera_state.free_look_active = held;

    let mut target_offset = Vec2::ZERO;
    if held {
        if let (Ok(ship_transform), Ok(window), Ok((camera, cam_gt))) =
            (ship_query.single(), windows_query.single(), camera_query.single())
        {
            if let Some(cursor) = window.cursor_position() {
                if let Ok(cursor_world) = camera.viewport_to_world_2d(cam_gt, cursor) {
                    let to_cursor = cursor_world - ship_transform.translation.truncate();
                    target_offset = to_cursor.clamp_length_max(FREE_LOOK_MAX_PAN);
                }
            }
        }
    }

    let blend = (FREE_LOOK_EASE * time.delta_secs()).min(1.0);
    camera_state.free_look_offset = camera_state.free_look_offset.lerp(target_offset, blend);
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

    // Target position with shake offset and free-look pan
    let target = Vec3::new(
        ship_transform.translation.x + camera_state.shake_offset.x + camera_state.free_look_offset.x,
        ship_transform.translation.y + camera_state.shake_offset.y + camera_state.free_look_offset.y,
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

/// Angular half-width of the forward light cone (40° = 80° total spread).
const CONE_HALF_ANGLE: f32 = 40.0 * std::f32::consts::PI / 180.0;

/// Applies a flashlight-style darkening effect: a small bubble of visibility
/// close around the ship (so you're not blind to your sides/rear at close
/// range) plus a much bigger cone pointed exactly at the cursor — computed
/// fresh from cursor position each frame rather than the ship's physical
/// facing (ShipPhysics.rotation), which turns at a limited rate and lagged
/// behind fast mouse movement. Falls back to the ship's last real facing
/// while free-looking (T) or if the cursor isn't over the window, since the
/// cursor means something else during free-look (camera pan, not aim/light).
/// Genuinely dark outside the cone/bubble instead of just dimmer.
/// Floodlight/Searchlight/DeepFloodlight modules (ShipLight, leftover from
/// the original submarine version) extend both ranges — previously they
/// were only read by a since-dead radar.rs system that had nothing left to
/// match (creatures disabled, and the POI/decoration marker components it
/// filtered on are never actually attached to anything in the space
/// version), so building one of these modules did precisely nothing.
/// Also hides entities beyond cull range for performance (GTA-style) — that
/// part stays pure-distance, not cone-gated, so off-screen combat/AI
/// doesn't pop in and out as you turn.
/// Environment (asteroids, real celestial bodies, POIs, wrecks) floor alpha
/// when outside the light — low enough to read as genuinely black, not just
/// dim, per "make everything black" — a sliver above zero purely so shapes
/// aren't a literal void pop-in/out right at the light's edge.
const ENV_FLOOR: f32 = 0.02;

/// Decorative background planets (starfield::BackgroundPlanet) never go
/// fully black — they're distant scenery, not something to hunt for with
/// the flashlight — just "way darker, barely see the color" when not being
/// looked at directly.
const BG_PLANET_FLOOR: f32 = 0.16;

fn update_depth_vignette(
    depth_state: Res<DepthState>,
    camera_state: Res<CameraState>,
    ship_query: Query<(&Transform, &ShipPhysics), With<Ship>>,
    light_query: Query<(&ShipLight, &Module)>,
    windows_query: Query<&Window>,
    cam_query: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    mut sprite_query: Query<
        (&mut Sprite, &GlobalTransform, &mut Visibility, Option<&ChildOf>),
        (
            Without<Ship>,
            Without<MainCamera>,
            Without<crate::vfx::starfield::StarfieldStar>,
            // Explicit even though every BackgroundPlanet also carries
            // StarfieldStar (and so is already excluded above) — Bevy's
            // static query-conflict check can't infer that implication from
            // Without<StarfieldStar> alone, so without this it flags this
            // query and bg_planet_query below as aliasing on `&mut Sprite`.
            Without<crate::vfx::starfield::BackgroundPlanet>,
            // Shield bubbles manage their own alpha entirely (0 when down,
            // proportional to charge when up) in shields::update_shields.
            // This system's "restore to at least 0.3 near the player" floor
            // was stomping that back to visible every frame for AI ships
            // (whose bubbles aren't covered by the player-child exclusion
            // below) — the bubble never actually looked like it dropped.
            Without<crate::combat::shields::ShieldBubble>,
            // Star/planet glow layers run their own per-frame pulse/shimmer
            // alpha animation (celestial_visuals::animate_star_glow etc.) —
            // touching their alpha here too would fight that animation and
            // flicker between the pulse value and this system's target each
            // frame. They read as ambient self-light, not something the
            // flashlight needs to reveal.
            Without<crate::vfx::celestial_visuals::StarGlow>,
            Without<crate::vfx::celestial_visuals::StarFlareGlow>,
            Without<crate::vfx::celestial_visuals::PlanetAtmosphere>,
            Without<LightConeVisual>,
        ),
    >,
    mut bg_planet_query: Query<
        (&mut Sprite, &GlobalTransform),
        (With<crate::vfx::starfield::BackgroundPlanet>, Without<Ship>, Without<MainCamera>),
    >,
    mut cone_visual_query: Query<(&mut Transform, &mut Visibility), (With<LightConeVisual>, Without<Ship>)>,
    ship_entity: Query<Entity, With<Ship>>,
) {
    let depth = depth_state.current_depth;
    let Ok((ship_transform, ship_physics)) = ship_query.single() else { return };
    let ship_pos = ship_transform.translation.truncate();
    let ship_entity = ship_entity.single().ok();

    let mut facing = ship_physics.rotation;
    if !camera_state.free_look_active {
        if let (Ok(window), Ok((camera, cam_gt))) = (windows_query.single(), cam_query.single()) {
            if let Some(cursor) = window.cursor_position() {
                if let Ok(cursor_world) = camera.viewport_to_world_2d(cam_gt, cursor) {
                    let to_cursor = cursor_world - ship_pos;
                    if to_cursor.length_squared() > 4.0 {
                        facing = to_cursor.y.atan2(to_cursor.x);
                    }
                }
            }
        }
    }

    let light_bonus: f32 = light_query.iter()
        .filter(|(_, module)| module.is_active)
        .map(|(light, _)| light.range * light.intensity)
        .sum();

    // Omnidirectional bubble: always visible regardless of facing, so
    // something right next to you isn't invisible just because you're
    // looking the wrong way. Cone range: the real "flashlight" reach,
    // only within CONE_HALF_ANGLE of where the ship is facing/aiming.
    let omni_radius = 900.0 + light_bonus * 0.1;
    let cone_range = (3500.0 + light_bonus * 0.9).max(omni_radius);

    // Cull distance — entities beyond this are hidden purely for performance,
    // independent of the light cone (see doc comment above).
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

        let world_pos = global_transform.translation().truncate();
        let to_target = world_pos - ship_pos;
        let dist = to_target.length();

        // GTA-style: completely hide entities far from the player
        if dist > cull_range {
            *visibility = Visibility::Hidden;
            continue;
        }

        // Make sure nearby entities are visible
        *visibility = Visibility::Inherited;

        // Skip alpha processing right at the home station — was depth<100,
        // which at a 54m test-undock still fully suppressed the system and
        // made it look broken; 20 is enough to avoid a jarring blackout the
        // instant you undock without hiding the effect from normal play.
        if depth < 20.0 {
            continue;
        }

        // Smoothly blend the reveal radius between the always-on bubble and
        // the full cone reach based on angle, instead of hard-switching the
        // instant the cursor crosses the cone edge (that switch used to
        // cause asteroids/stations/ships to pop straight from near-black to
        // full brightness). Same soft angular falloff the background
        // planets already use below — applied here uniformly to everything:
        // asteroids, stations, their individual hull/block sprites, and
        // enemy ships (previously ships got a separate hard Visibility::
        // Hidden pop instead of a fade — removed so they behave the same
        // as everything else now).
        let angular_factor = if dist <= 1.0 {
            1.0
        } else {
            let angle_to_target = to_target.y.atan2(to_target.x);
            let mut diff = angle_to_target - facing;
            while diff > std::f32::consts::PI { diff -= std::f32::consts::TAU; }
            while diff < -std::f32::consts::PI { diff += std::f32::consts::TAU; }
            let angle_abs = diff.abs();
            if angle_abs < CONE_HALF_ANGLE {
                1.0
            } else if angle_abs < CONE_HALF_ANGLE * 1.8 {
                1.0 - ((angle_abs - CONE_HALF_ANGLE) / (CONE_HALF_ANGLE * 0.8)).clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        let light_range = omni_radius + (cone_range - omni_radius) * angular_factor;

        if dist <= light_range {
            sprite.color.set_alpha(1.0);
        } else if dist > light_range * 1.5 {
            sprite.color.set_alpha(ENV_FLOOR);
        } else {
            let fade = 1.0 - ((dist - light_range) / (light_range * 0.5)).clamp(0.0, 1.0);
            sprite.color.set_alpha(fade.max(ENV_FLOOR));
        }
    }

    // Background decorative planets: not part of the main loop above (they
    // parallax-follow the camera so "distance" doesn't mean much — they're
    // always roughly the same distance away regardless of where the ship
    // actually flies) — revealed by sweeping the light cone across them
    // instead, with a soft angular falloff at the cone's edge.
    for (mut sprite, global_transform) in bg_planet_query.iter_mut() {
        if depth < 20.0 {
            sprite.color.set_alpha(1.0);
            continue;
        }
        let world_pos = global_transform.translation().truncate();
        let to_target = world_pos - ship_pos;
        let angle_to_target = to_target.y.atan2(to_target.x);
        let mut diff = angle_to_target - facing;
        while diff > std::f32::consts::PI { diff -= std::f32::consts::TAU; }
        while diff < -std::f32::consts::PI { diff += std::f32::consts::TAU; }
        let angle_abs = diff.abs();

        let alpha = if angle_abs < CONE_HALF_ANGLE {
            1.0
        } else if angle_abs < CONE_HALF_ANGLE * 1.8 {
            let t = 1.0 - ((angle_abs - CONE_HALF_ANGLE) / (CONE_HALF_ANGLE * 0.8)).clamp(0.0, 1.0);
            BG_PLANET_FLOOR + (1.0 - BG_PLANET_FLOOR) * t
        } else {
            BG_PLANET_FLOOR
        };
        sprite.color.set_alpha(alpha);
    }

    // The actual visible beam — a wedge sprite anchored at the ship,
    // rotated to `facing` and stretched to `cone_range` every frame (the
    // range itself breathes with equipped ShipLight modules). Everything
    // above this only ever changed the alpha of *other* sprites; without
    // this, empty space stayed empty-looking even when "in the light."
    if let Ok((mut cone_transform, mut cone_visibility)) = cone_visual_query.single_mut() {
        if depth < 20.0 {
            *cone_visibility = Visibility::Hidden;
        } else {
            *cone_visibility = Visibility::Inherited;
            cone_transform.translation = ship_pos.extend(-100.0);
            cone_transform.rotation = Quat::from_rotation_z(facing);
            cone_transform.scale = Vec3::new(cone_range, cone_range, 1.0);
        }
    }
}
