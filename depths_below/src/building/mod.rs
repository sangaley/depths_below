use bevy::prelude::*;
use std::collections::HashMap;
use smallvec::SmallVec;
use crate::states::{GameState, BuildState};
use crate::components::*;
use crate::resources::*;
use crate::events::*;
use crate::submarine::spawn_module;
use crate::sprite_map;

pub mod customization;
pub mod inspection;
pub mod multiblock;
pub mod build_history;
pub mod symmetry;
pub mod build_info;
pub mod clipboard;
pub mod templates;
pub mod template_ghost;

pub mod rooms;
pub mod registry;
pub mod stat_calculator;
pub mod blueprint;

pub use registry::ModuleRegistry;
pub use stat_calculator::StatCalculator;
pub use blueprint::BlueprintResource;

pub struct BuildingPlugin;

impl Plugin for BuildingPlugin {
    fn build(&self, app: &mut App) {
        app
            .add_state::<BuildState>()
            .init_resource::<BuildingState>()
            .init_resource::<rooms::RoomMap>()
            .init_resource::<GridOccupancy>()
            .init_resource::<BlueprintResource>()
            .init_resource::<build_history::BuildHistory>()
            .init_resource::<symmetry::SymmetryState>()
            .init_resource::<build_info::ModuleSearchState>()
            .init_resource::<clipboard::BuildClipboard>()
            .init_resource::<templates::TemplateState>()
            .insert_resource(registry::build_registry())
            .insert_resource({
                let mut reg = customization::parameters::CustomizationRegistry::default();
                customization::weapons::register_weapon_customizations(&mut reg);
                reg
            })
            .add_systems(
                Update,
                (
                    update_grid_occupancy,
                    handle_build_input,
                    update_ghost_preview,
                    handle_module_placement,
                    handle_module_removal,
                    process_hull_placement,
                    process_module_placement,
                    process_module_removal,
                    blueprint::save_blueprint_system,
                    blueprint::load_blueprint_system,
                    blueprint::delete_blueprint_system,
                    inspection::right_click_inspect,
                    inspection::handle_customize_click,
                )
                    .chain()
                    .run_if(in_state(GameState::StationDocked)),
            )
            // Room detection runs in both surface and exploring
            .add_systems(
                Update,
                (
                    rooms::update_room_map,
                    rooms::update_room_power,
                ).run_if(in_state(GameState::StationDocked)
                    .or_else(in_state(GameState::Exploring))),
            )
            // Custom module stat recalculation + weapon sync (runs in all states)
            .add_systems(
                Update,
                (recalculate_custom_module_stats, sync_calculated_to_weapon).chain(),
            )
            // Multi-block machine systems (connection detection, stat calc, damage chain)
            .add_systems(
                Update,
                (
                    multiblock::connections::rebuild_machine_connections,
                    multiblock::connections::calculate_barrel_stress
                        .after(multiblock::connections::rebuild_machine_connections),
                    multiblock::stats::calculate_machine_stats
                        .after(multiblock::connections::rebuild_machine_connections),
                    multiblock::stats::apply_machine_stats_to_weapons
                        .after(multiblock::stats::calculate_machine_stats),
                    multiblock::damage::process_block_destruction,
                ).run_if(in_state(GameState::StationDocked)
                    .or_else(in_state(GameState::Exploring))),
            )
            // Enhancer effects (separate system group to stay under tuple limit)
            .add_systems(
                Update,
                (
                    multiblock::enhancers::apply_weapon_enhancers,
                    multiblock::enhancers::apply_hull_enhancers,
                    multiblock::enhancers::apply_utility_enhancers,
                    multiblock::enhancers::emergency_o2_system,
                    multiblock::enhancers::emergency_shutdown_system,
                    multiblock::enhancers::afterburner_system,
                ).run_if(in_state(GameState::Exploring)),
            )
            // Build mode tools (undo, symmetry, overlays, info)
            .add_systems(
                Update,
                (
                    multiblock::build_helpers::draw_connection_lines,
                    build_history::undo_redo_input,
                    symmetry::toggle_symmetry,
                    build_info::toggle_cost_summary,
                    build_info::update_center_of_mass,
                    build_info::toggle_power_overlay,
                    build_info::toggle_heat_overlay,
                    clipboard::clipboard_input,
                    clipboard::clipboard_paste,
                    clipboard::paste_ghost_preview,
                    templates::template_input,
                    template_ghost::update_template_ghost,
                    template_ghost::chain_delete_system,
                ).run_if(in_state(GameState::StationDocked)),
            );
    }
}

