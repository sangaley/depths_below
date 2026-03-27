use bevy::prelude::*;
use crate::states::GameState;
use crate::components::*;
use crate::resources::*;
use crate::events::*;
use crate::building::rooms::RoomMap;
use crate::building::GridOccupancy;

pub struct CrewPlugin;

impl Plugin for CrewPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<CrewRoster>()
            .init_resource::<StaffingState>()
            .init_resource::<AutoAssignTimer>()
            // Staffing / efficiency systems run at both SurfaceBase and Exploring
            // so the HUD shows correct crew/station counts at the surface.
            .add_systems(
                Update,
                (
                    compute_module_efficiency,
                    update_staffing_state,
                    auto_assign_crew,
                    reconcile_hired_crew,
                )
                    .run_if(in_state(GameState::Exploring)
                        .or_else(in_state(GameState::SurfaceBase))
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
            );
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

/// Spawns the initial crew (8 crew, no skills)
pub fn spawn_starter_crew(
    mut commands: Commands,
    submarine_query: Query<Entity, With<Submarine>>,
    existing_crew: Query<Entity, With<CrewMember>>,
    mut roster: ResMut<CrewRoster>,
) {
    // Guard: don't spawn duplicate crew
    if !existing_crew.is_empty() {
        return;
    }
    let Ok(submarine) = submarine_query.get_single() else {
        return;
    };

    let crew_names = ["Jones", "Smith", "Chen", "Morgan", "Rivera", "Volkov", "Tanaka", "Okafor"];

    for (i, name) in crew_names.iter().enumerate() {
        let crew = commands.spawn((
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
                name: name.to_string(),
                health: 100.0,
                max_health: 100.0,
                oxygen: 100.0,
                morale: 100.0,
                state: CrewState::Idle,
            },
        )).set_parent(submarine).id();

        roster.members.push(crew);
    }

    info!("Spawned {} crew members", crew_names.len());
}

/// Computes ModuleEfficiency for all modules with a CrewStation.
/// staffing_factor: 0.5 unstaffed, 1.0 staffed (crew alive & not panicking/unconscious).
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
                {
                    1.0
                } else {
                    // Dead/panicking/unconscious crew — clear assignment
                    station.assigned_crew = None;
                    0.5
                }
            } else {
                // Crew entity no longer exists — clear assignment
                station.assigned_crew = None;
                0.5
            }
        } else {
            0.5
        };

        commands.entity(entity).insert(ModuleEfficiency {
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
    mut station_query: Query<(Entity, &mut CrewStation)>,
    crew_query: Query<(Entity, &CrewMember)>,
) {
    timer.timer.tick(time.delta());
    if !timer.timer.just_finished() {
        return;
    }

    // Collect all crew currently assigned to any station
    let mut assigned_crew: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    for (_, station) in station_query.iter() {
        if let Some(crew_entity) = station.assigned_crew {
            assigned_crew.insert(crew_entity);
        }
    }

    // Clean up dead/despawned crew from stations
    for (_, mut station) in station_query.iter_mut() {
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
    let mut unfilled: Vec<(Entity, u8)> = Vec::new();
    for (entity, station) in station_query.iter() {
        if station.priority > 0 && !station.manually_assigned && station.assigned_crew.is_none() {
            unfilled.push((entity, station.priority));
        }
    }

    // Sort by priority descending
    unfilled.sort_by(|a, b| b.1.cmp(&a.1));

    // Collect available crew (alive, not panicking/unconscious, not assigned)
    let mut available_crew: Vec<Entity> = Vec::new();
    for (entity, crew) in crew_query.iter() {
        if crew.health > 0.0
            && crew.state != CrewState::Panicking
            && crew.state != CrewState::Unconscious
            && !assigned_crew.contains(&entity)
        {
            available_crew.push(entity);
        }
    }

    // Assign in order
    let mut crew_idx = 0;
    for (station_entity, _priority) in unfilled {
        if crew_idx >= available_crew.len() {
            break;
        }
        let crew_entity = available_crew[crew_idx];
        crew_idx += 1;

        if let Ok((_, mut station)) = station_query.get_mut(station_entity) {
            station.assigned_crew = Some(crew_entity);
        }
    }
}

/// Updates crew needs (oxygen, morale)
fn update_crew_needs(
    time: Res<Time>,
    oxygen_state: Res<OxygenState>,
    depth_state: Res<DepthState>,
    mut crew_query: Query<&mut CrewMember>,
) {
    let oxygen_available = oxygen_state.oxygen_balance >= 0.0;

    for mut crew in crew_query.iter_mut() {
        if crew.health <= 0.0 {
            continue;
        }

        if !oxygen_available {
            crew.oxygen = (crew.oxygen - 10.0 * time.delta_seconds()).max(0.0);
        } else {
            crew.oxygen = (crew.oxygen + 20.0 * time.delta_seconds()).min(100.0);
        }

        if crew.oxygen < 50.0 || depth_state.current_depth > 500.0 {
            crew.morale = (crew.morale - 5.0 * time.delta_seconds()).max(0.0);
        } else {
            crew.morale = (crew.morale + 1.0 * time.delta_seconds()).min(100.0);
        }
    }
}

/// Maps each crew member's world position to a grid position and room via RoomMap.
fn update_crew_room_location(
    mut commands: Commands,
    mut crew_query: Query<(Entity, &GlobalTransform, Option<&mut CrewRoomLocation>), With<CrewMember>>,
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
            commands.entity(entity).insert(CrewRoomLocation {
                room_id,
                grid_position: grid,
            });
        }
    }
}

