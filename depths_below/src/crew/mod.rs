use bevy::prelude::*;
use crate::states::GameState;
use crate::components::*;
use crate::resources::*;
use crate::events::*;
use crate::building::rooms::RoomMap;
use crate::building::GridOccupancy;

pub mod eva_salvage;
use eva_salvage::EvaSalvaging;

pub struct CrewPlugin;

impl Plugin for CrewPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<CrewRoster>()
            .init_resource::<StaffingState>()
            .init_resource::<AutoAssignTimer>()
            // Staffing / efficiency systems run at both StationDocked and Exploring
            // so the HUD shows correct crew/station counts at the surface.
            .add_systems(
                Update,
                (
                    compute_module_efficiency,
                    update_staffing_state,
                    auto_assign_crew,
                    reconcile_hired_crew,
                    crew_arrive_with_quarters,
                )
                    .run_if(in_state(GameState::Exploring)
                        .or_else(in_state(GameState::StationDocked))
                        .or_else(in_state(GameState::Docked))),
            )
            // Gameplay crew systems only run while Exploring
            .add_systems(
                Update,
                (
                    update_crew_needs,
                    update_crew_room_location,
                    crew_emergency_dispatch.after(update_crew_room_location),
                    update_crew_ai.after(crew_emergency_dispatch),
                    crew_fire_suppression.after(update_crew_ai),
                    crew_repair_system.after(update_crew_ai),
                    check_crew_suffocation,
                    handle_crew_death,
                    medbay_healing,
                    messhall_morale,
                    recroom_morale_floor,
                    training_room_boost,
                    engineering_station_boost,
                )
                    .run_if(in_state(GameState::Exploring)),
            )
            // EVA salvage: crew ferry loot from wrecks (see eva_salvage.rs)
            .add_systems(
                Update,
                (
                    eva_salvage::order_salvage_detail,
                    eva_salvage::run_salvage_detail,
                    eva_salvage::eva_blast_damage,
                )
                    .chain()
                    .run_if(in_state(GameState::Exploring)),
            )
            .add_systems(OnExit(GameState::Exploring), eva_salvage::abort_eva_on_exit);
    }
}

/// Timer for periodic auto-assignment
#[derive(Resource)]
pub struct AutoAssignTimer {
    pub timer: Timer,
}

impl Default for AutoAssignTimer {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(2.0, TimerMode::Repeating),
        }
    }
}

/// INTERIM CREW SUPPLY — until station hiring exists, crew come WITH
/// the bunks: placing a quarters module (Barracks etc.) during a refit
/// spawns its berths' worth of new hands, as if they signed on with the
/// accommodation. Starter crew are unaffected (the initial ship spawn
/// doesn't emit ModulePlaced). Berths come from the registry def, not
/// the entity — the companion component may not be flushed yet in the
/// frame the placement event fires.
fn crew_arrive_with_quarters(
    mut commands: Commands,
    mut placed_events: MessageReader<ModulePlaced>,
    registry: Res<crate::building::ModuleRegistry>,
    ship_query: Query<Entity, With<Ship>>,
    mut roster: ResMut<CrewRoster>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    use rand::Rng;
    const NAMES: [&str; 12] = [
        "Reyes", "Okonkwo", "Falk", "Ito", "Marsh", "Deng",
        "Ferrara", "Boone", "Ades", "Kowal", "Nyx", "Sorren",
    ];

    let Ok(ship) = ship_query.single() else { return };
    for event in placed_events.read() {
        let crate::building::registry::CompanionData::Quarters { berths } =
            registry.get(event.module_type).companion
        else {
            continue;
        };

        let mut rng = rand::thread_rng();
        for i in 0..berths {
            let name = NAMES[rng.gen_range(0..NAMES.len())];
            let crew = commands
                .spawn((
                    (
                        Sprite {
                            color: Color::srgb(0.8, 0.6, 0.5),
                            custom_size: Some(Vec2::new(16.0, 16.0)),
                            ..default()
                        },
                        Transform::from_xyz(i as f32 * 14.0 - 20.0, -20.0, 0.5),
                    ),
                    CrewMember {
                        name: name.to_string(),
                        health: 100.0,
                        max_health: 100.0,
                        oxygen: 100.0,
                        morale: 100.0,
                        state: CrewState::Idle,
                    },
                ))
                .insert(ChildOf(ship))
                .id();
            roster.members.push(crew);
        }

        notifications.write(ShowNotification {
            message: format!(
                "{} crew signed on with the {}.",
                berths,
                event.module_type.name()
            ),
            notification_type: NotificationType::Success,
            duration: 3.0,
        });
    }
}

