use bevy::prelude::*;

// ============================================================================
// KINETIC AMMO TYPES
// 9 types, each with unique hit behavior and tradeoffs.
// Players load a magazine MIX at station. Fires in loaded order.
// Different weights affect fire rate. Can't change mid-combat.
// ============================================================================

/// The 9 kinetic ammo types
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, serde::Serialize, serde::Deserialize)]
pub enum KineticAmmoType {
    /// Solid penetrator — goes through hull, hits modules behind
    AP,
    /// Penetrates then detonates inside — devastating but passes through thin targets
    APHE,
    /// Surface detonation + fragments — great vs creatures, zero penetration
    HEFrag,
    /// Sets fires — sustained DOT, useless in vacuum
    Incendiary,
    /// Disables modules — zero physical damage
    EMPShell,
    /// Proximity airburst — anti-missile, anti-swarm, weak vs armor
    Flak,
    /// Shaped charge — extreme penetration at perpendicular, bad at angles
    HEAT,
    /// Squash head — damages modules behind armor via shockwave, no penetration
    HESH,
    /// Fin-stabilized sabot — fastest, extreme penetration, needle damage
    APFSDS,
}

impl KineticAmmoType {
    pub fn name(&self) -> &'static str {
        match self {
            Self::AP => "AP",
            Self::APHE => "APHE",
            Self::HEFrag => "HE-Frag",
            Self::Incendiary => "Incendiary",
            Self::EMPShell => "EMP Shell",
            Self::Flak => "Flak",
            Self::HEAT => "HEAT",
            Self::HESH => "HESH",
            Self::APFSDS => "APFSDS",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::AP => "Solid penetrator. Goes through hull, hits what's behind. Wasted if nothing behind armor.",
            Self::APHE => "Penetrates then detonates inside. Devastating. Passes through thin targets without arming.",
            Self::HEFrag => "Surface burst + fragments. Great vs creatures. Bounces off heavy armor.",
            Self::Incendiary => "Sets fires. Sustained damage. Useless against depressurized targets.",
            Self::EMPShell => "Disables modules. Zero physical damage. Useless vs creatures.",
            Self::Flak => "Proximity airburst. Anti-missile, anti-swarm. Terrible vs armored targets.",
            Self::HEAT => "Shaped charge jet. Extreme penetration at 90°. Angled hits do almost nothing.",
            Self::HESH => "Squash head. Shockwave damages modules behind armor. Needs solid hull to work.",
            Self::APFSDS => "Ultra-fast dart. Goes through everything. Needle damage — can overpenetrate.",
        }
    }

    /// Weight multiplier — heavier ammo = slower fire rate
    pub fn weight_mult(&self) -> f32 {
        match self {
            Self::AP => 1.0,          // Baseline
            Self::APHE => 1.3,        // Heavier — has explosive filler
            Self::HEFrag => 1.1,      // Slightly heavy
            Self::Incendiary => 1.0,  // Normal weight
            Self::EMPShell => 1.2,    // Electronics add weight
            Self::Flak => 0.9,        // Lighter — fragmentation casing
            Self::HEAT => 1.4,        // Heavy — shaped charge liner
            Self::HESH => 1.3,        // Heavy — plastic explosive
            Self::APFSDS => 0.7,      // Light — discarding sabot, tiny dart
        }
    }

    /// Cost per round in credits
    pub fn cost_per_round(&self) -> u32 {
        match self {
            Self::AP => 2,
            Self::APHE => 5,
            Self::HEFrag => 3,
            Self::Incendiary => 3,
            Self::EMPShell => 8,
            Self::Flak => 2,
            Self::HEAT => 6,
            Self::HESH => 5,
            Self::APFSDS => 10,
        }
    }

    /// Muzzle velocity multiplier — affects lead prediction
    pub fn velocity_mult(&self) -> f32 {
        match self {
            Self::AP => 1.0,
            Self::APHE => 0.9,       // Slightly slower — heavier
            Self::HEFrag => 0.85,    // Slower
            Self::Incendiary => 0.95,
            Self::EMPShell => 0.8,   // Slow — fragile electronics
            Self::Flak => 1.1,       // Fast — needs to reach area
            Self::HEAT => 0.75,      // Slowest — shaped charge is heavy
            Self::HESH => 0.8,       // Slow — soft nose
            Self::APFSDS => 1.5,     // Fastest — lightweight dart
        }
    }

    /// Penetration value
    pub fn penetration(&self) -> f32 {
        match self {
            Self::AP => 50.0,
            Self::APHE => 40.0,     // Less than AP — explosive takes space from penetrator
            Self::HEFrag => 0.0,    // Zero — detonates on surface
            Self::Incendiary => 5.0, // Minimal
            Self::EMPShell => 10.0,  // Light pen to get near electronics
            Self::Flak => 0.0,      // Airburst — doesn't hit armor directly
            Self::HEAT => 70.0,     // Extreme — but angle dependent
            Self::HESH => 0.0,      // Zero pen — works through shockwave
            Self::APFSDS => 90.0,   // Highest penetration
        }
    }

    /// Fraction of a round's damage that reaches a module still covered by
    /// live hull, the rest being spent on the armour itself. This is what
    /// makes ammo choice a decision: a dart goes almost straight through to
    /// the engine you aimed at, while flak can't touch an internal until the
    /// plating over it is gone. See armor_pass_through for the None case.
    pub fn pass_through(&self) -> f32 {
        (self.penetration() / 100.0).clamp(0.0, 0.9)
    }

    /// Direct damage on hit
    pub fn damage_mult(&self) -> f32 {
        match self {
            Self::AP => 1.0,
            Self::APHE => 1.4,       // Good — pen + explosion
            Self::HEFrag => 0.7,     // Per-target lower, but hits area
            Self::Incendiary => 0.5,  // Low initial, fire does the rest
            Self::EMPShell => 0.0,    // Zero physical damage
            Self::Flak => 0.4,       // Low per-fragment
            Self::HEAT => 1.8,       // High — focused jet
            Self::HESH => 0.3,       // Low direct — shockwave does internal
            Self::APFSDS => 0.8,     // Moderate — needle damage
        }
    }

    /// Projectile visual color
    pub fn color(&self) -> Color {
        match self {
            Self::AP => Color::srgb(0.8, 0.7, 0.3),      // Brass
            Self::APHE => Color::srgb(0.9, 0.5, 0.2),    // Orange-brass
            Self::HEFrag => Color::srgb(1.0, 0.6, 0.1),  // Bright orange
            Self::Incendiary => Color::srgb(1.0, 0.3, 0.1), // Red-orange
            Self::EMPShell => Color::srgb(0.4, 0.5, 0.9), // Blue
            Self::Flak => Color::srgb(0.9, 0.9, 0.4),    // Yellow
            Self::HEAT => Color::srgb(0.8, 0.4, 0.1),    // Dark orange
            Self::HESH => Color::srgb(0.7, 0.7, 0.3),    // Olive
            Self::APFSDS => Color::srgb(0.9, 0.9, 1.0),  // White-bright (fast)
        }
    }
}

