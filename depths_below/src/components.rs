// Component fields are part of the data model — not all are consumed by systems yet.
#![allow(dead_code)]

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// SUBMARINE COMPONENTS
// ============================================================================

/// Marker for the main ship entity
#[derive(Component)]
pub struct Ship;

/// Hull segment component
#[derive(Component, Clone, Serialize, Deserialize)]
pub struct HullSegment {
    pub health: f32,
    pub max_health: f32,
    pub radiation_shielding: f32,  // Max radiation tolerance before taking damage
    pub is_depressurized: bool,
    pub depressurization_level: f32,  // 0.0 to 1.0 (air lost)
    pub hull_layer: HullLayer,
    pub material: HullMaterial,
    pub grid_position: IVec2,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum HullLayer {
    Outer,
    Inner,
    Void,
    BulkheadDoor,
}

/// Hull material tiers - determines radiation shielding and durability
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum HullMaterial {
    #[default]
    Steel,          // Tier 1
    Titanium,       // Tier 2
    Composite,      // Tier 3
    AbyssalAlloy,   // Tier 4
}

impl HullMaterial {
    pub fn radiation_shielding(&self) -> f32 {
        match self {
            HullMaterial::Steel => 300.0,
            HullMaterial::Titanium => 500.0,
            HullMaterial::Composite => 1000.0,
            HullMaterial::AbyssalAlloy => 2500.0,
        }
    }

    pub fn health_multiplier(&self) -> f32 {
        match self {
            HullMaterial::Steel => 1.0,
            HullMaterial::Titanium => 1.5,
            HullMaterial::Composite => 2.0,
            HullMaterial::AbyssalAlloy => 3.0,
        }
    }

    pub fn cost(&self) -> u32 {
        match self {
            HullMaterial::Steel => 8,
            HullMaterial::Titanium => 23,
            HullMaterial::Composite => 60,
            HullMaterial::AbyssalAlloy => 150,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            HullMaterial::Steel => "Steel",
            HullMaterial::Titanium => "Titanium",
            HullMaterial::Composite => "Composite",
            HullMaterial::AbyssalAlloy => "Abyssal Alloy",
        }
    }

