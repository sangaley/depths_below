use bevy::prelude::*;
use crate::states::GameState;
use crate::resources::*;
use crate::events::*;
use crate::components::*;

mod generation;
mod chunks;
mod biomes;
pub mod home_base;
pub mod station_types;

#[allow(unused_imports)]
pub use generation::*;
pub use chunks::*;
pub use biomes::*;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<WorldState>()
            .init_resource::<ChunkManager>()
            .init_resource::<DiscoveredLocations>()
            // Home station structure exists from the very first (docked) state
            .add_systems(OnEnter(GameState::StationDocked), home_base::spawn_home_station)
            .add_systems(
                Update,
                (
                    update_chunks,
                    check_depth_zone_change,
                    update_biome,
                    check_poi_discovery,
                    salvage_wreck_system,
                    check_docking_proximity,
                    home_base::home_station_docking,
                    home_base::outpost_resupply,
                    home_base::update_base_arrow,
                    discover_log_entries,
                    apply_hazard_damage,
                )
                    .run_if(in_state(GameState::Exploring)),
            );
    }
}

/// Checks if player entered a new depth zone
fn check_depth_zone_change(
    ship_state: Res<DepthState>,
    mut last_zone: Local<Option<crate::components::ZoneType>>,
    mut zone_events: MessageWriter<DepthZoneChanged>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let current_zone = depth_to_zone(ship_state.current_depth);

    if Some(current_zone) != *last_zone {
        let first = last_zone.is_some();
        *last_zone = Some(current_zone);

        zone_events.write(DepthZoneChanged {
            new_depth: ship_state.current_depth,
            new_zone: current_zone,
        });

        if first {
            let zone_name = match current_zone {
                ZoneType::NearOrbit => "Near Orbit",
                ZoneType::AsteroidBelt => "Asteroid Belt",
                ZoneType::DeepSpace => "Deep Space",
                ZoneType::Nebula => "Nebula",
                ZoneType::BlackHole => "Black Hole Proximity",
            };
            notifications.write(ShowNotification {
                message: format!("Entering {}", zone_name),
                notification_type: NotificationType::Warning,
                duration: 3.0,
            });
        }
    }
}

fn depth_to_zone(depth: f32) -> crate::components::ZoneType {
    use crate::components::ZoneType;
    // Radial distance from Haven Station (origin). Thresholds sized for the
    // current cruise speeds — the old 200/500/1000/2000 were submarine depths
    // that a ship at full burn now crosses in a couple of seconds.
    match depth {
        d if d < 3000.0 => ZoneType::NearOrbit,
        d if d < 8000.0 => ZoneType::AsteroidBelt,
        d if d < 16000.0 => ZoneType::DeepSpace,
        d if d < 30000.0 => ZoneType::Nebula,
        _ => ZoneType::BlackHole,
    }
}

/// Updates current biome based on ship position
fn update_biome(
    ship_state: Res<DepthState>,
    ship_query: Query<&Transform, With<Ship>>,
    mut world_state: ResMut<WorldState>,
    mut notifications: MessageWriter<ShowNotification>,
    mut last_biome: Local<Option<BiomeType>>,
) {
    let Ok(ship_transform) = ship_query.single() else { return };

    let x = ship_transform.translation.x;
    let depth = ship_state.current_depth;

    // Determine biome from position and depth
    let biome = match depth {
        d if d < 200.0 => {
            if x.abs() > 2000.0 { BiomeType::AsteroidField } else { BiomeType::OpenVoid }
        }
        d if d < 500.0 => {
            if x > 1500.0 { BiomeType::CrystalFormation } else { BiomeType::OpenVoid }
        }
        d if d < 1000.0 => {
            if x < -1500.0 { BiomeType::IceShells } else { BiomeType::ThermalVents }
        }
        d if d < 2000.0 => BiomeType::DeadZone,
        _ => BiomeType::VoidRift,
    };

    if world_state.current_biome != biome {
        world_state.current_biome = biome;

        if last_biome.is_some() {
            notifications.write(ShowNotification {
                message: format!("Entered {:?} biome", biome),
                notification_type: NotificationType::Info,
                duration: 3.0,
            });
        }
        *last_biome = Some(biome);
    }
}

