//! The numbers behind a gun built from parts.
//!
//! These are the formulas the Gun Deck preview shows
//! (tools/gun-deck/index.html, "How the numbers work"), kept as plain
//! functions so the game, the preview and the tests agree on one set of
//! rules. Units are real ones: millimetres of gauge, metres of barrel and
//! shell (1 cell = 1 m), metres per second, seconds.

/// What a gun is built from, as counts. `gun_assembly` reads these off the
/// blocks around a firing piece; the preview reads them off its steppers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GunParts {
    pub gauge_increasers: u32,
    /// Each takes 20 mm off the bore, down to `MIN_GAUGE_MM`.
    pub gauge_reducers: u32,
    /// One entry per autoloader: its length in cells (= metres of shell it
    /// takes) and how many clips' worth of rack sit beside it.
    pub loaders: Vec<Loader>,
    pub barrel_m: u32,
    pub coolers: u32,
    pub absorbers: u32,
    pub rail_magnets: u32,
    pub muzzle_brake: bool,
    /// The shell it fires (combat::shell).
    pub shell: super::shell::ShellDesign,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Loader {
    pub length_m: u32,
    /// Clip cells beside this loader divided by its length: 2.0 is two full
    /// clips. Fractional racks count fractionally.
    pub clips: f32,
}

/// Everything the gun does, worked out from its parts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GunStats {
    pub gauge_mm: f32,
    /// Shell length: a gauge per module, half a gauge per casing.
    pub shell_m: f32,
    /// Barrel needed to burn the propellant fully.
    pub burn_m: f32,
    pub velocity_mps: f32,
    pub damage: f32,
    pub penetration_mm: f32,
    /// Loaders long enough for the shell. A loader too short for it can't
    /// load anything at all.
    pub working_loaders: u32,
    /// Seconds one loader takes per shell (its own clips speed it up).
    pub reload_s: f32,
    /// Shots a minute the loaders could manage together.
    pub rpm_loaders: f32,
    /// Shots a minute the coolers can keep up with.
    pub rpm_heat: f32,
    /// What the gun actually does: the slower of the two.
    pub rpm: f32,
    pub heat_per_shot: f32,
    pub cooling_per_s: f32,
    pub recoil: f32,
    pub spread_deg: f32,
    /// Shells held ready: one in each loader, three per clip.
    pub ready_rack: u32,
    pub rail_power_mw: f32,
    /// Range as a multiple of what the velocity alone gives (a base bleeder
    /// carries further).
    pub range_mult: f32,
}

pub const BASE_GAUGE_MM: f32 = 60.0;
pub const GAUGE_PER_INCREASER_MM: f32 = 55.0;
pub const MAX_GAUGE_INCREASERS: u32 = 8;
pub const MAX_GAUGE_MM: f32 = 500.0;
pub const GAUGE_PER_REDUCER_MM: f32 = 20.0;
/// The smallest bore: a 20 mm point defence gun.
pub const MIN_GAUGE_MM: f32 = 20.0;
/// A shell shorter than this many gauges tumbles: less accurate.
pub const STUBBY_SHELL_GAUGES: f32 = 3.0;
pub const FULL_BURN_VELOCITY: f32 = 850.0;
pub const RAIL_VELOCITY_PER_MAGNET: f32 = 90.0;
pub const RAIL_POWER_PER_MAGNET_MW: f32 = 1.5;
pub const SHELLS_PER_CLIP: u32 = 3;

pub fn gauge_mm(increasers: u32, reducers: u32) -> f32 {
    (BASE_GAUGE_MM + GAUGE_PER_INCREASER_MM * increasers.min(MAX_GAUGE_INCREASERS) as f32 - GAUGE_PER_REDUCER_MM * reducers as f32)
        .clamp(MIN_GAUGE_MM, MAX_GAUGE_MM)
}

