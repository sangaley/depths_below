use bevy::prelude::*;
use std::collections::HashMap;
use smallvec::SmallVec;
use crate::states::{GameState, BuildState};
use crate::components::*;
use crate::resources::*;
use crate::events::*;
use crate::ship::spawn_module;
use crate::sprite_map;

pub mod customization;
pub mod armour;
pub mod footprints;
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
            .init_state::<BuildState>()
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
            .init_resource::<customization::custom_presets::CustomPresetLibrary>()
            .add_systems(Startup, customization::custom_presets::load_custom_presets)
            // Weapon tuning → live stats. Runs in every state (Changed-filtered,
            // so it's a no-op unless a slider moved or a save just loaded) —
            // tuned stats must persist into flight, only EDITING is dock-gated.
            .add_systems(Update, customization::tuning::apply_weapon_tuning)
            .add_systems(
                Update,
                (
                    update_grid_occupancy,
                    handle_build_input,
                    update_ghost_preview,
                    handle_module_placement,
                    handle_module_removal,
                    blueprint::save_blueprint_system,
                    blueprint::load_blueprint_system,
                    blueprint::delete_blueprint_system,
                    // Weapon customization (right-click inspect/Tier2/Tier3)
                    // shelved for now — revisit once most stations/bases are
                    // done. inspection::right_click_inspect and
                    // handle_customize_click still exist, just not wired in.
                )
                    .chain()
                    .run_if(in_state(GameState::StationDocked)),
            )
            // Placement/removal event processors also run while EXPLORING —
            // the ghost-rebuild system (ship::rebuild) respawns destroyed
            // blocks in flight via the same PlaceHullRequest /
            // PlaceModuleRequest events the build mode uses.
            .add_systems(
                Update,
                (
                    process_hull_placement,
                    process_module_placement,
                    process_module_removal,
                    process_hull_removal,
                    // Last in the chain so the grids reflect this frame's
                    // placements and removals, not the previous frame's.
                    update_ship_grids,
                )
                    .chain()
                    .run_if(in_state(GameState::StationDocked)
                        .or_else(in_state(GameState::Exploring))),
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
                    // Must run after apply_machine_stats_to_weapons or the two
                    // systems race to write Weapon.damage/range/fire_rate with
                    // no defined order — see apply_weapon_enhancers' doc comment.
                    // Runs while DOCKED too (not just Exploring) so the tuning
                    // window's live stat readout reflects the composed result,
                    // and folds WeaponTuning in during its per-frame reset.
                    multiblock::enhancers::apply_weapon_enhancers
                        .after(multiblock::stats::apply_machine_stats_to_weapons),
                    // Cooldown duration follows the final composed fire_rate.
                    customization::tuning::sync_weapon_cooldowns
                        .after(multiblock::enhancers::apply_weapon_enhancers),
                    multiblock::damage::process_block_destruction,
                ).run_if(in_state(GameState::StationDocked)
                    .or_else(in_state(GameState::Exploring))),
            )
            // Enhancer effects (separate system group to stay under tuple limit)
            .add_systems(
                Update,
                (
                    multiblock::enhancers::apply_hull_enhancers,
                    // Must run after update_ship_state recomputes noise_level
                    // from scratch each frame, or the SignalJammer reduction
                    // below gets silently overwritten depending on schedule
                    // order (both run under GameState::Exploring).
                    multiblock::enhancers::apply_utility_enhancers
                        .after(crate::ship::update_ship_state),
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
                    build_history::undo_input,
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

/// Per-ship map from ship-local grid cell to the live block sitting in it.
///
/// Grid coordinates are ship-local, so a single global map can only ever
/// describe ONE ship — an AI ship's (1,0) and the player's (1,0) are
/// different blocks that collide on the same key. That restriction is why
/// `GridOccupancy` is player-scoped, and why several systems exist twice
/// (`queue_detonation`/`queue_ai_detonation`,
/// `trigger_chain_reactions`/`ai_chain_reactions`): the AI copy had to
/// re-derive in world space what the player copy reads off the grid.
///
/// Two deliberate differences from `GridOccupancy`:
/// - **Live blocks only.** Destroyed modules and hull are dropped, so a
///   dead plate doesn't keep stopping shells.
/// - **Maintained in flight,** not just at dock. Blocks get shot off during
///   combat, and hit resolution walks this map while that's happening.
#[derive(Component, Default)]
pub struct ShipGrid {
    pub cells: HashMap<IVec2, Entity>,
}

/// Per-block hit-resolution data, ADDITIVE alongside Module/HullSegment
/// (same pattern as ShipGrid). `ShipGrid` answers "which block is at this
/// cell"; `Block` answers "what does a round meet when it gets there".
/// Health deliberately stays on Module/HullSegment — two sources of truth
/// is worse than one awkward query.
///
/// `thickness` is in the same units as ammo penetration (0-90): hull seeds
/// from HullMaterial::damage_absorption() (15/30/50/80), modules are
/// unarmoured internals at 0. `slope` (0 = flat plate) is reserved for
/// angled armour — nothing reads it yet.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub cell: IVec2,
    pub kind: BlockKind,
    pub thickness: f32,
    /// Outward normal of the block's armoured face, in ship-local radians.
    ///
    /// `None` means the block is a plain box: it has no face of its own, so
    /// hit resolution uses whichever cell face the round arrived through.
    /// That is the overwhelming majority of blocks, and it's what makes an
    /// unsloped hull behave exactly as it did before slope existed.
    ///
    /// `Some(angle)` is a block whose SHAPE declares a facing — a wedge
    /// presents the same diagonal slab whichever side you approach from, so
    /// the normal is fixed in the hull's frame rather than derived per-face.
    pub facing: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Hull,
    Module,
}

impl Block {
    pub fn hull(cell: IVec2, material: HullMaterial) -> Self {
        Self { cell, kind: BlockKind::Hull, thickness: material.damage_absorption(), facing: None }
    }

    /// A module with no armour value — machinery, which is what most modules
    /// are. Also the fallback when a struck block has no Block component.
    pub fn module(cell: IVec2) -> Self {
        Self { cell, kind: BlockKind::Module, thickness: 0.0, facing: None }
    }

    /// A module as actually spawned: plating modules carry real armour and
    /// wedges carry a declared diagonal face.
    pub fn for_module(cell: IVec2, module_type: ModuleType, rotation: Rotation) -> Self {
        Self {
            cell,
            kind: BlockKind::Module,
            thickness: module_thickness(module_type),
            facing: module_facing(module_type, rotation),
        }
    }
}

/// Armour rating of a module, in the same units as
/// `HullMaterial::damage_absorption` (Steel 15 … Abyssal Alloy 80).
///
/// Almost every module is machinery and armours nothing — an exposed reactor
/// should eat the round. The plating modules are the exception, and they're
/// deliberately rated BELOW their raw HP suggests: a wedge trades material for
/// angle, so it deflects better than a flat plate and stops less.
fn module_thickness(module_type: ModuleType) -> f32 {
    match module_type {
        ModuleType::StaggeredArmorPlate => 45.0, // premium, 6 cells, no straight seam
        ModuleType::ArmorPlate | ModuleType::CornerArmorPlate => 40.0,
        ModuleType::AblativeArmor => 35.0,       // spends itself instead of the hull
        ModuleType::AngledArmorPlate => 30.0,    // less steel than a flat plate...
        ModuleType::HullReinforcePlate => 30.0,
        ModuleType::AngledHullPlate => 15.0,     // ...and this one is only framing
        _ => 0.0,
    }
}

/// Fixed outward normal for blocks whose shape declares one.
///
/// The wedge sprites (`vfx::block_visuals`) put their material along two
/// adjacent edges and expose the diagonal between them, so at `North` the
/// face looks out to the north-east — 45°. Each rotation carries it round to
/// the next corner, which is what `R` in build mode has always been picking;
/// it just didn't mean anything until now.
fn module_facing(module_type: ModuleType, rotation: Rotation) -> Option<f32> {
    match module_type {
        ModuleType::AngledArmorPlate | ModuleType::AngledHullPlate => {
            Some(std::f32::consts::FRAC_PI_4 + rotation.to_radians())
        }
        _ => None,
    }
}

/// One occupied cell met by `ShipGrid::walk`, in path order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridStep {
    pub entity: Entity,
    pub cell: IVec2,
    /// Distance along the walked segment at which the round entered this cell,
    /// in cell widths. With `span`, this reconstructs the entry and exit
    /// points — which is what a sub-cell shape test needs to clip against.
    pub t_enter: f32,
    /// Which side of the cell the ray came in through, as the unit offset
    /// from this cell toward the cell it arrived from (`(-1, 0)` = entered
    /// through the -x face). `(0, 0)` for the cell the walk started inside.
    pub entry_face: IVec2,
    /// Path length through the cell in cell widths — the line-of-sight
    /// thickness. 1.0 for a straight pass, sqrt(2) corner to corner. This is
    /// the number armour math should use directly; don't rebuild it as
    /// T / cos(theta).
    pub span: f32,
}

