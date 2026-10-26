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
    /// One entry per autoloader: its length in cells (= metres of shell it
    /// takes) and how many clips' worth of rack sit beside it.
    pub loaders: Vec<Loader>,
    pub barrel_m: u32,
    pub coolers: u32,
    pub absorbers: u32,
    pub rail_magnets: u32,
    pub muzzle_brake: bool,
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
    /// The sample shell: AP head, one solid body, two propellant casings.
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
}

pub const BASE_GAUGE_MM: f32 = 60.0;
pub const GAUGE_PER_INCREASER_MM: f32 = 55.0;
pub const MAX_GAUGE_INCREASERS: u32 = 8;
pub const MAX_GAUGE_MM: f32 = 500.0;
/// Shell length as a multiple of gauge: head + body + two half-length casings.
pub const SHELL_GAUGES: f32 = 3.0;
pub const FULL_BURN_VELOCITY: f32 = 850.0;
pub const RAIL_VELOCITY_PER_MAGNET: f32 = 90.0;
pub const RAIL_POWER_PER_MAGNET_MW: f32 = 1.5;
pub const SHELLS_PER_CLIP: u32 = 3;

pub fn gauge_mm(increasers: u32) -> f32 {
    (BASE_GAUGE_MM + GAUGE_PER_INCREASER_MM * increasers.min(MAX_GAUGE_INCREASERS) as f32).min(MAX_GAUGE_MM)
}

pub fn stats(parts: &GunParts) -> GunStats {
    let gauge = gauge_mm(parts.gauge_increasers);
    let g = gauge / 100.0;
    let shell_m = gauge * SHELL_GAUGES / 1000.0;
    // Two casings need 0.5 m of barrel per 100 mm of gauge each.
    let burn_m = g;
    let burn_frac = if burn_m > 0.0 { (parts.barrel_m as f32 / burn_m).min(1.0) } else { 1.0 };
    let velocity = FULL_BURN_VELOCITY * burn_frac.sqrt() + RAIL_VELOCITY_PER_MAGNET * parts.rail_magnets as f32;
    let speed = velocity / FULL_BURN_VELOCITY;
    let damage = 30.0 * g * g * speed;
    let penetration = gauge * 1.1 * speed;

    // A loader must be long enough for the shell; each one works in parallel,
    // sped up 25% per clip of rack beside it.
    let base_reload = 2.4 * g.powf(1.35) * shell_m;
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
    let heat_per_shot = g.powf(1.5) * 0.6;
    let cooling = 0.2 + 0.6 * parts.coolers as f32;
    let rpm_heat = 60.0 * cooling / heat_per_shot;
    let rpm = rpm_loaders.min(rpm_heat);
    let recoil = g.powf(1.5) * speed * 0.3 * if parts.muzzle_brake { 0.5 } else { 1.0 };
    let spread = 1.6 / (1.0 + 0.5 * parts.barrel_m as f32)
        + 0.25 * (recoil - parts.absorbers as f32).max(0.0)
        + if parts.muzzle_brake { 0.1 } else { 0.0 };

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
    }
}

/// Turn speed of a turret, degrees a second: its traverse motors against
/// everything standing on it.
pub fn traverse_deg_per_s(motors: u32, turret_mass_t: f32) -> f32 {
    if turret_mass_t <= 0.0 {
        return 80.0;
    }
    (260.0 * motors as f32 / turret_mass_t).clamp(2.0, 80.0)
}

#[cfg(test)]
mod gun_math_tests {
    use super::*;

    fn spinal() -> GunParts {
        // The Gun Deck preview's spinal gun.
        GunParts {
            gauge_increasers: 8,
            loaders: vec![Loader { length_m: 2, clips: 2.0 }; 4],
            barrel_m: 10,
            coolers: 6,
            absorbers: 3,
            rail_magnets: 2,
            muzzle_brake: false,
        }
    }

    #[test]
    fn the_preview_spinal_gun_comes_out_as_the_preview_says() {
        let s = stats(&spinal());
        assert_eq!(s.gauge_mm, 500.0);
        assert!((s.shell_m - 1.5).abs() < 1e-5);
        assert!((s.velocity_mps - 1030.0).abs() < 0.5);
        assert!((s.reload_s - 21.05).abs() < 0.1, "{}", s.reload_s);
        assert!((s.rpm - 11.4).abs() < 0.1, "{}", s.rpm);
        assert_eq!(s.working_loaders, 4);
        assert_eq!(s.ready_rack, 4 * 7);
        assert_eq!(s.rail_power_mw, 3.0);
    }

    #[test]
    fn gauge_caps_at_eight_increasers_and_500_mm() {
        assert_eq!(gauge_mm(0), 60.0);
        assert_eq!(gauge_mm(2), 170.0);
        assert_eq!(gauge_mm(8), 500.0);
        assert_eq!(gauge_mm(20), 500.0);
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
    fn heavier_turrets_turn_slower_and_motors_help() {
        assert!(traverse_deg_per_s(2, 80.0) < traverse_deg_per_s(2, 40.0));
        assert!(traverse_deg_per_s(4, 80.0) > traverse_deg_per_s(2, 80.0));
        assert_eq!(traverse_deg_per_s(1, 10_000.0), 2.0);
        assert_eq!(traverse_deg_per_s(10, 1.0), 80.0);
    }
}
