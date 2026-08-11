use crate::components::{ModuleType, CreatureType, HullMaterial, PoiType, DecorationType};

/// Maps ModuleType to sprite asset path. Returns None for unmapped types (colored rect fallback).
pub fn module_sprite_path(module_type: ModuleType) -> Option<&'static str> {
    Some(match module_type {
        // Power
        ModuleType::SmallReactor | ModuleType::StandardReactor => "sprites/modules/small_reactor.png",
        ModuleType::LargeReactor => "sprites/modules/large_reactor_2x1.png",
        ModuleType::BatteryBank => "sprites/modules/battery.png",
        ModuleType::RTG => "sprites/modules/small_reactor.png",
        ModuleType::FusionReactor => "sprites/modules/large_reactor_3x3.png",
        ModuleType::Capacitor => "sprites/modules/battery.png",
        ModuleType::PowerConduit => "sprites/modules/battery.png",
        ModuleType::SolarCell => "sprites/modules/small_reactor.png",
        // Propulsion
        ModuleType::SmallEngine | ModuleType::StandardEngine => "sprites/modules/standard_engine.png",
        ModuleType::LargeEngine => "sprites/modules/standard_engine_2x1.png",
        ModuleType::SilentDrive => "sprites/modules/silent_drive.png",
        ModuleType::ManeuveringThruster => "sprites/modules/standard_engine.png",
        ModuleType::JetDrive => "sprites/modules/standard_engine.png",
        ModuleType::EmergencyThruster => "sprites/modules/standard_engine.png",
        ModuleType::RudderAssembly => "sprites/modules/standard_engine.png",
        // Life Support
        ModuleType::OxygenScrubber => "sprites/modules/oxygen_scrubber.png",
        ModuleType::CO2Scrubber => "sprites/modules/life_support.png",
        ModuleType::WasteRecycler => "sprites/modules/life_support.png",
        ModuleType::AdvancedOxygenator => "sprites/modules/oxygen_scrubber_2x1.png",
        ModuleType::FireSuppression => "sprites/modules/life_support.png",
        ModuleType::AtmosphereMonitor => "sprites/modules/life_support.png",
        // Control
        ModuleType::NavigationConsole => "sprites/modules/navigation.png",
        ModuleType::HelmStation => "sprites/modules/navigation.png",
        // Weapons
        // Kinetic weapons
        ModuleType::Cannon => "sprites/modules/point_defense.png",
        ModuleType::Railgun => "sprites/modules/railgun_2x1.png",
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
        ModuleType::HydrophoneArray => "sprites/modules/passive_sonar_2x1.png",
        ModuleType::ThermalImager => "sprites/modules/depth_sensor.png",
        ModuleType::ProximityAlarm => "sprites/modules/depth_sensor.png",
        // Storage
        ModuleType::SmallCargo => "sprites/modules/cargo_hold.png",
        ModuleType::LargeCargo => "sprites/modules/cargo_hold_2x1.png",
        ModuleType::AmmoBay => "sprites/modules/cargo_hold.png",
        ModuleType::FuelTank => "sprites/modules/ballast_tank.png",
        ModuleType::SpecimenVault => "sprites/modules/research_lab.png",
        ModuleType::ReinforcedVault => "sprites/modules/cargo_hold.png",
        ModuleType::CryoStorage => "sprites/modules/research_lab.png",
        // Crew
        ModuleType::BasicQuarters => "sprites/modules/basic_quarters.png",
        ModuleType::Barracks => "sprites/modules/basic_quarters_2x1.png",
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
        ModuleType::AdvancedRepairBay => "sprites/modules/repair_station_2x1.png",
        ModuleType::DroneBay => "sprites/modules/repair_station_2x1.png",
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
        ModuleType::AICombatCore => "sprites/modules/navigation_2x1.png",
        ModuleType::ThermalVentGenerator => "sprites/modules/small_reactor.png",
        ModuleType::MineralExtractor => "sprites/modules/salvage_arm.png",
        ModuleType::CreatureContainment => "sprites/modules/research_lab_2x1.png",
        ModuleType::ResearchLab => "sprites/modules/research_lab_2x1.png",
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
        ModuleType::CornerArmorPlate => "sprites/modules/hull_beam_2x2.png",
        ModuleType::BridgeWing => "sprites/modules/navigation_3x2.png",
        ModuleType::SurgicalBay => "sprites/modules/medical_bay_3x2.png",
        ModuleType::GalleyMess => "sprites/modules/basic_quarters_2x2.png",
        ModuleType::BulkCargoHold => "sprites/modules/cargo_hold_2x2.png",
        ModuleType::DockingHub => "sprites/modules/docking_port_3x3.png",
        ModuleType::WellnessHub => "sprites/modules/basic_quarters_3x3.png",
        ModuleType::StaggeredArmorPlate => "sprites/modules/hull_beam_3x2.png",
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
/// Barrel-type weapons whose sprite is drawn long (1:3, turret centered,
/// barrel pointing "up"/North) so the barrel overhangs into the cells ahead.
/// spawner.rs sizes these tall and renders them on a higher z-layer so the
/// barrel draws over neighbouring blocks (Cosmoteer-style). Missile tubes,
/// mine layers and beam-only weapons are NOT barrel weapons.
pub fn is_barrel_weapon(module_type: ModuleType) -> bool {
    matches!(module_type,
        ModuleType::Cannon | ModuleType::Gatling | ModuleType::Coilgun | ModuleType::Railgun
        | ModuleType::MiningDrill | ModuleType::MagneticAccelerator | ModuleType::PointDefenseDrone)
}

pub fn sprite_base_rotation(module_type: ModuleType) -> f32 {
    use crate::components::ModuleCategory;
    // Base offset that aligns the sprite's native "facing" with the module's
    // placement Rotation. Derived from the two authored cases in the starter
    // ship (spawner.rs):
    //   • Engine art draws its exhaust pointing DOWN (South). Engines are
    //     placed Rotation::West (to_radians = +π/2) and must vent West (aft).
    //     South → West needs visual_angle = -π/2, so base = -π/2 - π/2 = -π.
    //   • Weapon art draws its muzzle pointing UP (North). Weapons are placed
    //     Rotation::East (to_radians = -π/2) and must fire East (forward).
    //     North → East needs visual_angle = -π/2, so base = 0.
    // Net: placement Rotation reads as the exhaust direction for engines and
    // the muzzle direction for weapons — the intuitive result at any angle.
    let is_engine = matches!(module_type.category(), ModuleCategory::Propulsion);
    let is_weapon = is_barrel_weapon(module_type)
        || matches!(module_type,
            ModuleType::Laser | ModuleType::PlasmaCaster | ModuleType::IonDisruptor
            | ModuleType::EMPPulse | ModuleType::HeavyMissile | ModuleType::GuidedMissile
            | ModuleType::ClusterRocket | ModuleType::TractorBeam | ModuleType::SalvageArm);
    if is_engine {
        -std::f32::consts::PI
    } else if is_weapon {
        0.0
    } else {
        0.0
    }
}

/// How far a module's working end protrudes PAST its block, and which way.
/// Returns `(extension_world_units, protrude_sign)` where the sign is +1 when
/// the art's protruding end is the TOP (art +Y — gun barrels) and -1 when it's
/// the BOTTOM (art −Y — engine nozzles). The extension lengthens the sprite
/// along its local vertical axis and is offset so the housing stays centred on
/// the cell while the barrel/nozzle hangs off the end (rendered over the
/// neighbour via a raised Z, see spawner).
///
/// Tied to the specific sprite FILES whose art has actually been drawn with an
/// extended canvas — a module gets overhang only once its sprite includes the
/// protruding region, otherwise the square art would just stretch.
pub fn sprite_overhang(module_type: ModuleType) -> (f32, f32) {
    match module_sprite_path(module_type) {
        // Engines: nozzle vents aft (art bottom, −1). 1×1 sprites only — the
        // art aspect matches the cell exactly there, so the extension doesn't
        // distort (multi-cell overhang needs the base-size fix first).
        Some("sprites/modules/standard_engine.png") => (30.0, -1.0),
        Some("sprites/modules/silent_drive.png") => (24.0, -1.0),
        // Weapons: barrel/arm leads forward (art top, +1).
        // point_defense / railgun are turrets now — their barrels are separate
        // rotating sprites (see turret_barrel_sprite), so the base has no overhang.
        Some("sprites/modules/torpedo_tube.png") => (26.0, 1.0),
        Some("sprites/modules/salvage_arm.png") => (34.0, 1.0),
        _ => (0.0, 0.0),
    }
}

/// The rotating barrel sprite for a gun turret, if this module is one. The base
/// mount stays static (the module's own sprite); this sprite is spawned as a
/// pivot-centred child the turret aim system rotates to point at the target.
pub fn turret_barrel_sprite(module_type: ModuleType) -> Option<&'static str> {
    match module_sprite_path(module_type) {
        Some("sprites/modules/point_defense.png") => Some("sprites/modules/turret_pd_barrel.png"),
        Some("sprites/modules/railgun.png") => Some("sprites/modules/turret_rg_barrel.png"),
        _ => None,
    }
}

/// Base traverse speed (radians/sec) for a turret weapon — the "how fast it
/// tracks" knob, per weapon. Light autocannons whip around; the railgun is
/// ponderous. Per-weapon customization scales this (WeaponTuning::traverse).
pub fn turret_turn_speed(module_type: ModuleType) -> f32 {
    match module_type {
        ModuleType::Gatling | ModuleType::PointDefenseDrone => 6.5,
        ModuleType::Cannon => 3.2,
        ModuleType::Coilgun | ModuleType::Laser | ModuleType::PlasmaCaster
        | ModuleType::IonDisruptor | ModuleType::EMPPulse => 2.6,
        ModuleType::Railgun | ModuleType::MagneticAccelerator => 1.5,
        ModuleType::MiningDrill => 2.0,
        _ => 3.0,
    }
}
