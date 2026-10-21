use bevy::prelude::*;
use crate::components::*;
use crate::resources::*;
use crate::celestial::resources::{GalaxyMap, GalaxyState, LootedPois, SystemStreamingManager, SystemDiscovery};
use crate::celestial::galaxy::{generate_galaxy_map, nearest_neighbors, nearest_neighbors_to_pos, WARM_NEIGHBOR_COUNT};
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
            // Ungated (like handle_load_request): it only acts when a
            // SaveGameRequest event is present, and quick-save (F5) fires that
            // during Exploring — a Paused-only gate here silently dropped those.
            // Must run before the state change lands, so the wipe is in place
            // by the time StationDocked spawns the starter ship and crew.
            .add_systems(Update, reset_for_new_game)
            .add_systems(Update, handle_save_request)
            .add_systems(Update, handle_load_request)
            .add_systems(Update, apply_pending_load)
            .add_systems(Update, rebuild_entities_from_save)
            .add_systems(Update, apply_module_health_overrides)
            .add_systems(Last, despawn_at_frame_end)
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

/// The player's own blocks, for saving. Not an AI ship's: enemy hull, modules
/// and crew carry the same components as the player's, and hull cells are
/// recorded from LOCAL transforms -- so an enemy plate at its own (2,3) was
/// written into the save as the player's (2,3). Autosave runs every two
/// minutes while exploring, which is exactly when enemies are around.
///
/// And not a section shot off the hull: severance unparents it, so its
/// transform is world-space, and it came back from a load bolted on at
/// whatever grid cell its world position divided out to.
type PlayerBlocks = (
    Without<crate::ai_ship::components::OwnedByAiShip>,
    Without<crate::combat::severance::DetachedSection>,
);

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
    ship_query: &Query<&Transform, With<Ship>>,
    // Every one of these MUST exclude AI ships -- see PlayerBlocks.
    module_query: &Query<(
        &Module,
        Option<&CustomModule>,
        Option<&crate::building::customization::tuning::WeaponTuning>,
        Option<&crate::building::customization::tuning::SelectedAmmo>,
    ), PlayerBlocks>,
    hull_query: &Query<(&HullSegment, &Transform), PlayerBlocks>,
    crew_query: &Query<(Entity, &CrewMember, Option<&CrewDuty>), Without<crate::ai_ship::components::OwnedByAiShip>>,
    current_state: &State<GameState>,
    galaxy_map: &GalaxyMap,
    streaming: &SystemStreamingManager,
    drifting_dead: &crate::crew::burial::DriftingDead,
    looted: &LootedPois,
    fuel: &FuelState,
    contracts: &crate::contracts::ContractState,
    reputation: &crate::contracts::FactionReputation,
    standing: &crate::diplomacy::FactionStanding,
    sim: &crate::ai_ship::components::WorldSimulation,
    stations: &crate::diplomacy::stations::FactionStations,
    war: &crate::celestial::territory::BorderWar,
) -> SaveData {
    let position = ship_query
        .single()
        .map(|t| t.translation.truncate())
        .unwrap_or(Vec2::ZERO);

    // Collect modules
    let modules: Vec<ModuleData> = module_query
        .iter()
        .map(|(module, custom, tuning, selected_ammo)| {
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
                tuning: tuning.copied(),
                selected_ammo: selected_ammo.map(|s| s.0),
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
        .map(|(_crew_entity, member, duty)| {
            CrewSaveData {
                name: member.name.clone(),
                health: member.health,
                max_health: member.max_health,
                oxygen: member.oxygen,
                morale: member.morale,
                assigned_module_grid: None,
                duty: duty.copied().unwrap_or_default(),
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
        ship: ShipBlueprint {
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
        current_system_id: streaming.loaded_system.unwrap_or(0),
        galaxy_seed: galaxy_map.galaxy_seed,
        drifting_dead: drifting_dead.bodies.clone(),
        galaxy_systems: galaxy_map.systems.iter().map(|s| SystemSaveData {
            id: s.id,
            discovery: s.discovery.as_u8(),
            last_updated: s.last_updated,
            resource_fraction_remaining: s.resource_fraction_remaining,
            looted_pois: looted.for_system(s.id).to_vec(),
            held: Some(crate::resources::HeldBy { owner: s.faction, capital: s.capital }),
        }).collect(),
        uncharted_galaxy_pos: streaming.loaded_system.is_none().then_some(streaming.current_galaxy_pos),
        fuel: Some(fuel.current_fuel),
        contracts: contracts.clone(),
        reputation: reputation.clone(),
        standing: Some(standing.clone()),
        world: sim.initialized.then(|| sim.snapshot()),
        destroyed_stations: stations.destroyed.clone(),
        border_war: Some(war.clone()),
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
    mut save_events: MessageReader<SaveGameRequest>,
    mut saved_events: MessageWriter<GameSaved>,
    mut notify_events: MessageWriter<ShowNotification>,
    depth_state: Res<DepthState>,
    hull_state: Res<HullState>,
    statistics: Res<Statistics>,
    inventory: Res<Inventory>,
    currency: Res<Currency>,
    unlocks: Res<Unlocks>,
    discovered_locations: Res<DiscoveredLocations>,
    // Bundled to stay under Bevy's 16-param cap: local world seed + galaxy state
    // + the drifting dead, and the run state that isn't the ship (fuel,
    // contracts, standing). This system is AT the cap; anything else new has
    // to join a tuple too.
    world_galaxy: (
        Res<WorldState>,
        Res<GalaxyMap>,
        Res<SystemStreamingManager>,
        Res<crate::crew::burial::DriftingDead>,
        Res<LootedPois>,
        Res<FuelState>,
        Res<crate::contracts::ContractState>,
        Res<crate::contracts::FactionReputation>,
        Res<crate::diplomacy::FactionStanding>,
        Res<crate::ai_ship::components::WorldSimulation>,
        Res<crate::diplomacy::stations::FactionStations>,
        Res<crate::celestial::territory::BorderWar>,
    ),
    current_state: Res<State<GameState>>,
    ship_query: Query<&Transform, With<Ship>>,
    module_query: Query<(
        &Module,
        Option<&CustomModule>,
        Option<&crate::building::customization::tuning::WeaponTuning>,
        Option<&crate::building::customization::tuning::SelectedAmmo>,
    ), PlayerBlocks>,
    hull_query: Query<(&HullSegment, &Transform), PlayerBlocks>,
    crew_query: Query<(Entity, &CrewMember, Option<&CrewDuty>), Without<crate::ai_ship::components::OwnedByAiShip>>,
) {
    let (world_state, galaxy_map, streaming, drifting_dead, looted, fuel, contracts, reputation, standing, sim, faction_stations, war) = &world_galaxy;
    for event in save_events.read() {
        let save_data = collect_save_data(
            event.slot,
            &depth_state,
            &hull_state,
            &statistics,
            &inventory,
            &currency,
            &unlocks,
            &discovered_locations,
            world_state,
            &ship_query,
            &module_query,
            &hull_query,
            &crew_query,
            &current_state,
            galaxy_map,
            streaming,
            drifting_dead,
            looted,
            fuel,
            contracts,
            reputation,
            standing,
            sim,
            faction_stations,
            war,
        );

        // Tier 3 customization is saved via customization_params field
        // (populated when ModuleCustomization query is added in a future refactor)

        let success = write_save(&save_data, event.slot);

        saved_events.write(GameSaved {
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

        notify_events.write(ShowNotification {
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
    mut load_events: MessageReader<LoadGameRequest>,
    mut notify_events: MessageWriter<ShowNotification>,
    mut pending: ResMut<PendingLoad>,
) {
    for event in load_events.read() {
        let path = save_path(event.slot);
        let data = match std::fs::read_to_string(&path) {
            Ok(d) => d,
            Err(e) => {
                error!("Failed to read save file {}: {}", path, e);
                notify_events.write(ShowNotification {
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
                notify_events.write(ShowNotification {
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
    run: (
        ResMut<FuelState>,
        ResMut<crate::contracts::ContractState>,
        ResMut<crate::contracts::FactionReputation>,
        ResMut<crate::diplomacy::FactionStanding>,
    ),
    mut commands: Commands,
) {
    let (mut fuel, mut contracts, mut reputation, mut standing) = run;
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
    // None of these were saved. A load from the menu came back with a full
    // tank (cleanup refills it) and an in-game one kept whatever was in it;
    // either way the contracts and standing were the session's, not the save's.
    fuel.current_fuel = save.fuel.unwrap_or(fuel.max_fuel).clamp(0.0, fuel.max_fuel);
    *contracts = save.contracts.clone();
    *reputation = save.reputation.clone();
    *standing = save.standing.clone().unwrap_or_default();

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
    mut loaded_events: MessageWriter<GameLoaded>,
    mut notify_events: MessageWriter<ShowNotification>,
    mut next_state: ResMut<NextState<GameState>>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    registry: Res<crate::building::registry::ModuleRegistry>,
    ship_query: Query<Entity, With<Ship>>,
    // Mirror of the save-side defect: unscoped, these despawned every live AI
    // ship's blocks and crew, gutting them into empty root entities on load.
    module_entities: Query<Entity, (With<Module>, Without<crate::ai_ship::components::OwnedByAiShip>)>,
    hull_entities: Query<Entity, (With<HullSegment>, Without<crate::ai_ship::components::OwnedByAiShip>)>,
    crew_entities: Query<Entity, (With<CrewMember>, Without<crate::ai_ship::components::OwnedByAiShip>)>,
    mut galaxy_map: ResMut<GalaxyMap>,
    mut streaming: ResMut<SystemStreamingManager>,
    mut drifting_dead: ResMut<crate::crew::burial::DriftingDead>,
    // The spawned star system and its population. AT Bevy's 16-param cap:
    // anything new joins one of these tuples.
    world: (
        ResMut<GalaxyState>,
        ResMut<LootedPois>,
        ResMut<crate::ai_ship::components::WorldSimulation>,
        Res<crate::vfx::procedural_textures::CelestialTextures>,
        Res<Time>,
        Query<Entity, With<crate::celestial::components::StarSystemMember>>,
        Query<Entity, With<crate::ai_ship::components::AiShip>>,
        ResMut<crate::diplomacy::stations::FactionStations>,
        ResMut<crate::celestial::territory::BorderWar>,
    ),
    // State the old ship was in the middle of, that the loaded one is not.
    flight: (
        ResMut<crate::ship::rebuild::RebuildQueue>,
        ResMut<crate::ship::towing::Towing>,
        ResMut<crate::ui::PendingWarpTarget>,
    ),
) {
    let (mut galaxy, mut looted, mut sim, textures, time, members, ai_ships, mut faction_stations, mut border_war) = world;
    let (mut rebuild_queue, mut towing, mut dash_target) = flight;
    let Some(save) = pending.save_data.take() else { return };
    let slot = pending.slot;

    // ---- Despawn existing ship entities ----
    // Hidden now, gone at the end of the frame (despawn_at_frame_end).
    // Despawned here, mid-frame, any system that had already queued a
    // command for one of them this frame crashed the load; which ones did
    // depended on scheduling order, so it came and went as systems were added.
    for entity in module_entities.iter().chain(hull_entities.iter()).chain(crew_entities.iter()) {
        commands.entity(entity).try_insert((DespawnAtFrameEnd, Visibility::Hidden));
    }

    // ---- Respawn ship at saved position ----
    // The same root a new run gets (ship::ship_root_bundle). Over an existing
    // root it also zeroes the old ship's motion: only the Transform used to be
    // replaced, so a quick-load mid-burn arrived still moving, spinning and
    // throttled up.
    let root = crate::ship::ship_root_bundle(save.position);
    let ship_entity = if let Ok(existing) = ship_query.single() {
        commands
            .entity(existing)
            .insert(root)
            // A jump or dash charging when F9 was pressed is the old ship's.
            .remove::<(crate::celestial::components::WarpCharging, crate::ui::MapWarpCharging)>();
        existing
    } else {
        commands.spawn(root).id()
    };
    commands.entity(ship_entity).insert(Depth(save.current_depth));
    *towing = default();
    dash_target.0 = None;
    // Ghosts are the old ship's missing blocks; left queued, crew rebuilt
    // them onto the loaded one. Their outline sprites hang off the root,
    // which survives the load, so they go explicitly.
    for ghost in rebuild_queue.ghosts.drain(..) {
        for sprite in ghost.sprites {
            commands.entity(sprite).try_despawn();
        }
    }

    // ---- Respawn hull segments ----
    for hull_data in &save.hull_segments {
        let hull_texture = asset_server.load(
            crate::sprite_map::hull_layer_sprite_path(hull_data.material, hull_data.hull_layer)
        );
        // Same tint per layer as blueprint::spawn_ship_from_design and
        // building::process_hull_placement, so a loaded hull matches a built one.
        let color = match hull_data.hull_layer {
            HullLayer::Outer | HullLayer::Hallway => Color::WHITE,
            HullLayer::Inner => Color::srgb(0.9, 0.9, 0.9),
            HullLayer::Void => Color::srgb(0.5, 0.5, 0.6),
            HullLayer::BulkheadDoor => Color::srgb(0.9, 0.8, 0.7),
        };
        commands.spawn((
            (Sprite {
                    image: hull_texture,
                    color,
                    // Full cell, matching blueprint.rs -- a loaded save used to
                    // come back with the old 2-unit gap between every plate.
                    custom_size: Some(Vec2::new(66.0, 66.0)),
                    ..default()
                }, Transform::from_xyz(
                    hull_data.grid_position.x as f32 * 66.0,
                    hull_data.grid_position.y as f32 * 66.0 - 33.0,
                    0.1,
                )),
            // Every other hull spawn carries these two. Without Block a plate
            // resolved as armour thickness 0 for the rest of the session, and
            // without the base colour damage tinting had nothing to darken from.
            BaseSpriteColor(color),
            crate::building::Block::hull(hull_data.grid_position, hull_data.material),
            BaseHullStats {
                max_health: hull_data.max_health,
                radiation_shielding: hull_data.radiation_shielding,
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
        )).insert(ChildOf(ship_entity));
    }

    // ---- Respawn modules ----
    for module_data in &save.ship.modules {
        let entity = crate::ship::spawn_module(
            &mut commands,
            &asset_server,
            ship_entity,
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
        // Restore stat tuning + loaded ammo — inserting counts as Changed, so
        // apply_weapon_tuning recomputes the live stats on the next frame.
        if let Some(tuning) = module_data.tuning {
            commands.entity(entity).insert(tuning);
        }
        if let Some(ammo) = module_data.selected_ammo {
            commands.entity(entity).insert(
                crate::building::customization::tuning::SelectedAmmo(ammo)
            );
        }
    }

    // ---- Respawn crew ----
    let mut roster_members = Vec::new();
    for (i, crew_data) in save.crew.iter().enumerate() {
        let crew_entity = commands.spawn((
            (Sprite {
                    color: Color::srgb(0.8, 0.6, 0.5),
                    custom_size: Some(Vec2::new(16.0, 16.0)),
                    ..default()
                }, Transform::from_xyz(
                    (i as f32 - 3.5) * 20.0,
                    0.0,
                    // Above the modules, like every other crew spawn — the
                    // walk planner pulls them onto real deck from here.
                    crate::crew::walking::CREW_Z,
                )),
            crew_data.duty,
            CrewMember {
                name: crew_data.name.clone(),
                health: crew_data.health,
                max_health: crew_data.max_health,
                oxygen: crew_data.oxygen,
                morale: crew_data.morale,
                state: CrewState::Idle,
            },
        )).insert(ChildOf(ship_entity)).id();
        roster_members.push(crew_entity);
    }
    commands.insert_resource(CrewRoster { members: roster_members });
    // Auto-assign will re-staff stations on next tick

    // ---- Restore the drifting dead ----
    // Positions are world-space per system; the entities for whichever system
    // the player lands in are spawned by burial::sync_drifting_dead on its
    // next run, so nothing is spawned here.
    drifting_dead.bodies = save.drifting_dead.clone();
    drifting_dead.reseed();

    // ---- Restore galaxy state ----
    // Regenerate the deterministic layout from the saved seed, then overlay the
    // persisted per-system mutable state (discovery / depletion) and re-point the
    // streaming manager at the system the player was in. Legacy saves have
    // galaxy_seed == 0 → leave the freshly-generated galaxy alone.
    if save.galaxy_seed != 0 {
        *galaxy_map = generate_galaxy_map(save.galaxy_seed);
        looted.0.clear();
        for sd in &save.galaxy_systems {
            if let Some(sys) = galaxy_map.systems.iter_mut().find(|s| s.id == sd.id) {
                sys.discovery = SystemDiscovery::from_u8(sd.discovery);
                sys.last_updated = sd.last_updated;
                sys.resource_fraction_remaining = sd.resource_fraction_remaining;
                if let Some(held) = sd.held {
                    sys.faction = held.owner;
                    sys.capital = held.capital;
                    sys.danger_tier = crate::celestial::territory::danger_of(sys);
                }
            }
            for &poi_slot in &sd.looted_pois {
                looted.mark(sd.id, poi_slot);
            }
        }
        if let Some(pos) = save.uncharted_galaxy_pos {
            // Saved after a blind jump into empty space: no system is Hot.
            streaming.loaded_system = None;
            streaming.current_galaxy_pos = pos;
            streaming.warm_systems = nearest_neighbors_to_pos(&galaxy_map, pos, None, WARM_NEIGHBOR_COUNT);
        } else {
            let cur = save.current_system_id;
            streaming.loaded_system = Some(cur);
            streaming.current_galaxy_pos = galaxy_map.systems.iter()
                .find(|s| s.id == cur)
                .map(|s| s.galaxy_pos)
                .unwrap_or(Vec2::ZERO);
            streaming.warm_systems = nearest_neighbors(&galaxy_map, cur, WARM_NEIGHBOR_COUNT);
        }

        // ---- The star system itself ----
        // This used to stop at pointing the streaming manager at the saved
        // system. Nothing despawned what was there or spawned what should be:
        // an in-game load kept the current system's bodies around a ship now
        // somewhere else, and a load from the menu got Haven whatever the
        // save said. Clear every system, then arrive in the saved one the way
        // a warp does.
        for entity in members.iter() {
            commands.entity(entity).try_despawn();
        }
        // The list only, not galaxy_time: that is the orbital clock.
        galaxy.systems.clear();
        if let Some(cur) = streaming.loaded_system {
            let now = time.elapsed_secs_f64();
            if let Some(info) = crate::celestial::galaxy::load_system(
                &mut commands, &asset_server, &textures, &mut galaxy_map, cur, now, &looted,
            ) {
                galaxy.current_system = cur;
                galaxy.next_system_id = cur + 1;
                galaxy.systems.push(info);
                galaxy.total_bodies = 1;
            }
        }
        // ...and its people, as the save left them: every fleet where it was,
        // as hurt as it was, the dead still dead. The ships on screen now
        // belong to the world being left; they go, and the saved ones
        // materialise from the restored simulation. An older save without a
        // world keeps the running one.
        if let Some(world) = &save.world {
            // Tagged, not despawned here: every other system this frame may
            // already hold a command for these ships, and a despawn in the
            // middle of the frame turned those into crashes. They go at the
            // end of the frame (ai_ship::clear_ships_left_by_load).
            for ship in ai_ships.iter() {
                commands.entity(ship).try_insert((
                    crate::ai_ship::components::LeftByLoad { frames: 3 },
                    Visibility::Hidden,
                    Transform::from_xyz(1.0e8, 1.0e8, 0.0),
                ));
            }
            sim.restore(world);
            faction_stations.live = None;
            faction_stations.destroyed = save.destroyed_stations.clone();
            *border_war = save.border_war.clone().unwrap_or_default();
        }
        // A running simulation populates the saved system
        // and its neighbours now (a no-op for any already populated). One not
        // yet started -- a load from the menu -- is left to
        // init_world_simulation, which does the same on entering Exploring.
        if sim.initialized {
            if let Some(cur) = streaming.loaded_system {
                crate::ai_ship::simulation::ensure_system_population(&mut sim, &galaxy_map, cur);
            }
            for warm_id in streaming.warm_systems.clone() {
                crate::ai_ship::simulation::ensure_system_population(&mut sim, &galaxy_map, warm_id);
            }
        }
        info!(
            "Galaxy restored: seed={}, current system={:?}, {} systems",
            save.galaxy_seed, streaming.loaded_system, galaxy_map.systems.len()
        );
    }

    // ---- Set game state ----
    if save.was_exploring {
        next_state.set(GameState::Exploring);
    } else {
        next_state.set(GameState::StationDocked);
    }

    loaded_events.write(GameLoaded { slot, success: true });

    let slot_name = if slot == AUTO_SAVE_SLOT {
        "auto-save".to_string()
    } else {
        format!("slot {}", slot + 1)
    };
    notify_events.write(ShowNotification {
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
    mut save_events: MessageWriter<SaveGameRequest>,
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
        save_events.write(SaveGameRequest { slot: AUTO_SAVE_SLOT });
    }
}

/// Wipe the previous run when a new expedition starts.
///
/// ROADMAP called this out as open: "New Expedition doesn't start a new
/// expedition. It sets the state and nothing else." Everything below was
/// inherited from the last run, which made a fresh start quietly not fresh.
///
/// It became load-bearing once the story landed. `Statistics.logs_found`
/// survived into the new run, and the ending keys on the finale entry, so a
/// player who finished once would start their next expedition and have the
/// ending fire immediately.
///
/// `Unlocks` is deliberately NOT reset: that is meta-progression across runs,
/// which is the one thing that is supposed to carry.
pub fn reset_for_new_game(
    mut requests: MessageReader<NewExpeditionRequest>,
    mut currency: ResMut<Currency>,
    mut inventory: ResMut<Inventory>,
    mut statistics: ResMut<Statistics>,
    mut contracts: ResMut<crate::contracts::ContractState>,
    mut reputation: ResMut<crate::contracts::FactionReputation>,
    mut standing: ResMut<crate::diplomacy::FactionStanding>,
    mut victory: ResMut<VictoryState>,
    mut death_cause: ResMut<DeathCause>,
    mut dead: ResMut<crate::crew::burial::DriftingDead>,
    mut finale: ResMut<crate::narrative::FinaleFound>,
    mut galaxy: ResMut<crate::celestial::resources::GalaxyMap>,
    mut streaming: ResMut<crate::celestial::resources::SystemStreamingManager>,
    world: (
        ResMut<GalaxyState>,
        ResMut<crate::ai_ship::components::WorldSimulation>,
        ResMut<LootedPois>,
        ResMut<crate::diplomacy::stations::FactionStations>,
        ResMut<crate::celestial::territory::BorderWar>,
    ),
) {
    let (mut spawned, mut sim, mut looted, mut faction_stations, mut border_war) = world;
    if requests.read().next().is_none() {
        return;
    }
    requests.clear();

    *currency = Currency::default();
    *inventory = Inventory::default();
    *statistics = Statistics::default();
    *contracts = crate::contracts::ContractState::default();
    *reputation = crate::contracts::FactionReputation::default();
    *standing = crate::diplomacy::FactionStanding::default();
    *victory = VictoryState::default();
    *death_cause = DeathCause::default();
    // The dead are permanent within a run, on purpose. They are not permanent
    // across runs: those were a different expedition's crew.
    *dead = crate::crew::burial::DriftingDead::default();
    finale.0 = false;

    // Emptying the systems list is the trigger: generate_galaxy_on_enter
    // early-returns when the galaxy is already populated, so clearing it is
    // what makes the next launch roll a fresh one.
    galaxy.systems.clear();
    *streaming = crate::celestial::resources::SystemStreamingManager::default();
    // The spawned side of the galaxy and its population. Their entities are
    // already gone (ship::cleanup_game_entities, on entering the menu); these
    // are the records that would otherwise make the next run skip spawning
    // its own -- spawn_initial_system only fills an empty GalaxyState, and an
    // `initialized` simulation never populates again.
    *spawned = GalaxyState::default();
    *sim = crate::ai_ship::components::WorldSimulation::default();
    *faction_stations = crate::diplomacy::stations::FactionStations::default();
    *border_war = crate::celestial::territory::BorderWar::default();
    // Derelicts stripped last run are full again in a new galaxy.
    *looted = LootedPois::default();

    info!("new expedition: previous run cleared");
}

/// Left behind by a load: hidden at once, despawned in `Last`, after every
/// system has applied what it queued this frame.
#[derive(Component)]
pub struct DespawnAtFrameEnd;

pub fn despawn_at_frame_end(mut commands: Commands, doomed: Query<Entity, With<DespawnAtFrameEnd>>) {
    for entity in doomed.iter() {
        commands.entity(entity).try_despawn();
    }
}