/// What a big bore gives up and gets, against a 60 mm autocannon: how many
/// times longer between shots, and how many times the damage in each. As in
/// From the Depths, a battleship's gun fires rarely and every shell matters: a
/// 170 mm gun is about 8 rpm for 4.3 times the shell, and a 500 mm gun a shot
/// every couple of minutes per loader for twenty times. Damage outruns the
/// wait, so the big gun is the better one, if you can afford to miss.
pub fn heavy_gun(gauge_mm: f32) -> (f32, f32) {
    let heft = (gauge_mm / MIN_GAUGE_FOR_HEFT).max(1.0);
    (heft.powf(1.26).min(MAX_SLOWER), heft.powf(1.4))
}
const MIN_GAUGE_FOR_HEFT: f32 = 60.0;
const MAX_SLOWER: f32 = 6.0;

pub fn stats(parts: &GunParts) -> GunStats {
    use super::shell::{ShellBase, ShellHead, HEAT_JET_DAMAGE, HEAT_PEN_PER_MM};
    let shell = &parts.shell;
    let gauge = gauge_mm(parts.gauge_increasers, parts.gauge_reducers);
    let g = gauge / 100.0;
    let shell_m = gauge * shell.length_gauges() / 1000.0;
    // Each casing needs 0.5 m of barrel per 100 mm of gauge to burn.
    let burn_m = 0.5 * g * shell.casings as f32;
    let burn_frac = if burn_m > 0.0 { (parts.barrel_m as f32 / burn_m).min(1.0) } else { 1.0 };
    let velocity = shell.full_burn_velocity() * burn_frac.sqrt() + RAIL_VELOCITY_PER_MAGNET * parts.rail_magnets as f32;
    let speed = velocity / FULL_BURN_VELOCITY;
    // Kinetic damage goes with the hard metal in the shell (2 for the
    // default AP head + solid body); a HEAT jet adds its own, at any speed.
    let mut damage = 30.0 * g * g * speed * shell.kinetic_weight() / 2.0;
    let (slower, harder) = heavy_gun(gauge);
    let penetration = if shell.head == ShellHead::Heat {
        damage += HEAT_JET_DAMAGE * g * g;
        gauge * HEAT_PEN_PER_MM
    } else {
        gauge * 1.1 * speed * shell.penetration_factor()
    };
    let damage = damage * harder;

    // A loader must be long enough for the shell; each one works in parallel,
    // sped up 25% per clip of rack beside it.
    let base_reload = 2.4 * g.powf(1.35) * shell_m * slower;
    let mut rpm_loaders = 0.0;
    let mut working = 0;
    let mut ready = 0;
    let mut fastest = f32::INFINITY;
    for l in &parts.loaders {
        if (l.length_m as f32) + 1e-4 < shell_m {
            continue;
        }
        let reload = base_reload / (1.0 + 0.25 * l.clips);
        fastest = fastest.min(reload);
        rpm_loaders += 60.0 / reload;
        working += 1;
        ready += 1 + (SHELLS_PER_CLIP as f32 * l.clips).floor() as u32;
    }
    // Propellant is the heat: the default shell's two casings make 0.6 g^1.5.
    let heat_per_shot = g.powf(1.5) * 0.3 * shell.casings as f32 * slower;
    let cooling = 0.2 + 0.6 * parts.coolers as f32;
    let rpm_heat = 60.0 * cooling / heat_per_shot;
    let rpm = rpm_loaders.min(rpm_heat);
    let recoil = g.powf(1.5) * speed * 0.3 * shell.projectile_modules() / 2.0 * if parts.muzzle_brake { 0.5 } else { 1.0 };
    let spread = (1.6 / (1.0 + 0.5 * parts.barrel_m as f32)
        + 0.25 * (recoil - parts.absorbers as f32).max(0.0)
        + if parts.muzzle_brake { 0.1 } else { 0.0 }
        // A stubby shell tumbles.
        + 0.5 * (STUBBY_SHELL_GAUGES - shell.length_gauges()).max(0.0))
        * if shell.base == ShellBase::Tracer { 0.75 } else { 1.0 };

    GunStats {
        gauge_mm: gauge,
        shell_m,
        burn_m,
        velocity_mps: velocity,
        damage,
        penetration_mm: penetration,
        working_loaders: working,
        reload_s: if fastest.is_finite() { fastest } else { 0.0 },
        rpm_loaders,
        rpm_heat,
        rpm,
        heat_per_shot,
        cooling_per_s: cooling,
        recoil,
        spread_deg: spread,
        ready_rack: ready,
        rail_power_mw: RAIL_POWER_PER_MAGNET_MW * parts.rail_magnets as f32,
        range_mult: if shell.base == ShellBase::Bleeder { 1.2 } else { 1.0 },
    }
}

