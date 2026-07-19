use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// AI SUBMARINE COMPONENTS
// ============================================================================

/// Marker component for AI-controlled ships — NEVER combined with Ship
#[derive(Component)]
pub struct AiShip;

/// Faction/type of AI ship
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub enum AiShipType {
    Leviathan,   // Creature-towed ships, net launchers, specimen vaults
    AbyssalCult, // Bio-organic hybrids, self-healing, kamikaze
    Drowned,     // Ghost ships, already damaged, erratic, rare loot
    PressureKing,// Deep-zone lords, pressure weapons, ram upward
    GlassEye,    // Silent stalkers, no weapons, broadcasts on death
    IronTide,    // Heavy battleships, railguns, tanky boss faction
    Blackwater,  // Elite mercs, tactical flanking, hunt bounties
    RustSwarm,   // Tiny junk ships, spawn in groups, kamikaze
    // --- True bosses: rare, spawn only at the extreme edge of explored
    // space, dwarf every other ship in the roster. Jackpot bounty targets.
    Dreadnought, // Mega-battleship — Iron Tide's design taken to its limit
    VoidTitan,   // Abyssal-leviathan hybrid — the largest, hardest kill in the game
}

/// Aggregated state for the AI ship
#[derive(Component)]
pub struct AiShipState {
    pub hull_integrity: f32,     // 0.0–1.0, aggregated from child HullSegments
    pub noise_level: f32,        // Sum of child Engine noise
    pub fuel: f32,
    pub max_fuel: f32,
    pub depth: f32,
    pub is_destroyed: bool,
    pub last_hit_timer: f32,     // Seconds since last damage (for "under fire" AI)
    /// Ship (player Ship entity or AI ship root) that last hit this ship,
    /// if attributable — see events::AiShipDamaged.attacker. "Under fire"
    /// retaliation targets this instead of guessing, so a ship caught in
    /// another AI ship's crossfire fights back against THAT ship, not
    /// reflexively the player.
    pub last_attacker: Option<Entity>,
}

impl Default for AiShipState {
    fn default() -> Self {
        Self {
            hull_integrity: 1.0,
            noise_level: 0.0,
            fuel: 500.0,
            max_fuel: 500.0,
            depth: 0.0,
            is_destroyed: false,
            last_hit_timer: 999.0,
            last_attacker: None,
        }
    }
}

/// Current high-level behavior of the AI ship
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum AiShipBehavior {
    Idle,
    Patrolling,
    FollowingTradeRoute,
    Salvaging,
    Fleeing,
    Engaging,
    EvadingCreature,
    Dead,
}

impl Default for AiShipBehavior {
    fn default() -> Self {
        Self::Idle
    }
}

/// Navigation data for AI ship
#[derive(Component)]
pub struct AiShipNav {
    pub waypoints: Vec<Vec2>,
    pub current_waypoint: usize,
    pub destination: Option<Vec2>,
    pub rotation: f32,
    pub throttle: f32,
}

impl Default for AiShipNav {
    fn default() -> Self {
        Self {
            waypoints: Vec::new(),
            current_waypoint: 0,
            destination: None,
            rotation: 0.0,
            throttle: 0.0,
        }
    }
}

/// The ship's current combat target — separate from AiShipNav.destination
/// because destination is often an OFFSET from the target (Blackwater's
/// flank position, PressureKing's ram-from-above point), not the target's
/// actual position. Weapons fire at `position`; movement still uses
/// AiShipNav.destination. Recomputed every brain tick (0.25s) in
/// ai_brain::ai_brain_system via a faction-agnostic distance/value scoring
/// pass over the player + every other living AI ship — see that file's
/// doc comment for which factions actually use it vs. staying player-only.
#[derive(Component, Default)]
pub struct AiShipTarget {
    pub entity: Option<Entity>,
    pub position: Vec2,
}

/// Timer for AI decision ticks (0.25s)
#[derive(Component)]
pub struct AiShipDecisionTimer {
    pub timer: Timer,
}

impl Default for AiShipDecisionTimer {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(0.25, TimerMode::Repeating),
        }
    }
}

/// Attached to every child HullSegment/Module of an AI ship
#[derive(Component)]
pub struct OwnedByAiShip {
    pub root: Entity,
}

/// Inserted on AI ship root when noise exceeds radar detection threshold
#[derive(Component)]
pub struct AiShipRadarContact {
    pub noise_signature: f32,
    pub revealed_timer: Timer,
}

