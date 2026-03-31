use bevy::prelude::*;
use crate::components::*;
use crate::resources::{OxygenState, HullState};
use crate::events::{ShowNotification, NotificationType};
use crate::building::registry::{ModuleRegistry, CompanionData};
use crate::sprite_map;

/// Spawns the initial starter submarine (guards against duplicates)
/// Submarine-shaped hull with tapered bow, engines at stern, weapons on exterior
/// Proper submarine layout: bridge forward, engines aft, weapons on hull edges
pub fn spawn_starter_submarine(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut oxygen_state: ResMut<OxygenState>,
    mut hull_state: ResMut<HullState>,
    registry: Res<ModuleRegistry>,
    mut notifications: EventWriter<ShowNotification>,
    existing_sub: Query<Entity, With<Submarine>>,
) {
    // Guard: don't spawn a second submarine
    if !existing_sub.is_empty() {
        return;
    }
    info!("Spawning starter vessel...");

    // Initialize oxygen
    oxygen_state.max_oxygen = 1800.0;
    oxygen_state.current_oxygen = 1800.0;
    hull_state.hull_integrity = 1.0;

    // Spawn the main submarine entity (invisible anchor for movement)
    let submarine = commands.spawn((
        SpatialBundle {
            transform: Transform::from_xyz(0.0, -50.0, 0.0),
            ..default()
        },
        Submarine,
        Velocity(Vec2::ZERO),
        Depth(0.0),
        ThrusterState {
            base_drift: 0.0,
            current: 0.0,
        },
        Health {
            current: 150.0,
            max: 150.0,
        },
        SubmarinePhysics::default(),
        crate::celestial::components::GravityAffected { mass: 5000.0 },
        crate::celestial::components::GravityForce::default(),
    )).id();

    // ========================================================================
    // Submarine-shaped hull layout (tapered bow & stern)
    // Uses x as forward axis (positive = bow/front)
    //
    // Hull shape (top-down, y is vertical):
    //                                     [O][O]
    //                            [O][O][O][O][O][O]
    //                      [O][O][O][O][O][O][O][O][O]
    //          [O][O][O][O][O][O][O][O][O][O][O][O][O][O]
    //    [O][O][O][O][O][O][O][O][O][O][O][O][O][O][O][O]
    //    [O][O][O][O][O][O][O][O][O][O][O][O][O][O][O][O]
    //          [O][O][O][O][O][O][O][O][O][O][O][O][O][O]
    //                      [O][O][O][O][O][O][O][O][O]
    //                            [O][O][O][O][O][O]
    //                                     [O][O]
    //
    // Stern (x=-7..-5): Engines + propulsion (exposed at back)
    // Aft  (x=-4..-2):  Fuel, ballast, O2, cooling
    // Mid  (x=-1..2):   Reactors, crew quarters, mess hall
    // Fore (x=3..5):    Bridge, sonar, repair, cargo
    // Bow  (x=6..8):    Weapons on hull edges, sensors at tip
    // ========================================================================

    let hull_texture = asset_server.load(sprite_map::hull_sprite_path(HullMaterial::Steel));

    // Define the submarine shape - each row is (y, x_min, x_max)
    let hull_rows: &[(i32, i32, i32)] = &[
        ( 4,   5,  6),  // tip of bow (narrow)
        ( 3,   2,  7),  // upper bow
        ( 2,   0,  8),  // upper body
        ( 1,  -3,  8),  // wide upper
        ( 0,  -5,  8),  // widest (centerline)
        (-1,  -5,  8),  // widest (centerline)
        (-2,  -3,  8),  // wide lower
        (-3,   0,  8),  // lower body
        (-4,   2,  7),  // lower bow
        (-5,   5,  6),  // tip of bow (narrow)
    ];

    // Spawn hull segments for the submarine shape
    for &(y, x_min, x_max) in hull_rows {
        for x in x_min..=x_max {
            // Determine if this is perimeter or interior
            let is_top_edge = !hull_rows.iter().any(|&(ry, rxmin, rxmax)| ry == y + 1 && x >= rxmin && x <= rxmax);
            let is_bot_edge = !hull_rows.iter().any(|&(ry, rxmin, rxmax)| ry == y - 1 && x >= rxmin && x <= rxmax);
            let is_left_edge = x == x_min;
            let is_right_edge = x == x_max;
            let is_perimeter = is_top_edge || is_bot_edge || is_left_edge || is_right_edge;

            let hull_layer = if is_perimeter { HullLayer::Outer } else { HullLayer::Inner };
            let color = if is_perimeter {
                Color::rgb(0.55, 0.55, 0.6)
            } else {
                Color::rgb(0.35, 0.35, 0.4)
            };

            commands.spawn((
                SpriteBundle {
                    texture: hull_texture.clone(),
                    sprite: Sprite {
                        color,
                        custom_size: Some(Vec2::new(64.0, 64.0)),
                        ..default()
                    },
                    transform: Transform::from_xyz(
                        x as f32 * 66.0,
                        y as f32 * 66.0,
                        0.1,
                    ),
                    ..default()
                },
                HullSegment {
                    grid_position: IVec2::new(x, y),
                    hull_layer,
                    ..HullSegment::default()
                },
            )).set_parent(submarine);
        }
    }

    // --- Bulkhead doors (compartment separators) ---
    let bulkhead_positions = [
        IVec2::new(-2, 0), IVec2::new(-2, -1),  // aft ↔ mid
        IVec2::new(3, 0), IVec2::new(3, -1),    // mid ↔ fore
    ];
    for pos in &bulkhead_positions {
        commands.spawn((
            SpriteBundle {
                texture: hull_texture.clone(),
                sprite: Sprite {
                    color: Color::rgb(0.5, 0.55, 0.7),
                    custom_size: Some(Vec2::new(64.0, 64.0)),
                    ..default()
                },
                transform: Transform::from_xyz(
                    pos.x as f32 * 66.0,
                    pos.y as f32 * 66.0,
                    0.1,
                ),
                ..default()
            },
            HullSegment {
                grid_position: *pos,
                hull_layer: HullLayer::BulkheadDoor,
                ..HullSegment::default()
            },
        )).set_parent(submarine);
    }

    // ========================================================================
    // MODULES — proper submarine layout
    // Engines at stern, weapons on edges, crew protected inside
    // ========================================================================

    // --- STERN: Propulsion (x=-4..-3, inside hull at y=0,-1) ---
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::StandardEngine, IVec2::new(-4, 0), Rotation::West, &registry);
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::StandardEngine, IVec2::new(-4, -1), Rotation::West, &registry);
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::FuelTank, IVec2::new(-3, 0), Rotation::North, &registry);
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::BallastTank, IVec2::new(-3, -1), Rotation::North, &registry);

    // --- AFT: Life support + cooling (x=-2..-1) ---
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::OxygenScrubber, IVec2::new(-2, 0), Rotation::North, &registry);
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::OxygenScrubber, IVec2::new(-2, -1), Rotation::North, &registry);
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::CoolingPump, IVec2::new(-1, 0), Rotation::North, &registry);
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::HeatVent, IVec2::new(-1, -1), Rotation::North, &registry);

    // --- MID: Power + Crew (x=0..2) - ONE reactor only, well-cooled ---
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::StandardReactor, IVec2::new(0, 0), Rotation::North, &registry);
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::CoolingPump, IVec2::new(0, -1), Rotation::North, &registry);
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::BasicQuarters, IVec2::new(1, 1), Rotation::North, &registry);
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::BasicQuarters, IVec2::new(1, 0), Rotation::North, &registry);
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::MessHall, IVec2::new(1, -1), Rotation::North, &registry);
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::FuelTank, IVec2::new(1, -2), Rotation::North, &registry);

    spawn_module(&mut commands, &asset_server, submarine, ModuleType::RepairBay, IVec2::new(2, 0), Rotation::North, &registry);
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::SmallCargo, IVec2::new(2, -1), Rotation::North, &registry);

    // --- FORE: Bridge + detection (x=4..5) ---
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::HelmStation, IVec2::new(4, 0), Rotation::East, &registry);
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::SonarArray, IVec2::new(4, -1), Rotation::East, &registry);
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::Floodlight, IVec2::new(5, 0), Rotation::East, &registry);

    // --- BOW WEAPONS: On the hull edges (exterior-facing) ---
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::HeavyMissile, IVec2::new(6, 1), Rotation::East, &registry);
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::HeavyMissile, IVec2::new(6, -2), Rotation::East, &registry);
    spawn_module(&mut commands, &asset_server, submarine, ModuleType::Gatling, IVec2::new(3, 2), Rotation::North, &registry);

    info!("Starter vessel spawned! (24 modules)");

    notifications.send(ShowNotification {
        message: "WASD: Move | Q/E: Thrusters | Space: Fire | Z: Radar | V: Warp | B: Build | C: Crew".into(),
        notification_type: NotificationType::Info,
        duration: 8.0,
    });
}