/// Scans for rooms with flooding or fire and dispatches idle crew to handle emergencies.
/// Temporarily clears non-manual CrewStation assignments for dispatched crew.
fn crew_emergency_dispatch(
    mut crew_query: Query<(Entity, &mut CrewMember)>,
    fire_query: Query<&Module, With<OnFire>>,
    room_map: Res<RoomMap>,
    mut station_query: Query<(Entity, &mut CrewStation)>,
    mut dispatch_events: EventWriter<CrewDispatched>,
) {
    // Build priority list of emergency rooms: flooding first, then fire
    let mut emergency_rooms: Vec<(usize, DispatchReason)> = Vec::new();

    for room in room_map.rooms.iter() {
        if room.is_breached && room.water_level > 0.0 {
            emergency_rooms.push((room.id, DispatchReason::Flooding));
        }
    }

    // Check for rooms with fire
    for module in fire_query.iter() {
        if let Some(&room_id) = room_map.tile_to_room.get(&module.grid_position) {
            if !emergency_rooms.iter().any(|(id, _)| *id == room_id) {
                emergency_rooms.push((room_id, DispatchReason::Fire));
            }
        }
    }

    if emergency_rooms.is_empty() {
        return;
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

    // Dispatch idle crew to emergencies
    for (entity, mut crew) in crew_query.iter_mut() {
        if crew.health <= 0.0 || crew.state != CrewState::Idle {
            continue;
        }
        if let Some(&(room_id, reason)) = emergency_rooms.first() {
            crew.state = CrewState::Repairing;

            // Clear station assignment if not manually assigned
            if let Some(station_entity) = station_assignments.get(&entity) {
                if let Ok((_, mut station)) = station_query.get_mut(*station_entity) {
                    station.assigned_crew = None;
                }
            }

            dispatch_events.send(CrewDispatched {
                crew: entity,
                room_id,
                reason,
            });
        }
    }
}

/// Updates crew AI behavior — now aware of both floods and fires.
fn update_crew_ai(
    hull_query: Query<(Entity, &HullSegment, &Transform)>,
    fire_query: Query<Entity, With<OnFire>>,
    mut crew_query: Query<&mut CrewMember>,
) {
    let has_flooded = hull_query.iter().any(|(_, hull, _)| hull.is_flooded);
    let has_fires = !fire_query.is_empty();
    let has_danger = has_flooded || has_fires;

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
    crew_query: Query<(&CrewMember, &CrewRoomLocation)>,
    mut fire_query: Query<(Entity, &mut OnFire, &Module, &mut Sprite), Without<DestroyedModule>>,
    room_map: Res<RoomMap>,
    mut extinguish_events: EventWriter<FireExtinguished>,
) {
    let dt = time.delta_seconds();

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

    // Apply suppression to fires
    for (entity, mut fire, module, mut sprite) in fire_query.iter_mut() {
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
            sprite.color = Color::rgb(0.2, 0.2, 0.2);
            extinguish_events.send(FireExtinguished {
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
    mut crew_query: Query<(Entity, &mut CrewMember)>,
    mut damage_events: EventWriter<CrewDamaged>,
    mut death_events: EventWriter<CrewDied>,
) {
    for (entity, mut crew) in crew_query.iter_mut() {
        if crew.health <= 0.0 {
            continue;
        }

        if crew.oxygen <= 0.0 {
            let damage = config.suffocation_damage_rate * time.delta_seconds();
            crew.health -= damage;

            damage_events.send(CrewDamaged {
                crew: entity,
                amount: damage,
                source: CrewDamageSource::Suffocation,
            });

            if crew.health <= 0.0 {
                death_events.send(CrewDied {
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
    mut notifications: EventWriter<ShowNotification>,
    mut repaired_notified: Local<bool>,
) {
    let dt = time.delta_seconds();

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
                        if hull.is_flooded && hull.flood_level > 0.0 {
                            let repair_rate = total_power * 0.05 * dt;
                            hull.flood_level = (hull.flood_level - repair_rate).max(0.0);
                            if hull.flood_level <= 0.0 {
                                hull.is_flooded = false;
                                any_repaired = true;
                            }
                        }
                        // Repair hull health if damaged and not flooded
                        if hull.health < hull.max_health && !hull.is_flooded {
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
        notifications.send(ShowNotification {
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
    mut death_events: EventReader<CrewDied>,
    mut roster: ResMut<CrewRoster>,
    mut statistics: ResMut<Statistics>,
    mut notifications: EventWriter<ShowNotification>,
    mut station_query: Query<&mut CrewStation>,
) {
    for event in death_events.iter() {
        roster.members.retain(|&e| e != event.crew);
        statistics.crew_lost += 1;

        // Clear station assignments for this crew
        for mut station in station_query.iter_mut() {
            if station.assigned_crew == Some(event.crew) {
                station.assigned_crew = None;
            }
        }

        notifications.send(ShowNotification {
            message: format!("{} has died! Cause: {:?}", event.name, event.cause),
            notification_type: NotificationType::Danger,
            duration: 4.0,
        });

        commands.entity(event.crew).despawn_recursive();
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
    let dt = time.delta_seconds();

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
    let dt = time.delta_seconds();

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
    let dt = time.delta_seconds();

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
    let dt = time.delta_seconds();

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

/// Finds crew members that aren't in the roster or parented to the submarine
/// and fixes them. This handles crew hired at docking stations.
fn reconcile_hired_crew(
    mut commands: Commands,
    crew_query: Query<(Entity, Option<&Parent>), With<CrewMember>>,
    submarine_query: Query<Entity, With<Submarine>>,
    mut roster: ResMut<CrewRoster>,
) {
    let Ok(submarine) = submarine_query.get_single() else { return };

    for (crew_entity, parent) in crew_query.iter() {
        // Add to roster if missing
        if !roster.members.contains(&crew_entity) {
            roster.members.push(crew_entity);
        }

        // Parent to submarine if orphaned
        if parent.is_none() {
            commands.entity(crew_entity).set_parent(submarine);
        }
    }
}
