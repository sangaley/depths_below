use bevy::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};
use crate::components::*;  // includes BulkheadSealed
use crate::resources::PowerGraph;

/// A detected room inside the submarine
#[derive(Debug, Clone)]
pub struct Room {
    pub id: usize,
    pub tiles: Vec<IVec2>,
    pub water_level: f32,       // 0.0 = dry, 1.0 = full
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
pub fn detect_rooms(
    hull_query: &Query<(&HullSegment, &Transform, &Parent)>,
    module_query: &Query<(&Module, &Transform)>,
    sealed_positions: &HashSet<IVec2>,
) -> RoomMap {
    // Collect all hull tile positions by layer
    let mut inner_hull_positions: HashSet<IVec2> = HashSet::new();
    let mut outer_hull_positions: HashSet<IVec2> = HashSet::new();
    let mut all_hull_positions: HashSet<IVec2> = HashSet::new();

    for (hull, transform, _parent) in hull_query.iter() {
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
    for (module, _transform) in module_query.iter() {
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
            water_level: 0.0,
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
    hull_query: Query<(&HullSegment, &Transform, &Parent)>,
    module_query: Query<(&Module, &Transform)>,
    sealed_query: Query<(&HullSegment, &Transform), With<BulkheadSealed>>,
    mut room_map: ResMut<RoomMap>,
) {
    // Build set of sealed bulkhead positions
    let sealed_positions: HashSet<IVec2> = sealed_query
        .iter()
        .map(|(_, transform)| transform_to_grid(transform))
        .collect();

    // Save flood state for every tile in every room
    let mut tile_flood_state: HashMap<IVec2, (f32, bool)> = HashMap::new();
    for room in room_map.rooms.iter() {
        if room.water_level > 0.0 || room.is_breached {
            for &tile in &room.tiles {
                tile_flood_state.insert(tile, (room.water_level, room.is_breached));
            }
        }
    }

    *room_map = detect_rooms(&hull_query, &module_query, &sealed_positions);

    // Restore flood state: new room inherits the highest water level from any
    // overlapping old tile, and is_breached if any overlapping tile was breached.
    for room in room_map.rooms.iter_mut() {
        let mut max_water = 0.0_f32;
        let mut any_breached = false;
        for tile in &room.tiles {
            if let Some(&(water_level, is_breached)) = tile_flood_state.get(tile) {
                max_water = max_water.max(water_level);
                any_breached = any_breached || is_breached;
            }
        }
        if max_water > 0.0 || any_breached {
            room.water_level = max_water;
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
