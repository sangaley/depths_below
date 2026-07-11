use crate::components::{ModuleType, CreatureType, HullMaterial, PoiType, DecorationType};

/// Maps ModuleType to sprite asset path. Returns None for unmapped types (colored rect fallback).
pub fn module_sprite_path(module_type: ModuleType) -> Option<&'static str> {
    Some(match module_type {
        // Power
        ModuleType::SmallReactor | ModuleType::StandardReactor => "sprites/modules/small_reactor.png",
        ModuleType::LargeReactor => "sprites/modules/large_reactor.png",
        ModuleType::BatteryBank => "sprites/modules/battery.png",
        ModuleType::RTG => "sprites/modules/small_reactor.png",
        ModuleType::FusionReactor => "sprites/modules/large_reactor.png",
        ModuleType::Capacitor => "sprites/modules/battery.png",
        ModuleType::PowerConduit => "sprites/modules/battery.png",
        ModuleType::SolarCell => "sprites/modules/small_reactor.png",
        // Propulsion
        ModuleType::SmallEngine | ModuleType::StandardEngine | ModuleType::LargeEngine => "sprites/modules/standard_engine.png",
        ModuleType::SilentDrive => "sprites/modules/silent_drive.png",
        ModuleType::ManeuveringThruster => "sprites/modules/standard_engine.png",
        ModuleType::JetDrive => "sprites/modules/standard_engine.png",
        ModuleType::EmergencyThruster => "sprites/modules/standard_engine.png",
        ModuleType::RudderAssembly => "sprites/modules/standard_engine.png",
        // Life Support
        ModuleType::OxygenScrubber => "sprites/modules/oxygen_scrubber.png",
        ModuleType::CO2Scrubber => "sprites/modules/life_support.png",
        ModuleType::WasteRecycler => "sprites/modules/life_support.png",
        ModuleType::AdvancedOxygenator => "sprites/modules/oxygen_scrubber.png",
        ModuleType::FireSuppression => "sprites/modules/life_support.png",
        ModuleType::AtmosphereMonitor => "sprites/modules/life_support.png",
        // Control
        ModuleType::NavigationConsole => "sprites/modules/navigation.png",
        ModuleType::HelmStation => "sprites/modules/navigation.png",
        // Weapons
        // Kinetic weapons
        ModuleType::Cannon => "sprites/modules/point_defense.png",
        ModuleType::Railgun => "sprites/modules/railgun.png",
        ModuleType::Coilgun => "sprites/modules/railgun.png",
        ModuleType::Gatling => "sprites/modules/point_defense.png",
        // Energy weapons
        ModuleType::Laser => "sprites/modules/railgun.png",
        ModuleType::PlasmaCaster => "sprites/modules/railgun.png",
        ModuleType::IonDisruptor => "sprites/modules/railgun.png",
        // Missile weapons
        ModuleType::HeavyMissile => "sprites/modules/torpedo_tube.png",
        ModuleType::GuidedMissile => "sprites/modules/torpedo_tube.png",
        ModuleType::ClusterRocket => "sprites/modules/mine_layer.png",
        // Utility weapons
        ModuleType::MiningDrill => "sprites/modules/railgun.png",
        ModuleType::TractorBeam => "sprites/modules/mine_layer.png",
        ModuleType::EMPPulse => "sprites/modules/railgun.png",
        // Detection
        ModuleType::RadarArray | ModuleType::AdvancedRadar => "sprites/modules/sonar_array.png",
        ModuleType::PassiveRadar => "sprites/modules/passive_sonar.png",
        ModuleType::DepthScanner => "sprites/modules/depth_sensor.png",
        ModuleType::HydrophoneArray => "sprites/modules/passive_sonar.png",
        ModuleType::ThermalImager => "sprites/modules/depth_sensor.png",
        ModuleType::ProximityAlarm => "sprites/modules/depth_sensor.png",
        // Storage
        ModuleType::SmallCargo | ModuleType::LargeCargo => "sprites/modules/cargo_hold.png",
        ModuleType::AmmoBay => "sprites/modules/cargo_hold.png",
        ModuleType::FuelTank => "sprites/modules/ballast_tank.png",
        ModuleType::SpecimenVault => "sprites/modules/research_lab.png",
        ModuleType::ReinforcedVault => "sprites/modules/cargo_hold.png",
        ModuleType::CryoStorage => "sprites/modules/research_lab.png",
        // Crew
        ModuleType::BasicQuarters | ModuleType::Barracks => "sprites/modules/basic_quarters.png",
        ModuleType::MedBay => "sprites/modules/medical_bay.png",
        ModuleType::MessHall => "sprites/modules/basic_quarters.png",
        ModuleType::RecRoom => "sprites/modules/basic_quarters.png",
        ModuleType::OfficerQuarters => "sprites/modules/basic_quarters.png",
        ModuleType::TrainingRoom => "sprites/modules/basic_quarters.png",
        ModuleType::Brig => "sprites/modules/basic_quarters.png",
        // Utility
        ModuleType::RepairBay => "sprites/modules/repair_station.png",
        ModuleType::ManeuverThruster => "sprites/modules/ballast_tank.png",
        ModuleType::Floodlight | ModuleType::Searchlight => "sprites/modules/floodlight.png",
        ModuleType::AirlockChamber => "sprites/modules/docking_port.png",
        ModuleType::DockingPort => "sprites/modules/docking_port.png",
        ModuleType::SalvageArm => "sprites/modules/salvage_arm.png",
        ModuleType::AdvancedRepairBay => "sprites/modules/repair_station.png",
        ModuleType::DroneBay => "sprites/modules/repair_station.png",
        ModuleType::DeepFloodlight => "sprites/modules/floodlight.png",
        ModuleType::StealthCoating => "sprites/modules/silent_drive.png",
        ModuleType::HullPatch => "sprites/modules/repair_station.png",
        ModuleType::SignalBuoy => "sprites/modules/floodlight.png",
        // Structural
        ModuleType::HullBeam => "sprites/modules/hull_beam.png",
        ModuleType::HullCorner => "sprites/modules/hull_beam.png",
        ModuleType::Bulkhead => "sprites/modules/hull_beam.png",
        ModuleType::PressureFrame => "sprites/modules/hull_beam.png",
        ModuleType::AirlockValve => "sprites/modules/docking_port.png",
        ModuleType::AccessHatch => "sprites/modules/docking_port.png",
        ModuleType::ViewPort => "sprites/modules/floodlight.png",
        ModuleType::ArmorPlate => "sprites/modules/hull_beam.png",
        // New blocks — placeholder sprites
        ModuleType::CoolingPump => "sprites/modules/repair_station.png",
        ModuleType::HeatVent => "sprites/modules/hull_beam.png",
        ModuleType::Transformer => "sprites/modules/battery.png",
        ModuleType::VectorThruster => "sprites/modules/standard_engine.png",
        ModuleType::AttitudeThruster => "sprites/modules/ballast_tank.png",
        ModuleType::OxygenTank => "sprites/modules/oxygen_scrubber.png",
        ModuleType::AirCirculator => "sprites/modules/life_support.png",
        ModuleType::CreatureScanner => "sprites/modules/depth_sensor.png",
        ModuleType::MineralScanner => "sprites/modules/depth_sensor.png",
        ModuleType::AmmoAutoloader => "sprites/modules/cargo_hold.png",
        ModuleType::EngineeringStation => "sprites/modules/repair_station.png",
        // Phase B modules
        ModuleType::ConveyorTube => "sprites/modules/battery.png",
        ModuleType::MaintenanceLocker => "sprites/modules/repair_station.png",
        ModuleType::FuelProcessor => "sprites/modules/ballast_tank.png",
        ModuleType::HullSealer => "sprites/modules/ballast_tank.png",
        ModuleType::EmergencyBulkhead => "sprites/modules/hull_beam.png",
        ModuleType::FirebreakWall => "sprites/modules/hull_beam.png",
        ModuleType::PressureSensor => "sprites/modules/depth_sensor.png",
        ModuleType::TargetingComputer => "sprites/modules/navigation.png",
        ModuleType::AutopilotCore => "sprites/modules/navigation.png",
        ModuleType::AICombatCore => "sprites/modules/navigation.png",
        ModuleType::ThermalVentGenerator => "sprites/modules/small_reactor.png",
        ModuleType::MineralExtractor => "sprites/modules/salvage_arm.png",
        ModuleType::CreatureContainment => "sprites/modules/research_lab.png",
        ModuleType::ResearchLab => "sprites/modules/repair_station.png",
        ModuleType::Corridor => "sprites/modules/hull_beam.png",
        ModuleType::LadderShaft => "sprites/modules/docking_port.png",
        ModuleType::MaintenanceTunnel => "sprites/modules/hull_beam.png",
        // Multi-block extension blocks (reuse existing sprites for now)
        ModuleType::BarrelExtension => "sprites/modules/hull_beam.png",
        ModuleType::AmmoFeedUnit => "sprites/modules/cargo_hold.png",
        ModuleType::CoolingJacket => "sprites/modules/repair_station.png",
        ModuleType::ReactorFuelRod => "sprites/modules/small_reactor.png",
        ModuleType::ReactorCooling => "sprites/modules/repair_station.png",
        ModuleType::EngineNozzle => "sprites/modules/standard_engine.png",
        ModuleType::ShieldEmitter => "sprites/modules/battery.png",
        // Advanced weapon enhancers
        ModuleType::MuzzleBrake => "sprites/modules/hull_beam.png",
        ModuleType::RecoilAbsorber => "sprites/modules/hull_beam.png",
        ModuleType::OverchargeCapacitor => "sprites/modules/battery.png",
        ModuleType::BoreEvacuator => "sprites/modules/hull_beam.png",
        ModuleType::MagneticAccelerator => "sprites/modules/railgun.png",
        ModuleType::FocusingArray => "sprites/modules/depth_sensor.png",
        ModuleType::WarheadBay => "sprites/modules/cargo_hold.png",
        // Advanced reactor enhancers
        ModuleType::FuelEnrichmentUnit => "sprites/modules/small_reactor.png",
        ModuleType::ContainmentField => "sprites/modules/battery.png",
        ModuleType::EmergencyShutdown => "sprites/modules/battery.png",
        ModuleType::PowerRegulator => "sprites/modules/battery.png",
        // Advanced engine enhancers
        ModuleType::Afterburner => "sprites/modules/standard_engine.png",
        ModuleType::ThrustVectoring => "sprites/modules/standard_engine.png",
        ModuleType::FuelInjector => "sprites/modules/ballast_tank.png",
        ModuleType::InertialDampener => "sprites/modules/battery.png",
        // Defense modules
        ModuleType::DecoyLauncher => "sprites/modules/mine_layer.png",
        ModuleType::ChaffDispenser => "sprites/modules/mine_layer.png",
        ModuleType::AblativeArmor => "sprites/modules/hull_beam.png",
        ModuleType::PointDefenseDrone => "sprites/modules/point_defense.png",
        ModuleType::HullReinforcePlate => "sprites/modules/hull_beam.png",
        // Advanced utility
        ModuleType::SignalJammer => "sprites/modules/sonar_array.png",
        ModuleType::GravityCompensator => "sprites/modules/battery.png",
        ModuleType::RadiationHardening => "sprites/modules/hull_beam.png",
        ModuleType::EmergencyO2Cache => "sprites/modules/oxygen_scrubber.png",
        ModuleType::BlackBox => "sprites/modules/navigation.png",
        // Structural enhancers
        ModuleType::ReinforcedJoint => "sprites/modules/hull_beam.png",
        ModuleType::VibrationDamper => "sprites/modules/hull_beam.png",
        ModuleType::ThermalInsulator => "sprites/modules/hull_beam.png",
        ModuleType::StructuralBrace => "sprites/modules/hull_beam.png",
        ModuleType::CornerArmorPlate => "sprites/modules/hull_beam.png",
        ModuleType::BridgeWing => "sprites/modules/navigation.png",
        ModuleType::SurgicalBay => "sprites/modules/medical_bay.png",
        ModuleType::GalleyMess => "sprites/modules/basic_quarters.png",
        ModuleType::BulkCargoHold => "sprites/modules/cargo_hold.png",
        ModuleType::DockingHub => "sprites/modules/docking_port.png",
        ModuleType::WellnessHub => "sprites/modules/basic_quarters.png",
        ModuleType::StaggeredArmorPlate => "sprites/modules/hull_beam.png",
        ModuleType::AngledHullPlate => "sprites/modules/hull_beam.png",
        ModuleType::AngledArmorPlate => "sprites/modules/hull_beam.png",
    })
}

