// Layout fields are part of the data model — not all are consumed yet.
#![allow(dead_code)]

use bevy::prelude::*;
use crate::components::{ModuleType, HullMaterial, HullLayer, Rotation};
use super::components::AiShipType;

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

pub struct AiShipLayout {
    pub hull_cells: Vec<HullCellDef>,
    pub modules: Vec<ModulePlacement>,
    pub body_size: Vec2,
    pub hull_material: HullMaterial,
}

impl AiShipLayout {
    /// Converts a built-in layout to the canonical design format
    /// (building::blueprint). The layouts in this file are the FALLBACK —
    /// designs/factions/<slug>.json wins when present, and each layout
    /// self-exports there on first spawn. Edit the JSON, not this file.
    pub fn to_design(&self, name: &str) -> crate::building::blueprint::Blueprint {
        use crate::building::blueprint::{Blueprint, BlueprintHullCell, BlueprintModule, BLUEPRINT_VERSION};
        Blueprint {
            name: name.into(),
            hull_cells: self.hull_cells.iter().map(|c| BlueprintHullCell {
                grid_pos: c.grid_pos,
                layer: c.layer,
                material: c.material,
            }).collect(),
            modules: self.modules.iter().map(|m| BlueprintModule {
                module_type: m.module_type,
                grid_pos: m.grid_pos,
                rotation: m.rotation,
                custom_name: None,
                subcomponents: None,
                extras: None,
            }).collect(),
            created_at: "builtin".into(),
            version: BLUEPRINT_VERSION,
        }
    }
}

/// Stable file name per faction (designs/factions/<slug>.json).
pub fn design_slug(ship_type: AiShipType) -> &'static str {
    match ship_type {
        AiShipType::Leviathan => "leviathan",
        AiShipType::AbyssalCult => "abyssal_cult",
        AiShipType::Drowned => "drowned",
        AiShipType::PressureKing => "pressure_king",
        AiShipType::GlassEye => "glass_eye",
        AiShipType::IronTide => "iron_tide",
        AiShipType::Blackwater => "blackwater",
        AiShipType::RustSwarm => "rust_swarm",
        AiShipType::Dreadnought => "dreadnought",
        AiShipType::VoidTitan => "void_titan",
    }
}

pub fn get_layout(ship_type: AiShipType) -> AiShipLayout {
    match ship_type {
        AiShipType::Leviathan => leviathan_layout(),
        AiShipType::AbyssalCult => abyssal_cult_layout(),
        AiShipType::Drowned => drowned_layout(),
        AiShipType::PressureKing => pressure_king_layout(),
        AiShipType::GlassEye => glass_eye_layout(),
        AiShipType::IronTide => iron_tide_layout(),
        AiShipType::Blackwater => blackwater_layout(),
        AiShipType::RustSwarm => rust_swarm_layout(),
        AiShipType::Dreadnought => dreadnought_layout(),
        AiShipType::VoidTitan => void_titan_layout(),
    }
}

