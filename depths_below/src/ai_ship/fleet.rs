//! Fleets: ships that travel, fight and spawn together.
//!
//! A system used to be populated with loners, pairs and packs of random
//! classes: a capital flew alone as often as not, a hauler fought like a
//! warship, and nothing guarded a station. Now a faction fields navies:
//!
//! - **Patrol**: one to four scouts and line ships.
//! - **Battle group**: a capital (or the boss) with line escorts.
//! - **Convoy**: haulers running between two points, with a guard.
//! - **Station guard**: line ships holding round the faction's station.
//! - **Raiders**: a fast pack of scouts that ranges wide.
//!
//! Off-screen a fleet is one point that moves; its members keep their
//! formation slots round it. It becomes real all at once, inside the block
//! budget or not at all, and goes back the same way. On screen the escorts
//! fly their slots on the leader, the whole fleet answers when one of them
//! is attacked, and civilian hulls run instead of fighting.

use bevy::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};

use super::components::{AiShipType, SimBehavior, SimulatedShip};
use super::roster::{self, ClassRole};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FleetKind {
    Patrol,
    BattleGroup,
    Convoy,
    StationGuard,
    Raiders,
}

/// A fleet in the world simulation. Its members are the ships whose `fleet`
/// is this id.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimFleet {
    pub id: u32,
    pub kind: FleetKind,
    pub faction: AiShipType,
    pub system_id: u32,
    pub position: Vec2,
    pub velocity: Vec2,
    /// Which way the formation faces, radians. Kept when the fleet stops, so
    /// a halted formation doesn't spin.
    pub heading: f32,
    pub destination: Vec2,
    pub home_zone: Vec2,
    pub patrol_radius: f32,
    pub cruise: f32,
    /// A convoy's run: it flies these in turn, out and back. Empty for
    /// everyone else.
    #[serde(default)]
    pub route: Vec<Vec2>,
    #[serde(default)]
    pub leg: usize,
}

impl SimFleet {
    pub fn new(kind: FleetKind, faction: AiShipType, system_id: u32, position: Vec2, home_zone: Vec2, patrol_radius: f32, cruise: f32) -> Self {
        SimFleet {
            id: 0,
            kind,
            faction,
            system_id,
            position,
            velocity: Vec2::ZERO,
            heading: 0.0,
            destination: position,
            home_zone,
            patrol_radius,
            cruise,
            route: Vec::new(),
            leg: 0,
        }
    }

    /// Where a member with this slot sits in the formation right now.
    pub fn slot_position(&self, slot: Vec2) -> Vec2 {
        self.position + Vec2::from_angle(self.heading).rotate(slot)
    }
}

/// The ship flies with a fleet: its slot on the leader, and whether it is the
/// leader. Live ships only; the simulation keeps the same on SimulatedShip.
#[derive(Component, Clone, Copy, Debug)]
pub struct FleetMember {
    pub fleet: u32,
    pub slot: Vec2,
    pub leader: bool,
}

/// A hauler, liner or freighter: it runs from a fight instead of joining it.
#[derive(Component, Clone, Copy, Debug)]
pub struct CivilianHull;

/// How often each kind of fleet turns up for a faction, as its lore has it.
/// A kind the faction's roster can't crew (a convoy with no haulers) is
/// skipped when it's rolled.
fn kind_weights(faction: AiShipType) -> &'static [(FleetKind, u32)] {
    use AiShipType::*;
    use FleetKind::*;
    match faction {
        // A line navy: patrols, battle groups, and the convoys that feed them.
        TerranHegemony => &[(Patrol, 35), (BattleGroup, 15), (Convoy, 20), (StationGuard, 15)],
        // Colossal and slow: the dreadnought never travels without its echoes.
        EternalHegemony => &[(Patrol, 55), (BattleGroup, 45)],
        // Mercenaries and bullion: raiders for hire, freighters under guard.
        GildedThrone => &[(Patrol, 30), (BattleGroup, 10), (Convoy, 25), (StationGuard, 15), (Raiders, 20)],
        BrokenChoir => &[(Patrol, 45), (BattleGroup, 15), (StationGuard, 15), (Raiders, 25)],
        SynthesisCollective => &[(Patrol, 45), (BattleGroup, 15), (StationGuard, 15), (Raiders, 25)],
        CorpseStars => &[(Patrol, 40), (BattleGroup, 20), (StationGuard, 20), (Raiders, 20)],
        StellarPreserve => &[(Patrol, 60), (BattleGroup, 20), (StationGuard, 20)],
        TheSilence => &[(Patrol, 70), (StationGuard, 30)],
        Shepherd => &[(Patrol, 60), (BattleGroup, 40)],
        // Scrap swarms: everything they field is a pack.
        RecursiveKingdom => &[(Patrol, 40), (Raiders, 60)],
    }
}