/// Bound on cells visited per walk. Exhaustion is a miss: a degenerate
/// direction must not loop, and nothing the game fires crosses 24 cells
/// (1,584 units) between two frames.
pub const MAX_WALK_STEPS: usize = 24;

impl ShipGrid {
    /// Swept walk from `from` to `to` (ship-local CELL coordinates — block
    /// centres sit on integers, cell (gx, gy) spans gx±0.5) yielding every
    /// live block the segment passes through, nearest first.
    ///
    /// Amanatides-Woo DDA, not Bresenham: Bresenham visits one cell per
    /// column and skips the diagonal corner cell, so a shell at the right
    /// angle would pass through solid armour. Consecutive steps through the
    /// same entity (multi-cell modules) are merged into one step whose span
    /// is the total path through that block.
    pub fn walk(&self, from: Vec2, to: Vec2) -> SmallVec<[GridStep; 8]> {
        let mut out = SmallVec::new();
        let delta = to - from;
        let len = delta.length();
        if !len.is_finite() {
            return out;
        }
        // Degenerate segment: a point test of the cell we're standing in.
        if len < 1e-4 {
            let cell = IVec2::new(from.x.round() as i32, from.y.round() as i32);
            if let Some(entity) = self.get(cell) {
                out.push(GridStep { entity, cell, entry_face: IVec2::ZERO, t_enter: 0.0, span: 0.0 });
            }
            return out;
        }
        let dir = delta / len;

        // Shift by half a cell so boundaries sit on integers.
        let p = from + Vec2::splat(0.5);
        let mut cell = IVec2::new(p.x.floor() as i32, p.y.floor() as i32);
        let step = IVec2::new(dir.x.signum() as i32 * (dir.x != 0.0) as i32,
                              dir.y.signum() as i32 * (dir.y != 0.0) as i32);
        let axis_t = |d: f32, pos: f32, c: i32, s: i32| -> (f32, f32) {
            if s == 0 {
                (f32::INFINITY, f32::INFINITY)
            } else {
                let boundary = if s > 0 { c as f32 + 1.0 } else { c as f32 };
                ((boundary - pos) / d, 1.0 / d.abs())
            }
        };
        let (mut t_max_x, t_delta_x) = axis_t(dir.x, p.x, cell.x, step.x);
        let (mut t_max_y, t_delta_y) = axis_t(dir.y, p.y, cell.y, step.y);

        let mut t_enter = 0.0_f32;
        let mut entry_face = IVec2::ZERO;
        for _ in 0..MAX_WALK_STEPS {
            let t_exit = t_max_x.min(t_max_y).min(len);
            let span = t_exit - t_enter;
            // A zero-span cell is a corner graze, not a pass-through.
            if span > 1e-6 {
                if let Some(entity) = self.get(cell) {
                    match out.last_mut() {
                        Some(last) if last.entity == entity => last.span += span,
                        _ => out.push(GridStep { entity, cell, entry_face, t_enter, span }),
                    }
                }
            }
            if t_exit >= len {
                break;
            }
            if t_max_x < t_max_y {
                cell.x += step.x;
                t_enter = t_max_x;
                t_max_x += t_delta_x;
                entry_face = IVec2::new(-step.x, 0);
            } else {
                cell.y += step.y;
                t_enter = t_max_y;
                t_max_y += t_delta_y;
                entry_face = IVec2::new(0, -step.y);
            }
        }
        out
    }

