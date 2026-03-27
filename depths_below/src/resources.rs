// Resource fields are part of the data model — not all are consumed by systems yet.
#![allow(dead_code)]

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::components::{
    ModuleType, ModuleCategory, Rotation, HullLayer, HullMaterial, SubComponentType,
    CalculatedStats, WeaponStats, EngineStats, ReactorStats, LifeSupportStats,
    ComponentPieceType, ComponentPiece,
};
use crate::states::GameState;

/// Remembers which state we paused from so ESC resumes correctly
#[derive(Resource, Default)]
pub struct PrePauseState(pub Option<GameState>);

/// Tracks how long the current Exploring session has been running.
/// Resets on each OnEnter(Exploring) to provide proper grace periods.
#[derive(Resource, Default)]
pub struct ExploringSessionTimer {
    pub elapsed: f32,
}

// ============================================================================
// SUBMARINE RESOURCES
// ============================================================================

#[derive(Resource, Default)]
pub struct DepthState {
    pub current_depth: f32,
    pub target_depth: f32,
}

#[derive(Resource, Default)]
pub struct PowerState {
    pub total_power_generation: f32,
    pub total_power_consumption: f32,
    pub power_balance: f32,
}

#[derive(Resource, Default)]
pub struct PowerGraph {
    pub powered_tiles: HashSet<IVec2>,
}

/// Double-buffered heat map for heat diffusion calculations
#[derive(Resource, Default)]
pub struct HeatNetworkState {
    pub temperatures: HashMap<IVec2, f32>,
    pub prev_temperatures: HashMap<IVec2, f32>,
}

#[derive(Resource, Default)]
pub struct ResearchState {
    pub research_points: f32,
    pub research_rate: f32,
}

#[derive(Resource, Default)]
pub struct AutopilotState {
    pub enabled: bool,
    pub target_depth: f32,
}

/// Accumulated targeting computer accuracy bonus for the current frame.
#[derive(Resource, Default)]
pub struct TargetingBonus {
    pub accuracy_bonus: f32,
}

#[derive(Resource, Default)]
pub struct OxygenState {
    pub total_oxygen_generation: f32,
    pub total_oxygen_consumption: f32,
    pub oxygen_balance: f32,
    pub current_oxygen: f32,
    pub max_oxygen: f32,
}

#[derive(Resource, Default)]
pub struct HullState {
    pub hull_integrity: f32,
    pub max_depth_rating: f32,
    pub total_weight: f32,
}

#[derive(Resource, Default)]
pub struct NoiseState {
    pub noise_level: f32,
}

/// Fuel state for combustion engines (Phase 3.3)
#[derive(Resource)]
pub struct FuelState {
    pub current_fuel: f32,
    pub max_fuel: f32,
    pub fuel_consumption_rate: f32,
}

impl Default for FuelState {
    fn default() -> Self {
        Self {
            current_fuel: 500.0,
            max_fuel: 500.0,
            fuel_consumption_rate: 1.0,
        }
    }
}