pub fn hull_sprite_path(material: HullMaterial) -> &'static str {
    match material {
        HullMaterial::Steel => "sprites/hull/hull_steel.png",
        HullMaterial::Titanium => "sprites/hull/hull_titanium.png",
        HullMaterial::Composite => "sprites/hull/hull_composite.png",
        HullMaterial::AbyssalAlloy => "sprites/hull/hull_abyssal.png",
    }
}

pub fn creature_sprite_path(creature_type: CreatureType) -> &'static str {
    match creature_type {
        CreatureType::VoidDrifter => "sprites/creatures/ambient/void_drifter.png",
        CreatureType::Stalker => "sprites/creatures/hostile/stalker.png",
        CreatureType::Leviathan => "sprites/creatures/hostile/leviathan.png",
        CreatureType::ParasiteSwarm => "sprites/creatures/hostile/parasite.png",
    }
}

pub fn poi_sprite_path(poi_type: PoiType) -> &'static str {
    match poi_type {
        PoiType::Wreck => "sprites/environment/wreck.png",
        PoiType::Cave => "sprites/environment/cave.png",
        PoiType::Ruins => "sprites/environment/ruins.png",
        PoiType::ThermalVent => "sprites/environment/thermal_vent.png",
        PoiType::Settlement => "sprites/environment/settlement.png",
    }
}