/// Spawns the initial crew (8 crew, no skills)
pub fn spawn_starter_crew(
    mut commands: Commands,
    ship_query: Query<Entity, With<Ship>>,
    existing_crew: Query<Entity, With<CrewMember>>,
    mut roster: ResMut<CrewRoster>,
) {
    // Guard: don't spawn duplicate crew
    if !existing_crew.is_empty() {
        return;
    }
    let Ok(ship) = ship_query.single() else {
        return;
    };

    let crew_names = ["Jones", "Smith", "Chen", "Morgan", "Rivera", "Volkov", "Tanaka", "Okafor"];

    for (i, name) in crew_names.iter().enumerate() {
        let crew = commands.spawn((
            (Sprite {
                    color: Color::srgb(0.8, 0.6, 0.5),
                    custom_size: Some(Vec2::new(16.0, 16.0)),
                    ..default()
                }, Transform::from_xyz(
                    (i as f32 - 3.5) * 20.0,
                    0.0,
                    0.5,
                )),
            CrewMember {
                name: name.to_string(),
                health: 100.0,
                max_health: 100.0,
                oxygen: 100.0,
                morale: 100.0,
                state: CrewState::Idle,
            },
        )).insert(ChildOf(ship)).id();

        roster.members.push(crew);
    }

    info!("Spawned {} crew members", crew_names.len());
}

/// Computes ModuleEfficiency for all modules with a CrewStation.
/// staffing_factor: 0.0 unstaffed, 1.0 staffed (crew alive, aboard, and
/// not panicking/unconscious) — a station nobody operates DOES NOT RUN.
/// That's the teeth behind crew scarcity: send everyone out on salvage
/// and the unmanned reactors/engines/guns go dark until they're back.
/// value = damage_efficiency * staffing_factor
fn compute_module_efficiency(
    mut commands: Commands,
    mut station_query: Query<(Entity, &Module, &mut CrewStation)>,
    crew_query: Query<&CrewMember>,
) {
    for (entity, module, mut station) in station_query.iter_mut() {
        let ratio = if module.max_health > 0.0 { module.health / module.max_health } else { 1.0 };
        let damage_eff = ModuleDamageState::from_health_ratio(ratio).efficiency();

        let staffing_factor = if let Some(crew_entity) = station.assigned_crew {
            if let Ok(crew) = crew_query.get(crew_entity) {
                if crew.health > 0.0
                    && crew.state != CrewState::Panicking
                    && crew.state != CrewState::Unconscious
                    && crew.state != CrewState::Salvaging
                {
                    1.0
                } else {
                    // Dead/panicking/unconscious/EVA crew — clear assignment
                    station.assigned_crew = None;
                    0.0
                }
            } else {
                // Crew entity no longer exists — clear assignment
                station.assigned_crew = None;
                0.0
            }
        } else {
            0.0
        };

        // try_insert: wreck modules carry CrewStation too, and a drill or
        // EVA detail may have dismantled (despawned) this block this frame
        commands.entity(entity).try_insert(ModuleEfficiency {
            value: damage_eff * staffing_factor,
            staffing_factor,
        });
    }
}

