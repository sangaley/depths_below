use bevy::prelude::*;
use bevy::render::view::window::screenshot::{save_to_disk, Screenshot};
use crate::components::{Ship, ShipPhysics};
use crate::resources::{BuildCategory, BuildingState, InputState};
use crate::states::{BuildState, GameState};
use crate::ai_ship::components::{AiShip, WorldSimulation, SimBehavior};
use crate::combat::targeting::selection::{TargetSelection, TargetType};
use crate::combat::targeting::fire_groups::FireGroupState;

// ============================================================================
// DEMO / SELF-PLAYTEST MODE — dev tooling, not a game feature.
// Three independent env-gated modes:
//   DEPTHS_DEMO=1       — full autopilot: skips the menu, flies toward and
//                          fires at the nearest enemy, saves periodic engine
//                          screenshots. Used for unattended log-based testing.
//   DEPTHS_SKIP_MENU=1  — skips the menu/station sequence only. Drops
//                          straight into Exploring with the starter ship,
//                          the normal 37-ship world simulation, and full
//                          manual control (no autopilot).
//   DEPTHS_SHOTS=<n>    — engine-side screenshot every n seconds during
//                          NORMAL play: no autopilot, no menu skip, nothing
//                          else changed. F7 takes one on demand. Captures the
//                          render target, not the display, so it never
//                          photographs whatever app happens to be in front.
//                          It CANNOT see through a window that is fully
//                          covered: macOS stops that window drawing, and the
//                          frames come back black. Use DEPTHS_OFFSCREEN for
//                          anything run in the background.
//                          Dir: DEPTHS_SHOTS_DIR (default /tmp/depths_shots).
//   DEPTHS_OFFSCREEN=1  — render into an image instead of the window, so
//                          capture works however buried the window is. The
//                          window itself shows nothing; for unattended runs.
//   DEPTHS_KEYS=<script> — tap keys at set times, e.g. "3:KeyM,5:F7,8:Escape".
//                          "6:Minus~3" HOLDS a key for 3s instead -- needed
//                          for anything read with `pressed` rather than
//                          `just_pressed`, like camera zoom.
//                          Seconds of REAL time, so autoplay's time dilation
//                          can't shift it. F7 is the capture key, so a script
//                          can walk the screens and photograph each one.
//                          Goes through the same ButtonInput press the
//                          autoplay director and the gamepad bridge use --
//                          never OS keystrokes, which land in whatever app is
//                          focused rather than the game.
//   DEPTHS_CASCADE=0.85 — pin the story's progress level, so a late beat can
//                          be looked at without an hour of flying first.
//   DEPTHS_CASCADE_TRACE=1 — print the arc's state once a second.
//   DEPTHS_BUILD_TAB=<name> — stop at the station, open build mode and select
//                          that build tab, so the palette can be looked at
//                          without clicking through to it. Name matches
//                          BuildCategory::name() case-insensitively
//                          ("structural", "life support", "hull").
//   DEPTHS_BUILD_SLOT=<n> — with the above, park on slot n instead of the
//                          first, to check the far end of a scrolling strip.
//   DEPTHS_BUILD_LAUNCH=<s> — with the above, launch after s seconds with the
//                          build menu still open. Build overlays are drawn in
//                          world space and torn down by systems that only run
//                          at the berth, so leaving that way is exactly how
//                          they get stranded there.
//   DEPTHS_MOVETEST=1   — bare movement sandbox: instant skip (no menu/
//                          station flash), starter ship, manual control,
//                          and NO AI ships spawned — just open space and
//                          stars, for isolating flight feel from everything
//                          else.
// ============================================================================

pub struct DemoPlugin;

/// Seconds to sit in MainMenu / StationDocked before auto-advancing.
/// Movetest uses a near-zero delay so there's no visible flash of either
/// screen; the other modes keep a short delay so their own setup (ship
/// spawn, etc.) has a moment to run.
#[derive(Resource)]
struct DemoAdvanceDelays {
    menu: f32,
    station: f32,
}