/// Discovers POIs when ship gets close
fn check_poi_discovery(
    ship_query: Query<&GlobalTransform, With<Ship>>,
    mut poi_query: Query<(&GlobalTransform, &mut PointOfInterest)>,
    mut discovered: ResMut<DiscoveredLocations>,
    mut poi_events: MessageWriter<PoiDiscovered>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let Ok(ship_gt) = ship_query.single() else { return };
    let ship_pos = ship_gt.translation().truncate();

    for (poi_gt, mut poi) in poi_query.iter_mut() {
        if poi.discovered {
            continue;
        }

        let poi_pos = poi_gt.translation().truncate();
        let dist = ship_pos.distance(poi_pos);

        if dist < 200.0 {
            poi.discovered = true;

            match poi.poi_type {
                PoiType::Wreck => discovered.wrecks.push(poi_pos),
                PoiType::Cave => discovered.caves.push(poi_pos),
                PoiType::Settlement => discovered.settlements.push(poi_pos),
                _ => discovered.special.push((poi_pos, format!("{:?}", poi.poi_type))),
            }

            poi_events.write(PoiDiscovered {
                poi_type: poi.poi_type,
                position: poi_pos,
            });

            notifications.write(ShowNotification {
                message: format!("Discovered {}!", poi.poi_type.display_name()),
                notification_type: NotificationType::Success,
                duration: 3.0,
            });
        }
    }
}

/// Check for docking proximity to settlements
fn check_docking_proximity(
    keyboard: Res<ButtonInput<KeyCode>>,
    ship_query: Query<&GlobalTransform, With<Ship>>,
    poi_query: Query<(Entity, &GlobalTransform, &PointOfInterest)>,
    mut docking_events: MessageWriter<DockingStarted>,
    mut notifications: MessageWriter<ShowNotification>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if !keyboard.just_pressed(KeyCode::KeyF) {
        return;
    }

    let Ok(ship_gt) = ship_query.single() else { return };
    let ship_pos = ship_gt.translation().truncate();

    for (entity, poi_gt, poi) in poi_query.iter() {
        if poi.poi_type != PoiType::Settlement {
            continue;
        }

        let dist = ship_pos.distance(poi_gt.translation().truncate());
        if dist < 150.0 {
            docking_events.write(DockingStarted { target: entity });
            notifications.write(ShowNotification {
                message: "Docking at settlement...".into(),
                notification_type: NotificationType::Info,
                duration: 2.0,
            });
            next_state.set(GameState::Docked);
            return;
        }
    }
}

/// Salvage loot from nearby wrecks with F key
fn salvage_wreck_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    ship_query: Query<&GlobalTransform, With<Ship>>,
    mut wreck_query: Query<(&GlobalTransform, &mut Wreck, &mut PointOfInterest)>,
    salvage_query: Query<&SalvageSystem, With<Module>>,
    mut inventory: ResMut<Inventory>,
    mut currency: ResMut<Currency>,
    mut statistics: ResMut<Statistics>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    if !keyboard.just_pressed(KeyCode::KeyF) {
        return;
    }

    let Ok(ship_gt) = ship_query.single() else { return };
    let ship_pos = ship_gt.translation().truncate();

    // Check if we have a salvage module (better range and yield)
    let has_salvage = salvage_query.iter().next().is_some();
    let salvage_range = if has_salvage { 150.0 } else { 80.0 };
    let items_per_salvage = if has_salvage { 2u32 } else { 1u32 };

    for (wreck_gt, mut wreck, mut poi) in wreck_query.iter_mut() {
        if poi.poi_type != PoiType::Wreck {
            continue;
        }

        let dist = ship_pos.distance(wreck_gt.translation().truncate());
        if dist > salvage_range {
            continue;
        }

        if wreck.loot_remaining == 0 {
            notifications.write(ShowNotification {
                message: "This wreck has been fully salvaged.".into(),
                notification_type: NotificationType::Info,
                duration: 2.0,
            });
            return;
        }

        let loot_types = [
            ItemType::ScrapMetal,
            ItemType::Crystal,
            ItemType::FuelCell,
            ItemType::RareAlloy,
            ItemType::AmmoCrate,
        ];

        for _ in 0..items_per_salvage {
            if wreck.loot_remaining == 0 {
                break;
            }

            let item = loot_types[rand::random::<usize>() % loot_types.len()];
            if inventory.add_item(item, 1) {
                wreck.loot_remaining -= 1;
                currency.credits += 15;

                notifications.write(ShowNotification {
                    message: format!("Salvaged {} (+15c)", item.name()),
                    notification_type: NotificationType::Success,
                    duration: 2.5,
                });
            } else {
                notifications.write(ShowNotification {
                    message: "Inventory full! Sell cargo at a settlement.".into(),
                    notification_type: NotificationType::Warning,
                    duration: 3.0,
                });
                break;
            }
        }

        if wreck.loot_remaining == 0 {
            poi.discovered = true;
            wreck.is_explored = true;
            statistics.wrecks_salvaged += 1;
            notifications.write(ShowNotification {
                message: "Wreck fully salvaged!".into(),
                notification_type: NotificationType::Info,
                duration: 3.0,
            });
        }

        return; // Only salvage one wreck per keypress
    }
}