    pub fn damage_absorption(&self) -> f32 {
        match self {
            HullMaterial::Steel => 15.0,
            HullMaterial::Titanium => 30.0,
            HullMaterial::Composite => 50.0,
            HullMaterial::AbyssalAlloy => 80.0,
        }
    }
}

impl Default for HullSegment {
    fn default() -> Self {
        let material = HullMaterial::Steel;
        Self {
            health: 100.0 * material.health_multiplier(),
            max_health: 100.0 * material.health_multiplier(),
            radiation_shielding: material.radiation_shielding(),
            is_depressurized: false,
            depressurization_level: 0.0,
            hull_layer: HullLayer::Outer,
            material,
            grid_position: IVec2::ZERO,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ModuleDamageState {
    #[default]
    Operational,  // 100-60% HP
    Damaged,      // 60-30% HP
    Critical,     // 30-1% HP
    Destroyed,    // 0% HP
}

impl ModuleDamageState {
    pub fn from_health_ratio(ratio: f32) -> Self {
        match ratio {
            r if r <= 0.0 => Self::Destroyed,
            r if r <= 0.30 => Self::Critical,
            r if r <= 0.60 => Self::Damaged,
            _ => Self::Operational,
        }
    }
    pub fn efficiency(&self) -> f32 {
        match self {
            Self::Operational => 1.0,
            Self::Damaged => 0.6,
            Self::Critical => 0.25,
            Self::Destroyed => 0.0,
        }
    }
}

/// Returns the effective efficiency for a module, accounting for staffing if available.
pub fn effective_efficiency(module: &Module, eff: Option<&ModuleEfficiency>) -> f32 {
    if let Some(e) = eff {
        return e.value;
    }
    // fallback: damage-only for modules without crew stations
    let ratio = if module.max_health > 0.0 { module.health / module.max_health } else { 1.0 };
    ModuleDamageState::from_health_ratio(ratio).efficiency()
}

#[derive(Component)]
pub struct DestroyedModule {
    pub original_type: ModuleType,
}

/// Temporary marker for overriding module health/active state after load.
/// Applied once then removed.
#[derive(Component)]
pub struct ModuleHealthOverride {
    pub health: f32,
    pub is_active: bool,
}

// ============================================================================
// CHAIN REACTION / FIRE / CASCADE COMPONENTS
// ============================================================================

/// Marks a module as explosive — when destroyed, it detonates after a delay
#[derive(Component)]
pub struct Explosive {
    pub blast_radius: f32,       // grid cells (1.5 = adjacent + some)
    pub blast_damage: f32,       // base damage at center
    pub explosive_type: ExplosiveType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplosiveType {
    Reactor,   // high damage, large radius
    Ammo,      // medium damage, fast fuse
    Fuel,      // medium damage, starts fire
    Battery,   // small damage, starts fire
}

/// Inserted on a destroyed Explosive module — counts down to detonation
#[derive(Component)]
pub struct PendingDetonation {
    pub timer: Timer,
    pub blast_radius: f32,
    pub blast_damage: f32,
    pub explosive_type: ExplosiveType,
    pub grid_position: IVec2,
}

/// Module is on fire — takes DoT, spreads to neighbors, suppressed by decompression
#[derive(Component)]
pub struct OnFire {
    pub intensity: f32,          // 0.0–1.0
    pub damage_per_second: f32,
    pub spread_timer: Timer,     // try spread every N seconds
    pub duration: Timer,         // self-extinguish after this
}

/// Marker for hull segments that have been fully destroyed (0 HP)
#[derive(Component)]
pub struct HullDestroyed;

/// Stable snapshot of a hull segment's material-defined stats, taken once at
/// spawn. apply_hull_enhancers must reset from here every frame before
/// reapplying adjacency bonuses — same reasoning as BaseWeaponStats: without
/// a fixed base to reset to, `hull.max_health *= 1.30` compounds forever for
/// any tile sitting next to a reinforcement module.
#[derive(Component, Clone, Copy, Debug)]
pub struct BaseHullStats {
    pub max_health: f32,
    pub radiation_shielding: f32,
}

/// Queued for removal from the world — the block stays in place (still
/// tinted, still occupying its grid cell) for one short beat after being
/// marked destroyed, then despawns, leaving a real gap. The delay isn't
/// cosmetic: several other systems (severance, chain reactions, reactor
/// meltdown, detonation queueing) react to the same destruction markers
/// across different plugins with no guaranteed relative frame ordering —
/// this guarantees all of them get to see the entity before it's gone,
/// instead of racing an immediate despawn.
#[derive(Component)]
pub struct PendingRemoval {
    pub timer: Timer,
}

// ============================================================================
// MODULE SYSTEM
// ============================================================================

/// Module base component - all modules have this
#[derive(Component, Clone, Serialize, Deserialize)]
pub struct Module {
    pub module_type: ModuleType,
    pub health: f32,
    pub max_health: f32,
    pub power_consumption: f32,
    pub power_generation: f32,
    pub is_active: bool,
    pub grid_position: IVec2,
    pub size: IVec2,
    pub rotation: Rotation,
}

/// Sprite color as spawned, before any damage tinting — captured once so the
/// gradual damage tint has a stable color to blend from/back to instead of
/// darkening the already-darkened color further each frame.
#[derive(Component, Clone, Copy)]
pub struct BaseSpriteColor(pub Color);

/// 4-direction rotation for modules
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum Rotation {
    #[default]
    North,
    East,
    South,
    West,
}

impl Rotation {
    pub fn rotate_cw(&self) -> Self {
        match self {
            Rotation::North => Rotation::East,
            Rotation::East => Rotation::South,
            Rotation::South => Rotation::West,
            Rotation::West => Rotation::North,
        }
    }

    /// Transform a local grid offset by this rotation
    pub fn rotate_offset(&self, offset: IVec2) -> IVec2 {
        match self {
            Rotation::North => offset,
            Rotation::East => IVec2::new(offset.y, -offset.x),
            Rotation::South => IVec2::new(-offset.x, -offset.y),
            Rotation::West => IVec2::new(-offset.y, offset.x),
        }
    }

    pub fn to_radians(&self) -> f32 {
        match self {
            Rotation::North => 0.0,
            Rotation::East => -std::f32::consts::FRAC_PI_2,
            Rotation::South => std::f32::consts::PI,
            Rotation::West => std::f32::consts::FRAC_PI_2,
        }
    }
}

/// Module categories for building UI and logic grouping
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ModuleCategory {
    Power,
    Propulsion,
    LifeSupport,
    Control,
    Weapons,
    Detection,
    Storage,
    Crew,
    Utility,
    Structural,
}

impl ModuleCategory {
    pub const ALL: &'static [ModuleCategory] = &[
        ModuleCategory::Power,
        ModuleCategory::Propulsion,
        ModuleCategory::LifeSupport,
        ModuleCategory::Control,
        ModuleCategory::Weapons,
        ModuleCategory::Detection,
        ModuleCategory::Storage,
        ModuleCategory::Crew,
        ModuleCategory::Utility,
        ModuleCategory::Structural,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            ModuleCategory::Power => "Power",
            ModuleCategory::Propulsion => "Propulsion",
            ModuleCategory::LifeSupport => "Life Support",
            ModuleCategory::Control => "Control",
            ModuleCategory::Weapons => "Weapons",
            ModuleCategory::Detection => "Detection",
            ModuleCategory::Storage => "Storage",
            ModuleCategory::Crew => "Crew",
            ModuleCategory::Utility => "Utility",
            ModuleCategory::Structural => "Structural",
        }
    }

    pub fn module_types(&self) -> &'static [ModuleType] {
        match self {
            ModuleCategory::Power => &[
                ModuleType::SmallReactor,
                ModuleType::StandardReactor,
                ModuleType::LargeReactor,
                ModuleType::BatteryBank,
                ModuleType::RTG,
                ModuleType::FusionReactor,
                ModuleType::Capacitor,
                ModuleType::PowerConduit,
                ModuleType::SolarCell,
                ModuleType::Transformer,
                ModuleType::ThermalVentGenerator,
            ],
            ModuleCategory::Propulsion => &[
                ModuleType::SmallEngine,
                ModuleType::StandardEngine,
                ModuleType::LargeEngine,
                ModuleType::SilentDrive,
                ModuleType::ManeuveringThruster,
                ModuleType::JetDrive,
                ModuleType::EmergencyThruster,
                ModuleType::RudderAssembly,
                ModuleType::VectorThruster,
                ModuleType::AttitudeThruster,
            ],
            ModuleCategory::LifeSupport => &[
                ModuleType::OxygenScrubber,
                ModuleType::CO2Scrubber,
                ModuleType::WasteRecycler,
                ModuleType::AdvancedOxygenator,
                ModuleType::FireSuppression,
                ModuleType::AtmosphereMonitor,
                ModuleType::OxygenTank,
                ModuleType::AirCirculator,
            ],
            ModuleCategory::Control => &[
                ModuleType::NavigationConsole,
                ModuleType::HelmStation,
                ModuleType::TargetingComputer,
                ModuleType::AutopilotCore,
                ModuleType::AICombatCore,
                ModuleType::BridgeWing,
            ],
            ModuleCategory::Weapons => &[
                ModuleType::Cannon,
                ModuleType::Railgun,
                ModuleType::Coilgun,
                ModuleType::Gatling,
                ModuleType::Laser,
                ModuleType::PlasmaCaster,
                ModuleType::IonDisruptor,
                ModuleType::HeavyMissile,
                ModuleType::GuidedMissile,
                ModuleType::ClusterRocket,
                ModuleType::MiningDrill,
                ModuleType::TractorBeam,
                ModuleType::EMPPulse,
                ModuleType::AmmoAutoloader,
            ],
            ModuleCategory::Detection => &[
                ModuleType::RadarArray,
                ModuleType::AdvancedRadar,
                ModuleType::PassiveRadar,
                ModuleType::DepthScanner,
                ModuleType::HydrophoneArray,
                ModuleType::ThermalImager,
                ModuleType::ProximityAlarm,
                ModuleType::CreatureScanner,
                ModuleType::MineralScanner,
                ModuleType::PressureSensor,
            ],
            ModuleCategory::Storage => &[
                ModuleType::SmallCargo,
                ModuleType::LargeCargo,
                ModuleType::AmmoBay,
                ModuleType::FuelTank,
                ModuleType::SpecimenVault,
                ModuleType::ReinforcedVault,
                ModuleType::CryoStorage,
                ModuleType::CreatureContainment,
                ModuleType::BulkCargoHold,
            ],
            ModuleCategory::Crew => &[
                ModuleType::BasicQuarters,
                ModuleType::Barracks,
                ModuleType::MedBay,
                ModuleType::MessHall,
                ModuleType::RecRoom,
                ModuleType::OfficerQuarters,
                ModuleType::TrainingRoom,
                ModuleType::Brig,
                ModuleType::EngineeringStation,
                ModuleType::SurgicalBay,
                ModuleType::GalleyMess,
                ModuleType::WellnessHub,
            ],
            ModuleCategory::Utility => &[
                ModuleType::RepairBay,
                ModuleType::ManeuverThruster,
                ModuleType::Floodlight,
                ModuleType::Searchlight,
                ModuleType::AirlockChamber,
                ModuleType::DockingPort,
                ModuleType::SalvageArm,
                ModuleType::AdvancedRepairBay,
                ModuleType::DroneBay,
                ModuleType::DeepFloodlight,
                ModuleType::StealthCoating,
                ModuleType::HullPatch,
                ModuleType::SignalBuoy,
                ModuleType::CoolingPump,
                ModuleType::HeatVent,
                ModuleType::ConveyorTube,
                ModuleType::MaintenanceLocker,
                ModuleType::FuelProcessor,
                ModuleType::HullSealer,
                ModuleType::MineralExtractor,
                ModuleType::ResearchLab,
                ModuleType::DockingHub,
                ModuleType::AngledArmorPlate,
            ],
            ModuleCategory::Structural => &[
                ModuleType::HullBeam,
                ModuleType::HullCorner,
                ModuleType::Bulkhead,
                ModuleType::PressureFrame,
                ModuleType::AirlockValve,
                ModuleType::AccessHatch,
                ModuleType::ViewPort,
                ModuleType::ArmorPlate,
                ModuleType::EmergencyBulkhead,
                ModuleType::FirebreakWall,
                ModuleType::Corridor,
                ModuleType::LadderShaft,
                ModuleType::MaintenanceTunnel,
                ModuleType::CornerArmorPlate,
                ModuleType::StaggeredArmorPlate,
                ModuleType::AngledHullPlate,
            ],
        }
    }
}

/// All module types in the game (90 variants)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub enum ModuleType {
    // Power (9)
    SmallReactor,
    StandardReactor,
    LargeReactor,
    BatteryBank,
    RTG,
    FusionReactor,
    Capacitor,
    PowerConduit,
    SolarCell,

    // Propulsion (8)
    SmallEngine,
    StandardEngine,
    LargeEngine,
    SilentDrive,
    ManeuveringThruster,
    JetDrive,
    EmergencyThruster,
    RudderAssembly,

    // Life Support (6)
    OxygenScrubber,
    CO2Scrubber,
    WasteRecycler,
    AdvancedOxygenator,
    FireSuppression,
    AtmosphereMonitor,

    // Control (2)
    NavigationConsole,
    HelmStation,

    // Weapons — Kinetic (4)
    Cannon,
    Railgun,
    Coilgun,
    Gatling,
    // Weapons — Energy (3)
    Laser,
    PlasmaCaster,
    IonDisruptor,
    // Weapons — Missile (3)
    HeavyMissile,
    GuidedMissile,
    ClusterRocket,
    // Weapons — Utility (3)
    MiningDrill,
    TractorBeam,
    EMPPulse,

    // Detection (7)
    RadarArray,
    AdvancedRadar,
    PassiveRadar,
    DepthScanner,
    HydrophoneArray,
    ThermalImager,
    ProximityAlarm,

    // Storage (7)
    SmallCargo,
    LargeCargo,
    AmmoBay,
    FuelTank,
    SpecimenVault,
    ReinforcedVault,
    CryoStorage,

    // Crew (8)
    BasicQuarters,
    Barracks,
    MedBay,
    MessHall,
    RecRoom,
    OfficerQuarters,
    TrainingRoom,
    Brig,