/// The build tab `DEPTHS_BUILD_TAB` asks for, if it names a real one.
///
/// Build mode is only reachable while docked and only by keypress, which made
/// the palette the one screen that could not be photographed unattended. Two
/// of the demo's blockers lived there -- a tab whose slots placed blocks other
/// than their labels, and a whole category with no tab at all -- and neither
/// was visible from a log line.
fn requested_build_tab() -> Option<BuildCategory> {
    let want = std::env::var("DEPTHS_BUILD_TAB").ok()?;
    let want = want.trim().to_ascii_lowercase();
    BuildCategory::ALL
        .iter()
        .copied()
        .find(|c| c.name().to_ascii_lowercase() == want)
        .or_else(|| {
            warn!(
                "DEPTHS_BUILD_TAB={want:?} matches no build category; expected one of {:?}",
                BuildCategory::ALL.iter().map(|c| c.name()).collect::<Vec<_>>()
            );
            None
        })
}

/// Hold at the station with the palette open on the requested tab.
fn open_build_tab(
    category: BuildCategory,
    state: Res<State<GameState>>,
    build_state: Res<State<BuildState>>,
    mut next_build: ResMut<NextState<BuildState>>,
    mut building: ResMut<BuildingState>,
    mut done: Local<bool>,
) {
    if *done || *state.get() != GameState::StationDocked {
        return;
    }
    let index = BuildCategory::ALL.iter().position(|c| *c == category);
    let Some(index) = index else { return };
    building.category_index = index;
    // DEPTHS_BUILD_SLOT parks on one slot rather than the first, so a strip
    // long enough to scroll can be checked at its far end.
    building.selected_index = std::env::var("DEPTHS_BUILD_SLOT")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if *build_state.get() == BuildState::Inactive {
        next_build.set(BuildState::Placing);
    }
    *done = true;
    info!(
        "BUILD TAB MODE: docked, palette open on {} ({} items), slot {} = {}",
        category.name(),
        category.item_count(),
        building.selected_index,
        building.selection_name()
    );
}

/// Counts what is actually resident, once a second, under DEPTHS_CENSUS=1.
///
/// The world holds dozens of ships but only simulates most of them as numbers;
/// a hull becomes real entities inside RENDER_DISTANCE and is despawned again
/// past DESPAWN_DISTANCE. Any argument about what enemy ships cost has to be
/// about the resident count, not the fleet count, and the two are very
/// different numbers.
fn census(
    time: Res<Time>,
    mut next: Local<f32>,
    ai: Query<(), With<AiShip>>,
    owned: Query<(), With<crate::ai_ship::components::OwnedByAiShip>>,
    hull: Query<&ChildOf, With<crate::components::HullSegment>>,
    ai_roots: Query<Entity, With<AiShip>>,
    navs: Query<&crate::crew::navigation::NavGrid>,
    dead: Res<crate::crew::burial::DriftingDead>,
    concealed: Query<(), With<crate::ai_ship::interior::Concealed>>,
) {
    *next -= time.delta_secs();
    if *next > 0.0 {
        return;
    }
    *next = 1.0;
    let roots: std::collections::HashSet<Entity> = ai_roots.iter().collect();
    let ai_hull = hull.iter().filter(|c| roots.contains(&c.parent())).count();
    let nav_cells: usize = navs.iter().map(|n| n.cells.len()).sum();
    let empty_navs = navs.iter().filter(|n| n.cells.is_empty()).count();
    info!(
        "CENSUS ships={} ai_modules={} ai_hull={} nav_grids={} nav_cells={} empty_grids={} concealed={} bodies={}",
        ai.iter().count(),
        owned.iter().count(),
        ai_hull,
        navs.iter().count(),
        nav_cells,
        empty_navs,
        concealed.iter().count(),
        dead.bodies.len()
    );
}