/// Ammo hit behavior — what happens on impact
#[derive(Clone, Debug)]
pub enum AmmoHitBehavior {
    /// Penetrates and continues (AP, APFSDS)
    Penetrate {
        remaining_pen: f32,
        damage_falloff: f32, // Damage reduction per layer penetrated
    },
    /// Penetrates then explodes inside (APHE)
    PenetrateExplode {
        penetration: f32,
        blast_damage: f32,
        blast_radius: f32,
        min_armor_to_arm: f32, // Needs this much armor to arm the fuse
    },
    /// Surface explosion + fragments (HE-Frag)
    SurfaceExplode {
        blast_damage: f32,
        blast_radius: f32,
        fragment_count: u32,
        fragment_damage: f32,
    },
    /// Sets fire (Incendiary)
    Ignite {
        fire_intensity: f32,
        fire_duration: f32,
    },
    /// Disables electronics (EMP Shell)
    EMPDisable {
        disable_radius: f32,
        disable_duration: f32,
    },
    /// Proximity airburst (Flak)
    ProximityBurst {
        trigger_distance: f32,
        fragment_count: u32,
        fragment_damage: f32,
        fragment_radius: f32,
    },
    /// Shaped charge jet (HEAT)
    ShapedCharge {
        jet_penetration: f32,
        jet_damage: f32,
        angle_sensitivity: f32, // 0.0 = any angle, 1.0 = must be perpendicular
    },
    /// Shockwave through armor (HESH)
    Shockwave {
        shockwave_damage: f32,
        shockwave_radius: f32, // How many blocks deep the shockwave goes
        requires_solid_hull: bool,
    },
}