    // Utility (15)
    RepairBay,
    ManeuverThruster,
    Floodlight,
    Searchlight,
    AirlockChamber,
    DockingPort,
    SalvageArm,
    AdvancedRepairBay,
    DroneBay,
    DeepFloodlight,
    StealthCoating,
    HullPatch,
    SignalBuoy,
    CoolingPump,
    HeatVent,

    // Structural (8)
    HullBeam,
    HullCorner,
    Bulkhead,
    PressureFrame,
    AirlockValve,
    AccessHatch,
    ViewPort,
    ArmorPlate,

    // Power (new)
    Transformer,

    // Propulsion (new)
    VectorThruster,
    AttitudeThruster,

    // Life Support (new)
    OxygenTank,
    AirCirculator,

    // Detection (new)
    CreatureScanner,
    MineralScanner,

    // Weapons (new)
    AmmoAutoloader,

    // Crew (new)
    EngineeringStation,

    // Phase B: Logistics
    ConveyorTube,
    MaintenanceLocker,
    FuelProcessor,

    // Phase B: Damage Infrastructure
    HullSealer,
    EmergencyBulkhead,
    FirebreakWall,
    PressureSensor,

    // Phase B: Navigation & Control
    TargetingComputer,
    AutopilotCore,
    AICombatCore,

    // Phase B: Environmental Interaction
    ThermalVentGenerator,
    MineralExtractor,
    CreatureContainment,
    ResearchLab,

    // Phase B: Interior Tiles
    Corridor,
    LadderShaft,
    MaintenanceTunnel,

    // Multi-block extension blocks (core)
    BarrelExtension,
    AmmoFeedUnit,
    CoolingJacket,
    ReactorFuelRod,
    ReactorCooling,
    EngineNozzle,
    ShieldEmitter,

    // Advanced weapon enhancers (optional optimization)
    MuzzleBrake,          // Reduces recoil, increases accuracy
    RecoilAbsorber,       // Protects adjacent blocks from firing stress
    OverchargeCapacitor,  // One-shot damage boost, long cooldown
    BoreEvacuator,        // Clears barrel fumes, faster follow-up shots
    MagneticAccelerator,  // Boosts projectile velocity (railgun/coilgun)
    FocusingArray,        // Tightens energy weapon beam
    WarheadBay,           // Extra torpedo/missile storage

    // Advanced reactor enhancers
    FuelEnrichmentUnit,   // More power per fuel rod, more heat
    ContainmentField,     // Reduces explosion radius if reactor blows
    EmergencyShutdown,    // Auto-kills reactor before meltdown
    PowerRegulator,       // Smooths output fluctuations, reduces waste

    // Advanced engine enhancers
    Afterburner,          // Temporary speed boost, burns fuel fast
    ThrustVectoring,      // Improves turning at high speed
    FuelInjector,         // Better fuel efficiency
    InertialDampener,     // Reduces drift, tighter handling

    // Defense modules (passive/active protection)
    DecoyLauncher,        // Distracts guided missiles
    ChaffDispenser,       // Disrupts targeting systems
    AblativeArmor,        // Takes hits instead of hull, replaceable
    PointDefenseDrone,    // Auto-shoots incoming projectiles
    HullReinforcePlate,   // Reduces cascade chance in adjacent blocks

    // Advanced utility
    SignalJammer,         // Reduces enemy detection range
    GravityCompensator,   // Reduces gravity pull on ship
    RadiationHardening,   // Reduces radiation damage to adjacent modules
    EmergencyO2Cache,     // Burst oxygen when life support fails
    BlackBox,             // Records ship data, survives destruction

    // Structural enhancers
    ReinforcedJoint,      // Reduces cascade explosion chance in barrel chains
    VibrationDamper,      // Reduces accuracy loss from adjacent firing
    ThermalInsulator,     // Blocks heat spread between sections
    StructuralBrace,      // Increases HP of adjacent hull/modules
    CornerArmorPlate,     // L-shaped armor block — leaves its notch cell free for a neighbor