// ============================================================================
// GRID OCCUPANCY - tracks which cells are taken
// ============================================================================

#[derive(Resource, Default)]
pub struct GridOccupancy {
    pub cells: HashMap<IVec2, Entity>,
}

impl GridOccupancy {
    /// Get all grid cells a module occupies given origin, size, and rotation.
    /// Uses SmallVec to avoid heap allocation for modules up to 2x2.
    pub fn cells_for(origin: IVec2, size: IVec2, rotation: Rotation) -> SmallVec<[IVec2; 4]> {
        let mut cells = SmallVec::new();
        for x in 0..size.x {
            for y in 0..size.y {
                let offset = rotation.rotate_offset(IVec2::new(x, y));
                cells.push(origin + offset);
            }
        }
        cells
    }

    /// Check if all cells for a module placement are free
    pub fn can_place(&self, origin: IVec2, size: IVec2, rotation: Rotation) -> bool {
        for cell in Self::cells_for(origin, size, rotation) {
            if self.cells.contains_key(&cell) {
                return false;
            }
        }
        true
    }
}

/// Rebuilds grid occupancy from all modules and hull segments.
/// Skips rebuild when entity count hasn't changed (cheap change detection).
fn update_grid_occupancy(
    module_query: Query<(Entity, &Module), Or<(Changed<Module>, Added<Module>)>>,
    hull_query: Query<(Entity, &HullSegment, &Transform), Or<(Changed<HullSegment>, Added<HullSegment>)>>,
    all_modules: Query<(Entity, &Module)>,
    all_hulls: Query<(Entity, &HullSegment, &Transform)>,
    mut occupancy: ResMut<GridOccupancy>,
    mut last_count: Local<usize>,
) {
    let current_count = all_modules.iter().count() + all_hulls.iter().count();
    let has_changes = !module_query.is_empty() || !hull_query.is_empty();
    if current_count == *last_count && !occupancy.cells.is_empty() && !has_changes {
        return;
    }
    *last_count = current_count;

    occupancy.cells.clear();

    for (entity, module) in all_modules.iter() {
        let cells = GridOccupancy::cells_for(module.grid_position, module.size, module.rotation);
        for cell in cells {
            occupancy.cells.insert(cell, entity);
        }
    }

    for (entity, _hull, transform) in all_hulls.iter() {
        let grid = rooms::transform_to_grid(transform);
        occupancy.cells.insert(grid, entity);
    }
}

// ============================================================================
// BUILD INPUT
// ============================================================================

/// Checks if a hull material is unlocked
fn is_hull_material_unlocked(material: HullMaterial, unlocks: &crate::resources::Unlocks) -> bool {
    match material {
        HullMaterial::Steel => true,
        HullMaterial::Titanium => unlocks.hull_types.contains(&"titanium".to_string()),
        HullMaterial::Composite => unlocks.hull_types.contains(&"composite".to_string()),
        HullMaterial::AbyssalAlloy => unlocks.hull_types.contains(&"abyssal_alloy".to_string()),
    }
}