/// Spawns a module entity using the registry for stats and companion components
pub fn spawn_module(
    commands: &mut Commands,
    asset_server: &AssetServer,
    parent: Entity,
    module_type: ModuleType,
    grid_pos: IVec2,
    rotation: Rotation,
    registry: &ModuleRegistry,
) -> Entity {
    let def = registry.get(module_type);

    // Calculate sprite size and center position for multi-cell modules
    let cells = crate::building::GridOccupancy::cells_for(grid_pos, def.size, rotation);
    let (min_x, max_x, min_y, max_y) = cells.iter().fold(
        (i32::MAX, i32::MIN, i32::MAX, i32::MIN),
        |(mnx, mxx, mny, mxy), c| (mnx.min(c.x), mxx.max(c.x), mny.min(c.y), mxy.max(c.y)),
    );
    let center_x = (min_x as f32 + max_x as f32) / 2.0 * 66.0;
    let center_y = (min_y as f32 + max_y as f32) / 2.0 * 66.0 - 33.0;
    let sprite_w = 60.0 + (max_x - min_x) as f32 * 66.0;
    let sprite_h = 60.0 + (max_y - min_y) as f32 * 66.0;

    let sprite_path = sprite_map::module_sprite_path(module_type)
        .unwrap_or("sprites/modules/small_reactor.png");
    let texture = asset_server.load(sprite_path);

    let visual_angle = rotation.to_radians() + sprite_map::sprite_base_rotation(module_type);

    let module_entity = commands.spawn((
        SpriteBundle {
            texture,
            sprite: Sprite {
                // Brighten module colors so they're visible over dark textures
                color: Color::rgb(
                    (def.color.r() * 1.5).min(1.0),
                    (def.color.g() * 1.5).min(1.0),
                    (def.color.b() * 1.5).min(1.0),
                ),
                custom_size: Some(Vec2::new(sprite_w, sprite_h)),
                ..default()
            },
            transform: Transform {
                translation: Vec3::new(center_x, center_y, 0.2),
                rotation: Quat::from_rotation_z(visual_angle),
                ..default()
            },
            ..default()
        },
        Module {
            module_type,
            health: def.health,
            max_health: def.health,
            power_consumption: def.power_consumption,
            power_generation: def.power_generation,
            // Essential modules start active, others start inactive to save power
            is_active: matches!(module_type.category(),
                ModuleCategory::Power | ModuleCategory::Propulsion | ModuleCategory::LifeSupport
            ) || matches!(module_type,
                ModuleType::HelmStation | ModuleType::BallastTank | ModuleType::CoolingPump
                | ModuleType::HeatVent | ModuleType::BasicQuarters | ModuleType::Barracks
                | ModuleType::Floodlight | ModuleType::RepairBay
            ),
            grid_position: grid_pos,
            size: def.size,
            rotation,
        },
        Selectable,
    )).id();

    insert_companion_components(commands, module_entity, &def.companion);

    // FirebreakWall gets a marker component for fire blocking
    if module_type == ModuleType::FirebreakWall {
        commands.entity(module_entity).insert(FirebreakMarker);
    }

    // Insert CrewStation if this module type requires one
    if def.crew_station {
        commands.entity(module_entity).insert(CrewStation {
            priority: 5,
            assigned_crew: None,
            manually_assigned: false,
        });
    }

    // Insert ModuleTemperature for heat network — defaults by category
    let (max_temp, conductivity) = match module_type.category() {
        ModuleCategory::Power => {
            // Reactors run hotter
            if matches!(module_type, ModuleType::SmallReactor | ModuleType::StandardReactor
                | ModuleType::LargeReactor | ModuleType::FusionReactor | ModuleType::RTG) {
                (100.0, 0.8)
            } else {
                (80.0, 0.5)
            }
        }
        ModuleCategory::Propulsion => (80.0, 0.6),
        ModuleCategory::Weapons => (60.0, 0.4),
        ModuleCategory::Structural => (200.0, 0.3),
        _ => {
            // CoolingPump/HeatVent are heat-resistant and highly conductive
            if matches!(module_type, ModuleType::CoolingPump | ModuleType::HeatVent) {
                (150.0, 1.0)
            } else {
                (80.0, 0.5)
            }
        }
    };
    commands.entity(module_entity).insert(ModuleTemperature {
        current: 0.0,
        max_temp,
        conductivity,
    });

    // Storage modules that are explosive
    match module_type {
        ModuleType::FuelTank => {
            commands.entity(module_entity).insert(Explosive {
                blast_radius: 2.0,
                blast_damage: 40.0,
                explosive_type: ExplosiveType::Fuel,
            });
        }
        ModuleType::AmmoBay => {
            commands.entity(module_entity).insert(Explosive {
                blast_radius: 2.0,
                blast_damage: 60.0,
                explosive_type: ExplosiveType::Ammo,
            });
        }
        ModuleType::BatteryBank => {
            commands.entity(module_entity).insert(Explosive {
                blast_radius: 1.0,
                blast_damage: 20.0,
                explosive_type: ExplosiveType::Battery,
            });
        }
        _ => {}
    }

    // Add ModuleCustomization for customizable weapons (Tier 2+3 support)
    if def.customizable && module_type.category() == ModuleCategory::Weapons {
        commands.entity(module_entity).insert(
            crate::building::customization::parameters::ModuleCustomization::default()
        );
    }

    // Add MachineBlock component for multi-block machines
    {
        use crate::building::multiblock::components::*;
        let machine_role = match module_type {
            // Weapon cores
            ModuleType::Cannon | ModuleType::Railgun | ModuleType::Coilgun |
            ModuleType::Gatling | ModuleType::Laser | ModuleType::PlasmaCaster |
            ModuleType::IonDisruptor | ModuleType::HeavyMissile | ModuleType::GuidedMissile |
            ModuleType::ClusterRocket | ModuleType::MiningDrill | ModuleType::TractorBeam |
            ModuleType::EMPPulse => Some((BlockRole::Core, true)),
            // Reactor cores
            ModuleType::SmallReactor | ModuleType::StandardReactor |
            ModuleType::LargeReactor | ModuleType::FusionReactor => Some((BlockRole::Core, true)),
            // Engine cores
            ModuleType::SmallEngine | ModuleType::StandardEngine |
            ModuleType::LargeEngine => Some((BlockRole::Core, true)),
            // Extension blocks
            ModuleType::BarrelExtension => Some((BlockRole::Barrel, false)),
            ModuleType::AmmoFeedUnit => Some((BlockRole::AmmoFeed, false)),
            ModuleType::CoolingJacket => Some((BlockRole::Cooling, false)),
            ModuleType::ReactorFuelRod => Some((BlockRole::FuelRod, false)),
            ModuleType::ReactorCooling => Some((BlockRole::Cooling, false)),
            ModuleType::EngineNozzle => Some((BlockRole::Nozzle, false)),
            ModuleType::ShieldEmitter => Some((BlockRole::ShieldEmitter, false)),
            _ => None,
        };

        if let Some((role, is_core)) = machine_role {
            commands.entity(module_entity).insert(MachineBlock {
                role,
                connected_core: if is_core { Some(module_entity) } else { None },
                chain_distance: 0,
                next_in_chain: None,
                prev_in_chain: None,
            });

            if is_core {
                commands.entity(module_entity).insert(MachineStats::default());
            }

            // Barrel blocks get stress tracking and cascade risk
            if role == BlockRole::Barrel {
                commands.entity(module_entity).insert(BarrelStress {
                    load: 1,
                    effective_cascade_chance: 0.15,
                });
                commands.entity(module_entity).insert(CascadeRisk::default());
            }
        }
    }

    commands.entity(module_entity).set_parent(parent);

    module_entity
}