/// Name every sprite sitting near the ship's origin, under DEPTHS_WHATSTHERE=1.
///
/// Built because three separate guesses at "what is that green thing in the
/// middle of the screen" were all wrong, and counting green pixels in a
/// screenshot was worse than useless -- it matched the main menu's own text
/// and the red power numbers scattered over the hull.
///
/// An exclusive system so it can ask the world what components an entity
/// actually carries. Component names need Bevy's `debug` feature to print;
/// without it the colour, size and distance are still enough to identify a
/// sprite in a codebase you can grep. `ViewVisibility` here is last frame's
/// value, since it is computed in PostUpdate -- treat a hit as "exists and is
/// probably drawn", then crop the frame and look before concluding anything.
fn whats_there(world: &mut World) {
    use bevy::ecs::system::SystemState;
    let mut once: SystemState<(
        Query<&GlobalTransform, With<Ship>>,
        Query<(Entity, &Sprite, &GlobalTransform, &ViewVisibility)>,
    )> = SystemState::new(world);
    let Ok((ships, sprites)) = once.get(world) else { return };
    let Ok(ship_gt) = ships.single() else { return };
    let origin = ship_gt.translation().truncate();

    let mut hits: Vec<(Entity, String, f32)> = Vec::new();
    for (entity, sprite, gt, view) in sprites.iter() {
        // Only what is actually drawn. Listing every sprite regardless caught
        // four hidden footprint tiles parked at the origin and sent me after
        // the wrong thing entirely.
        if !view.get() {
            continue;
        }
        let p = gt.translation().truncate();
        let d = p.distance(origin);
        if d > 60.0 {
            continue;
        }
        let c = sprite.color.to_srgba();
        hits.push((
            entity,
            format!(
                "rgba({:.2},{:.2},{:.2},{:.2}) size={:?}",
                c.red, c.green, c.blue, c.alpha, sprite.custom_size
            ),
            d,
        ));
    }

    for (entity, desc, d) in hits {
        let names: Vec<String> = world
            .inspect_entity(entity)
            .map(|infos| infos.map(|i| i.name().to_string()).collect())
            .unwrap_or_default();
        info!("WHATSTHERE {entity:?} d={d:.0} {desc}\n    components: {}", names.join(", "));
    }
}

pub fn skip_ai_ship_spawn() -> bool {
    std::env::var("DEPTHS_MOVETEST").ok().as_deref() == Some("1")
}