/// Helper: build ship-shaped hull from row definitions (y, x_min, x_max)
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
// LEVIATHAN RIDERS - Creature-towed ship with harness/capture gear
// Organic-looking, wide for creature containment, net launchers on sides
// ============================================================================
fn leviathan_layout() -> AiShipLayout {
    let material = HullMaterial::Steel;
    let rows: &[(i32, i32, i32)] = &[
        ( 3,   4,  7),
        ( 2,   1,  8),
        ( 1,  -2,  9),
        ( 0,  -3, 10),
        (-1,  -3, 10),
        (-2,  -2,  9),
        (-3,   1,  8),
        (-4,   4,  7),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // Small backup engines (main movement is creature-towed)
        ModulePlacement { module_type: ModuleType::SmallEngine, grid_pos: IVec2::new(-3, 1), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::SmallEngine, grid_pos: IVec2::new(-3, -1), rotation: Rotation::West },
        // Power
        ModulePlacement { module_type: ModuleType::SmallReactor, grid_pos: IVec2::new(-1, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(0, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::ManeuverThruster, grid_pos: IVec2::new(-2, 0), rotation: Rotation::North },
        // Creature containment — now a proper menagerie
        ModulePlacement { module_type: ModuleType::CreatureContainment, grid_pos: IVec2::new(2, 1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::CreatureContainment, grid_pos: IVec2::new(2, -1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::SpecimenVault, grid_pos: IVec2::new(1, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::SpecimenVault, grid_pos: IVec2::new(3, 0), rotation: Rotation::North },
        // Net launchers and light weapons on hull edges
        ModulePlacement { module_type: ModuleType::TractorBeam, grid_pos: IVec2::new(9, 1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::TractorBeam, grid_pos: IVec2::new(9, -1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Gatling, grid_pos: IVec2::new(8, 2), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::Gatling, grid_pos: IVec2::new(8, -2), rotation: Rotation::South },
        // Scanners for finding creatures
        ModulePlacement { module_type: ModuleType::CreatureScanner, grid_pos: IVec2::new(6, 2), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::Floodlight, grid_pos: IVec2::new(10, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::BasicQuarters, grid_pos: IVec2::new(5, 0), rotation: Rotation::North },
    ];
    AiShipLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}

// ============================================================================
// ABYSSAL CULT - Bio-organic hybrid, eerie bioluminescent, self-healing hull
// Composite material (organic), acid/electric bio-weapons
// ============================================================================
fn abyssal_cult_layout() -> AiShipLayout {
    let material = HullMaterial::Composite;
    // Organic, bulbous shape
    let rows: &[(i32, i32, i32)] = &[
        ( 4,   3,  6),
        ( 3,   0,  9),
        ( 2,  -2, 10),
        ( 1,  -4, 10),
        ( 0,  -5, 10),
        (-1,  -5, 10),
        (-2,  -4, 10),
        (-3,  -2, 10),
        (-4,   0,  9),
        (-5,   3,  6),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // "Creature heart" reactor cluster (standard reactors reflavored)
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(1, 1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(1, -1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(2, 0), rotation: Rotation::North },
        // Engines
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-3, 1), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-3, -1), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::SmallEngine, grid_pos: IVec2::new(-3, 0), rotation: Rotation::West },
        // Thruster
        ModulePlacement { module_type: ModuleType::ManeuverThruster, grid_pos: IVec2::new(-1, 0), rotation: Rotation::North },
        // Bio-weapons on exterior — more coverage
        ModulePlacement { module_type: ModuleType::PlasmaCaster, grid_pos: IVec2::new(9, 1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::PlasmaCaster, grid_pos: IVec2::new(9, -2), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::PlasmaCaster, grid_pos: IVec2::new(8, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::PlasmaCaster, grid_pos: IVec2::new(7, 2), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::IonDisruptor, grid_pos: IVec2::new(6, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::IonDisruptor, grid_pos: IVec2::new(7, -3), rotation: Rotation::South },
        // Healing/support — this hull regenerates
        ModulePlacement { module_type: ModuleType::RepairBay, grid_pos: IVec2::new(3, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::RepairBay, grid_pos: IVec2::new(4, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::BasicQuarters, grid_pos: IVec2::new(5, 0), rotation: Rotation::North },
    ];
    AiShipLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}

// ============================================================================
// THE DROWNED - Ghost ships, partially destroyed, holes in hull
// Steel (rusted), modules randomly missing, eerie design
// ============================================================================
fn drowned_layout() -> AiShipLayout {
    let material = HullMaterial::Steel;
    // Damaged, asymmetric shape (holes represented by missing cells)
    let rows: &[(i32, i32, i32)] = &[
        ( 3,   5,  8),
        ( 2,   1, 10),
        ( 1,  -3, 11),
        ( 0,  -5, 12),
        (-1,  -5, 10),
        (-2,  -2,  9),
        (-3,   1,  8),
        (-4,   5,  7),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // Barely functional engines
        ModulePlacement { module_type: ModuleType::SmallEngine, grid_pos: IVec2::new(-2, 0), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::SmallEngine, grid_pos: IVec2::new(-2, -1), rotation: Rotation::West },
        // Flickering reactors
        ModulePlacement { module_type: ModuleType::SmallReactor, grid_pos: IVec2::new(1, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::SmallReactor, grid_pos: IVec2::new(0, -1), rotation: Rotation::North },
        // Random weapons still active
        ModulePlacement { module_type: ModuleType::HeavyMissile, grid_pos: IVec2::new(11, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyMissile, grid_pos: IVec2::new(9, -4), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Gatling, grid_pos: IVec2::new(6, 2), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::Gatling, grid_pos: IVec2::new(7, -4), rotation: Rotation::South },
        ModulePlacement { module_type: ModuleType::ClusterRocket, grid_pos: IVec2::new(8, 1), rotation: Rotation::East },
        // Empty quarters (no crew)
        ModulePlacement { module_type: ModuleType::BasicQuarters, grid_pos: IVec2::new(4, 0), rotation: Rotation::North },
        // Old cargo
        ModulePlacement { module_type: ModuleType::SmallCargo, grid_pos: IVec2::new(6, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::SmallCargo, grid_pos: IVec2::new(3, -2), rotation: Rotation::North },
    ];
    AiShipLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}

// ============================================================================
// PRESSURE KINGS - Deep-zone heavy tanks, abyssal alloy, pressure weapons
// Compact, dense, extremely armored
// ============================================================================
fn pressure_king_layout() -> AiShipLayout {
    let material = HullMaterial::AbyssalAlloy;
    // Dense, compact diamond shape
    let rows: &[(i32, i32, i32)] = &[
        ( 4,   4,  7),
        ( 3,   1,  9),
        ( 2,  -1, 11),
        ( 1,  -3, 12),
        ( 0,  -4, 13),
        (-1,  -4, 13),
        (-2,  -3, 12),
        (-3,  -1, 11),
        (-4,   1,  9),
        (-5,   4,  7),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // Powerful engines for ramming
        ModulePlacement { module_type: ModuleType::LargeEngine, grid_pos: IVec2::new(-3, 1), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::LargeEngine, grid_pos: IVec2::new(-3, -1), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-3, 0), rotation: Rotation::West },
        // Heavy power
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(1, 1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(1, -1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::LargeReactor, grid_pos: IVec2::new(0, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::ManeuverThruster, grid_pos: IVec2::new(-1, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::FuelTank, grid_pos: IVec2::new(-2, 0), rotation: Rotation::North },
        // Pressure weapons on edges — a full battery now
        ModulePlacement { module_type: ModuleType::EMPPulse, grid_pos: IVec2::new(11, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::IonDisruptor, grid_pos: IVec2::new(11, 1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::IonDisruptor, grid_pos: IVec2::new(11, -2), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Railgun, grid_pos: IVec2::new(12, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Railgun, grid_pos: IVec2::new(10, 2), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyMissile, grid_pos: IVec2::new(9, -3), rotation: Rotation::East },
        // Reinforced interior
        ModulePlacement { module_type: ModuleType::RepairBay, grid_pos: IVec2::new(4, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::AdvancedRepairBay, grid_pos: IVec2::new(5, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::BasicQuarters, grid_pos: IVec2::new(6, 0), rotation: Rotation::North },
        // Deep sensors
        ModulePlacement { module_type: ModuleType::DepthScanner, grid_pos: IVec2::new(7, 0), rotation: Rotation::East },
    ];
    AiShipLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}

// ============================================================================
// GLASS EYE - Stealth surveillance, narrow, sensor-heavy, no weapons
// Composite, silent drive, fastest flee speed
// ============================================================================
fn glass_eye_layout() -> AiShipLayout {
    let material = HullMaterial::Composite;
    // Long, thin needle shape
    let rows: &[(i32, i32, i32)] = &[
        ( 2,   4, 12),
        ( 1,  -4, 14),
        ( 0,  -6, 15),
        (-1,  -6, 15),
        (-2,  -4, 14),
        (-3,   4, 12),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // Silent engines
        ModulePlacement { module_type: ModuleType::SmallEngine, grid_pos: IVec2::new(-4, 1), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::SmallEngine, grid_pos: IVec2::new(-4, -1), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::SmallEngine, grid_pos: IVec2::new(-4, 0), rotation: Rotation::West },
        // Quiet reactors
        ModulePlacement { module_type: ModuleType::SmallReactor, grid_pos: IVec2::new(-1, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::SmallReactor, grid_pos: IVec2::new(0, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::ManeuverThruster, grid_pos: IVec2::new(-2, 0), rotation: Rotation::North },
        // Stealth coating
        ModulePlacement { module_type: ModuleType::StealthCoating, grid_pos: IVec2::new(1, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::StealthCoating, grid_pos: IVec2::new(2, 0), rotation: Rotation::North },
        // Massive sensor array (the "glass eye") — the whole point of the ship
        ModulePlacement { module_type: ModuleType::AdvancedRadar, grid_pos: IVec2::new(11, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::PassiveRadar, grid_pos: IVec2::new(10, 1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::PassiveRadar, grid_pos: IVec2::new(10, -1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HydrophoneArray, grid_pos: IVec2::new(12, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::CreatureScanner, grid_pos: IVec2::new(8, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::MineralScanner, grid_pos: IVec2::new(6, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::DepthScanner, grid_pos: IVec2::new(4, 0), rotation: Rotation::East },
        // Signal buoy (broadcasts intel)
        ModulePlacement { module_type: ModuleType::SignalBuoy, grid_pos: IVec2::new(13, 0), rotation: Rotation::East },
    ];
    AiShipLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}

// ============================================================================
// IRON TIDE - Heavy battleship, massive hull, multiple weapon systems.
// Titanium, slow but devastating firepower — the strongest "normal" faction,
// though the true bosses (Dreadnought, Void Titan) now dwarf even this.
// ============================================================================
fn iron_tide_layout() -> AiShipLayout {
    let material = HullMaterial::Titanium;
    // Massive wide battleship
    let rows: &[(i32, i32, i32)] = &[
        ( 5,   7, 10),
        ( 4,   4, 12),
        ( 3,   1, 13),
        ( 2,  -1, 14),
        ( 1,  -3, 14),
        ( 0,  -5, 14),
        (-1,  -5, 14),
        (-2,  -3, 14),
        (-3,  -1, 14),
        (-4,   1, 13),
        (-5,   4, 12),
        (-6,   7, 10),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // 4 large engines
        ModulePlacement { module_type: ModuleType::LargeEngine, grid_pos: IVec2::new(-4, 1), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::LargeEngine, grid_pos: IVec2::new(-4, -1), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-2, 1), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-2, -1), rotation: Rotation::West },
        // Heavy power plant
        ModulePlacement { module_type: ModuleType::LargeReactor, grid_pos: IVec2::new(2, 1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(2, -1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(4, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::FuelTank, grid_pos: IVec2::new(0, 1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::FuelTank, grid_pos: IVec2::new(0, -1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::ManeuverThruster, grid_pos: IVec2::new(1, 0), rotation: Rotation::North },
        // Crew
        ModulePlacement { module_type: ModuleType::Barracks, grid_pos: IVec2::new(6, 1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::BasicQuarters, grid_pos: IVec2::new(7, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::MessHall, grid_pos: IVec2::new(7, -1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::RepairBay, grid_pos: IVec2::new(3, 1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::AdvancedRepairBay, grid_pos: IVec2::new(3, -1), rotation: Rotation::North },
        // Weapons array (devastating)
        ModulePlacement { module_type: ModuleType::Railgun, grid_pos: IVec2::new(13, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Railgun, grid_pos: IVec2::new(13, 2), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Railgun, grid_pos: IVec2::new(13, -3), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyMissile, grid_pos: IVec2::new(12, 1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyMissile, grid_pos: IVec2::new(12, -2), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyMissile, grid_pos: IVec2::new(11, 4), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Cannon, grid_pos: IVec2::new(9, 3), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::Cannon, grid_pos: IVec2::new(9, -4), rotation: Rotation::South },
        ModulePlacement { module_type: ModuleType::Gatling, grid_pos: IVec2::new(8, 2), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::Gatling, grid_pos: IVec2::new(8, -3), rotation: Rotation::South },
        ModulePlacement { module_type: ModuleType::PointDefenseDrone, grid_pos: IVec2::new(5, 3), rotation: Rotation::North },
        // Bridge
        ModulePlacement { module_type: ModuleType::HelmStation, grid_pos: IVec2::new(9, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::RadarArray, grid_pos: IVec2::new(11, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::TargetingComputer, grid_pos: IVec2::new(10, -1), rotation: Rotation::East },
    ];
    AiShipLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}

// ============================================================================
// BLACKWATER PMC - Elite tactical ship, balanced, flanking design
// Titanium, well-armed but not overkill, designed for coordination
// ============================================================================
fn blackwater_layout() -> AiShipLayout {
    let material = HullMaterial::Titanium;
    let rows: &[(i32, i32, i32)] = &[
        ( 3,   6, 10),
        ( 2,   2, 12),
        ( 1,  -1, 13),
        ( 0,  -3, 14),
        (-1,  -3, 14),
        (-2,  -1, 13),
        (-3,   2, 12),
        (-4,   6, 10),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // Fast engines
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-2, 1), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-2, -1), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::SmallEngine, grid_pos: IVec2::new(-2, 0), rotation: Rotation::West },
        // Power
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(1, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::FuelTank, grid_pos: IVec2::new(0, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::ManeuverThruster, grid_pos: IVec2::new(0, -1), rotation: Rotation::North },
        // Tactical systems
        ModulePlacement { module_type: ModuleType::TargetingComputer, grid_pos: IVec2::new(4, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::RadarArray, grid_pos: IVec2::new(10, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::RepairBay, grid_pos: IVec2::new(3, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::StealthCoating, grid_pos: IVec2::new(2, 0), rotation: Rotation::North },
        // Crew
        ModulePlacement { module_type: ModuleType::BasicQuarters, grid_pos: IVec2::new(5, 0), rotation: Rotation::North },
        // Weapons (precise, not overwhelming, but more of it)
        ModulePlacement { module_type: ModuleType::HeavyMissile, grid_pos: IVec2::new(12, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyMissile, grid_pos: IVec2::new(12, -1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Railgun, grid_pos: IVec2::new(11, 2), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Gatling, grid_pos: IVec2::new(8, 1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::Gatling, grid_pos: IVec2::new(8, -2), rotation: Rotation::South },
        ModulePlacement { module_type: ModuleType::IonDisruptor, grid_pos: IVec2::new(6, 2), rotation: Rotation::North },
    ];
    AiShipLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}

// ============================================================================
// RUST SWARM - Tiny junk ships, minimal systems, expendable
// Steel (rusted), asymmetric, few modules, kamikaze tendencies
// ============================================================================
fn rust_swarm_layout() -> AiShipLayout {
    let material = HullMaterial::Steel;
    // Tiny asymmetric junk ship — a bit bigger than before, but still the
    // smallest thing flying. "Tiny and expendable" is the whole point.
    let rows: &[(i32, i32, i32)] = &[
        ( 2,   2,  5),
        ( 1,  -1,  6),
        ( 0,  -2,  6),
        (-1,  -2,  6),
        (-2,  -1,  6),
        (-3,   2,  5),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // One sputtering engine
        ModulePlacement { module_type: ModuleType::SmallEngine, grid_pos: IVec2::new(-1, 0), rotation: Rotation::West },
        // Tiny reactor barely keeping things running
        ModulePlacement { module_type: ModuleType::SmallReactor, grid_pos: IVec2::new(0, 0), rotation: Rotation::North },
        // Two weapons now (mine layer + a scavenged gun - cheap and dirty)
        ModulePlacement { module_type: ModuleType::ClusterRocket, grid_pos: IVec2::new(5, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Gatling, grid_pos: IVec2::new(4, 1), rotation: Rotation::North },
        // Scrap cargo
        ModulePlacement { module_type: ModuleType::SmallCargo, grid_pos: IVec2::new(2, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::SmallCargo, grid_pos: IVec2::new(3, -1), rotation: Rotation::North },
    ];
    AiShipLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}

// ============================================================================
// DREADNOUGHT - Iron Tide's design taken to its limit: a true mega-battleship.
// Titanium, roughly 1.5x Iron Tide's footprint in every dimension, with
// weapon coverage to match. Spawns only far past the star system — finding
// one at all is most of the fight.
// ============================================================================
fn dreadnought_layout() -> AiShipLayout {
    let material = HullMaterial::Titanium;
    let rows: &[(i32, i32, i32)] = &[
        ( 6,   9, 13),
        ( 5,   6, 15),
        ( 4,   3, 16),
        ( 3,   0, 17),
        ( 2,  -3, 17),
        ( 1,  -6, 17),
        ( 0,  -8, 17),
        (-1,  -8, 17),
        (-2,  -6, 17),
        (-3,  -3, 17),
        (-4,   0, 17),
        (-5,   3, 16),
        (-6,   6, 15),
        (-7,   9, 13),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // Six large engines — this thing is heavy
        ModulePlacement { module_type: ModuleType::LargeEngine, grid_pos: IVec2::new(-6, 1), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::LargeEngine, grid_pos: IVec2::new(-6, 0), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::LargeEngine, grid_pos: IVec2::new(-6, -1), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-4, 1), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-4, -1), rotation: Rotation::West },
        // Power plant — a small city's worth
        ModulePlacement { module_type: ModuleType::LargeReactor, grid_pos: IVec2::new(3, 1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::LargeReactor, grid_pos: IVec2::new(3, -1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(5, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::FuelTank, grid_pos: IVec2::new(1, 1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::FuelTank, grid_pos: IVec2::new(1, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::FuelTank, grid_pos: IVec2::new(1, -1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::ManeuverThruster, grid_pos: IVec2::new(2, 0), rotation: Rotation::North },
        // Crew — a real complement
        ModulePlacement { module_type: ModuleType::Barracks, grid_pos: IVec2::new(7, 1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Barracks, grid_pos: IVec2::new(7, -1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::BasicQuarters, grid_pos: IVec2::new(8, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::MessHall, grid_pos: IVec2::new(9, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::AdvancedRepairBay, grid_pos: IVec2::new(4, 1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::AdvancedRepairBay, grid_pos: IVec2::new(4, -1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::RepairBay, grid_pos: IVec2::new(4, 0), rotation: Rotation::North },
        // Weapons array — nearly double Iron Tide's coverage
        ModulePlacement { module_type: ModuleType::Railgun, grid_pos: IVec2::new(16, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Railgun, grid_pos: IVec2::new(16, 3), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Railgun, grid_pos: IVec2::new(16, -4), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Railgun, grid_pos: IVec2::new(15, 5), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyMissile, grid_pos: IVec2::new(14, 2), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyMissile, grid_pos: IVec2::new(14, -3), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyMissile, grid_pos: IVec2::new(13, 4), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyMissile, grid_pos: IVec2::new(13, -5), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Cannon, grid_pos: IVec2::new(11, 5), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::Cannon, grid_pos: IVec2::new(11, -6), rotation: Rotation::South },
        ModulePlacement { module_type: ModuleType::Cannon, grid_pos: IVec2::new(9, 6), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::Cannon, grid_pos: IVec2::new(9, -7), rotation: Rotation::South },
        ModulePlacement { module_type: ModuleType::Gatling, grid_pos: IVec2::new(10, 4), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::Gatling, grid_pos: IVec2::new(10, -5), rotation: Rotation::South },
        ModulePlacement { module_type: ModuleType::Gatling, grid_pos: IVec2::new(7, 4), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::Gatling, grid_pos: IVec2::new(7, -5), rotation: Rotation::South },
        ModulePlacement { module_type: ModuleType::IonDisruptor, grid_pos: IVec2::new(12, 1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::IonDisruptor, grid_pos: IVec2::new(12, -2), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::PointDefenseDrone, grid_pos: IVec2::new(6, 3), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::PointDefenseDrone, grid_pos: IVec2::new(6, -4), rotation: Rotation::South },
        // Bridge
        ModulePlacement { module_type: ModuleType::HelmStation, grid_pos: IVec2::new(12, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::RadarArray, grid_pos: IVec2::new(14, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::TargetingComputer, grid_pos: IVec2::new(13, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::TargetingComputer, grid_pos: IVec2::new(15, 0), rotation: Rotation::East },
    ];
    AiShipLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}

// ============================================================================
// VOID TITAN - The largest, hardest kill in the game. Abyssal Cult's organic
// hull language taken to a monstrous scale, armed like a Dreadnought and
// self-healing like the Cult it's descended from. Spawns beyond everything
// else in explored space.
// ============================================================================
fn void_titan_layout() -> AiShipLayout {
    let material = HullMaterial::AbyssalAlloy;
    let rows: &[(i32, i32, i32)] = &[
        ( 8,  10, 14),
        ( 7,   6, 18),
        ( 6,   3, 20),
        ( 5,   0, 21),
        ( 4,  -3, 22),
        ( 3,  -6, 22),
        ( 2,  -8, 22),
        ( 1, -10, 22),
        ( 0, -11, 22),
        (-1, -11, 22),
        (-2, -10, 22),
        (-3,  -8, 22),
        (-4,  -6, 22),
        (-5,  -3, 22),
        (-6,   0, 21),
        (-7,   3, 20),
        (-8,   6, 18),
        (-9,  10, 14),
    ];
    let hull_cells = build_shaped_hull(rows, material);
    let modules = vec![
        // Massive engine cluster
        ModulePlacement { module_type: ModuleType::LargeEngine, grid_pos: IVec2::new(-9, 2), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::LargeEngine, grid_pos: IVec2::new(-9, 0), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::LargeEngine, grid_pos: IVec2::new(-9, -2), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-6, 2), rotation: Rotation::West },
        ModulePlacement { module_type: ModuleType::StandardEngine, grid_pos: IVec2::new(-6, -2), rotation: Rotation::West },
        // Reactor core — the "heart" (Cult lineage)
        ModulePlacement { module_type: ModuleType::LargeReactor, grid_pos: IVec2::new(2, 1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::LargeReactor, grid_pos: IVec2::new(2, -1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(4, 0), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::StandardReactor, grid_pos: IVec2::new(0, 2), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::FuelTank, grid_pos: IVec2::new(-2, 1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::FuelTank, grid_pos: IVec2::new(-2, -1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::ManeuverThruster, grid_pos: IVec2::new(-1, 0), rotation: Rotation::North },
        // Self-healing organic tissue — extensive, like the Cult
        ModulePlacement { module_type: ModuleType::AdvancedRepairBay, grid_pos: IVec2::new(6, 1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::AdvancedRepairBay, grid_pos: IVec2::new(6, -1), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::RepairBay, grid_pos: IVec2::new(5, 2), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::RepairBay, grid_pos: IVec2::new(5, -2), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::BasicQuarters, grid_pos: IVec2::new(7, 0), rotation: Rotation::North },
        // Overwhelming firepower — bio-weapons and conventional side by side
        ModulePlacement { module_type: ModuleType::PlasmaCaster, grid_pos: IVec2::new(20, 1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::PlasmaCaster, grid_pos: IVec2::new(20, -1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::PlasmaCaster, grid_pos: IVec2::new(21, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Railgun, grid_pos: IVec2::new(19, 4), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Railgun, grid_pos: IVec2::new(19, -5), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Railgun, grid_pos: IVec2::new(18, 6), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::Railgun, grid_pos: IVec2::new(18, -7), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyMissile, grid_pos: IVec2::new(17, 3), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyMissile, grid_pos: IVec2::new(17, -4), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyMissile, grid_pos: IVec2::new(16, 5), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::HeavyMissile, grid_pos: IVec2::new(16, -6), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::IonDisruptor, grid_pos: IVec2::new(15, 2), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::IonDisruptor, grid_pos: IVec2::new(15, -3), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::IonDisruptor, grid_pos: IVec2::new(14, 7), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::IonDisruptor, grid_pos: IVec2::new(14, -8), rotation: Rotation::South },
        ModulePlacement { module_type: ModuleType::Cannon, grid_pos: IVec2::new(12, 6), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::Cannon, grid_pos: IVec2::new(12, -7), rotation: Rotation::South },
        ModulePlacement { module_type: ModuleType::Gatling, grid_pos: IVec2::new(10, 7), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::Gatling, grid_pos: IVec2::new(10, -8), rotation: Rotation::South },
        ModulePlacement { module_type: ModuleType::PointDefenseDrone, grid_pos: IVec2::new(8, 6), rotation: Rotation::North },
        ModulePlacement { module_type: ModuleType::PointDefenseDrone, grid_pos: IVec2::new(8, -7), rotation: Rotation::South },
        // Sensors — it sees everything coming
        ModulePlacement { module_type: ModuleType::AdvancedRadar, grid_pos: IVec2::new(13, 0), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::TargetingComputer, grid_pos: IVec2::new(13, 1), rotation: Rotation::East },
        ModulePlacement { module_type: ModuleType::TargetingComputer, grid_pos: IVec2::new(13, -1), rotation: Rotation::East },
    ];
    AiShipLayout { hull_cells, modules, body_size: hull_size(rows), hull_material: material }
}