    // Shaped modules — shape chosen for what the module does, see MODULES.md
    BridgeWing,           // T-shaped command bridge — wings extend past the hull for docking visibility
    SurgicalBay,          // T-shaped sickbay — narrow triage entry into a wider treatment bay
    GalleyMess,           // L-shaped mess — long galley run + a dining nook
    BulkCargoHold,        // L-shaped cargo hold — fills a leftover hull corner
    DockingHub,           // Plus-shaped hub — multiple simultaneous docking connections
    WellnessHub,          // Plus-shaped gym/training hub — one node, access from every side
    StaggeredArmorPlate,  // S-shaped armor — no single straight seam runs through it
    AngledHullPlate,      // Wedge-cut hull framing piece — visual taper, rotation sets which corner
    AngledArmorPlate,     // Wedge-cut armor plate — visual taper, rotation sets which corner
}

impl ModuleType {
    pub fn category(&self) -> ModuleCategory {
        match self {
            ModuleType::SmallReactor | ModuleType::StandardReactor |
            ModuleType::LargeReactor | ModuleType::BatteryBank |
            ModuleType::RTG | ModuleType::FusionReactor |
            ModuleType::Capacitor | ModuleType::PowerConduit |
            ModuleType::SolarCell |
            ModuleType::Transformer |
            ModuleType::ThermalVentGenerator |
            ModuleType::FuelEnrichmentUnit |
            ModuleType::ContainmentField |
            ModuleType::EmergencyShutdown |
            ModuleType::PowerRegulator => ModuleCategory::Power,

            ModuleType::SmallEngine | ModuleType::StandardEngine |
            ModuleType::LargeEngine | ModuleType::SilentDrive |
            ModuleType::ManeuveringThruster | ModuleType::JetDrive |
            ModuleType::EmergencyThruster |
            ModuleType::RudderAssembly |
            ModuleType::VectorThruster |
            ModuleType::AttitudeThruster |
            ModuleType::Afterburner |
            ModuleType::ThrustVectoring |
            ModuleType::FuelInjector |
            ModuleType::InertialDampener => ModuleCategory::Propulsion,

            ModuleType::OxygenScrubber | ModuleType::CO2Scrubber |
            ModuleType::WasteRecycler | ModuleType::AdvancedOxygenator |
            ModuleType::FireSuppression |
            ModuleType::AtmosphereMonitor |
            ModuleType::OxygenTank |
            ModuleType::AirCirculator => ModuleCategory::LifeSupport,

            ModuleType::NavigationConsole |
            ModuleType::HelmStation |
            ModuleType::TargetingComputer |
            ModuleType::AutopilotCore |
            ModuleType::BridgeWing |
            ModuleType::AICombatCore => ModuleCategory::Control,

            ModuleType::Cannon | ModuleType::Railgun |
            ModuleType::Coilgun | ModuleType::Gatling |
            ModuleType::Laser | ModuleType::PlasmaCaster |
            ModuleType::IonDisruptor |
            ModuleType::HeavyMissile | ModuleType::GuidedMissile |
            ModuleType::ClusterRocket |
            ModuleType::MiningDrill | ModuleType::TractorBeam |
            ModuleType::EMPPulse |
            ModuleType::AmmoAutoloader |
            ModuleType::BarrelExtension |
            ModuleType::AmmoFeedUnit |
            ModuleType::CoolingJacket |
            ModuleType::MuzzleBrake |
            ModuleType::RecoilAbsorber |
            ModuleType::OverchargeCapacitor |
            ModuleType::BoreEvacuator |
            ModuleType::MagneticAccelerator |
            ModuleType::FocusingArray |
            ModuleType::WarheadBay => ModuleCategory::Weapons,

            ModuleType::ReactorFuelRod |
            ModuleType::ReactorCooling => ModuleCategory::Power,

            ModuleType::EngineNozzle => ModuleCategory::Propulsion,

            ModuleType::ShieldEmitter |
            ModuleType::DecoyLauncher |
            ModuleType::ChaffDispenser |
            ModuleType::AblativeArmor |
            ModuleType::PointDefenseDrone |
            ModuleType::HullReinforcePlate |
            ModuleType::SignalJammer |
            ModuleType::GravityCompensator |
            ModuleType::RadiationHardening |
            ModuleType::EmergencyO2Cache |
            ModuleType::BlackBox => ModuleCategory::Utility,

            ModuleType::ReinforcedJoint |
            ModuleType::VibrationDamper |
            ModuleType::ThermalInsulator |
            ModuleType::StructuralBrace |
            ModuleType::CornerArmorPlate |
            ModuleType::StaggeredArmorPlate |
            ModuleType::AngledHullPlate => ModuleCategory::Structural,

            ModuleType::RadarArray | ModuleType::AdvancedRadar |
            ModuleType::PassiveRadar | ModuleType::DepthScanner |
            ModuleType::HydrophoneArray | ModuleType::ThermalImager |
            ModuleType::ProximityAlarm |
            ModuleType::CreatureScanner |
            ModuleType::MineralScanner |
            ModuleType::PressureSensor => ModuleCategory::Detection,

            ModuleType::SmallCargo | ModuleType::LargeCargo |
            ModuleType::AmmoBay | ModuleType::FuelTank |
            ModuleType::SpecimenVault | ModuleType::ReinforcedVault |
            ModuleType::CryoStorage |
            ModuleType::BulkCargoHold |
            ModuleType::CreatureContainment => ModuleCategory::Storage,

            ModuleType::BasicQuarters | ModuleType::Barracks |
            ModuleType::MedBay | ModuleType::MessHall |
            ModuleType::RecRoom | ModuleType::OfficerQuarters |
            ModuleType::TrainingRoom |
            ModuleType::Brig |
            ModuleType::SurgicalBay |
            ModuleType::GalleyMess |
            ModuleType::WellnessHub |
            ModuleType::EngineeringStation => ModuleCategory::Crew,

            ModuleType::RepairBay | ModuleType::ManeuverThruster |
            ModuleType::Floodlight | ModuleType::Searchlight |
            ModuleType::AirlockChamber | ModuleType::DockingPort |
            ModuleType::SalvageArm | ModuleType::AdvancedRepairBay |
            ModuleType::DroneBay | ModuleType::DeepFloodlight |
            ModuleType::StealthCoating | ModuleType::HullPatch |
            ModuleType::SignalBuoy |
            ModuleType::CoolingPump |
            ModuleType::HeatVent |
            ModuleType::ConveyorTube |
            ModuleType::MaintenanceLocker |
            ModuleType::FuelProcessor |
            ModuleType::HullSealer |
            ModuleType::MineralExtractor |
            ModuleType::DockingHub |
            ModuleType::AngledArmorPlate |
            ModuleType::ResearchLab => ModuleCategory::Utility,

            ModuleType::HullBeam | ModuleType::HullCorner |
            ModuleType::Bulkhead | ModuleType::PressureFrame |
            ModuleType::AirlockValve | ModuleType::AccessHatch |
            ModuleType::ViewPort |
            ModuleType::ArmorPlate |
            ModuleType::EmergencyBulkhead |
            ModuleType::FirebreakWall |
            ModuleType::Corridor |
            ModuleType::LadderShaft |
            ModuleType::MaintenanceTunnel => ModuleCategory::Structural,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            ModuleType::SmallReactor => "Small Reactor",
            ModuleType::StandardReactor => "Standard Reactor",
            ModuleType::LargeReactor => "Large Reactor",
            ModuleType::BatteryBank => "Battery Bank",
            ModuleType::RTG => "RTG",
            ModuleType::FusionReactor => "Fusion Reactor",
            ModuleType::Capacitor => "Capacitor",
            ModuleType::PowerConduit => "Power Conduit",
            ModuleType::SolarCell => "Solar Cell",
            ModuleType::SmallEngine => "Small Engine",
            ModuleType::StandardEngine => "Standard Engine",
            ModuleType::LargeEngine => "Large Engine",
            ModuleType::SilentDrive => "Stealth Drive",
            ModuleType::ManeuveringThruster => "Maneuvering Thruster",
            ModuleType::JetDrive => "Jet Drive",
            ModuleType::EmergencyThruster => "Emergency Thruster",
            ModuleType::RudderAssembly => "Rudder Assembly",
            ModuleType::OxygenScrubber => "O2 Scrubber",
            ModuleType::CO2Scrubber => "CO2 Scrubber",
            ModuleType::WasteRecycler => "Waste Recycler",
            ModuleType::AdvancedOxygenator => "Advanced Oxygenator",
            ModuleType::FireSuppression => "Fire Suppression",
            ModuleType::AtmosphereMonitor => "Atmosphere Monitor",
            ModuleType::NavigationConsole => "Navigation Console",
            ModuleType::HelmStation => "Helm Station",
            ModuleType::Cannon => "Cannon",
            ModuleType::Railgun => "Railgun",
            ModuleType::Coilgun => "Coilgun",
            ModuleType::Gatling => "Gatling",
            ModuleType::Laser => "Laser",
            ModuleType::PlasmaCaster => "Plasma Caster",
            ModuleType::IonDisruptor => "Ion Disruptor",
            ModuleType::HeavyMissile => "Heavy Missile Launcher",
            ModuleType::GuidedMissile => "Guided Missile",
            ModuleType::ClusterRocket => "Cluster Rocket",
            ModuleType::MiningDrill => "Mining Drill",
            ModuleType::TractorBeam => "Tractor Beam",
            ModuleType::EMPPulse => "EMP Pulse",
            ModuleType::RadarArray => "Radar Array",
            ModuleType::AdvancedRadar => "Advanced Radar",
            ModuleType::PassiveRadar => "Passive Sensor",
            ModuleType::DepthScanner => "Long Range Scanner",
            ModuleType::HydrophoneArray => "Signal Interceptor",
            ModuleType::ThermalImager => "Thermal Imager",
            ModuleType::ProximityAlarm => "Proximity Alarm",
            ModuleType::SmallCargo => "Small Cargo",
            ModuleType::LargeCargo => "Large Cargo",
            ModuleType::AmmoBay => "Ammo Bay",
            ModuleType::FuelTank => "Fuel Tank",
            ModuleType::SpecimenVault => "Specimen Vault",
            ModuleType::ReinforcedVault => "Reinforced Vault",
            ModuleType::CryoStorage => "Cryo Storage",
            ModuleType::BasicQuarters => "Basic Quarters",
            ModuleType::Barracks => "Barracks",
            ModuleType::MedBay => "Med Bay",
            ModuleType::MessHall => "Mess Hall",
            ModuleType::RecRoom => "Rec Room",
            ModuleType::OfficerQuarters => "Officer Quarters",
            ModuleType::TrainingRoom => "Training Room",
            ModuleType::Brig => "Brig",
            ModuleType::RepairBay => "Repair Bay",
            ModuleType::ManeuverThruster => "Maneuvering Thruster",
            ModuleType::Floodlight => "Spotlight",
            ModuleType::Searchlight => "Searchlight",
            ModuleType::AirlockChamber => "Airlock Chamber",
            ModuleType::DockingPort => "Docking Port",
            ModuleType::SalvageArm => "Breaker Drill",
            ModuleType::AdvancedRepairBay => "Advanced Repair Bay",
            ModuleType::DroneBay => "Drone Bay",
            ModuleType::DeepFloodlight => "High-Power Spotlight",
            ModuleType::StealthCoating => "Stealth Coating",
            ModuleType::HullPatch => "Hull Patch",
            ModuleType::SignalBuoy => "Signal Buoy",
            ModuleType::HullBeam => "Hull Beam",
            ModuleType::HullCorner => "Hull Corner",
            ModuleType::Bulkhead => "Bulkhead",
            ModuleType::PressureFrame => "Radiation Shield",
            ModuleType::AirlockValve => "Airlock Valve",
            ModuleType::AccessHatch => "Access Hatch",
            ModuleType::ViewPort => "View Port",
            ModuleType::ArmorPlate => "Armor Plate",
            ModuleType::CoolingPump => "Cooling Pump",
            ModuleType::HeatVent => "Heat Vent",
            ModuleType::Transformer => "Transformer",
            ModuleType::VectorThruster => "Vector Thruster",
            ModuleType::AttitudeThruster => "Attitude Thruster",
            ModuleType::OxygenTank => "Oxygen Tank",
            ModuleType::AirCirculator => "Air Circulator",
            ModuleType::CreatureScanner => "Creature Scanner",
            ModuleType::MineralScanner => "Mineral Scanner",
            ModuleType::AmmoAutoloader => "Ammo Autoloader",
            ModuleType::EngineeringStation => "Engineering Station",
            ModuleType::ConveyorTube => "Conveyor Tube",
            ModuleType::MaintenanceLocker => "Maintenance Locker",
            ModuleType::FuelProcessor => "Fuel Processor",
            ModuleType::HullSealer => "Hull Seal System",
            ModuleType::EmergencyBulkhead => "Emergency Bulkhead",
            ModuleType::FirebreakWall => "Firebreak Wall",
            ModuleType::PressureSensor => "Radiation Sensor",
            ModuleType::TargetingComputer => "Targeting Computer",
            ModuleType::AutopilotCore => "Autopilot Core",
            ModuleType::AICombatCore => "AI Combat Core",
            ModuleType::ThermalVentGenerator => "Thermal Vent Generator",
            ModuleType::MineralExtractor => "Mineral Extractor",
            ModuleType::CreatureContainment => "Creature Containment",
            ModuleType::ResearchLab => "Research Lab",
            ModuleType::Corridor => "Corridor",
            ModuleType::LadderShaft => "Shaft",
            ModuleType::MaintenanceTunnel => "Maintenance Tunnel",
            // Multi-block extension blocks
            ModuleType::BarrelExtension => "Barrel Extension",
            ModuleType::AmmoFeedUnit => "Ammo Feed",
            ModuleType::CoolingJacket => "Cooling Jacket",
            ModuleType::ReactorFuelRod => "Fuel Rod",
            ModuleType::ReactorCooling => "Reactor Cooling",
            ModuleType::EngineNozzle => "Engine Nozzle",
            ModuleType::ShieldEmitter => "Shield Emitter",
            // Advanced weapon enhancers
            ModuleType::MuzzleBrake => "Muzzle Brake",
            ModuleType::RecoilAbsorber => "Recoil Absorber",
            ModuleType::OverchargeCapacitor => "Overcharge Capacitor",
            ModuleType::BoreEvacuator => "Bore Evacuator",
            ModuleType::MagneticAccelerator => "Magnetic Accelerator",
            ModuleType::FocusingArray => "Focusing Array",
            ModuleType::WarheadBay => "Warhead Bay",
            // Advanced reactor enhancers
            ModuleType::FuelEnrichmentUnit => "Fuel Enrichment Unit",
            ModuleType::ContainmentField => "Containment Field",
            ModuleType::EmergencyShutdown => "Emergency Shutdown",
            ModuleType::PowerRegulator => "Power Regulator",
            // Advanced engine enhancers
            ModuleType::Afterburner => "Afterburner",
            ModuleType::ThrustVectoring => "Thrust Vectoring",
            ModuleType::FuelInjector => "Fuel Injector",
            ModuleType::InertialDampener => "Inertial Dampener",
            // Defense modules
            ModuleType::DecoyLauncher => "Decoy Launcher",
            ModuleType::ChaffDispenser => "Chaff Dispenser",
            ModuleType::AblativeArmor => "Ablative Armor",
            ModuleType::PointDefenseDrone => "Point Defense Drone",
            ModuleType::HullReinforcePlate => "Hull Reinforce Plate",
            // Advanced utility
            ModuleType::SignalJammer => "Signal Jammer",
            ModuleType::GravityCompensator => "Gravity Compensator",
            ModuleType::RadiationHardening => "Radiation Hardening",
            ModuleType::EmergencyO2Cache => "Emergency O2 Cache",
            ModuleType::BlackBox => "Black Box",
            // Structural enhancers
            ModuleType::ReinforcedJoint => "Reinforced Joint",
            ModuleType::VibrationDamper => "Vibration Damper",
            ModuleType::ThermalInsulator => "Thermal Insulator",
            ModuleType::StructuralBrace => "Structural Brace",
            ModuleType::CornerArmorPlate => "Corner Armor Plate",
            ModuleType::BridgeWing => "Bridge Wing",
            ModuleType::SurgicalBay => "Surgical Bay",
            ModuleType::GalleyMess => "Galley Mess",
            ModuleType::BulkCargoHold => "Bulk Cargo Hold",
            ModuleType::DockingHub => "Docking Hub",
            ModuleType::WellnessHub => "Wellness Hub",
            ModuleType::StaggeredArmorPlate => "Staggered Armor Plate",
            ModuleType::AngledHullPlate => "Angled Hull Plate",
            ModuleType::AngledArmorPlate => "Angled Armor Plate",
        }
    }
}

// ============================================================================
// EXISTING MODULE COMPANION COMPONENTS (kept for backward compat)
// ============================================================================

/// Reactor specific data
#[derive(Component)]
pub struct Reactor {
    pub output: f32,
    pub heat: f32,
    pub max_heat: f32,
    pub explosion_risk: bool,
}

/// Engine specific data
#[derive(Component)]
pub struct Engine {
    pub thrust: f32,
    pub fuel_consumption: f32,
    pub noise_level: f32,
}

/// Oxygen scrubber data
#[derive(Component)]
pub struct OxygenScrubber {
    pub output: f32,
}

/// Thruster data (replaces thruster for space maneuvering)
#[derive(Component)]
pub struct Thruster {
    pub thrust_power: f32,
    pub current_output: f32,    // 0 = idle, 1 = full thrust
}

/// Cargo hold data
#[derive(Component)]
pub struct CargoHold {
    pub capacity: f32,
    pub current_weight: f32,
}

/// Weapon data
#[derive(Component)]
pub struct Weapon {
    pub damage: f32,
    pub range: f32,
    pub fire_rate: f32,
    pub ammo: u32,
    pub max_ammo: u32,
}

/// Projectile entity - travels through the world and damages on contact
#[derive(Component)]
pub struct Projectile {
    pub damage: f32,
    pub speed: f32,
    pub direction: Vec2,
    pub lifetime: Timer,
    pub from_player: bool,
    pub ammo_type: AmmoType,
}

/// Deployed mine - arms after delay, detonates on creature proximity
#[derive(Component)]
pub struct Mine {
    pub damage: f32,
    pub detection_radius: f32,
    pub arm_timer: Timer,
    pub lifetime: Timer,
}

/// Mine explosion - damages all creatures in blast radius over a brief period
#[derive(Component)]
pub struct MineExplosion {
    pub damage: f32,
    pub blast_radius: f32,
    pub applied: bool,
    pub timer: Timer,
}

/// Radar data
#[derive(Component)]
pub struct Radar {
    pub range: f32,
    pub noise_on_ping: f32,
    pub is_pinging: bool,
}

/// Light data
#[derive(Component)]
pub struct ShipLight {
    pub range: f32,
    pub intensity: f32,
    pub attracts_creatures: bool,
}

// ============================================================================
// NEW WEAPON COMPANION COMPONENTS
// ============================================================================

/// Weapon mount type - determines firing arc
#[derive(Component)]
pub struct WeaponMount {
    pub mount_type: MountType,
    pub firing_arc: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MountType {
    Fixed,
    Turret,
    Broadside,
}

/// Ammo storage for weapon systems
#[derive(Component)]
pub struct AmmoStorage {
    pub ammo_type: AmmoType,
    pub capacity: u32,
    pub current: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AmmoType {
    Missile,
    Bullet,
    Charge,
    Mine,
}

impl AmmoType {
    pub fn speed_mult(&self) -> f32 {
        match self {
            AmmoType::Missile => 0.8,
            AmmoType::Bullet  => 1.5,
            AmmoType::Charge  => 0.6,
            AmmoType::Mine    => 0.0,
        }
    }