/// Handles building mode input
fn handle_build_input(
    keyboard: Res<Input<KeyCode>>,
    mut build_state: ResMut<BuildingState>,
    mut next_state: ResMut<NextState<BuildState>>,
    current_state: Res<State<BuildState>>,
    mut customization_state: ResMut<CustomizationState>,
    mut placement_state: ResMut<ComponentPlacementState>,
    registry: Res<ModuleRegistry>,
    unlocks: Res<crate::resources::Unlocks>,
    mut notifications: EventWriter<ShowNotification>,
) {
    // B: Toggle build mode
    if keyboard.just_pressed(KeyCode::B) {
        match current_state.get() {
            BuildState::Inactive => next_state.set(BuildState::Placing),
            _ => next_state.set(BuildState::Inactive),
        }
    }

    // All keys below only apply when build mode is active
    if *current_state.get() == BuildState::Inactive {
        return;
    }

    // Tab: Cycle categories
    if keyboard.just_pressed(KeyCode::Tab) {
        build_state.next_category();
        build_state.auto_rotated = true; // Re-enable auto-rotation on selection change
        info!("Category: {} | {}", build_state.current_category().name(), build_state.selection_name());
    }

    // BracketRight / BracketLeft: Cycle items within category
    if keyboard.just_pressed(KeyCode::BracketRight) {
        build_state.next_item();
        build_state.auto_rotated = true;
        info!("Selected: {}", build_state.selection_name());
    }
    if keyboard.just_pressed(KeyCode::BracketLeft) {
        build_state.prev_item();
        build_state.auto_rotated = true;
        info!("Selected: {}", build_state.selection_name());
    }

    // R: Rotate (manual override, disables auto-rotation until ghost moves)
    if keyboard.just_pressed(KeyCode::R) {
        build_state.rotation = build_state.rotation.rotate_cw();
        build_state.auto_rotated = false;
        info!("Rotation: {:?}", build_state.rotation);
    }

    // M: Cycle hull material (only in Hull category), skipping locked materials
    if keyboard.just_pressed(KeyCode::M) {
        let materials = [
            HullMaterial::Steel,
            HullMaterial::Titanium,
            HullMaterial::Composite,
            HullMaterial::AbyssalAlloy,
        ];
        let current_idx = materials.iter().position(|&m| m == build_state.hull_material).unwrap_or(0);
        let mut found = false;
        for i in 1..materials.len() {
            let next = materials[(current_idx + i) % materials.len()];
            if is_hull_material_unlocked(next, &unlocks) {
                build_state.hull_material = next;
                found = true;
                break;
            }
        }
        if !found {
            notifications.send(ShowNotification {
                message: "No other hull materials unlocked. Buy upgrades at the shop (U key at surface).".into(),
                notification_type: NotificationType::Warning,
                duration: 3.0,
            });
        }
        info!("Material: {} ({:.0}m)", build_state.hull_material.name(), build_state.hull_material.radiation_shielding());
    }

    // X: Toggle deletion mode
    if keyboard.just_pressed(KeyCode::X) {
        match current_state.get() {
            BuildState::Deleting => next_state.set(BuildState::Placing),
            _ => next_state.set(BuildState::Deleting),
        }
    }

    // G: Open customization panel for current selection (if customizable)
    if keyboard.just_pressed(KeyCode::G) {
        if let BuildSelection::Module(module_type) = build_state.current_selection() {
            let module_def = registry.get(module_type);
            if module_def.customizable {
                customization_state.start_customizing(module_type);
                notifications.send(ShowNotification {
                    message: format!("⚙ Quick Customizing {}", module_type.name()),
                    notification_type: NotificationType::Info,
                    duration: 2.0,
                });
            } else {
                notifications.send(ShowNotification {
                    message: format!("{} is not customizable", module_type.name()),
                    notification_type: NotificationType::Info,
                    duration: 1.5,
                });
            }
        }
    }

    // P: Open component placement panel for current selection (if customizable)
    if keyboard.just_pressed(KeyCode::P) {
        if let BuildSelection::Module(module_type) = build_state.current_selection() {
            let module_def = registry.get(module_type);
            if module_def.customizable {
                placement_state.start_placing(module_type);
                next_state.set(BuildState::PlacingComponent);
                notifications.send(ShowNotification {
                    message: format!("🔧 Component Builder: {} - Click pieces to assemble", module_type.name()),
                    notification_type: NotificationType::Info,
                    duration: 3.0,
                });
            } else {
                notifications.send(ShowNotification {
                    message: format!("{} cannot be built from components", module_type.name()),
                    notification_type: NotificationType::Info,
                    duration: 1.5,
                });
            }
        }
    }
}

// ============================================================================
// GHOST PREVIEW & VALIDATION
// ============================================================================