/// Fastest a turret turns, degrees a second: a light point defence mount
/// on two motors gets here.
pub const MAX_TRAVERSE_DEG_S: f32 = 180.0;

/// Degrees a second one traverse motor turns a tonne of turret. Was 260: a
/// player's 170 mm main battery turned at 13-26 deg/s and never caught up
/// with the cursor while the ship swung.
const TRAVERSE_PER_MOTOR: f32 = 400.0;

/// Turn speed of a turret, degrees a second: its traverse motors against
/// everything standing on it.
pub fn traverse_deg_per_s(motors: u32, turret_mass_t: f32) -> f32 {
    if turret_mass_t <= 0.0 {
        return MAX_TRAVERSE_DEG_S;
    }
    (TRAVERSE_PER_MOTOR * motors as f32 / turret_mass_t).clamp(2.0, MAX_TRAVERSE_DEG_S)
}

#[cfg(test)]
mod gun_math_tests {
    use super::*;

    fn spinal() -> GunParts {
        // The Gun Deck preview's spinal gun.
        GunParts {
            gauge_increasers: 8,
            gauge_reducers: 0,
            loaders: vec![Loader { length_m: 2, clips: 2.0 }; 4],
            barrel_m: 10,
            coolers: 6,
            absorbers: 3,
            rail_magnets: 2,
            muzzle_brake: false,
            shell: Default::default(),
        }
    }

    #[test]
    fn a_bigger_gun_fires_slower_and_hits_far_harder() {
        // Small bores are left as they were.
        assert_eq!(heavy_gun(60.0), (1.0, 1.0));
        assert_eq!(heavy_gun(20.0), (1.0, 1.0));
        let (slower, harder) = heavy_gun(170.0);
        assert!((3.0..4.5).contains(&slower), "{slower}");
        assert!((4.0..5.0).contains(&harder), "{harder}");
        // At every size up the shell gains more than the wait costs.
        let mut last = 1.0;
        for gauge in [100.0, 170.0, 280.0, 400.0, 500.0] {
            let (slower, harder) = heavy_gun(gauge);
            let per_second = harder / slower;
            assert!(per_second >= last, "{gauge}: {per_second}");
            last = per_second;
        }
    }

    #[test]
    fn the_preview_spinal_gun_comes_out_as_the_preview_says() {
        let s = stats(&spinal());
        assert_eq!(s.gauge_mm, 500.0);
        assert!((s.shell_m - 1.5).abs() < 1e-5);
        assert!((s.velocity_mps - 1030.0).abs() < 0.5);
        // 21 s and 11.4 rpm by the loaders alone, then six times as slow.
        assert!((s.reload_s - 21.05 * 6.0).abs() < 0.6, "{}", s.reload_s);
        assert!((s.rpm - 11.4 / 6.0).abs() < 0.1, "{}", s.rpm);
        assert_eq!(s.working_loaders, 4);
        assert_eq!(s.ready_rack, 4 * 7);
        assert_eq!(s.rail_power_mw, 3.0);
    }

    #[test]
    fn gauge_caps_at_eight_increasers_and_500_mm() {
        assert_eq!(gauge_mm(0, 0), 60.0);
        assert_eq!(gauge_mm(2, 0), 170.0);
        assert_eq!(gauge_mm(8, 0), 500.0);
        assert_eq!(gauge_mm(20, 0), 500.0);
        assert_eq!(gauge_mm(0, 1), 40.0);
        assert_eq!(gauge_mm(0, 2), 20.0);
        assert_eq!(gauge_mm(0, 5), 20.0);
    }

    #[test]
    fn a_loader_too_short_for_the_shell_loads_nothing() {
        let mut p = spinal();
        p.loaders = vec![Loader { length_m: 1, clips: 0.0 }];
        let s = stats(&p);
        assert_eq!(s.working_loaders, 0);
        assert_eq!(s.rpm, 0.0);
    }

