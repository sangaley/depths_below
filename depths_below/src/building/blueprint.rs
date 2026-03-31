use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::components::*;
use crate::events::*;
use crate::states::BuildState;

// ============================================================================
// DATA MODEL
// ============================================================================

/// A saved submarine blueprint
#[derive(Serialize, Deserialize, Clone)]
pub struct Blueprint {
    pub name: String,
    pub hull_cells: Vec<BlueprintHullCell>,
    pub modules: Vec<BlueprintModule>,
    pub created_at: String,
    /// Version tag for forward-compat; current version = 1
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
                            if bp.version != 1 {
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

/// Ctrl+S: save current submarine's hull & modules as a named blueprint.
/// Only saves entities that are children of the Submarine entity.
pub fn save_blueprint_system(
    keyboard: Res<Input<KeyCode>>,
    time: Res<Time>,
    sub_query: Query<&Children, With<Submarine>>,
    hull_query: Query<&HullSegment>,
    module_query: Query<&Module>,
    mut resource: ResMut<BlueprintResource>,
    mut notifications: EventWriter<ShowNotification>,
) {
    // Tick cooldown
    resource.save_cooldown.tick(time.delta());

    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !(ctrl && keyboard.just_pressed(KeyCode::S)) {
        return;
    }

    // Prevent rapid-fire saves
    if !resource.save_cooldown.finished() {
        return;
    }

    let Ok(children) = sub_query.get_single() else {
        notifications.send(ShowNotification {
            message: "No ship to save.".into(),
            notification_type: NotificationType::Warning,
            duration: 2.0,
        });
        return;
    };

    // Only save entities that are children of the submarine
    let hull_cells: Vec<BlueprintHullCell> = children.iter()
        .filter_map(|&child| hull_query.get(child).ok())
        .map(|h| BlueprintHullCell {
            grid_pos: h.grid_position,
            layer: h.hull_layer,
            material: h.material,
        })
        .collect();

    let modules: Vec<BlueprintModule> = children.iter()
        .filter_map(|&child| module_query.get(child).ok())
        .map(|m| BlueprintModule {
            module_type: m.module_type,
            grid_pos: m.grid_position,
            rotation: m.rotation,
        })
        .collect();

    if hull_cells.is_empty() && modules.is_empty() {
        notifications.send(ShowNotification {
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
        version: 1,
    };

    let summary = blueprint.summary();

    match write_blueprint_to_disk(&blueprint) {
        Ok(path) => {
            info!("Blueprint saved to {:?}", path);
            scan_blueprints(&mut resource);
            resource.save_cooldown.reset();

            notifications.send(ShowNotification {
                message: format!("Saved '{}' ({})", name, summary),
                notification_type: NotificationType::Success,
                duration: 3.0,
            });
        }
        Err(e) => {
            notifications.send(ShowNotification {
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
    keyboard: Res<Input<KeyCode>>,
    mut commands: Commands,
    mut resource: ResMut<BlueprintResource>,
    hull_entities: Query<Entity, With<HullSegment>>,
    module_entities: Query<Entity, With<Module>>,
    crew_entities: Query<Entity, With<CrewMember>>,
    mut place_hull_events: EventWriter<PlaceHullRequest>,
    mut place_module_events: EventWriter<PlaceModuleRequest>,
    mut notifications: EventWriter<ShowNotification>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !(ctrl && keyboard.just_pressed(KeyCode::L)) {
        return;
    }

    // Lazy-scan on first access
    if !resource.scanned {
        scan_blueprints(&mut resource);
    }

    if resource.blueprints.is_empty() {
        notifications.send(ShowNotification {
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
        commands.entity(entity).despawn_recursive();
    }
    for entity in module_entities.iter() {
        commands.entity(entity).despawn_recursive();
    }
    for entity in crew_entities.iter() {
        commands.entity(entity).despawn_recursive();
    }

    // Rebuild from blueprint — free placement (no cost, no per-item notifications)
    for hull_cell in &blueprint.hull_cells {
        place_hull_events.send(PlaceHullRequest {
            layer: hull_cell.layer,
            material: hull_cell.material,
            grid_position: hull_cell.grid_pos,
            free: true,
        });
    }

    for module in &blueprint.modules {
        place_module_events.send(PlaceModuleRequest {
            module_type: module.module_type,
            grid_position: module.grid_pos,
            rotation: module.rotation,
            custom_name: None,
            subcomponents: None,
            free: true,
        });
    }

    let summary = blueprint.summary();
    let position = format!("{}/{}", idx + 1, count);

    notifications.send(ShowNotification {
        message: format!("Loaded '{}' [{}] ({})", blueprint.name, position, summary),
        notification_type: NotificationType::Success,
        duration: 3.0,
    });
}

/// Ctrl+D: delete the currently selected blueprint (only works in build mode)
pub fn delete_blueprint_system(
    keyboard: Res<Input<KeyCode>>,
    build_state: Res<State<BuildState>>,
    mut resource: ResMut<BlueprintResource>,
    mut notifications: EventWriter<ShowNotification>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !(ctrl && keyboard.just_pressed(KeyCode::D)) {
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
                notifications.send(ShowNotification {
                    message: format!("Deleted blueprint '{}'", bp_name),
                    notification_type: NotificationType::Warning,
                    duration: 2.0,
                });
            }
            Err(e) => {
                notifications.send(ShowNotification {
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