/// Updates ghost position and validates placement.
/// Tracks mouse in both Placing and Deleting modes.
fn update_ghost_preview(
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    mut build_state: ResMut<BuildingState>,
    current_state: Res<State<BuildState>>,
    occupancy: Res<GridOccupancy>,
    module_query: Query<&Module>,
    hull_query: Query<(&HullSegment, &Transform, &Parent)>,
    registry: Res<ModuleRegistry>,
    currency: Res<Currency>,
) {
    // Track mouse position in both Placing and Deleting modes
    let state = *current_state.get();
    if state != BuildState::Placing && state != BuildState::Deleting {
        return;
    }

    let Ok(window) = windows.get_single() else { return };
    let Ok((camera, camera_transform)) = camera_query.get_single() else { return };

    if let Some(cursor_pos) = window.cursor_position()
        .and_then(|p| camera.viewport_to_world_2d(camera_transform, p))
    {
        let grid_size = 66.0;
        let grid_pos = IVec2::new(
            (cursor_pos.x / grid_size).round() as i32,
            (cursor_pos.y / grid_size).round() as i32,
        );

        let ghost_moved = build_state.ghost_position != grid_pos;
        build_state.ghost_position = grid_pos;

        // Auto-rotate modules when ghost moves (unless user manually rotated)
        if ghost_moved && build_state.auto_rotated {
            if let BuildSelection::Module(_) = build_state.current_selection() {
                if let Some(rot) = auto_rotate(grid_pos, &occupancy) {
                    build_state.rotation = rot;
                }
            }
        }

        // Only validate placement in Placing mode
        if state != BuildState::Placing {
            return;
        }

        let selection = build_state.current_selection();
        let rotation = build_state.rotation;

        // Determine size of what we're placing
        let size = match selection {
            BuildSelection::Hull(_) => IVec2::new(1, 1),
            BuildSelection::Module(mt) => registry.get(mt).size,
        };

        // Check overlap using GridOccupancy (supports multi-cell)
        let no_overlap = occupancy.can_place(grid_pos, size, rotation);

        // Adjacency check - at least one cell of the new module must be adjacent
        // to an existing module or hull segment
        let placement_cells = GridOccupancy::cells_for(grid_pos, size, rotation);
        let has_neighbor = placement_cells.iter().any(|&cell| {
            for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                let neighbor = cell + offset;
                if occupancy.cells.contains_key(&neighbor) {
                    return true;
                }
            }
            false
        });

        let is_first = module_query.iter().count() == 0
            && hull_query.iter().count() == 0;

        // Positional rules for modules
        let position_ok = check_position_rules(
            &selection,
            grid_pos,
            &module_query,
        );

        // Affordability check
        let can_afford = match selection {
            BuildSelection::Hull(_) => currency.credits >= build_state.hull_material.cost(),
            BuildSelection::Module(mt) => currency.credits >= registry.get(mt).cost,
        };

        // Multi-block directional validation for extension blocks
        let multiblock_ok = {
            let selection_mt = match &selection {
                BuildSelection::Module(mt) => Some(*mt),
                _ => None,
            };
            if let Some(mt) = selection_mt {
                match multiblock::build_helpers::module_type_to_role(mt) {
                    Some(_) => {
                        // This is a multi-block extension — validate direction
                        // We can't pass the full query here, so check adjacency to any MachineBlock core
                        true // Detailed validation happens at placement time
                    }
                    None => true, // Not a multi-block module, no extra validation
                }
            } else {
                true
            }
        };

        let valid = no_overlap && (has_neighbor || is_first) && position_ok && can_afford && multiblock_ok;
        build_state.is_valid_placement = valid;
        build_state.placement_reason = if valid {
            None
        } else if !no_overlap {
            Some("Overlaps existing module or hull".into())
        } else if !has_neighbor && !is_first {
            Some("Must be adjacent to existing structure".into())
        } else if !position_ok {
            match &selection {
                BuildSelection::Module(mt) => {
                    let cat = mt.category();
                    if cat == ModuleCategory::Propulsion {
                        Some("Propulsion must be at the rear".into())
                    } else if cat == ModuleCategory::Crew {
                        Some("Crew quarters cannot be next to reactors".into())
                    } else {
                        Some("Position rule violated".into())
                    }
                }
                _ => Some("Position rule violated".into()),
            }
        } else {
            let cost = match &selection {
                BuildSelection::Hull(_) => build_state.hull_material.cost(),
                BuildSelection::Module(mt) => registry.get(*mt).cost,
            };
            Some(format!("Not enough credits (need {}c)", cost))
        };
    }
}