/// Discover log entries when near POIs that have them
fn discover_log_entries(
    ship_query: Query<&GlobalTransform, With<Ship>>,
    log_query: Query<(&GlobalTransform, &LogEntry, &PointOfInterest), Without<Ship>>,
    mut statistics: ResMut<Statistics>,
    mut notifications: MessageWriter<ShowNotification>,
    mut discovered_logs: Local<Vec<String>>,
) {
    let Ok(ship_gt) = ship_query.single() else { return };
    let ship_pos = ship_gt.translation().truncate();

    for (poi_gt, log, _poi) in log_query.iter() {
        let poi_pos = poi_gt.translation().truncate();
        let dist = ship_pos.distance(poi_pos);

        if dist < 120.0 && !discovered_logs.contains(&log.title) {
            discovered_logs.push(log.title.clone());

            // Record in statistics
            if !statistics.logs_found.contains(&log.title) {
                statistics.logs_found.push(log.title.clone());
            }

            // Show the log entry as a long notification
            notifications.write(ShowNotification {
                message: format!("[LOG: {}] {}", log.title, log.text),
                notification_type: NotificationType::Info,
                duration: 8.0,
            });
        }
    }
}

/// Applies damage and forces from environmental hazard zones
fn apply_hazard_damage(
    time: Res<Time>,
    ship_query: Query<&GlobalTransform, With<Ship>>,
    hazard_query: Query<(&GlobalTransform, &HazardZone)>,
    mut damage_events: MessageWriter<ShipDamaged>,
    mut notifications: MessageWriter<ShowNotification>,
    mut warned_thermal: Local<bool>,
    mut warned_current: Local<bool>,
) {
    let Ok(ship_gt) = ship_query.single() else { return };
    let ship_pos = ship_gt.translation().truncate();

    for (hazard_gt, hazard) in hazard_query.iter() {
        let hazard_pos = hazard_gt.translation().truncate();
        let dist = ship_pos.distance(hazard_pos);

        if dist > hazard.radius {
            continue;
        }

        match &hazard.hazard_type {
            HazardType::ThermalVent => {
                let damage = hazard.damage_per_second * time.delta_secs();
                if damage > 0.01 {
                    damage_events.write(ShipDamaged {
                        source: DamageSource::Fire,
                        amount: damage,
                        position: Some(hazard_pos),
                        direction: Some((hazard_pos - ship_pos).normalize_or_zero()),
                    });
                }

                if !*warned_thermal {
                    *warned_thermal = true;
                    notifications.write(ShowNotification {
                        message: "Thermal vent! Hull taking heat damage!".into(),
                        notification_type: NotificationType::Danger,
                        duration: 3.0,
                    });
                }
            }
            HazardType::StrongCurrent(_direction) => {
                // Strong currents don't damage, they apply force
                // (Movement would be affected externally; for now just warn)
                if !*warned_current {
                    *warned_current = true;
                    notifications.write(ShowNotification {
                        message: "Strong current! Navigation affected!".into(),
                        notification_type: NotificationType::Warning,
                        duration: 3.0,
                    });
                }
            }
        }
    }

    // Reset warnings when ship moves away from all hazards
    let near_any = hazard_query.iter().any(|(gt, hz)| {
        ship_pos.distance(gt.translation().truncate()) < hz.radius
    });
    if !near_any {
        *warned_thermal = false;
        *warned_current = false;
    }
}