impl KineticAmmoType {
    /// Get the hit behavior for this ammo type, scaled by weapon damage.
    /// Radii are in world units, sized against the 66-unit block grid —
    /// the original values predated the grid and couldn't even reach an
    /// adjacent block (e.g. blast_radius 40 vs 66 between block centers).
    pub fn hit_behavior(&self, base_damage: f32) -> AmmoHitBehavior {
        match self {
            Self::AP => AmmoHitBehavior::Penetrate {
                remaining_pen: self.penetration(),
                damage_falloff: 0.3, // Loses 30% per layer
            },
            Self::APHE => AmmoHitBehavior::PenetrateExplode {
                penetration: self.penetration(),
                blast_damage: base_damage * 0.8,
                blast_radius: 75.0,  // hit block + its direct neighbors
                min_armor_to_arm: 15.0, // Thin targets = fuse doesn't arm
            },
            Self::HEFrag => AmmoHitBehavior::SurfaceExplode {
                blast_damage: base_damage * 0.5,
                blast_radius: 110.0, // one full ring of blocks
                fragment_count: 8,
                fragment_damage: base_damage * 0.15,
            },
            Self::Incendiary => AmmoHitBehavior::Ignite {
                fire_intensity: 0.6,
                fire_duration: 8.0,
            },
            Self::EMPShell => AmmoHitBehavior::EMPDisable {
                disable_radius: 120.0, // reaches modules behind armor
                disable_duration: 6.0,
            },
            Self::Flak => AmmoHitBehavior::ProximityBurst {
                trigger_distance: 30.0,
                fragment_count: 12,
                fragment_damage: base_damage * 0.1,
                fragment_radius: 130.0, // wide, weak — saturation weapon
            },
            Self::HEAT => AmmoHitBehavior::ShapedCharge {
                jet_penetration: self.penetration(),
                jet_damage: base_damage * 1.5,
                angle_sensitivity: 0.7, // Needs mostly perpendicular hit
            },
            Self::HESH => AmmoHitBehavior::Shockwave {
                shockwave_damage: base_damage * 0.6,
                shockwave_radius: 2.0, // 2 blocks deep
                requires_solid_hull: true,
            },
            Self::APFSDS => AmmoHitBehavior::Penetrate {
                remaining_pen: self.penetration(),
                damage_falloff: 0.15, // Only loses 15% per layer — goes through everything
            },
        }
    }
}

// ============================================================================
// MAGAZINE SYSTEM — loaded mix of ammo types
// ============================================================================

/// A loaded magazine with a specific mix of ammo types
#[derive(Component, Clone, Debug)]
pub struct LoadedMagazine {
    /// Ammo types in firing order
    pub rounds: Vec<KineticAmmoType>,
    /// Current round index
    pub current_round: usize,
    /// Total rounds remaining
    pub remaining: u32,
}

impl LoadedMagazine {
    /// Create a magazine with a single ammo type
    pub fn uniform(ammo_type: KineticAmmoType, count: u32) -> Self {
        Self {
            rounds: vec![ammo_type],
            current_round: 0,
            remaining: count,
        }
    }

    /// Create a magazine with a mixed load
    /// Pattern repeats: e.g., [AP, AP, HEFrag] fires AP, AP, HEFrag, AP, AP, HEFrag...
    pub fn mixed(pattern: Vec<KineticAmmoType>, total_rounds: u32) -> Self {
        Self {
            rounds: pattern,
            current_round: 0,
            remaining: total_rounds,
        }
    }

    /// Get the next round to fire
    pub fn next_round(&mut self) -> Option<KineticAmmoType> {
        if self.remaining == 0 || self.rounds.is_empty() {
            return None;
        }

        let round = self.rounds[self.current_round % self.rounds.len()];
        self.current_round += 1;
        self.remaining -= 1;
        Some(round)
    }

    /// Average weight multiplier for the loaded mix (affects fire rate)
    pub fn avg_weight_mult(&self) -> f32 {
        if self.rounds.is_empty() { return 1.0; }
        let total: f32 = self.rounds.iter().map(|r| r.weight_mult()).sum();
        total / self.rounds.len() as f32
    }

    /// Total cost of the magazine
    pub fn total_cost(&self) -> u32 {
        if self.rounds.is_empty() { return 0; }
        // Cost is per-round based on the mix pattern, multiplied by total rounds
        let pattern_cost: u32 = self.rounds.iter().map(|r| r.cost_per_round()).sum();
        let avg_cost = pattern_cost / self.rounds.len() as u32;
        avg_cost * self.remaining
    }
}