/// Auto-rotates a module to face outward from the submarine.
/// Checks the 4 cardinal directions from `grid_pos`; the direction with the
/// fewest occupied neighbors is considered "outward".  Ties are broken by
/// preferring the direction away from the submarine's center (0, 0).
fn auto_rotate(grid_pos: IVec2, occupancy: &GridOccupancy) -> Option<Rotation> {
    // Directions: (offset, Rotation that makes the module face that direction)
    let directions: [(IVec2, Rotation); 4] = [
        (IVec2::Y,     Rotation::North), // up
        (IVec2::NEG_Y, Rotation::South), // down
        (IVec2::X,     Rotation::East),  // right
        (IVec2::NEG_X, Rotation::West),  // left
    ];

    // Count how many of the 4 neighbors are occupied
    let neighbor_count: i32 = directions.iter()
        .map(|(off, _)| if occupancy.cells.contains_key(&(grid_pos + *off)) { 1 } else { 0 })
        .sum();

    // If no neighbors at all, can't determine orientation
    if neighbor_count == 0 {
        return None;
    }

    // For each direction, score it: prefer direction with NO neighbor (= outward edge)
    // then break ties by distance from center
    let mut best: Option<(Rotation, f32)> = None;
    for (off, rot) in &directions {
        let has_neighbor = occupancy.cells.contains_key(&(grid_pos + *off));
        if has_neighbor {
            continue; // This direction faces inward — skip
        }
        // Tie-break: prefer direction that points away from center
        let outward_score = (grid_pos.as_vec2() + off.as_vec2()).length();
        if best.map_or(true, |(_, s)| outward_score > s) {
            best = Some((*rot, outward_score));
        }
    }

    best.map(|(rot, _)| rot)
}

/// Checks positional rules for module placement
fn check_position_rules(
    selection: &BuildSelection,
    grid_pos: IVec2,
    module_query: &Query<&Module>,
) -> bool {
    match selection {
        BuildSelection::Hull(_) => true,
        BuildSelection::Module(mt) => {
            let cat = mt.category();
            match cat {
                // Propulsion: should be at the rear (rightmost x positions)
                ModuleCategory::Propulsion => {
                    let max_x = module_query.iter()
                        .filter(|m| m.module_type.category() != ModuleCategory::Propulsion)
                        .map(|m| m.grid_position.x)
                        .max();
                    max_x.map_or(true, |mx| grid_pos.x >= mx)
                }
                // Crew: not adjacent to power modules (heat/radiation)
                ModuleCategory::Crew => {
                    let adjacent_to_power = module_query.iter().any(|m| {
                        m.module_type.category() == ModuleCategory::Power
                            && (m.grid_position - grid_pos).as_vec2().length() < 1.5
                    });
                    !adjacent_to_power
                }
                _ => true,
            }
        }
    }
}

// ============================================================================
// PLACEMENT & REMOVAL INPUT
// ============================================================================

/// Handles placing new modules/hull via click
fn handle_module_placement(
    mouse: Res<Input<MouseButton>>,
    build_state: Res<BuildingState>,
    current_state: Res<State<BuildState>>,
    mut place_module_events: EventWriter<PlaceModuleRequest>,
    mut place_hull_events: EventWriter<PlaceHullRequest>,
    symmetry_state: Res<symmetry::SymmetryState>,
    occupancy: Res<GridOccupancy>,
) {
    if *current_state.get() != BuildState::Placing {
        return;
    }

    if mouse.just_pressed(MouseButton::Left) && build_state.is_valid_placement {
        let pos = build_state.ghost_position;
        let rot = build_state.rotation;

        match build_state.current_selection() {
            BuildSelection::Hull(layer) => {
                place_hull_events.send(PlaceHullRequest {
                    layer,
                    material: build_state.hull_material,
                    grid_position: pos,
                    free: false,
                });
                // Symmetry: mirror hull placement
                if symmetry_state.enabled {
                    let mirror_pos = symmetry::mirror_position(pos);
                    if mirror_pos != pos && !occupancy.cells.contains_key(&mirror_pos) {
                        place_hull_events.send(PlaceHullRequest {
                            layer,
                            material: build_state.hull_material,
                            grid_position: mirror_pos,
                            free: false,
                        });
                    }
                }
            }
            BuildSelection::Module(module_type) => {
                place_module_events.send(PlaceModuleRequest {
                    module_type,
                    grid_position: pos,
                    rotation: rot,
                    custom_name: None,
                    subcomponents: None,
                    free: false,
                });
                // Symmetry: mirror module placement
                if symmetry_state.enabled {
                    let mirror_pos = symmetry::mirror_position(pos);
                    let mirror_rot = symmetry::mirror_rotation(rot);
                    if mirror_pos != pos && !occupancy.cells.contains_key(&mirror_pos) {
                        place_module_events.send(PlaceModuleRequest {
                            module_type,
                            grid_position: mirror_pos,
                            rotation: mirror_rot,
                            custom_name: None,
                            subcomponents: None,
                            free: false,
                        });
                    }
                }
            }
        }
    }
}