pub fn decoration_sprite_path(decoration_type: DecorationType) -> Option<&'static str> {
    match decoration_type {
        DecorationType::Rock => Some("sprites/environment/rock.png"),
        DecorationType::SporeGrowth => Some("sprites/environment/spore_growth.png"),
        DecorationType::Crystal => Some("sprites/environment/crystal_formation.png"),
        DecorationType::EnergySpot => Some("sprites/environment/bioluminescent_spot.png"),
        DecorationType::ThermalVentSmoke => None, // No sprite — keep as colored rect for smoke effect
        DecorationType::RockDebris => Some("sprites/environment/rock_debris.png"),
    }
}

/// Effect sprite paths for combat visuals
pub fn effect_sprite_path(effect: &str) -> &'static str {
    match effect {
        "torpedo" => "sprites/effects/torpedo_trail.png",
        "enemy_projectile" => "sprites/effects/enemy_projectile.png",
        "bubble" => "sprites/effects/bubble.png",
        "electric_shock" => "sprites/effects/electric_shock.png",
        "explosion" => "sprites/effects/explosion.png",
        "radar_ring" => "sprites/effects/sonar_ring.png",
        _ => "sprites/effects/torpedo_trail.png",
    }
}

/// Per-module sprite base rotation offset in radians.
/// Most sprites are drawn facing "up" (North = 0). Modules whose sprite
/// has a different natural orientation need an offset so that rotation
/// math works correctly.
pub fn sprite_base_rotation(module_type: ModuleType) -> f32 {
    match module_type {
        // Forward-firing weapons are drawn pointing right (East), offset by -π/2
        ModuleType::HeavyMissile | ModuleType::Railgun | ModuleType::Cannon
        | ModuleType::GuidedMissile | ModuleType::ClusterRocket
        | ModuleType::TractorBeam | ModuleType::MiningDrill => -std::f32::consts::FRAC_PI_2,
        // Engines are drawn pointing right (East)
        ModuleType::SmallEngine | ModuleType::StandardEngine | ModuleType::LargeEngine
        | ModuleType::SilentDrive | ModuleType::ManeuveringThruster
        | ModuleType::JetDrive | ModuleType::EmergencyThruster
        | ModuleType::RudderAssembly
        | ModuleType::VectorThruster => -std::f32::consts::FRAC_PI_2,
        // Salvage arm drawn pointing right
        ModuleType::SalvageArm => -std::f32::consts::FRAC_PI_2,
        _ => 0.0,
    }
}
