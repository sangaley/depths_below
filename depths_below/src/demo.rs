use bevy::prelude::*;
use bevy::render::view::window::screenshot::{save_to_disk, Screenshot};
use crate::components::{Ship, ShipPhysics};
use crate::resources::InputState;
use crate::states::GameState;
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

pub fn skip_ai_ship_spawn() -> bool {
    std::env::var("DEPTHS_MOVETEST").ok().as_deref() == Some("1")
}

impl Plugin for DemoPlugin {
    fn build(&self, app: &mut App) {
        let full_demo = std::env::var("DEPTHS_DEMO").ok().as_deref() == Some("1");
        let skip_menu = std::env::var("DEPTHS_SKIP_MENU").ok().as_deref() == Some("1");
        let move_test = skip_ai_ship_spawn();
        if !full_demo && !skip_menu && !move_test {
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

    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
}