/// Default magazine configs for common loadouts
pub fn default_magazines() -> Vec<(&'static str, Vec<KineticAmmoType>, &'static str)> {
    vec![
        ("Standard AP", vec![KineticAmmoType::AP], "Solid penetrators. Reliable against armor."),
        ("Anti-Armor Mix", vec![KineticAmmoType::AP, KineticAmmoType::AP, KineticAmmoType::APHE], "Two AP to weaken, one APHE to finish."),
        ("Anti-Creature", vec![KineticAmmoType::HEFrag], "Surface burst + fragments. Shreds unarmored targets."),
        ("Fire Starter", vec![KineticAmmoType::Incendiary, KineticAmmoType::Incendiary, KineticAmmoType::HEFrag], "Set fires, then fragment."),
        ("Point Defense", vec![KineticAmmoType::Flak], "Proximity burst. Anti-missile, anti-swarm."),
        ("System Killer", vec![KineticAmmoType::AP, KineticAmmoType::EMPShell], "Penetrate hull, disable electronics inside."),
        ("Shaped Charge", vec![KineticAmmoType::HEAT], "Maximum single-target penetration. Angle matters."),
        ("Concussion", vec![KineticAmmoType::HESH], "Shockwave through hull. Damages internal modules without penetrating."),
        ("Sabot Dart", vec![KineticAmmoType::APFSDS], "Ultra-fast. Goes through everything. Expensive."),
        ("Kitchen Sink", vec![KineticAmmoType::AP, KineticAmmoType::HEFrag, KineticAmmoType::Incendiary, KineticAmmoType::APHE], "A bit of everything."),
    ]
}

/// What comes off the BACK of a plate when a round gets through it.
///
/// Distinct from the round's own effect in `AmmoHitBehavior`: that's the shell
/// doing something (detonating, burning, disabling). This is the ARMOUR
/// failing — fragments of the plate's inner face driven into whatever is
/// behind it. It's why a penetration is categorically worse than a dent, and
/// it's where the rounds differ most, because it describes what each one does
/// once it's inside.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpallProfile {
    /// Fragments thrown off the inner face.
    pub fragments: u32,
    /// Half-angle of the cone they spray into, in degrees. A long dart makes a
    /// neat hole; a squash head detaches a whole scab.
    pub cone_degrees: f32,
    /// Damage per fragment, as a fraction of the round's damage.
    pub damage_frac: f32,
    /// How far in the fragments carry, in cells.
    pub reach: f32,
    /// Whether it spalls WITHOUT penetrating. True only for HESH, whose entire
    /// identity is defeating armour it can't get through — it's the answer to
    /// a sloped hull, and the reason its penetration is 0 by design.
    pub through_solid: bool,
}

impl SpallProfile {
    pub const NONE: Self = Self {
        fragments: 0, cone_degrees: 0.0, damage_frac: 0.0, reach: 0.0, through_solid: false,
    };
}

/// Per-round spall. The shape of the cone is the round's signature:
///
///   APFSDS  narrow and deep   — a needle. Goes far in, wrecks little.
///   HEAT    narrow and hot    — kills whatever is directly behind the hole.
///   AP      the baseline.
///   APHE    wide              — its own blast finishes the compartment.
///   HESH    widest, and the only one that works through UNBREACHED armour.
pub fn spall(ammo: Option<KineticAmmoType>) -> SpallProfile {
    use KineticAmmoType::*;
    match ammo {
        Some(APFSDS) => SpallProfile { fragments: 3, cone_degrees: 12.0, damage_frac: 0.08, reach: 4.0, through_solid: false },
        Some(HEAT) => SpallProfile { fragments: 3, cone_degrees: 10.0, damage_frac: 0.30, reach: 3.0, through_solid: false },
        Some(APHE) => SpallProfile { fragments: 6, cone_degrees: 50.0, damage_frac: 0.12, reach: 2.0, through_solid: false },
        Some(HESH) => SpallProfile { fragments: 10, cone_degrees: 80.0, damage_frac: 0.18, reach: 2.0, through_solid: true },
        Some(Incendiary) => SpallProfile { fragments: 2, cone_degrees: 30.0, damage_frac: 0.05, reach: 2.0, through_solid: false },
        // Surface bursts and EMP never get inside to shed anything.
        Some(HEFrag) | Some(Flak) | Some(EMPShell) => SpallProfile::NONE,
        // AP, and the unspecialised default for beams/rams/AI fire, which
        // arrives with no ammo profile recorded.
        Some(AP) | None => SpallProfile { fragments: 5, cone_degrees: 35.0, damage_frac: 0.10, reach: 2.0, through_solid: false },
    }
}

