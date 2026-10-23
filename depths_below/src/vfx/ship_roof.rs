//! Zoomed out, your ship shows its plating, not its insides.
//!
//! Close up the ship is a cutaway: decks, rooms, crew walking the halls. From
//! far out all of that is noise the size of a pixel, and the ship stopped
//! reading as a ship. Past ROOF_FROM zoom a roof of hull plating fades in over
//! every enclosed cell -- decks, corridors, and the rooms and machinery inside
//! -- while whatever touches open space stays on top: guns, engines, plates,
//! the hull's own edge. The roof is one flat plate in the hull's colour, so
//! the ship reads as a single solid structure, and each cell of it only stands
//! while something stands under it: shoot a hole and the roof opens there too.
//!
//! The roof exists only while it can be seen. Zoomed in there are no tiles
//! at all.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;

use crate::ai_ship::components::OwnedByAiShip;
use crate::building::{footprints, grid_to_local, ShipGrid};
use crate::camera::CameraState;
use crate::components::{DestroyedModule, HullDestroyed, HullLayer, HullMaterial, HullSegment, Module, Ship};

/// Zoom where the roof starts to show, and where it's solid.
const ROOF_FROM: f32 = 3.5;
const ROOF_SOLID: f32 = 5.5;
/// Above modules (0.2-0.4), damage overlays (0.35) and crew (0.6).
const ROOF_Z: f32 = 0.7;

/// One cell of the roof, a child of the ship.
#[derive(Component)]
pub struct RoofTile;

/// How solid the roof is at a zoom.
pub fn roof_alpha(zoom: f32) -> f32 {
    ((zoom - ROOF_FROM) / (ROOF_SOLID - ROOF_FROM)).clamp(0.0, 1.0)
}

/// Which cells the roof covers: every enclosed cell, and none that an
/// outboard module (one touching open space) stands on.
pub fn roof_cells(hull: &[(IVec2, HullLayer)], modules: &[Vec<IVec2>]) -> HashSet<IVec2> {
    let mut occupied: HashSet<IVec2> = hull.iter().map(|(c, _)| *c).collect();
    for cells in modules {
        occupied.extend(cells.iter().copied());
    }
    let open = |c: IVec2| [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y].iter().any(|d| !occupied.contains(&(c + *d)));
    let mut outboard: HashSet<IVec2> = HashSet::default();
    let mut roof: HashSet<IVec2> = hull.iter().filter(|(_, l)| *l != HullLayer::Outer).map(|(c, _)| *c).collect();
    for cells in modules {
        if cells.iter().any(|c| open(*c)) {
            outboard.extend(cells.iter().copied());
        } else {
            roof.extend(cells.iter().copied());
        }
    }
    roof.retain(|c| !outboard.contains(c));
    roof
}