    /// Live block occupying a cell, if any.
    pub fn get(&self, cell: IVec2) -> Option<Entity> {
        self.cells.get(&cell).copied()
    }

    /// Whether any live block occupies a cell.
    pub fn contains(&self, cell: IVec2) -> bool {
        self.cells.contains_key(&cell)
    }

    /// Get all grid cells a module occupies given origin, size, and rotation.
    /// Uses SmallVec to avoid heap allocation for modules up to 2x2.
    ///
    /// `footprint` overrides the plain WxH rectangle with an explicit set of
    /// relative offsets (see `footprints::footprint_override`) for modules
    /// with a non-rectangular shape. `None` reproduces the original
    /// rectangle behavior exactly.
    pub fn cells_for(origin: IVec2, size: IVec2, rotation: Rotation, footprint: Option<&[IVec2]>) -> SmallVec<[IVec2; 4]> {
        let mut cells = SmallVec::new();
        match footprint {
            Some(offsets) => {
                for &offset in offsets {
                    cells.push(origin + rotation.rotate_offset(offset));
                }
            }
            None => {
                for x in 0..size.x {
                    for y in 0..size.y {
                        let offset = rotation.rotate_offset(IVec2::new(x, y));
                        cells.push(origin + offset);
                    }
                }
            }
        }
        cells
    }
}

impl GridOccupancy {
    /// Cells a module occupies. Same footprint maths as `ShipGrid::cells_for`
    /// — kept here so the ~8 static callers still compile while the grid
    /// migrates to the per-ship component.
    pub fn cells_for(origin: IVec2, size: IVec2, rotation: Rotation, footprint: Option<&[IVec2]>) -> SmallVec<[IVec2; 4]> {
        ShipGrid::cells_for(origin, size, rotation, footprint)
    }

    /// Check if all cells for a module placement are free
    pub fn can_place(&self, origin: IVec2, size: IVec2, rotation: Rotation, footprint: Option<&[IVec2]>) -> bool {
        for cell in Self::cells_for(origin, size, rotation, footprint) {
            if self.cells.contains_key(&cell) {
                return false;
            }
        }
        true
    }
}

/// Rebuilds grid occupancy from the PLAYER ship's modules and hull segments.
/// Skips rebuild when entity count hasn't changed (cheap change detection).
///
/// Scoped to the player ship only: AI ships reuse the same local grid
/// coordinates (their modules also sit at positions like (1,0)), so mixing
/// them into one global map made an AI ship's explosion at *its* (1,0)
/// damage the player's module at (1,0). Grid coordinates are only
/// meaningful per-ship.
fn update_grid_occupancy(
    module_query: Query<(Entity, &Module), Or<(Changed<Module>, Added<Module>)>>,
    hull_query: Query<(Entity, &HullSegment, &Transform), Or<(Changed<HullSegment>, Added<HullSegment>)>>,
    all_modules: Query<(Entity, &Module, &ChildOf)>,
    all_hulls: Query<(Entity, &HullSegment, &Transform, &ChildOf)>,
    ship_query: Query<Entity, With<Ship>>,
    mut occupancy: ResMut<GridOccupancy>,
    mut last_count: Local<usize>,
) {
    let Ok(player_ship) = ship_query.single() else { return };

    let current_count = all_modules.iter().count() + all_hulls.iter().count();
    let has_changes = !module_query.is_empty() || !hull_query.is_empty();
    if current_count == *last_count && !occupancy.cells.is_empty() && !has_changes {
        return;
    }
    *last_count = current_count;

    occupancy.cells.clear();

    for (entity, module, parent) in all_modules.iter() {
        if parent.parent() != player_ship { continue; }
        let footprint = footprints::footprint_override(module.module_type);
        let cells = GridOccupancy::cells_for(module.grid_position, module.size, module.rotation, footprint);
        for cell in cells {
            occupancy.cells.insert(cell, entity);
        }
    }

    for (entity, _hull, transform, parent) in all_hulls.iter() {
        if parent.parent() != player_ship { continue; }
        let grid = rooms::transform_to_grid(transform);
        occupancy.cells.insert(grid, entity);
    }
}