/// Pass-through for a hit whose round has no ammo profile — an unspecialised
/// shell, a beam, a ram. Armour stops most of it; a little always bleeds
/// through to whatever is bolted behind the plate.
pub fn armor_pass_through(ammo: Option<KineticAmmoType>) -> f32 {
    match ammo {
        Some(a) => a.pass_through(),
        None => 0.15,
    }
}

#[cfg(test)]
mod spall_tests {
    use super::*;

    /// The cone shape is each round's signature for what it does INSIDE a
    /// ship, and it's the axis they differ on most. A dart bores a neat hole;
    /// a squash head detaches a whole scab.
    #[test]
    fn cone_width_separates_the_rounds() {
        let dart = spall(Some(KineticAmmoType::APFSDS));
        let jet = spall(Some(KineticAmmoType::HEAT));
        let ap = spall(Some(KineticAmmoType::AP));
        let aphe = spall(Some(KineticAmmoType::APHE));
        let hesh = spall(Some(KineticAmmoType::HESH));

        assert!(dart.cone_degrees < ap.cone_degrees, "a long rod should bore, not scatter");
        assert!(jet.cone_degrees < ap.cone_degrees, "a shaped charge jet is focused");
        assert!(aphe.cone_degrees > ap.cone_degrees);
        assert!(hesh.cone_degrees > aphe.cone_degrees, "HESH should be the widest");
    }

    /// APFSDS reaches deepest and hurts least per fragment — the overpenetration
    /// trade its description already promises.
    #[test]
    fn a_dart_goes_deep_and_does_little() {
        let dart = spall(Some(KineticAmmoType::APFSDS));
        let ap = spall(Some(KineticAmmoType::AP));
        assert!(dart.reach > ap.reach);
        assert!(dart.damage_frac < ap.damage_frac);
        assert!(dart.fragments < ap.fragments);
    }

    /// HEAT concentrates: fewest fragments, hardest each. It kills the one
    /// thing behind the hole rather than gutting a compartment.
    #[test]
    fn a_shaped_charge_concentrates() {
        let jet = spall(Some(KineticAmmoType::HEAT));
        let ap = spall(Some(KineticAmmoType::AP));
        assert!(jet.damage_frac > ap.damage_frac);
        assert!(jet.fragments <= ap.fragments);
    }

    /// HESH is the ONLY round that spalls without getting through. That's its
    /// whole identity — the answer to armour too thick or too angled to
    /// breach — and the reason its penetration is 0 by design.
    #[test]
    fn only_hesh_spalls_through_unbreached_armour() {
        for ammo in [
            KineticAmmoType::AP, KineticAmmoType::APHE, KineticAmmoType::HEFrag,
            KineticAmmoType::Incendiary, KineticAmmoType::EMPShell, KineticAmmoType::Flak,
            KineticAmmoType::HEAT, KineticAmmoType::APFSDS,
        ] {
            assert!(!spall(Some(ammo)).through_solid, "{ammo:?} must not spall through solid armour");
        }
        assert!(spall(Some(KineticAmmoType::HESH)).through_solid);
        assert_eq!(KineticAmmoType::HESH.penetration(), 0.0, "HESH earns its spall by not penetrating");
    }

    /// Surface bursts never get inside, so they shed nothing.
    #[test]
    fn surface_bursts_and_emp_shed_nothing() {
        for ammo in [KineticAmmoType::HEFrag, KineticAmmoType::Flak, KineticAmmoType::EMPShell] {
            assert_eq!(spall(Some(ammo)), SpallProfile::NONE, "{ammo:?} should not spall");
        }
    }

    /// Unspecialised fire — beams, rams, AI shots with no ammo recorded — gets
    /// the AP baseline rather than nothing, so a breach always means something.
    #[test]
    fn unspecialised_fire_gets_the_baseline() {
        assert_eq!(spall(None), spall(Some(KineticAmmoType::AP)));
        assert!(spall(None).fragments > 0);
    }
}
