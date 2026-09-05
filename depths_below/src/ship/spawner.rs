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
/// Star-Destroyer wedge, symmetric across the centerline (top-down, x forward):
///
///     stern (flat, engines)                       bow (sharp point)
///     [O][O][O][O][O][O][O][O][O][O]
///     [O][O][O][O][O][O][O][O][O][O][O][O][O][O]
///     [O][O][O][O][O][O][O][O][O][O][O][O][O][O][O][O][O][O][O][O]  <- spine
///     [O][O][O][O][O][O][O][O][O][O][O][O][O][O]
///     [O][O][O][O][O][O][O][O][O][O]
///
/// Stern: engine bank/reactors/fuel · Mid: crew + gun deck · Bow: bridge/missiles/armor
pub(crate) fn builtin_starter_design() -> crate::building::blueprint::Blueprint {
    use crate::building::blueprint::{Blueprint, BlueprintHullCell, BlueprintModule, BLUEPRINT_VERSION};

    // Isosceles wedge: flat wide stern at -x, tapering to a sharp bow at +x.
    // Symmetric across y=0. Each row is (y, x_min, x_max).
    let hull_rows: &[(i32, i32, i32)] = &[
        ( 5,  -8, -7),
        ( 4,  -8, -3),
        ( 3,  -8,  1),
        ( 2,  -8,  5),
        ( 1,  -8,  9),
        ( 0,  -8, 11),   // spine: flat stern (-8) to bow tip (+11)
        (-1,  -8,  9),
        (-2,  -8,  5),
        (-3,  -8,  1),
        (-4,  -8, -3),
        (-5,  -8, -7),
    ];

    // No fixed bulkhead doors in the symmetric starter — an odd door set would
    // break the mirror symmetry. Typed empty array so the .contains()/loop below
    // still compile.
    let bulkheads: [IVec2; 0] = [];

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
            tuning: Some(crate::building::customization::tuning::WeaponTuning { velocity, fire_rate, damage, traverse: 1.0 }),
            fire_group: Some(fire_group),
            ammo: ammo.map(crate::building::customization::tuning::SelectedAmmo),
        }),
    };

    // Symmetric across the centerline: paired modules mirror ±y (same rotation —
    // these sprites have base_rotation 0 / no directional overhang, so a N<->S
    // flip would only render the bottom copy upside-down); singletons and
    // centered T-modules (medbay/bridge) sit on the spine. The one unavoidable
    // exception is the 2×1 spinal Railgun, which can't straddle an odd-width
    // spine exactly — a negligible half-cell offset under a centered barrel.
    let modules = vec![
        // --- Stern / engineering: 5-wide engine bank, fuel, reactors, life support ---
        m(ModuleType::StandardEngine, -8, 1, Rotation::West),
        m(ModuleType::StandardEngine, -8, -1, Rotation::West),
        m(ModuleType::StandardEngine, -8, 2, Rotation::West),
        m(ModuleType::StandardEngine, -8, -2, Rotation::West),
        m(ModuleType::StandardEngine, -8, 0, Rotation::West),
        m(ModuleType::FuelTank, -7, 1, Rotation::North),
        m(ModuleType::FuelTank, -7, -1, Rotation::North),
        m(ModuleType::ManeuverThruster, -7, 4, Rotation::North),
        m(ModuleType::ManeuverThruster, -7, -4, Rotation::North),
        m(ModuleType::StandardReactor, -6, 1, Rotation::North),
        m(ModuleType::StandardReactor, -6, -1, Rotation::North),
        m(ModuleType::OxygenScrubber, -5, 2, Rotation::North),
        m(ModuleType::OxygenScrubber, -5, -2, Rotation::North),
        m(ModuleType::CoolingPump, -5, 1, Rotation::North),
        m(ModuleType::CoolingPump, -5, -1, Rotation::North),
        m(ModuleType::HeatVent, -4, 3, Rotation::North),
        m(ModuleType::HeatVent, -4, -3, Rotation::North),
        // --- Crew + mid spine: medbay (centered), quarters, repair ---
        m(ModuleType::SurgicalBay, -4, 1, Rotation::East),
        m(ModuleType::BasicQuarters, -3, 1, Rotation::North),
        m(ModuleType::BasicQuarters, -3, -1, Rotation::North),
        m(ModuleType::RepairBay, -2, 0, Rotation::North),
        // Galley (+y) + cargo (-y): chiral L-shapes placed so their cells are
        // exact y-mirrors of each other — a matched 2x2 room on each side.
        m(ModuleType::GalleyMess, -1, 2, Rotation::North),
        m(ModuleType::BulkCargoHold, 0, -3, Rotation::West),
        // --- Gun deck: spinal railgun, twin cannons, twin gatling PD ---
        mw(ModuleType::Railgun, 0, 0, Rotation::East, 2, 1.15, 0.85, 1.2, Some(crate::combat::ammo_types::KineticAmmoType::APFSDS)),
        mw(ModuleType::Cannon, 1, 2, Rotation::East, 0, 1.0, 1.0, 1.15, Some(crate::combat::ammo_types::KineticAmmoType::APHE)),
        mw(ModuleType::Cannon, 1, -2, Rotation::East, 0, 1.0, 1.0, 1.15, Some(crate::combat::ammo_types::KineticAmmoType::APHE)),
        m(ModuleType::ShieldEmitter, 2, 2, Rotation::North),
        m(ModuleType::ShieldEmitter, 2, -2, Rotation::North),
        mw(ModuleType::Gatling, 3, 2, Rotation::East, 1, 1.0, 1.2, 1.0, Some(crate::combat::ammo_types::KineticAmmoType::Flak)),
        mw(ModuleType::Gatling, 3, -2, Rotation::East, 1, 1.0, 1.2, 1.0, Some(crate::combat::ammo_types::KineticAmmoType::Flak)),
        // --- Forward / command: sensors, floodlight, bridge (centered), missiles, prow armor ---
        m(ModuleType::RadarArray, 3, 0, Rotation::East),
        m(ModuleType::Floodlight, 4, 0, Rotation::East),
        m(ModuleType::BridgeWing, 5, 1, Rotation::East),
        mw(ModuleType::HeavyMissile, 7, 1, Rotation::East, 3, 1.0, 1.0, 1.1, None),
        mw(ModuleType::HeavyMissile, 7, -1, Rotation::East, 3, 1.0, 1.0, 1.1, None),
    ];

    // The player's ship gets the same treatment as every faction hull: plating
    // derived from its own outline, one plate per step of the wedge, plus caps
    // along the dorsal and ventral edges.
    //
    // It had two AngledArmorPlates before, at (8,0) and (9,0) — both on the
    // SPINE, which is hull. Hull wins its own cell in ShipGrid, so neither was
    // ever the thing a round met; they were decoration. Anything derived here
    // lands outboard, where it can actually be hit.
    let occupied: std::collections::HashSet<IVec2> = hull_cells
        .iter()
        .map(|c| c.grid_pos)
        .chain(modules.iter().map(|m| m.grid_pos))
        .collect();
    let mut modules = modules;
    let plating = crate::building::armour::belt(hull_rows, ModuleType::AngledArmorPlate)
        .into_iter()
        .chain(crate::building::armour::caps(hull_rows, ModuleType::AngledArmorPlate, 2));
    for (grid_pos, module_type, rotation) in plating {
        if occupied.contains(&grid_pos) {
            continue;
        }
        modules.push(BlueprintModule {
            module_type,
            grid_pos,
            rotation,
            custom_name: None,
            subcomponents: None,
            extras: None,
        });
    }

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
    // Local (unrotated) sprite size. Barrel weapons get a forward-extended
    // canvas — their art is drawn 1:3 with the turret centered and the barrel
    // pointing up — so the barrel overhangs the cells ahead. Combined with the
    // raised z below, the barrel renders over neighbouring blocks. The extension
    // is applied in LOCAL space and then run through the same quarter-turn swap,
    // so the barrel follows the weapon's facing at any rotation.
    let local_w = 60.0 + bounds_w * 66.0;
    // Directional parts (thruster nozzles, gun barrels) protrude PAST their
    // block: the sprite is lengthened along its local vertical axis by
    // `overhang`, and the whole sprite is nudged so the housing stays centred on
    // the cell while the extension hangs off the protruding end. `protrude` is
    // +1 when that end is the art's top (barrels), -1 when it's the bottom
    // (nozzles). The matching art carries the extra length in its canvas so
    // nothing stretches.
    let (overhang, protrude) = sprite_map::sprite_overhang(module_type);
    let local_h = 60.0 + bounds_h * 66.0;
    // Footprint dims after the quarter-turn swap (handles rotated multi-cell
    // aspect). The swap must run on the FOOTPRINT only.
    let (foot_w, foot_h) = if quarter % 2 == 1 {
        (local_h, local_w)
    } else {
        (local_w, local_h)
    };
    // The overhang lengthens the TEXTURE's own vertical axis (barrel/nozzle),
    // which is always sprite_h regardless of the footprint swap. Adding it before
    // the swap put it on the width for East/West-facing parts (odd quarter-turns)
    // and stretched them — the in-game gun/engine distortion.
    let sprite_w = foot_w;
    let sprite_h = foot_h + overhang;
    // Offset in art-local space (art +Y = up), rotated into world by the same
    // visual angle so the barrel/nozzle follows the part's facing at any turn.
    let sprite_off = Quat::from_rotation_z(visual_angle)
        * Vec3::new(0.0, protrude * overhang * 0.5, 0.0);
    // Raise directional parts so the protruding end renders over the neighbour.
    let sprite_z = if overhang > 0.0 { 0.4 } else { 0.2 };

    // The new module sprites carry their own colour and detail, so render
    // them at full white — the old per-module def.color multiply (a leftover
    // from the flat placeholder art that relied on colour to tell modules
    // apart) washed the detailed sprites into solid coloured blocks. Damage
    // darkening and wreck greying still work: they multiply DOWN from this
    // base (stored in BaseSpriteColor), and white is their neutral start.
    // Wedges draw nothing here: their whole point is a silhouette that ISN'T a
    // square, and the shared module sprite is a full textured square. Painting
    // a dark triangle over it just made a square with a triangle on it — the
    // outline never actually got cut. The triangle is built from child quads
    // below instead, so the block's shape is the shape.
    let is_wedge = matches!(module_type, ModuleType::AngledArmorPlate | ModuleType::AngledHullPlate);
    let module_base_color = if is_wedge { Color::NONE } else { Color::WHITE };

    let module_entity = commands.spawn((
        (Sprite {
                image: texture,
                color: module_base_color,
                custom_size: Some(Vec2::new(sprite_w, sprite_h)),
                ..default()
            }, Transform {
                translation: Vec3::new(center_x + sprite_off.x, center_y + sprite_off.y, sprite_z),
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
            // Weapons AND Detection included: a gun that silently won't fire — or
            // a radar you placed that silently won't scan ("no active detection
            // module") — because of a hidden power toggle reads as a bug, not a
            // mechanic.
            is_active: matches!(module_type.category(),
                ModuleCategory::Power | ModuleCategory::Propulsion | ModuleCategory::LifeSupport
                | ModuleCategory::Weapons | ModuleCategory::Detection
            ) || matches!(module_type,
                ModuleType::HelmStation | ModuleType::ManeuverThruster | ModuleType::CoolingPump
                | ModuleType::HeatVent | ModuleType::BasicQuarters | ModuleType::Barracks
                | ModuleType::Floodlight | ModuleType::RepairBay
            ),
            grid_position: grid_pos,
            size: def.size,
            rotation,
        },
        crate::building::Block::for_module(grid_pos, module_type, rotation),
        Selectable,
    )).id();

    insert_companion_components(commands, module_entity, &def.companion);

    // Gun turrets: the module sprite above is the static base mount; add a
    // pivot-centred barrel child sprite that the aim system rotates to track the
    // target (cursor for the player, the player ship for AI).
    if let Some(barrel_path) = sprite_map::turret_barrel_sprite(module_type) {
        let barrel = commands.spawn((
            Sprite {
                image: asset_server.load(barrel_path),
                custom_size: Some(Vec2::splat(132.0)),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 0.3),
            TurretBarrel,
        )).id();
        commands.entity(module_entity).add_child(barrel);
        commands.entity(module_entity).insert(Turret {
            turn_speed: sprite_map::turret_turn_speed(module_type),
            world_angle: 0.0,
        });
    }

    // WEDGES: the plate itself, as a staircase of quads filling the half-cell
    // the armour actually occupies — the same half `Block::facing` reports to
    // hit resolution, so what you see is what a round meets.
    //
    // Children inherit the parent's visual_angle, so this is authored once
    // facing north-east and R turns it with the block.
    if is_wedge {
        // A 1x1 module sprite is 60 units, NOT the 66-unit cell pitch — see
        // local_w/local_h above. Sizing to the cell overhangs the neighbours.
        const H: f32 = 30.0;
        const BANDS: usize = 20;
        let band = (H * 2.0) / BANDS as f32;
        let face = def.color;
        // Matches the hull plating around it rather than reading as a flat
        // colour chip: a darker body with a lit edge along the cut.
        let body = face.mix(&Color::BLACK, 0.35);
        for i in 0..BANDS {
            let y = H - band * (i as f32 + 0.5);
            // `facing` is an outward normal, so the plate's material sits
            // BEHIND it: the half with x + y <= 0. At height y that runs from
            // the left edge in to x = -y. Drawing the other half puts the
            // mass on the outside and leaves the plate hanging off the hull
            // with a gap where it should be bolted on.
            let width = (H - y).clamp(0.0, H * 2.0);
            if width <= 0.5 { continue; }
            let quad = commands.spawn((
                Sprite {
                    color: body,
                    custom_size: Some(Vec2::new(width, band + 0.5)),
                    ..default()
                },
                Transform::from_xyz(-H + width * 0.5, y, 0.05),
            )).id();
            commands.entity(module_entity).add_child(quad);
        }
        // Lit edge along the hypotenuse — the sloped face, and the only part of
        // the block that tells you which way it's turned.
        let edge = commands.spawn((
            Sprite {
                color: face.mix(&Color::WHITE, 0.35),
                custom_size: Some(Vec2::new(H * 2.0 * std::f32::consts::SQRT_2 - 2.0, 3.0)),
                ..default()
            },
            Transform {
                translation: Vec3::new(0.0, 0.0, 0.08),
                rotation: Quat::from_rotation_z(-std::f32::consts::FRAC_PI_4),
                ..default()
            },
        )).id();
        commands.entity(module_entity).add_child(edge);
    }

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

#[cfg(test)]
mod starter_tests {
    use super::*;
    use std::collections::HashSet;

    fn is_plate(mt: ModuleType) -> bool {
        matches!(mt, ModuleType::AngledArmorPlate | ModuleType::AngledHullPlate)
    }

    /// The player's hull is held to the same rule as every faction hull:
    /// plating outboard, never buried. The starter used to carry two plates on
    /// its own spine, which ShipGrid resolves to hull — armour that could not
    /// be hit.
    #[test]
    fn starter_plating_is_outboard_and_attached() {
        let design = builtin_starter_design();
        let hull: HashSet<IVec2> = design.hull_cells.iter().map(|c| c.grid_pos).collect();
        let plates: Vec<_> = design.modules.iter().filter(|m| is_plate(m.module_type)).collect();

        assert!(plates.len() >= 8, "the wedge should carry a real belt, got {}", plates.len());
        for p in plates {
            assert!(!hull.contains(&p.grid_pos),
                "plate at {:?} is buried in hull and would never be hit", p.grid_pos);
            let touching = [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y]
                .iter().any(|d| hull.contains(&(p.grid_pos + *d)));
            assert!(touching, "plate at {:?} is floating free of the ship", p.grid_pos);
        }
    }

    /// Nothing may double-book a cell — the derived plating is filtered against
    /// the hand-placed modules, and this is what proves the filter works.
    #[test]
    fn starter_has_no_overlapping_modules() {
        let design = builtin_starter_design();
        let mut seen = HashSet::new();
        for m in &design.modules {
            assert!(seen.insert(m.grid_pos),
                "two modules both at {:?}", m.grid_pos);
        }
    }
}
