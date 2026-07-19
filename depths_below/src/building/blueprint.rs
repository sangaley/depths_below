use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::building::customization::tuning::{SelectedAmmo, WeaponTuning};
use crate::combat::targeting::fire_groups::FireGroup;
use crate::components::*;
use crate::events::*;
use crate::states::BuildState;

// ============================================================================
// DATA MODEL
//
// The Blueprint is THE canonical ship design format — player saves, the
// starter vessel, and (in progress) AI faction ships all speak it. v1 files
// (hull + module types only) still load; v2 adds per-module state: weapon
// tuning, fire groups, ammo selection, custom modules.
// ============================================================================

/// Highest design version this build reads/writes.
pub const BLUEPRINT_VERSION: u32 = 2;

/// A saved ship blueprint
#[derive(Serialize, Deserialize, Clone)]
pub struct Blueprint {
    pub name: String,
    pub hull_cells: Vec<BlueprintHullCell>,
    pub modules: Vec<BlueprintModule>,
    pub created_at: String,
    /// Version tag for forward-compat; see BLUEPRINT_VERSION
    pub version: u32,
}

impl Blueprint {
    /// Human-readable summary of the blueprint contents
    pub fn summary(&self) -> String {
        let module_count = self.modules.len();
        let hull_count = self.hull_cells.len();

        // Count unique module categories
        let mut categories: Vec<ModuleCategory> = self.modules.iter()
            .map(|m| m.module_type.category())
            .collect();
        categories.sort_by_key(|c| *c as u8);
        categories.dedup();
        let cat_count = categories.len();

        format!("{} hull, {} modules ({} categories)", hull_count, module_count, cat_count)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BlueprintHullCell {
    pub grid_pos: IVec2,
    pub layer: HullLayer,
    pub material: HullMaterial,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BlueprintModule {
    pub module_type: ModuleType,
    pub grid_pos: IVec2,
    pub rotation: Rotation,
    /// Custom-built module name (None = stock module)
    #[serde(default)]
    pub custom_name: Option<String>,
    /// Sub-components of a custom-built module
    #[serde(default)]
    pub subcomponents: Option<Vec<SubComponentType>>,
    /// Non-default per-module state (tuning, fire group, ammo)
    #[serde(default)]
    pub extras: Option<ModuleExtras>,
}

/// Per-module state a design carries beyond type + position. Only
/// non-default values are recorded, so v1-era files and untouched modules
/// serialize compactly.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default)]
pub struct ModuleExtras {
    #[serde(default)]
    pub tuning: Option<WeaponTuning>,
    #[serde(default)]
    pub fire_group: Option<u8>,
    #[serde(default)]
    pub ammo: Option<SelectedAmmo>,
}

impl ModuleExtras {
    pub fn is_empty(&self) -> bool {
        self.tuning.is_none() && self.fire_group.is_none() && self.ammo.is_none()
    }
}

/// Inserts a design's per-module state onto a freshly spawned module.
pub fn apply_module_extras(commands: &mut Commands, entity: Entity, extras: &ModuleExtras) {
    if let Some(tuning) = extras.tuning {
        commands.entity(entity).try_insert(tuning);
    }
    if let Some(group) = extras.fire_group {
        commands.entity(entity).try_insert(FireGroup { group });
    }
    if let Some(ammo) = extras.ammo {
        commands.entity(entity).try_insert(ammo);
    }
}

/// Resource holding available blueprints (scanned from disk)
#[derive(Resource)]
pub struct BlueprintResource {
    pub blueprints: Vec<Blueprint>,
    pub selected_index: usize,
    /// Cooldown to prevent rapid-fire saves
    pub save_cooldown: Timer,
    /// Whether the blueprint list has been scanned this session
    pub scanned: bool,
}

impl Default for BlueprintResource {
    fn default() -> Self {
        Self {
            blueprints: Vec::new(),
            selected_index: 0,
            save_cooldown: Timer::from_seconds(1.0, TimerMode::Once),
            scanned: false,
        }
    }
}

// ============================================================================
// DISK I/O
// ============================================================================

fn blueprints_dir() -> PathBuf {
    PathBuf::from("blueprints")
}

/// Scans the blueprints/ directory and loads all blueprint JSON files
pub fn scan_blueprints(resource: &mut BlueprintResource) {
    resource.blueprints.clear();
    let dir = blueprints_dir();
    if !dir.exists() {
        resource.scanned = true;
        return;
    }
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "json") {
                match fs::read_to_string(&path) {
                    Ok(data) => match serde_json::from_str::<Blueprint>(&data) {
                        Ok(bp) => {
                            if bp.version > BLUEPRINT_VERSION {
                                warn!("Skipping blueprint {:?}: unsupported version {}", path, bp.version);
                                continue;
                            }
                            resource.blueprints.push(bp);
                        }
                        Err(e) => { warn!("Skipping invalid blueprint {:?}: {}", path, e); }
                    },
                    Err(e) => { warn!("Could not read blueprint {:?}: {}", path, e); }
                }
            }
        }
    }
    resource.blueprints.sort_by(|a, b| a.name.cmp(&b.name));
    resource.scanned = true;
    info!("Scanned {} blueprints from {:?}", resource.blueprints.len(), dir);
}