/// Handles removing modules
fn handle_module_removal(
    mouse: Res<Input<MouseButton>>,
    build_state: Res<BuildingState>,
    current_state: Res<State<BuildState>>,
    occupancy: Res<GridOccupancy>,
    module_query: Query<(Entity, &Module)>,
    mut remove_events: EventWriter<RemoveModuleRequest>,
) {
    let state = *current_state.get();
    let in_deleting = state == BuildState::Deleting;
    let in_placing = state == BuildState::Placing;

    if !in_deleting && !in_placing {
        return;
    }

    let should_delete = (in_deleting && mouse.just_pressed(MouseButton::Left))
        || (in_placing && mouse.just_pressed(MouseButton::Right));

    if should_delete {
        // Use GridOccupancy to find the entity at the clicked cell
        // This works for any cell a multi-cell module occupies, not just origin
        if let Some(&entity) = occupancy.cells.get(&build_state.ghost_position) {
            if let Ok((_, module)) = module_query.get(entity) {
                // Protect last power source
                if module.module_type.category() == ModuleCategory::Power {
                    let power_count = module_query.iter()
                        .filter(|(_, m)| m.module_type.category() == ModuleCategory::Power)
                        .count();
                    if power_count <= 1 {
                        return;
                    }
                }
                remove_events.send(RemoveModuleRequest { module: entity });
            }
        }
    }
}

// ============================================================================
// EVENT PROCESSING
// ============================================================================

/// Processes PlaceHullRequest events
fn process_hull_placement(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut events: EventReader<PlaceHullRequest>,
    sub_query: Query<Entity, With<Submarine>>,
    mut notifications: EventWriter<ShowNotification>,
    mut currency: ResMut<Currency>,
) {
    let Ok(submarine) = sub_query.get_single() else { return };

    for event in events.iter() {
        let grid_pos = event.grid_position;
        let material = event.material;

        // Tint by layer type for visual distinction
        let color = match event.layer {
            HullLayer::Outer => Color::WHITE,
            HullLayer::Inner => Color::rgb(0.9, 0.9, 0.9),
            HullLayer::Void => Color::rgb(0.5, 0.5, 0.6),
            HullLayer::BulkheadDoor => Color::rgb(0.9, 0.8, 0.7),
        };

        let texture = asset_server.load(sprite_map::hull_sprite_path(material));

        commands.spawn((
            SpriteBundle {
                texture,
                sprite: Sprite {
                    color,
                    custom_size: Some(Vec2::new(64.0, 64.0)),
                    ..default()
                },
                transform: Transform::from_xyz(
                    grid_pos.x as f32 * 66.0,
                    grid_pos.y as f32 * 66.0 - 33.0,
                    0.1,
                ),
                ..default()
            },
            HullSegment {
                hull_layer: event.layer,
                material,
                radiation_shielding: material.radiation_shielding(),
                health: 100.0 * material.health_multiplier(),
                max_health: 100.0 * material.health_multiplier(),
                grid_position: grid_pos,
                ..default()
            },
        )).set_parent(submarine);

        let layer_name = match event.layer {
            HullLayer::Outer => "Outer Hull",
            HullLayer::Inner => "Inner Hull",
            HullLayer::Void => "Void Space",
            HullLayer::BulkheadDoor => "Bulkhead Door",
        };

        if !event.free {
            let cost = material.cost();
            currency.credits = currency.credits.saturating_sub(cost);

            notifications.send(ShowNotification {
                message: format!("Placed {} ({}) -{}c", layer_name, material.name(), cost),
                notification_type: NotificationType::Success,
                duration: 1.5,
            });
        }
    }
}