/// Wreck entity spawned after AI ship destruction
#[derive(Component)]
pub struct AiShipWreck {
    pub ship_type: AiShipType,
    pub loot_remaining: u32,
    /// Fraction of blocks still intact at the moment of death (0..1).
    /// Forensic record of how gently the kill was done — biases loot
    /// composition when salvaging, not just quantity.
    pub intact_frac: f32,
}

// ============================================================================
// WORLD SIMULATION - Off-screen faction tracking
// ============================================================================

/// A simulated (off-screen) AI ship tracked by position only
#[derive(Clone, Debug)]
pub struct SimulatedShip {
    pub faction: AiShipType,
    pub position: Vec2,
    pub velocity: Vec2,
    pub health: f32,      // 0.0-1.0
    pub fuel: f32,
    pub behavior: SimBehavior,
    pub home_zone: Vec2,  // Center of their territory
    /// How far this ship drifts from home_zone before turning back. Was a
    /// flat 2500.0 for every ship regardless of its actual territory size —
    /// fine when territories were all bunched within ~12,000 units of
    /// spawn, but pointless once territories got spread across the real
    /// solar-system scale (tens/hundreds of thousands of units): a ship
    /// with a 15,000-unit-radius territory would still snap back after
    /// drifting 2500.
    pub patrol_radius: f32,
    pub spawned: bool,    // true if currently a real entity on screen
    /// Set when a bounty contract has tagged this specific ship as its
    /// target — carried onto the real entity (see BountyTarget) once it
    /// spawns, and read back off when it's destroyed so contract tracking
    /// can tell "this exact ship died" from "some ship of that faction died".
    pub bounty_id: Option<u32>,
}

