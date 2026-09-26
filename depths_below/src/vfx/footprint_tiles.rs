//! Drawing a non-rectangular block as the shape it actually occupies.
//!
//! Most modules fill a plain WxH rectangle and one sprite stretched over that
//! rectangle is the truth. Eight do not: the T-tetromino sickbay and bridge
//! wing, the L-tromino galley, hold and corner plate, the S-tetromino
//! staggered plating, the plus-pentomino hubs. `ModuleDef.size` gives their
//! BOUNDING BOX, and `footprints::footprint_override` gives the cells inside
//! it they really claim — four of six, three of four, five of nine.
//!
//! Sizing one sprite to the bounding box therefore painted cells the block
//! does not own, straight over whatever the neighbour had put there. A
//! Surgical Bay covered six cells and occupied four; the two corners of its
//! bar sat on top of the corridor beside it. Reported as "some of the blocks
//! sprites aren't adding up to the actual block size".
//!
//! The art is authored as a clean grid — every one of these files is exactly
//! 378 px per cell — so the fix is to cut it up and lay one tile per cell the
//! block owns. Nothing is stretched and nothing is redrawn; the corners simply
//! stop being painted.
//!
//! It has to be deferred: `Sprite::rect` is in texture pixels, and at spawn
//! the image is usually still loading. The marker survives until the size is
//! known, which is normally the next frame or two.

use bevy::prelude::*;

use crate::components::Module;

/// A module whose art still has to be cut to its footprint.
///
/// `cells` are the UNROTATED footprint offsets and `origin`/`span` the
/// unrotated bounding box, all in cells. Children are authored in that
/// unrotated frame and the parent's own rotation turns them, the same way the
/// wedge plating builds its triangle — so all four orientations fall out
/// without a second code path. Sound only because every module in the
/// override table has a zero `sprite_base_rotation`; an engine or a gun would
/// need the art's base offset taken out of the child placement first.
#[derive(Component, Clone)]
pub struct FootprintTiles {
    pub image: Handle<Image>,
    pub cells: Vec<IVec2>,
    pub origin: IVec2,
    pub span: IVec2,
}

/// Cell pitch in world units. Matches the hull grid.
const CELL: f32 = 66.0;

pub fn cut_art_to_footprint(
    mut commands: Commands,
    images: Res<Assets<Image>>,
    pending: Query<(Entity, &FootprintTiles), With<Module>>,
) {
    for (entity, tiles) in pending.iter() {
        let Some(image) = images.get(&tiles.image) else {
            continue; // still loading; try again next frame
        };
        let size = image.size().as_vec2();
        let span = tiles.span.as_vec2();
        let tile = size / span;

        // The bounding box centre, in cells, relative to the module origin.
        // The parent already sits there, so children hang off it.
        let centre = tiles.origin.as_vec2() + (span - Vec2::ONE) * 0.5;

        for cell in &tiles.cells {
            let col = (cell.x - tiles.origin.x) as f32;
            // Texture rows run top-down, grid rows bottom-up.
            let row = span.y - 1.0 - (cell.y - tiles.origin.y) as f32;
            let min = Vec2::new(col * tile.x, row * tile.y);
            let child = commands
                .spawn((
                    Sprite {
                        image: tiles.image.clone(),
                        rect: Some(Rect { min, max: min + tile }),
                        custom_size: Some(Vec2::splat(CELL)),
                        ..default()
                    },
                    Transform::from_xyz(
                        (cell.x as f32 - centre.x) * CELL,
                        (cell.y as f32 - centre.y) * CELL,
                        0.01,
                    ),
                ))
                .id();
            commands.entity(entity).add_child(child);
        }
        commands.entity(entity).remove::<FootprintTiles>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::building::footprints::footprint_override;
    use crate::components::ModuleType;

    /// Width and height out of a PNG's IHDR, which is always the first chunk:
    /// 8-byte signature, 4-byte length, 4-byte type, then the two dimensions
    /// as big-endian u32s.
    fn png_size(path: &str) -> (u32, u32) {
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(&bytes[12..16], b"IHDR", "{path} is not a PNG");
        let n = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        (n(16), n(20))
    }

    /// The shapes in `footprints::footprint_override`, and what has to be
    /// true of each for the art to be cuttable. GalleyMess and BulkCargoHold
    /// were here until they went back to plain 2x2 rooms -- this list going
    /// stale is what this assert is for, so keep it in step with the table.
    ///
    /// Two things. It must claim FEWER cells than its bounding box, or it does
    /// not belong in the table at all. And its texture must divide into a
    /// whole number of pixels per cell, or every tile slices off-register and
    /// shows a seam through the middle of the art instead of at the edge.
    #[test]
    fn the_overridden_shapes_all_tile_cleanly() {
        let registry = crate::building::registry::build_registry();
        let shaped = [
            ModuleType::CornerArmorPlate,
            ModuleType::BridgeWing,
            ModuleType::SurgicalBay,
            ModuleType::StaggeredArmorPlate,
            ModuleType::DockingHub,
            ModuleType::WellnessHub,
        ];
        for module_type in shaped {
            let cells = footprint_override(module_type)
                .unwrap_or_else(|| panic!("{module_type:?} lost its footprint override"));
            let size = registry.get(module_type).size;
            let area = (size.x * size.y) as usize;
            assert!(
                cells.len() < area,
                "{module_type:?} overrides its footprint but still fills the whole {}x{} box",
                size.x, size.y
            );
            let Some(sprite) = crate::sprite_map::module_sprite_path(module_type) else {
                panic!("{module_type:?} has no sprite to cut");
            };
            let (w, h) = png_size(&format!("assets/{sprite}"));
            assert_eq!(
                (w % size.x as u32, h % size.y as u32),
                (0, 0),
                "{module_type:?}: {sprite} is {w}x{h}, which is not a whole number of \
                 pixels per cell across a {}x{} box",
                size.x, size.y
            );
        }
    }
}
