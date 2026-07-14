use bevy::prelude::IVec2;
use crate::components::ModuleType;

// ============================================================================
// NON-RECTANGULAR FOOTPRINTS
// Most modules occupy a plain WxH rectangle (ModuleDef.size). A handful can
// instead occupy an explicit, non-rectangular set of cells within their
// bounding box — this is the override table for those.
// Offsets are relative to the module's origin cell (0,0) and get rotated the
// same way rectangle corners do, so all 4 orientations fall out for free.
//
// Each shape here is picked because of what the module *does*, not because
// the shape looks interesting — see MODULES.md for the reasoning per module.
// ============================================================================

// L-tromino, notch open toward +x/+y (top-right of its 2x2 bounding box).
// Used by corner-hugging armor and anything that wraps a hull corner.
const L_TROMINO_A: [IVec2; 3] = [
    IVec2::new(0, 0),
    IVec2::new(1, 0),
    IVec2::new(0, 1),
];

// L-tromino, notch open toward -x/-y (bottom-left) — the mirror orientation.
// Used for the "long run + a nook" shape (galley corridor + dining nook,
// cargo hold filling a leftover corner).
const L_TROMINO_B: [IVec2; 3] = [
    IVec2::new(0, 0),
    IVec2::new(1, 0),
    IVec2::new(1, 1),
];

// T-tetromino — a 3-wide bar with a single stem, 3x2 bounding box.
// Used for "wide top for field of view / treatment area, narrow stem for
// access back into the ship" (bridge wings, triage-to-treatment sickbay).
const T_TETROMINO: [IVec2; 4] = [
    IVec2::new(0, 0),
    IVec2::new(1, 0),
    IVec2::new(2, 0),
    IVec2::new(1, 1),
];

// S-tetromino — offset zigzag, 3x2 bounding box.
// Used for staggered armor plating: no single straight seam runs through it.
const S_TETROMINO: [IVec2; 4] = [
    IVec2::new(0, 0),
    IVec2::new(1, 0),
    IVec2::new(1, 1),
    IVec2::new(2, 1),
];

// Plus/cross pentomino — center + all 4 cardinal neighbors, 3x3 bounding box.
// Used for true multi-directional hubs (multiple simultaneous docking
// connections, a la ISS node modules) — not a routing/logistics network,
// just a room with more than one "side."
const PLUS_PENTOMINO: [IVec2; 5] = [
    IVec2::new(1, 0),
    IVec2::new(0, 1),
    IVec2::new(1, 1),
    IVec2::new(2, 1),
    IVec2::new(1, 2),
];

pub fn footprint_override(module_type: ModuleType) -> Option<&'static [IVec2]> {
    match module_type {
        ModuleType::CornerArmorPlate => Some(&L_TROMINO_A),
        ModuleType::GalleyMess | ModuleType::BulkCargoHold => Some(&L_TROMINO_B),
        ModuleType::BridgeWing | ModuleType::SurgicalBay => Some(&T_TETROMINO),
        ModuleType::StaggeredArmorPlate => Some(&S_TETROMINO),
        ModuleType::DockingHub | ModuleType::WellnessHub => Some(&PLUS_PENTOMINO),
        _ => None,
    }
}
