use bevy::prelude::*;
use crate::components::*;
use crate::resources::{OxygenState, HullState};
use crate::events::{ShowNotification, NotificationType};
use crate::building::registry::{ModuleRegistry, CompanionData};
use crate::sprite_map;

/// Spawns the initial starter ship (guards against duplicates)
/// Ship-shaped hull with tapered bow, engines at stern, weapons on exterior
/// Proper ship layout: bridge forward, engines aft, weapons on hull edges
pub fn spawn_starter_ship(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut oxygen_state: ResMut<OxygenState>,
    mut hull_state: ResMut<HullState>,
    registry: Res<ModuleRegistry>,
    mut notifications: MessageWriter<ShowNotification>,
    existing_ship: Query<Entity, With<Ship>>,
    mut rebuild_queue: ResMut<crate::ship::rebuild::RebuildQueue>,
) {
    // Guard: don't spawn a second ship
    if !existing_ship.is_empty() {
        return;
    }
    info!("Spawning starter vessel...");

    // Fresh run — stale ghosts belonged to the previous ship (their
    // sprites died with it as children of the old root).
    rebuild_queue.ghosts.clear();

    // Initialize oxygen
    oxygen_state.max_oxygen = 1800.0;
    oxygen_state.current_oxygen = 1800.0;
    hull_state.hull_integrity = 1.0;

    // Spawn the main ship entity (invisible anchor for movement)
    let ship = commands.spawn((
        Transform::from_xyz(0.0, -50.0, 0.0),
        Ship,
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
        ShipPhysics::default(),
        crate::celestial::components::GravityAffected { mass: 5000.0 },
        crate::celestial::components::GravityForce::default(),
    )).id();

    // The starter destroyer is design data, not spawn calls — see
    // builtin_starter_design(). designs/starter.json overrides the built-in
    // (exported there on first run so it can be edited as JSON).
    let design = crate::building::blueprint::load_design_file("designs/starter.json")
        .unwrap_or_else(|| {
            let design = builtin_starter_design();
            if let Err(e) = crate::building::blueprint::write_design_file("designs/starter.json", &design) {
                warn!("Could not export starter design: {}", e);
            }
            design
        });

    crate::building::blueprint::spawn_ship_from_design(
        &mut commands,
        &asset_server,
        &registry,
        ship,
        &design,
    );

    info!(
        "Starter vessel '{}' spawned ({} hull, {} modules)",
        design.name,
        design.hull_cells.len(),
        design.modules.len()
    );

    notifications.write(ShowNotification {
        message: "Mouse: Aim | W/S: Thrust | A/D: Strafe | Shift: Brake | Space/Click: Fire | R: Shield | F: Dock".into(),
        notification_type: NotificationType::Info,
        duration: 8.0,
    });
}

