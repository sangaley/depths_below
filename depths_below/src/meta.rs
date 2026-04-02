use bevy::prelude::*;
use crate::components::*;
use crate::resources::*;
use crate::events::*;
use crate::states::GameState;

const SAVE_DIR: &str = "saves";
const MAX_SLOTS: u32 = 3;
const AUTO_SAVE_SLOT: u32 = 99; // Special slot number for auto-save

pub struct MetaPlugin;

impl Plugin for MetaPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<Unlocks>()
            .init_resource::<Statistics>()
            .init_resource::<Inventory>()
            .init_resource::<Currency>()
            .init_resource::<AutoSaveTimer>()
            .init_resource::<SaveLoadMenuState>()
            .init_resource::<PendingLoad>()
            .init_resource::<PendingEntityRebuild>()
            .add_systems(Startup, load_unlocks)
            .add_systems(OnEnter(GameState::GameOver), save_unlocks)
            .add_systems(Update, handle_save_request.run_if(in_state(GameState::Paused)))
            .add_systems(Update, handle_load_request)
            .add_systems(Update, apply_pending_load)
            .add_systems(Update, rebuild_entities_from_save)
            .add_systems(Update, apply_module_health_overrides)
            .add_systems(
                Update,
                auto_save_system.run_if(
                    in_state(GameState::Exploring)
                        .or_else(in_state(GameState::StationDocked))
                ),
            );
    }
}

/// Load unlocks from file on startup
fn load_unlocks(mut unlocks: ResMut<Unlocks>) {
    let path = "meta/unlocks.json";
    if let Ok(data) = std::fs::read_to_string(path) {
        if let Ok(loaded) = serde_json::from_str::<Unlocks>(&data) {
            *unlocks = loaded;
            info!("Loaded unlocks from {}", path);
        }
    }
}

/// Save unlocks to file on game over
fn save_unlocks(unlocks: Res<Unlocks>) {
    let _ = std::fs::create_dir_all("meta");
    let path = "meta/unlocks.json";
    if let Ok(data) = serde_json::to_string_pretty(unlocks.as_ref()) {
        let _ = std::fs::write(path, data);
        info!("Saved unlocks to {}", path);
    }
}

// ============================================================================
// SAVE SYSTEM
// ============================================================================

/// Get save file path for a given slot
fn save_path(slot: u32) -> String {
    if slot == AUTO_SAVE_SLOT {
        format!("{}/autosave.json", SAVE_DIR)
    } else {
        format!("{}/slot_{}.json", SAVE_DIR, slot)
    }
}

/// Read save slot info without loading the full save
pub fn read_slot_info(slot: u32) -> Option<SaveSlotInfo> {
    let path = save_path(slot);
    let data = std::fs::read_to_string(&path).ok()?;
    let save: SaveData = serde_json::from_str(&data).ok()?;
    Some(save.slot_info)
}

/// Check which slots have saves
pub fn get_save_slots() -> Vec<(u32, Option<SaveSlotInfo>)> {
    let mut slots = Vec::new();
    for i in 0..MAX_SLOTS {
        slots.push((i, read_slot_info(i)));
    }
    // Also check auto-save
    slots.push((AUTO_SAVE_SLOT, read_slot_info(AUTO_SAVE_SLOT)));
    slots
}

