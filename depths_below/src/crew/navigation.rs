//! Interior navigation: which ship-local cells crew can walk, and how to get
//! between two of them.
//!
//! Crew walk on hallways and nothing else. `HullLayer::Hallway` is the only
//! surface that carries them; the outer shell, the structural `Inner` hull and
//! the void between hulls are all things you route AROUND. That makes where
//! the hallways go a real design decision — a post with no hallway route to it
//! cannot be manned, however much floor happens to surround it.
//!
//! The exception is the posts themselves. Crew have to stand somewhere to do
//! their job, so a module they occupy — a crew station or a bunk — is walkable
//! even though the machinery next to it isn't. Crossing one is expensive:
//! squeezing through the gun deck to reach the reactor should lose to walking
//! the corridor. Every other module is solid.
//!
//! A destroyed block drops out of the map the moment it is marked, which is
//! what makes battle damage sever routes for free.

use bevy::prelude::*;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};

use crate::ai_ship::components::AiShip;
use crate::building::{footprints, rooms::transform_to_grid, ShipGrid};
use crate::components::*;

/// What a crew member finds when they step into a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavCell {
    /// Laid decking. The only surface crew cross freely.
    Hallway,
    /// A module crew occupy — a crew station or a bunk. Walkable because
    /// somebody has to stand there, but dear enough that routing THROUGH one
    /// loses to any reasonable hallway.
    Post,
    /// A bulkhead door. Passable only while unsealed.
    Door { sealed: bool },
}

impl NavCell {
    /// Traversal cost in tenths, so paths stay integer-comparable. `MIN_COST`
    /// below must track the cheapest value here or the A* heuristic stops
    /// being admissible.
    pub const fn cost(self) -> u32 {
        match self {
            NavCell::Hallway => 6,
            NavCell::Door { .. } => 6,
            NavCell::Post => 14,
        }
    }

    pub const fn passable(self) -> bool {
        !matches!(self, NavCell::Door { sealed: true })
    }
}

/// Cheapest possible step. The heuristic scales by this to stay admissible.
const MIN_COST: u32 = 6;

/// Per-ship map of walkable interior, in ship-LOCAL cells.
///
/// Deliberately not derived from `ShipGrid`: that map holds one entity per
/// cell and hull wins ties, so on the starter the 35 cells carrying both a
/// hull segment and a module report only the hull. Navigation needs to know
/// about both, so it indexes the block queries itself.
#[derive(Component, Default)]
pub struct NavGrid {
    pub cells: HashMap<IVec2, NavCell>,
    /// Bumped on every rebuild so in-flight paths can notice the ship changed
    /// shape under them and replan.
    pub version: u32,
}

impl NavGrid {
    pub fn get(&self, cell: IVec2) -> Option<NavCell> {
        self.cells.get(&cell).copied()
    }

    /// Can a crew member stand here right now?
    pub fn passable(&self, cell: IVec2) -> bool {
        self.get(cell).is_some_and(NavCell::passable)
    }

    fn step_cost(&self, cell: IVec2) -> Option<u32> {
        self.get(cell).filter(|c| c.passable()).map(NavCell::cost)
    }

    /// Nearest passable cell to `from`, searched outward in rings. Used to
    /// place crew at spawn and to recover anyone who ends up standing in a
    /// cell that just got shot away.
    pub fn nearest_passable(&self, from: IVec2) -> Option<IVec2> {
        if self.passable(from) {
            return Some(from);
        }
        // Ships are ~20 cells across at the widest; this bound is generous.
        const MAX_RING: i32 = 32;
        for ring in 1..=MAX_RING {
            let mut best: Option<(IVec2, i32)> = None;
            for dy in -ring..=ring {
                for dx in -ring..=ring {
                    // Only the shell of the ring — the inside was searched already.
                    if dx.abs() != ring && dy.abs() != ring {
                        continue;
                    }
                    let cell = from + IVec2::new(dx, dy);
                    if !self.passable(cell) {
                        continue;
                    }
                    let d = dx * dx + dy * dy;
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((cell, d));
                    }
                }
            }
            if let Some((cell, _)) = best {
                return Some(cell);
            }
        }
        None
    }
}