/// Counts total berths, crew, staffed/total stations. Writes to StaffingState.
fn update_staffing_state(
    quarters_query: Query<(&Quarters, &Module)>,
    station_query: Query<&CrewStation>,
    crew_query: Query<&CrewMember>,
    mut staffing: ResMut<StaffingState>,
) {
    let mut total_berths = 0u32;
    for (quarters, module) in quarters_query.iter() {
        if module.is_active && module.health > 0.0 {
            total_berths += quarters.berths;
        }
    }

    let mut staffed = 0u32;
    let mut total = 0u32;
    for station in station_query.iter() {
        total += 1;
        if let Some(crew_entity) = station.assigned_crew {
            // Only count as staffed if the crew is alive and still exists
            if let Ok(crew) = crew_query.get(crew_entity) {
                if crew.health > 0.0 {
                    staffed += 1;
                }
            }
        }
    }

    // Count living crew
    let alive_crew = crew_query.iter().filter(|c| c.health > 0.0).count() as u32;

    staffing.total_berths = total_berths;
    staffing.total_crew = alive_crew;
    staffing.staffed_stations = staffed;
    staffing.total_stations = total;
}

/// Priority-based auto-assignment of crew to stations.
fn auto_assign_crew(
    time: Res<Time>,
    mut timer: ResMut<AutoAssignTimer>,
    mut station_query: Query<(Entity, &mut CrewStation, Has<KeepManned>)>,
    crew_query: Query<(Entity, &CrewMember)>,
) {
    timer.timer.tick(time.delta());
    if !timer.timer.just_finished() {
        return;
    }

    // Collect all crew currently assigned to any station
    let mut assigned_crew: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    for (_, station, _) in station_query.iter() {
        if let Some(crew_entity) = station.assigned_crew {
            assigned_crew.insert(crew_entity);
        }
    }

    // Clean up dead/despawned crew from stations
    for (_, mut station, _) in station_query.iter_mut() {
        if let Some(crew_entity) = station.assigned_crew {
            if let Ok((_, crew)) = crew_query.get(crew_entity) {
                if crew.health <= 0.0 {
                    station.assigned_crew = None;
                    assigned_crew.remove(&crew_entity);
                }
            } else {
                // Entity no longer exists
                station.assigned_crew = None;
                assigned_crew.remove(&crew_entity);
            }
        }
    }

    // Collect unfilled stations (priority > 0, not manually assigned)
    let mut unfilled: Vec<(Entity, u8, bool)> = Vec::new();
    for (entity, station, pinned) in station_query.iter() {
        if station.priority > 0 && !station.manually_assigned && station.assigned_crew.is_none() {
            unfilled.push((entity, station.priority, pinned));
        }
    }

    // Pinned (keep-manned) posts staff first, then by priority descending
    unfilled.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.cmp(&a.1)));

    // Collect available crew (alive, not panicking/unconscious, not assigned)
    let mut available_crew: Vec<Entity> = Vec::new();
    for (entity, crew) in crew_query.iter() {
        if crew.health > 0.0
            && crew.state != CrewState::Panicking
            && crew.state != CrewState::Unconscious
            && crew.state != CrewState::Salvaging
            && !assigned_crew.contains(&entity)
        {
            available_crew.push(entity);
        }
    }

    // Assign in order
    let mut crew_idx = 0;
    for (station_entity, _priority, _pinned) in unfilled {
        if crew_idx >= available_crew.len() {
            break;
        }
        let crew_entity = available_crew[crew_idx];
        crew_idx += 1;

        if let Ok((_, mut station, _)) = station_query.get_mut(station_entity) {
            station.assigned_crew = Some(crew_entity);
        }
    }
}

/// Updates crew needs (oxygen, morale)
fn update_crew_needs(
    time: Res<Time>,
    oxygen_state: Res<OxygenState>,
    depth_state: Res<DepthState>,
    // EVA crew are on suit systems — needs frozen while outside
    mut crew_query: Query<&mut CrewMember, Without<EvaSalvaging>>,
) {
    let oxygen_available = oxygen_state.oxygen_balance >= 0.0;

    for mut crew in crew_query.iter_mut() {
        if crew.health <= 0.0 {
            continue;
        }

        if !oxygen_available {
            crew.oxygen = (crew.oxygen - 10.0 * time.delta_secs()).max(0.0);
        } else {
            crew.oxygen = (crew.oxygen + 20.0 * time.delta_secs()).min(100.0);
        }

        if crew.oxygen < 50.0 {
            crew.morale = (crew.morale - 5.0 * time.delta_secs()).max(0.0);
        } else if depth_state.current_depth > 500.0 {
            // Deep-space dread erodes morale but bottoms out ABOVE both
            // the panic threshold (20) and the recovery threshold (30):
            // depth alone makes crew jumpy, never permanently catatonic.
            // Draining to 0 locked every deep-zone crew into panic forever
            // — nobody could man a station or crew a salvage detail out
            // where the wrecks actually are.
            crew.morale = (crew.morale - 5.0 * time.delta_secs()).max(35.0);
        } else {
            crew.morale = (crew.morale + 1.0 * time.delta_secs()).min(100.0);
        }
    }
}