/// Writes a Blueprint to disk as pretty-printed JSON
fn write_blueprint_to_disk(bp: &Blueprint) -> Result<PathBuf, String> {
    let dir = blueprints_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| format!("Cannot create blueprints dir: {}", e))?;
    }
    // Sanitize name for filesystem
    let safe_name: String = bp.name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    let path = dir.join(format!("{}.json", safe_name));
    let json = serde_json::to_string_pretty(bp)
        .map_err(|e| format!("Serialization failed: {}", e))?;
    fs::write(&path, json)
        .map_err(|e| format!("Write failed: {}", e))?;
    Ok(path)
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Ctrl+S: save current ship's hull & modules as a named blueprint.
/// Only saves entities that are children of the Ship entity.
pub fn save_blueprint_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    ship_query: Query<&Children, With<Ship>>,
    hull_query: Query<&HullSegment>,
    module_query: Query<(Entity, &Module)>,
    tuning_query: Query<&WeaponTuning>,
    fire_group_query: Query<&FireGroup>,
    ammo_query: Query<&SelectedAmmo>,
    custom_query: Query<(&CustomModule, &Children)>,
    subcomp_query: Query<&SubComponent>,
    mut resource: ResMut<BlueprintResource>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    // Tick cooldown
    resource.save_cooldown.tick(time.delta());

    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !(ctrl && keyboard.just_pressed(KeyCode::KeyS)) {
        return;
    }

    // Prevent rapid-fire saves
    if !resource.save_cooldown.is_finished() {
        return;
    }

    let Ok(children) = ship_query.single() else {
        notifications.write(ShowNotification {
            message: "No ship to save.".into(),
            notification_type: NotificationType::Warning,
            duration: 2.0,
        });
        return;
    };

    // Only save entities that are children of the ship
    let hull_cells: Vec<BlueprintHullCell> = children.iter()
        .filter_map(|child| hull_query.get(child).ok())
        .map(|h| BlueprintHullCell {
            grid_pos: h.grid_position,
            layer: h.hull_layer,
            material: h.material,
        })
        .collect();

    let modules: Vec<BlueprintModule> = children.iter()
        .filter_map(|child| module_query.get(child).ok())
        .map(|(entity, m)| {
            // Record only non-default per-module state — untouched modules
            // stay as compact as a v1 entry
            let tuning = tuning_query.get(entity).ok().copied().filter(|t| {
                t.velocity != 1.0 || t.fire_rate != 1.0 || t.damage != 1.0
            });
            let fire_group = fire_group_query.get(entity).ok()
                .map(|g| g.group)
                .filter(|&g| g != 0);
            let ammo = ammo_query.get(entity).ok().copied()
                .filter(|a| a.0 != crate::combat::ammo_types::KineticAmmoType::AP);
            let extras = ModuleExtras { tuning, fire_group, ammo };

            let (custom_name, subcomponents) = match custom_query.get(entity) {
                Ok((custom, module_children)) => (
                    Some(custom.custom_name.clone()),
                    Some(
                        module_children.iter()
                            .filter_map(|c| subcomp_query.get(c).ok())
                            .map(|sc| sc.subcomponent_type.clone())
                            .collect(),
                    ),
                ),
                Err(_) => (None, None),
            };

            BlueprintModule {
                module_type: m.module_type,
                grid_pos: m.grid_position,
                rotation: m.rotation,
                custom_name,
                subcomponents,
                extras: if extras.is_empty() { None } else { Some(extras) },
            }
        })
        .collect();

    if hull_cells.is_empty() && modules.is_empty() {
        notifications.write(ShowNotification {
            message: "Nothing to save — build some hull/modules first.".into(),
            notification_type: NotificationType::Warning,
            duration: 2.0,
        });
        return;
    }

    let timestamp = chrono_lite_timestamp();
    let name = format!("sub_{}m_{}", modules.len(), timestamp);

    let blueprint = Blueprint {
        name: name.clone(),
        hull_cells,
        modules,
        created_at: timestamp,
        version: BLUEPRINT_VERSION,
    };

    let summary = blueprint.summary();

    match write_blueprint_to_disk(&blueprint) {
        Ok(path) => {
            info!("Blueprint saved to {:?}", path);
            scan_blueprints(&mut resource);
            resource.save_cooldown.reset();

            notifications.write(ShowNotification {
                message: format!("Saved '{}' ({})", name, summary),
                notification_type: NotificationType::Success,
                duration: 3.0,
            });
        }
        Err(e) => {
            notifications.write(ShowNotification {
                message: format!("Save failed: {}", e),
                notification_type: NotificationType::Danger,
                duration: 3.0,
            });
        }
    }
}