/// Submarine blueprint for saving/loading
#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct SubmarineBlueprint {
    pub hull_segments: Vec<HullSegmentData>,
    pub modules: Vec<ModuleData>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct HullSegmentData {
    pub grid_position: IVec2,
    pub health: f32,
    pub depth_rating: f32,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ModuleData {
    pub module_type: ModuleType,
    pub grid_position: IVec2,
    pub health: f32,
    #[serde(default = "default_rotation")]
    pub rotation: Rotation,
    #[serde(default = "default_true")]
    pub is_active: bool,
    pub custom_data: Option<CustomModuleData>,
}

fn default_rotation() -> Rotation { Rotation::North }
fn default_true() -> bool { true }

/// Serialized data for custom modules
#[derive(Serialize, Deserialize, Clone)]
pub struct CustomModuleData {
    pub custom_name: String,
    pub subcomponents: Vec<SubComponentType>,
}

impl Default for SubmarineBlueprint {
    fn default() -> Self {
        Self {
            hull_segments: Vec::new(),
            modules: Vec::new(),
        }
    }
}

// ============================================================================
// WORLD RESOURCES
// ============================================================================

#[derive(Resource)]
pub struct WorldState {
    pub seed: u64,
    pub time_of_day: f32,
    pub current_biome: BiomeType,
}

impl Default for WorldState {
    fn default() -> Self {
        Self {
            seed: 0,
            time_of_day: 0.0,
            current_biome: BiomeType::OpenOcean,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BiomeType {
    OpenOcean,
    KelpForest,
    CoralReef,
    DeepTrench,
    ThermalVents,
    IceCaverns,
    AbyssalPlain,
    SunkenCity,
}

/// Tracks which chunks are loaded
#[derive(Resource)]
pub struct ChunkManager {
    pub loaded_chunks: HashMap<IVec2, Entity>,
    pub chunk_size: f32,
    pub render_distance: i32,
}

impl Default for ChunkManager {
    fn default() -> Self {
        Self {
            loaded_chunks: HashMap::new(),
            chunk_size: 512.0,
            render_distance: 3,
        }
    }
}

/// Tracks discovered locations for the map
#[derive(Resource, Default, Serialize, Deserialize, Clone)]
pub struct DiscoveredLocations {
    pub wrecks: Vec<Vec2>,
    pub caves: Vec<Vec2>,
    pub settlements: Vec<Vec2>,
    pub special: Vec<(Vec2, String)>,
}

// ============================================================================
// ECOSYSTEM RESOURCES
// ============================================================================

use crate::components::CreatureType;

/// Record of a creature kill for cascade detection
pub struct EcoKillRecord {
    pub killer_type: Option<CreatureType>,
    pub victim_type: CreatureType,
    pub position: Vec2,
    pub time: f32,
    pub by_player: bool,
}

/// Tracks the living ecosystem state
#[derive(Resource, Default)]
pub struct EcosystemState {
    pub population_counts: HashMap<CreatureType, u32>,
    pub recent_kills: Vec<EcoKillRecord>,
    pub ambient_population: HashMap<crate::components::AmbientKind, u32>,
    pub total_elapsed: f32,
}

/// All ecosystem tuning constants
#[derive(Resource)]
pub struct EcosystemConfig {
    pub max_total_creatures: u32,
    pub per_type_caps: HashMap<CreatureType, u32>,
    pub hunt_hunger_threshold: f32,
    pub starve_hunger_threshold: f32,
    pub starvation_damage: f32,
    pub reproduction_hunger_max: f32,
    pub reproduction_cooldown: f32,
    pub territory_default_radius: f32,
    pub noise_emit_interval: f32,
    pub noise_decay_rate: f32,
    pub max_trail_points: usize,
    pub cascade_kill_count: u32,
    pub cascade_time_window: f32,
    pub max_corpses: usize,
    pub corpse_decay_time: f32,
}

impl Default for EcosystemConfig {
    fn default() -> Self {
        let mut per_type_caps = HashMap::new();
        per_type_caps.insert(CreatureType::Leviathan, 1);
        per_type_caps.insert(CreatureType::Stalker, 4);
        per_type_caps.insert(CreatureType::BlindHunter, 3);
        per_type_caps.insert(CreatureType::Ambusher, 3);
        per_type_caps.insert(CreatureType::ElectricEel, 3);
        per_type_caps.insert(CreatureType::LureFish, 3);
        per_type_caps.insert(CreatureType::SwarmQueen, 2);
        per_type_caps.insert(CreatureType::Scavenger, 5);
        per_type_caps.insert(CreatureType::Parasite, 8);
        per_type_caps.insert(CreatureType::Watcher, 2);

        Self {
            max_total_creatures: 30,
            per_type_caps,
            hunt_hunger_threshold: 40.0,
            starve_hunger_threshold: 90.0,
            starvation_damage: 2.0,
            reproduction_hunger_max: 20.0,
            reproduction_cooldown: 60.0,
            territory_default_radius: 300.0,
            noise_emit_interval: 0.5,
            noise_decay_rate: 10.0,
            max_trail_points: 100,
            cascade_kill_count: 3,
            cascade_time_window: 30.0,
            max_corpses: 8,
            corpse_decay_time: 120.0,
        }
    }
}

// ============================================================================
// CREW RESOURCES
// ============================================================================

#[derive(Resource, Default)]
pub struct CrewRoster {
    pub members: Vec<Entity>,
}

#[derive(Resource, Default)]
pub struct StaffingState {
    pub total_berths: u32,
    pub total_crew: u32,
    pub staffed_stations: u32,
    pub total_stations: u32,
}

// ============================================================================
// INVENTORY / ECONOMY
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum ItemType {
    ScrapMetal,
    Crystal,
    BioSample,
    FuelCell,
    RareAlloy,
    AncientArtifact,
    AmmoCrate,
}

impl ItemType {
    pub fn name(&self) -> &'static str {
        match self {
            ItemType::ScrapMetal => "Scrap Metal",
            ItemType::Crystal => "Crystal",
            ItemType::BioSample => "Bio Sample",
            ItemType::FuelCell => "Fuel Cell",
            ItemType::RareAlloy => "Rare Alloy",
            ItemType::AncientArtifact => "Ancient Artifact",
            ItemType::AmmoCrate => "Ammo Crate",
        }
    }

    pub fn weight(&self) -> f32 {
        match self {
            ItemType::ScrapMetal => 5.0,
            ItemType::Crystal => 2.0,
            ItemType::BioSample => 1.0,
            ItemType::FuelCell => 3.0,
            ItemType::RareAlloy => 8.0,
            ItemType::AncientArtifact => 10.0,
            ItemType::AmmoCrate => 4.0,
        }
    }
}

#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct Inventory {
    pub items: HashMap<ItemType, u32>,
    pub max_capacity: f32,
    pub current_weight: f32,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            items: HashMap::new(),
            max_capacity: 50.0,
            current_weight: 0.0,
        }
    }
}

