//! AUTOPLAY — unattended full-run director. Dev tooling, not a game feature.
//!
//! Where `demo.rs` flies at the nearest enemy and takes a photo every four
//! seconds, this plays the actual game: it sits through flight training, buys
//! a loadout, takes bounties off the board, hunts them down, strips the
//! wrecks, docks, turns in, and warps to the next system when the local board
//! runs dry — until a credit target is hit or the run wedges.
//!
//! **It plays through the real input layer.** Every decision comes out as a
//! synthetic `ButtonInput<KeyCode>` press (the same bridge `gamepad.rs` uses
//! for pad buttons) plus `InputState.gamepad_aim` for facing. Nothing here
//! writes game state directly, so a run exercises the same code a human does
//! — if the director can't do something, a player with a controller can't
//! either, and that's a finding rather than a harness bug.
//!
//! Env:
//!   DEPTHS_AUTOPLAY=1              — arm the director
//!   DEPTHS_AUTOPLAY_TARGET=15000   — credits to stop at (default 15000)
//!   DEPTHS_AUTOPLAY_DIR=<path>     — journal + screenshots (default /tmp/depths_run)
//!   DEPTHS_AUTOPLAY_MAXSPEED=6     — max time dilation on empty transits (default 6)
//!   DEPTHS_AUTOPLAY_DEADLINE=7200  — wall-clock seconds before giving up (default 7200)

use bevy::ecs::system::SystemParam;
use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::render::view::window::screenshot::{Screenshot, ScreenshotCaptured};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::ai_ship::components::{AiShip, AiShipWreck};
use crate::celestial::poi::SpacePoi;
use crate::combat::targeting::selection::TargetSelection;
use crate::components::{Ship, Velocity, Wreck};
use crate::contracts::{ContractState, ContractStatus, MissionBoardOpen};
use crate::crew::eva_salvage::EvaSalvaging;
use crate::events::{AiShipDestroyed, NotificationType, ShowNotification};
use crate::resources::{Currency, FuelState, HullState, InputState, StaffingState};
use crate::states::GameState;
use crate::tutorial::{Advance, Tutorial};
use crate::world::home_base::{SystemStations, DOCK_RANGE};

// ============================================================================
// TUNING
// ============================================================================

/// Where the director wants to sit while fighting. The starter battery reaches
/// 2200 (gatling) to 9600 (heavy missile), and `auto_engage` will pick blocks
/// out to 9000, so parking well outside knife range still puts most guns on
/// target. The old 750 flew the ship through its whole envelope without firing
/// and then rammed — momentum is forever in this game, so closing to contact
/// is a decision, not a default.
const STANDOFF: f32 = 2600.0;
/// Open fire at this range. Past the gatling's reach, comfortably inside the
/// cannon/railgun/missile envelope.
const FIRE_RANGE: f32 = 5000.0;
/// Closing faster than this inside standoff means retro-thrust, or we overshoot
/// and end up hull-to-hull.
const CLOSING_SPEED_CAP: f32 = 240.0;
/// Break off and run for a station below this hull fraction.
const RETREAT_HULL: f32 = 0.35;
/// Loot interact range (poi.rs loots derelicts/anomalies within 1400).
const INTERACT_RANGE: f32 = 1200.0;
/// Where to park to work a wreck. eva_salvage dispatches inside ORDER_RANGE
/// (3000) but breaks the detail off past BREAK_RANGE (1800), so sit well
/// inside the break distance and hold still — drifting is what loses a detail.
const SALVAGE_HOLD: f32 = 1300.0;
/// Drift past this while a detail is out and we go back to closing the gap.
const SALVAGE_LEASH: f32 = 1650.0;
/// Give up on a wreck after this long rather than parking there forever.
const SALVAGE_TIMEOUT: f32 = 150.0;
/// No credit movement for this long (game seconds) → log a stall and re-plan.
const STALL_SECONDS: f32 = 240.0;
/// Minimum gap between screenshots, so a chatty phase can't fill the disk.
const SHOT_COOLDOWN: f32 = 3.0;

// ============================================================================
// STATE
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Main menu — start a new expedition (which arms the tutorial).
    Boot,
    /// Sit through flight training, doing what each step asks.
    Training,
    /// Docked: spend credits on a fighting loadout.
    Outfit,
    /// Docked: open the board and take everything affordable.
    Board,
    /// Docked: launch into open space.
    Launch,
    /// Fly to the objective and kill it.
    Hunt,
    /// Strip the wreck / loot the derelict we're parked on.
    Strip,
    /// Head home, dock, turn in, sell.
    Home,
    /// Credit target reached.
    Done,
    /// Deadline or unrecoverable stall.
    Aborted,
}

