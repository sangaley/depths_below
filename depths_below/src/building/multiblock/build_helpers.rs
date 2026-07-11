use bevy::prelude::*;
use crate::components::*;
use crate::building::GridOccupancy;
use super::components::*;

// ============================================================================
// MULTI-BLOCK BUILD HELPERS
// Connection lines, directional validation, stat preview, ghost colors,
// quick-build templates, rotation-aware chain direction.
// ============================================================================

/// Visual connection line between two grid positions
#[derive(Component)]
pub struct ConnectionLine;

/// Stat preview floating text shown during build mode
#[derive(Component)]
pub struct StatPreviewText;

/// Quick-build template definition
pub struct WeaponTemplate {
    pub name: &'static str,
    pub description: &'static str,
    pub core: ModuleType,
    pub blocks: Vec<(ModuleType, IVec2)>, // Offsets from core position
    pub total_cost: u32,
}

// ============================================================================
// CONNECTION LINE VISUALIZATION
// ============================================================================

/// Draw colored lines between connected blocks. Green=connected, red=disconnected.
pub fn draw_connection_lines(
    mut commands: Commands,
    block_query: Query<(&Module, &MachineBlock, &GlobalTransform)>,
    existing_lines: Query<Entity, With<ConnectionLine>>,
) {
    // Despawn old lines
    for entity in existing_lines.iter() {
        commands.entity(entity).despawn();
    }

    // Draw new lines for each connection
    for (_module, block, global_transform) in block_query.iter() {
        if let Some(next_entity) = block.next_in_chain {
            if let Ok((_, _, next_gt)) = block_query.get(next_entity) {
                let from = global_transform.translation().truncate();
                let to = next_gt.translation().truncate();
                let midpoint = (from + to) / 2.0;
                let diff = to - from;
                let length = diff.length();
                let angle = diff.y.atan2(diff.x);

                let color = if block.connected_core.is_some() {
                    Color::srgba(0.2, 0.8, 0.3, 0.5) // Green = connected
                } else {
                    Color::srgba(0.8, 0.2, 0.2, 0.5) // Red = disconnected
                };

                commands.spawn((
                    (Sprite {
                            color,
                            custom_size: Some(Vec2::new(length, 3.0)),
                            ..default()
                        }, Transform {
                            translation: Vec3::new(midpoint.x, midpoint.y, 0.15),
                            rotation: Quat::from_rotation_z(angle),
                            ..default()
                        }),
                    ConnectionLine,
                ));
            }
        }

        // Disconnected blocks pulse red
        if block.connected_core.is_none() && block.role != BlockRole::Core {
            let pos = global_transform.translation().truncate();
            commands.spawn((
                (Sprite {
                        color: Color::srgba(0.8, 0.1, 0.1, 0.3),
                        custom_size: Some(Vec2::splat(64.0)),
                        ..default()
                    }, Transform::from_xyz(pos.x, pos.y, 0.16)),
                ConnectionLine,
            ));
        }
    }
}

// ============================================================================
// DIRECTIONAL PLACEMENT VALIDATION
// ============================================================================

/// Check if an extension block can be placed at the given position,
/// considering the direction of the core/chain it would connect to.
pub fn validate_multiblock_placement(
    module_type: ModuleType,
    grid_pos: IVec2,
    rotation: Rotation,
    occupancy: &GridOccupancy,
    block_query: &Query<(&Module, &MachineBlock)>,
) -> MultiblockPlacementResult {
    let role = module_type_to_role(module_type);
    let Some(role) = role else {
        return MultiblockPlacementResult::NotMultiblock;
    };

    match role {
        BlockRole::Barrel | BlockRole::Nozzle => {
            // Must be placed in the direction the previous block faces
            // Check: is there a core or barrel in the opposite direction of our rotation?
            let source_dir = rotation_to_offset(rotation.rotate_cw().rotate_cw()); // Behind us
            let source_pos = grid_pos + source_dir;

            if let Some(&source_entity) = occupancy.cells.get(&source_pos) {
                if let Ok((_source_module, source_block)) = block_query.get(source_entity) {
                    if source_block.role == BlockRole::Core || source_block.role == role {
                        // Valid: connected to a core or same-type chain
                        return MultiblockPlacementResult::Valid {
                            connects_to: source_entity,
                            stat_preview: barrel_stat_preview(source_block),
                        };
                    }
                }
            }
            MultiblockPlacementResult::Invalid("Must extend from weapon core or barrel in facing direction".into())
        }
        BlockRole::AmmoFeed | BlockRole::Cooling => {
            // Must be adjacent to a core
            let mut found_core = false;
            for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                let adj = grid_pos + offset;
                if let Some(&adj_entity) = occupancy.cells.get(&adj) {
                    if let Ok((_, adj_block)) = block_query.get(adj_entity) {
                        if adj_block.role == BlockRole::Core {
                            found_core = true;
                            break;
                        }
                    }
                }
            }
            if found_core {
                MultiblockPlacementResult::Valid {
                    connects_to: Entity::PLACEHOLDER,
                    stat_preview: match role {
                        BlockRole::AmmoFeed => "+20% fire rate".into(),
                        BlockRole::Cooling => "+40 heat capacity".into(),
                        _ => String::new(),
                    },
                }
            } else {
                MultiblockPlacementResult::Invalid("Must be placed adjacent to a weapon core".into())
            }
        }
        BlockRole::FuelRod => {
            // Must be adjacent to a reactor core
            let mut found_reactor = false;
            for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                let adj = grid_pos + offset;
                if let Some(&adj_entity) = occupancy.cells.get(&adj) {
                    if let Ok((adj_module, adj_block)) = block_query.get(adj_entity) {
                        if adj_block.role == BlockRole::Core && matches!(adj_module.module_type,
                            ModuleType::SmallReactor | ModuleType::StandardReactor |
                            ModuleType::LargeReactor | ModuleType::FusionReactor) {
                            found_reactor = true;
                            break;
                        }
                    }
                }
            }
            if found_reactor {
                MultiblockPlacementResult::Valid {
                    connects_to: Entity::PLACEHOLDER,
                    stat_preview: "+15 power output".into(),
                }
            } else {
                MultiblockPlacementResult::Invalid("Must be placed adjacent to a reactor".into())
            }
        }
        _ => MultiblockPlacementResult::NotMultiblock,
    }
}

