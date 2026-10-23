//! Zoomed out, your ship shows its plating, not its insides.
//!
//! Close up the ship is a cutaway: decks, rooms, crew walking the halls. From
//! far out all of that is noise the size of a pixel, and the ship stopped
//! reading as a ship. Past ROOF_FROM zoom a roof of hull plating fades in over
//! every enclosed cell -- decks, corridors, and the rooms and machinery inside
//! -- while whatever touches open space stays on top: guns, engines, plates,
//! the hull's own edge. The roof is laid in riveted plates of the hull's
//! colour, so the ship reads as one solid structure with a surface, and each
//! cell of it only stands while something stands under it: shoot a hole and
//! the roof opens there too.
//!
//! The roof exists only while it can be seen. Zoomed in there are no tiles
//! at all.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;

use crate::ai_ship::components::OwnedByAiShip;
use crate::building::{footprints, grid_to_local, ShipGrid};
use crate::camera::CameraState;
use crate::components::{DestroyedModule, HullDestroyed, HullLayer, HullMaterial, HullSegment, Module, ModuleCategory, Ship};

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
/// outboard module (one touching open space) or a weapon stands on. Each
/// module is its footprint and whether it stands above the hull (a weapon).
pub fn roof_cells(hull: &[(IVec2, HullLayer)], modules: &[(Vec<IVec2>, bool)]) -> HashSet<IVec2> {
    let mut occupied: HashSet<IVec2> = hull.iter().map(|(c, _)| *c).collect();
    for (cells, _) in modules {
        occupied.extend(cells.iter().copied());
    }
    let open = |c: IVec2| [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y].iter().any(|d| !occupied.contains(&(c + *d)));
    let mut outboard: HashSet<IVec2> = HashSet::default();
    let mut roof: HashSet<IVec2> = hull.iter().filter(|(_, l)| *l != HullLayer::Outer).map(|(c, _)| *c).collect();
    for (cells, above) in modules {
        if *above || cells.iter().any(|c| open(*c)) {
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
    assets: Res<AssetServer>,
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
    let footprints: Vec<(Vec<IVec2>, bool)> = mine_modules
        .iter()
        .map(|m| {
            let cells = ShipGrid::cells_for(m.grid_position, m.size, m.rotation, footprints::footprint_override(m.module_type)).to_vec();
            (cells, stands_above(m))
        })
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

    // Laid in plates of 2x2 cells (tools/art/gen_roof_panels.py): flat in
    // the hull's colour with a seam and rivets round each plate, and each
    // plate a shade off its neighbours. Tiling the hull texture itself shaded
    // every cell top to bottom and read as corrugated sheet; one flat colour
    // read as no surface at all.
    let image = assets.load(roof_path(material));
    for cell in roof {
        let plate = cell.div_euclid(IVec2::splat(2));
        let q = cell.rem_euclid(IVec2::splat(2));
        // Image rows run top-down; grid y runs up.
        let rect = Rect::new(q.x as f32 * 128.0, (1 - q.y) as f32 * 128.0, q.x as f32 * 128.0 + 128.0, (2 - q.y) as f32 * 128.0);
        let tone = plate_tone(plate);
        commands.spawn((
            Sprite {
                image: image.clone(),
                rect: Some(rect),
                color: Color::srgba(tone, tone, tone, alpha),
                custom_size: Some(Vec2::splat(crate::building::GRID_SIZE)),
                ..default()
            },
            Transform::from_translation(grid_to_local(cell).extend(ROOF_Z)),
            RoofTile,
            ChildOf(ship),
        ));
    }
}

/// Weapons stand above the hull: zoomed out they stay in sight over the
/// roof wherever they're mounted, barrels and all.
fn stands_above(module: &Module) -> bool {
    module.module_type.category() == ModuleCategory::Weapons
}

/// A weapon lifted over the roof, and the height it came from.
#[derive(Component)]
pub struct RaisedOverRoof(f32);

/// Above the roof.
const RAISED_Z: f32 = ROOF_Z + 0.05;

/// While the roof shows, the ship's weapons ride above it; zoomed back in
/// they return to their own height (under the crew who man them).
pub fn raise_weapons_over_roof(
    mut commands: Commands,
    camera: Res<CameraState>,
    ships: Query<Entity, With<Ship>>,
    mut weapons: Query<(Entity, &Module, &ChildOf, &mut Transform, Option<&RaisedOverRoof>), Without<OwnedByAiShip>>,
) {
    let up = roof_alpha(camera.zoom) > 0.0;
    let Ok(ship) = ships.single() else { return };
    for (entity, module, parent, mut tf, raised) in weapons.iter_mut() {
        if parent.parent() != ship || !stands_above(module) {
            continue;
        }
        match (up, raised) {
            (true, None) => {
                commands.entity(entity).try_insert(RaisedOverRoof(tf.translation.z));
                tf.translation.z = RAISED_Z;
            }
            (false, Some(RaisedOverRoof(z))) => {
                tf.translation.z = *z;
                commands.entity(entity).try_remove::<RaisedOverRoof>();
            }
            _ => {}
        }
    }
}

/// Off the flight screen weapons go back to their own height.
pub fn lower_weapons(mut commands: Commands, mut raised: Query<(Entity, &mut Transform, &RaisedOverRoof)>) {
    for (entity, mut tf, RaisedOverRoof(z)) in raised.iter_mut() {
        tf.translation.z = *z;
        commands.entity(entity).try_remove::<RaisedOverRoof>();
    }
}

/// The roof plate texture for a hull material.
fn roof_path(material: HullMaterial) -> &'static str {
    match material {
        HullMaterial::Steel => "sprites/hull/roof_steel.png",
        HullMaterial::Titanium => "sprites/hull/roof_titanium.png",
        HullMaterial::Composite => "sprites/hull/roof_composite.png",
        HullMaterial::AbyssalAlloy => "sprites/hull/roof_abyssal.png",
    }
}

/// Each plate a shade lighter or darker than the next (0.93 to 1.05), the
/// same every time for the same plate.
pub fn plate_tone(plate: IVec2) -> f32 {
    let h = (plate.x as u32).wrapping_mul(0x9E37_79B1) ^ (plate.y as u32).wrapping_mul(0x85EB_CA77);
    let h = (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D);
    0.93 + 0.12 * ((h >> 8) & 0xFF) as f32 / 255.0
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
    fn neighbouring_plates_differ_and_each_stays_the_same() {
        let tones: Vec<f32> = (0..8).map(|x| plate_tone(IVec2::new(x, 3))).collect();
        assert!(tones.iter().all(|t| (0.93..=1.05).contains(t)));
        assert!(tones.windows(2).any(|w| (w[0] - w[1]).abs() > 0.01), "plates all the same shade");
        assert_eq!(plate_tone(IVec2::new(-4, 9)), plate_tone(IVec2::new(-4, 9)));
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
        let reactor = (vec![IVec2::new(2, 1)], false);
        let gun = (vec![IVec2::new(5, 1)], true);
        let roof = roof_cells(&hull, &[reactor, gun]);
        assert!(roof.contains(&IVec2::new(1, 1)) && roof.contains(&IVec2::new(3, 1)), "the corridor");
        assert!(roof.contains(&IVec2::new(2, 1)), "the reactor inside");
        assert!(!roof.contains(&IVec2::new(5, 1)), "the gun on the bow");
        assert!(!roof.contains(&IVec2::new(0, 1)), "the skin is already plating");
    }

    #[test]
    fn a_weapon_inside_the_hull_stays_in_sight() {
        let mut hull = Vec::new();
        for x in 0..5 {
            for y in 0..5 {
                let edge = x == 0 || x == 4 || y == 0 || y == 4;
                hull.push((IVec2::new(x, y), if edge { HullLayer::Outer } else { HullLayer::Hallway }));
            }
        }
        let turret = (vec![IVec2::new(2, 2)], true);
        let quarters = (vec![IVec2::new(1, 1)], false);
        let roof = roof_cells(&hull, &[turret, quarters]);
        assert!(!roof.contains(&IVec2::new(2, 2)), "the turret in the middle");
        assert!(roof.contains(&IVec2::new(1, 1)), "the quarters beside it");
    }
}
