use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// AI SUBMARINE COMPONENTS
// ============================================================================

/// Marker component for AI-controlled ships — NEVER combined with Ship
#[derive(Component)]
pub struct AiShip;

/// Faction/type of AI ship.
///
/// The names come from the Space Empire story bible. The behaviour under each
/// one predates it and is unchanged — every mapping was chosen so the name
/// never contradicts what the faction already does.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub enum AiShipType {
    /// Creature-keepers. Protect life at any cost, including pruning the
    /// diseased branches. Nets and specimen vaults; flees rather than fights.
    StellarPreserve,
    /// Flesh-machine merger. Bio-organic hybrids, self-healing, kamikaze.
    /// The screaming in their networks is just data corruption.
    SynthesisCollective,
    /// The shattered gestalt. Ghost ships, already damaged, erratic, rare
    /// loot. Isolated frequencies carry voices that sound like your own.
    BrokenChoir,
    /// Entropy cult out of the previous universe. Territorial deep lords with
    /// crushing weapons who ram anything that enters their reach.
    CorpseStars,
    /// They removed their own ambition to end war, and it worked. No weapons
    /// at all, never initiates, broadcasts once on death.
    TheSilence,
    /// Humanist military. Heavy battleships, railguns, discipline, deep
    /// reserves. The unification wars are a forbidden topic.
    TerranHegemony,
    /// The rich of a thousand civilizations, who bought their way out of three
    /// dying universes. Elite mercenaries, tactical flanking, bounty hunters.
    GildedThrone,
    /// One person cloned into a civilization, still arguing with itself. Tiny
    /// identical hulls that arrive in groups and ram.
    RecursiveKingdom,
    // --- True bosses: rare, spawn only at the extreme edge of explored
    // space, dwarf every other ship in the roster. Jackpot bounty targets.
    /// The Hegemony's own past, still at peak power and still obeying dead
    /// emperors. Their design taken to its limit.
    EternalHegemony,
    /// It does not want to kill you. It wants you to become one. The largest
    /// and hardest kill in the game.
    Shepherd,
}