/// The starter destroyer expressed as design data (Blueprint v2). This is
/// the built-in fallback; on first run it's exported to designs/starter.json
/// and the file wins from then on.
///
/// Hull shape (top-down, x is forward, bow at +x):
///                                     [O][O]
///                            [O][O][O][O][O][O]
///                      [O][O][O][O][O][O][O][O][O]
///          [O][O][O][O][O][O][O][O][O][O][O][O][O][O]
///    [O][O][O][O][O][O][O][O][O][O][O][O][O][O][O][O]
///
/// Stern: engines/reactors/fuel · Mid: crew + gun deck · Bow: missiles/armor
fn builtin_starter_design() -> crate::building::blueprint::Blueprint {
    use crate::building::blueprint::{Blueprint, BlueprintHullCell, BlueprintModule, BLUEPRINT_VERSION};

    // Destroyer profile: long, narrow, pointed bow. Each row is (y, x_min, x_max).
    let hull_rows: &[(i32, i32, i32)] = &[
        ( 3,  -2,  4),   // upper superstructure
        ( 2,  -4,  7),   // upper deck
        ( 1,  -6,  9),   // upper hull
        ( 0,  -7, 10),   // spine (bow tip at +x)
        (-1,  -6,  9),   // lower hull
        (-2,  -4,  7),   // lower deck
        (-3,  -2,  4),   // lower superstructure
    ];

    // Compartment separators — these REPLACE the plain cell at their
    // position (the old spawn code double-stacked a second segment there).
    let bulkheads = [
        IVec2::new(-1, 0), IVec2::new(-1, -1),  // engineering ↔ gun deck
        IVec2::new(2, -1), IVec2::new(3, -1),   // gun deck ↔ bridge
    ];

    let mut hull_cells = Vec::new();
    for &(y, x_min, x_max) in hull_rows {
        for x in x_min..=x_max {
            let pos = IVec2::new(x, y);
            if bulkheads.contains(&pos) {
                continue;
            }
            let is_top = !hull_rows.iter().any(|&(ry, rxmin, rxmax)| ry == y + 1 && x >= rxmin && x <= rxmax);
            let is_bot = !hull_rows.iter().any(|&(ry, rxmin, rxmax)| ry == y - 1 && x >= rxmin && x <= rxmax);
            let layer = if is_top || is_bot || x == x_min || x == x_max {
                HullLayer::Outer
            } else {
                HullLayer::Inner
            };
            hull_cells.push(BlueprintHullCell {
                grid_pos: pos,
                layer,
                material: HullMaterial::Steel,
            });
        }
    }
    for pos in bulkheads {
        hull_cells.push(BlueprintHullCell {
            grid_pos: pos,
            layer: HullLayer::BulkheadDoor,
            material: HullMaterial::Steel,
        });
    }

    let m = |module_type: ModuleType, x: i32, y: i32, rotation: Rotation| BlueprintModule {
        module_type,
        grid_pos: IVec2::new(x, y),
        rotation,
        custom_name: None,
        subcomponents: None,
        extras: None,
    };

    // Weapon variant: fire group + tuning multipliers (0.5-2.0x, see
    // TUNING_MIN/MAX) + optional kinetic ammo (Bullet-type weapons only —
    // Cannon/Railgun/Coilgun/Gatling; missiles/energy weapons pass None).
    let mw = |module_type: ModuleType, x: i32, y: i32, rotation: Rotation,
              fire_group: u8, velocity: f32, fire_rate: f32, damage: f32,
              ammo: Option<crate::combat::ammo_types::KineticAmmoType>| BlueprintModule {
        module_type,
        grid_pos: IVec2::new(x, y),
        rotation,
        custom_name: None,
        subcomponents: None,
        extras: Some(crate::building::blueprint::ModuleExtras {
            tuning: Some(crate::building::customization::tuning::WeaponTuning { velocity, fire_rate, damage }),
            fire_group: Some(fire_group),
            ammo: ammo.map(crate::building::customization::tuning::SelectedAmmo),
        }),
    };

    let modules = vec![
        // Stern: engineering (4 engines, twin reactors, fuel)
        m(ModuleType::StandardEngine, -7, 0, Rotation::West),
        m(ModuleType::StandardEngine, -6, 1, Rotation::West),
        m(ModuleType::StandardEngine, -6, -1, Rotation::West),
        m(ModuleType::StandardEngine, -6, 0, Rotation::West),
        m(ModuleType::FuelTank, -5, 0, Rotation::North),
        m(ModuleType::FuelTank, -5, 1, Rotation::North),
        m(ModuleType::ManeuverThruster, -5, -1, Rotation::North),
        m(ModuleType::OxygenScrubber, -4, 0, Rotation::North),
        m(ModuleType::OxygenScrubber, -4, -1, Rotation::North),
        m(ModuleType::CoolingPump, -4, 1, Rotation::North),
        m(ModuleType::HeatVent, -3, 1, Rotation::North),
        m(ModuleType::StandardReactor, -3, 0, Rotation::North),
        m(ModuleType::StandardReactor, -3, -1, Rotation::North),
        m(ModuleType::CoolingPump, -2, -1, Rotation::North),
        // Crew
        m(ModuleType::BasicQuarters, -2, 0, Rotation::North),
        m(ModuleType::BasicQuarters, -2, 1, Rotation::North),
        m(ModuleType::GalleyMess, -1, 2, Rotation::North),
        m(ModuleType::SurgicalBay, -2, -3, Rotation::North),
        // Gun deck: railgun spine + twin cannons + twin gatlings
        mw(ModuleType::Railgun, 0, 0, Rotation::East, 2, 1.15, 0.85, 1.2, Some(crate::combat::ammo_types::KineticAmmoType::APFSDS)),
        mw(ModuleType::Cannon, 0, 1, Rotation::East, 0, 1.0, 1.0, 1.15, Some(crate::combat::ammo_types::KineticAmmoType::APHE)),
        mw(ModuleType::Cannon, 0, -1, Rotation::East, 0, 1.0, 1.0, 1.15, Some(crate::combat::ammo_types::KineticAmmoType::APHE)),
        mw(ModuleType::Gatling, 2, 2, Rotation::East, 1, 1.0, 1.2, 1.0, Some(crate::combat::ammo_types::KineticAmmoType::Flak)),
        mw(ModuleType::Gatling, 0, -2, Rotation::East, 1, 1.0, 1.2, 1.0, Some(crate::combat::ammo_types::KineticAmmoType::Flak)),
        // Shields + logistics
        m(ModuleType::ShieldEmitter, 4, 2, Rotation::North),
        m(ModuleType::ShieldEmitter, 6, -1, Rotation::North),
        m(ModuleType::BulkCargoHold, 2, 0, Rotation::North),
        m(ModuleType::RepairBay, 4, 0, Rotation::North),
        m(ModuleType::Floodlight, 5, 0, Rotation::East),
        m(ModuleType::RadarArray, 6, 0, Rotation::East),
        // Bridge
        m(ModuleType::BridgeWing, 4, -2, Rotation::North),
        // Bow: missile battery + armor prow
        mw(ModuleType::HeavyMissile, 7, 1, Rotation::East, 3, 1.0, 1.0, 1.1, None),
        mw(ModuleType::HeavyMissile, 7, -1, Rotation::East, 3, 1.0, 1.0, 1.1, None),
        m(ModuleType::CornerArmorPlate, 8, 0, Rotation::North),
        m(ModuleType::AngledArmorPlate, 10, 0, Rotation::North),
        m(ModuleType::StaggeredArmorPlate, 1, -3, Rotation::North),
    ];

    Blueprint {
        name: "starter_destroyer".into(),
        hull_cells,
        modules,
        created_at: "builtin".into(),
        version: BLUEPRINT_VERSION,
    }
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
    let footprint = crate::building::footprints::footprint_override(module_type);
    let cells = crate::building::GridOccupancy::cells_for(grid_pos, def.size, rotation, footprint);
    let (min_x, max_x, min_y, max_y) = cells.iter().fold(
        (i32::MAX, i32::MIN, i32::MAX, i32::MIN),
        |(mnx, mxx, mny, mxy), c| (mnx.min(c.x), mxx.max(c.x), mny.min(c.y), mxy.max(c.y)),
    );
    let center_x = (min_x as f32 + max_x as f32) / 2.0 * 66.0;
    let center_y = (min_y as f32 + max_y as f32) / 2.0 * 66.0 - 33.0;

    let sprite_path = sprite_map::module_sprite_path(module_type)
        .unwrap_or("sprites/modules/small_reactor.png");
    let texture = asset_server.load(sprite_path);

    let visual_angle = rotation.to_radians() + sprite_map::sprite_base_rotation(module_type);

    // Sprite dimensions must cover the ROTATED cell bounds after the
    // sprite itself is rotated by visual_angle — which is NOT the cell
    // rotation (it includes each texture's base-art offset, e.g. engine
    // art drawn 90° off). So take the rotated cell bounds and un-rotate
    // them by the final visual angle: if it's an odd quarter-turn the
    // width/height swap. Anything else leaves multi-cell modules lying
    // 90° across their claimed cells ("between the grid").
    let bounds_w = (max_x - min_x) as f32;
    let bounds_h = (max_y - min_y) as f32;
    let quarter = ((visual_angle / std::f32::consts::FRAC_PI_2).round() as i32).rem_euclid(4);
    let (cells_w, cells_h) = if quarter % 2 == 1 {
        (bounds_h, bounds_w)
    } else {
        (bounds_w, bounds_h)
    };
    let sprite_w = 60.0 + cells_w * 66.0;
    let sprite_h = 60.0 + cells_h * 66.0;

    let module_base_color = {
        let srgba = def.color.to_srgba();
        Color::srgb(
            (srgba.red * 1.5).min(1.0),
            (srgba.green * 1.5).min(1.0),
            (srgba.blue * 1.5).min(1.0),
        )
    };

    let module_entity = commands.spawn((
        (Sprite {
                image: texture,
                color: module_base_color,
                custom_size: Some(Vec2::new(sprite_w, sprite_h)),
                ..default()
            }, Transform {
                translation: Vec3::new(center_x, center_y, 0.2),
                rotation: Quat::from_rotation_z(visual_angle),
                ..default()
            }),
        BaseSpriteColor(module_base_color),
        Module {
            module_type,
            health: def.health,
            max_health: def.health,
            power_consumption: def.power_consumption,
            power_generation: def.power_generation,
            // Essential modules start active, others start inactive to save power.
            // Weapons included: a gun that silently won't fire because of a
            // hidden power toggle reads as a bug, not a mechanic.
            is_active: matches!(module_type.category(),
                ModuleCategory::Power | ModuleCategory::Propulsion | ModuleCategory::LifeSupport
                | ModuleCategory::Weapons
            ) || matches!(module_type,
                ModuleType::HelmStation | ModuleType::ManeuverThruster | ModuleType::CoolingPump
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

    // Insert CrewStation if this module type requires one. Priority
    // orders auto-assignment — vital systems staff first, because an
    // UNMANNED station doesn't run at all (see compute_module_efficiency),
    // and 8 crew never cover every station on a real ship.
    if def.crew_station {
        let priority = match module_type {
            ModuleType::HelmStation => 9,
            _ => match module_type.category() {
                ModuleCategory::Power => 10,
                ModuleCategory::Propulsion => 9,
                ModuleCategory::LifeSupport => 8,
                ModuleCategory::Weapons => 6,
                ModuleCategory::Control | ModuleCategory::Detection => 4,
                _ => 3,
            },
        };
        commands.entity(module_entity).insert(CrewStation {
            priority,
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

    // Stat tuning (power-budget sliders) — defaults are identity multipliers,
    // so AI ships spawning through this same path are unaffected.
    {
        use crate::building::customization::tuning;
        if tuning::is_tunable_weapon(module_type) {
            commands.entity(module_entity).insert(tuning::WeaponTuning::default());
        }
        if tuning::is_kinetic_weapon(module_type) {
            commands.entity(module_entity).insert(tuning::SelectedAmmo::default());
        }
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
                // Stable base-stat snapshot — see BaseWeaponStats docs for
                // why calculate_machine_stats must never read live Weapon
                // values as its "base".
                if let CompanionData::Weapon { damage, range, fire_rate, ammo, .. } = &def.companion {
                    commands.entity(module_entity).insert(BaseWeaponStats {
                        damage: *damage,
                        range: *range,
                        fire_rate: *fire_rate,
                        max_ammo: *ammo,
                    });
                }
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

    commands.entity(module_entity).insert(ChildOf(parent));

    module_entity
}

/// Spawns a custom module with ship-components
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

    // Spawn ship-component entities as children
    for subcomponent_type in subcomponents {
        let subcomponent_entity = commands.spawn(SubComponent {
            subcomponent_type,
            parent_module: module_entity,
        }).id();

        commands.entity(subcomponent_entity).insert(ChildOf(module_entity));
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
                waste_recycling: 0.0,
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
                    // Fixed widened from 30° — that required aiming almost
                    // exactly where the weapon was physically mounted, so
                    // Fixed-mount guns (Railgun, some missiles) frequently
                    // just didn't fire at all while aiming normally.
                    firing_arc: match mount_type {
                        MountType::Fixed => 120.0,
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
                crate::combat::targeting::fire_groups::FireGroup::default(),
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
        CompanionData::Radar { range, noise_on_ping } => {
            commands.entity(entity).insert(Radar {
                range: *range,
                noise_on_ping: *noise_on_ping,
                is_pinging: false,
            });
        }
        CompanionData::PassiveRadar { range } => {
            commands.entity(entity).insert(Radar {
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
            commands.entity(entity).insert(ShipLight {
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
