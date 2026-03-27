// Layout fields are part of the data model — not all are consumed yet.
#![allow(dead_code)]

use bevy::prelude::*;
use crate::components::{ModuleType, HullMaterial, HullLayer, Rotation};
use super::components::AiSubType;

pub struct HullCellDef {
    pub grid_pos: IVec2,
    pub layer: HullLayer,
    pub material: HullMaterial,
}

pub struct ModulePlacement {
    pub module_type: ModuleType,
    pub grid_pos: IVec2,
    pub rotation: Rotation,
}

pub struct AiSubLayout {
    pub hull_cells: Vec<HullCellDef>,
    pub modules: Vec<ModulePlacement>,
    pub body_size: Vec2,
    pub hull_material: HullMaterial,
}

pub fn get_layout(sub_type: AiSubType) -> AiSubLayout {
    match sub_type {
        AiSubType::Leviathan => leviathan_layout(),
        AiSubType::AbyssalCult => abyssal_cult_layout(),
        AiSubType::Drowned => drowned_layout(),
        AiSubType::PressureKing => pressure_king_layout(),
        AiSubType::GlassEye => glass_eye_layout(),
        AiSubType::IronTide => iron_tide_layout(),
        AiSubType::Blackwater => blackwater_layout(),
        AiSubType::RustSwarm => rust_swarm_layout(),
    }
}

/// Helper: build submarine-shaped hull from row definitions (y, x_min, x_max)
fn build_shaped_hull(rows: &[(i32, i32, i32)], material: HullMaterial) -> Vec<HullCellDef> {
    let mut hull_cells = Vec::new();
    for &(y, x_min, x_max) in rows {
        for x in x_min..=x_max {
            let is_top = !rows.iter().any(|&(ry, rxmin, rxmax)| ry == y + 1 && x >= rxmin && x <= rxmax);
            let is_bot = !rows.iter().any(|&(ry, rxmin, rxmax)| ry == y - 1 && x >= rxmin && x <= rxmax);
            let is_left = x == x_min;
            let is_right = x == x_max;
            let layer = if is_top || is_bot || is_left || is_right { HullLayer::Outer } else { HullLayer::Inner };
            hull_cells.push(HullCellDef { grid_pos: IVec2::new(x, y), layer, material });
        }
    }
    hull_cells
}

fn hull_size(rows: &[(i32, i32, i32)]) -> Vec2 {
    let x_min = rows.iter().map(|r| r.1).min().unwrap_or(0);
    let x_max = rows.iter().map(|r| r.2).max().unwrap_or(0);
    let y_min = rows.iter().map(|r| r.0).min().unwrap_or(0);
    let y_max = rows.iter().map(|r| r.0).max().unwrap_or(0);
    Vec2::new((x_max - x_min + 1) as f32 * 66.0, (y_max - y_min + 1) as f32 * 66.0)
}