    pub fn hit_radius_mult(&self) -> f32 {
        match self {
            AmmoType::Missile => 1.5,
            AmmoType::Bullet  => 0.7,
            AmmoType::Charge  => 2.5,
            AmmoType::Mine    => 1.0,
        }
    }

    pub fn is_aoe(&self) -> bool {
        matches!(self, AmmoType::Charge)
    }

    pub fn projectile_color(&self) -> Color {
        match self {
            AmmoType::Missile => Color::srgb(1.0, 0.9, 0.3),
            AmmoType::Bullet  => Color::srgb(1.0, 1.0, 1.0),
            AmmoType::Charge  => Color::srgb(0.4, 0.6, 1.0),
            AmmoType::Mine    => Color::srgb(0.6, 0.6, 0.6),
        }
    }

    pub fn projectile_size(&self) -> Vec2 {
        match self {
            AmmoType::Missile => Vec2::new(18.0, 7.0),
            AmmoType::Bullet  => Vec2::new(10.0, 4.0),
            AmmoType::Charge  => Vec2::new(22.0, 22.0),
            AmmoType::Mine    => Vec2::splat(14.0),
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            AmmoType::Missile => "Missile launched!",
            AmmoType::Bullet  => "Firing!",
            AmmoType::Charge  => "Charge fired!",
            AmmoType::Mine    => "Mine deployed!",
        }
    }
}

/// Targeting system for auto-aim
#[derive(Component)]
pub struct TargetingSystem {
    pub tracking_speed: f32,
    pub lock_on_time: f32,
    pub max_targets: u32,
}

// ============================================================================
// NEW LIFE SUPPORT / DETECTION / UTILITY COMPANION COMPONENTS
// ============================================================================

/// Life support system (CO2 filtering, water recycling)
#[derive(Component)]
pub struct LifeSupportSystem {
    pub o2_generation: f32,
    pub co2_filtering: f32,
    pub waste_recycling: f32,
}

/// Detection system (non-radar detection)
#[derive(Component)]
pub struct DetectionSystem {
    pub range: f32,
    pub is_passive: bool,
    pub scan_interval: f32,
}

/// Repair system capability
#[derive(Component)]
pub struct RepairSystem {
    pub repair_rate: f32,
    pub hull_repair: bool,
    pub module_repair: bool,
}

/// Salvage system
#[derive(Component)]
pub struct SalvageSystem {
    pub range: f32,
    pub efficiency: f32,
}

/// Docking system
#[derive(Component)]
pub struct DockingComp {
    pub docking_speed: f32,
}

/// Navigation system
#[derive(Component)]
pub struct NavigationComp {
    pub map_range: f32,
    pub autopilot: bool,
}

/// Capacitor: stores power for burst discharge
#[derive(Component)]
pub struct CapacitorComp {
    pub capacity: f32,
    pub charge: f32,
    pub charge_rate: f32,
}

/// Power conduit: routes power through non-adjacent modules
#[derive(Component)]
pub struct PowerConduitComp {
    pub throughput: f32,
}

/// Fire suppression system
#[derive(Component)]
pub struct FireSuppressionComp {
    pub effectiveness: f32,
    pub active: bool,
}

/// Radiation shielding reinforcement: increases radiation tolerance for adjacent hull
#[derive(Component)]
pub struct RadiationShieldingComp {
    pub shielding_bonus: f32,
}

/// Drone bay: deploys repair/scout drones
#[derive(Component)]
pub struct DroneBayComp {
    pub drone_count: u32,
    pub drone_range: f32,
    pub drones_deployed: u32,
}

// ============================================================================
// HEAT NETWORK + NEW BLOCK COMPONENTS
// ============================================================================

/// Per-module temperature tracking for the heat network system
#[derive(Component)]
pub struct ModuleTemperature {
    pub current: f32,
    pub max_temp: f32,
    pub conductivity: f32,
}

/// Active cooling pump — draws heat from adjacent blocks
#[derive(Component)]
pub struct CoolingPumpComp {
    pub cooling_rate: f32,
}

/// Passive heat vent — dissipates heat to environment
#[derive(Component)]
pub struct HeatVentComp {
    pub dissipation_rate: f32,
}

/// Power transformer — reduces power loss over distance
#[derive(Component)]
pub struct TransformerComp {
    pub efficiency: f32,
}

/// Oxygen storage tank — stores O2 reserve for emergencies
#[derive(Component)]
pub struct OxygenTankComp {
    pub capacity: f32,
    pub stored: f32,
}

/// Ammo auto-loader — boosts fire rate of adjacent weapon systems
#[derive(Component)]
pub struct AmmoAutoloaderComp {
    pub reload_bonus: f32,
}

/// Conveyor tube — moves ammo/resources between adjacent modules
#[derive(Component)]
pub struct ConveyorTubeComp {
    pub speed: f32,
}

/// Fuel processor — refines fuel, reduces consumption
#[derive(Component)]
pub struct FuelProcessorComp {
    pub efficiency: f32,
}

/// Hull seal system — automated breach sealing for depressurized rooms
#[derive(Component)]
pub struct HullSealComp {
    pub seal_rate: f32,
}

/// Targeting computer — boosts weapon accuracy
#[derive(Component)]
pub struct TargetingComputerComp {
    pub accuracy_bonus: f32,
}

/// AI combat core — auto-targets highest threat
#[derive(Component)]
pub struct AICombatCoreComp {
    pub priority_bonus: f32,
}

/// Research lab — generates research points from specimens
#[derive(Component)]
pub struct ResearchLabComp {
    pub research_speed: f32,
}

/// Marker: firebreak wall blocks fire spread unconditionally
#[derive(Component)]
pub struct FirebreakMarker;

/// Marker component for damage overlay visibility
#[derive(Component)]
pub struct DamageOverlayVisible;

/// Marker for damage overlay sprite children
#[derive(Component)]
pub struct DamageOverlaySprite;

// ============================================================================
// CREW COMPONENTS
// ============================================================================

#[derive(Component, Clone, Serialize, Deserialize)]
pub struct CrewMember {
    pub name: String,
    pub health: f32,
    pub max_health: f32,
    pub oxygen: f32,
    pub morale: f32,
    pub state: CrewState,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum CrewState {
    Idle,
    Working,
    Moving,
    Repairing,
    Panicking,
    Unconscious,
    /// On EVA ferrying loot from a wreck (see crew::eva_salvage)
    Salvaging,
}

/// A module that can be staffed by one crew member.
#[derive(Component)]
pub struct CrewStation {
    pub priority: u8,              // 0 = don't auto-assign, 1-10 = priority
    pub assigned_crew: Option<Entity>,
    pub manually_assigned: bool,   // player locked this assignment
}

/// Right-click pin on a crew station: this post keeps its operator —
/// salvage details and other crew drafts never pull them. Unpinned
/// stations lose their crew to salvage duty (and stop working while
/// unmanned). Defaults when nothing is pinned: helm + one gun.
#[derive(Component)]
pub struct KeepManned;

/// Marks a module as providing crew berths.
#[derive(Component)]
pub struct Quarters {
    pub berths: u32,
}

/// Crew welfare facility (passive effect).
#[derive(Component)]
pub struct CrewFacility {
    pub facility_type: FacilityType,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FacilityType {
    MessHall,           // morale recovery boost
    RecRoom,            // morale floor at 30
    MedBay,             // heals crew in same room
    TrainingRoom,       // crew skill improvement
    EngineeringStation, // boosts repair rate of nearby modules
}

/// Computed efficiency combining damage + staffing. Updated each frame.
#[derive(Component)]
pub struct ModuleEfficiency {
    pub value: f32,           // damage_eff * staffing_eff
    pub staffing_factor: f32, // 0.5 unstaffed, 1.0 staffed
}

// ============================================================================
// WORLD COMPONENTS
// ============================================================================

#[derive(Component)]
pub struct Chunk {
    pub position: IVec2,
    pub is_explored: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ZoneType {
    NearOrbit,    // Low radiation zone
    AsteroidBelt, // Moderate radiation
    DeepSpace,    // High radiation
    Nebula,       // Extreme radiation
    BlackHole,    // Endgame
}

#[derive(Component)]
pub struct Wreck {
    pub loot_remaining: u32,
    pub is_explored: bool,
}

#[derive(Component)]
pub struct PointOfInterest {
    pub poi_type: PoiType,
    pub discovered: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum PoiType {
    Wreck,
    Cave,
    Ruins,
    ThermalVent,
    Settlement,
}

impl PoiType {
    /// Space-appropriate display names — the enum variants keep their old
    /// submarine-era identifiers, but nothing player-facing should ever say
    /// "Cave" in deep space.
    pub fn display_name(&self) -> &'static str {
        match self {
            PoiType::Wreck => "Drifting Wreck",
            PoiType::Cave => "Hollow Asteroid",
            PoiType::Ruins => "Ancient Ruins",
            PoiType::ThermalVent => "Plasma Vent",
            PoiType::Settlement => "Outpost Colony",
        }
    }
}

/// Log entry that can be found at POIs
#[derive(Component)]
pub struct LogEntry {
    pub title: String,
    pub text: String,
    pub depth_hint: f32,
}

// ============================================================================
// WORLD DECORATION COMPONENTS
// ============================================================================

#[derive(Component)]
pub struct WorldDecoration {
    pub decoration_type: DecorationType,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecorationType {
    Rock,
    SporeGrowth,
    Crystal,
    EnergySpot,
    ThermalVentSmoke,
    RockDebris,
}

// ============================================================================
// CREATURE COMPONENTS
// ============================================================================

#[derive(Component)]
pub struct Creature {
    pub creature_type: CreatureType,
    pub health: f32,
    pub max_health: f32,
    pub damage: f32,
    pub speed: f32,
    pub detection_range: f32,
    pub attack_cooldown: f32,
    pub food_value: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub enum CreatureType {
    VoidDrifter,     // Passive ambient life, clusters near planets
    Stalker,         // Ambush predator, uses gravity/terrain
    Leviathan,       // Apex predator, massive, territorial
    ParasiteSwarm,   // Tiny swarmers, attach to hull
}

/// What a creature is targeting
#[derive(Clone, Copy, Debug)]
pub enum EcoTarget {
    Ship(Entity),
    AiShip(Entity),
    Creature(Entity),
    Corpse(Entity),
    Position(Vec2),
}

#[derive(Component)]
pub struct CreatureAI {
    pub state: CreatureAIState,
    pub target: Option<EcoTarget>,
    pub home_position: Vec2,
    pub wander_radius: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CreatureAIState {
    Idle,
    Wandering,
    Hunting,
    Attacking,
    Fleeing,
    Observing,
    Feeding,
    Patrolling,
    Migrating,
    Investigating,
}

/// Attack cooldown for creatures
#[derive(Component)]
pub struct AttackCooldown {
    pub timer: Timer,
}

/// Weapon cooldown for ship weapons
#[derive(Component)]
pub struct WeaponCooldown {
    pub timer: Timer,
}

/// Radar ping visual ring
#[derive(Component)]
pub struct RadarPing {
    pub radius: f32,
    pub max_radius: f32,
    pub speed: f32,
}

/// Marks an entity as revealed by radar
#[derive(Component)]
pub struct RadarRevealed {
    pub timer: Timer,
}

/// Scatter behavior — creatures flee briefly from threats then resume
#[derive(Component)]
pub struct ScatterBehavior {
    pub scatter_timer: f32,
    pub base_velocity: Vec2,
}

/// Sprite sheet animation for creatures.
/// Tracks current frame, timing, and frame ranges for different animation states.
#[derive(Component)]
pub struct CreatureAnimation {
    pub timer: Timer,
    /// Number of swim frames (starting at index 0)
    pub swim_frames: usize,
    /// Number of attack frames (starting after swim frames)
    pub attack_frames: usize,
    /// Total frames in the sheet
    pub total_frames: usize,
    /// Current frame index
    pub current_frame: usize,
}

// ============================================================================
// ECOSYSTEM COMPONENTS
// ============================================================================

/// Biological needs that drive creature behavior
#[derive(Component)]
pub struct CreatureNeeds {
    pub hunger: f32,
    pub energy: f32,
    pub hunger_rate: f32,
    pub energy_drain_rate: f32,
}

/// Food chain tier
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum FoodChainTier {
    Apex,
    Predator,
    MesoPredator,
    Scavenger,
    Hive,
    Observer,
    Prey,
}

/// Defines a creature's role in the food chain
#[derive(Component)]
pub struct FoodChainRole {
    pub tier: FoodChainTier,
    pub prey_types: Vec<CreatureType>,
    pub threat_types: Vec<CreatureType>,
    pub attacks_ship: bool,
}

/// Territory a creature claims and defends
#[derive(Component)]
pub struct Territory {
    pub center: Vec2,
    pub radius: f32,
    pub aggression: f32,
}

/// A dead creature's remains
#[derive(Component)]
pub struct Corpse {
    pub creature_type: CreatureType,
    pub food_remaining: f32,
    pub decay_timer: f32,
}

/// Spatial memory for creatures
#[derive(Component)]
pub struct CreatureMemory {
    pub danger_zones: Vec<(Vec2, f32)>,
    pub food_locations: Vec<(Vec2, f32)>,
    pub last_seen_ship: Option<(Vec2, f32)>,
}

impl Default for CreatureMemory {
    fn default() -> Self {
        Self {
            danger_zones: Vec::new(),
            food_locations: Vec::new(),
            last_seen_ship: None,
        }
    }
}

/// Breeding capability
#[derive(Component)]
pub struct Reproductive {
    pub gestation_timer: f32,
    pub gestation_duration: f32,
    pub offspring_count: u32,
    pub satiation_threshold: f32,
}

/// Path for long-distance migration
#[derive(Component)]
pub struct MigrationPath {
    pub waypoints: Vec<Vec2>,
    pub current_waypoint: usize,
    pub arrival_radius: f32,
}

/// A point in the ship's noise trail
#[derive(Component)]
pub struct NoiseTrailPoint {
    pub intensity: f32,
    pub decay_rate: f32,
}

/// Timer tracking how long a creature has been hungry for migration checks
#[derive(Component)]
pub struct HungerDuration {
    pub timer: f32,
}

// ============================================================================
// SUBMARINE PHYSICS
// ============================================================================

/// Realistic ship physics model
#[derive(Component)]
pub struct ShipPhysics {
    pub mass: f32,
    pub drag_coefficient: f32,
    pub frontal_area: f32,
    pub angular_velocity: f32,
    pub rotation: f32,            // Current facing angle in radians
    pub throttle: f32,            // -1.0 to 1.0
    pub rudder: f32,              // -1.0 to 1.0
}

impl Default for ShipPhysics {
    fn default() -> Self {
        Self {
            mass: 1200.0,           // Heavier = more inertia, feels like a real ship
            drag_coefficient: 0.0,  // No drag in space. Momentum is forever. You must thrust to stop.
            frontal_area: 4.0,
            angular_velocity: 0.0,
            rotation: 0.0,
            throttle: 0.0,
            rudder: 0.0,
        }
    }
}

// ============================================================================
// PHYSICS / MOVEMENT
// ============================================================================

#[derive(Component)]
pub struct Velocity(pub Vec2);

#[derive(Component)]
pub struct Depth(pub f32);

#[derive(Component)]
pub struct ThrusterState {
    pub base_drift: f32,        // Natural drift from gravity/momentum
    pub current: f32,           // Modified by thrusters
}

// ============================================================================
// GENERAL
// ============================================================================

#[derive(Component)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

#[derive(Component)]
pub struct Selectable;

// ============================================================================
// CUSTOMIZATION SYSTEM
// ============================================================================

/// Marks a module as custom-built with dynamic stat calculation
#[derive(Component, Clone, Serialize, Deserialize)]
pub struct CustomModule {
    pub base_type: ModuleType,
    pub custom_name: String,
}

/// Sub-component that modifies parent module stats
#[derive(Component, Clone, Serialize, Deserialize)]
pub struct SubComponent {
    pub subcomponent_type: SubComponentType,
    pub parent_module: Entity,
}

/// Sub-component types for all module categories
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SubComponentType {
    // Weapons
    BarrelComponent { length: f32, caliber: f32, thickness: f32 },
    ChamberComponent { volume: f32, pressure: f32 },
    LoaderComponent { mechanism: LoaderMechanism, speed: f32 },
    MagazineComponent { capacity: u32 },

    // Engines
    CombustionChamber { efficiency: f32 },
    PropellerBlade { pitch: f32, count: u32 },
    FuelTank { capacity: f32 },

    // Reactors
    FuelRod { enrichment: f32, count: u32 },
    Coolant { flow_rate: f32 },
    Shielding { thickness: f32 },

    // Life Support
    OxygenScrubber { filter_size: f32 },
    CO2Absorber { efficiency: f32 },
}

/// Loader mechanism types for weapons
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum LoaderMechanism {
    Manual,
    Automatic,
    Rotary,
}

/// Calculated stats (cached, recalculated when ship-components change)
#[derive(Component, Clone, Debug, Default)]
pub struct CalculatedStats {
    pub weapon: Option<WeaponStats>,
    pub engine: Option<EngineStats>,
    pub reactor: Option<ReactorStats>,
    pub life_support: Option<LifeSupportStats>,
}

/// Calculated weapon stats from ship-components
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WeaponStats {
    pub damage: f32,
    pub range: f32,
    pub fire_rate: f32,
    pub max_ammo: u32,
    pub power_cost: f32,
}

/// Calculated engine stats from ship-components
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EngineStats {
    pub thrust: f32,
    pub fuel_efficiency: f32,
    pub noise: f32,
}

/// Calculated reactor stats from ship-components
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ReactorStats {
    pub power_output: f32,
    pub heat_generation: f32,
    pub explosion_risk: f32,
}

/// Calculated life support stats from ship-components
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LifeSupportStats {
    pub o2_generation: f32,
    pub co2_filtering: f32,
    pub crew_capacity: u32,
}

// ============================================================================
// ADVANCED COMPONENT PLACEMENT SYSTEM
// ============================================================================

/// Physical piece types that can be placed within a module's internal grid
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComponentPieceType {
    // Weapons
    Barrel,
    Chamber,
    Loader,
    Magazine,

    // Engines
    CombustionChamber,
    Propeller,
    FuelTank,
    CoolingSystem,

    // Reactors
    FuelRod,
    CoolantPipe,
    Shielding,
    ControlRod,

    // Life Support
    ScrubberFilter,
    CO2Absorber,
    AirCirculation,
}

impl ComponentPieceType {
    /// Get display name for this piece type
    pub fn name(&self) -> &'static str {
        match self {
            ComponentPieceType::Barrel => "Barrel",
            ComponentPieceType::Chamber => "Chamber",
            ComponentPieceType::Loader => "Loader",
            ComponentPieceType::Magazine => "Magazine",
            ComponentPieceType::CombustionChamber => "Combustion Chamber",
            ComponentPieceType::Propeller => "Propeller",
            ComponentPieceType::FuelTank => "Fuel Tank",
            ComponentPieceType::CoolingSystem => "Cooling System",
            ComponentPieceType::FuelRod => "Fuel Rod",
            ComponentPieceType::CoolantPipe => "Coolant Pipe",
            ComponentPieceType::Shielding => "Shielding",
            ComponentPieceType::ControlRod => "Control Rod",
            ComponentPieceType::ScrubberFilter => "Scrubber Filter",
            ComponentPieceType::CO2Absorber => "CO2 Absorber",
            ComponentPieceType::AirCirculation => "Air Circulation",
        }
    }

}

/// A physical component piece placed within a module's internal grid
#[derive(Component, Clone, Serialize, Deserialize)]
pub struct ComponentPiece {
    pub piece_type: ComponentPieceType,
    pub internal_position: IVec2,
    pub size: IVec2,
    pub properties: HashMap<String, f32>,
}

// ============================================================================
// CRISIS MANAGEMENT COMPONENTS
// ============================================================================

/// Marker: a BulkheadDoor hull segment that is sealed (blocks decompression/fire spread)
#[derive(Component)]
pub struct BulkheadSealed;

/// Tracks which room a crew member is currently in
#[derive(Component)]
pub struct CrewRoomLocation {
    pub room_id: Option<usize>,
    pub grid_position: IVec2,
}

// ============================================================================
// ENVIRONMENTAL HAZARD COMPONENTS
// ============================================================================

/// A zone that damages the ship or applies forces
#[derive(Component)]
pub struct HazardZone {
    pub hazard_type: HazardType,
    pub radius: f32,
    pub damage_per_second: f32,
}

/// Types of environmental hazards
#[derive(Clone, Debug)]
pub enum HazardType {
    ThermalVent,
    StrongCurrent(Vec2),
}

// ============================================================================
// UI COMPONENTS
// ============================================================================

/// Notification toast that fades out
#[derive(Component)]
pub struct NotificationToast {
    pub timer: Timer,
}

/// Marker for game over overlay
#[derive(Component)]
pub struct GameOverOverlay;

/// Marker for pause menu overlay
#[derive(Component)]
pub struct PauseMenuOverlay;

/// Marker for crew management overlay
#[derive(Component)]
pub struct CrewMenuOverlay;

/// Marker for main menu overlay
#[derive(Component)]
pub struct MainMenuOverlay;

/// Marker for module management panel overlay
#[derive(Component)]
pub struct ModulePanelOverlay;

/// Per-row in module panel, stores the module entity
#[derive(Component)]
pub struct ModuleListItem(pub Entity);

/// Selected index on the module panel root
#[derive(Component)]
pub struct ModuleListSelection(pub usize);

/// Ghost preview for building
#[derive(Component)]
pub struct BuildGhost;

/// Extra ghost tile for non-rectangular footprints — the main BuildGhost
/// sprite covers the module's first occupied cell, these cover the rest,
/// so the ghost never visually claims a cell the module doesn't actually occupy.
#[derive(Component)]
pub struct BuildGhostCell(pub usize);

/// Notification container
#[derive(Component)]
pub struct NotificationContainer;

/// Marker for docking/trading overlay
#[derive(Component)]
pub struct DockingOverlay;

/// Currently selected service in the docking menu
#[derive(Component)]
/// (selected service row, sell-cargo choice index — 0 = ALL, then one
/// per held item stack; see ui sell_choices)
pub struct DockingMenuSelection(pub usize, pub usize);

/// Individual service row in the docking menu
#[derive(Component)]
pub struct DockingServiceItem(pub usize);

/// Marker for upgrade shop overlay
#[derive(Component)]
pub struct UpgradeShopOverlay;

// ============================================================================
// ABYSSAL HORROR COMPONENTS
// ============================================================================

/// Marks a creature under abyssal influence (watching instead of hunting)
#[derive(Component)]
pub struct AbyssalInfluence {
    pub watching: bool,
    pub original_state: CreatureAIState,
}

/// Fake radar blip — not attached to any real creature
#[derive(Component)]
pub struct PhantomBlip {
    pub lifetime: Timer,
    pub drift: Vec2,
}

/// Marks creatures participating in a synchronized flee event
#[derive(Component)]
pub struct SynchronizedFlee {
    pub flee_direction: Vec2,
    pub duration: Timer,
}

/// Marks entities frozen by a time glitch
#[derive(Component)]
pub struct TimeGlitchFrozen {
    pub duration: Timer,
    pub saved_velocity: Vec2,
}

/// Currently selected item in the upgrade shop
#[derive(Component)]
pub struct UpgradeShopSelection(pub usize);

/// Individual upgrade row in the shop
#[derive(Component)]
pub struct UpgradeShopItem(pub usize);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_material_radiation_shielding_increases_with_tier() {
        let ratings = [
            HullMaterial::Steel.radiation_shielding(),
            HullMaterial::Titanium.radiation_shielding(),
            HullMaterial::Composite.radiation_shielding(),
            HullMaterial::AbyssalAlloy.radiation_shielding(),
        ];