impl Inventory {
    pub fn add_item(&mut self, item: ItemType, count: u32) -> bool {
        let added_weight = item.weight() * count as f32;
        if self.max_capacity > 0.0 && self.current_weight + added_weight > self.max_capacity {
            return false;
        }
        *self.items.entry(item).or_insert(0) += count;
        self.current_weight += added_weight;
        true
    }

    pub fn remove_item(&mut self, item: ItemType, count: u32) -> bool {
        if let Some(current) = self.items.get_mut(&item) {
            if *current >= count {
                *current -= count;
                self.current_weight -= item.weight() * count as f32;
                if *current == 0 {
                    self.items.remove(&item);
                }
                return true;
            }
        }
        false
    }

}

#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct Currency {
    pub credits: u32,
}

impl Default for Currency {
    fn default() -> Self {
        Self { credits: 500 }
    }
}

// ============================================================================
// PROGRESSION / UNLOCKS
// ============================================================================

#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct Unlocks {
    pub modules: Vec<String>,
    pub hull_types: Vec<String>,
    pub upgrades: Vec<String>,
    pub blueprints_found: Vec<String>,
}

impl Default for Unlocks {
    fn default() -> Self {
        Self {
            modules: vec![
                "reactor".into(), "engine".into(), "quarters".into(),
                "oxygen".into(), "ballast".into(), "light".into(),
            ],
            hull_types: vec!["standard".into()],
            upgrades: Vec::new(),
            blueprints_found: Vec::new(),
        }
    }
}

#[derive(Resource, Default, Serialize, Deserialize, Clone)]
pub struct Statistics {
    pub max_depth_reached: f32,
    pub creatures_encountered: HashMap<String, u32>,
    pub creatures_killed: u32,
    pub wrecks_salvaged: u32,
    pub crew_lost: u32,
    pub submarines_lost: u32,
    pub play_time_seconds: f32,
    pub logs_found: Vec<String>,
}

// ============================================================================
// VICTORY
// ============================================================================

#[derive(Resource, Default)]
pub struct VictoryState {
    pub achieved: bool,
}

// ============================================================================
// GAME CONFIG
// ============================================================================

#[derive(Resource)]
pub struct GameConfig {
    // Pressure
    pub pressure_damage_multiplier: f32,
    pub pressure_per_meter: f32,

    // Oxygen
    pub base_oxygen_consumption_per_crew: f32,
    pub suffocation_damage_rate: f32,

    // Movement
    pub base_submarine_speed: f32,
    pub depth_change_speed: f32,

    // Creatures
    pub creature_spawn_rate: f32,
    pub creature_detection_noise_threshold: f32,
    pub creature_detection_light_threshold: f32,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            pressure_damage_multiplier: 1.0,
            pressure_per_meter: 0.1,
            base_oxygen_consumption_per_crew: 5.0,
            suffocation_damage_rate: 5.0,
            base_submarine_speed: 100.0,
            depth_change_speed: 20.0,
            creature_spawn_rate: 0.1,
            creature_detection_noise_threshold: 50.0,
            creature_detection_light_threshold: 30.0,
        }
    }
}

// ============================================================================
// INPUT STATE
// ============================================================================

#[derive(Resource, Default)]
pub struct InputState {
    pub movement: Vec2,
    pub depth_input: f32,       // -1 rise, +1 sink
    pub mouse_world_pos: Vec2,
    pub mouse_grid_pos: IVec2,
    pub ballast_input: f32,     // Q/E for ballast control
}

// ============================================================================
// BUILDING MODE (category-based selection)
// ============================================================================

/// Build categories include hull as a special category + all module categories
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildCategory {
    Hull,
    Power,
    Propulsion,
    LifeSupport,
    Control,
    Weapons,
    Detection,
    Storage,
    Crew,
    Utility,
    Custom,
}