/// Ctrl+L: cycle through and load a saved blueprint.
/// Despawns existing hull, modules, and crew. Places blueprint contents for free.
pub fn load_blueprint_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut resource: ResMut<BlueprintResource>,
    hull_entities: Query<Entity, With<HullSegment>>,
    module_entities: Query<Entity, With<Module>>,
    crew_entities: Query<Entity, With<CrewMember>>,
    mut place_hull_events: MessageWriter<PlaceHullRequest>,
    mut place_module_events: MessageWriter<PlaceModuleRequest>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !(ctrl && keyboard.just_pressed(KeyCode::KeyL)) {
        return;
    }

    // Lazy-scan on first access
    if !resource.scanned {
        scan_blueprints(&mut resource);
    }

    if resource.blueprints.is_empty() {
        notifications.write(ShowNotification {
            message: "No blueprints found. Save one first with Ctrl+S.".into(),
            notification_type: NotificationType::Warning,
            duration: 2.5,
        });
        return;
    }

    let count = resource.blueprints.len();
    let idx = resource.selected_index % count;
    let blueprint = resource.blueprints[idx].clone();
    resource.selected_index = (idx + 1) % count;

    // Clear existing hull, modules, and crew
    for entity in hull_entities.iter() {
        commands.entity(entity).despawn();
    }
    for entity in module_entities.iter() {
        commands.entity(entity).despawn();
    }
    for entity in crew_entities.iter() {
        commands.entity(entity).despawn();
    }

    // Rebuild from blueprint — free placement (no cost, no per-item notifications)
    for hull_cell in &blueprint.hull_cells {
        place_hull_events.write(PlaceHullRequest {
            layer: hull_cell.layer,
            material: hull_cell.material,
            grid_position: hull_cell.grid_pos,
            free: true,
        });
    }

    for module in &blueprint.modules {
        place_module_events.write(PlaceModuleRequest {
            module_type: module.module_type,
            grid_position: module.grid_pos,
            rotation: module.rotation,
            custom_name: module.custom_name.clone(),
            subcomponents: module.subcomponents.clone(),
            extras: module.extras,
            free: true,
        });
    }

    let summary = blueprint.summary();
    let position = format!("{}/{}", idx + 1, count);

    notifications.write(ShowNotification {
        message: format!("Loaded '{}' [{}] ({})", blueprint.name, position, summary),
        notification_type: NotificationType::Success,
        duration: 3.0,
    });
}

/// Ctrl+D: delete the currently selected blueprint (only works in build mode)
pub fn delete_blueprint_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    build_state: Res<State<BuildState>>,
    mut resource: ResMut<BlueprintResource>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !(ctrl && keyboard.just_pressed(KeyCode::KeyD)) {
        return;
    }

    // Only allow deletion while in build mode
    if *build_state.get() == BuildState::Inactive {
        return;
    }

    if resource.blueprints.is_empty() {
        return;
    }

    // Delete the most recently loaded blueprint (selected_index - 1)
    let count = resource.blueprints.len();
    let idx = if resource.selected_index == 0 { count - 1 } else { resource.selected_index - 1 };
    let idx = idx % count;
    let bp_name = resource.blueprints[idx].name.clone();

    // Remove from disk
    let safe_name: String = bp_name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    let path = blueprints_dir().join(format!("{}.json", safe_name));
    if path.exists() {
        match fs::remove_file(&path) {
            Ok(_) => {
                notifications.write(ShowNotification {
                    message: format!("Deleted blueprint '{}'", bp_name),
                    notification_type: NotificationType::Warning,
                    duration: 2.0,
                });
            }
            Err(e) => {
                notifications.write(ShowNotification {
                    message: format!("Failed to delete '{}': {}", bp_name, e),
                    notification_type: NotificationType::Danger,
                    duration: 3.0,
                });
                return;
            }
        }
    }

    // Refresh list
    scan_blueprints(&mut resource);
    resource.selected_index = 0;
}