/// How fast a kind of fleet cruises off-screen. A fleet goes at its slowest
/// member's pace, so a battle group crawls and raiders run.
fn cruise_for(kind: FleetKind, rng: &mut impl Rng) -> f32 {
    match kind {
        FleetKind::Patrol => rng.gen_range(80.0..120.0),
        FleetKind::BattleGroup => rng.gen_range(50.0..70.0),
        FleetKind::Convoy => rng.gen_range(55.0..75.0),
        FleetKind::StationGuard => rng.gen_range(45.0..60.0),
        FleetKind::Raiders => rng.gen_range(110.0..140.0),
    }
}

/// How many of each capped role a system holds so far.
pub type Held = Vec<(ClassRole, usize)>;

fn held_count(held: &Held, role: ClassRole) -> usize {
    held.iter().find(|(r, _)| *r == role).map_or(0, |(_, n)| *n)
}

fn hold(held: &mut Held, role: ClassRole) {
    match held.iter_mut().find(|(r, _)| *r == role) {
        Some((_, n)) => *n += 1,
        None => held.push((role, 1)),
    }
}

/// The role a ship plays: its class's, or Line for a faction's original design.
pub fn role_of(faction: AiShipType, class: Option<&'static str>) -> ClassRole {
    class.and_then(|k| roster::find(faction, k)).map_or(ClassRole::Line, |c| c.role)
}

/// One hull in one of `roles`, under the system's caps. `Some(None)` is the
/// faction's original design: what a faction with no roster flies, and what
/// stands in for a heavy (`heavy`) or line hull when the roster still keeps
/// its original in rotation. None: nothing fits.
fn pick(
    faction: AiShipType,
    roles: &[ClassRole],
    rng: &mut impl Rng,
    held: &mut Held,
) -> Option<Option<&'static str>> {
    let ships = roster::roster(faction);
    if ships.is_empty() {
        return Some(None);
    }
    let fits: Vec<_> = ships
        .iter()
        .filter(|c| roles.contains(&c.role) && held_count(held, c.role) < c.role.per_system_cap())
        .collect();
    let total: u32 = fits.iter().map(|c| c.weight).sum();
    if total > 0 {
        let mut roll = rng.gen_range(0..total);
        for c in fits {
            if roll < c.weight {
                hold(held, c.role);
                return Some(Some(c.key));
            }
            roll -= c.weight;
        }
    }
    // A faction still flying its original hull alongside a partial roster:
    // the original is its heavy (the Eternal dreadnought) or a line ship.
    if roster::original_weight(faction) > 0 {
        if roles.contains(&ClassRole::Boss) && held_count(held, ClassRole::Boss) < ClassRole::Boss.per_system_cap() {
            hold(held, ClassRole::Boss);
            return Some(None);
        }
        if roles.contains(&ClassRole::Line) {
            return Some(None);
        }
    }
    None
}

/// The first of `choices` that yields a hull.
fn pick_any(
    faction: AiShipType,
    choices: &[&[ClassRole]],
    rng: &mut impl Rng,
    held: &mut Held,
) -> Option<Option<&'static str>> {
    choices.iter().find_map(|roles| pick(faction, roles, rng, held))
}