impl BuildCategory {
    pub const ALL: &'static [BuildCategory] = &[
        BuildCategory::Hull,
        BuildCategory::Power,
        BuildCategory::Propulsion,
        BuildCategory::LifeSupport,
        BuildCategory::Control,
        BuildCategory::Weapons,
        BuildCategory::Detection,
        BuildCategory::Storage,
        BuildCategory::Crew,
        BuildCategory::Utility,
        BuildCategory::Custom,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            BuildCategory::Hull => "Hull",
            BuildCategory::Power => "Power",
            BuildCategory::Propulsion => "Propulsion",
            BuildCategory::LifeSupport => "Life Support",
            BuildCategory::Control => "Control",
            BuildCategory::Weapons => "Weapons",
            BuildCategory::Detection => "Detection",
            BuildCategory::Storage => "Storage",
            BuildCategory::Crew => "Crew",
            BuildCategory::Utility => "Utility",
            BuildCategory::Custom => "Custom",
        }
    }

    /// Convert to ModuleCategory. Returns None for Hull and Custom.
    pub fn to_module_category(&self) -> Option<ModuleCategory> {
        match self {
            BuildCategory::Hull | BuildCategory::Custom => None,
            BuildCategory::Power => Some(ModuleCategory::Power),
            BuildCategory::Propulsion => Some(ModuleCategory::Propulsion),
            BuildCategory::LifeSupport => Some(ModuleCategory::LifeSupport),
            BuildCategory::Control => Some(ModuleCategory::Control),
            BuildCategory::Weapons => Some(ModuleCategory::Weapons),
            BuildCategory::Detection => Some(ModuleCategory::Detection),
            BuildCategory::Storage => Some(ModuleCategory::Storage),
            BuildCategory::Crew => Some(ModuleCategory::Crew),
            BuildCategory::Utility => Some(ModuleCategory::Utility),
        }
    }

    pub fn item_count(&self) -> usize {
        match self {
            BuildCategory::Hull => 4, // Outer, Inner, Void, BulkheadDoor
            BuildCategory::Custom => 0, // No saved blueprints yet (will be expanded later)
            other => other.to_module_category()
                .map(|c| c.module_types().len())
                .unwrap_or(0),
        }
    }
}

/// What's currently selected for placement
#[derive(Clone, Copy, Debug)]
pub enum BuildSelection {
    Hull(HullLayer),
    Module(ModuleType),
}

/// Building mode state with category-based selection
#[derive(Resource)]
pub struct BuildingState {
    pub category_index: usize,
    pub selected_index: usize,
    pub rotation: Rotation,
    pub hull_material: HullMaterial,
    pub is_valid_placement: bool,
    pub placement_reason: Option<String>,
    pub ghost_position: IVec2,
    /// When true, rotation was set by auto-rotate (will be overridden on ghost move).
    /// When false, user manually set rotation with R key.
    pub auto_rotated: bool,
}

const HULL_LAYERS: [HullLayer; 4] = [
    HullLayer::Outer,
    HullLayer::Inner,
    HullLayer::Void,
    HullLayer::BulkheadDoor,
];

impl Default for BuildingState {
    fn default() -> Self {
        Self {
            category_index: 0,
            selected_index: 0,
            rotation: Rotation::North,
            hull_material: HullMaterial::Steel,
            is_valid_placement: false,
            placement_reason: None,
            ghost_position: IVec2::ZERO,
            auto_rotated: true,
        }
    }
}

impl BuildingState {
    pub fn current_category(&self) -> BuildCategory {
        BuildCategory::ALL[self.category_index % BuildCategory::ALL.len()]
    }

    pub fn current_selection(&self) -> BuildSelection {
        let cat = self.current_category();
        match cat {
            BuildCategory::Hull => {
                let idx = self.selected_index % HULL_LAYERS.len();
                BuildSelection::Hull(HULL_LAYERS[idx])
            }
            BuildCategory::Custom => {
                // For now, return first customizable module (TorpedoTube)
                // Later this will be saved custom blueprints
                BuildSelection::Module(ModuleType::TorpedoTube)
            }
            _ => {
                if let Some(module_cat) = cat.to_module_category() {
                    let types = module_cat.module_types();
                    if types.is_empty() {
                        BuildSelection::Module(ModuleType::StandardReactor)
                    } else {
                        let idx = self.selected_index % types.len();
                        BuildSelection::Module(types[idx])
                    }
                } else {
                    BuildSelection::Module(ModuleType::StandardReactor)
                }
            }
        }
    }

    pub fn selection_name(&self) -> &'static str {
        match self.current_selection() {
            BuildSelection::Hull(layer) => match layer {
                HullLayer::Outer => "Outer Hull",
                HullLayer::Inner => "Inner Hull",
                HullLayer::Void => "Void Space",
                HullLayer::BulkheadDoor => "Bulkhead Door",
            },
            BuildSelection::Module(mt) => mt.name(),
        }
    }

    pub fn next_category(&mut self) {
        self.category_index = (self.category_index + 1) % BuildCategory::ALL.len();
        self.selected_index = 0;
    }

    pub fn next_item(&mut self) {
        let count = self.current_category().item_count();
        if count > 0 {
            self.selected_index = (self.selected_index + 1) % count;
        }
    }

    pub fn prev_item(&mut self) {
        let count = self.current_category().item_count();
        if count > 0 {
            if self.selected_index == 0 {
                self.selected_index = count - 1;
            } else {
                self.selected_index -= 1;
            }
        }
    }
}