const NEIGHBOURS: [IVec2; 4] = [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y];

/// A* frontier entry. `Ord` is deliberately reversed on `f` so `BinaryHeap`
/// (a max-heap) pops the cheapest node.
#[derive(PartialEq, Eq)]
struct Frontier {
    f: u32,
    g: u32,
    cell: IVec2,
}

impl Ord for Frontier {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .f
            .cmp(&self.f)
            // Tie-break toward the node already closer to the goal, then on
            // coordinates so equal-cost paths are deterministic run to run.
            .then_with(|| other.g.cmp(&self.g))
            .then_with(|| (self.cell.x, self.cell.y).cmp(&(other.cell.x, other.cell.y)))
    }
}

impl PartialOrd for Frontier {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn heuristic(a: IVec2, b: IVec2) -> u32 {
    ((a.x - b.x).unsigned_abs() + (a.y - b.y).unsigned_abs()) * MIN_COST
}

/// Cheapest walk from `from` to `to`, inclusive of both ends.
///
/// Four-neighbour on purpose: diagonals would let crew cut the corner between
/// two blocked cells, which reads as walking through a wall junction.
///
/// `None` means genuinely unreachable — a severed section, a sealed bulkhead
/// with no way around, or a destination that is not deck at all. Callers are
/// expected to treat that as information, not as an error.
pub fn find_path(nav: &NavGrid, from: IVec2, to: IVec2) -> Option<Vec<IVec2>> {
    if !nav.passable(from) || !nav.passable(to) {
        return None;
    }
    if from == to {
        return Some(vec![from]);
    }

    let mut open = BinaryHeap::new();
    let mut came_from: HashMap<IVec2, IVec2> = HashMap::new();
    let mut best_g: HashMap<IVec2, u32> = HashMap::new();

    best_g.insert(from, 0);
    open.push(Frontier { f: heuristic(from, to), g: 0, cell: from });

    while let Some(Frontier { g, cell, .. }) = open.pop() {
        if cell == to {
            let mut path = vec![cell];
            let mut current = cell;
            while let Some(&prev) = came_from.get(&current) {
                path.push(prev);
                current = prev;
            }
            path.reverse();
            return Some(path);
        }
        // A cheaper route to this cell was queued after this entry was.
        if best_g.get(&cell).is_some_and(|&best| g > best) {
            continue;
        }
        for offset in NEIGHBOURS {
            let next = cell + offset;
            let Some(step) = nav.step_cost(next) else { continue };
            let next_g = g + step;
            if best_g.get(&next).is_some_and(|&best| next_g >= best) {
                continue;
            }
            best_g.insert(next, next_g);
            came_from.insert(next, cell);
            open.push(Frontier { f: next_g + heuristic(next, to), g: next_g, cell: next });
        }
    }
    None
}

/// Every cell reachable from `from`. Cheaper than pathing to each candidate
/// in turn when a caller needs to ask "which of these many stations can this
/// crew member actually get to".
///
/// Only the tests call this so far; it is the query auto-assignment needs
/// once an unreachable post is supposed to go dark rather than just be
/// unwalkable-to.
#[allow(dead_code)]
pub fn reachable_from(nav: &NavGrid, from: IVec2) -> HashSet<IVec2> {
    let mut seen = HashSet::new();
    if !nav.passable(from) {
        return seen;
    }
    let mut queue = VecDeque::from([from]);
    seen.insert(from);
    while let Some(cell) = queue.pop_front() {
        for offset in NEIGHBOURS {
            let next = cell + offset;
            if nav.passable(next) && seen.insert(next) {
                queue.push_back(next);
            }
        }
    }
    seen
}

/// The deck rule, in one place: which hull layers a person can stand on.
/// `None` is a wall.
pub fn hull_nav_cell(layer: HullLayer, sealed: bool) -> Option<NavCell> {
    match layer {
        HullLayer::Hallway => Some(NavCell::Hallway),
        HullLayer::BulkheadDoor => Some(NavCell::Door { sealed }),
        HullLayer::Inner | HullLayer::Outer | HullLayer::Void => None,
    }
}

/// Rebuilds every ship's `NavGrid` from its own live blocks.
///
/// Gated the same way `update_ship_grids` is: block counts are a cheap
/// archetype iteration, the rebuild underneath is not, and on the vast
/// majority of frames nothing has been placed or destroyed. Sealed bulkheads
/// are counted too — sealing a door changes where crew can walk, and it does
/// not add or remove a block.
pub fn rebuild_nav_grids(
    mut commands: Commands,
    ships: Query<Entity, Or<(With<Ship>, With<AiShip>)>>,
    modules: Query<
        (&Module, &ChildOf, Has<CrewStation>, Has<Quarters>),
        Without<DestroyedModule>,
    >,
    hulls: Query<(&HullSegment, &Transform, &ChildOf, Has<BulkheadSealed>), Without<HullDestroyed>>,
    mut grids: Query<&mut NavGrid>,
    mut last_counts: Local<(usize, usize, usize, usize)>,
) {
    let sealed = hulls.iter().filter(|(_, _, _, sealed)| *sealed).count();
    let counts = (modules.iter().count(), hulls.iter().count(), ships.iter().count(), sealed);
    if counts == *last_counts && !grids.is_empty() {
        return;
    }
    *last_counts = counts;

    // Pre-seed every ship so one that just lost its last block gets an empty
    // grid written rather than keeping a stale one.
    let mut per_ship: HashMap<Entity, HashMap<IVec2, NavCell>> =
        ships.iter().map(|ship| (ship, HashMap::new())).collect();

    // Hull first: it lays the deck. Anything not listed here is a wall, and
    // absence from the map is what makes it impassable.
    for (hull, transform, parent, sealed) in hulls.iter() {
        let Some(cells) = per_ship.get_mut(&parent.parent()) else { continue };
        let Some(cell) = hull_nav_cell(hull.hull_layer, sealed) else { continue };
        cells.insert(transform_to_grid(transform), cell);
    }

    // Modules then claim their cells. Machinery is solid and takes its cells
    // OFF the map even if a hallway was laid under it — you can't walk through
    // a reactor because someone decked the floor first. Posts do the opposite:
    // a crew station or a bunk is somewhere a person stands, so it goes on the
    // map whether or not it sits on decking.
    //
    // Posts are applied in a second pass so a post always wins its cell,
    // regardless of the order the queries happen to yield modules in.
    let mut posts: Vec<(Entity, IVec2)> = Vec::new();
    for (module, parent, is_station, is_quarters) in modules.iter() {
        let Some(cells) = per_ship.get_mut(&parent.parent()) else { continue };
        let footprint = footprints::footprint_override(module.module_type);
        let occupied =
            ShipGrid::cells_for(module.grid_position, module.size, module.rotation, footprint);
        if is_station || is_quarters {
            posts.extend(occupied.iter().map(|c| (parent.parent(), *c)));
        } else {
            for cell in occupied {
                cells.remove(&cell);
            }
        }
    }
    for (ship, cell) in posts {
        let Some(cells) = per_ship.get_mut(&ship) else { continue };
        cells.insert(cell, NavCell::Post);
    }

    for (ship, cells) in per_ship {
        match grids.get_mut(ship) {
            Ok(mut grid) => {
                grid.cells = cells;
                grid.version = grid.version.wrapping_add(1);
            }
            Err(_) => {
                // try_insert: a ship can be despawned the same frame it is
                // rebuilt (recursive destruction despawn).
                commands.entity(ship).try_insert(NavGrid { cells, version: 1 });
            }
        }
    }
}

#[cfg(test)]
mod nav_tests {
    use super::*;
    use crate::building::blueprint::Blueprint;