impl Plugin for DemoPlugin {
    fn build(&self, app: &mut App) {
        let full_demo = std::env::var("DEPTHS_DEMO").ok().as_deref() == Some("1");
        let skip_menu = std::env::var("DEPTHS_SKIP_MENU").ok().as_deref() == Some("1");
        let move_test = skip_ai_ship_spawn();
        let build_tab = requested_build_tab();
        if std::env::var("DEPTHS_WHATSTHERE").ok().as_deref() == Some("1") {
            app.add_systems(Update, whats_there.run_if(bevy::time::common_conditions::on_timer(
                std::time::Duration::from_millis(900),
            )));
        }
        if std::env::var("DEPTHS_CENSUS").ok().as_deref() == Some("1") {
            app.add_systems(Update, census);
        }
        if !full_demo && !skip_menu && !move_test && build_tab.is_none() {
            return;
        }

        // Stop at the station rather than launching: the palette only exists
        // while docked, so advancing to Exploring would close the thing we
        // came to look at.
        if let Some(category) = build_tab {
            if let Some(secs) = std::env::var("DEPTHS_BUILD_LAUNCH")
                .ok()
                .and_then(|v| v.trim().parse::<f32>().ok())
            {
                app.add_systems(Update, move |
                    time: Res<Time>,
                    state: Res<State<GameState>>,
                    mut next: ResMut<NextState<GameState>>,
                    mut waited: Local<f32>,
                | {
                    if *state.get() != GameState::StationDocked {
                        return;
                    }
                    *waited += time.delta_secs();
                    if *waited > secs {
                        info!("BUILD TAB MODE: launching with the build menu open");
                        next.set(GameState::Exploring);
                    }
                });
            }
            app.insert_resource(DemoAdvanceDelays { menu: 1.0, station: f32::INFINITY })
                .add_systems(Update, demo_advance_states)
                .add_systems(Update, move |
                    state: Res<State<GameState>>,
                    build_state: Res<State<BuildState>>,
                    next_build: ResMut<NextState<BuildState>>,
                    building: ResMut<BuildingState>,
                    done: Local<bool>,
                | open_build_tab(category, state, build_state, next_build, building, done));
            return;
        }

        let delays = if move_test {
            DemoAdvanceDelays { menu: 0.05, station: 0.05 }
        } else {
            DemoAdvanceDelays { menu: 2.0, station: 4.0 }
        };
        app.insert_resource(delays)
            .add_systems(Update, demo_advance_states);

        // DEPTHS_MOVETEST_ENEMY_AUTOPILOT drives the player ship at the
        // single movetest dummy automatically, for unattended faction-
        // behavior verification (same demo_autopilot used by full
        // DEPTHS_DEMO, just also enabled in the single-dummy sandbox).
        // Combine with DEPTHS_MOVETEST_ENEMY_FACTION to test a specific
        // faction's decision tree in isolation.
        let movetest_autopilot = move_test
            && std::env::var("DEPTHS_MOVETEST_ENEMY_AUTOPILOT").ok().as_deref() == Some("1");

        if full_demo || movetest_autopilot {
            info!("DEMO MODE: autopilot + periodic engine screenshots active");
            app.add_systems(Update, demo_screenshots)
                .add_systems(
                    Update,
                    demo_autopilot
                        .in_set(crate::states::ShipSet::Movement)
                        .before(crate::ship::ship_movement)
                        .after(crate::combat::targeting::fire_groups::fire_group_input)
                        .run_if(in_state(GameState::Exploring)),
                );
        } else if move_test {
            info!("MOVETEST MODE: empty space, starter ship, no AI ships, manual control");
        } else {
            info!("SKIP MENU MODE: jumping straight into Exploring, manual control");
        }
    }
}

/// Menu → station → launch, on a timer instead of keypresses.
fn demo_advance_states(
    time: Res<Time>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    delays: Res<DemoAdvanceDelays>,
    mut t: Local<f32>,
) {
    *t += time.delta_secs();
    match state.get() {
        GameState::MainMenu if *t > delays.menu => {
            next.set(GameState::StationDocked);
            *t = 0.0;
        }
        GameState::StationDocked if *t > delays.station => {
            next.set(GameState::Exploring);
            *t = 0.0;
        }
        _ => {}
    }
}

/// Fly toward the nearest enemy (spawned entity first, simulated ship as a
/// distant waypoint otherwise), keep a fighting distance, select it as the
/// target, and hold fire when in range.
fn demo_autopilot(
    mut input_state: ResMut<InputState>,
    mut selection: ResMut<TargetSelection>,
    mut fire_state: ResMut<FireGroupState>,
    sim: Res<WorldSimulation>,
    ai_ships: Query<(Entity, &Transform), With<AiShip>>,
    mut ship_query: Query<(&mut Transform, &mut ShipPhysics), (With<Ship>, Without<AiShip>)>,
) {
    let Ok((mut transform, mut physics)) = ship_query.single_mut() else { return };
    let pos = transform.translation.truncate();

    let mut best_dist = f32::MAX;
    let mut target_pos: Option<Vec2> = None;
    let mut target_entity: Option<Entity> = None;

    for (entity, t) in ai_ships.iter() {
        let p = t.translation.truncate();
        let d = pos.distance(p);
        if d < best_dist {
            best_dist = d;
            target_pos = Some(p);
            target_entity = Some(entity);
        }
    }
    if target_entity.is_none() {
        for s in sim.ships.iter().filter(|s| !s.spawned && s.behavior != SimBehavior::Dead) {
            let d = pos.distance(s.position);
            if d < best_dist {
                best_dist = d;
                target_pos = Some(s.position);
            }
        }
    }
    let Some(tp) = target_pos else { return };

    // Steer the nose straight at the target (authoritative — the cursor
    // isn't over the window during unattended runs, so nothing fights this)
    let dir = tp - pos;
    let angle = dir.y.atan2(dir.x);
    physics.rotation = angle;
    physics.angular_velocity = 0.0;

    // Approach, then hold a fighting distance
    input_state.movement.y = if best_dist > 700.0 {
        1.0
    } else if best_dist < 350.0 {
        -0.4
    } else {
        0.2
    };
    input_state.movement.x = 0.0;

    if let Some(entity) = target_entity {
        selection.target = Some(entity);
        selection.target_type = TargetType::Ship;
        let in_range = best_dist < 900.0;
        fire_state.firing = [in_range; 4];
    } else {
        selection.target = None;
        fire_state.firing = [false; 4];
    }
}