/// The faction's name as the player should ever see it.
///
/// Canonical: every player-facing string goes through here. Formatting an
/// `AiShipType` with `{:?}` prints the Rust identifier instead, which had
/// already leaked into a kill notification once.
pub fn faction_display_name(ship_type: AiShipType) -> &'static str {
    use AiShipType::*;
    match ship_type {
        StellarPreserve => "Stellar Preserve",
        SynthesisCollective => "Synthesis Collective",
        BrokenChoir => "Broken Choir",
        CorpseStars => "Corpse Stars",
        TheSilence => "The Silence",
        TerranHegemony => "Terran Hegemony",
        GildedThrone => "Gilded Throne",
        RecursiveKingdom => "Recursive Kingdom",
        EternalHegemony => "Eternal Hegemony",
        Shepherd => "The Shepherd",
    }
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
    /// Reinforcement aggro: when a same-faction ally within earshot calls for
    /// backup (ai_distress_system), this is set to whoever THEY'RE fighting
    /// and alert_timer counts down. The brain treats a live alert as a
    /// high-priority reason to converge and engage that target even for a
    /// ship that wouldn't have picked the fight on its own — that's how a
    /// whole patrol/nest piles onto the player once one of them is in a
    /// scrap. Decays so summoned ships eventually return to their patrol.
    pub alert_target: Option<Entity>,
    pub alert_timer: f32,
    /// Seconds until this ship can broadcast another distress call — keeps a
    /// ship mid-fight from screaming for help every single frame.
    pub distress_cooldown: f32,
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
            alert_target: None,
            alert_timer: 0.0,
            distress_cooldown: 0.0,
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
/// because destination is often an OFFSET from the target (Gilded Throne's
/// flank position, Corpse Stars's ram-from-above point), not the target's
/// actual position. Weapons fire at `position`; movement still uses
/// AiShipNav.destination. Recomputed every brain tick (0.25s) in
/// ai_brain::ai_brain_system via a faction-agnostic distance/value scoring
/// pass over the player + every other living AI ship — see that file's
/// doc comment for which factions actually use it vs. staying player-only.
#[derive(Component, Default)]
pub struct AiShipTarget {
    pub entity: Option<Entity>,
    pub position: Vec2,
    /// The specific enemy module this ship is trying to shoot OUT — a weapon,
    /// engine, or reactor, chosen by faction doctrine (ai_ship::combat's
    /// aim_priority). Weapons aim at this block's live world position instead
    /// of the hull centroid, so a tactical faction can methodically disable
    /// your guns / strand your drive rather than just grinding the hull.
    /// Cleared when the target entity changes (ai_brain) and re-picked lazily
    /// (ai_weapon_fire_system) when the current pick is destroyed.
    pub subsystem: Option<Entity>,
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
    /// Which star system this ship belongs to — gates whether
    /// tick_world_simulation actually ticks it (Hot/Warm systems only, see
    /// SystemStreamingManager) or leaves it frozen (Cold). 0 = Haven.
    pub system_id: u32,
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
    ///
    /// `active_systems` restricts candidates to ships in the Hot/Warm
    /// systems (SystemStreamingManager) — since ships now belong to
    /// different star systems, a contract tagging a ship in some Cold
    /// system the player has never been near would be an unreachable
    /// bounty with no way to even see it on the current map.
    pub fn tag_bounty_target(&mut self, faction: AiShipType, active_systems: &[u32], rng: &mut impl rand::Rng) -> Option<(u32, f32)> {
        let unspawned: Vec<usize> = self.ships.iter().enumerate()
            .filter(|(_, s)| s.faction == faction && s.behavior != SimBehavior::Dead && s.bounty_id.is_none() && !s.spawned && active_systems.contains(&s.system_id))
            .map(|(i, _)| i)
            .collect();
        let candidates = if !unspawned.is_empty() {
            unspawned
        } else {
            self.ships.iter().enumerate()
                .filter(|(_, s)| s.faction == faction && s.behavior != SimBehavior::Dead && s.bounty_id.is_none() && active_systems.contains(&s.system_id))
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
    /// How many vessels this faction keeps in one of its systems.
    ///
    /// Density, not headcount, is what the player feels. Ships are scattered
    /// over `radius * 0.8` and only become real entities inside the 10,000-unit
    /// materialisation bubble (ai_ship::simulation::RENDER_DISTANCE), so the
    /// count that matters is `ship_count * (10_000 / (0.8 * radius))^2`.
    ///
    /// These were tuned when the whole game was ONE system and every faction
    /// ship shared it with the player. The multi-system galaxy then spread the
    /// same handful across ~30 systems, which dropped most territories below
    /// ONE expected contact -- the void went quiet. Current values target 2-6
    /// contacts inside the bubble.
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
        // Recursive Kingdom - shallow scrapyards, closest to spawn
        FactionTerritory {
            faction: AiShipType::RecursiveKingdom,
            center: Vec2::new(30_000.0, -10_000.0),
            radius: 15_000.0,
            ship_count: 8, // many small ships
        },
        // Stellar Preserve - shallow preserves, where the tame things are kept
        FactionTerritory {
            faction: AiShipType::StellarPreserve,
            center: Vec2::new(-45_000.0, -20_000.0),
            radius: 14_000.0,
            ship_count: 4,
        },
        // Synthesis Collective - mid-depth, where flesh and machine were first married
        FactionTerritory {
            faction: AiShipType::SynthesisCollective,
            center: Vec2::new(60_000.0, -50_000.0),
            radius: 16_000.0,
            ship_count: 6,
        },
        // The Silence - everywhere, lurking
        FactionTerritory {
            faction: AiShipType::TheSilence,
            center: Vec2::new(-85_000.0, -55_000.0),
            radius: 18_000.0,
            ship_count: 5,
        },
        // Gilded Throne PMC - mid-depth patrol routes, out toward the outer system
        FactionTerritory {
            faction: AiShipType::GildedThrone,
            center: Vec2::new(60_000.0, -180_000.0),
            radius: 18_000.0,
            ship_count: 6,
        },
        // The Broken Choir - scattered everywhere, no home (biggest spread)
        FactionTerritory {
            faction: AiShipType::BrokenChoir,
            center: Vec2::new(-60_000.0, -200_000.0),
            radius: 24_000.0,
            ship_count: 8,
        },
        // Terran Hegemony - deep military zone, far outer system
        FactionTerritory {
            faction: AiShipType::TerranHegemony,
            center: Vec2::new(150_000.0, -250_000.0),
            radius: 18_000.0,
            ship_count: 4, // rare but powerful
        },
        // Corpse Stars - deep zone only, farthest out of the "normal"
        // factions (still a real gap before the star itself at ~492k, which
        // stays a distant endgame destination rather than just another
        // territory)
        FactionTerritory {
            faction: AiShipType::CorpseStars,
            center: Vec2::new(-140_000.0, -320_000.0),
            radius: 35_000.0,
            ship_count: 3,
        },
        // Eternal Hegemony — one lone mega-battleship, patrolling well past the
        // star system. Finding it at all is most of the challenge.
        FactionTerritory {
            faction: AiShipType::EternalHegemony,
            center: Vec2::new(400_000.0, -420_000.0), // ~580k out
            radius: 60_000.0,
            ship_count: 1,
        },
        // The Shepherd — the single hardest kill in the game, sitting beyond
        // everything else in explored space.
        FactionTerritory {
            faction: AiShipType::Shepherd,
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
        AiShipType::Shepherd => 8.0,      // the hardest kill in the game
        AiShipType::EternalHegemony => 6.0,    // mega-battleship
        AiShipType::TerranHegemony => 3.0,      // Battleship - strongest "normal" faction
        AiShipType::GildedThrone => 2.0,     // Elite mercs
        AiShipType::CorpseStars => 2.5,   // Heavy armor + weapons
        AiShipType::SynthesisCollective => 1.5,    // Bio-weapons
        AiShipType::StellarPreserve => 1.2,      // Creature + some weapons
        AiShipType::BrokenChoir => 1.0,        // Already damaged
        AiShipType::RecursiveKingdom => 0.5,      // Weak individually
        AiShipType::TheSilence => 0.1,       // No weapons
    }
}

/// Per-faction color for the galaxy map — a visited system's pip is colored
/// by whose territory it is, not by danger_tier (that's directly derivable
/// from the faction anyway once you know it, and a real faction identity is
/// more useful than a 4-bucket threat color once you're familiar with the
/// roster). Deliberately a different, brighter palette than
/// ai_ship::spawner::ship_tint — that one's tuned for a ship sprite lit at
/// combat distance, several of those hues (Gilded Throne, Corpse Stars) are
/// near-black and would be invisible as a small flat map pip.
pub fn faction_map_color(faction: AiShipType) -> bevy::prelude::Color {
    use bevy::prelude::Color;
    match faction {
        AiShipType::Shepherd => Color::srgb(1.0, 0.85, 0.2),      // bright gold
        AiShipType::EternalHegemony => Color::srgb(0.95, 0.2, 0.2),    // crimson
        AiShipType::StellarPreserve => Color::srgb(0.25, 0.85, 0.75),    // teal
        AiShipType::SynthesisCollective => Color::srgb(0.75, 0.35, 0.95),  // purple
        AiShipType::BrokenChoir => Color::srgb(0.6, 0.8, 0.65),        // pale gray-green
        AiShipType::CorpseStars => Color::srgb(0.55, 0.35, 0.9),  // violet
        AiShipType::TheSilence => Color::srgb(0.9, 0.92, 0.95),      // near-white
        AiShipType::TerranHegemony => Color::srgb(0.65, 0.7, 0.8),       // steel blue-gray
        AiShipType::GildedThrone => Color::srgb(0.4, 0.45, 0.65),    // slate blue
        AiShipType::RecursiveKingdom => Color::srgb(0.95, 0.55, 0.2),     // rusty orange
    }
}

/// Fraction of a faction's crew-eligible stations (Reactor/Engine/Weapon/
/// etc — anything the module registry marks crew_station:true) that actually
/// get a warm body. auto_assign_crew fills stations by priority (Power, then
/// Propulsion, then Weapons last), so at/above 1.0 every station — guns
/// included — is manned, and the surplus above 1.0 is spare hands ready to
/// backfill a gun when its crewman dies. Kept as a per-faction number so a
/// disciplined battleship still runs deeper reserves than a scrappy raider,
/// but every faction now sits at 1.0+ so their guns actually fire: the old
/// sub-1.0 values (Recursive Kingdom 0.6, Broken Choir 0.55) left weak factions with half
/// their guns permanently dark, which read in play as "enemies that don't
/// always shoot." This is deliberately separate from faction_power (a
/// combat-strength RATING). Weak factions stay weak through lower faction_power
/// (thinner hull, less damage) — not by leaving their weapons unstaffed.
pub fn crew_fill_fraction(faction: AiShipType) -> f32 {
    match faction {
        AiShipType::Shepherd => 1.6,      // apex predator, crew to spare
        AiShipType::EternalHegemony => 1.5,
        AiShipType::TerranHegemony => 1.4,       // disciplined battleship, deep reserves
        AiShipType::GildedThrone => 1.3,     // tight professional crew
        AiShipType::CorpseStars => 1.3,
        AiShipType::SynthesisCollective => 1.25,   // reckless zealots, fully manned guns
        AiShipType::StellarPreserve => 1.2,
        AiShipType::TheSilence => 1.1,       // skeleton crew (no weapons anyway)
        AiShipType::RecursiveKingdom => 1.15,     // scrappy, but now mans all its guns
        AiShipType::BrokenChoir => 1.1,        // ghost ship, still staffs the guns
    }
}

/// Small AI-only derate on reactor power generation for the factions
/// already flagged weakest by crew_fill_fraction (Recursive Kingdom, Broken Choir,
/// The Silence). Applied in ai_ship::power's per-ship BFS, NOT the shared
/// building::registry ModuleDef.power_generation the player's own ships
/// also read from — so this can't touch player or other-faction balance.
/// Deliberately mild: at full health every faction still runs a healthy
/// positive balance (this isn't meant to silence guns on its own), it just
/// thins the margin so reactor damage — which already reduces generation
/// via efficiency — tips these factions into hold-fire sooner than a
/// faction with a fat multi-x buffer.
pub fn power_output_multiplier(faction: AiShipType) -> f32 {
    match faction {
        AiShipType::RecursiveKingdom => 0.85,
        AiShipType::BrokenChoir => 0.8,
        AiShipType::TheSilence => 0.85,
        _ => 1.0,
    }
}

/// Whether a faction actually engages in ship-to-ship combat. The Silence never
/// attacks (silent stalkers); Stellar Preserve riders flee rather than fight. Every
/// other faction does. Used to gate distress broadcast/response so a
/// non-combatant never answers — or issues — a call for backup it would never
/// act on anyway.
pub fn faction_fights(faction: AiShipType) -> bool {
    !matches!(faction, AiShipType::TheSilence | AiShipType::StellarPreserve)
}

/// Returns whether two factions are hostile to each other
pub fn factions_hostile(a: AiShipType, b: AiShipType) -> bool {
    use AiShipType::*;
    if a == b { return false; } // same faction = allies
    match (a, b) {
        // Bosses are hostile to everything, including each other — rampaging
        // apex threats, not aligned with any faction's politics.
        (Shepherd, _) | (_, Shepherd) => true,
        (EternalHegemony, _) | (_, EternalHegemony) => true,
        // Synthesis Collective attacks Stellar Preserve (rival claims on what life is for)
        (SynthesisCollective, StellarPreserve) | (StellarPreserve, SynthesisCollective) => true,
        // Terran Hegemony attacks everyone except Gilded Throne (allied mercs)
        (TerranHegemony, GildedThrone) | (GildedThrone, TerranHegemony) => false,
        (TerranHegemony, _) | (_, TerranHegemony) => true,
        // Gilded Throne hunts pirates (Recursive Kingdom) and the Collective
        (GildedThrone, RecursiveKingdom) | (RecursiveKingdom, GildedThrone) => true,
        (GildedThrone, SynthesisCollective) | (SynthesisCollective, GildedThrone) => true,
        // Recursive Kingdom attacks everyone weaker
        (RecursiveKingdom, TheSilence) | (TheSilence, RecursiveKingdom) => true,
        (RecursiveKingdom, StellarPreserve) | (StellarPreserve, RecursiveKingdom) => true,
        // Corpse Stars attack anyone in deep zone
        (CorpseStars, _) | (_, CorpseStars) => true,
        // Broken Choir attack everything (no consensus left to negotiate with)
        (BrokenChoir, _) | (_, BrokenChoir) => true,
        // The Silence never attacks
        (TheSilence, _) | (_, TheSilence) => false,
        _ => false,
    }
}

/// A ship whose last reactor has been breached. It has MELTDOWN_SECONDS
/// (ai_ship::combat) left: shield down, guns still hot off the capacitors,
/// then it detonates. See combat::tick_reactor_meltdown.
#[derive(Component)]
pub struct ReactorMeltdown {
    pub remaining: f32,
}

/// What a ship has learned from watching its own rounds come off you.
///
/// Counts ricochets against the current target. Past a threshold the crew
/// concludes the angle is the problem and changes what they're loading —
/// which is the difference between an enemy that keeps failing the same way
/// and one that adapts. Reset when they switch targets, so learning doesn't
/// carry across to a ship they haven't studied.
#[derive(Component, Default, Debug)]
pub struct AiGunneryLog {
    pub ricochets: u32,
    pub switched: bool,
}

/// Ricochets against one target before the crew changes ammunition.
pub const RICOCHETS_BEFORE_SWITCH: u32 = 4;