/// Maps each crew member's world position to a grid position and room via RoomMap.
fn update_crew_room_location(
    mut commands: Commands,
    mut crew_query: Query<(Entity, &GlobalTransform, Option<&mut CrewRoomLocation>), (With<CrewMember>, Without<EvaSalvaging>)>,
    room_map: Res<RoomMap>,
) {
    for (entity, global_transform, location) in crew_query.iter_mut() {
        let world_pos = global_transform.translation();
        let grid = IVec2::new(
            (world_pos.x / 66.0).round() as i32,
            ((world_pos.y + 33.0) / 66.0).round() as i32,
        );
        let room_id = room_map.tile_to_room.get(&grid).copied();

        if let Some(mut loc) = location {
            loc.room_id = room_id;
            loc.grid_position = grid;
        } else {
            // try_insert: the crew member may die and despawn this frame
            commands.entity(entity).try_insert(CrewRoomLocation {
                room_id,
                grid_position: grid,
            });
        }
    }
}

/// Scans for rooms with decompression or fire and dispatches idle crew to handle emergencies.
/// Temporarily clears non-manual CrewStation assignments for dispatched crew.
/// Room-scoped both ways: only crew IN an emergency room get flagged
/// (repair/suppression power is room-local and crew can't walk between
/// rooms, so flagging distant crew just locked them in Repairing doing
/// nothing — which starved every other job, e.g. salvage details), and
/// Repairing crew whose room is calm get released back to Idle.
fn crew_emergency_dispatch(
    ship_query: Query<Entity, With<Ship>>,
    child_query: Query<&ChildOf>,
    mut crew_query: Query<(Entity, &mut CrewMember, Option<&CrewRoomLocation>)>,
    fire_query: Query<(Entity, &Module), With<OnFire>>,
    room_map: Res<RoomMap>,
    mut station_query: Query<(Entity, &mut CrewStation)>,
    mut dispatch_events: MessageWriter<CrewDispatched>,
) {
    // Build priority list of emergency rooms: decompression first, then fire
    let mut emergency_rooms: Vec<(usize, DispatchReason)> = Vec::new();

    for room in room_map.rooms.iter() {
        if room.is_breached && room.air_level < 1.0 {
            emergency_rooms.push((room.id, DispatchReason::Decompression));
        }
    }

    // Check for rooms with fire — our modules only; a burning wreck's
    // grid positions can phantom-match our room map's tiles.
    let ship = ship_query.single().ok();
    for (entity, module) in fire_query.iter() {
        if child_query.get(entity).ok().map(|p| p.0) != ship {
            continue;
        }
        if let Some(&room_id) = room_map.tile_to_room.get(&module.grid_position) {
            if !emergency_rooms.iter().any(|(id, _)| *id == room_id) {
                emergency_rooms.push((room_id, DispatchReason::Fire));
            }
        }
    }

    // Collect crew assigned to stations (to know who to pull)
    let mut station_assignments: std::collections::HashMap<Entity, Entity> = std::collections::HashMap::new();
    for (station_entity, station) in station_query.iter() {
        if let Some(crew_entity) = station.assigned_crew {
            if !station.manually_assigned {
                station_assignments.insert(crew_entity, station_entity);
            }
        }
    }

    for (entity, mut crew, location) in crew_query.iter_mut() {
        if crew.health <= 0.0 {
            continue;
        }
        let room = location.and_then(|l| l.room_id);
        let emergency_here = room.and_then(|r| {
            emergency_rooms.iter().find(|(id, _)| *id == r).copied()
        });

        match (crew.state, emergency_here) {
            (CrewState::Idle, Some((room_id, reason))) => {
                crew.state = CrewState::Repairing;

                // Clear station assignment if not manually assigned
                if let Some(station_entity) = station_assignments.get(&entity) {
                    if let Ok((_, mut station)) = station_query.get_mut(*station_entity) {
                        station.assigned_crew = None;
                    }
                }

                dispatch_events.write(CrewDispatched {
                    crew: entity,
                    room_id,
                    reason,
                });
            }
            (CrewState::Repairing, None) => {
                crew.state = CrewState::Idle;
            }
            _ => {}
        }
    }
}

