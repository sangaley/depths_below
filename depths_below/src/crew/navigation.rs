//! Interior navigation: which ship-local cells crew can walk, and how to get
//! between two of them.
//!
//! The deck is derived, the walls are authored. `HullLayer::Inner` is what
//! both hull generators (`ship/spawner.rs` and `ai_ship/layouts.rs`) assign to
//! every enclosed cell, so it already describes the floor of every ship in the
//! game — the starter and all ten factions get a walkable interior without a
//! single design being re-authored. `Outer` is the shell, `Void` is the gap
//! between hulls, and neither is somewhere a person can stand.
//!
//! Modules sit ON the deck rather than replacing it, so they stay walkable and
//! just cost more to cross. That falls out of the grid rather than being a
//! special case: armour plates sit OUTBOARD on cells with no hull under them
//! (see `building::armour`), so they never enter the map at all.
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
    /// Purpose-built passage — `Corridor`, `LadderShaft`, `MaintenanceTunnel`.
    /// The registry has promised "crew moves faster through corridors" since
    /// those blocks were added; this is where that becomes true.
    Corridor,
    /// Bare deck: inner hull with nothing standing on it.
    Floor,
    /// Deck with machinery on it. Passable, but you squeeze past.
    Machinery,
    /// A bulkhead door. Passable only while unsealed.
    Door { sealed: bool },
}

impl NavCell {
    /// Traversal cost in tenths, so paths stay integer-comparable. `MIN_COST`
    /// below must track the cheapest value here or the A* heuristic stops
    /// being admissible.
    pub const fn cost(self) -> u32 {
        match self {
            NavCell::Corridor => 6,
            NavCell::Floor => 10,
            NavCell::Machinery => 18,
            NavCell::Door { .. } => 10,
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
        HullLayer::Inner => Some(NavCell::Floor),
        HullLayer::BulkheadDoor => Some(NavCell::Door { sealed }),
        HullLayer::Outer | HullLayer::Void => None,
    }
}

/// Blocks whose whole job is letting people through.
pub fn is_passage(module_type: ModuleType) -> bool {
    matches!(
        module_type,
        ModuleType::Corridor | ModuleType::LadderShaft | ModuleType::MaintenanceTunnel
    )
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
    modules: Query<(&Module, &ChildOf), Without<DestroyedModule>>,
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

    // Modules then refine the deck they stand on. A module with no deck under
    // it (an outboard armour plate) is skipped rather than inserted, so it
    // stays a wall — you can't walk on plating bolted to the outside.
    for (module, parent) in modules.iter() {
        let Some(cells) = per_ship.get_mut(&parent.parent()) else { continue };
        let footprint = footprints::footprint_override(module.module_type);
        let refined = if is_passage(module.module_type) {
            NavCell::Corridor
        } else {
            NavCell::Machinery
        };
        for cell in ShipGrid::cells_for(module.grid_position, module.size, module.rotation, footprint) {
            // Only upgrade actual floor — a door keeps being a door, and a
            // cell with no hull stays off the map.
            if cells.get(&cell) == Some(&NavCell::Floor) {
                cells.insert(cell, refined);
            }
        }
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
    /// its own source. Rows read top-down; `.` floor, `#` wall (absent),
    /// `c` corridor, `m` machinery, `d` open door, `D` sealed door.
    /// Cell (0,0) is the BOTTOM-left, matching ship-local grid orientation.
    fn sketch(rows: &[&str]) -> NavGrid {
        let mut cells = HashMap::new();
        let height = rows.len() as i32;
        for (row_index, row) in rows.iter().enumerate() {
            let y = height - 1 - row_index as i32;
            for (x, ch) in row.chars().enumerate() {
                let cell = match ch {
                    '.' => NavCell::Floor,
                    'c' => NavCell::Corridor,
                    'm' => NavCell::Machinery,
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
        let nav = sketch(&["....."]);
        let path = find_path(&nav, IVec2::new(0, 0), IVec2::new(4, 0)).unwrap();
        assert_eq!(path.len(), 5);
        assert_eq!(path[0], IVec2::new(0, 0));
        assert_eq!(path[4], IVec2::new(4, 0));
    }

    #[test]
    fn routes_around_a_wall() {
        // A bulkhead across the middle with a gap at the top.
        let nav = sketch(&[
            ".....",
            "..#..",
            "..#..",
        ]);
        let path = find_path(&nav, IVec2::new(0, 0), IVec2::new(4, 0)).unwrap();
        // It must go up and over, never through the blocked column.
        assert!(path.iter().any(|c| c.y == 2), "path never used the gap: {path:?}");
        assert!(!path.contains(&IVec2::new(2, 0)));
        assert!(!path.contains(&IVec2::new(2, 1)));
    }

    #[test]
    fn a_sealed_door_is_a_wall_and_an_open_one_is_not() {
        let open = sketch(&["..d.."]);
        assert!(find_path(&open, IVec2::new(0, 0), IVec2::new(4, 0)).is_some());

        let sealed = sketch(&["..D.."]);
        assert_eq!(find_path(&sealed, IVec2::new(0, 0), IVec2::new(4, 0)), None);
    }

    #[test]
    fn prefers_the_longer_corridor_to_the_shorter_squeeze() {
        // Four steps straight across, three of them machinery: 18*3 + 10 = 64.
        // Eight steps around the outside, seven of them corridor: 6*7 + 10 = 52.
        // Fewer cells is not the same as less effort, and the corridor wins.
        let nav = sketch(&[
            "ccccc",
            "c...c",
            ".mmm.",
        ]);
        let path = find_path(&nav, IVec2::new(0, 0), IVec2::new(4, 0)).unwrap();
        assert!(
            !path.contains(&IVec2::new(2, 0)),
            "crew squeezed through machinery instead of using the corridor: {path:?}"
        );
        assert_eq!(path.len(), 9, "expected the eight-step way round: {path:?}");
        assert_eq!(cost_of(&nav, &path), 52);
    }

    #[test]
    fn a_severed_section_is_unreachable() {
        let nav = sketch(&[
            "..#..",
            "..#..",
            "..#..",
        ]);
        assert_eq!(find_path(&nav, IVec2::new(0, 0), IVec2::new(4, 0)), None);
        assert_eq!(reachable_from(&nav, IVec2::new(0, 0)).len(), 6);
    }

    #[test]
    fn nearest_passable_finds_the_deck_from_inside_a_wall() {
        let nav = sketch(&[
            "#####",
            "##.##",
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
        for module in &design.modules {
            let refined = if is_passage(module.module_type) {
                NavCell::Corridor
            } else {
                NavCell::Machinery
            };
            let footprint = footprints::footprint_override(module.module_type);
            let size = registry.get(module.module_type).size;
            for cell in ShipGrid::cells_for(module.grid_pos, size, module.rotation, footprint) {
                if cells.get(&cell) == Some(&NavCell::Floor) {
                    cells.insert(cell, refined);
                }
            }
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