/// The hulls of one fleet of `kind`, leader first. None when the faction
/// can't crew it (no haulers for a convoy, no capital left for a battle
/// group under the system's caps).
pub fn compose(
    faction: AiShipType,
    kind: FleetKind,
    rng: &mut impl Rng,
    held: &mut Held,
) -> Option<Vec<Option<&'static str>>> {
    use ClassRole::*;
    let mut members = Vec::new();
    // Escorts are picked against a scratch copy of the caps, then committed:
    // a refused fleet must not use up the system's capital allowance.
    let mut trial = held.clone();
    match kind {
        FleetKind::Patrol => {
            let size = match rng.gen_range(0..100) {
                0..=19 => 1,
                20..=54 => 2,
                55..=84 => 3,
                _ => 4,
            };
            for _ in 0..size {
                members.push(pick_any(faction, &[&[Scout, Line, Experimental], &[Scout, Line]], rng, &mut trial)?);
            }
        }
        FleetKind::BattleGroup => {
            members.push(pick(faction, &[Boss, Capital], rng, &mut trial)?);
            for _ in 0..rng.gen_range(2..=4) {
                if let Some(m) = pick_any(faction, &[&[Line, Experimental], &[Line], &[Scout]], rng, &mut trial) {
                    members.push(m);
                }
            }
            if rng.gen_bool(0.5) {
                if let Some(m) = pick(faction, &[Scout], rng, &mut trial) {
                    members.push(m);
                }
            }
        }
        FleetKind::Convoy => {
            // The haulers must be real civilians, not a stand-in hull.
            if roster::roster(faction).iter().all(|c| c.role != Civilian) {
                return None;
            }
            for _ in 0..rng.gen_range(2..=3) {
                members.push(pick(faction, &[Civilian], rng, &mut trial)?);
            }
            for _ in 0..rng.gen_range(1..=2) {
                if let Some(m) = pick_any(faction, &[&[Line], &[Scout]], rng, &mut trial) {
                    members.push(m);
                }
            }
        }
        FleetKind::StationGuard => {
            for _ in 0..rng.gen_range(2..=4) {
                members.push(pick_any(faction, &[&[Line], &[Scout, Line, Experimental]], rng, &mut trial)?);
            }
        }
        FleetKind::Raiders => {
            for _ in 0..rng.gen_range(3..=5) {
                members.push(pick_any(faction, &[&[Scout], &[Line]], rng, &mut trial)?);
            }
        }
    }
    // Biggest first, so the leader is the hull the rest form up on -- except
    // where the leader is the point: the capital, or a convoy's lead hauler.
    if matches!(kind, FleetKind::Patrol | FleetKind::StationGuard | FleetKind::Raiders) {
        members.sort_by_key(|&c| std::cmp::Reverse(super::spawner::design_blocks(faction, c)));
    }
    *held = trial;
    Some(members)
}

/// Roll a kind of fleet the faction fields, and crew it. Tries the other
/// kinds when the roll can't be crewed; a Patrol always can.
pub fn roll_fleet(
    faction: AiShipType,
    rng: &mut impl Rng,
    held: &mut Held,
    allow: impl Fn(FleetKind) -> bool,
) -> Option<(FleetKind, Vec<Option<&'static str>>)> {
    let mut kinds: Vec<(FleetKind, u32)> = kind_weights(faction).iter().copied().filter(|(k, _)| allow(*k)).collect();
    while !kinds.is_empty() {
        let total: u32 = kinds.iter().map(|(_, w)| w).sum();
        let mut roll = rng.gen_range(0..total.max(1));
        let mut chosen = 0;
        for (i, (_, w)) in kinds.iter().enumerate() {
            if roll < *w {
                chosen = i;
                break;
            }
            roll -= w;
        }
        let (kind, _) = kinds.remove(chosen);
        if let Some(members) = compose(faction, kind, rng, held) {
            return Some((kind, members));
        }
    }
    None
}

/// Clear water between neighbours in a formation, past both hulls.
const FORMATION_GAP: f32 = 500.0;

/// Formation slots for hulls of these extents, leader first at the centre.
/// A swept wedge: the escorts fall back and out on alternate flanks, each
/// one's slot clear of the hull beside it on that flank.
pub fn formation(extents: &[f32]) -> Vec<Vec2> {
    let Some(&lead) = extents.first() else { return Vec::new() };
    let mut slots = vec![Vec2::ZERO];
    // How far out each flank already reaches.
    let mut reach = [lead, lead];
    for (i, &ext) in extents.iter().enumerate().skip(1) {
        let side = (i - 1) % 2;
        let lateral = reach[side] + FORMATION_GAP + ext;
        reach[side] = lateral + ext;
        let sign = if side == 0 { 1.0 } else { -1.0 };
        slots.push(Vec2::new(-lateral * 0.5, sign * lateral));
    }
    slots
}