/// Engine-rendered screenshot every few seconds into DEPTHS_DEMO_DIR.
fn demo_screenshots(
    mut commands: Commands,
    time: Res<Time>,
    mut since_last: Local<f32>,
    mut index: Local<u32>,
    offscreen: Option<Res<OffscreenTarget>>,
) {
    *since_last += time.delta_secs();
    if *since_last < 4.0 {
        return;
    }
    *since_last = 0.0;

    let dir = std::env::var("DEPTHS_DEMO_DIR").unwrap_or_else(|_| "/tmp/depths_frames".to_string());
    let _ = std::fs::create_dir_all(&dir);
    let path = format!("{}/frame_{:03}.png", dir, *index);
    *index += 1;

    commands.spawn(screenshot_of(offscreen.as_deref())).observe(save_to_disk(path));
}

// ============================================================================
// ENGINE-SIDE CAPTURE — usable during ordinary play.
//
// The existing DEPTHS_DEMO capture is welded to the autopilot, which is no use
// for photographing a session somebody is actually playing. This is the same
// mechanism with nothing else attached.
//
// It matters that this is an engine capture rather than an OS screen grab.
// `Screenshot::primary_window()` reads the render target, so it never catches
// another app the way an OS grab of the display does — verified the hard way.
// But a render target only holds what was drawn, and macOS stops drawing a
// window that is fully covered: a background playtest got one real frame,
// the instant the window opened, then 63 blank. DEPTHS_OFFSCREEN renders into
// an image instead, which keeps drawing regardless; see `OffscreenTarget`.
// ============================================================================

/// The image the game renders into when `DEPTHS_OFFSCREEN=1`, instead of the
/// window.
///
/// Engine-side capture photographs a render target, so it never catches
/// another app -- but it can only photograph what the game actually DRAWS,
/// and macOS stops a window drawing once another window fully covers it.
/// Playtests run with this app in front, so a background run produced one
/// real frame (the instant the window opened) and then nothing: the director
/// itself reported 63 of 64 frames blank. Rendering into an image takes the
/// window out of the capture path entirely.
///
/// The window shows nothing while this is on. It is for unattended runs.
#[derive(Resource, Clone)]
pub struct OffscreenTarget(pub Handle<Image>);

/// Marks the camera once it has been pointed at the offscreen image, so the
/// redirect happens exactly once however late the camera spawns.
#[derive(Component)]
struct RendersOffscreen;

/// The screenshot to take: of the offscreen image when the game is rendering
/// into one, of the window otherwise. Every capture goes through this, or one
/// of them would quietly keep photographing an empty window.
pub fn screenshot_of(offscreen: Option<&OffscreenTarget>) -> Screenshot {
    match offscreen {
        Some(target) => Screenshot::image(target.0.clone()),
        None => Screenshot::primary_window(),
    }
}

/// The window's LOGICAL size (see main.rs), so the offscreen view frames the
/// same patch of space and lays the HUD out identically at scale 1.0.
const OFFSCREEN_SIZE: (u32, u32) = (1280, 720);