// ============================================================================
// DESIGN FILES & DIRECT SPAWNING
// Shared by the starter vessel (ship::spawner) and — next slice — AI ships.
// Unlike blueprint loading (event-driven, replaces the player ship), this
// path spawns a design's blocks directly under an existing root entity.
// ============================================================================

/// Loads a single design file. None if missing, unreadable, or from a
/// newer format version.
pub fn load_design_file(path: impl AsRef<Path>) -> Option<Blueprint> {
    let path = path.as_ref();
    let data = fs::read_to_string(path).ok()?;
    match serde_json::from_str::<Blueprint>(&data) {
        Ok(bp) if bp.version <= BLUEPRINT_VERSION => Some(bp),
        Ok(bp) => {
            warn!("Design {:?} has unsupported version {}", path, bp.version);
            None
        }
        Err(e) => {
            warn!("Design {:?} failed to parse: {}", path, e);
            None
        }
    }
}

/// Writes a design as pretty JSON, creating parent directories.
pub fn write_design_file(path: impl AsRef<Path>, bp: &Blueprint) -> Result<(), String> {
    let path = path.as_ref();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("Cannot create {:?}: {}", dir, e))?;
    }
    let json = serde_json::to_string_pretty(bp).map_err(|e| format!("Serialize failed: {}", e))?;
    fs::write(path, json).map_err(|e| format!("Write failed: {}", e))
}

/// Spawns every block of a design as children of `ship`, applying each
/// module's extras. Hull spawning mirrors building::process_hull_placement
/// (same tints, same stats) so designed ships and hand-built ships are
/// indistinguishable.
pub fn spawn_ship_from_design(
    commands: &mut Commands,
    asset_server: &AssetServer,
    registry: &crate::building::ModuleRegistry,
    ship: Entity,
    design: &Blueprint,
) {
    for cell in &design.hull_cells {
        let color = match cell.layer {
            HullLayer::Outer => Color::WHITE,
            HullLayer::Inner => Color::srgb(0.9, 0.9, 0.9),
            HullLayer::Void => Color::srgb(0.5, 0.5, 0.6),
            HullLayer::BulkheadDoor => Color::srgb(0.9, 0.8, 0.7),
        };
        let texture = asset_server.load(crate::sprite_map::hull_sprite_path(cell.material));
        let health = 100.0 * cell.material.health_multiplier();

        commands.spawn((
            (Sprite {
                    image: texture,
                    color,
                    custom_size: Some(Vec2::new(64.0, 64.0)),
                    ..default()
                }, Transform::from_xyz(
                    cell.grid_pos.x as f32 * 66.0,
                    cell.grid_pos.y as f32 * 66.0 - 33.0,
                    0.1,
                )),
            BaseSpriteColor(color),
            BaseHullStats {
                max_health: health,
                radiation_shielding: cell.material.radiation_shielding(),
            },
            HullSegment {
                hull_layer: cell.layer,
                material: cell.material,
                radiation_shielding: cell.material.radiation_shielding(),
                health,
                max_health: health,
                grid_position: cell.grid_pos,
                ..default()
            },
            ChildOf(ship),
        ));
    }

    for m in &design.modules {
        let entity = if let (Some(name), Some(subs)) = (&m.custom_name, &m.subcomponents) {
            crate::ship::spawn_custom_module(
                commands, asset_server, ship, m.module_type,
                name.clone(), m.grid_pos, m.rotation, subs.clone(), registry,
            )
        } else {
            crate::ship::spawn_module(
                commands, asset_server, ship, m.module_type,
                m.grid_pos, m.rotation, registry,
            )
        };
        if let Some(extras) = &m.extras {
            apply_module_extras(commands, entity, extras);
        }
    }
}

// ============================================================================
// HELPERS
// ============================================================================

/// Unix-seconds timestamp without external crate
fn chrono_lite_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", secs)
}