/// Build a fleet's members at `position`, slotted into formation. The caller
/// adds them with `WorldSimulation::add_fleet`.
pub fn crew(
    fleet: &SimFleet,
    classes: &[Option<&'static str>],
) -> Vec<SimulatedShip> {
    let extents: Vec<f32> = classes.iter().map(|&c| super::spawner::design_extent(fleet.faction, c)).collect();
    formation(&extents)
        .into_iter()
        .zip(classes)
        .map(|(slot, &class)| {
            let mut ship = SimulatedShip::patrolling(
                fleet.system_id,
                fleet.faction,
                fleet.position,
                fleet.home_zone,
                fleet.patrol_radius,
                fleet.cruise,
            );
            ship.class = class;
            ship.slot = slot;
            ship
        })
        .collect()
}

/// A fleet, built and named for its kind: cruise speed rolled, members
/// slotted. Returns None when the faction can't crew a fleet at all.
pub fn new_fleet(
    faction: AiShipType,
    kind: FleetKind,
    classes: &[Option<&'static str>],
    system_id: u32,
    position: Vec2,
    home_zone: Vec2,
    patrol_radius: f32,
    rng: &mut impl Rng,
) -> (SimFleet, Vec<SimulatedShip>) {
    let mut fleet = SimFleet::new(kind, faction, system_id, position, home_zone, patrol_radius, cruise_for(kind, rng));
    fleet.heading = rng.gen_range(0.0..std::f32::consts::TAU);
    let members = crew(&fleet, classes);
    (fleet, members)
}

/// How much fight a ship has in it off-screen: its size, what's left of it,
/// and whether it's built to fight at all.
pub fn strength(ship: &SimulatedShip) -> f32 {
    let role = match role_of(ship.faction, ship.class) {
        ClassRole::Civilian => 0.15,
        _ => 1.0,
    };
    let faction = match ship.faction {
        AiShipType::TheSilence => 0.05,
        AiShipType::StellarPreserve => 0.5,
        _ => 1.0,
    };
    super::spawner::design_blocks(ship.faction, ship.class) as f32 * ship.health * role * faction
}

/// Seconds for two evenly matched groups to wear each other out off-screen.
const BATTLE_SECONDS: f32 = 90.0;
/// How close two groups must come (past their own spread) to fight.
const BATTLE_RANGE: f32 = 2_500.0;

/// One side in an off-screen fight: a fleet's living, off-screen members, or
/// a ship that flies alone.
struct Side {
    faction: AiShipType,
    center: Vec2,
    spread: f32,
    members: Vec<usize>,
}

/// Off-screen fights, weighed by strength.
///
/// Every pair of hostile groups within range trade damage for `dt`: each side
/// deals its strength, and the damage lands on the other side's smallest
/// hulls first (escorts screen their capital). A Lancer that meets a
/// Sovereign's battle group is gone in a tick or two; two even patrols wear
/// each other down over a minute and a half. It used to be a coin flip
/// between any two ships within 500 units, whatever their size.
pub fn resolve_battles(ships: &mut [SimulatedShip], active: impl Fn(u32) -> bool, dt: f32) {
    let mut sides: Vec<Side> = Vec::new();
    let mut by_fleet: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for (i, s) in ships.iter().enumerate() {
        if s.spawned || s.behavior == SimBehavior::Dead || !active(s.system_id) {
            continue;
        }
        let side = if s.fleet != 0 {
            *by_fleet.entry(s.fleet).or_insert_with(|| {
                sides.push(Side { faction: s.faction, center: Vec2::ZERO, spread: 0.0, members: Vec::new() });
                sides.len() - 1
            })
        } else {
            sides.push(Side { faction: s.faction, center: Vec2::ZERO, spread: 0.0, members: Vec::new() });
            sides.len() - 1
        };
        sides[side].members.push(i);
    }
    for side in &mut sides {
        let n = side.members.len() as f32;
        side.center = side.members.iter().map(|&i| ships[i].position).sum::<Vec2>() / n;
        side.spread = side.members.iter().map(|&i| ships[i].position.distance(side.center)).fold(0.0, f32::max);
    }

    // Damage each side takes this tick (in blocks), and from whom.
    let mut incoming = vec![0.0_f32; sides.len()];
    let mut foe: Vec<Option<usize>> = vec![None; sides.len()];
    for a in 0..sides.len() {
        for b in (a + 1)..sides.len() {
            if !super::components::factions_hostile(sides[a].faction, sides[b].faction) {
                continue;
            }
            let reach = BATTLE_RANGE + sides[a].spread + sides[b].spread;
            if sides[a].center.distance(sides[b].center) > reach {
                continue;
            }
            let power = |s: &Side| s.members.iter().map(|&i| strength(&ships[i])).sum::<f32>();
            incoming[b] += power(&sides[a]) * dt / BATTLE_SECONDS;
            incoming[a] += power(&sides[b]) * dt / BATTLE_SECONDS;
            foe[a] = foe[a].or(Some(sides[b].members[0]));
            foe[b] = foe[b].or(Some(sides[a].members[0]));
        }
    }

    for (k, side) in sides.iter().enumerate() {
        let Some(opponent) = foe[k] else { continue };
        let mut damage = incoming[k];
        let mut order = side.members.clone();
        order.sort_by_key(|&i| super::spawner::design_blocks(ships[i].faction, ships[i].class));
        for i in order {
            let ship = &mut ships[i];
            ship.behavior = SimBehavior::Fighting(opponent);
            if damage <= 0.0 {
                continue;
            }
            let blocks = super::spawner::design_blocks(ship.faction, ship.class).max(1) as f32;
            // Bounty targets never die to off-screen combat: a contract on a
            // ship a rival quietly killed could never be finished.
            let floor = if ship.bounty_id.is_some() { 0.05 } else { 0.0 };
            let lost = (damage / blocks).min(ship.health - floor).max(0.0);
            ship.health -= lost;
            damage -= lost * blocks;
            if ship.health <= 0.0 {
                ship.health = 0.0;
                ship.behavior = SimBehavior::Dead;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formation_slots_never_overlap() {
        let extents = [2_400.0, 300.0, 900.0, 300.0, 1_200.0, 600.0];
        let slots = formation(&extents);
        assert_eq!(slots.len(), extents.len());
        assert_eq!(slots[0], Vec2::ZERO);
        for i in 0..slots.len() {
            for j in (i + 1)..slots.len() {
                let gap = slots[i].distance(slots[j]) - extents[i] - extents[j];
                assert!(gap > 0.0, "slots {i} and {j} overlap by {}", -gap);
            }
        }
    }

    #[test]
    fn a_battle_group_is_led_by_its_heavy() {
        let mut rng = rand::thread_rng();
        for _ in 0..30 {
            let mut held = Held::new();
            let members = compose(AiShipType::TerranHegemony, FleetKind::BattleGroup, &mut rng, &mut held)
                .expect("the Hegemony can always crew a battle group in an empty system");
            assert_eq!(members[0], Some("sovereign"));
            assert!(members.len() >= 3, "a capital never travels alone: {members:?}");
        }
    }

    #[test]
    fn caps_hold_across_fleets() {
        let mut rng = rand::thread_rng();
        let mut held = Held::new();
        let mut capitals = 0;
        for _ in 0..20 {
            if let Some(members) = compose(AiShipType::TerranHegemony, FleetKind::BattleGroup, &mut rng, &mut held) {
                capitals += members.iter().filter(|&&c| role_of(AiShipType::TerranHegemony, c) == ClassRole::Capital).count();
            }
        }
        assert_eq!(capitals, ClassRole::Capital.per_system_cap());
    }

    #[test]
    fn a_convoy_is_haulers_first_and_guarded() {
        let mut rng = rand::thread_rng();
        for _ in 0..30 {
            let mut held = Held::new();
            let m = compose(AiShipType::TerranHegemony, FleetKind::Convoy, &mut rng, &mut held).unwrap();
            assert_eq!(role_of(AiShipType::TerranHegemony, m[0]), ClassRole::Civilian);
            assert!(m.iter().any(|&c| role_of(AiShipType::TerranHegemony, c) != ClassRole::Civilian), "{m:?}");
        }
        let mut held = Held::new();
        assert!(compose(AiShipType::CorpseStars, FleetKind::Convoy, &mut rng, &mut held).is_none());
    }

    #[test]
    fn every_faction_can_field_a_fleet() {
        let mut rng = rand::thread_rng();
        for faction in [
            AiShipType::StellarPreserve, AiShipType::SynthesisCollective, AiShipType::BrokenChoir,
            AiShipType::CorpseStars, AiShipType::TheSilence, AiShipType::TerranHegemony,
            AiShipType::GildedThrone, AiShipType::RecursiveKingdom, AiShipType::EternalHegemony,
            AiShipType::Shepherd,
        ] {
            let mut held = Held::new();
            let fleet = roll_fleet(faction, &mut rng, &mut held, |_| true);
            assert!(fleet.is_some_and(|(_, m)| !m.is_empty()), "{faction:?}");
        }
    }

    fn ship_at(faction: AiShipType, class: Option<&'static str>, fleet: u32, x: f32) -> SimulatedShip {
        let mut s = SimulatedShip::patrolling(1, faction, Vec2::new(x, 0.0), Vec2::ZERO, 10_000.0, 80.0);
        s.class = class;
        s.fleet = fleet;
        s
    }

    #[test]
    fn the_stronger_side_wins_off_screen() {
        // A lone Lancer against a Sovereign and two Bulwarks.
        let mut ships = vec![
            ship_at(AiShipType::RecursiveKingdom, None, 0, 0.0),
            ship_at(AiShipType::TerranHegemony, Some("sovereign"), 7, 1_500.0),
            ship_at(AiShipType::TerranHegemony, Some("bulwark"), 7, 3_000.0),
            ship_at(AiShipType::TerranHegemony, Some("bulwark"), 7, 3_500.0),
        ];
        assert!(super::super::components::factions_hostile(ships[0].faction, ships[1].faction));
        for _ in 0..10 {
            resolve_battles(&mut ships, |_| true, 2.0);
        }
        assert_eq!(ships[0].behavior, SimBehavior::Dead, "the loner should be gone");
        assert!(ships[1..].iter().all(|s| s.behavior != SimBehavior::Dead), "the battle group should stand");
        // Escorts screen the capital: it's the least hurt.
        assert!(ships[1].health >= ships[2].health.min(ships[3].health));
    }

    #[test]
    fn far_apart_groups_do_not_fight() {
        let mut ships = vec![
            ship_at(AiShipType::RecursiveKingdom, None, 0, 0.0),
            ship_at(AiShipType::TerranHegemony, Some("bulwark"), 0, 50_000.0),
        ];
        resolve_battles(&mut ships, |_| true, 2.0);
        assert!(ships.iter().all(|s| s.health == 1.0));
    }

    /// Ten minutes of a running fight between two big fleets: health stays a
    /// number between 0 and 1, the dead stay dead, and it ends.
    #[test]
    fn a_long_fight_stays_sane() {
        let mut ships = Vec::new();
        for (i, class) in ["sovereign", "bulwark", "warden", "lancer", "lancer"].iter().enumerate() {
            ships.push(ship_at(AiShipType::TerranHegemony, roster::intern(class), 1, i as f32 * 300.0));
        }
        for i in 0..6 {
            ships.push(ship_at(AiShipType::CorpseStars, roster::intern("mourner"), 2, 1_000.0 + i as f32 * 300.0));
        }
        for _ in 0..300 {
            resolve_battles(&mut ships, |_| true, 2.0);
            for s in &ships {
                assert!(s.health.is_finite() && (0.0..=1.0).contains(&s.health), "health {}", s.health);
                if s.behavior == SimBehavior::Dead {
                    assert_eq!(s.health, 0.0);
                }
            }
        }
        let alive = |f: u32| ships.iter().any(|s| s.fleet == f && s.behavior != SimBehavior::Dead);
        assert!(!(alive(1) && alive(2)), "ten minutes in, both fleets still stand");
    }
}