/// Updates crew AI behavior — now aware of both decompression and fires.
fn update_crew_ai(
    ship_query: Query<Entity, With<Ship>>,
    child_query: Query<&ChildOf>,
    hull_query: Query<(Entity, &HullSegment, &Transform)>,
    fire_query: Query<Entity, With<OnFire>>,
    // EVA crew's state machine is owned by eva_salvage while they're out
    mut crew_query: Query<&mut CrewMember, Without<EvaSalvaging>>,
) {
    let Ok(ship) = ship_query.single() else { return };
    // Danger must be OUR danger — unscoped, any holed/burning wreck
    // drifting nearby kept the crew stuck in Repairing forever.
    let has_depressurized = hull_query.iter().any(|(entity, hull, _)| {
        hull.is_depressurized && child_query.get(entity).is_ok_and(|p| p.0 == ship)
    });
    let has_fires = fire_query
        .iter()
        .any(|entity| child_query.get(entity).is_ok_and(|p| p.0 == ship));
    let has_danger = has_depressurized || has_fires;

    for mut crew in crew_query.iter_mut() {
        if crew.health <= 0.0 {
            continue;
        }

        match crew.state {
            CrewState::Repairing => {
                // Return to idle when no more danger
                if !has_danger {
                    crew.state = CrewState::Idle;
                }
            }
            CrewState::Panicking => {
                if crew.morale > 30.0 {
                    crew.state = CrewState::Idle;
                }
            }
            _ => {}
        }

        if crew.oxygen < 20.0 || crew.morale < 20.0 {
            crew.state = CrewState::Panicking;
        }
    }
}

/// Crew in Repairing state suppress fires in their room.
/// Since skills are removed, each crew member contributes a flat suppression value.
fn crew_fire_suppression(
    time: Res<Time>,
    mut commands: Commands,
    ship_query: Query<Entity, With<Ship>>,
    child_query: Query<&ChildOf>,
    crew_query: Query<(&CrewMember, &CrewRoomLocation)>,
    mut fire_query: Query<(Entity, &mut OnFire, &Module, &mut Sprite), Without<DestroyedModule>>,
    room_map: Res<RoomMap>,
    mut extinguish_events: MessageWriter<FireExtinguished>,
) {
    let dt = time.delta_secs();

    // Build per-room suppression power from repairing crew (flat 1.0 per crew)
    let mut room_suppression: std::collections::HashMap<usize, f32> = std::collections::HashMap::new();
    for (crew, location) in crew_query.iter() {
        if crew.state != CrewState::Repairing || crew.health <= 0.0 {
            continue;
        }
        if let Some(room_id) = location.room_id {
            *room_suppression.entry(room_id).or_insert(0.0) += 0.8;
        }
    }

    if room_suppression.is_empty() {
        return;
    }

    // Apply suppression to fires — our modules only (see dispatch note)
    let ship = ship_query.single().ok();
    for (entity, mut fire, module, mut sprite) in fire_query.iter_mut() {
        if child_query.get(entity).ok().map(|p| p.0) != ship {
            continue;
        }
        let Some(&room_id) = room_map.tile_to_room.get(&module.grid_position) else {
            continue;
        };
        let Some(&suppression) = room_suppression.get(&room_id) else {
            continue;
        };

        fire.intensity -= suppression * 0.03 * dt;
        fire.damage_per_second = 8.0 * fire.intensity.max(0.0);

        if fire.intensity < 0.05 {
            commands.entity(entity).remove::<OnFire>();
            sprite.color = Color::srgb(0.2, 0.2, 0.2);
            extinguish_events.write(FireExtinguished {
                module: entity,
                cause: FireExtinguishCause::CrewSuppressed,
            });
        }
    }
}