/// Collect current game state into SaveData
fn collect_save_data(
    slot: u32,
    depth_state: &DepthState,
    hull_state: &HullState,
    statistics: &Statistics,
    inventory: &Inventory,
    currency: &Currency,
    unlocks: &Unlocks,
    discovered_locations: &DiscoveredLocations,
    world_state: &WorldState,
    sub_query: &Query<&Transform, With<Submarine>>,
    module_query: &Query<(&Module, Option<&CustomModule>)>,
    hull_query: &Query<(&HullSegment, &Transform)>,
    crew_query: &Query<(Entity, &CrewMember)>,
    current_state: &State<GameState>,
) -> SaveData {
    let position = sub_query
        .get_single()
        .map(|t| t.translation.truncate())
        .unwrap_or(Vec2::ZERO);

    // Collect modules
    let modules: Vec<ModuleData> = module_query
        .iter()
        .map(|(module, custom)| {
            let custom_data = custom.map(|c| CustomModuleData {
                custom_name: c.custom_name.clone(),
                subcomponents: Vec::new(),
            });
            ModuleData {
                module_type: module.module_type,
                grid_position: module.grid_position,
                health: module.health,
                rotation: module.rotation,
                is_active: module.is_active,
                custom_data,
                customization_params: None, // Tier 3 params saved in future version
            }
        })
        .collect();

    // Collect hull segments with material info
    let hull_segments: Vec<HullSaveData> = hull_query
        .iter()
        .map(|(seg, transform)| {
            let grid_x = (transform.translation.x / 66.0).round() as i32;
            let grid_y = ((transform.translation.y + 33.0) / 66.0).round() as i32;
            HullSaveData {
                grid_position: IVec2::new(grid_x, grid_y),
                health: seg.health,
                max_health: seg.max_health,
                radiation_shielding: seg.radiation_shielding,
                material: seg.material,
                hull_layer: seg.hull_layer,
            }
        })
        .collect();

    // Collect crew (auto-assign restores assignments on load)
    let crew: Vec<CrewSaveData> = crew_query
        .iter()
        .map(|(_crew_entity, member)| {
            CrewSaveData {
                name: member.name.clone(),
                health: member.health,
                max_health: member.max_health,
                oxygen: member.oxygen,
                morale: member.morale,
                assigned_module_grid: None,
            }
        })
        .collect();

    // Timestamp
    let timestamp = format!("Save {}", slot);

    SaveData {
        version: 1,
        slot_info: SaveSlotInfo {
            slot,
            timestamp,
            depth: depth_state.current_depth,
            play_time: statistics.play_time_seconds,
            hull_integrity: hull_state.hull_integrity,
        },
        submarine: SubmarineBlueprint {
            hull_segments: Vec::new(), // Legacy field
            modules,
        },
        hull_segments,
        crew,
        inventory: inventory.clone(),
        currency: currency.clone(),
        unlocks: unlocks.clone(),
        statistics: statistics.clone(),
        discovered_locations: discovered_locations.clone(),
        position,
        current_depth: depth_state.current_depth,
        world_seed: world_state.seed,
        was_exploring: *current_state.get() == GameState::Exploring,
        current_system_id: 0, // Will be set by save system that reads GalaxyState
        galaxy_seed: world_state.seed,
    }
}

/// Write save data to disk
fn write_save(save_data: &SaveData, slot: u32) -> bool {
    let _ = std::fs::create_dir_all(SAVE_DIR);
    let path = save_path(slot);
    match serde_json::to_string_pretty(save_data) {
        Ok(json) => {
            match std::fs::write(&path, json) {
                Ok(_) => {
                    info!("Game saved to {}", path);
                    true
                }
                Err(e) => {
                    error!("Failed to write save file {}: {}", path, e);
                    false
                }
            }
        }
        Err(e) => {
            error!("Failed to serialize save data: {}", e);
            false
        }
    }
}

/// Handle save game requests
fn handle_save_request(
    mut save_events: EventReader<SaveGameRequest>,
    mut saved_events: EventWriter<GameSaved>,
    mut notify_events: EventWriter<ShowNotification>,
    depth_state: Res<DepthState>,
    hull_state: Res<HullState>,
    statistics: Res<Statistics>,
    inventory: Res<Inventory>,
    currency: Res<Currency>,
    unlocks: Res<Unlocks>,
    discovered_locations: Res<DiscoveredLocations>,
    world_state: Res<WorldState>,
    current_state: Res<State<GameState>>,
    sub_query: Query<&Transform, With<Submarine>>,
    module_query: Query<(&Module, Option<&CustomModule>)>,
    hull_query: Query<(&HullSegment, &Transform)>,
    crew_query: Query<(Entity, &CrewMember)>,
) {
    for event in save_events.iter() {
        let save_data = collect_save_data(
            event.slot,
            &depth_state,
            &hull_state,
            &statistics,
            &inventory,
            &currency,
            &unlocks,
            &discovered_locations,
            &world_state,
            &sub_query,
            &module_query,
            &hull_query,
            &crew_query,
            &current_state,
        );

        // Tier 3 customization is saved via customization_params field
        // (populated when ModuleCustomization query is added in a future refactor)

        let success = write_save(&save_data, event.slot);

        saved_events.send(GameSaved {
            slot: event.slot,
            success,
        });

        let msg = if success {
            if event.slot == AUTO_SAVE_SLOT {
                "Auto-save complete".to_string()
            } else {
                format!("Game saved to slot {}", event.slot + 1)
            }
        } else {
            "Save failed!".to_string()
        };

        notify_events.send(ShowNotification {
            message: msg,
            notification_type: if success { NotificationType::Success } else { NotificationType::Danger },
            duration: 3.0,
        });
    }
}

