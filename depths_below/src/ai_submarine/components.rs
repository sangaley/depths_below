use bevy::prelude::*;

// ============================================================================
// AI SUBMARINE COMPONENTS
// ============================================================================

/// Marker component for AI-controlled submarines — NEVER combined with Submarine
#[derive(Component)]
pub struct AiSubmarine;

/// Faction/type of AI submarine
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum AiSubType {
    Leviathan,   // Creature-towed subs, net launchers, specimen vaults
    AbyssalCult, // Bio-organic hybrids, self-healing, kamikaze
    Drowned,     // Ghost ships, already damaged, erratic, rare loot
    PressureKing,// Deep-zone lords, pressure weapons, ram upward
    GlassEye,    // Silent stalkers, no weapons, broadcasts on death
    IronTide,    // Heavy battleships, railguns, tanky boss faction
    Blackwater,  // Elite mercs, tactical flanking, hunt bounties
    RustSwarm,   // Tiny junk subs, spawn in groups, kamikaze
}

/// Aggregated state for the AI submarine
#[derive(Component)]
pub struct AiSubState {
    pub hull_integrity: f32,     // 0.0–1.0, aggregated from child HullSegments
    pub noise_level: f32,        // Sum of child Engine noise
    pub fuel: f32,
    pub max_fuel: f32,
    pub depth: f32,
    pub is_destroyed: bool,
    pub last_hit_timer: f32,     // Seconds since last damage (for "under fire" AI)
}

impl Default for AiSubState {
    fn default() -> Self {
        Self {
            hull_integrity: 1.0,
            noise_level: 0.0,
            fuel: 500.0,
            max_fuel: 500.0,
            depth: 0.0,
            is_destroyed: false,
            last_hit_timer: 999.0,
        }
    }
}

/// Current high-level behavior of the AI submarine
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum AiSubBehavior {
    Idle,
    Patrolling,
    FollowingTradeRoute,
    Salvaging,
    Fleeing,
    Engaging,
    EvadingCreature,
    Dead,
}

impl Default for AiSubBehavior {
    fn default() -> Self {
        Self::Idle
    }
}

/// Navigation data for AI submarine
#[derive(Component)]
pub struct AiSubNav {
    pub waypoints: Vec<Vec2>,
    pub current_waypoint: usize,
    pub destination: Option<Vec2>,
    pub rotation: f32,
    pub throttle: f32,
}

impl Default for AiSubNav {
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

/// Timer for AI decision ticks (0.25s)
#[derive(Component)]
pub struct AiSubDecisionTimer {
    pub timer: Timer,
}

impl Default for AiSubDecisionTimer {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(0.25, TimerMode::Repeating),
        }
    }
}

/// Attached to every child HullSegment/Module of an AI submarine
#[derive(Component)]
pub struct OwnedByAiSub {
    pub root: Entity,
}

/// Inserted on AI sub root when noise exceeds sonar detection threshold
#[derive(Component)]
pub struct AiSubSonarContact {
    pub noise_signature: f32,
    pub revealed_timer: Timer,
}

/// Wreck entity spawned after AI submarine destruction
#[derive(Component)]
pub struct AiSubWreck {
    pub sub_type: AiSubType,
    pub loot_remaining: u32,
}

// ============================================================================
// WORLD SIMULATION - Off-screen faction tracking
// ============================================================================

/// A simulated (off-screen) AI submarine tracked by position only
#[derive(Clone, Debug)]
pub struct SimulatedSub {
    pub faction: AiSubType,
    pub position: Vec2,
    pub velocity: Vec2,
    pub health: f32,      // 0.0-1.0
    pub fuel: f32,
    pub behavior: SimBehavior,
    pub home_zone: Vec2,  // Center of their territory
    pub spawned: bool,    // true if currently a real entity on screen
}

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
    pub subs: Vec<SimulatedSub>,
    pub tick_timer: Timer,
    pub initialized: bool,
}

impl Default for WorldSimulation {
    fn default() -> Self {
        Self {
            subs: Vec::new(),
            tick_timer: Timer::from_seconds(2.0, TimerMode::Repeating),
            initialized: false,
        }
    }
}

/// Faction territory definition
pub struct FactionTerritory {
    pub faction: AiSubType,
    pub center: Vec2,
    pub radius: f32,
    pub sub_count: usize,
}

/// Returns initial faction territories and populations
pub fn faction_territories() -> Vec<FactionTerritory> {
    vec![
        // Leviathan Riders - shallow hunting grounds
        FactionTerritory {
            faction: AiSubType::Leviathan,
            center: Vec2::new(6000.0, -4000.0),
            radius: 2000.0,
            sub_count: 2,
        },
        // Abyssal Cult - mid-depth sacred waters
        FactionTerritory {
            faction: AiSubType::AbyssalCult,
            center: Vec2::new(-5500.0, -6000.0),
            radius: 2500.0,
            sub_count: 3,
        },
        // The Drowned - scattered everywhere, no home
        FactionTerritory {
            faction: AiSubType::Drowned,
            center: Vec2::new(4500.0, -7000.0),
            radius: 5000.0,
            sub_count: 3,
        },
        // Pressure Kings - deep zone only
        FactionTerritory {
            faction: AiSubType::PressureKing,
            center: Vec2::new(-4000.0, -10000.0),
            radius: 3000.0,
            sub_count: 2,
        },
        // Glass Eye - everywhere, lurking
        FactionTerritory {
            faction: AiSubType::GlassEye,
            center: Vec2::new(-6000.0, -3500.0),
            radius: 4000.0,
            sub_count: 2,
        },
        // Iron Tide - deep military zone
        FactionTerritory {
            faction: AiSubType::IronTide,
            center: Vec2::new(7000.0, -8000.0),
            radius: 2500.0,
            sub_count: 1, // rare but powerful
        },
        // Blackwater PMC - mid-depth patrol routes
        FactionTerritory {
            faction: AiSubType::Blackwater,
            center: Vec2::new(-7000.0, -5000.0),
            radius: 3000.0,
            sub_count: 2,
        },
        // Rust Swarm - shallow scrapyards
        FactionTerritory {
            faction: AiSubType::RustSwarm,
            center: Vec2::new(5000.0, -2000.0),
            radius: 2000.0,
            sub_count: 5, // many small subs
        },
    ]
}

/// Returns whether two factions are hostile to each other
pub fn factions_hostile(a: AiSubType, b: AiSubType) -> bool {
    use AiSubType::*;
    if a == b { return false; } // same faction = allies
    match (a, b) {
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