// ============================================================================
// CUSTOMIZATION MODE
// ============================================================================

/// State for customizing modules with sliders and sub-component placement
#[derive(Resource)]
pub struct CustomizationState {
    pub active: bool,
    pub module_type: ModuleType,
    pub properties: HashMap<String, f32>,
    pub preview_stats: CalculatedStats,
}

impl Default for CustomizationState {
    fn default() -> Self {
        Self {
            active: false,
            module_type: ModuleType::TorpedoTube,
            properties: HashMap::new(),
            preview_stats: CalculatedStats::default(),
        }
    }
}

impl CustomizationState {
    /// Start customizing a module type
    pub fn start_customizing(&mut self, module_type: ModuleType) {
        self.active = true;
        self.module_type = module_type;
        self.properties.clear();

        // Set default property values based on module type
        match module_type.category() {
            ModuleCategory::Weapons => {
                self.properties.insert("barrel_length".to_string(), 5.0);
                self.properties.insert("caliber".to_string(), 50.0);
                self.properties.insert("chamber_pressure".to_string(), 100.0);
            }
            ModuleCategory::Propulsion => {
                self.properties.insert("efficiency".to_string(), 1.0);
                self.properties.insert("propeller_count".to_string(), 4.0);
                self.properties.insert("propeller_pitch".to_string(), 1.0);
            }
            ModuleCategory::Power => {
                self.properties.insert("enrichment".to_string(), 1.0);
                self.properties.insert("fuel_rod_count".to_string(), 4.0);
                self.properties.insert("coolant_flow".to_string(), 100.0);
            }
            ModuleCategory::LifeSupport => {
                self.properties.insert("filter_size".to_string(), 1.0);
                self.properties.insert("absorber_efficiency".to_string(), 1.0);
            }
            _ => {}
        }

        self.recalculate_preview();
    }

    /// Update a property value
    pub fn update_property(&mut self, key: &str, value: f32) {
        self.properties.insert(key.to_string(), value);
        self.recalculate_preview();
    }

    /// Recalculate preview stats from current properties
    pub fn recalculate_preview(&mut self) {
        use crate::building::StatCalculator;

        // Build sub-components from properties
        let subcomponents = self.build_subcomponents();

        // Get base stats (would need registry, but for now use defaults)
        let base_stats = match self.module_type.category() {
            ModuleCategory::Weapons => CalculatedStats {
                weapon: Some(WeaponStats {
                    damage: 30.0,
                    range: 400.0,
                    fire_rate: 0.5,
                    max_ammo: 20,
                    power_cost: 25.0,
                }),
                ..Default::default()
            },
            ModuleCategory::Propulsion => CalculatedStats {
                engine: Some(EngineStats {
                    thrust: 50.0,
                    fuel_efficiency: 1.0,
                    noise: 10.0,
                }),
                ..Default::default()
            },
            ModuleCategory::Power => CalculatedStats {
                reactor: Some(ReactorStats {
                    power_output: 60.0,
                    heat_generation: 80.0,
                    explosion_risk: 0.1,
                }),
                ..Default::default()
            },
            ModuleCategory::LifeSupport => CalculatedStats {
                life_support: Some(LifeSupportStats {
                    o2_generation: 10.0,
                    co2_filtering: 8.0,
                    crew_capacity: 5,
                }),
                ..Default::default()
            },
            _ => CalculatedStats::default(),
        };

        self.preview_stats = StatCalculator::calculate_stats(
            self.module_type,
            &subcomponents,
            &base_stats,
        );
    }

    /// Build sub-components from current property values
    pub fn build_subcomponents(&self) -> Vec<SubComponentType> {
        use crate::components::*;

        let mut subcomponents = Vec::new();

        match self.module_type.category() {
            ModuleCategory::Weapons => {
                if let (Some(&length), Some(&caliber)) = (
                    self.properties.get("barrel_length"),
                    self.properties.get("caliber"),
                ) {
                    subcomponents.push(SubComponentType::BarrelComponent {
                        length,
                        caliber,
                        thickness: 5.0,
                    });
                }

                if let Some(&pressure) = self.properties.get("chamber_pressure") {
                    subcomponents.push(SubComponentType::ChamberComponent {
                        volume: 50.0,
                        pressure,
                    });
                }

                subcomponents.push(SubComponentType::LoaderComponent {
                    mechanism: LoaderMechanism::Automatic,
                    speed: 1.0,
                });
            }
            ModuleCategory::Propulsion => {
                if let Some(&efficiency) = self.properties.get("efficiency") {
                    subcomponents.push(SubComponentType::CombustionChamber { efficiency });
                }

                if let (Some(&pitch), Some(&count)) = (
                    self.properties.get("propeller_pitch"),
                    self.properties.get("propeller_count"),
                ) {
                    subcomponents.push(SubComponentType::PropellerBlade {
                        pitch,
                        count: count as u32,
                    });
                }
            }
            ModuleCategory::Power => {
                if let (Some(&enrichment), Some(&count)) = (
                    self.properties.get("enrichment"),
                    self.properties.get("fuel_rod_count"),
                ) {
                    subcomponents.push(SubComponentType::FuelRod {
                        enrichment,
                        count: count as u32,
                    });
                }

                if let Some(&flow) = self.properties.get("coolant_flow") {
                    subcomponents.push(SubComponentType::Coolant { flow_rate: flow });
                }
            }
            ModuleCategory::LifeSupport => {
                if let Some(&size) = self.properties.get("filter_size") {
                    subcomponents.push(SubComponentType::OxygenScrubber { filter_size: size });
                }

                if let Some(&efficiency) = self.properties.get("absorber_efficiency") {
                    subcomponents.push(SubComponentType::CO2Absorber { efficiency });
                }
            }
            _ => {}
        }

        subcomponents
    }