/// Processes PlaceModuleRequest events (registry-based)
fn process_module_placement(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut events: EventReader<PlaceModuleRequest>,
    sub_query: Query<Entity, With<Submarine>>,
    registry: Res<ModuleRegistry>,
    mut placed_events: EventWriter<ModulePlaced>,
    mut notifications: EventWriter<ShowNotification>,
    mut currency: ResMut<Currency>,
) {
    let Ok(submarine) = sub_query.get_single() else { return };

    for event in events.iter() {
        // Check if this is a custom module
        let entity = if let (Some(custom_name), Some(subcomponents)) = (&event.custom_name, &event.subcomponents) {
            // Spawn custom module with sub-components
            crate::submarine::spawn_custom_module(
                &mut commands,
                &asset_server,
                submarine,
                event.module_type,
                custom_name.clone(),
                event.grid_position,
                event.rotation,
                subcomponents.clone(),
                &registry,
            )
        } else {
            // Spawn regular module
            spawn_module(
                &mut commands,
                &asset_server,
                submarine,
                event.module_type,
                event.grid_position,
                event.rotation,
                &registry,
            )
        };

        placed_events.send(ModulePlaced {
            module: entity,
            module_type: event.module_type,
            grid_position: event.grid_position,
        });

        if !event.free {
            let cost = registry.get(event.module_type).cost;
            currency.credits = currency.credits.saturating_sub(cost);

            let message = if event.custom_name.is_some() {
                format!("Placed Custom {} -{}c", event.module_type.name(), cost)
            } else {
                format!("Placed {} -{}c", event.module_type.name(), cost)
            };

            notifications.send(ShowNotification {
                message,
                notification_type: NotificationType::Success,
                duration: 1.5,
            });
        }
    }
}

/// Processes RemoveModuleRequest events
fn process_module_removal(
    mut commands: Commands,
    mut events: EventReader<RemoveModuleRequest>,
    module_query: Query<&Module>,
    mut removed_events: EventWriter<ModuleRemoved>,
    mut notifications: EventWriter<ShowNotification>,
    mut currency: ResMut<Currency>,
    registry: Res<ModuleRegistry>,
) {
    for event in events.iter() {
        if let Ok(module) = module_query.get(event.module) {
            let cost = registry.get(module.module_type).cost;
            let refund = (cost as f32 * 0.75) as u32;
            currency.credits += refund;

            removed_events.send(ModuleRemoved {
                module_type: module.module_type,
                grid_position: module.grid_position,
            });

            notifications.send(ShowNotification {
                message: format!("Removed {} +{}c refund", module.module_type.name(), refund),
                notification_type: NotificationType::Warning,
                duration: 1.5,
            });

            commands.entity(event.module).despawn_recursive();
        }
    }
}

// ============================================================================
// CUSTOM MODULE STAT RECALCULATION
// ============================================================================

/// Recalculates stats for custom modules when their sub-components change
fn recalculate_custom_module_stats(
    mut commands: Commands,
    changed_modules: Query<
        (Entity, &CustomModule, &Children),
        Or<(Changed<CustomModule>, Changed<Children>)>
    >,
    subcomponent_query: Query<&SubComponent>,
    registry: Res<ModuleRegistry>,
) {
    for (entity, custom_module, children) in changed_modules.iter() {
        // Collect all sub-component types from children
        let subcomponents: Vec<SubComponentType> = children.iter()
            .filter_map(|&child| subcomponent_query.get(child).ok())
            .map(|sc| sc.subcomponent_type.clone())
            .collect();

        // Get base stats from registry
        let module_def = registry.get(custom_module.base_type);
        let base_stats = &module_def.base_stats;

        // Calculate new stats
        let calculated = StatCalculator::calculate_stats(
            custom_module.base_type,
            &subcomponents,
            base_stats,
        );

        // Insert or update CalculatedStats component
        commands.entity(entity).insert(calculated);
    }
}

/// Syncs CalculatedStats weapon data back to the Weapon component (max_ammo, clamped ammo).
fn sync_calculated_to_weapon(
    mut weapon_query: Query<(&mut Weapon, &CalculatedStats), Changed<CalculatedStats>>,
) {
    for (mut weapon, calculated) in weapon_query.iter_mut() {
        if let Some(ref ws) = calculated.weapon {
            weapon.max_ammo = ws.max_ammo;
            weapon.ammo = weapon.ammo.min(ws.max_ammo);
        }
    }
}