#[derive(Resource)]
pub struct Director {
    pub phase: Phase,
    phase_elapsed: f32,
    run_elapsed: f32,
    /// Keys held down this frame.
    held: HashSet<KeyCode>,
    /// Keys to flash for exactly one frame (so `just_pressed` fires once).
    taps: Vec<KeyCode>,
    /// What we pressed last frame, so we know what to release.
    emulated: HashSet<KeyCode>,
    /// Facing, fed to InputState.gamepad_aim.
    aim: Option<Vec2>,
    target: u32,
    deadline: f32,
    max_speed: f32,
    dir: String,
    journal: Option<File>,
    shots: u32,
    shot_cooldown: f32,
    last_credits: u32,
    stall_timer: f32,
    kills: u32,
    /// Guards one-shot work inside a phase (outfit buys, board sweep).
    step: u32,
    /// Tutorial info-steps need Space on a rhythm, not every frame.
    beat: f32,
    /// Frame delta, so phases can run their own timers.
    dt: f32,
    /// Salvage is dispatch-and-wait: F sends a detail out and pressing it
    /// again RECALLS them. These track that handshake so we press it once.
    detail_sent: bool,
    detail_seen: bool,
    strip_wait: f32,
    loot_cooldown: f32,
    /// Wrecks we tried and got nothing from. `Wreck::loot_remaining` already
    /// filters stripped hulks, but a wreck can also refuse a detail because
    /// there is no idle crew — without remembering those, Hunt sees the wreck,
    /// diverts to Strip, fails, and bounces straight back forever.
    spent_wrecks: HashSet<Entity>,
    /// Grace period after leaving Strip so a bad wreck can't re-trigger it on
    /// the very next frame.
    strip_cooldown: f32,
    /// A capture came back blank and is owed a retake.
    shot_pending: bool,
    shot_retries: u32,
    blanks_seen: u32,
    /// Seconds until the next periodic status line.
    status_in: f32,
    /// How long we've been holding station for an EVA detail.
    eva_hold: f32,
}

impl Director {
    fn tap(&mut self, key: KeyCode) {
        self.taps.push(key);
    }
    fn hold(&mut self, key: KeyCode) {
        self.held.insert(key);
    }

    /// Structured line into the run journal. This is the artifact a human
    /// actually reads afterwards, so it gets timestamps and phase context.
    fn log(&mut self, kind: &str, msg: &str) {
        let line = format!(
            "{{\"t\":{:.1},\"phase\":\"{:?}\",\"kind\":\"{}\",\"msg\":{}}}",
            self.run_elapsed,
            self.phase,
            kind,
            json_string(msg)
        );
        info!("[AUTOPLAY] {} {}", kind, msg);
        if let Some(f) = self.journal.as_mut() {
            let _ = writeln!(f, "{}", line);
            let _ = f.flush();
        }
    }

    fn go(&mut self, phase: Phase) {
        if self.phase == phase {
            return;
        }
        let msg = format!("{:?} -> {:?}", self.phase, phase);
        self.log("phase", &msg);
        self.phase = phase;
        self.phase_elapsed = 0.0;
        self.step = 0;
        self.detail_sent = false;
        self.detail_seen = false;
        self.strip_wait = 0.0;
    }
}

/// Minimal JSON string escaping — notification text is game-authored and can
/// contain quotes, and a malformed journal is worse than no journal.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn env_f32(key: &str, default: f32) -> f32 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

// ============================================================================
// PLUGIN
// ============================================================================

pub struct AutoplayPlugin;

impl Plugin for AutoplayPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("DEPTHS_AUTOPLAY").ok().as_deref() != Some("1") {
            return;
        }

        let dir = std::env::var("DEPTHS_AUTOPLAY_DIR")
            .unwrap_or_else(|_| "/tmp/depths_run".to_string());
        let _ = std::fs::create_dir_all(&dir);
        let journal = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(format!("{}/journal.jsonl", dir))
            .ok();

        let target = env_f32("DEPTHS_AUTOPLAY_TARGET", 15000.0) as u32;
        let mut director = Director {
            phase: Phase::Boot,
            phase_elapsed: 0.0,
            run_elapsed: 0.0,
            held: HashSet::new(),
            taps: Vec::new(),
            emulated: HashSet::new(),
            aim: None,
            target,
            deadline: env_f32("DEPTHS_AUTOPLAY_DEADLINE", 7200.0),
            max_speed: env_f32("DEPTHS_AUTOPLAY_MAXSPEED", 6.0),
            dir,
            journal,
            shots: 0,
            shot_cooldown: 0.0,
            last_credits: 0,
            stall_timer: 0.0,
            kills: 0,
            step: 0,
            beat: 0.0,
            dt: 0.0,
            detail_sent: false,
            detail_seen: false,
            strip_wait: 0.0,
            loot_cooldown: 0.0,
            spent_wrecks: HashSet::new(),
            strip_cooldown: 0.0,
            shot_pending: false,
            shot_retries: 0,
            blanks_seen: 0,
            status_in: 0.0,
            eva_hold: 0.0,
        };
        director.log("start", &format!("target {}c", target));

        info!("AUTOPLAY: unattended run armed, target {}c", target);

        app.insert_resource(director)
            // After Bevy's own input pass, so emulated presses get correct
            // just_pressed semantics — same reasoning as gamepad.rs.
            .add_systems(PreUpdate, apply_synthetic_input.after(InputSystems))
            .add_systems(
                Update,
                (
                    director_clock,
                    director_brain,
                    director_timescale,
                    director_status,
                    director_watch,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                apply_aim
                    .in_set(crate::states::ShipSet::Movement)
                    .before(crate::ship::ship_movement),
            );
    }
}