// ============================================================================
// LEVIATHAN RIDERS - Creature-towed sub with harness/capture gear
// Organic-looking, wide for creature containment, net launchers on sides
// ============================================================================
fn leviathan_layout() -> AiSubLayout {
    let material = HullMaterial::Steel;
    let rows: &[(i32, i32, i32)] = &[
        ( 2,   2,  5),
        ( 1,   0,  6),
        ( 0,  -1,  7),
        (-1,  -1,  7),
        (-2,   0,  6),
        (-3,   2,  5),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // Small backup engines (main movement is creature-towed)
        ModulePlacement { module_type: ModuleType::SmallEngine, grid_pos: IVec2::new(0, 0), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::SmallEngine, grid_pos: IVec2::new(0, -1), rotation: Rotation::West },
        // Power
        ModulePlacement { module_type: ModuleType::SmallReactor, grid_pos: IVec2::new(1, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::BallastTank, grid_pos: IVec2::new(1, -1), rotation: Rotation::North },
        // Creature containment center
        ModulePlacement { module_type: ModuleType::CreatureContainment, grid_pos: IVec2::new(3, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::SpecimenVault, grid_pos: IVec2::new(3, -1), rotation: Rotation::North },
        // Net launchers on hull edges
        ModulePlacement { module_type: ModuleType::NetLauncher, grid_pos: IVec2::new(6, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::NetLauncher, grid_pos: IVec2::new(6, -1), rotation: Rotation::East },
        // Scanners for finding creatures
        ModulePlacement { module_type: ModuleType::CreatureScanner, grid_pos: IVec2::new(5, 1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::Floodlight, grid_pos: IVec2::new(7, 0), rotation: Rotation::East },
    ];
    AiSubLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}

// ============================================================================
// ABYSSAL CULT - Bio-organic hybrid, eerie bioluminescent, self-healing hull
// Composite material (organic), acid/electric bio-weapons
// ============================================================================
fn abyssal_cult_layout() -> AiSubLayout {
    let material = HullMaterial::Composite;
    // Organic, bulbous shape
    let rows: &[(i32, i32, i32)] = &[
        ( 3,   2,  4),
        ( 2,   0,  6),
        ( 1,  -1,  7),
        ( 0,  -2,  7),
        (-1,  -2,  7),
        (-2,  -1,  7),
        (-3,   0,  6),
        (-4,   2,  4),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // "Creature heart" reactor (standard reactor reflavored)
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(1, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(1, -1), rotation: Rotation::North },
        // Engines
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-1, 0), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-1, -1), rotation: Rotation::West },
        // Ballast
        ModulePlacement { module_type: ModuleType::BallastTank, grid_pos: IVec2::new(0, 0), rotation: Rotation::North },
        // Bio-weapons on exterior
        ModulePlacement { module_type: ModuleType::AcidSprayer, grid_pos: IVec2::new(6, 1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::AcidSprayer, grid_pos: IVec2::new(6, -2), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::ElectricDischarger, grid_pos: IVec2::new(5, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::SonicPulse, grid_pos: IVec2::new(4, 0), rotation: Rotation::North },
        // Healing/support
        ModulePlacement { module_type: ModuleType::RepairBay, grid_pos: IVec2::new(2, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::BasicQuarters, grid_pos: IVec2::new(3, 0), rotation: Rotation::North },
    ];
    AiSubLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}

// ============================================================================
// THE DROWNED - Ghost ships, partially destroyed, holes in hull
// Steel (rusted), modules randomly missing, eerie design
// ============================================================================
fn drowned_layout() -> AiSubLayout {
    let material = HullMaterial::Steel;
    // Damaged, asymmetric shape (holes represented by missing cells)
    let rows: &[(i32, i32, i32)] = &[
        ( 2,   3,  5),
        ( 1,   0,  7),
        ( 0,  -2,  8),
        (-1,  -2,  7),
        (-2,   0,  6),
        (-3,   3,  5),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // Barely functional engines
        ModulePlacement { module_type: ModuleType::SmallEngine, grid_pos: IVec2::new(-1, 0), rotation: Rotation::West },
        // Flickering reactor
        ModulePlacement { module_type: ModuleType::SmallReactor, grid_pos: IVec2::new(1, 0), rotation: Rotation::North },
        // Random weapons still active
        ModulePlacement { module_type: ModuleType::TorpedoTube, grid_pos: IVec2::new(7, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::PointDefense, grid_pos: IVec2::new(4, 1), rotation: Rotation::North },
        // Empty quarters (no crew)
        ModulePlacement { module_type: ModuleType::BasicQuarters, grid_pos: IVec2::new(3, 0), rotation: Rotation::North },
        // Old cargo
        ModulePlacement { module_type: ModuleType::SmallCargo, grid_pos: IVec2::new(5, 0), rotation: Rotation::North },
    ];
    AiSubLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}

// ============================================================================
// PRESSURE KINGS - Deep-zone heavy tanks, abyssal alloy, pressure weapons
// Compact, dense, extremely armored
// ============================================================================
fn pressure_king_layout() -> AiSubLayout {
    let material = HullMaterial::AbyssalAlloy;
    // Dense, compact diamond shape
    let rows: &[(i32, i32, i32)] = &[
        ( 3,   3,  5),
        ( 2,   1,  7),
        ( 1,  -1,  8),
        ( 0,  -2,  9),
        (-1,  -2,  9),
        (-2,  -1,  8),
        (-3,   1,  7),
        (-4,   3,  5),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // Powerful engines for ramming
        ModulePlacement { module_type: ModuleType::LargeEngine, grid_pos: IVec2::new(-1, 0), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-1, -1), rotation: Rotation::West },
        // Heavy power
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(1, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(1, -1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::BallastTank, grid_pos: IVec2::new(0, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::FuelTank, grid_pos: IVec2::new(0, -1), rotation: Rotation::North },
        // Pressure weapons on edges
        ModulePlacement { module_type: ModuleType::EMPEmitter, grid_pos: IVec2::new(7, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::SonicPulse, grid_pos: IVec2::new(7, -1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyTorpedoTube, grid_pos: IVec2::new(8, 0), rotation: Rotation::East },
        // Reinforced interior
        ModulePlacement { module_type: ModuleType::RepairBay, grid_pos: IVec2::new(3, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::BasicQuarters, grid_pos: IVec2::new(4, 0), rotation: Rotation::North },
        // Deep sensors
        ModulePlacement { module_type: ModuleType::DepthScanner, grid_pos: IVec2::new(5, 0), rotation: Rotation::East },
    ];
    AiSubLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}

// ============================================================================
// GLASS EYE - Stealth surveillance, narrow, sensor-heavy, no weapons
// Composite, silent drive, fastest flee speed
// ============================================================================
fn glass_eye_layout() -> AiSubLayout {
    let material = HullMaterial::Composite;
    // Long, thin needle shape
    let rows: &[(i32, i32, i32)] = &[
        ( 1,   2, 10),
        ( 0,  -3, 11),
        (-1,  -3, 11),
        (-2,   2, 10),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // Silent engines
        ModulePlacement { module_type: ModuleType::SmallEngine, grid_pos: IVec2::new(-2, 0), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::SmallEngine, grid_pos: IVec2::new(-2, -1), rotation: Rotation::West },
        // Quiet reactor
        ModulePlacement { module_type: ModuleType::SmallReactor, grid_pos: IVec2::new(0, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::BallastTank, grid_pos: IVec2::new(-1, 0), rotation: Rotation::North },
        // Stealth coating
        ModulePlacement { module_type: ModuleType::StealthCoating, grid_pos: IVec2::new(1, 0), rotation: Rotation::North },
        // Massive sensor array (the "glass eye")
        ModulePlacement { module_type: ModuleType::AdvancedSonar, grid_pos: IVec2::new(8, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::PassiveSonar, grid_pos: IVec2::new(7, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HydrophoneArray, grid_pos: IVec2::new(9, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::CreatureScanner, grid_pos: IVec2::new(6, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::MineralScanner, grid_pos: IVec2::new(5, 0), rotation: Rotation::East },
        // Signal buoy (broadcasts intel)
        ModulePlacement { module_type: ModuleType::SignalBuoy, grid_pos: IVec2::new(10, 0), rotation: Rotation::East },
    ];
    AiSubLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}

// ============================================================================
// IRON TIDE - Heavy battleship, massive hull, multiple weapon systems
// Titanium, slow but devastating firepower, the "boss" faction
// ============================================================================
fn iron_tide_layout() -> AiSubLayout {
    let material = HullMaterial::Titanium;
    // Massive wide battleship
    let rows: &[(i32, i32, i32)] = &[
        ( 4,   6,  9),
        ( 3,   3, 10),
        ( 2,   1, 11),
        ( 1,  -2, 12),
        ( 0,  -4, 12),
        (-1,  -4, 12),
        (-2,  -2, 12),
        (-3,   1, 11),
        (-4,   3, 10),
        (-5,   6,  9),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // 4 large engines
        ModulePlacement { module_type: ModuleType::LargeEngine, grid_pos: IVec2::new(-3, 0), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::LargeEngine, grid_pos: IVec2::new(-3, -1), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-1, 0), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-1, -1), rotation: Rotation::West },
        // Heavy power plant
        ModulePlacement { module_type: ModuleType::LargeReactor, grid_pos: IVec2::new(2, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(2, -1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(4, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::FuelTank, grid_pos: IVec2::new(0, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::FuelTank, grid_pos: IVec2::new(0, -1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::BallastTank, grid_pos: IVec2::new(1, 0), rotation: Rotation::North },
        // Crew
        ModulePlacement { module_type: ModuleType::Barracks, grid_pos: IVec2::new(5, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::BasicQuarters, grid_pos: IVec2::new(6, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::MessHall, grid_pos: IVec2::new(6, -1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::RepairBay, grid_pos: IVec2::new(3, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::AdvancedRepairBay, grid_pos: IVec2::new(3, -1), rotation: Rotation::North },
        // Weapons array (devastating)
        ModulePlacement { module_type: ModuleType::RailGun, grid_pos: IVec2::new(11, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyTorpedoTube, grid_pos: IVec2::new(11, 2), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyTorpedoTube, grid_pos: IVec2::new(11, -3), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::TorpedoTube, grid_pos: IVec2::new(10, 1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::TorpedoTube, grid_pos: IVec2::new(10, -2), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::FlakCannon, grid_pos: IVec2::new(8, 3), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::FlakCannon, grid_pos: IVec2::new(8, -4), rotation: Rotation::South },
        ModulePlacement { module_type: ModuleType::PointDefense, grid_pos: IVec2::new(7, 2), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::PointDefense, grid_pos: IVec2::new(7, -3), rotation: Rotation::South },
        // Bridge
        ModulePlacement { module_type: ModuleType::HelmStation, grid_pos: IVec2::new(8, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::SonarArray, grid_pos: IVec2::new(9, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::TargetingComputer, grid_pos: IVec2::new(9, -1), rotation: Rotation::East },
    ];
    AiSubLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}

// ============================================================================
// BLACKWATER PMC - Elite tactical sub, balanced, flanking design
// Titanium, well-armed but not overkill, designed for coordination
// ============================================================================
fn blackwater_layout() -> AiSubLayout {
    let material = HullMaterial::Titanium;
    let rows: &[(i32, i32, i32)] = &[
        ( 2,   4,  7),
        ( 1,   1,  9),
        ( 0,  -2, 10),
        (-1,  -2, 10),
        (-2,   1,  9),
        (-3,   4,  7),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // Fast engines
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-1, 0), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-1, -1), rotation: Rotation::West },
        // Power
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(1, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::FuelTank, grid_pos: IVec2::new(0, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::BallastTank, grid_pos: IVec2::new(0, -1), rotation: Rotation::North },
        // Tactical systems
        ModulePlacement { module_type: ModuleType::TargetingComputer, grid_pos: IVec2::new(3, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::SonarArray, grid_pos: IVec2::new(7, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::RepairBay, grid_pos: IVec2::new(2, 0), rotation: Rotation::North },
        // Crew
        ModulePlacement { module_type: ModuleType::BasicQuarters, grid_pos: IVec2::new(4, 0), rotation: Rotation::North },
        // Weapons (precise, not overwhelming)
        ModulePlacement { module_type: ModuleType::TorpedoTube, grid_pos: IVec2::new(9, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::TorpedoTube, grid_pos: IVec2::new(9, -1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::PointDefense, grid_pos: IVec2::new(6, 1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::PointDefense, grid_pos: IVec2::new(6, -2), rotation: Rotation::South },
    ];
    AiSubLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}

// ============================================================================
// RUST SWARM - Tiny junk subs, minimal systems, expendable
// Steel (rusted), asymmetric, few modules, kamikaze tendencies
// ============================================================================
fn rust_swarm_layout() -> AiSubLayout {
    let material = HullMaterial::Steel;
    // Tiny asymmetric junk sub
    let rows: &[(i32, i32, i32)] = &[
        ( 1,   1,  3),
        ( 0,  -1,  4),
        (-1,  -1,  4),
        (-2,   1,  3),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // One sputtering engine
        ModulePlacement { module_type: ModuleType::SmallEngine, grid_pos: IVec2::new(0, 0), rotation: Rotation::West },
        // Tiny reactor barely keeping things running
        ModulePlacement { module_type: ModuleType::SmallReactor, grid_pos: IVec2::new(1, 0), rotation: Rotation::North },
        // One weapon (mine layer - cheap and dirty)
        ModulePlacement { module_type: ModuleType::MineLayer, grid_pos: IVec2::new(3, 0), rotation: Rotation::East },
        // Scrap cargo
        ModulePlacement { module_type: ModuleType::SmallCargo, grid_pos: IVec2::new(2, 0), rotation: Rotation::North },
    ];
    AiSubLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}