/// Checks for crew suffocation
fn check_crew_suffocation(
    time: Res<Time>,
    config: Res<GameConfig>,
    mut crew_query: Query<(Entity, &mut CrewMember), Without<EvaSalvaging>>,
    mut damage_events: MessageWriter<CrewDamaged>,
    mut death_events: MessageWriter<CrewDied>,
) {
    for (entity, mut crew) in crew_query.iter_mut() {
        if crew.health <= 0.0 {
            continue;
        }

        if crew.oxygen <= 0.0 {
            let damage = config.suffocation_damage_rate * time.delta_secs();
            crew.health -= damage;

            damage_events.write(CrewDamaged {
                crew: entity,
                amount: damage,
                source: CrewDamageSource::Suffocation,
            });

            if crew.health <= 0.0 {
                death_events.write(CrewDied {
                    crew: entity,
                    name: crew.name.clone(),
                    cause: CrewDamageSource::Suffocation,
                });
            }
        }
    }
}

/// Room-local crew repair system with RepairBay boost.
/// Crew contribute flat repair power (no skills).
fn crew_repair_system(
    time: Res<Time>,
    crew_query: Query<(&CrewMember, &CrewRoomLocation)>,
    repair_bays: Query<(&Module, &RepairSystem), Without<DestroyedModule>>,
    mut hull_query: Query<&mut HullSegment>,
    mut module_query: Query<&mut Module, (Without<DestroyedModule>, Without<RepairSystem>)>,
    room_map: Res<RoomMap>,
    occupancy: Res<GridOccupancy>,
    mut notifications: MessageWriter<ShowNotification>,
    mut repaired_notified: Local<bool>,
) {
    let dt = time.delta_secs();

    // Build per-room repair power from repairing crew (flat 1.0 per crew)
    let mut room_repair_power: std::collections::HashMap<usize, f32> = std::collections::HashMap::new();
    for (crew, location) in crew_query.iter() {
        if crew.state != CrewState::Repairing || crew.health <= 0.0 {
            continue;
        }
        if let Some(room_id) = location.room_id {
            *room_repair_power.entry(room_id).or_insert(0.0) += 1.0;
        }
    }

    // Build per-room RepairBay boost
    let mut room_repair_boost: std::collections::HashMap<usize, f32> = std::collections::HashMap::new();
    for (module, repair_sys) in repair_bays.iter() {
        if let Some(&room_id) = room_map.tile_to_room.get(&module.grid_position) {
            let boost = room_repair_boost.entry(room_id).or_insert(0.0);
            *boost += repair_sys.repair_rate;
        }
    }

    let mut any_repaired = false;

    // Repair hull segments room-by-room
    for (room_id, crew_power) in room_repair_power.iter() {
        let boost = room_repair_boost.get(room_id).copied().unwrap_or(0.0);
        let total_power = crew_power + boost;

        if let Some(room) = room_map.rooms.get(*room_id) {
            for &tile in &room.tiles {
                // Find hull segments at this tile via occupancy
                if let Some(&entity) = occupancy.cells.get(&tile) {
                    if let Ok(mut hull) = hull_query.get_mut(entity) {
                        if hull.is_depressurized && hull.depressurization_level > 0.0 {
                            let repair_rate = total_power * 0.05 * dt;
                            hull.depressurization_level = (hull.depressurization_level - repair_rate).max(0.0);
                            if hull.depressurization_level <= 0.0 {
                                hull.is_depressurized = false;
                                any_repaired = true;
                            }
                        }
                        // Repair hull health if damaged and not depressurized
                        if hull.health < hull.max_health && !hull.is_depressurized {
                            let heal_rate = total_power * 2.0 * dt;
                            hull.health = (hull.health + heal_rate).min(hull.max_health);
                        }
                    }
                }
            }
        }
    }

    // RepairBay passive module repair (even without crew, repair_rate * dt)
    for (bay_module, repair_sys) in repair_bays.iter() {
        if let Some(&room_id) = room_map.tile_to_room.get(&bay_module.grid_position) {
            if let Some(room) = room_map.rooms.get(room_id) {
                for &tile in &room.tiles {
                    if let Some(&entity) = occupancy.cells.get(&tile) {
                        if let Ok(mut module) = module_query.get_mut(entity) {
                            if module.health < module.max_health && module.health > 0.0 {
                                module.health = (module.health + repair_sys.repair_rate * dt).min(module.max_health);
                            }
                        }
                    }
                }
            }
        }
    }

    if any_repaired && !*repaired_notified {
        *repaired_notified = true;
        notifications.write(ShowNotification {
            message: "Crew repaired a hull breach!".into(),
            notification_type: NotificationType::Success,
            duration: 3.0,
        });
    }
    if !any_repaired {
        *repaired_notified = false;
    }
}