fn create_offscreen_target(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    use bevy::render::render_resource::{TextureFormat, TextureUsages};
    let mut image = Image::new_target_texture(
        OFFSCREEN_SIZE.0,
        OFFSCREEN_SIZE.1,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    // `new_target_texture` sets the flags for drawing INTO the image. A
    // screenshot copies it back OUT, which needs COPY_SRC as well -- without
    // it this captures black for a reason that has nothing to do with macOS,
    // and would be misread as "offscreen rendering doesn't work".
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    commands.insert_resource(OffscreenTarget(images.add(image)));
    info!("capture: rendering offscreen at {}x{}", OFFSCREEN_SIZE.0, OFFSCREEN_SIZE.1);
}

fn aim_camera_offscreen(
    mut commands: Commands,
    target: Res<OffscreenTarget>,
    cameras: Query<Entity, (With<crate::camera::MainCamera>, Without<RendersOffscreen>)>,
) {
    for camera in &cameras {
        commands.entity(camera).insert((
            bevy::camera::RenderTarget::Image(bevy::camera::ImageRenderTarget {
                handle: target.0.clone(),
                scale_factor: 1.0,
            }),
            // Without this the HUD vanishes from every frame. Bevy draws UI on
            // the camera marked IsDefaultUiCamera, or failing that the highest
            // camera targeting the PRIMARY WINDOW -- and an image target is
            // neither, so the whole interface was drawing to nowhere. No UI
            // root in this game sets UiTargetCamera, so this one marker moves
            // all of it.
            bevy::ui::IsDefaultUiCamera,
            RendersOffscreen,
        ));
    }
}

/// A key press script read from `DEPTHS_KEYS`. See the header for the format.
#[derive(Resource, Default)]
struct ScriptedKeys {
    /// (seconds, key, hold seconds), sorted by time. Hold 0 is a one-frame tap.
    events: Vec<(f32, KeyCode, f32)>,
    next: usize,
    /// Keys currently down and when to let them go. A tap releases the next
    /// frame -- one frame down is what `just_pressed` reads.
    held: Vec<(KeyCode, f32)>,
}

/// Key names as Bevy spells them. Only what a script plausibly needs: letters,
/// digits, function keys and the few named keys the game binds.
fn parse_key(name: &str) -> Option<KeyCode> {
    let name = name.trim();
    if let Some(c) = name.strip_prefix("Key").filter(|c| c.len() == 1) {
        let i = c.chars().next()? as u8;
        if (b'A'..=b'Z').contains(&i) {
            const L: [KeyCode; 26] = [
                KeyCode::KeyA, KeyCode::KeyB, KeyCode::KeyC, KeyCode::KeyD, KeyCode::KeyE,
                KeyCode::KeyF, KeyCode::KeyG, KeyCode::KeyH, KeyCode::KeyI, KeyCode::KeyJ,
                KeyCode::KeyK, KeyCode::KeyL, KeyCode::KeyM, KeyCode::KeyN, KeyCode::KeyO,
                KeyCode::KeyP, KeyCode::KeyQ, KeyCode::KeyR, KeyCode::KeyS, KeyCode::KeyT,
                KeyCode::KeyU, KeyCode::KeyV, KeyCode::KeyW, KeyCode::KeyX, KeyCode::KeyY,
                KeyCode::KeyZ,
            ];
            return Some(L[(i - b'A') as usize]);
        }
    }
    if let Some(d) = name.strip_prefix("Digit").filter(|d| d.len() == 1) {
        const D: [KeyCode; 10] = [
            KeyCode::Digit0, KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4,
            KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9,
        ];
        return d.parse::<usize>().ok().map(|n| D[n]);
    }
    Some(match name {
        "F1" => KeyCode::F1, "F2" => KeyCode::F2, "F3" => KeyCode::F3, "F4" => KeyCode::F4,
        "F5" => KeyCode::F5, "F6" => KeyCode::F6, "F7" => KeyCode::F7, "F8" => KeyCode::F8,
        "F9" => KeyCode::F9, "F10" => KeyCode::F10, "F11" => KeyCode::F11, "F12" => KeyCode::F12,
        "Tab" => KeyCode::Tab,
        "Escape" => KeyCode::Escape,
        "Enter" => KeyCode::Enter,
        "ArrowUp" => KeyCode::ArrowUp,
        "ArrowDown" => KeyCode::ArrowDown,
        "ArrowLeft" => KeyCode::ArrowLeft,
        "ArrowRight" => KeyCode::ArrowRight,
        "Space" => KeyCode::Space,
        "Semicolon" => KeyCode::Semicolon,
        "BracketLeft" => KeyCode::BracketLeft,
        "BracketRight" => KeyCode::BracketRight,
        "ShiftLeft" => KeyCode::ShiftLeft,
        "Minus" => KeyCode::Minus,
        "Equal" => KeyCode::Equal,
        _ => return None,
    })
}

/// "3:KeyM,5:F7" -> [(3.0, M), (5.0, F7)]. A malformed entry is reported and
/// skipped rather than aborting the run: a typo in one step should not cost
/// the whole capture session.
fn parse_script(script: &str) -> Vec<(f32, KeyCode, f32)> {
    let mut events: Vec<(f32, KeyCode, f32)> = script
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .filter_map(|step| {
            let parsed = (|| {
                let (t, rest) = step.split_once(':')?;
                let (k, hold) = match rest.split_once('~') {
                    Some((k, h)) => (k, h.trim().parse::<f32>().ok()?),
                    None => (rest, 0.0),
                };
                Some((t.trim().parse::<f32>().ok()?, parse_key(k)?, hold))
            })();
            if parsed.is_none() {
                warn!("DEPTHS_KEYS: ignoring '{}'", step.trim());
            }
            parsed
        })
        .collect();
    events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    events
}

fn play_scripted_keys(
    time: Res<Time<Real>>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    mut script: ResMut<ScriptedKeys>,
) {
    let now = time.elapsed_secs();
    // Release what is due; keep pressing what is still being held, since
    // ButtonInput forgets nothing on its own but other systems may release.
    script.held.retain(|&(key, until)| {
        if now >= until {
            keyboard.release(key);
            false
        } else {
            keyboard.press(key);
            true
        }
    });
    while let Some(&(at, key, hold)) = script.events.get(script.next) {
        if now < at {
            break;
        }
        keyboard.press(key);
        // A tap's release time is "now", so it lets go on the next frame.
        script.held.push((key, now + hold));
        script.next += 1;
    }
}

/// Marker so the capture plugin can be added unconditionally and cost nothing
/// when neither the env var nor the key is used.
pub struct CapturePlugin;

#[derive(Resource)]
struct CaptureState {
    every: Option<f32>,
    since: f32,
    index: u32,
    dir: String,
}

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        let every = std::env::var("DEPTHS_SHOTS")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|v| *v > 0.0);
        let dir = std::env::var("DEPTHS_SHOTS_DIR")
            .unwrap_or_else(|_| "/tmp/depths_shots".to_string());
        if every.is_some() {
            info!("capture: every {}s into {}  (F7 for one now)", every.unwrap(), dir);
        }
        app.insert_resource(CaptureState { every, since: 0.0, index: 0, dir })
            .add_systems(Update, capture_frames);
        if let Ok(script) = std::env::var("DEPTHS_KEYS") {
            let events = parse_script(&script);
            info!("DEPTHS_KEYS: {} scripted presses", events.len());
            app.insert_resource(ScriptedKeys { events, ..default() }).add_systems(
                PreUpdate,
                play_scripted_keys.after(bevy::input::InputSystems),
            );
        }
        if std::env::var("DEPTHS_OFFSCREEN").ok().as_deref() == Some("1") {
            app.add_systems(Startup, create_offscreen_target)
                .add_systems(
                    Update,
                    aim_camera_offscreen.run_if(resource_exists::<OffscreenTarget>),
                );
        }
    }
}