// ============================================================================
// INPUT BRIDGE
// ============================================================================

/// Presses/releases the keys the brain asked for. Held keys are pressed every
/// frame (idempotent); taps land for one frame and are released the next, so
/// each tap produces exactly one `just_pressed`.
fn apply_synthetic_input(mut keyboard: ResMut<ButtonInput<KeyCode>>, mut d: ResMut<Director>) {
    let mut desired: HashSet<KeyCode> = d.held.clone();
    for key in d.taps.drain(..) {
        desired.insert(key);
    }

    let stale: Vec<KeyCode> = d.emulated.difference(&desired).copied().collect();
    for key in stale {
        keyboard.release(key);
    }
    for key in desired.iter() {
        keyboard.press(*key);
    }

    d.emulated = desired;
    d.held.clear();
}

/// Facing goes through the controller aim path — ship_movement and the
/// dumb-fire weapons both already read it, so nothing here special-cases
/// autoplay.
fn apply_aim(mut input_state: ResMut<InputState>, d: Res<Director>) {
    if let Some(aim) = d.aim {
        input_state.gamepad_aim = Some(aim);
    }
}

// ============================================================================
// CLOCK / BOOKKEEPING
// ============================================================================

fn director_clock(time: Res<Time<Real>>, mut d: ResMut<Director>) {
    let dt = time.delta_secs();
    d.run_elapsed += dt;
    d.phase_elapsed += dt;
    d.shot_cooldown = (d.shot_cooldown - dt).max(0.0);
    d.loot_cooldown = (d.loot_cooldown - dt).max(0.0);
    d.strip_cooldown = (d.strip_cooldown - dt).max(0.0);
    d.beat += dt;
    d.dt = dt;
}

// ============================================================================
// THE BRAIN
// ============================================================================