/// Handles crew death events - despawn and update roster.
/// Also clears any CrewStation assignments for the dead crew.
fn handle_crew_death(
    mut commands: Commands,
    mut death_events: MessageReader<CrewDied>,
    mut roster: ResMut<CrewRoster>,
    mut statistics: ResMut<Statistics>,
    mut notifications: MessageWriter<ShowNotification>,
    mut station_query: Query<&mut CrewStation>,
) {
    for event in death_events.read() {
        roster.members.retain(|&e| e != event.crew);
        statistics.crew_lost += 1;

        // Clear station assignments for this crew
        for mut station in station_query.iter_mut() {
            if station.assigned_crew == Some(event.crew) {
                station.assigned_crew = None;
            }
        }

        notifications.write(ShowNotification {
            message: format!("{} has died! Cause: {:?}", event.name, event.cause),
            notification_type: NotificationType::Danger,
            duration: 4.0,
        });

        commands.entity(event.crew).despawn();
    }
}

// ============================================================================
// CREW FACILITY SYSTEMS (Phase 7)
// ============================================================================

/// MedBay heals crew in the same room. Heal rate = 10 HP/s * efficiency.
fn medbay_healing(
    time: Res<Time>,
    facility_query: Query<(&CrewFacility, &Module, Option<&ModuleEfficiency>)>,
    room_map: Res<RoomMap>,
    mut crew_query: Query<(&mut CrewMember, &CrewRoomLocation)>,
) {
    let dt = time.delta_secs();

    for (facility, module, eff) in facility_query.iter() {
        if facility.facility_type != FacilityType::MedBay || !module.is_active {
            continue;
        }

        let efficiency = effective_efficiency(module, eff);
        if efficiency <= 0.0 { continue; }

        let Some(&room_id) = room_map.tile_to_room.get(&module.grid_position) else {
            continue;
        };

        let heal_rate = 10.0 * efficiency * dt;

        for (mut crew, location) in crew_query.iter_mut() {
            if crew.health <= 0.0 || crew.health >= crew.max_health {
                continue;
            }
            if location.room_id == Some(room_id) {
                crew.health = (crew.health + heal_rate).min(crew.max_health);
            }
        }
    }
}

/// Active MessHall gives +2 morale/s to all crew (global, passive).
fn messhall_morale(
    time: Res<Time>,
    facility_query: Query<(&CrewFacility, &Module)>,
    mut crew_query: Query<&mut CrewMember>,
) {
    let dt = time.delta_secs();

    let has_active_messhall = facility_query.iter().any(|(f, m)| {
        f.facility_type == FacilityType::MessHall && m.is_active && m.health > 0.0
    });

    if !has_active_messhall {
        return;
    }

    for mut crew in crew_query.iter_mut() {
        if crew.health > 0.0 {
            crew.morale = (crew.morale + 2.0 * dt).min(100.0);
        }
    }
}

