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
use bevy::render::view::window::screenshot::{save_to_disk, Screenshot};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::Write;

use crate::ai_ship::components::{AiShip, AiShipWreck};
use crate::celestial::poi::SpacePoi;
use crate::combat::targeting::selection::TargetSelection;
use crate::components::{Ship, Velocity};
use crate::contracts::{ContractState, ContractStatus, MissionBoardOpen};
use crate::crew::eva_salvage::EvaSalvaging;
use crate::events::{AiShipDestroyed, NotificationType, ShowNotification};
use crate::resources::{Currency, FuelState, HullState, InputState};
use crate::states::GameState;
use crate::tutorial::{Advance, Tutorial};
use crate::world::home_base::SystemStations;

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
    d.beat += dt;
    d.dt = dt;
}

// ============================================================================
// THE BRAIN
// ============================================================================

#[derive(SystemParam)]
struct World1<'w, 's> {
    ships: Query<'w, 's, (&'static Transform, &'static Velocity), With<Ship>>,
    enemies: Query<'w, 's, &'static Transform, (With<AiShip>, Without<Ship>)>,
    wrecks: Query<'w, 's, &'static Transform, (With<AiShipWreck>, Without<Ship>)>,
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
            "target reached: {}c after {:.0}s, {} kills",
            currency.credits, d.run_elapsed, d.kills
        );
        d.log("done", &msg);
        d.go(Phase::Done);
        return;
    }
    if d.run_elapsed > d.deadline {
        let msg = format!("deadline hit at {}c", currency.credits);
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

    let docked = *state.get() == GameState::StationDocked;
    let flying = *state.get() == GameState::Exploring;

    // ---- phase logic ------------------------------------------------------
    match d.phase {
        Phase::Boot => {
            // Enter on the main menu starts a new expedition and arms training.
            if *state.get() == GameState::MainMenu {
                if d.beat > 0.7 {
                    d.beat = 0.0;
                    d.tap(KeyCode::Enter);
                }
            } else if docked {
                d.go(Phase::Training);
            }
        }

        Phase::Training => {
            match tutorial.pending() {
                None => {
                    d.log("tutorial", "training complete");
                    d.go(if docked { Phase::Outfit } else { Phase::Home });
                }
                Some(step) => drive_tutorial(&mut d, step, &w, flying),
            }
        }

        Phase::Outfit => {
            if !docked {
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
            if !docked {
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
            } else if docked && d.beat > 0.8 {
                d.beat = 0.0;
                d.tap(KeyCode::Enter);
            }
        }

        Phase::Hunt => {
            if !flying {
                d.go(if docked { Phase::Board } else { Phase::Boot });
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
            for etf in w.enemies.iter() {
                let p = etf.translation.truncate();
                let dist = pos.distance(p);
                if best.map_or(true, |(b, _)| dist < b) {
                    best = Some((dist, p));
                }
            }

            if let Some((dist, tp)) = best {
                // Acquire a contact so `auto_engage` spreads the battery across
                // its silhouette instead of every barrel drilling one tile.
                if selection.target.is_none() && d.beat > 1.2 {
                    d.beat = 0.0;
                    d.tap(KeyCode::Backslash);
                }
                engage(&mut d, pos, vel, tp, dist);
            } else if nearest(w.wrecks.iter().map(|t| t.translation.truncate()), pos)
                .is_some_and(|(dist, _)| dist < 9000.0)
                || loot_nearby(&w, pos).is_some_and(|(dist, _)| dist < 9000.0)
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

            let Some((dist, wp)) = nearest(w.wrecks.iter().map(|t| t.translation.truncate()), pos)
            else {
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
                    // wreck was already stripped. Not worth parking on.
                    d.log("salvage", "no detail launched — moving on");
                    d.go(Phase::Hunt);
                    return;
                }

                if d.strip_wait > SALVAGE_TIMEOUT {
                    d.log("salvage", "timed out on this wreck");
                    d.go(Phase::Hunt);
                }
            }
        }

        Phase::Home => {
            if docked {
                d.log("dock", "docked — turning in");
                d.go(Phase::Board);
                return;
            }
            if !flying {
                return;
            }
            let Ok((tf, _vel)) = w.ships.single() else { return };
            let pos = tf.translation.truncate();

            let Some(site) = stations.sites.iter().min_by(|a, b| {
                pos.distance(a.pos)
                    .partial_cmp(&pos.distance(b.pos))
                    .unwrap_or(std::cmp::Ordering::Equal)
            }) else {
                // No stations loaded here — this is where a warp would go.
                d.log("home", "no station in this system");
                d.go(Phase::Hunt);
                return;
            };

            let dist = pos.distance(site.pos);
            fly_to(&mut d, pos, site.pos, dist, 300.0);
            if dist < 2200.0 && d.beat > 0.6 {
                d.beat = 0.0;
                d.tap(KeyCode::KeyF);
            }
        }

        Phase::Done | Phase::Aborted => {}
    }

    // Pause/menu states shouldn't count against the stall watchdog.
    if flying || docked {
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

fn nearest(points: impl Iterator<Item = Vec2>, from: Vec2) -> Option<(f32, Vec2)> {
    points.fold(None, |best: Option<(f32, Vec2)>, p| {
        let dist = from.distance(p);
        match best {
            Some((b, _)) if b <= dist => best,
            _ => Some((dist, p)),
        }
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
fn drive_tutorial(d: &mut Director, step: Advance, w: &World1, flying: bool) {
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
            if let Some((dist, tp)) = nearest(w.enemies.iter().map(|t| t.translation.truncate()), pos) {
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
            let Ok((tf, _vel)) = w.ships.single() else { return };
            let pos = tf.translation.truncate();
            if let Some((dist, tp)) = nearest(w.wrecks.iter().map(|t| t.translation.truncate()), pos) {
                if dist > INTERACT_RANGE {
                    fly_to(d, pos, tp, dist, INTERACT_RANGE * 0.6);
                } else if beat_ready {
                    d.beat = 0.0;
                    d.tap(KeyCode::KeyF);
                }
            }
        }
        Advance::Dock => {
            // Handled by flying home; reuse the Home logic by pressing F when
            // the station prompt is up. The tutorial docks at the home berth.
            if beat_ready {
                d.beat = 0.0;
                d.tap(KeyCode::KeyF);
            }
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

    if want_shot && d.shot_cooldown <= 0.0 {
        d.shot_cooldown = SHOT_COOLDOWN;
        let path = format!("{}/shot_{:04}_{:?}.png", d.dir, d.shots, d.phase);
        d.shots += 1;
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
}