/// Spawns a custom module with sub-components
pub fn spawn_custom_module(
    commands: &mut Commands,
    asset_server: &AssetServer,
    parent: Entity,
    base_type: ModuleType,
    custom_name: String,
    grid_pos: IVec2,
    rotation: Rotation,
    subcomponents: Vec<SubComponentType>,
    registry: &ModuleRegistry,
) -> Entity {
    // First spawn the base module using the standard function
    let module_entity = spawn_module(
        commands,
        asset_server,
        parent,
        base_type,
        grid_pos,
        rotation,
        registry,
    );

    // Add CustomModule component
    commands.entity(module_entity).insert(CustomModule {
        base_type,
        custom_name,
    });

    // Spawn sub-component entities as children
    for subcomponent_type in subcomponents {
        let subcomponent_entity = commands.spawn(SubComponent {
            subcomponent_type,
            parent_module: module_entity,
        }).id();

        commands.entity(subcomponent_entity).set_parent(module_entity);
    }

    // The recalculation system will automatically trigger due to Changed<Children>

    module_entity
}

/// Inserts companion components on a module entity based on registry data
fn insert_companion_components(commands: &mut Commands, entity: Entity, companion: &CompanionData) {
    match companion {
        CompanionData::None => {}
        CompanionData::Reactor { output, max_heat, explosion_risk } => {
            commands.entity(entity).insert(Reactor {
                output: *output,
                heat: 0.0,
                max_heat: *max_heat,
                explosion_risk: *explosion_risk,
            });
            if *explosion_risk {
                let blast_radius = if *output >= 200.0 { 2.5 }
                    else if *output >= 100.0 { 2.0 }
                    else { 1.5 };
                commands.entity(entity).insert(Explosive {
                    blast_radius,
                    blast_damage: *output * 0.5,
                    explosive_type: ExplosiveType::Reactor,
                });
            }
        }
        CompanionData::Engine { thrust, noise_level } => {
            commands.entity(entity).insert(Engine {
                thrust: *thrust,
                fuel_consumption: 1.0,
                noise_level: *noise_level,
            });
        }
        CompanionData::OxygenScrubber { output } => {
            commands.entity(entity).insert(OxygenScrubber {
                output: *output,
            });
        }
        CompanionData::LifeSupport { o2_gen, co2_filter } => {
            commands.entity(entity).insert(LifeSupportSystem {
                o2_generation: *o2_gen,
                co2_filtering: *co2_filter,
                water_recycling: 0.0,
            });
        }
        CompanionData::Thruster { thrust_power } => {
            commands.entity(entity).insert(Thruster {
                thrust_power: *thrust_power,
                current_output: 0.5,
            });
        }
        CompanionData::Cargo { capacity } => {
            commands.entity(entity).insert(CargoHold {
                capacity: *capacity,
                current_weight: 0.0,
            });
        }
        CompanionData::Weapon { damage, range, fire_rate, ammo, mount_type, ammo_type } => {
            commands.entity(entity).insert((
                Weapon {
                    damage: *damage,
                    range: *range,
                    fire_rate: *fire_rate,
                    ammo: *ammo,
                    max_ammo: *ammo,
                },
                WeaponCooldown {
                    timer: Timer::from_seconds(1.0 / fire_rate, TimerMode::Once),
                },
                WeaponMount {
                    mount_type: *mount_type,
                    firing_arc: match mount_type {
                        MountType::Fixed => 30.0,
                        MountType::Turret => 360.0,
                        MountType::Broadside => 180.0,
                    },
                },
                TargetingSystem {
                    tracking_speed: 1.0,
                    lock_on_time: 0.5,
                    max_targets: 1,
                },
                AmmoStorage {
                    ammo_type: *ammo_type,
                    capacity: *ammo * 2,
                    current: *ammo,
                },
            ));
            // Physical ammo weapons are explosive (not energy Charge)
            if matches!(ammo_type, AmmoType::Missile | AmmoType::Bullet | AmmoType::Mine) {
                let capped_ammo = (*ammo).min(10) as f32;
                commands.entity(entity).insert(Explosive {
                    blast_radius: 1.5,
                    blast_damage: *damage * 0.3 * capped_ammo,
                    explosive_type: ExplosiveType::Ammo,
                });
            }
        }
        CompanionData::Sonar { range, noise_on_ping } => {
            commands.entity(entity).insert(Sonar {
                range: *range,
                noise_on_ping: *noise_on_ping,
                is_pinging: false,
            });
        }
        CompanionData::PassiveSonar { range } => {
            commands.entity(entity).insert(Sonar {
                range: *range,
                noise_on_ping: 0.0,
                is_pinging: false,
            });
        }
        CompanionData::Detection { range } => {
            commands.entity(entity).insert(DetectionSystem {
                range: *range,
                is_passive: true,
                scan_interval: 2.0,
            });
        }
        CompanionData::Light { range, intensity, attracts_creatures } => {
            commands.entity(entity).insert(SubmarineLight {
                range: *range,
                intensity: *intensity,
                attracts_creatures: *attracts_creatures,
            });
        }
        CompanionData::Repair { rate } => {
            commands.entity(entity).insert(RepairSystem {
                repair_rate: *rate,
                hull_repair: true,
                module_repair: true,
            });
        }
        CompanionData::Navigation { map_range } => {
            commands.entity(entity).insert(NavigationComp {
                map_range: *map_range,
                autopilot: false,
            });
        }
        CompanionData::Docking => {
            commands.entity(entity).insert(DockingComp {
                docking_speed: 1.0,
            });
        }
        CompanionData::Salvage { range, efficiency } => {
            commands.entity(entity).insert(SalvageSystem {
                range: *range,
                efficiency: *efficiency,
            });
        }
        CompanionData::Quarters { berths } => {
            commands.entity(entity).insert(Quarters {
                berths: *berths,
            });
        }
        CompanionData::CrewFacility { facility_type } => {
            commands.entity(entity).insert(CrewFacility {
                facility_type: *facility_type,
            });
        }
        CompanionData::Capacitor { capacity, charge_rate } => {
            commands.entity(entity).insert(CapacitorComp {
                capacity: *capacity,
                charge: 0.0,
                charge_rate: *charge_rate,
            });
        }
        CompanionData::PowerConduit { throughput } => {
            commands.entity(entity).insert(PowerConduitComp {
                throughput: *throughput,
            });
        }
        CompanionData::FireSuppression { effectiveness } => {
            commands.entity(entity).insert(FireSuppressionComp {
                effectiveness: *effectiveness,
                active: true,
            });
        }
        CompanionData::RadiationShielding { shielding_bonus } => {
            commands.entity(entity).insert(RadiationShieldingComp {
                shielding_bonus: *shielding_bonus,
            });
        }
        CompanionData::DroneBay { drone_count, drone_range } => {
            commands.entity(entity).insert(DroneBayComp {
                drone_count: *drone_count,
                drone_range: *drone_range,
                drones_deployed: 0,
            });
        }
        CompanionData::CoolingPump { cooling_rate } => {
            commands.entity(entity).insert(CoolingPumpComp {
                cooling_rate: *cooling_rate,
            });
        }
        CompanionData::HeatVent { dissipation_rate } => {
            commands.entity(entity).insert(HeatVentComp {
                dissipation_rate: *dissipation_rate,
            });
        }
        CompanionData::Transformer { efficiency } => {
            commands.entity(entity).insert(TransformerComp {
                efficiency: *efficiency,
            });
        }
        CompanionData::OxygenTank { capacity } => {
            commands.entity(entity).insert(OxygenTankComp {
                capacity: *capacity,
                stored: *capacity,
            });
        }
        CompanionData::AmmoAutoloader { reload_bonus } => {
            commands.entity(entity).insert(AmmoAutoloaderComp {
                reload_bonus: *reload_bonus,
            });
        }
        CompanionData::ConveyorTube { speed } => {
            commands.entity(entity).insert(ConveyorTubeComp {
                speed: *speed,
            });
        }
        CompanionData::FuelProcessor { efficiency } => {
            commands.entity(entity).insert(FuelProcessorComp {
                efficiency: *efficiency,
            });
        }
        CompanionData::HullSeal { seal_rate } => {
            commands.entity(entity).insert(HullSealComp {
                seal_rate: *seal_rate,
            });
        }
        CompanionData::TargetingComputer { accuracy_bonus } => {
            commands.entity(entity).insert(TargetingComputerComp {
                accuracy_bonus: *accuracy_bonus,
            });
        }
        CompanionData::AICombatCore { priority_bonus } => {
            commands.entity(entity).insert(AICombatCoreComp {
                priority_bonus: *priority_bonus,
            });
        }
        CompanionData::ResearchLab { research_speed } => {
            commands.entity(entity).insert(ResearchLabComp {
                research_speed: *research_speed,
            });
        }
    }
}