    /// Cancel customization
    pub fn cancel(&mut self) {
        self.active = false;
        self.properties.clear();
    }
}

// ============================================================================
// COMPONENT PLACEMENT STATE
// ============================================================================

/// Resource for component piece placement mode
#[derive(Resource)]
pub struct ComponentPlacementState {
    pub active: bool,
    pub module_type: ModuleType,
    pub selected_piece_type: Option<ComponentPieceType>,
    pub placed_pieces: Vec<ComponentPiece>,
    pub ghost_position: Option<IVec2>,
}

impl Default for ComponentPlacementState {
    fn default() -> Self {
        Self {
            active: false,
            module_type: ModuleType::TorpedoTube,
            selected_piece_type: None,
            placed_pieces: Vec::new(),
            ghost_position: None,
        }
    }
}

impl ComponentPlacementState {
    /// Start component placement mode
    pub fn start_placing(&mut self, module_type: ModuleType) {
        self.active = true;
        self.module_type = module_type;
        self.selected_piece_type = None;
        self.placed_pieces.clear();
        self.ghost_position = None;
    }

    /// Select a piece type to place
    pub fn select_piece(&mut self, piece_type: ComponentPieceType) {
        self.selected_piece_type = Some(piece_type);
    }

    /// Place a component piece at a position
    pub fn place_piece(&mut self, position: IVec2, piece_type: ComponentPieceType) -> bool {
        // Check if position is already occupied
        if self.placed_pieces.iter().any(|p| {
            let end_pos = p.internal_position + p.size;
            position.x >= p.internal_position.x && position.x < end_pos.x
                && position.y >= p.internal_position.y && position.y < end_pos.y
        }) {
            return false;
        }

        // Check if within grid bounds (4x4)
        if position.x < 0 || position.x >= 4 || position.y < 0 || position.y >= 4 {
            return false;
        }

        // Get piece size
        let size = match piece_type {
            ComponentPieceType::Barrel | ComponentPieceType::FuelTank => IVec2::new(2, 1),
            _ => IVec2::new(1, 1),
        };

        // Check if piece fits
        if position.x + size.x > 4 || position.y + size.y > 4 {
            return false;
        }

        // Validate placement rules
        if !self.validate_placement(&piece_type, position) {
            return false;
        }

        // Place the piece
        let piece = ComponentPiece {
            piece_type: piece_type.clone(),
            internal_position: position,
            size,
            properties: std::collections::HashMap::new(),
        };

        self.placed_pieces.push(piece);
        true
    }

    /// Validate placement rules (barrels at front, chambers behind, etc.)
    fn validate_placement(&self, piece_type: &ComponentPieceType, position: IVec2) -> bool {
        match piece_type {
            ComponentPieceType::Barrel => {
                // Barrels must be at front (leftmost columns 0-1)
                position.x <= 1
            }
            ComponentPieceType::Chamber | ComponentPieceType::CombustionChamber => {
                // Chambers must be in middle (columns 1-2)
                position.x >= 1 && position.x <= 2
            }
            ComponentPieceType::Loader => {
                // Loaders must be behind chambers (columns 2-3)
                position.x >= 2
            }
            ComponentPieceType::FuelRod => {
                // Fuel rods must be in center (columns 1-2, rows 1-2)
                position.x >= 1 && position.x <= 2 && position.y >= 1 && position.y <= 2
            }
            ComponentPieceType::CoolantPipe => {
                // Coolant must be adjacent to fuel rods
                self.is_adjacent_to_piece(position, ComponentPieceType::FuelRod)
            }
            _ => true, // No special rules for other pieces
        }
    }

