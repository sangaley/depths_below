use crate::components::{ModuleType, CreatureType, HullMaterial, AmbientKind, PoiType, DecorationType};

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
        ModuleType::WaterRecycler => "sprites/modules/life_support.png",
        ModuleType::AdvancedOxygenator => "sprites/modules/oxygen_scrubber.png",
        ModuleType::FireSuppression => "sprites/modules/life_support.png",
        ModuleType::AtmosphereMonitor => "sprites/modules/life_support.png",
        // Control
        ModuleType::NavigationConsole => "sprites/modules/navigation.png",
        ModuleType::HelmStation => "sprites/modules/navigation.png",
        // Weapons
        ModuleType::TorpedoTube | ModuleType::HeavyTorpedoTube => "sprites/modules/torpedo_tube.png",
        ModuleType::PointDefense => "sprites/modules/point_defense.png",
        ModuleType::ElectricDischarger => "sprites/modules/railgun.png",
        ModuleType::SonicPulse => "sprites/modules/sonar_array.png",
        ModuleType::MineLayer => "sprites/modules/mine_layer.png",
        ModuleType::RailGun => "sprites/modules/railgun.png",
        ModuleType::FlakCannon => "sprites/modules/point_defense.png",
        ModuleType::NetLauncher => "sprites/modules/mine_layer.png",
        ModuleType::AcidSprayer => "sprites/modules/railgun.png",
        ModuleType::EMPEmitter => "sprites/modules/railgun.png",
        // Detection
        ModuleType::SonarArray | ModuleType::AdvancedSonar => "sprites/modules/sonar_array.png",
        ModuleType::PassiveSonar => "sprites/modules/passive_sonar.png",
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
        ModuleType::BallastTank => "sprites/modules/ballast_tank.png",
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
        ModuleType::FloodValve => "sprites/modules/docking_port.png",
        ModuleType::AccessHatch => "sprites/modules/docking_port.png",
        ModuleType::ViewPort => "sprites/modules/floodlight.png",
        ModuleType::ArmorPlate => "sprites/modules/hull_beam.png",
        // New blocks — placeholder sprites
        ModuleType::CoolingPump => "sprites/modules/repair_station.png",
        ModuleType::HeatVent => "sprites/modules/hull_beam.png",
        ModuleType::Transformer => "sprites/modules/battery.png",
        ModuleType::VectorThruster => "sprites/modules/standard_engine.png",
        ModuleType::TrimTank => "sprites/modules/ballast_tank.png",
        ModuleType::OxygenTank => "sprites/modules/oxygen_scrubber.png",
        ModuleType::AirCirculator => "sprites/modules/life_support.png",
        ModuleType::CreatureScanner => "sprites/modules/depth_sensor.png",
        ModuleType::MineralScanner => "sprites/modules/depth_sensor.png",
        ModuleType::TorpedoLoader => "sprites/modules/cargo_hold.png",
        ModuleType::EngineeringStation => "sprites/modules/repair_station.png",
        // Phase B modules
        ModuleType::ConveyorTube => "sprites/modules/battery.png",
        ModuleType::MaintenanceLocker => "sprites/modules/repair_station.png",
        ModuleType::FuelProcessor => "sprites/modules/ballast_tank.png",
        ModuleType::WaterPump => "sprites/modules/ballast_tank.png",
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
        CreatureType::Scavenger => "sprites/creatures/hostile/scavenger.png",
        CreatureType::Stalker => "sprites/creatures/hostile/stalker.png",
        CreatureType::Ambusher => "sprites/creatures/hostile/ambusher.png",
        CreatureType::ElectricEel => "sprites/creatures/hostile/electric_eel.png",
        CreatureType::BlindHunter => "sprites/creatures/hostile/blind_hunter.png",
        CreatureType::LureFish => "sprites/creatures/hostile/lure_fish.png",
        CreatureType::SwarmQueen => "sprites/creatures/hostile/swarm_queen.png",
        CreatureType::Leviathan => "sprites/creatures/hostile/leviathan.png",
        CreatureType::Parasite => "sprites/creatures/hostile/parasite.png",
        CreatureType::Watcher => "sprites/creatures/hostile/watcher.png",
    }
}

pub fn ambient_sprite_path(kind: AmbientKind) -> &'static str {
    match kind {
        AmbientKind::SmallFish => "sprites/creatures/ambient/small_fish.png",
        AmbientKind::Jellyfish => "sprites/creatures/ambient/jellyfish.png",
        AmbientKind::SchoolFish => "sprites/creatures/ambient/school_fish.png",
        AmbientKind::DeepFish => "sprites/creatures/ambient/deep_fish.png",
        AmbientKind::GiantSquid => "sprites/creatures/ambient/giant_squid.png",
        AmbientKind::Whale => "sprites/creatures/ambient/whale.png",
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
        DecorationType::Algae => Some("sprites/environment/kelp.png"),
        DecorationType::Coral => Some("sprites/environment/coral.png"),
        DecorationType::BioluminescentSpot => Some("sprites/environment/bioluminescent_spot.png"),
        DecorationType::ThermalVentSmoke => None, // No sprite — keep as colored rect for smoke effect
        DecorationType::SandMound => Some("sprites/environment/sand_mound.png"),
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
        "sonar_ring" => "sprites/effects/sonar_ring.png",
        _ => "sprites/effects/torpedo_trail.png",
    }
}

/// Per-module sprite base rotation offset in radians.
/// Most sprites are drawn facing "up" (North = 0). Modules whose sprite
/// has a different natural orientation need an offset so that rotation
/// math works correctly.
pub fn sprite_base_rotation(module_type: ModuleType) -> f32 {
    match module_type {
        // Torpedo tubes are drawn pointing right (East), offset by -π/2
        ModuleType::TorpedoTube | ModuleType::HeavyTorpedoTube
        | ModuleType::RailGun | ModuleType::NetLauncher => -std::f32::consts::FRAC_PI_2,
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