/// Lay the roof when it comes into view or the ship changes under it, and take
/// it away when you zoom back in.
#[allow(clippy::too_many_arguments)]
pub fn update_ship_roof(
    mut commands: Commands,
    camera: Res<CameraState>,
    ships: Query<Entity, With<Ship>>,
    hull: Query<(&HullSegment, &ChildOf), (Without<HullDestroyed>, Without<OwnedByAiShip>)>,
    modules: Query<(&Module, &ChildOf), (Without<DestroyedModule>, Without<OwnedByAiShip>)>,
    mut tiles: Query<(Entity, &mut Sprite), With<RoofTile>>,
    mut last: Local<Option<(Entity, usize, usize)>>,
) {
    let alpha = roof_alpha(camera.zoom);
    let Ok(ship) = ships.single() else { return };
    if alpha <= 0.0 {
        if last.is_some() {
            for (tile, _) in tiles.iter() {
                commands.entity(tile).try_despawn();
            }
            *last = None;
        }
        return;
    }
    let mine_hull: Vec<(&HullSegment, &ChildOf)> = hull.iter().filter(|(_, p)| p.parent() == ship).collect();
    let mine_modules: Vec<&Module> = modules.iter().filter(|(_, p)| p.parent() == ship).map(|(m, _)| m).collect();
    let key = (ship, mine_hull.len(), mine_modules.len());
    // Tiles cleared from outside (clear_ship_roof) are laid again too.
    if *last == Some(key) && !tiles.is_empty() {
        for (_, mut sprite) in tiles.iter_mut() {
            if (sprite.color.alpha() - alpha).abs() > 0.001 {
                sprite.color.set_alpha(alpha);
            }
        }
        return;
    }
    *last = Some(key);
    for (tile, _) in tiles.iter() {
        commands.entity(tile).try_despawn();
    }

    let cells: Vec<(IVec2, HullLayer)> = mine_hull.iter().map(|(s, _)| (s.grid_position, s.hull_layer)).collect();
    let footprints: Vec<Vec<IVec2>> = mine_modules
        .iter()
        .map(|m| ShipGrid::cells_for(m.grid_position, m.size, m.rotation, footprints::footprint_override(m.module_type)).to_vec())
        .collect();
    let roof = roof_cells(&cells, &footprints);
    // The ship's skin, cell by cell; the roof is the commonest of it.
    let skin: HashMap<IVec2, HullMaterial> = mine_hull.iter().map(|(s, _)| (s.grid_position, s.material)).collect();
    let mut counts: Vec<(HullMaterial, usize)> = Vec::new();
    for m in skin.values() {
        match counts.iter_mut().find(|(k, _)| k == m) {
            Some((_, n)) => *n += 1,
            None => counts.push((*m, 1)),
        }
    }
    let material = counts.into_iter().max_by_key(|(_, n)| *n).map_or(HullMaterial::Steel, |(m, _)| m);

    // One flat plate in the hull's own colour (its texture, averaged), not
    // the plate texture tiled: that is shaded across each tile, and repeated
    // over a whole deck it read as corrugated sheet instead of one hull.
    let [r, g, b] = crate::ai_ship::preview::texture_average(material, HullLayer::Outer);
    let plate = Color::srgba(r / 255.0, g / 255.0, b / 255.0, alpha);
    for cell in roof {
        commands.spawn((
            Sprite::from_color(plate, Vec2::splat(crate::building::GRID_SIZE)),
            Transform::from_translation(grid_to_local(cell).extend(ROOF_Z)),
            RoofTile,
            ChildOf(ship),
        ));
    }
}

/// Off the flight screen (docked, building, paused) the roof comes off;
/// update_ship_roof lays it again on return.
pub fn clear_ship_roof(mut commands: Commands, tiles: Query<Entity, With<RoofTile>>) {
    for tile in tiles.iter() {
        commands.entity(tile).try_despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_roof_fades_in_as_you_pull_back() {
        assert_eq!(roof_alpha(1.8), 0.0, "the default view is the cutaway");
        assert_eq!(roof_alpha(ROOF_SOLID + 1.0), 1.0);
        assert!(roof_alpha(4.5) > 0.0 && roof_alpha(4.5) < 1.0);
    }

    #[test]
    fn enclosed_rooms_are_roofed_and_outboard_guns_are_not() {
        // A 5x3 box: outer skin around a 3x1 interior.
        let mut hull = Vec::new();
        for x in 0..5 {
            for y in 0..3 {
                let edge = x == 0 || x == 4 || y == 0 || y == 2;
                hull.push((IVec2::new(x, y), if edge { HullLayer::Outer } else { HullLayer::Hallway }));
            }
        }
        let reactor = vec![IVec2::new(2, 1)];
        let gun = vec![IVec2::new(5, 1)];
        let roof = roof_cells(&hull, &[reactor, gun]);
        assert!(roof.contains(&IVec2::new(1, 1)) && roof.contains(&IVec2::new(3, 1)), "the corridor");
        assert!(roof.contains(&IVec2::new(2, 1)), "the reactor inside");
        assert!(!roof.contains(&IVec2::new(5, 1)), "the gun on the bow");
        assert!(!roof.contains(&IVec2::new(0, 1)), "the skin is already plating");
    }
}