// ============================================================================
// LOAD SYSTEM
// ============================================================================

/// Pending load data resource - parsed save data waiting to be applied
#[derive(Resource, Default)]
struct PendingLoad(Option<(u32, SaveData)>);

/// Phase 1: Read save file and store in PendingLoad resource
fn handle_load_request(
    mut load_events: EventReader<LoadGameRequest>,
    mut notify_events: EventWriter<ShowNotification>,
    mut pending: ResMut<PendingLoad>,
) {
    for event in load_events.iter() {
        let path = save_path(event.slot);
        let data = match std::fs::read_to_string(&path) {
            Ok(d) => d,
            Err(e) => {
                error!("Failed to read save file {}: {}", path, e);
                notify_events.send(ShowNotification {
                    message: "Load failed: file not found".to_string(),
                    notification_type: NotificationType::Danger,
                    duration: 3.0,
                });
                continue;
            }
        };

        let save: SaveData = match serde_json::from_str(&data) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to parse save file: {}", e);
                notify_events.send(ShowNotification {
                    message: "Load failed: corrupted save".to_string(),
                    notification_type: NotificationType::Danger,
                    duration: 3.0,
                });
                continue;
            }
        };

        pending.0 = Some((event.slot, save));
    }
}

/// Phase 2: Apply pending load data to game state
fn apply_pending_load(
    mut pending: ResMut<PendingLoad>,
    mut depth_state: ResMut<DepthState>,
    mut hull_state: ResMut<HullState>,
    mut statistics: ResMut<Statistics>,
    mut inventory: ResMut<Inventory>,
    mut currency: ResMut<Currency>,
    mut unlocks: ResMut<Unlocks>,
    mut discovered_locations: ResMut<DiscoveredLocations>,
    mut world_state: ResMut<WorldState>,
    mut commands: Commands,
) {
    let Some((slot, save)) = pending.0.take() else { return };

    // ---- Restore resources ----
    depth_state.current_depth = save.current_depth;
    depth_state.target_depth = save.current_depth;
    *statistics = save.statistics.clone();
    *inventory = save.inventory.clone();
    *currency = save.currency.clone();
    *unlocks = save.unlocks.clone();
    *discovered_locations = save.discovered_locations.clone();
    world_state.seed = save.world_seed;
    hull_state.hull_integrity = save.slot_info.hull_integrity;

    // Store save data in a resource for entity rebuild (handled in phase 3)
    commands.insert_resource(PendingEntityRebuild {
        save_data: Some(save),
        slot,
    });
}

/// Save data waiting for entity rebuild
#[derive(Resource, Default)]
struct PendingEntityRebuild {
    save_data: Option<SaveData>,
    slot: u32,
}