/// Active RecRoom prevents crew morale from dropping below 30.
fn recroom_morale_floor(
    facility_query: Query<(&CrewFacility, &Module)>,
    mut crew_query: Query<&mut CrewMember>,
) {
    let has_active_recroom = facility_query.iter().any(|(f, m)| {
        f.facility_type == FacilityType::RecRoom && m.is_active && m.health > 0.0
    });

    if !has_active_recroom {
        return;
    }

    for mut crew in crew_query.iter_mut() {
        if crew.health > 0.0 && crew.morale < 30.0 {
            crew.morale = 30.0;
        }
    }
}

/// Active TrainingRoom gives +1 morale/s and raises the panic threshold from 20 to 10.
/// Trained crew hold it together longer under stress.
fn training_room_boost(
    time: Res<Time>,
    facility_query: Query<(&CrewFacility, &Module)>,
    mut crew_query: Query<&mut CrewMember>,
) {
    let dt = time.delta_secs();

    let has_active_training = facility_query.iter().any(|(f, m)| {
        f.facility_type == FacilityType::TrainingRoom && m.is_active && m.health > 0.0
    });

    if !has_active_training {
        return;
    }

    for mut crew in crew_query.iter_mut() {
        if crew.health > 0.0 {
            // Morale boost (half of MessHall rate)
            crew.morale = (crew.morale + 1.0 * dt).min(100.0);
            // Trained crew resist panic better: recover from panicking at lower morale
            if crew.state == CrewState::Panicking && crew.morale > 15.0 && crew.oxygen > 10.0 {
                crew.state = CrewState::Idle;
            }
        }
    }
}

/// Active EngineeringStation boosts repair rate of nearby modules (+25%).
/// When staffed, RepairBay and HullPatch modules within 3 cells get a repair speed bonus.
fn engineering_station_boost(
    time: Res<Time>,
    facility_query: Query<(&CrewFacility, &Module, Option<&ModuleEfficiency>), Without<RepairSystem>>,
    mut repair_query: Query<(&mut Module, &RepairSystem), Without<DestroyedModule>>,
) {
    let dt = time.delta_secs();

    // Collect active engineering station positions with their efficiency
    let stations: Vec<(IVec2, f32)> = facility_query.iter()
        .filter(|(f, m, _)| {
            f.facility_type == FacilityType::EngineeringStation && m.is_active && m.health > 0.0
        })
        .map(|(_, m, eff)| {
            let efficiency = effective_efficiency(m, eff);
            (m.grid_position, efficiency)
        })
        .collect();

    if stations.is_empty() {
        return;
    }

    // Boost nearby repair modules
    for (mut repair_module, repair_sys) in repair_query.iter_mut() {
        if !repair_module.is_active { continue; }

        for &(station_pos, efficiency) in &stations {
            let dist = (repair_module.grid_position - station_pos).as_vec2().length();
            if dist <= 3.0 {
                // +25% repair rate bonus scaled by efficiency
                let bonus_heal = repair_sys.repair_rate * 0.25 * efficiency * dt;
                // Apply to the repair module's own health as a small self-maintenance effect
                if repair_module.health < repair_module.max_health {
                    repair_module.health = (repair_module.health + bonus_heal).min(repair_module.max_health);
                }
            }
        }
    }
}

/// Finds crew members that aren't in the roster or parented to the ship
/// and fixes them. This handles crew hired at docking stations.
fn reconcile_hired_crew(
    mut commands: Commands,
    // EVA crew are deliberately un-parented — don't "fix" them mid-flight
    crew_query: Query<(Entity, Option<&ChildOf>), (With<CrewMember>, Without<EvaSalvaging>)>,
    ship_query: Query<Entity, With<Ship>>,
    mut roster: ResMut<CrewRoster>,
) {
    let Ok(ship) = ship_query.single() else { return };

    for (crew_entity, parent) in crew_query.iter() {
        // Add to roster if missing
        if !roster.members.contains(&crew_entity) {
            roster.members.push(crew_entity);
        }

        // Parent to ship if orphaned
        if parent.is_none() {
            commands.entity(crew_entity).insert(ChildOf(ship));
        }
    }
}