fn capture_frames(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut st: ResMut<CaptureState>,
    offscreen: Option<Res<OffscreenTarget>>,
) {
    let on_demand = keys.just_pressed(KeyCode::F7);

    let periodic = match st.every {
        Some(every) => {
            st.since += time.delta_secs();
            if st.since >= every {
                st.since = 0.0;
                true
            } else {
                false
            }
        }
        None => false,
    };

    if !on_demand && !periodic {
        return;
    }

    if std::fs::create_dir_all(&st.dir).is_err() {
        warn!("capture: cannot create {}", st.dir);
        return;
    }
    let path = format!("{}/shot_{:04}.png", st.dir, st.index);
    st.index += 1;
    if on_demand {
        info!("capture: {}", path);
    }
    commands
        .spawn(screenshot_of(offscreen.as_deref()))
        .observe(save_to_disk(path));
}

#[cfg(test)]
mod offscreen_tests {
    use super::*;
    use bevy::render::render_resource::TextureUsages;

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>();
        app.add_systems(Startup, create_offscreen_target);
        app.add_systems(
            Update,
            aim_camera_offscreen.run_if(resource_exists::<OffscreenTarget>),
        );
        app
    }

    /// The image has to be readable BACK out. `new_target_texture` only sets
    /// the flags for drawing into it; without COPY_SRC every capture comes
    /// back black -- the same symptom as a covered window, for an unrelated
    /// reason, and exactly the kind that gets misdiagnosed.
    #[test]
    fn the_target_can_be_drawn_into_and_read_back_out() {
        let mut app = app();
        app.update();
        let handle = app.world().resource::<OffscreenTarget>().0.clone();
        let usage = app.world().resource::<Assets<Image>>().get(&handle).unwrap().texture_descriptor.usage;
        assert!(usage.contains(TextureUsages::RENDER_ATTACHMENT), "cannot be rendered into");
        assert!(usage.contains(TextureUsages::COPY_SRC), "cannot be read back -- every capture would be black");
    }

    /// Matches the window's logical size, so the frame shows the same patch
    /// of space and the HUD lays out as it does on screen.
    #[test]
    fn the_target_matches_the_window() {
        let mut app = app();
        app.update();
        let handle = app.world().resource::<OffscreenTarget>().0.clone();
        let image = app.world().resource::<Assets<Image>>().get(&handle).unwrap();
        assert_eq!((image.width(), image.height()), OFFSCREEN_SIZE);
    }

    /// The camera is redirected AND made the UI camera. The second half is the
    /// one that bit: Bevy draws UI on the IsDefaultUiCamera, or failing that
    /// the highest camera targeting the primary window. An image target is
    /// neither, so without the marker the whole HUD drew to nowhere and every
    /// frame showed the world with no interface at all.
    #[test]
    fn the_camera_renders_offscreen_with_its_hud() {
        let mut app = app();
        let camera = app.world_mut().spawn(crate::camera::MainCamera).id();
        app.update();
        app.update();
        let world = app.world();
        assert!(
            matches!(world.get::<bevy::camera::RenderTarget>(camera), Some(bevy::camera::RenderTarget::Image(_))),
            "camera still renders to the window"
        );
        assert!(
            world.get::<bevy::ui::IsDefaultUiCamera>(camera).is_some(),
            "camera renders offscreen but is not the UI camera -- the HUD would be missing"
        );
    }

    /// Redirected once, not every frame. A second insert each frame would
    /// churn the camera's render target for nothing.
    #[test]
    fn the_camera_is_redirected_once() {
        let mut app = app();
        let camera = app.world_mut().spawn(crate::camera::MainCamera).id();
        for _ in 0..4 {
            app.update();
        }
        assert!(app.world().get::<RendersOffscreen>(camera).is_some());
    }
}