    /// Check if a position is adjacent to a specific piece type
    fn is_adjacent_to_piece(&self, position: IVec2, piece_type: ComponentPieceType) -> bool {
        let adjacent_offsets = [
            IVec2::new(-1, 0),
            IVec2::new(1, 0),
            IVec2::new(0, -1),
            IVec2::new(0, 1),
        ];

        for offset in adjacent_offsets.iter() {
            let adj_pos = position + *offset;
            if self.placed_pieces.iter().any(|p| {
                p.piece_type == piece_type && p.internal_position == adj_pos
            }) {
                return true;
            }
        }

        false
    }

    /// Remove a piece at a position
    pub fn remove_piece(&mut self, position: IVec2) -> bool {
        if let Some(index) = self.placed_pieces.iter().position(|p| {
            let end_pos = p.internal_position + p.size;
            position.x >= p.internal_position.x && position.x < end_pos.x
                && position.y >= p.internal_position.y && position.y < end_pos.y
        }) {
            self.placed_pieces.remove(index);
            true
        } else {
            false
        }
    }

    /// Finalize and return placed pieces
    pub fn finalize(&mut self) -> Vec<ComponentPiece> {
        self.active = false;
        std::mem::take(&mut self.placed_pieces)
    }

    /// Cancel placement
    pub fn cancel(&mut self) {
        self.active = false;
        self.placed_pieces.clear();
        self.selected_piece_type = None;
    }

    /// Get all pieces connected to a piece at a position (flood-fill)
    pub fn get_connected_pieces(&self, start_position: IVec2) -> Vec<usize> {
        let mut connected = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();

        // Find the piece at start_position
        let start_piece_idx = match self.placed_pieces.iter().position(|p| {
            let end_pos = p.internal_position + p.size;
            start_position.x >= p.internal_position.x && start_position.x < end_pos.x
                && start_position.y >= p.internal_position.y && start_position.y < end_pos.y
        }) {
            Some(idx) => idx,
            None => return connected,
        };

        let start_piece = &self.placed_pieces[start_piece_idx];
        let target_type = start_piece.piece_type.clone();

        queue.push_back(start_piece_idx);
        visited.insert(start_piece_idx);

        while let Some(current_idx) = queue.pop_front() {
            connected.push(current_idx);
            let current_piece = &self.placed_pieces[current_idx];

            // Check all adjacent positions
            let adjacent_offsets = [
                IVec2::new(-1, 0),
                IVec2::new(1, 0),
                IVec2::new(0, -1),
                IVec2::new(0, 1),
            ];

            for offset in adjacent_offsets.iter() {
                let check_pos = current_piece.internal_position + *offset;

                // Find piece at this position
                if let Some(neighbor_idx) = self.placed_pieces.iter().enumerate().position(|(idx, p)| {
                    p.piece_type == target_type
                        && p.internal_position == check_pos
                        && !visited.contains(&idx)
                }) {
                    if !visited.contains(&neighbor_idx) {
                        visited.insert(neighbor_idx);
                        queue.push_back(neighbor_idx);
                    }
                }
            }
        }

        connected
    }
}

/// Resource for piece customization context menu
#[derive(Resource)]
pub struct PieceCustomizationState {
    pub active: bool,
    pub target_position: IVec2,
    pub connected_pieces: Vec<usize>,
    pub customize_group: bool,
    pub properties: std::collections::HashMap<String, f32>,
}

impl Default for PieceCustomizationState {
    fn default() -> Self {
        Self {
            active: false,
            target_position: IVec2::ZERO,
            connected_pieces: Vec::new(),
            customize_group: false,
            properties: std::collections::HashMap::new(),
        }
    }
}

impl PieceCustomizationState {
    /// Start customizing a piece or group
    pub fn start_customizing(&mut self, position: IVec2, connected_pieces: Vec<usize>, customize_group: bool) {
        self.active = true;
        self.target_position = position;
        self.connected_pieces = connected_pieces;
        self.customize_group = customize_group;
        self.properties.clear();
    }

    /// Apply customization and return the modified properties
    pub fn apply(&mut self) -> std::collections::HashMap<String, f32> {
        self.active = false;
        std::mem::take(&mut self.properties)
    }

    /// Cancel customization
    pub fn cancel(&mut self) {
        self.active = false;
        self.properties.clear();
    }
}

// ============================================================================
// SAVE DATA
// ============================================================================

/// Serialized crew data
#[derive(Serialize, Deserialize, Clone)]
pub struct CrewSaveData {
    pub name: String,
    pub health: f32,
    pub max_health: f32,
    pub oxygen: f32,
    pub morale: f32,
    #[serde(default)]
    pub assigned_module_grid: Option<IVec2>,
}

/// Hull segment with material info for save/load
#[derive(Serialize, Deserialize, Clone)]
pub struct HullSaveData {
    pub grid_position: IVec2,
    pub health: f32,
    pub max_health: f32,
    pub depth_rating: f32,
    pub material: HullMaterial,
    pub hull_layer: HullLayer,
}