    /// Builds a grid from an ASCII sketch, so a test's intent is visible in
    /// its own source. Rows read top-down; `=` hallway, `P` crew post,
    /// `d` open door, `D` sealed door, anything else solid.
    /// Cell (0,0) is the BOTTOM-left, matching ship-local grid orientation.
    fn sketch(rows: &[&str]) -> NavGrid {
        let mut cells = HashMap::new();
        let height = rows.len() as i32;
        for (row_index, row) in rows.iter().enumerate() {
            let y = height - 1 - row_index as i32;
            for (x, ch) in row.chars().enumerate() {
                let cell = match ch {
                    '=' => NavCell::Hallway,
                    'P' => NavCell::Post,
                    'd' => NavCell::Door { sealed: false },
                    'D' => NavCell::Door { sealed: true },
                    _ => continue,
                };
                cells.insert(IVec2::new(x as i32, y), cell);
            }
        }
        NavGrid { cells, version: 1 }
    }

    fn cost_of(nav: &NavGrid, path: &[IVec2]) -> u32 {
        path.iter().skip(1).map(|c| nav.get(*c).unwrap().cost()).sum()
    }

    #[test]
    fn walks_a_straight_corridor() {
        let nav = sketch(&["====="]);
        let path = find_path(&nav, IVec2::new(0, 0), IVec2::new(4, 0)).unwrap();
        assert_eq!(path.len(), 5);
        assert_eq!(path[0], IVec2::new(0, 0));
        assert_eq!(path[4], IVec2::new(4, 0));
    }