/// Rebuilds every ship's `ShipGrid` from its own live blocks.
///
/// Runs in flight as well as at dock, unlike `update_grid_occupancy`: hit
/// resolution walks these maps while blocks are being shot off, and a grid
/// that only refreshed at a station would send shells into cells whose
/// block died two fights ago.
///
/// Gated on block counts changing. Counting is a cheap archetype iteration
/// with no hashing; the rebuild underneath it is ~2k HashMap inserts, so
/// the check pays for itself on every frame nothing is destroyed (which is
/// nearly all of them).
pub fn update_ship_grids(
    mut commands: Commands,
    ships: Query<Entity, Or<(With<Ship>, With<crate::ai_ship::components::AiShip>)>>,
    modules: Query<(Entity, &Module, &ChildOf), Without<DestroyedModule>>,
    hulls: Query<(Entity, &Transform, &ChildOf), (With<HullSegment>, Without<HullDestroyed>)>,
    mut grids: Query<&mut ShipGrid>,
    mut last_counts: Local<(usize, usize, usize)>,
) {
    let counts = (modules.iter().count(), hulls.iter().count(), ships.iter().count());
    if counts == *last_counts && !grids.is_empty() {
        return;
    }
    *last_counts = counts;

    // Bucket live blocks by the ship that owns them. Pre-seeding every ship
    // means a vessel that just lost its last block still gets an empty grid
    // written rather than keeping a stale one.
    let mut per_ship: HashMap<Entity, HashMap<IVec2, Entity>> = ships
        .iter()
        .map(|ship| (ship, HashMap::new()))
        .collect();

    for (entity, module, parent) in modules.iter() {
        let Some(cells) = per_ship.get_mut(&parent.parent()) else { continue };
        let footprint = footprints::footprint_override(module.module_type);
        for cell in ShipGrid::cells_for(module.grid_position, module.size, module.rotation, footprint) {
            cells.insert(cell, entity);
        }
    }

    // Hull is always 1x1, and spawns at `grid * 66.0` (y offset -33.0) on
    // player and AI ships alike, so the transform inverts cleanly. Modules
    // can't use this path: multi-cell ones are centred on their footprint
    // centroid, not on their origin cell.
    for (entity, transform, parent) in hulls.iter() {
        let Some(cells) = per_ship.get_mut(&parent.parent()) else { continue };
        cells.insert(rooms::transform_to_grid(transform), entity);
    }

    for (ship, cells) in per_ship {
        match grids.get_mut(ship) {
            Ok(mut grid) => grid.cells = cells,
            Err(_) => { commands.entity(ship).try_insert(ShipGrid { cells }); }
        }
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
    keyboard: Res<ButtonInput<KeyCode>>,
    mut build_state: ResMut<BuildingState>,
    mut next_state: ResMut<NextState<BuildState>>,
    current_state: Res<State<BuildState>>,
    mut customization_state: ResMut<CustomizationState>,
    mut placement_state: ResMut<ComponentPlacementState>,
    registry: Res<ModuleRegistry>,
    unlocks: Res<crate::resources::Unlocks>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    // B: Toggle build mode
    if keyboard.just_pressed(KeyCode::KeyB) {
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
    if keyboard.just_pressed(KeyCode::KeyR) {
        build_state.rotation = build_state.rotation.rotate_cw();
        build_state.auto_rotated = false;
        info!("Rotation: {:?}", build_state.rotation);
    }

    // M: Cycle hull material (only in Hull category), skipping locked materials
    if keyboard.just_pressed(KeyCode::KeyM) {
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
            notifications.write(ShowNotification {
                message: "No other hull materials unlocked yet.".into(),
                notification_type: NotificationType::Warning,
                duration: 3.0,
            });
        }
        info!("Material: {} ({:.0}m)", build_state.hull_material.name(), build_state.hull_material.radiation_shielding());
    }

    // X: Toggle deletion mode
    if keyboard.just_pressed(KeyCode::KeyX) {
        match current_state.get() {
            BuildState::Deleting => next_state.set(BuildState::Placing),
            _ => next_state.set(BuildState::Deleting),
        }
    }

    // G: Open customization panel for current selection (if customizable)
    if keyboard.just_pressed(KeyCode::KeyG) {
        if let BuildSelection::Module(module_type) = build_state.current_selection() {
            let module_def = registry.get(module_type);
            if module_def.customizable {
                customization_state.start_customizing(module_type);
                notifications.write(ShowNotification {
                    message: format!("⚙ Quick Customizing {}", module_type.name()),
                    notification_type: NotificationType::Info,
                    duration: 2.0,
                });
            } else {
                notifications.write(ShowNotification {
                    message: format!("{} is not customizable", module_type.name()),
                    notification_type: NotificationType::Info,
                    duration: 1.5,
                });
            }
        }
    }

    // P: Open component placement panel for current selection (if customizable)
    if keyboard.just_pressed(KeyCode::KeyP) {
        if let BuildSelection::Module(module_type) = build_state.current_selection() {
            let module_def = registry.get(module_type);
            if module_def.customizable {
                placement_state.start_placing(module_type);
                next_state.set(BuildState::PlacingComponent);
                notifications.write(ShowNotification {
                    message: format!("🔧 Component Builder: {} - Click pieces to assemble", module_type.name()),
                    notification_type: NotificationType::Info,
                    duration: 3.0,
                });
            } else {
                notifications.write(ShowNotification {
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

/// Converts the mouse cursor to a ship-local grid cell.
///
/// Grid coordinates are ship-local (hull/module tiles are children of
/// the ship, positioned relative to it — see spawn_module /
/// process_hull_placement / rooms::transform_to_grid, all of which
/// use `grid_y * 66 - 33` as the local Y). The cursor position from
/// viewport_to_world_2d is in WORLD space, so it has to be
/// transformed into the ship's local space first — dividing world
/// coordinates directly by grid_size only produces the right cell when
/// the ship happens to be sitting exactly at world origin with zero
/// rotation, which is essentially never true once you've actually flown
/// anywhere. Every cursor→grid conversion must go through here.
pub fn cursor_to_ship_grid(
    window: &Window,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    ship_gt: &GlobalTransform,
) -> Option<IVec2> {
    let cursor_pos = window.cursor_position()
        .and_then(|p| camera.viewport_to_world_2d(camera_transform, p).ok())?;
    let grid_size = 66.0;
    let cursor_world = Vec3::new(cursor_pos.x, cursor_pos.y, 0.0);
    let local = ship_gt.rotation().inverse() * (cursor_world - ship_gt.translation());
    Some(IVec2::new(
        (local.x / grid_size).round() as i32,
        ((local.y + 33.0) / grid_size).round() as i32,
    ))
}

/// Updates ghost position and validates placement.
/// Tracks mouse in both Placing and Deleting modes.
fn update_ghost_preview(
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    ship_query: Query<&GlobalTransform, (With<Ship>, Without<Camera>)>,
    mut build_state: ResMut<BuildingState>,
    current_state: Res<State<BuildState>>,
    occupancy: Res<GridOccupancy>,
    // Player modules only — wrecks keep real Module children since the
    // destruction rework, and their local grid coords poisoned the
    // positional rules (engines became unplaceable anywhere).
    module_query: Query<&Module, Without<crate::ai_ship::components::OwnedByAiShip>>,
    hull_query: Query<(&HullSegment, &Transform, &ChildOf)>,
    registry: Res<ModuleRegistry>,
    currency: Res<Currency>,
) {
    // Track mouse position in both Placing and Deleting modes
    let state = *current_state.get();
    if state != BuildState::Placing && state != BuildState::Deleting {
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Ok((camera, camera_transform)) = camera_query.single() else { return };
    let Ok(ship_gt) = ship_query.single() else { return };

    if let Some(grid_pos) = cursor_to_ship_grid(window, camera, camera_transform, ship_gt) {
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
        let footprint = match selection {
            BuildSelection::Hull(_) => None,
            BuildSelection::Module(mt) => footprints::footprint_override(mt),
        };

        // Block limit check (250 max)
        let block_count = module_query.iter().count() + hull_query.iter().count();
        let under_limit = block_count < crate::combat::limits::MAX_SHIP_BLOCKS;

        // Check overlap using GridOccupancy (supports multi-cell)
        let no_overlap = occupancy.can_place(grid_pos, size, rotation, footprint);

        // Adjacency check - at least one cell of the new module must be adjacent
        // to an existing module or hull segment
        let placement_cells = GridOccupancy::cells_for(grid_pos, size, rotation, footprint);
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

        let valid = no_overlap && (has_neighbor || is_first) && position_ok && can_afford && multiblock_ok && under_limit;
        build_state.is_valid_placement = valid;
        build_state.placement_reason = if valid {
            None
        } else if !no_overlap {
            Some("Overlaps existing module or hull".into())
        } else if !has_neighbor && !is_first {
            Some("Must be adjacent to existing structure".into())
        } else if !under_limit {
            Some(format!("Block limit reached ({}/{})", block_count, crate::combat::limits::MAX_SHIP_BLOCKS))
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

/// Auto-rotates a module to face outward from the ship.
/// Checks the 4 cardinal directions from `grid_pos`; the direction with the
/// fewest occupied neighbors is considered "outward".  Ties are broken by
/// preferring the direction away from the ship's center (0, 0).
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
    module_query: &Query<&Module, Without<crate::ai_ship::components::OwnedByAiShip>>,
) -> bool {
    match selection {
        BuildSelection::Hull(_) => true,
        BuildSelection::Module(mt) => {
            let cat = mt.category();
            match cat {
                // Propulsion: at the rear. The ship builds nose-right —
                // the starter vessel's engines sit at the LEFTMOST x —
                // so rear means minimum x, not maximum (the old check
                // was backwards and rejected every engine placement).
                ModuleCategory::Propulsion => {
                    let min_x = module_query.iter()
                        .filter(|m| m.module_type.category() != ModuleCategory::Propulsion)
                        .map(|m| m.grid_position.x)
                        .min();
                    min_x.map_or(true, |mn| grid_pos.x <= mn)
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

/// Handles placing new modules/hull via click.
/// Hull can also be painted by holding the button and dragging — one block
/// per cell the ghost passes through. Modules stay click-per-place so a drag
/// can't accidentally buy three reactors.
fn handle_module_placement(
    mouse: Res<ButtonInput<MouseButton>>,
    build_state: Res<BuildingState>,
    current_state: Res<State<BuildState>>,
    mut place_module_events: MessageWriter<PlaceModuleRequest>,
    mut place_hull_events: MessageWriter<PlaceHullRequest>,
    symmetry_state: Res<symmetry::SymmetryState>,
    occupancy: Res<GridOccupancy>,
    mut last_painted: Local<Option<IVec2>>,
) {
    if *current_state.get() != BuildState::Placing {
        return;
    }

    if !mouse.pressed(MouseButton::Left) {
        *last_painted = None;
    }

    // TEMP [DEBUG_BUILD]: diagnosing a report of placement silently failing
    // after returning from combat. Remove once root-caused.
    if mouse.just_pressed(MouseButton::Left) {
        info!(
            "[DEBUG_BUILD] click at grid_pos={:?} rotation={:?} selection={:?} valid={} reason={:?} occupancy_len={}",
            build_state.ghost_position, build_state.rotation, build_state.current_selection(),
            build_state.is_valid_placement, build_state.placement_reason, occupancy.cells.len()
        );
    }

    let is_hull = matches!(build_state.current_selection(), BuildSelection::Hull(_));
    let drag_paint = is_hull
        && mouse.pressed(MouseButton::Left)
        && *last_painted != Some(build_state.ghost_position);

    if (mouse.just_pressed(MouseButton::Left) || drag_paint) && build_state.is_valid_placement {
        let pos = build_state.ghost_position;
        let rot = build_state.rotation;
        *last_painted = Some(pos);

        match build_state.current_selection() {
            BuildSelection::Hull(layer) => {
                place_hull_events.write(PlaceHullRequest {
                    layer,
                    material: build_state.hull_material,
                    grid_position: pos,
                    free: false,
                });
                // Symmetry: mirror hull placement
                if symmetry_state.enabled {
                    let mirror_pos = symmetry::mirror_position(pos);
                    if mirror_pos != pos && !occupancy.cells.contains_key(&mirror_pos) {
                        place_hull_events.write(PlaceHullRequest {
                            layer,
                            material: build_state.hull_material,
                            grid_position: mirror_pos,
                            free: false,
                        });
                    }
                }
            }
            BuildSelection::Module(module_type) => {
                place_module_events.write(PlaceModuleRequest {
                    module_type,
                    grid_position: pos,
                    rotation: rot,
                    custom_name: None,
                    subcomponents: None,
            extras: None,
                    free: false,
                });
                // Symmetry: mirror module placement
                if symmetry_state.enabled {
                    let mirror_pos = symmetry::mirror_position(pos);
                    let mirror_rot = symmetry::mirror_rotation(rot);
                    if mirror_pos != pos && !occupancy.cells.contains_key(&mirror_pos) {
                        place_module_events.write(PlaceModuleRequest {
                            module_type,
                            grid_position: mirror_pos,
                            rotation: mirror_rot,
                            custom_name: None,
                            subcomponents: None,
            extras: None,
                            free: false,
                        });
                    }
                }
            }
        }
    }
}

/// Handles removing modules.
/// In delete mode the button can be held and dragged to sweep away several
/// blocks — one removal per cell entered. Right-click removal while placing
/// stays single-click.
fn handle_module_removal(
    mouse: Res<ButtonInput<MouseButton>>,
    build_state: Res<BuildingState>,
    current_state: Res<State<BuildState>>,
    occupancy: Res<GridOccupancy>,
    module_query: Query<(Entity, &Module)>,
    hull_query: Query<Entity, With<HullSegment>>,
    mut remove_events: MessageWriter<RemoveModuleRequest>,
    mut remove_hull_events: MessageWriter<RemoveHullRequest>,
    mut last_deleted: Local<Option<IVec2>>,
) {
    let state = *current_state.get();
    let in_deleting = state == BuildState::Deleting;
    let in_placing = state == BuildState::Placing;

    if !in_deleting && !in_placing {
        return;
    }

    if !mouse.pressed(MouseButton::Left) {
        *last_deleted = None;
    }

    let drag_delete = in_deleting
        && mouse.pressed(MouseButton::Left)
        && *last_deleted != Some(build_state.ghost_position);

    let should_delete = drag_delete
        || (in_placing && mouse.just_pressed(MouseButton::Right));

    if should_delete {
        if in_deleting {
            *last_deleted = Some(build_state.ghost_position);
        }
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
                remove_events.write(RemoveModuleRequest { module: entity });
            } else if hull_query.get(entity).is_ok() {
                remove_hull_events.write(RemoveHullRequest { hull: entity });
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
    mut events: MessageReader<PlaceHullRequest>,
    ship_query: Query<Entity, With<Ship>>,
    mut notifications: MessageWriter<ShowNotification>,
    mut currency: ResMut<Currency>,
    mut history: ResMut<build_history::BuildHistory>,
) {
    let Ok(ship) = ship_query.single() else { return };

    for event in events.read() {
        let grid_pos = event.grid_position;
        let material = event.material;

        // Tint by layer type for visual distinction
        let color = match event.layer {
            HullLayer::Outer => Color::WHITE,
            HullLayer::Inner => Color::srgb(0.9, 0.9, 0.9),
            HullLayer::Void => Color::srgb(0.5, 0.5, 0.6),
            HullLayer::BulkheadDoor => Color::srgb(0.9, 0.8, 0.7),
        };

        let texture = asset_server.load(sprite_map::hull_sprite_path(material));

        let hull_entity = commands.spawn((
            (Sprite {
                    image: texture,
                    color,
                    custom_size: Some(Vec2::new(64.0, 64.0)),
                    ..default()
                }, Transform::from_xyz(
                    grid_pos.x as f32 * 66.0,
                    grid_pos.y as f32 * 66.0 - 33.0,
                    0.1,
                )),
            BaseSpriteColor(color),
            BaseHullStats {
                max_health: 100.0 * material.health_multiplier(),
                radiation_shielding: material.radiation_shielding(),
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
            Block::hull(grid_pos, material),
        )).insert(ChildOf(ship)).id();

        let layer_name = match event.layer {
            HullLayer::Outer => "Outer Hull",
            HullLayer::Inner => "Inner Hull",
            HullLayer::Void => "Void Space",
            HullLayer::BulkheadDoor => "Bulkhead Door",
        };

        if !event.free {
            let cost = material.cost();
            currency.credits = currency.credits.saturating_sub(cost);
            history.record(build_history::BuildAction::PlaceHull {
                entity: hull_entity,
                material,
                cost,
            });

            notifications.write(ShowNotification {
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
    mut events: MessageReader<PlaceModuleRequest>,
    ship_query: Query<Entity, With<Ship>>,
    registry: Res<ModuleRegistry>,
    mut placed_events: MessageWriter<ModulePlaced>,
    mut notifications: MessageWriter<ShowNotification>,
    mut currency: ResMut<Currency>,
    mut history: ResMut<build_history::BuildHistory>,
) {
    let Ok(ship) = ship_query.single() else { return };

    for event in events.read() {
        // Check if this is a custom module
        let entity = if let (Some(custom_name), Some(subcomponents)) = (&event.custom_name, &event.subcomponents) {
            // Spawn custom module with ship-components
            crate::ship::spawn_custom_module(
                &mut commands,
                &asset_server,
                ship,
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
                ship,
                event.module_type,
                event.grid_position,
                event.rotation,
                &registry,
            )
        };

        // Restore design state (tuning, fire group, ammo) if the request
        // carried any — blueprint loads and ghost rebuilds do.
        if let Some(extras) = &event.extras {
            blueprint::apply_module_extras(&mut commands, entity, extras);
        }

        placed_events.write(ModulePlaced {
            module: entity,
            module_type: event.module_type,
            grid_position: event.grid_position,
        });

        if !event.free {
            let cost = registry.get(event.module_type).cost;
            currency.credits = currency.credits.saturating_sub(cost);
            history.record(build_history::BuildAction::PlaceModule {
                entity,
                module_type: event.module_type,
                cost,
            });

            let message = if event.custom_name.is_some() {
                format!("Placed Custom {} -{}c", event.module_type.name(), cost)
            } else {
                format!("Placed {} -{}c", event.module_type.name(), cost)
            };

            notifications.write(ShowNotification {
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
    mut events: MessageReader<RemoveModuleRequest>,
    module_query: Query<&Module>,
    mut removed_events: MessageWriter<ModuleRemoved>,
    mut notifications: MessageWriter<ShowNotification>,
    mut currency: ResMut<Currency>,
    registry: Res<ModuleRegistry>,
) {
    for event in events.read() {
        if let Ok(module) = module_query.get(event.module) {
            let cost = registry.get(module.module_type).cost;
            let refund = (cost as f32 * 0.75) as u32;
            currency.credits += refund;

            removed_events.write(ModuleRemoved {
                module_type: module.module_type,
                grid_position: module.grid_position,
            });

            notifications.write(ShowNotification {
                message: format!("Removed {} +{}c refund", module.module_type.name(), refund),
                notification_type: NotificationType::Warning,
                duration: 1.5,
            });

            commands.entity(event.module).despawn();
        }
    }
}

/// Processes RemoveHullRequest events (build-mode hull deletion, 75% refund
/// like modules)
fn process_hull_removal(
    mut commands: Commands,
    mut events: MessageReader<RemoveHullRequest>,
    hull_query: Query<&HullSegment>,
    mut notifications: MessageWriter<ShowNotification>,
    mut currency: ResMut<Currency>,
) {
    for event in events.read() {
        if let Ok(hull) = hull_query.get(event.hull) {
            let refund = (hull.material.cost() as f32 * 0.75) as u32;
            currency.credits += refund;

            notifications.write(ShowNotification {
                message: format!("Removed {} hull +{}c refund", hull.material.name(), refund),
                notification_type: NotificationType::Warning,
                duration: 1.5,
            });

            commands.entity(event.hull).despawn();
        }
    }
}

// ============================================================================
// CUSTOM MODULE STAT RECALCULATION
// ============================================================================

/// Recalculates stats for custom modules when their ship-components change
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
        // Collect all ship-component types from children
        let subcomponents: Vec<SubComponentType> = children.iter()
            .filter_map(|child| subcomponent_query.get(child).ok())
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

#[cfg(test)]
mod ship_grid_tests {
    use super::*;
    use crate::ai_ship::components::AiShip;

    fn hull_at(grid: IVec2) -> (HullSegment, Transform) {
        (
            HullSegment { grid_position: grid, ..default() },
            // Same placement maths ship/spawner.rs and ai_ship/spawner.rs use.
            Transform::from_xyz(grid.x as f32 * 66.0, grid.y as f32 * 66.0 - 33.0, 0.1),
        )
    }

    fn module_at(grid: IVec2, size: IVec2) -> Module {
        Module {
            module_type: ModuleType::AmmoBay,
            health: 100.0,
            max_health: 100.0,
            power_consumption: 0.0,
            power_generation: 0.0,
            is_active: true,
            grid_position: grid,
            size,
            rotation: Rotation::North,
        }
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_systems(Update, update_ship_grids);
        app
    }

    /// The whole reason the per-ship grid exists: two ships with blocks at the
    /// same ship-LOCAL cell. A single global map can only ever hold one of
    /// them, which is why GridOccupancy is player-scoped.
    #[test]
    fn grids_do_not_collide_across_ships() {
        let mut app = test_app();
        let player = app.world_mut().spawn(Ship).id();
        let enemy = app.world_mut().spawn(AiShip).id();

        let cell = IVec2::new(1, 0);
        let player_block = app.world_mut().spawn(hull_at(cell)).insert(ChildOf(player)).id();
        let enemy_block = app.world_mut().spawn(hull_at(cell)).insert(ChildOf(enemy)).id();

        app.update();

        assert_eq!(app.world().get::<ShipGrid>(player).unwrap().get(cell), Some(player_block));
        assert_eq!(app.world().get::<ShipGrid>(enemy).unwrap().get(cell), Some(enemy_block));
    }

    /// Destroyed blocks leave the grid — a dead plate must not keep stopping
    /// shells. This is the difference from GridOccupancy that hit resolution
    /// depends on.
    #[test]
    fn destroyed_blocks_leave_the_grid() {
        let mut app = test_app();
        let ship = app.world_mut().spawn(Ship).id();
        let cell = IVec2::new(2, 3);
        let block = app.world_mut().spawn(hull_at(cell)).insert(ChildOf(ship)).id();

        app.update();
        assert!(app.world().get::<ShipGrid>(ship).unwrap().contains(cell));

        app.world_mut().entity_mut(block).insert(HullDestroyed);
        app.update();

        assert!(!app.world().get::<ShipGrid>(ship).unwrap().contains(cell),
            "a destroyed plate is still occupying its cell");
    }

    /// A multi-cell module claims every cell of its footprint, so a shell
    /// crossing any of them finds the same entity.
    #[test]
    fn multi_cell_modules_claim_their_whole_footprint() {
        let mut app = test_app();
        let ship = app.world_mut().spawn(Ship).id();
        let origin = IVec2::new(0, 0);
        let module = app.world_mut()
            .spawn(module_at(origin, IVec2::new(3, 2)))
            .insert(ChildOf(ship))
            .id();

        app.update();

        let grid = app.world().get::<ShipGrid>(ship).unwrap();
        assert_eq!(grid.cells.len(), 6);
        for x in 0..3 {
            for y in 0..2 {
                assert_eq!(grid.get(origin + IVec2::new(x, y)), Some(module));
            }
        }
    }

    /// Hull spawns at `grid * 66.0` with a -33.0 y offset; the grid has to
    /// invert that exactly or every block lands one cell off.
    #[test]
    fn hull_transforms_round_trip_to_their_cell() {
        let mut app = test_app();
        let ship = app.world_mut().spawn(Ship).id();
        let cells = [IVec2::new(0, 0), IVec2::new(-4, 7), IVec2::new(5, -3)];
        for cell in cells {
            app.world_mut().spawn(hull_at(cell)).insert(ChildOf(ship));
        }

        app.update();

        let grid = app.world().get::<ShipGrid>(ship).unwrap();
        for cell in cells {
            assert!(grid.contains(cell), "hull at {cell:?} did not land in its own cell");
        }
    }
}

#[cfg(test)]
mod block_tests {
    use super::*;

    #[test]
    fn hull_thickness_seeds_from_material_absorption() {
        for material in [HullMaterial::Steel, HullMaterial::Titanium, HullMaterial::Composite, HullMaterial::AbyssalAlloy] {
            let block = Block::hull(IVec2::new(2, -1), material);
            assert_eq!(block.thickness, material.damage_absorption());
            assert_eq!(block.kind, BlockKind::Hull);
            assert_eq!(block.cell, IVec2::new(2, -1));
            assert_eq!(block.facing, None, "plain hull takes the face it was hit on");
        }
    }

    #[test]
    fn modules_are_unarmoured() {
        let block = Block::module(IVec2::ZERO);
        assert_eq!(block.kind, BlockKind::Module);
        assert_eq!(block.thickness, 0.0);
    }
}

#[cfg(test)]
mod walk_tests {
    use super::*;

    /// Grid with one distinct entity per listed cell, plus the World that
    /// owns those entities (dropping it would not invalidate the ids, but
    /// keeping it makes the intent obvious).
    fn grid_of(cells: &[IVec2]) -> (World, ShipGrid) {
        let mut world = World::new();
        let map = cells.iter().map(|&c| (c, world.spawn_empty().id())).collect();
        (world, ShipGrid { cells: map })
    }

    fn cells_hit(steps: &[GridStep]) -> Vec<IVec2> {
        steps.iter().map(|s| s.cell).collect()
    }

    #[test]
    fn straight_pass_visits_each_cell_once_with_unit_span() {
        let (_w, grid) = grid_of(&[IVec2::new(0, 0), IVec2::new(1, 0), IVec2::new(2, 0)]);
        let steps = grid.walk(Vec2::new(-1.0, 0.0), Vec2::new(3.0, 0.0));
        assert_eq!(cells_hit(&steps), vec![IVec2::new(0, 0), IVec2::new(1, 0), IVec2::new(2, 0)]);
        for s in &steps {
            assert!((s.span - 1.0).abs() < 1e-4, "span {}", s.span);
        }
        assert_eq!(steps[0].entry_face, IVec2::new(-1, 0));
    }

    #[test]
    fn a_225_unit_frame_step_still_registers() {
        // Railgun + APFSDS: 225 units/frame = 3.4 cells. A point test at the
        // new position skips clean over a one-cell wall; the swept walk
        // cannot.
        let (_w, grid) = grid_of(&[IVec2::new(0, 0)]);
        let before = Vec2::new(-2.0, 0.0);
        let after = before + Vec2::new(225.0 / 66.0, 0.0);
        assert!(after.x > 1.0, "the wall is strictly between the two samples");
        let steps = grid.walk(before, after);
        assert_eq!(cells_hit(&steps), vec![IVec2::new(0, 0)]);
    }

    #[test]
    fn much_faster_than_anything_fired_still_registers() {
        let (_w, grid) = grid_of(&[IVec2::new(5, 3)]);
        let steps = grid.walk(Vec2::new(-4.0, 0.0), Vec2::new(14.0, 6.0));
        assert_eq!(cells_hit(&steps), vec![IVec2::new(5, 3)]);
    }

    #[test]
    fn diagonal_visits_corner_cells_bresenham_would_skip() {
        // Slope 1/2 from (0,0) to (2,1) passes through (1,0) AND (1,1). A
        // one-cell-per-column stepper picks one of them.
        for corner in [IVec2::new(1, 0), IVec2::new(1, 1)] {
            let (_w, grid) = grid_of(&[corner]);
            let steps = grid.walk(Vec2::ZERO, Vec2::new(2.0, 1.0));
            assert_eq!(cells_hit(&steps), vec![corner], "corner {corner:?} was skipped");
        }
    }

    #[test]
    fn diagonal_span_is_line_of_sight_thickness() {
        let (_w, grid) = grid_of(&[IVec2::ZERO]);
        // Corner to corner of cell (0,0): from (-0.5,-0.5) to (0.5,0.5).
        let steps = grid.walk(Vec2::new(-1.0, -1.0), Vec2::new(1.0, 1.0));
        let cell = steps.iter().find(|s| s.cell == IVec2::ZERO).expect("centre cell hit");
        assert!((cell.span - 2f32.sqrt()).abs() < 1e-3, "span {}", cell.span);
    }

    #[test]
    fn multi_cell_module_is_one_step_with_summed_span() {
        let mut world = World::new();
        let module = world.spawn_empty().id();
        let grid = ShipGrid {
            cells: [(IVec2::new(0, 0), module), (IVec2::new(1, 0), module)].into_iter().collect(),
        };
        let steps = grid.walk(Vec2::new(-1.0, 0.0), Vec2::new(2.0, 0.0));
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].entity, module);
        assert!((steps[0].span - 2.0).abs() < 1e-4, "span {}", steps[0].span);
    }

    #[test]
    fn empty_and_missed_walks_yield_nothing() {
        let (_w, grid) = grid_of(&[IVec2::new(0, 3)]);
        assert!(grid.walk(Vec2::new(-1.0, 0.0), Vec2::new(3.0, 0.0)).is_empty());
        let (_w2, empty) = grid_of(&[]);
        assert!(empty.walk(Vec2::new(-1.0, 0.0), Vec2::new(3.0, 0.0)).is_empty());
    }

    #[test]
    fn zero_length_walk_is_a_point_test() {
        let (_w, grid) = grid_of(&[IVec2::new(2, -1)]);
        let inside = grid.walk(Vec2::new(2.2, -1.3), Vec2::new(2.2, -1.3));
        assert_eq!(cells_hit(&inside), vec![IVec2::new(2, -1)]);
        assert!(grid.walk(Vec2::new(0.0, 0.0), Vec2::new(0.0, 0.0)).is_empty());
    }

    #[test]
    fn walk_is_bounded_and_survives_nan() {
        let (_w, grid) = grid_of(&[IVec2::new(50, 0)]);
        // 200 cells requested, cap is MAX_WALK_STEPS: exhaustion is a miss.
        assert!(grid.walk(Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)).is_empty());
        assert!(grid.walk(Vec2::new(f32::NAN, 0.0), Vec2::new(1.0, 0.0)).is_empty());
        assert!(grid.walk(Vec2::ZERO, Vec2::new(f32::INFINITY, 0.0)).is_empty());
    }
}