/// Save file header for slot display
#[derive(Serialize, Deserialize, Clone)]
pub struct SaveSlotInfo {
    pub slot: u32,
    pub timestamp: String,
    pub depth: f32,
    pub play_time: f32,
    pub hull_integrity: f32,
}

#[derive(Resource, Serialize, Deserialize)]
pub struct SaveData {
    pub version: u32,
    pub slot_info: SaveSlotInfo,
    pub submarine: SubmarineBlueprint,
    pub hull_segments: Vec<HullSaveData>,
    pub crew: Vec<CrewSaveData>,
    pub inventory: Inventory,
    pub currency: Currency,
    pub unlocks: Unlocks,
    pub statistics: Statistics,
    pub discovered_locations: DiscoveredLocations,
    pub position: Vec2,
    pub current_depth: f32,
    pub world_seed: u64,
    pub was_exploring: bool,
}

impl Default for SaveData {
    fn default() -> Self {
        Self {
            version: 1,
            slot_info: SaveSlotInfo {
                slot: 0,
                timestamp: String::new(),
                depth: 0.0,
                play_time: 0.0,
                hull_integrity: 1.0,
            },
            submarine: SubmarineBlueprint::default(),
            hull_segments: Vec::new(),
            crew: Vec::new(),
            inventory: Inventory::default(),
            currency: Currency::default(),
            unlocks: Unlocks::default(),
            statistics: Statistics::default(),
            discovered_locations: DiscoveredLocations::default(),
            position: Vec2::ZERO,
            current_depth: 0.0,
            world_seed: 0,
            was_exploring: false,
        }
    }
}

/// Resource tracking auto-save timer
#[derive(Resource)]
pub struct AutoSaveTimer {
    pub timer: Timer,
    pub enabled: bool,
}

impl Default for AutoSaveTimer {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(120.0, TimerMode::Repeating),
            enabled: true,
        }
    }
}

/// Resource for the save/load menu overlay
#[derive(Resource, Default)]
pub struct SaveLoadMenuState {
    pub is_saving: bool,
    pub selected_slot: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuel_state_defaults() {
        let fuel = FuelState::default();
        assert!((fuel.current_fuel - 500.0).abs() < f32::EPSILON);
        assert!((fuel.max_fuel - 500.0).abs() < f32::EPSILON);
        assert!((fuel.fuel_consumption_rate - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn game_config_defaults() {
        let config = GameConfig::default();
        assert!(config.pressure_damage_multiplier > 0.0);
        assert!(config.pressure_per_meter > 0.0);
        assert!(config.base_oxygen_consumption_per_crew > 0.0);
        assert!(config.suffocation_damage_rate > 0.0);
        assert!(config.base_submarine_speed > 0.0);
    }

    #[test]
    fn inventory_add_and_remove_items() {
        let mut inv = Inventory {
            items: HashMap::new(),
            max_capacity: 100.0,
            current_weight: 0.0,
        };

        // Add items
        assert!(inv.add_item(ItemType::ScrapMetal, 2));
        assert_eq!(inv.items.get(&ItemType::ScrapMetal), Some(&2));
        assert!((inv.current_weight - 10.0).abs() < f32::EPSILON); // 5.0 * 2

        // Remove one
        assert!(inv.remove_item(ItemType::ScrapMetal, 1));
        assert_eq!(inv.items.get(&ItemType::ScrapMetal), Some(&1));

        // Remove more than available should fail
        assert!(!inv.remove_item(ItemType::ScrapMetal, 5));
    }

    #[test]
    fn inventory_respects_capacity() {
        let mut inv = Inventory {
            items: HashMap::new(),
            max_capacity: 10.0,
            current_weight: 0.0,
        };

        // ScrapMetal weighs 5.0 each, so 2 fit but 3 don't
        assert!(inv.add_item(ItemType::ScrapMetal, 2));
        assert!(!inv.add_item(ItemType::ScrapMetal, 1));
    }

    #[test]
    fn building_state_category_cycling() {
        let mut state = BuildingState::default();
        let initial = state.current_category();
        assert_eq!(initial, BuildCategory::Hull);

        state.next_category();
        assert_eq!(state.current_category(), BuildCategory::Power);

        // Cycle through all categories
        for _ in 0..BuildCategory::ALL.len() - 1 {
            state.next_category();
        }
        assert_eq!(state.current_category(), BuildCategory::Hull);
    }

    #[test]
    fn building_state_item_cycling() {
        let mut state = BuildingState::default();
        // Hull has 4 items
        state.next_item();
        assert_eq!(state.selected_index, 1);
        state.prev_item();
        assert_eq!(state.selected_index, 0);
        state.prev_item();
        assert_eq!(state.selected_index, 3); // wraps around
    }
}