    #[test]
    fn routes_around_a_wall() {
        // A bulkhead across the middle with a gap at the top.
        let nav = sketch(&[
            "=====",
            "==#==",
            "==#==",
        ]);
        let path = find_path(&nav, IVec2::new(0, 0), IVec2::new(4, 0)).unwrap();
        // It must go up and over, never through the blocked column.
        assert!(path.iter().any(|c| c.y == 2), "path never used the gap: {path:?}");
        assert!(!path.contains(&IVec2::new(2, 0)));
        assert!(!path.contains(&IVec2::new(2, 1)));
    }

    #[test]
    fn a_sealed_door_is_a_wall_and_an_open_one_is_not() {
        let open = sketch(&["==d=="]);
        assert!(find_path(&open, IVec2::new(0, 0), IVec2::new(4, 0)).is_some());

        let sealed = sketch(&["==D=="]);
        assert_eq!(find_path(&sealed, IVec2::new(0, 0), IVec2::new(4, 0)), None);
    }

    #[test]
    fn walks_the_long_way_round_rather_than_through_the_gun_deck() {
        // Straight across shoves through six manned posts: 14*6 = 84.
        // The way round is nine hallway steps plus the post itself: 6*9 + 14 = 68.
        // Fewer cells is not less effort, and pushing past working crew loses.
        let nav = sketch(&[
            "=======",
            "=     =",
            "PPPPPPP",
        ]);
        let path = find_path(&nav, IVec2::new(0, 0), IVec2::new(6, 0)).unwrap();
        assert!(
            !path.contains(&IVec2::new(3, 0)),
            "crew shoved through the gun deck instead of using the hallway: {path:?}"
        );
        assert_eq!(cost_of(&nav, &path), 6 * 9 + 14);
    }

    #[test]
    fn a_severed_section_is_unreachable() {
        let nav = sketch(&[
            "==#==",
            "==#==",
            "==#==",
        ]);
        assert_eq!(find_path(&nav, IVec2::new(0, 0), IVec2::new(4, 0)), None);
        assert_eq!(reachable_from(&nav, IVec2::new(0, 0)).len(), 6);
    }

    #[test]
    fn nearest_passable_finds_the_deck_from_inside_a_wall() {
        let nav = sketch(&[
            "#####",
            "##=##",
            "#####",
        ]);
        assert_eq!(nearest_of(&nav, IVec2::new(0, 0)), Some(IVec2::new(2, 1)));
        // Already standing somewhere valid: don't move them.
        assert_eq!(nearest_of(&nav, IVec2::new(2, 1)), Some(IVec2::new(2, 1)));
    }

    fn nearest_of(nav: &NavGrid, cell: IVec2) -> Option<IVec2> {
        nav.nearest_passable(cell)
    }