/// Attached to an AI ship's root entity when it was spawned from a
/// bounty-tagged SimulatedShip. Read by ai_ship::combat when the ship dies
/// to populate AiShipDestroyed::bounty_id.
#[derive(Component, Clone, Copy)]
pub struct BountyTarget(pub u32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SimBehavior {
    Roaming,
    Fighting(usize), // index of opponent in sim list
    Fleeing,
    Trading,
    CapturingCreature,
    Patrolling,
    Dead,
}

/// Global off-screen world simulation
#[derive(Resource)]
pub struct WorldSimulation {
    pub ships: Vec<SimulatedShip>,
    pub tick_timer: Timer,
    pub initialized: bool,
    next_bounty_id: u32,
}

impl Default for WorldSimulation {
    fn default() -> Self {
        Self {
            ships: Vec::new(),
            tick_timer: Timer::from_seconds(2.0, TimerMode::Repeating),
            initialized: false,
            next_bounty_id: 1,
        }
    }
}

impl WorldSimulation {
    /// Picks a random living, untagged ship belonging to `faction` and tags
    /// it as a bounty target. Returns (bounty_id, spawn distance from origin)
    /// so the contract can use the target's real position for its reward and
    /// live map marker instead of an approximate territory center.
    ///
    /// Prefers ships that aren't currently spawned as real entities: tagging
    /// only touches the SimulatedShip record, and a ship that's already
    /// spawned won't retroactively get the BountyTarget component that
    /// carries the tag onto the real entity — so tagging an already-spawned
    /// ship would leave it unkillable-for-the-contract until it despawns and
    /// respawns. Falls back to spawned ships if that's all a faction has
    /// left (better a rare edge case than no bounty at all for a
    /// small-population faction).
    pub fn tag_bounty_target(&mut self, faction: AiShipType, rng: &mut impl rand::Rng) -> Option<(u32, f32)> {
        let unspawned: Vec<usize> = self.ships.iter().enumerate()
            .filter(|(_, s)| s.faction == faction && s.behavior != SimBehavior::Dead && s.bounty_id.is_none() && !s.spawned)
            .map(|(i, _)| i)
            .collect();
        let candidates = if !unspawned.is_empty() {
            unspawned
        } else {
            self.ships.iter().enumerate()
                .filter(|(_, s)| s.faction == faction && s.behavior != SimBehavior::Dead && s.bounty_id.is_none())
                .map(|(i, _)| i)
                .collect()
        };
        let &idx = candidates.get(rng.gen_range(0..candidates.len().max(1)))?;
        let id = self.next_bounty_id;
        self.next_bounty_id += 1;
        let ship = &mut self.ships[idx];
        ship.bounty_id = Some(id);
        Some((id, ship.position.length()))
    }

    /// Current position of a tagged bounty target, if it's still tracked
    /// (i.e. hasn't been despawned/reset). Used for the live map marker.
    pub fn bounty_position(&self, bounty_id: u32) -> Option<Vec2> {
        self.ships.iter().find(|s| s.bounty_id == Some(bounty_id)).map(|s| s.position)
    }

    /// Frees a bounty tag without touching the ship otherwise — called when
    /// a contract is abandoned or fails. Without this, an abandoned bounty
    /// would tag its ship forever, which for a single-ship faction (the
    /// bosses) would permanently lock out ever offering that faction as a
    /// bounty again.
    pub fn untag_bounty(&mut self, bounty_id: u32) {
        if let Some(ship) = self.ships.iter_mut().find(|s| s.bounty_id == Some(bounty_id)) {
            ship.bounty_id = None;
        }
    }
}

/// Faction territory definition
pub struct FactionTerritory {
    pub faction: AiShipType,
    pub center: Vec2,
    pub radius: f32,
    pub ship_count: usize,
}

/// Returns initial faction territories and populations.
///
/// Scaled to match the rest of the universe: planets orbit at 25k-45k+ per
/// index from their star, and the first star system sits 200k/450k out from
/// spawn ("the sun is a destination, not a spawn point" — see
/// celestial/mod.rs). These territories used to all be packed within
/// ~12,000 units of the origin — a ship you'd just destroyed and its
/// neighbor from the *next* territory over were both within weapon
/// engagement range (up to 7,000 units) practically all the time, so
/// finishing one fight meant immediately starting the next with no room to
/// scavenge the wreck. Centers are now tens of thousands of units apart —
/// comfortably beyond any territory's radius plus engagement range — with
/// radii large enough that a territory itself is a real area to explore,
/// not a single point.
pub fn faction_territories() -> Vec<FactionTerritory> {
    vec![
        // Rust Swarm - shallow scrapyards, closest to spawn
        FactionTerritory {
            faction: AiShipType::RustSwarm,
            center: Vec2::new(30_000.0, -10_000.0),
            radius: 15_000.0,
            ship_count: 5, // many small ships
        },
        // Leviathan Riders - shallow hunting grounds
        FactionTerritory {
            faction: AiShipType::Leviathan,
            center: Vec2::new(-45_000.0, -20_000.0),
            radius: 18_000.0,
            ship_count: 2,
        },
        // Abyssal Cult - mid-depth sacred waters
        FactionTerritory {
            faction: AiShipType::AbyssalCult,
            center: Vec2::new(60_000.0, -50_000.0),
            radius: 22_000.0,
            ship_count: 3,
        },
        // Glass Eye - everywhere, lurking
        FactionTerritory {
            faction: AiShipType::GlassEye,
            center: Vec2::new(-85_000.0, -55_000.0),
            radius: 28_000.0,
            ship_count: 2,
        },
        // Blackwater PMC - mid-depth patrol routes, out toward the outer system
        FactionTerritory {
            faction: AiShipType::Blackwater,
            center: Vec2::new(60_000.0, -180_000.0),
            radius: 28_000.0,
            ship_count: 3,
        },
        // The Drowned - scattered everywhere, no home (biggest spread)
        FactionTerritory {
            faction: AiShipType::Drowned,
            center: Vec2::new(-60_000.0, -200_000.0),
            radius: 45_000.0,
            ship_count: 4,
        },
        // Iron Tide - deep military zone, far outer system
        FactionTerritory {
            faction: AiShipType::IronTide,
            center: Vec2::new(150_000.0, -250_000.0),
            radius: 28_000.0,
            ship_count: 2, // rare but powerful
        },
        // Pressure Kings - deep zone only, farthest out of the "normal"
        // factions (still a real gap before the star itself at ~492k, which
        // stays a distant endgame destination rather than just another
        // territory)
        FactionTerritory {
            faction: AiShipType::PressureKing,
            center: Vec2::new(-140_000.0, -320_000.0),
            radius: 35_000.0,
            ship_count: 3,
        },
        // Dreadnought — one lone mega-battleship, patrolling well past the
        // star system. Finding it at all is most of the challenge.
        FactionTerritory {
            faction: AiShipType::Dreadnought,
            center: Vec2::new(400_000.0, -420_000.0), // ~580k out
            radius: 60_000.0,
            ship_count: 1,
        },
        // Void Titan — the single hardest kill in the game, sitting beyond
        // everything else in explored space.
        FactionTerritory {
            faction: AiShipType::VoidTitan,
            center: Vec2::new(-600_000.0, -600_000.0), // ~850k out
            radius: 80_000.0,
            ship_count: 1,
        },
    ]
}

/// Combat power rating for a faction — used for off-screen sim combat and
/// to weight bounty-contract rewards by how dangerous the target is.
pub fn faction_power(faction: AiShipType) -> f32 {
    match faction {
        AiShipType::VoidTitan => 8.0,      // the hardest kill in the game
        AiShipType::Dreadnought => 6.0,    // mega-battleship
        AiShipType::IronTide => 3.0,      // Battleship - strongest "normal" faction
        AiShipType::Blackwater => 2.0,     // Elite mercs
        AiShipType::PressureKing => 2.5,   // Heavy armor + weapons
        AiShipType::AbyssalCult => 1.5,    // Bio-weapons
        AiShipType::Leviathan => 1.2,      // Creature + some weapons
        AiShipType::Drowned => 1.0,        // Already damaged
        AiShipType::RustSwarm => 0.5,      // Weak individually
        AiShipType::GlassEye => 0.1,       // No weapons
    }
}

/// Fraction of a faction's crew-eligible stations (Reactor/Engine/Weapon/
/// etc — anything the module registry marks crew_station:true) that
/// actually get a warm body. The AI-side equivalent of the player's own
/// chronic short-staffing ("8 crew never cover every station on a real
/// ship" — ship/spawner.rs): auto_assign_crew fills stations by priority
/// (Power, then Propulsion, then Weapons last), so below 1.0 means guns go
/// dark first while reactors/engines keep running; above 1.0 means spare
/// hands ready to backfill when someone dies. This is deliberately separate
/// from faction_power (a combat-strength RATING) — it's the mechanism that
/// makes weak factions mechanically weak (RustSwarm ships that only half-
/// fire) rather than just numerically weak.
pub fn crew_fill_fraction(faction: AiShipType) -> f32 {
    match faction {
        AiShipType::VoidTitan => 1.3,      // apex predator, crew to spare
        AiShipType::Dreadnought => 1.2,
        AiShipType::IronTide => 1.1,       // disciplined battleship, fully crewed
        AiShipType::Blackwater => 1.0,     // tight professional crew, no slack
        AiShipType::PressureKing => 0.95,
        AiShipType::AbyssalCult => 0.9,    // reckless zealots, not undercrewed by design
        AiShipType::Leviathan => 0.8,
        AiShipType::GlassEye => 0.7,       // silent skeleton crew (no weapons anyway)
        AiShipType::RustSwarm => 0.6,      // scrappy, individually weak — half their guns dark
        AiShipType::Drowned => 0.55,       // ghost ship, half-crewed by nature
    }
}

/// Returns whether two factions are hostile to each other
pub fn factions_hostile(a: AiShipType, b: AiShipType) -> bool {
    use AiShipType::*;
    if a == b { return false; } // same faction = allies
    match (a, b) {
        // Bosses are hostile to everything, including each other — rampaging
        // apex threats, not aligned with any faction's politics.
        (VoidTitan, _) | (_, VoidTitan) => true,
        (Dreadnought, _) | (_, Dreadnought) => true,
        // Abyssal Cult attacks Leviathan Riders (they capture creatures)
        (AbyssalCult, Leviathan) | (Leviathan, AbyssalCult) => true,
        // Iron Tide attacks everyone except Blackwater (allied mercs)
        (IronTide, Blackwater) | (Blackwater, IronTide) => false,
        (IronTide, _) | (_, IronTide) => true,
        // Blackwater hunts pirates (Rust Swarm) and Cult
        (Blackwater, RustSwarm) | (RustSwarm, Blackwater) => true,
        (Blackwater, AbyssalCult) | (AbyssalCult, Blackwater) => true,
        // Rust Swarm attacks everyone weaker
        (RustSwarm, GlassEye) | (GlassEye, RustSwarm) => true,
        (RustSwarm, Leviathan) | (Leviathan, RustSwarm) => true,
        // Pressure Kings attack anyone in deep zone
        (PressureKing, _) | (_, PressureKing) => true,
        // Drowned attack everything (mindless)
        (Drowned, _) | (_, Drowned) => true,
        // Glass Eye never attacks
        (GlassEye, _) | (_, GlassEye) => false,
        _ => false,
    }
}