/// Phase 3: Despawn old entities and respawn from save data
fn rebuild_entities_from_save(
    mut pending: ResMut<PendingEntityRebuild>,
    mut loaded_events: EventWriter<GameLoaded>,
    mut notify_events: EventWriter<ShowNotification>,
    mut next_state: ResMut<NextState<GameState>>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    registry: Res<crate::building::registry::ModuleRegistry>,
    sub_query: Query<Entity, With<Submarine>>,
    module_entities: Query<Entity, With<Module>>,
    hull_entities: Query<Entity, With<HullSegment>>,
    crew_entities: Query<Entity, With<CrewMember>>,
) {
    let Some(save) = pending.save_data.take() else { return };
    let slot = pending.slot;

    // ---- Despawn existing submarine entities ----
    for entity in module_entities.iter() {
        commands.entity(entity).despawn_recursive();
    }
    for entity in hull_entities.iter() {
        commands.entity(entity).despawn_recursive();
    }
    for entity in crew_entities.iter() {
        commands.entity(entity).despawn_recursive();
    }

    // ---- Respawn submarine at saved position ----
    let sub_entity = if let Ok(existing) = sub_query.get_single() {
        commands.entity(existing).insert(
            Transform::from_xyz(save.position.x, save.position.y, 0.0)
        );
        existing
    } else {
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgb(0.3, 0.3, 0.5),
                    custom_size: Some(Vec2::new(200.0, 80.0)),
                    ..default()
                },
                transform: Transform::from_xyz(save.position.x, save.position.y, 0.0),
                ..default()
            },
            Submarine,
            Velocity(Vec2::ZERO),
            Depth(save.current_depth),
            ThrusterState { base_drift: 0.0, current: 0.0 },
            Health { current: 100.0, max: 100.0 },
            SubmarinePhysics::default(),
        )).id()
    };

    // ---- Respawn hull segments ----
    for hull_data in &save.hull_segments {
        let hull_texture = asset_server.load(
            crate::sprite_map::hull_sprite_path(hull_data.material)
        );
        commands.spawn((
            SpriteBundle {
                texture: hull_texture,
                sprite: Sprite {
                    custom_size: Some(Vec2::new(64.0, 64.0)),
                    ..default()
                },
                transform: Transform::from_xyz(
                    hull_data.grid_position.x as f32 * 66.0,
                    hull_data.grid_position.y as f32 * 66.0 - 33.0,
                    0.1,
                ),
                ..default()
            },
            HullSegment {
                health: hull_data.health,
                max_health: hull_data.max_health,
                radiation_shielding: hull_data.radiation_shielding,
                is_depressurized: false,
                depressurization_level: 0.0,
                hull_layer: hull_data.hull_layer,
                material: hull_data.material,
                grid_position: hull_data.grid_position,
            },
        )).set_parent(sub_entity);
    }

    // ---- Respawn modules ----
    for module_data in &save.submarine.modules {
        let entity = crate::submarine::spawn_module(
            &mut commands,
            &asset_server,
            sub_entity,
            module_data.module_type,
            module_data.grid_position,
            module_data.rotation,
            &registry,
        );
        // Restore saved health and active state (spawn_module uses registry defaults)
        commands.entity(entity).insert(ModuleHealthOverride {
            health: module_data.health,
            is_active: module_data.is_active,
        });
    }

    // ---- Respawn crew ----
    let mut roster_members = Vec::new();
    for (i, crew_data) in save.crew.iter().enumerate() {
        let crew_entity = commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgb(0.8, 0.6, 0.5),
                    custom_size: Some(Vec2::new(16.0, 16.0)),
                    ..default()
                },
                transform: Transform::from_xyz(
                    (i as f32 - 3.5) * 20.0,
                    0.0,
                    0.5,
                ),
                ..default()
            },
            CrewMember {
                name: crew_data.name.clone(),
                health: crew_data.health,
                max_health: crew_data.max_health,
                oxygen: crew_data.oxygen,
                morale: crew_data.morale,
                state: CrewState::Idle,
            },
        )).set_parent(sub_entity).id();
        roster_members.push(crew_entity);
    }
    commands.insert_resource(CrewRoster { members: roster_members });
    // Auto-assign will re-staff stations on next tick

    // ---- Set game state ----
    if save.was_exploring {
        next_state.set(GameState::Exploring);
    } else {
        next_state.set(GameState::StationDocked);
    }

    loaded_events.send(GameLoaded { slot, success: true });

    let slot_name = if slot == AUTO_SAVE_SLOT {
        "auto-save".to_string()
    } else {
        format!("slot {}", slot + 1)
    };
    notify_events.send(ShowNotification {
        message: format!("Game loaded from {}", slot_name),
        notification_type: NotificationType::Success,
        duration: 3.0,
    });

    info!("Game loaded (depth: {:.0}m)", save.current_depth);
}

// ============================================================================
// MODULE HEALTH OVERRIDE (applied once after load)
// ============================================================================

/// Applies saved health/active state to modules after load, then removes the marker.
fn apply_module_health_overrides(
    mut commands: Commands,
    mut query: Query<(Entity, &mut Module, &ModuleHealthOverride)>,
) {
    for (entity, mut module, override_data) in query.iter_mut() {
        module.health = override_data.health;
        module.is_active = override_data.is_active;
        commands.entity(entity).remove::<ModuleHealthOverride>();
    }
}

// ============================================================================
// AUTO-SAVE
// ============================================================================

/// Auto-save every 2 minutes while exploring or at surface
fn auto_save_system(
    time: Res<Time>,
    mut auto_save: ResMut<AutoSaveTimer>,
    mut save_events: EventWriter<SaveGameRequest>,
    current_state: Res<State<GameState>>,
) {
    if !auto_save.enabled {
        return;
    }

    // Only auto-save while exploring or at surface base
    match current_state.get() {
        GameState::Exploring | GameState::StationDocked => {}
        _ => return,
    }

    auto_save.timer.tick(time.delta());
    if auto_save.timer.just_finished() {
        info!("Auto-save triggered");
        save_events.send(SaveGameRequest { slot: AUTO_SAVE_SLOT });
    }
}