    /// Derives a nav grid from design data with the same rules
    /// `rebuild_nav_grids` uses, so the real ships can be checked without
    /// standing up an ECS world.
    fn nav_from_design(design: &Blueprint) -> NavGrid {
        let registry = crate::building::registry::build_registry();
        let mut cells = HashMap::new();
        for hull in &design.hull_cells {
            if let Some(cell) = hull_nav_cell(hull.layer, false) {
                cells.insert(hull.grid_pos, cell);
            }
        }
        let mut posts = Vec::new();
        for module in &design.modules {
            let def = registry.get(module.module_type);
            let footprint = footprints::footprint_override(module.module_type);
            let occupied =
                ShipGrid::cells_for(module.grid_pos, def.size, module.rotation, footprint);
            let is_post = def.crew_station
                || matches!(def.companion, crate::building::registry::CompanionData::Quarters { .. });
            if is_post {
                posts.extend(occupied);
            } else {
                for cell in occupied {
                    cells.remove(&cell);
                }
            }
        }
        for cell in posts {
            cells.insert(cell, NavCell::Post);
        }
        NavGrid { cells, version: 1 }
    }

    /// The whole premise of deriving the deck from `HullLayer::Inner`: the
    /// ship the player actually starts with must be walkable end to end
    /// without anyone authoring a single corridor. If this fails, crew would
    /// spawn into a ship whose stations they cannot reach.
    #[test]
    fn the_starter_ship_interior_is_one_connected_space() {
        let design = crate::ship::builtin_starter_design();
        let nav = nav_from_design(&design);
        assert!(!nav.cells.is_empty(), "starter design produced no walkable cells");

        let start = *nav.cells.keys().next().unwrap();
        let reachable = reachable_from(&nav, start);
        let unreachable: Vec<_> = nav
            .cells
            .keys()
            .filter(|c| !reachable.contains(c))
            .copied()
            .collect();
        assert!(
            unreachable.is_empty(),
            "{} of {} interior cells are cut off from the rest: {:?}",
            unreachable.len(),
            nav.cells.len(),
            unreachable
        );
    }

    /// The builtin is only a fallback — `designs/starter.json` is what the
    /// game actually spawns, and it wins whenever it exists. A walkable
    /// builtin next to a shipped design nobody can walk is exactly the drift
    /// that would put crew on a ship they can't cross, so check the file.
    #[test]
    fn the_shipped_starter_design_is_walkable() {
        let design = crate::building::blueprint::load_design_file("designs/starter.json")
            .expect("designs/starter.json missing or unparseable");
        let nav = nav_from_design(&design);

        let halls = nav.cells.values().filter(|c| **c == NavCell::Hallway).count();
        assert!(halls > 0, "the shipped starter has no hallways — crew cannot move at all");

        let start = *nav
            .cells
            .iter()
            .find(|(_, c)| **c == NavCell::Hallway)
            .expect("no hallway to start from")
            .0;
        let reachable = reachable_from(&nav, start);
        let stranded: Vec<_> = nav
            .cells
            .iter()
            .filter(|(cell, kind)| **kind == NavCell::Post && !reachable.contains(cell))
            .map(|(cell, _)| *cell)
            .collect();
        assert!(
            stranded.is_empty(),
            "{} crew posts have no hallway route and would go unmanned: {:?}",
            stranded.len(),
            stranded
        );
    }

    /// Armour plates sit outboard, on cells with no hull under them. If they
    /// ever entered the nav map, crew would stroll out onto the plating.
    #[test]
    fn outboard_armour_is_not_walkable() {
        let design = crate::ship::builtin_starter_design();
        let hull: HashSet<IVec2> = design.hull_cells.iter().map(|h| h.grid_pos).collect();
        let nav = nav_from_design(&design);

        let plates = design
            .modules
            .iter()
            .filter(|m| !hull.contains(&m.grid_pos))
            .count();
        assert!(plates > 0, "starter has no outboard modules — test proves nothing");

        for module in &design.modules {
            if !hull.contains(&module.grid_pos) {
                assert!(
                    !nav.passable(module.grid_pos),
                    "outboard module at {:?} became walkable deck",
                    module.grid_pos
                );
            }
        }
    }
}