#[derive(SystemParam)]
struct World1<'w, 's> {
    ships: Query<'w, 's, (&'static Transform, &'static Velocity), With<Ship>>,
    enemies: Query<'w, 's, (Entity, &'static Transform), (With<AiShip>, Without<AiShipWreck>, Without<Ship>)>,
    wrecks: Query<'w, 's, (Entity, &'static Transform, &'static Wreck), Without<Ship>>,
    pois: Query<'w, 's, (&'static Transform, &'static SpacePoi), Without<Ship>>,
    eva: Query<'w, 's, (), With<EvaSalvaging>>,
}

fn director_brain(
    mut d: ResMut<Director>,
    state: Res<State<GameState>>,
    mut next_state: ResMut<NextState<GameState>>,
    tutorial: Res<Tutorial>,
    currency: Res<Currency>,
    hull: Res<HullState>,
    fuel: Res<FuelState>,
    contracts: Res<ContractState>,
    board_open: Res<MissionBoardOpen>,
    stations: Res<SystemStations>,
    selection: Res<TargetSelection>,
    w: World1,
) {
    if matches!(d.phase, Phase::Done | Phase::Aborted) {
        return;
    }

    // ---- goal check -------------------------------------------------------
    if currency.credits >= d.target {
        let msg = format!(
            "target reached: {}c after {:.0}s, {} kills, {} frames saved ({} blank)",
            currency.credits,
            d.run_elapsed,
            d.kills,
            SAVED_CAPTURES.load(Ordering::Relaxed),
            BLANK_CAPTURES.load(Ordering::Relaxed)
        );
        d.log("done", &msg);
        d.go(Phase::Done);
        return;
    }
    if d.run_elapsed > d.deadline {
        let msg = format!(
            "deadline hit at {}c, {} kills, {} frames saved ({} blank)",
            currency.credits,
            d.kills,
            SAVED_CAPTURES.load(Ordering::Relaxed),
            BLANK_CAPTURES.load(Ordering::Relaxed)
        );
        d.log("abort", &msg);
        d.go(Phase::Aborted);
        return;
    }

    // ---- credit-progress watchdog ----------------------------------------
    if currency.credits != d.last_credits {
        if currency.credits > d.last_credits {
            let msg = format!("+{}c (now {})", currency.credits - d.last_credits, currency.credits);
            d.log("credits", &msg);
        }
        d.last_credits = currency.credits;
        d.stall_timer = 0.0;
    }

    // StationDocked is the one that has build mode, the board and hiring.
    // `Docked` is a different thing entirely (outpost/wreck), and Loading /
    // Paused are transients. Treating anything non-Exploring as "not docked"
    // is what made the director thrash Outfit -> Hunt -> Boot in three frames
    // the moment it touched an outpost.
    let at_station = *state.get() == GameState::StationDocked;
    let flying = *state.get() == GameState::Exploring;

    // Crew outside pins the ship. A detail breaks off past BREAK_RANGE, so
    // flying anywhere with people on the hull strands them — which is exactly
    // how an earlier run lost a detail six crew at a time while it hurried off
    // to dock. Hold station and let them finish.
    if flying && !w.eva.is_empty() {
        // Record that a detail really did go out. Strip's completion test runs
        // below this guard and would otherwise never observe it, and would
        // write the wreck off as "no detail launched" the moment they landed.
        d.detail_seen = true;
        d.eva_hold += d.dt;

        if let Ok((_, vel_c)) = w.ships.single() {
            if vel_c.0.length() > 30.0 {
                d.hold(KeyCode::ShiftLeft);
            }
        }

        // A detail that never comes home would pin the ship for the rest of
        // the run. Recall them the way a player would and carry on.
        if d.eva_hold > 150.0 && d.beat > 1.5 {
            d.beat = 0.0;
            d.eva_hold = 0.0;
            d.log("salvage", "detail out too long — recalling");
            d.tap(KeyCode::KeyF);
        }
        return;
    }
    d.eva_hold = 0.0;

    match *state.get() {
        GameState::Paused => {
            if d.beat > 1.0 {
                d.beat = 0.0;
                d.tap(KeyCode::Escape);
            }
            return;
        }
        GameState::Docked => {
            if d.beat > 1.0 {
                d.beat = 0.0;
                d.tap(KeyCode::Enter);
            }
            return;
        }
        GameState::Loading | GameState::GameOver => return,
        _ => {}
    }

    // ---- phase logic ------------------------------------------------------
    match d.phase {
        Phase::Boot => {
            // Enter on the main menu starts a new expedition and arms training.
            if *state.get() == GameState::MainMenu {
                if d.beat > 0.7 {
                    d.beat = 0.0;
                    d.tap(KeyCode::Enter);
                }
            } else if at_station {
                d.go(Phase::Training);
            }
        }

        Phase::Training => {
            match tutorial.pending() {
                None => {
                    d.log("tutorial", "training complete");
                    d.go(if at_station { Phase::Outfit } else { Phase::Home });
                }
                Some(step) => {
                    // Training is a nice-to-have on a run whose point is the
                    // economy loop. If a step can't be satisfied, dismiss it
                    // ([;]) and get on with the game rather than burning the
                    // whole deadline on the tutorial card.
                    if d.phase_elapsed > 240.0 {
                        let msg = format!("stuck on {:?} for 240s — dismissing training", step);
                        d.log("tutorial", &msg);
                        d.tap(KeyCode::Semicolon);
                    } else {
                        drive_tutorial(&mut d, step, &w, &stations, flying)
                    }
                }
            }
        }

        Phase::Outfit => {
            if !at_station {
                d.go(Phase::Hunt);
                return;
            }
            // The starter hull is already a working ship; the honest thing to
            // check here is that the shipyard opens and closes cleanly. Actual
            // module buying is mouse-driven placement (see module docs).
            if d.step == 0 && d.phase_elapsed > 1.5 {
                d.tap(KeyCode::KeyB);
                d.step = 1;
                d.log("outfit", "opened shipyard");
            } else if d.step == 1 && d.phase_elapsed > 4.0 {
                d.tap(KeyCode::Escape);
                d.step = 2;
            } else if d.step == 2 && d.phase_elapsed > 5.5 {
                d.go(Phase::Board);
            }
        }

        Phase::Board => {
            if !at_station {
                d.go(Phase::Hunt);
                return;
            }
            let active = contracts
                .active_contracts
                .iter()
                .filter(|c| c.status == ContractStatus::Active)
                .count();

            if !board_open.0 {
                if active >= 3 || d.step > 24 {
                    d.go(Phase::Launch);
                } else if d.beat > 0.6 {
                    d.beat = 0.0;
                    d.tap(KeyCode::KeyJ);
                }
                return;
            }

            // Board is open: walk the list and Enter on each available job.
            // Down-then-Enter repeatedly takes the top of the list as it
            // shrinks, which is exactly what a player does.
            if d.beat > 0.45 {
                d.beat = 0.0;
                d.step += 1;
                if active >= 3 || d.step > 24 {
                    d.tap(KeyCode::KeyJ); // close board
                } else if d.step % 2 == 1 {
                    d.tap(KeyCode::Enter);
                } else {
                    d.tap(KeyCode::ArrowDown);
                }
            }
        }

        Phase::Launch => {
            if flying {
                d.log("launch", "in open space");
                d.go(Phase::Hunt);
            } else if at_station && d.beat > 0.8 {
                d.beat = 0.0;
                d.tap(KeyCode::Enter);
            }
        }

        Phase::Hunt => {
            if !flying {
                if at_station {
                    d.go(Phase::Board);
                }
                return;
            }
            let integrity = hull.hull_integrity;
            if integrity < RETREAT_HULL || fuel.current_fuel < fuel.max_fuel * 0.15 {
                let msg = format!("breaking off: hull {:.0}%, fuel {:.0}", integrity * 100.0, fuel.current_fuel);
                d.log("retreat", &msg);
                d.go(Phase::Home);
                return;
            }

            let Ok((tf, vel_c)) = w.ships.single() else { return };
            let pos = tf.translation.truncate();
            let vel = vel_c.0;

            // Nearest live hostile first; that's what a contract wants dead.
            let mut best: Option<(f32, Vec2)> = None;
            for (_, etf) in w.enemies.iter() {
                let p = etf.translation.truncate();
                let dist = pos.distance(p);
                if best.map_or(true, |(b, _)| dist < b) {
                    best = Some((dist, p));
                }
            }

            if let Some((dist, tp)) = best {
                // Acquire a contact so `auto_engage` spreads the battery across
                // its silhouette instead of every barrel drilling one tile.
                // Acquire, and re-cycle if what we locked is no longer among
                // the living — a defeated hull stays an AiShip entity, so a
                // stale lock means the battery keeps drilling a corpse.
                let live_lock = selection
                    .target
                    .is_some_and(|t| w.enemies.iter().any(|(e, _)| e == t));
                if !live_lock && d.beat > 1.2 {
                    d.beat = 0.0;
                    d.tap(KeyCode::Backslash);
                }
                engage(&mut d, pos, vel, tp, dist);
            } else if d.strip_cooldown <= 0.0
                && (salvageable(&w, &d, pos).is_some_and(|(dist, _, _)| dist < 9000.0)
                    || loot_nearby(&w, pos).is_some_and(|(dist, _)| dist < 9000.0))
            {
                d.go(Phase::Strip);
            } else {
                // Nothing in sight: ping for contacts, and drift outward so
                // the sweep covers new space rather than the same empty box.
                if d.beat > 4.0 {
                    d.beat = 0.0;
                    d.tap(KeyCode::KeyZ);
                }
                let heading = Vec2::from_angle(d.run_elapsed * 0.05);
                d.aim = Some(heading);
                d.hold(KeyCode::KeyW);
                if d.phase_elapsed > 180.0 {
                    d.log("hunt", "no contacts in 3min — going home to re-board");
                    d.go(Phase::Home);
                }
            }
        }

        Phase::Strip => {
            if !flying {
                d.go(Phase::Board);
                return;
            }
            let Ok((tf, vel_c)) = w.ships.single() else { return };
            let pos = tf.translation.truncate();
            let speed = vel_c.0.length();
            let detail_out = !w.eva.is_empty();
            let dt = d.dt;

            // Freebie on the way past: E loots a derelict outright, no crew
            // errand involved. Cooldown so one POI isn't hammered every frame.
            if d.loot_cooldown <= 0.0 {
                if let Some((dist, _)) = loot_nearby(&w, pos) {
                    if dist < INTERACT_RANGE {
                        d.loot_cooldown = 3.0;
                        d.tap(KeyCode::KeyE);
                    }
                }
            }

            let Some((dist, wp, wreck_entity)) = salvageable(&w, &d, pos) else {
                // No wreck. Chase a lootable POI if one is close, else hunt.
                match loot_nearby(&w, pos) {
                    Some((d2, p2)) if d2 < 8000.0 => {
                        fly_to(&mut d, pos, p2, d2, INTERACT_RANGE * 0.7)
                    }
                    _ => d.go(Phase::Hunt),
                }
                return;
            };

            if !d.detail_sent {
                // Close, then STOP, then dispatch once. Sending a detail while
                // still carrying speed just drags them off the wreck.
                if dist > SALVAGE_HOLD {
                    fly_to(&mut d, pos, wp, dist, SALVAGE_HOLD);
                } else if speed > 60.0 {
                    d.hold(KeyCode::ShiftLeft);
                } else {
                    d.tap(KeyCode::KeyF);
                    d.detail_sent = true;
                    d.strip_wait = 0.0;
                    d.log("salvage", "detail dispatched");
                }
            } else {
                // Detail is out. Do NOT touch F again — that recalls them.
                // The only job now is holding station inside the leash.
                if dist > SALVAGE_LEASH {
                    fly_to(&mut d, pos, wp, dist, SALVAGE_HOLD);
                } else if speed > 40.0 {
                    d.hold(KeyCode::ShiftLeft);
                }

                d.strip_wait += dt;
                if detail_out {
                    d.detail_seen = true;
                } else if d.detail_seen {
                    d.log("salvage", "detail back aboard");
                    d.go(Phase::Hunt);
                    return;
                } else if d.strip_wait > 6.0 {
                    // F was pressed but nobody went out — no idle crew, or the
                    // wreck was already stripped. Write it off so Hunt doesn't
                    // send us straight back to it.
                    d.spent_wrecks.insert(wreck_entity);
                    d.strip_cooldown = 8.0;
                    d.log("salvage", "no detail launched — writing this wreck off");
                    d.go(Phase::Hunt);
                    return;
                }

                if d.strip_wait > SALVAGE_TIMEOUT {
                    d.spent_wrecks.insert(wreck_entity);
                    d.strip_cooldown = 8.0;
                    d.log("salvage", "timed out on this wreck");
                    d.go(Phase::Hunt);
                }
            }
        }

        Phase::Home => {
            if at_station {
                d.log("dock", "docked — turning in");
                d.go(Phase::Board);
                return;
            }
            if !flying {
                return;
            }
            let Ok((tf, _vel)) = w.ships.single() else { return };
            let pos = tf.translation.truncate();

            if !head_for_dock(&mut d, &stations, pos, !w.eva.is_empty()) {
                // No station loaded here — this is where an interstellar hop
                // would go once warp routing is wired in.
                d.log("home", "no station in this system");
                d.go(Phase::Hunt);
            }
        }

        Phase::Done | Phase::Aborted => {}
    }

    // Pause/menu states shouldn't count against the stall watchdog.
    if flying || at_station {
        d.stall_timer += 1.0 / 60.0;
    }
    if d.stall_timer > STALL_SECONDS {
        d.stall_timer = 0.0;
        let msg = format!("no credit movement in {:.0}s during {:?}", STALL_SECONDS, d.phase);
        d.log("stall", &msg);
        d.go(Phase::Home);
    }

    let _ = &mut next_state;
}

/// Point at `tp` and manage throttle so we close to `hold` and sit there.
fn fly_to(d: &mut Director, pos: Vec2, tp: Vec2, dist: f32, hold: f32) {
    let delta = tp - pos;
    if delta.length_squared() > 1.0 {
        d.aim = Some(delta.normalize());
    }
    if dist > hold * 1.2 {
        d.hold(KeyCode::KeyW);
    } else if dist < hold * 0.6 {
        d.hold(KeyCode::KeyS);
    }
}

/// Fight at weapon range: hold `STANDOFF`, retro-thrust rather than coast into
/// contact, drift sideways so we aren't a stationary target, and keep the
/// trigger down the whole time we're inside `FIRE_RANGE`.
fn engage(d: &mut Director, pos: Vec2, vel: Vec2, tp: Vec2, dist: f32) {
    let delta = tp - pos;
    if delta.length_squared() > 1.0 {
        d.aim = Some(delta.normalize());
    }

    // Closing speed along the line to the target — positive means we're
    // gaining on it.
    let closing = if delta.length_squared() > 1.0 {
        vel.dot(delta.normalize())
    } else {
        0.0
    };

    if dist > STANDOFF * 1.15 {
        // Still far: burn in, but stop accelerating once we're already fast
        // enough that we'd sail straight past.
        if closing < CLOSING_SPEED_CAP * 2.5 {
            d.hold(KeyCode::KeyW);
        }
    } else if dist < STANDOFF * 0.7 {
        d.hold(KeyCode::KeyS);
    } else if closing > CLOSING_SPEED_CAP {
        // In the pocket but still drifting in — kill the closure.
        d.hold(KeyCode::ShiftLeft);
    }

    // Orbit slowly so we present a moving target instead of a parked one.
    if dist < STANDOFF * 1.6 {
        if (d.run_elapsed as i32 / 6) % 2 == 0 {
            d.hold(KeyCode::KeyA);
        } else {
            d.hold(KeyCode::KeyD);
        }
    }

    if dist < FIRE_RANGE {
        d.hold(KeyCode::Space);
    }
}

/// Fly to the nearest station and press F only once actually in range.
///
/// This has to be one shared routine: out in open space F does NOT dock, it
/// toggles an EVA salvage detail. The first version tapped F on the spot for
/// the tutorial's dock step and spent two minutes dispatching and recalling
/// the same seven crew instead of ever flying home.
fn head_for_dock(
    d: &mut Director,
    stations: &SystemStations,
    pos: Vec2,
    crew_out: bool,
) -> bool {
    let Some(site) = stations.sites.iter().min_by(|a, b| {
        pos.distance(a.pos)
            .partial_cmp(&pos.distance(b.pos))
            .unwrap_or(std::cmp::Ordering::Equal)
    }) else {
        return false;
    };

    let dist = pos.distance(site.pos);
    fly_to(d, pos, site.pos, dist, 400.0);

    // Only inside the real docking radius, and on a slow beat so we aren't
    // mashing a key that means something else the moment we drift out.
    // F is overloaded: at a station it docks, and within eva_salvage's
    // ORDER_RANGE of a wreck it ALSO throws a detail out. One press did both
    // in an earlier run, docking the ship with nineteen crew on the hull.
    if dist < DOCK_RANGE * 0.9 && d.beat > 1.0 && !crew_out {
        d.beat = 0.0;
        d.tap(KeyCode::KeyF);
    }
    true
}

fn nearest(points: impl Iterator<Item = Vec2>, from: Vec2) -> Option<(f32, Vec2)> {
    points.fold(None, |best: Option<(f32, Vec2)>, p| {
        let dist = from.distance(p);
        match best {
            Some((b, _)) if b <= dist => best,
            _ => Some((dist, p)),
        }
    })
}

/// Nearest wreck that still has something in it and hasn't already refused us.
fn salvageable(w: &World1, d: &Director, pos: Vec2) -> Option<(f32, Vec2, Entity)> {
    w.wrecks
        .iter()
        .filter(|(e, _, wreck)| wreck.loot_remaining > 0 && !d.spent_wrecks.contains(e))
        .map(|(e, t, _)| {
            let p = t.translation.truncate();
            (pos.distance(p), p, e)
        })
        .fold(None, |best: Option<(f32, Vec2, Entity)>, cur| match best {
            Some((b, _, _)) if b <= cur.0 => best,
            _ => Some(cur),
        })
}

fn loot_nearby(w: &World1, pos: Vec2) -> Option<(f32, Vec2)> {
    nearest(
        w.pois
            .iter()
            .filter(|(_, poi)| !poi.looted)
            .map(|(t, _)| t.translation.truncate()),
        pos,
    )
}

/// Flight training reads real keys, so the director answers each step with the
/// key that step is waiting for.
fn drive_tutorial(
    d: &mut Director,
    step: Advance,
    w: &World1,
    stations: &SystemStations,
    flying: bool,
) {
    // Info steps just want [Space], but on a human rhythm — one press per
    // beat, not one per frame.
    let beat_ready = d.beat > 0.9;
    match step {
        Advance::Launch => {
            if beat_ready {
                d.beat = 0.0;
                d.tap(KeyCode::Enter);
            }
        }
        Advance::Continue => {
            if beat_ready {
                d.beat = 0.0;
                d.tap(KeyCode::Space);
            }
        }
        Advance::Thrust => {
            d.hold(KeyCode::KeyW);
        }
        Advance::Scan => {
            if beat_ready {
                d.beat = 0.0;
                d.tap(KeyCode::KeyZ);
            }
        }
        Advance::Kill => {
            if !flying {
                return;
            }
            let Ok((tf, vel_c)) = w.ships.single() else { return };
            let pos = tf.translation.truncate();
            let vel = vel_c.0;
            if let Some((dist, tp)) = nearest(w.enemies.iter().map(|(_, t)| t.translation.truncate()), pos) {
                // The training raider is clamped to 700 range, so this is safe
                // to do from well outside its reach.
                engage(d, pos, vel, tp, dist);
            } else if beat_ready {
                d.beat = 0.0;
                d.tap(KeyCode::KeyZ);
            }
        }
        Advance::Salvage => {
            if !flying {
                return;
            }
            let Ok((tf, vel_c)) = w.ships.single() else { return };
            let pos = tf.translation.truncate();
            // The step completes the moment a detail is actually out, so once
            // anyone is on the hull there is nothing left to do here.
            if !w.eva.is_empty() {
                return;
            }
            // One dispatch — but a press that produced no detail must not latch
            // forever. It wedged a full run on this step for four minutes:
            // detail_sent stayed true, nobody was outside, and the branch
            // returned early on every frame from then on.
            if d.detail_sent {
                d.strip_wait += d.dt;
                if d.strip_wait < 5.0 {
                    return;
                }
                d.log("salvage", "training dispatch did not take — retrying");
                d.detail_sent = false;
                d.strip_wait = 0.0;
            }
            if let Some((dist, tp, _)) = salvageable(w, d, pos) {
                if dist > SALVAGE_HOLD {
                    fly_to(d, pos, tp, dist, SALVAGE_HOLD);
                } else if vel_c.0.length() > 60.0 {
                    d.hold(KeyCode::ShiftLeft);
                } else {
                    d.tap(KeyCode::KeyF);
                    d.detail_sent = true;
                    d.log("salvage", "training detail dispatched");
                }
            }
        }
        Advance::Dock => {
            if !flying {
                return;
            }
            let Ok((tf, _vel)) = w.ships.single() else { return };
            head_for_dock(d, stations, tf.translation.truncate(), !w.eva.is_empty());
        }
        Advance::Build => {
            if beat_ready {
                d.beat = 0.0;
                d.tap(KeyCode::KeyB);
            }
        }
        Advance::Contracts => {
            if beat_ready {
                d.beat = 0.0;
                d.tap(KeyCode::KeyJ);
            }
        }
        Advance::Crew => {
            if beat_ready {
                d.beat = 0.0;
                d.tap(KeyCode::KeyC);
            }
        }
    }
}

// ============================================================================
// TIME DILATION
// ============================================================================

/// Empty transits are the boring part of a two-hour run, so they run fast;
/// anything with a hostile, a wreck, or a menu on screen drops back to 1x so
/// what the run finds is what a player would have seen.
fn director_timescale(
    d: Res<Director>,
    mut virtual_time: ResMut<Time<Virtual>>,
    enemies: Query<(), With<AiShip>>,
    ships: Query<&Transform, With<Ship>>,
    board_open: Res<MissionBoardOpen>,
) {
    if d.max_speed <= 1.0 {
        return;
    }
    let contact = !enemies.is_empty();
    let busy = matches!(
        d.phase,
        Phase::Boot | Phase::Training | Phase::Outfit | Phase::Board | Phase::Strip
    ) || board_open.0;

    let speed = if contact || busy {
        1.0
    } else if ships.single().is_ok() {
        d.max_speed
    } else {
        1.0
    };

    if (virtual_time.relative_speed() - speed).abs() > 0.01 {
        virtual_time.set_relative_speed(speed);
    }
}

// ============================================================================
// OBSERVATION — screenshots + everything the game told the player
// ============================================================================

/// Captures that came back empty. macOS stops updating an occluded window's
/// surface, so any frame taken while the game is behind another window is
/// solid black — 57KB of nothing. Counting them lets the director retry
/// instead of filling the run directory with blanks it will never look at.
static BLANK_CAPTURES: AtomicU32 = AtomicU32::new(0);
static SAVED_CAPTURES: AtomicU32 = AtomicU32::new(0);

/// Save a capture only if it actually contains an image.
///
/// Replaces bevy's `save_to_disk`, which writes whatever it is handed. Samples
/// a sparse grid of pixels rather than the whole frame — a real frame lights
/// up within the first handful of samples, and a blank one costs a few hundred
/// comparisons to rule out.
fn save_if_legible(path: PathBuf) -> impl FnMut(On<ScreenshotCaptured>) {
    move |captured| {
        let Ok(dyn_img) = captured.image.clone().try_into_dynamic() else {
            return;
        };
        let rgb = dyn_img.to_rgb8();

        let mut lit = 0u32;
        for px in rgb.pixels().step_by(101) {
            if px.0[0] as u32 + px.0[1] as u32 + px.0[2] as u32 > 24 {
                lit += 1;
                if lit >= 8 {
                    break;
                }
            }
        }

        if lit < 8 {
            BLANK_CAPTURES.fetch_add(1, Ordering::Relaxed);
            return;
        }

        match rgb.save(&path) {
            Ok(()) => {
                SAVED_CAPTURES.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => error!("[AUTOPLAY] could not save {}: {e}", path.display()),
        }
    }
}

/// A heartbeat line every 30s. Staffing is on it because a gun with nobody on
/// it does not fire (combat::weapon_is_crewed) — a run that quietly stops
/// shooting looks identical in the journal to one that never found a target,
/// and this is what tells the two apart.
fn director_status(
    mut d: ResMut<Director>,
    currency: Res<Currency>,
    hull: Res<HullState>,
    fuel: Res<FuelState>,
    staffing: Res<StaffingState>,
    tutorial: Res<Tutorial>,
) {
    d.status_in -= d.dt;
    if d.status_in > 0.0 {
        return;
    }
    d.status_in = 30.0;

    let msg = format!(
        "{}c | hull {:.0}% | fuel {:.0} | crew {}/{} berths | {}/{} posts manned | {} kills{}",
        currency.credits,
        hull.hull_integrity * 100.0,
        fuel.current_fuel,
        staffing.total_crew,
        staffing.total_berths,
        staffing.staffed_stations,
        staffing.total_stations,
        d.kills,
        match tutorial.pending() {
            Some(step) => format!(" | training: {:?}", step),
            None => String::new(),
        },
    );
    d.log("status", &msg);
}

fn director_watch(
    mut commands: Commands,
    mut d: ResMut<Director>,
    mut notifications: MessageReader<ShowNotification>,
    mut deaths: MessageReader<AiShipDestroyed>,
    mut last_phase: Local<Option<Phase>>,
) {
    let mut want_shot = false;

    // Every hostile that stopped fighting, and how. `cause` is the interesting
    // half — Struck vs Meltdown vs Gutted says which of the combat rules
    // actually decided the fight.
    let kills: Vec<String> = deaths
        .read()
        .map(|k| format!("{:?} killed ({:?})", k.ship_type, k.cause))
        .collect();
    for kill in kills {
        d.kills += 1;
        d.log("kill", &kill);
        want_shot = true;
    }

    // A phase change is always worth a frame.
    if *last_phase != Some(d.phase) {
        *last_phase = Some(d.phase);
        want_shot = true;
    }

    // The notification stream is the game narrating itself — log all of it,
    // and photograph the ones that mean something went right or wrong.
    let notes: Vec<(String, NotificationType)> = notifications
        .read()
        .map(|n| (n.message.clone(), n.notification_type))
        .collect();
    for (message, kind) in notes {
        let tag = match kind {
            NotificationType::Danger => "danger",
            NotificationType::Warning => "warning",
            NotificationType::Success => "success",
            NotificationType::Info => "info",
        };
        d.log(tag, &message);
        if matches!(kind, NotificationType::Danger | NotificationType::Success) {
            want_shot = true;
        }
    }

    // A blank capture means the window was behind something. Keep the request
    // alive and retake on a short cycle so the shot lands as soon as the game
    // is visible again, rather than losing the moment entirely.
    let blanks = BLANK_CAPTURES.load(Ordering::Relaxed);
    if blanks != d.blanks_seen {
        d.blanks_seen = blanks;
        if d.shot_retries < 40 {
            d.shot_pending = true;
            d.shot_retries += 1;
            d.shot_cooldown = 2.0;
        } else if d.shot_pending {
            d.shot_pending = false;
            d.log("capture", "gave up on a shot — window occluded too long");
        }
    }

    if (want_shot || d.shot_pending) && d.shot_cooldown <= 0.0 {
        if want_shot {
            d.shot_retries = 0;
        }
        d.shot_cooldown = SHOT_COOLDOWN;
        d.shot_pending = false;
        let path = PathBuf::from(format!("{}/shot_{:04}_{:?}.png", d.dir, d.shots, d.phase));
        d.shots += 1;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_if_legible(path));
    }
}