pub enum MultiblockPlacementResult {
    Valid { connects_to: Entity, stat_preview: String },
    Invalid(String),
    NotMultiblock,
}

fn barrel_stat_preview(source: &MachineBlock) -> String {
    let barrel_num = source.chain_distance + 1;
    format!("+60 range, +12% damage (barrel #{})", barrel_num + 1)
}

pub fn module_type_to_role(mt: ModuleType) -> Option<BlockRole> {
    match mt {
        ModuleType::BarrelExtension => Some(BlockRole::Barrel),
        ModuleType::AmmoFeedUnit => Some(BlockRole::AmmoFeed),
        ModuleType::CoolingJacket => Some(BlockRole::Cooling),
        ModuleType::ReactorFuelRod => Some(BlockRole::FuelRod),
        ModuleType::ReactorCooling => Some(BlockRole::Cooling),
        ModuleType::EngineNozzle => Some(BlockRole::Nozzle),
        ModuleType::ShieldEmitter => Some(BlockRole::ShieldEmitter),
        _ => None,
    }
}

fn rotation_to_offset(rotation: Rotation) -> IVec2 {
    match rotation {
        Rotation::North => IVec2::Y,
        Rotation::East => IVec2::X,
        Rotation::South => IVec2::NEG_Y,
        Rotation::West => IVec2::NEG_X,
    }
}

// ============================================================================
// QUICK-BUILD TEMPLATES
// ============================================================================

pub fn weapon_templates() -> Vec<WeaponTemplate> {
    vec![
        WeaponTemplate {
            name: "Basic Cannon",
            description: "Cannon core + 2 barrels. Simple, effective.",
            core: ModuleType::Cannon,
            blocks: vec![
                (ModuleType::BarrelExtension, IVec2::new(1, 0)),
                (ModuleType::BarrelExtension, IVec2::new(2, 0)),
            ],
            total_cost: 150 + 30 + 30,
        },
        WeaponTemplate {
            name: "Long Railgun",
            description: "Railgun + 4 barrels + cooling. Maximum range sniper.",
            core: ModuleType::Railgun,
            blocks: vec![
                (ModuleType::BarrelExtension, IVec2::new(2, 0)),
                (ModuleType::BarrelExtension, IVec2::new(3, 0)),
                (ModuleType::BarrelExtension, IVec2::new(4, 0)),
                (ModuleType::BarrelExtension, IVec2::new(5, 0)),
                (ModuleType::CoolingJacket, IVec2::new(0, 1)),
            ],
            total_cost: 400 + 30 * 4 + 35,
        },
        WeaponTemplate {
            name: "Gatling Nest",
            description: "Gatling + 1 barrel + 2 ammo feeds. Sustained fire.",
            core: ModuleType::Gatling,
            blocks: vec![
                (ModuleType::BarrelExtension, IVec2::new(1, 0)),
                (ModuleType::AmmoFeedUnit, IVec2::new(0, 1)),
                (ModuleType::AmmoFeedUnit, IVec2::new(0, -1)),
            ],
            total_cost: 100 + 30 + 40 + 40,
        },
        WeaponTemplate {
            name: "Torpedo Battery",
            description: "Torpedo launcher + warhead bay + ammo feed.",
            core: ModuleType::HeavyMissile,
            blocks: vec![
                (ModuleType::WarheadBay, IVec2::new(-1, 0)),
                (ModuleType::AmmoFeedUnit, IVec2::new(0, 1)),
            ],
            total_cost: 200 + 60 + 40,
        },
        WeaponTemplate {
            name: "Laser Array",
            description: "Laser + focusing array + 2 cooling jackets. Sustained beam.",
            core: ModuleType::Laser,
            blocks: vec![
                (ModuleType::FocusingArray, IVec2::new(1, 0)),
                (ModuleType::CoolingJacket, IVec2::new(0, 1)),
                (ModuleType::CoolingJacket, IVec2::new(0, -1)),
            ],
            total_cost: 200 + 80 + 35 + 35,
        },
    ]
}