    #[test]
    fn a_short_barrel_costs_velocity_and_damage() {
        let mut p = spinal();
        p.rail_magnets = 0;
        let full = stats(&p);
        p.barrel_m = 1; // a fifth of the 5 m burn
        let short = stats(&p);
        assert!((short.velocity_mps - 850.0 * 0.2f32.sqrt()).abs() < 0.5);
        assert!(short.damage < full.damage);
    }

    #[test]
    fn coolers_raise_the_heat_ceiling_not_the_loaders() {
        let mut p = spinal();
        p.coolers = 0;
        let hot = stats(&p);
        assert!(hot.rpm_heat < hot.rpm_loaders, "with no coolers heat sets the pace");
        assert_eq!(hot.rpm, hot.rpm_heat);
    }

    #[test]
    fn more_casings_need_more_barrel() {
        use crate::combat::shell::ShellDesign;
        let mut p = spinal();
        p.rail_magnets = 0;
        p.barrel_m = 5;
        let two = stats(&p);
        p.shell = ShellDesign { casings: 4, ..Default::default() };
        let four_short = stats(&p);
        assert_eq!(four_short.burn_m, 10.0);
        p.barrel_m = 10;
        let four_long = stats(&p);
        assert!(four_long.velocity_mps > two.velocity_mps, "a full burn of more propellant is faster");
        assert!(four_short.velocity_mps < four_long.velocity_mps, "half burnt, it's slower");
        assert!(four_long.heat_per_shot > two.heat_per_shot);
        assert!(four_long.shell_m > two.shell_m, "and the shell's longer");
    }

    #[test]
    fn a_heat_jet_goes_as_deep_at_any_speed() {
        use crate::combat::shell::{ShellDesign, ShellHead};
        let mut p = spinal();
        p.shell = ShellDesign { head: ShellHead::Heat, ..Default::default() };
        p.rail_magnets = 0;
        let fast = stats(&p);
        p.barrel_m = 1;
        let slow = stats(&p);
        assert_eq!(fast.penetration_mm, slow.penetration_mm);
        assert!(slow.damage > 0.0);
    }

    #[test]
    fn stubby_shells_scatter_and_tracers_steady_them() {
        use crate::combat::shell::{ShellBase, ShellDesign};
        let p = spinal();
        let normal = stats(&p).spread_deg;
        let mut stub = p.clone();
        stub.shell = ShellDesign { casings: 1, bodies: vec![], ..Default::default() };
        assert!(stats(&stub).spread_deg > normal);
        let mut traced = p.clone();
        traced.shell.base = ShellBase::Tracer;
        assert!(stats(&traced).spread_deg < normal);
        let mut bled = p.clone();
        bled.shell.base = ShellBase::Bleeder;
        assert!(stats(&bled).range_mult > 1.0);
    }

    #[test]
    fn heavier_turrets_turn_slower_and_motors_help() {
        assert!(traverse_deg_per_s(2, 80.0) < traverse_deg_per_s(2, 40.0));
        assert!(traverse_deg_per_s(4, 80.0) > traverse_deg_per_s(2, 80.0));
        assert_eq!(traverse_deg_per_s(1, 10_000.0), 2.0);
        assert_eq!(traverse_deg_per_s(10, 1.0), MAX_TRAVERSE_DEG_S);
    }

    /// Two reducers make a 20 mm point defence gun: a tiny shell, a dozen
    /// and more shots a second, against 4-5 for the old 60 mm flak.
    #[test]
    fn reducers_make_a_small_fast_gun() {
        use crate::combat::shell::{ShellBase, ShellBody, ShellDesign, ShellHead};
        let flak = ShellDesign { base: ShellBase::None, casings: 2, bodies: vec![ShellBody::Frag, ShellBody::ProximityFuse], head: ShellHead::Frag };
        let gun = |reducers| GunParts {
            gauge_reducers: reducers,
            loaders: vec![Loader { length_m: 1, clips: 1.0 }],
            barrel_m: 2,
            coolers: 1,
            shell: flak.clone(),
            ..Default::default()
        };
        let small = stats(&gun(2));
        let old = stats(&gun(0));
        assert_eq!(small.gauge_mm, 20.0);
        assert!(small.shell_m < old.shell_m / 2.0);
        assert!(small.rpm > 12.0 * 60.0, "{} rpm", small.rpm);
        assert!(small.rpm > old.rpm * 2.0);
    }
}
