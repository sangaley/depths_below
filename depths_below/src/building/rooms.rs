use bevy::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};
use crate::components::*;  // includes BulkheadSealed
use crate::resources::PowerGraph;

/// A detected room inside the ship
#[derive(Debug, Clone)]
pub struct Room {
    pub id: usize,
    pub tiles: Vec<IVec2>,
    pub air_level: f32,       // 1.0 = pressurized, 0.0 = vacuum
    pub is_breached: bool,
    pub has_power: bool,
}

/// Resource tracking all detected rooms
#[derive(Resource, Default)]
pub struct RoomMap {
    pub rooms: Vec<Room>,
    /// Maps grid position -> room id
    pub tile_to_room: HashMap<IVec2, usize>,
}

/// Grid size constant matching the rest of the building system
const GRID_SIZE: f32 = 66.0;

/// Converts a Transform to a grid position
pub fn transform_to_grid(t: &Transform) -> IVec2 {
    IVec2::new(
        (t.translation.x / GRID_SIZE).round() as i32,
        ((t.translation.y + 33.0) / GRID_SIZE).round() as i32,
    )
}

/// Flood-fill to detect enclosed rooms from inner hull tiles.
/// A "room" is a connected group of empty interior cells bounded by inner hull.
/// For now: any Module grid_position that is surrounded by hull = part of a room.
/// PLAYER SHIP ONLY: hull/module queries span every ship in the world (grid
/// positions are ship-local, so an AI ship's tiles routinely collide
/// numerically with the player's). Unscoped, AI hull/modules leaked into the
/// player's flood-fill — a wall from a ship the player has never seen could
/// block a room, or an AI module could get counted as player interior space.
pub fn detect_rooms(
    hull_query: &Query<(&HullSegment, &Transform, &ChildOf)>,
    module_query: &Query<(&Module, &Transform, &ChildOf)>,
    sealed_positions: &HashSet<IVec2>,
    player_ship: Entity,
) -> RoomMap {
    // Collect all hull tile positions by layer
    let mut inner_hull_positions: HashSet<IVec2> = HashSet::new();
    let mut outer_hull_positions: HashSet<IVec2> = HashSet::new();
    let mut all_hull_positions: HashSet<IVec2> = HashSet::new();

    for (hull, transform, parent) in hull_query.iter() {
        if parent.parent() != player_ship { continue; }
        let grid = transform_to_grid(transform);
        all_hull_positions.insert(grid);
        match hull.hull_layer {
            HullLayer::Inner => { inner_hull_positions.insert(grid); }
            HullLayer::BulkheadDoor => {
                // Only sealed bulkheads act as walls; unsealed are passable
                if sealed_positions.contains(&grid) {
                    inner_hull_positions.insert(grid);
                }
            }
            HullLayer::Outer => { outer_hull_positions.insert(grid); }
            HullLayer::Void => {}
        }
    }

    // Collect all module positions as "interior" cells
    let mut module_positions: HashSet<IVec2> = HashSet::new();
    for (module, _transform, parent) in module_query.iter() {
        if parent.parent() != player_ship { continue; }
        module_positions.insert(module.grid_position);
    }

    // Flood-fill from each unvisited module position
    // Connected module positions (adjacent, not separated by inner hull) form a room
    let mut visited: HashSet<IVec2> = HashSet::new();
    let mut rooms = Vec::new();
    let mut tile_to_room = HashMap::new();

    for &pos in &module_positions {
        if visited.contains(&pos) {
            continue;
        }

        // BFS flood fill
        let mut queue = VecDeque::new();
        let mut room_tiles = Vec::new();
        queue.push_back(pos);
        visited.insert(pos);

        while let Some(current) = queue.pop_front() {
            room_tiles.push(current);

            // Check 4 neighbors
            for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                let neighbor = current + offset;
                if visited.contains(&neighbor) {
                    continue;
                }
                // Stop at inner hull boundaries (walls)
                if inner_hull_positions.contains(&neighbor) {
                    continue;
                }
                // Only flood into other module positions (interior space)
                if module_positions.contains(&neighbor) {
                    visited.insert(neighbor);
                    queue.push_back(neighbor);
                }
            }
        }

        let room_id = rooms.len();
        for &tile in &room_tiles {
            tile_to_room.insert(tile, room_id);
        }

        rooms.push(Room {
            id: room_id,
            tiles: room_tiles,
            air_level: 1.0,
            is_breached: false,
            has_power: false,
        });
    }

    RoomMap { rooms, tile_to_room }
}

/// System that recalculates rooms when hull tiles change, preserving flood state.
/// Uses tile-set intersection: each tile remembers its old room's flood state,
/// and the new room inherits the worst (highest water) state from overlapping tiles.
pub fn update_room_map(
    ship_query: Query<Entity, With<Ship>>,
    hull_query: Query<(&HullSegment, &Transform, &ChildOf)>,
    module_query: Query<(&Module, &Transform, &ChildOf)>,
    sealed_query: Query<(&HullSegment, &Transform, &ChildOf), With<BulkheadSealed>>,
    mut room_map: ResMut<RoomMap>,
) {
    let Ok(player_ship) = ship_query.single() else { return };

    // Build set of sealed bulkhead positions
    let sealed_positions: HashSet<IVec2> = sealed_query
        .iter()
        .filter(|(_, _, parent)| parent.parent() == player_ship)
        .map(|(_, transform, _)| transform_to_grid(transform))
        .collect();

    // Save air state for every tile in every room
    let mut tile_air_state: HashMap<IVec2, (f32, bool)> = HashMap::new();
    for room in room_map.rooms.iter() {
        if room.air_level < 1.0 || room.is_breached {
            for &tile in &room.tiles {
                tile_air_state.insert(tile, (room.air_level, room.is_breached));
            }
        }
    }

    *room_map = detect_rooms(&hull_query, &module_query, &sealed_positions, player_ship);

    // Restore air state: new room inherits the lowest air level from any
    // overlapping old tile, and is_breached if any overlapping tile was breached.
    for room in room_map.rooms.iter_mut() {
        let mut min_air = 1.0_f32;
        let mut any_breached = false;
        for tile in &room.tiles {
            if let Some(&(air_level, is_breached)) = tile_air_state.get(tile) {
                min_air = min_air.min(air_level);
                any_breached = any_breached || is_breached;
            }
        }
        if min_air < 1.0 || any_breached {
            room.air_level = min_air;
            room.is_breached = any_breached;
        }
    }
}

/// System that checks power connectivity using the PowerGraph.
/// A room has power if any of its tiles are in the power graph.
pub fn update_room_power(
    mut room_map: ResMut<RoomMap>,
    power_graph: Res<PowerGraph>,
) {
    for room in room_map.rooms.iter_mut() {
        room.has_power = room.tiles.iter().any(|t| power_graph.powered_tiles.contains(t));
    }
}