        for i in 1..ratings.len() {
            assert!(ratings[i] > ratings[i - 1],
                "Higher tier hull material should have higher radiation shielding");
        }
    }

    #[test]
    fn hull_material_health_multipliers_increase_with_tier() {
        let multipliers = [
            HullMaterial::Steel.health_multiplier(),
            HullMaterial::Titanium.health_multiplier(),
            HullMaterial::Composite.health_multiplier(),
            HullMaterial::AbyssalAlloy.health_multiplier(),
        ];

        for i in 1..multipliers.len() {
            assert!(multipliers[i] > multipliers[i - 1],
                "Higher tier hull material should have higher health multiplier");
        }
    }

    #[test]
    fn hull_segment_default_uses_steel() {
        let hull = HullSegment::default();
        assert_eq!(hull.material, HullMaterial::Steel);
        assert!((hull.health - 100.0).abs() < f32::EPSILON);
        assert!((hull.max_health - 100.0).abs() < f32::EPSILON);
        assert!((hull.radiation_shielding - 300.0).abs() < f32::EPSILON);
        assert!(!hull.is_depressurized);
        assert!((hull.depressurization_level - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rotation_cycles_correctly() {
        let r = Rotation::North;
        let r = r.rotate_cw();
        assert_eq!(r, Rotation::East);
        let r = r.rotate_cw();
        assert_eq!(r, Rotation::South);
        let r = r.rotate_cw();
        assert_eq!(r, Rotation::West);
        let r = r.rotate_cw();
        assert_eq!(r, Rotation::North);
    }

    #[test]
    fn all_module_categories_have_types() {
        for cat in ModuleCategory::ALL {
            assert!(!cat.module_types().is_empty(),
                "Category {:?} should have at least one module type", cat);
        }
    }

    #[test]
    fn module_type_category_matches_category_module_types() {
        for cat in ModuleCategory::ALL {
            for mt in cat.module_types() {
                assert_eq!(mt.category(), *cat,
                    "ModuleType {:?} category should be {:?}", mt, cat);
            }
        }
    }

}
