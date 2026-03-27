use bevy::prelude::*;
use crate::states::GameState;
use crate::resources::*;
use crate::events::*;
use crate::components::*;

mod generation;
mod chunks;
mod biomes;

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
            .add_systems(
                Update,
                (
                    update_chunks,
                    check_depth_zone_change,
                    update_biome,
                    check_poi_discovery,
                    salvage_wreck_system,
                    check_docking_proximity,
                    discover_log_entries,
                    apply_hazard_damage,
                )
                    .run_if(in_state(GameState::Exploring)),
            );
    }
}

/// Checks if player entered a new depth zone
fn check_depth_zone_change(
    submarine_state: Res<DepthState>,
    mut last_zone: Local<Option<crate::components::ZoneType>>,
    mut zone_events: EventWriter<DepthZoneChanged>,
    mut notifications: EventWriter<ShowNotification>,
) {
    let current_zone = depth_to_zone(submarine_state.current_depth);

    if Some(current_zone) != *last_zone {
        let first = last_zone.is_some();
        *last_zone = Some(current_zone);

        zone_events.send(DepthZoneChanged {
            new_depth: submarine_state.current_depth,
            new_zone: current_zone,
        });

        if first {
            let zone_name = match current_zone {
                ZoneType::Light => "Light Zone",
                ZoneType::Twilight => "Twilight Zone",
                ZoneType::Dark => "Dark Zone",
                ZoneType::Abyss => "The Abyss",
                ZoneType::Trench => "The Trench",
            };
            notifications.send(ShowNotification {
                message: format!("Entering {}", zone_name),
                notification_type: NotificationType::Warning,
                duration: 3.0,
            });
        }
    }
}

fn depth_to_zone(depth: f32) -> crate::components::ZoneType {
    use crate::components::ZoneType;
    match depth {
        d if d < 200.0 => ZoneType::Light,
        d if d < 500.0 => ZoneType::Twilight,
        d if d < 1000.0 => ZoneType::Dark,
        d if d < 2000.0 => ZoneType::Abyss,
        _ => ZoneType::Trench,
    }
}

/// Updates current biome based on submarine position
fn update_biome(
    sub_state: Res<DepthState>,
    sub_query: Query<&Transform, With<Submarine>>,
    mut world_state: ResMut<WorldState>,
    mut notifications: EventWriter<ShowNotification>,
    mut last_biome: Local<Option<BiomeType>>,
) {
    let Ok(sub_transform) = sub_query.get_single() else { return };

    let x = sub_transform.translation.x;
    let depth = sub_state.current_depth;

    // Determine biome from position and depth
    let biome = match depth {
        d if d < 200.0 => {
            if x.abs() > 2000.0 { BiomeType::KelpForest } else { BiomeType::OpenOcean }
        }
        d if d < 500.0 => {
            if x > 1500.0 { BiomeType::CoralReef } else { BiomeType::OpenOcean }
        }
        d if d < 1000.0 => {
            if x < -1500.0 { BiomeType::IceCaverns } else { BiomeType::ThermalVents }
        }
        d if d < 2000.0 => BiomeType::AbyssalPlain,
        _ => BiomeType::DeepTrench,
    };

    if world_state.current_biome != biome {
        world_state.current_biome = biome;

        if last_biome.is_some() {
            notifications.send(ShowNotification {
                message: format!("Entered {:?} biome", biome),
                notification_type: NotificationType::Info,
                duration: 3.0,
            });
        }
        *last_biome = Some(biome);
    }
}

/// Discovers POIs when submarine gets close
fn check_poi_discovery(
    sub_query: Query<&GlobalTransform, With<Submarine>>,
    mut poi_query: Query<(&GlobalTransform, &mut PointOfInterest)>,
    mut discovered: ResMut<DiscoveredLocations>,
    mut poi_events: EventWriter<PoiDiscovered>,
    mut notifications: EventWriter<ShowNotification>,
) {
    let Ok(sub_gt) = sub_query.get_single() else { return };
    let sub_pos = sub_gt.translation().truncate();

    for (poi_gt, mut poi) in poi_query.iter_mut() {
        if poi.discovered {
            continue;
        }

        let poi_pos = poi_gt.translation().truncate();
        let dist = sub_pos.distance(poi_pos);

        if dist < 200.0 {
            poi.discovered = true;

            match poi.poi_type {
                PoiType::Wreck => discovered.wrecks.push(poi_pos),
                PoiType::Cave => discovered.caves.push(poi_pos),
                PoiType::Settlement => discovered.settlements.push(poi_pos),
                _ => discovered.special.push((poi_pos, format!("{:?}", poi.poi_type))),
            }

            poi_events.send(PoiDiscovered {
                poi_type: poi.poi_type,
                position: poi_pos,
            });

            notifications.send(ShowNotification {
                message: format!("Discovered {:?}!", poi.poi_type),
                notification_type: NotificationType::Success,
                duration: 3.0,
            });
        }
    }
}

/// Check for docking proximity to settlements
fn check_docking_proximity(
    keyboard: Res<Input<KeyCode>>,
    sub_query: Query<&GlobalTransform, With<Submarine>>,
    poi_query: Query<(Entity, &GlobalTransform, &PointOfInterest)>,
    mut docking_events: EventWriter<DockingStarted>,
    mut notifications: EventWriter<ShowNotification>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if !keyboard.just_pressed(KeyCode::F) {
        return;
    }

    let Ok(sub_gt) = sub_query.get_single() else { return };
    let sub_pos = sub_gt.translation().truncate();

    for (entity, poi_gt, poi) in poi_query.iter() {
        if poi.poi_type != PoiType::Settlement {
            continue;
        }

        let dist = sub_pos.distance(poi_gt.translation().truncate());
        if dist < 150.0 {
            docking_events.send(DockingStarted { target: entity });
            notifications.send(ShowNotification {
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
    keyboard: Res<Input<KeyCode>>,
    sub_query: Query<&GlobalTransform, With<Submarine>>,
    mut wreck_query: Query<(&GlobalTransform, &mut Wreck, &mut PointOfInterest)>,
    salvage_query: Query<&SalvageSystem, With<Module>>,
    mut inventory: ResMut<Inventory>,
    mut currency: ResMut<Currency>,
    mut statistics: ResMut<Statistics>,
    mut notifications: EventWriter<ShowNotification>,
) {
    if !keyboard.just_pressed(KeyCode::F) {
        return;
    }

    let Ok(sub_gt) = sub_query.get_single() else { return };
    let sub_pos = sub_gt.translation().truncate();

    // Check if we have a salvage module (better range and yield)
    let has_salvage = salvage_query.iter().next().is_some();
    let salvage_range = if has_salvage { 150.0 } else { 80.0 };
    let items_per_salvage = if has_salvage { 2u32 } else { 1u32 };

    for (wreck_gt, mut wreck, mut poi) in wreck_query.iter_mut() {
        if poi.poi_type != PoiType::Wreck {
            continue;
        }

        let dist = sub_pos.distance(wreck_gt.translation().truncate());
        if dist > salvage_range {
            continue;
        }

        if wreck.loot_remaining == 0 {
            notifications.send(ShowNotification {
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

                notifications.send(ShowNotification {
                    message: format!("Salvaged {} (+15c)", item.name()),
                    notification_type: NotificationType::Success,
                    duration: 2.5,
                });
            } else {
                notifications.send(ShowNotification {
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
            notifications.send(ShowNotification {
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
    sub_query: Query<&GlobalTransform, With<Submarine>>,
    log_query: Query<(&GlobalTransform, &LogEntry, &PointOfInterest), Without<Submarine>>,
    mut statistics: ResMut<Statistics>,
    mut notifications: EventWriter<ShowNotification>,
    mut discovered_logs: Local<Vec<String>>,
) {
    let Ok(sub_gt) = sub_query.get_single() else { return };
    let sub_pos = sub_gt.translation().truncate();

    for (poi_gt, log, _poi) in log_query.iter() {
        let poi_pos = poi_gt.translation().truncate();
        let dist = sub_pos.distance(poi_pos);

        if dist < 120.0 && !discovered_logs.contains(&log.title) {
            discovered_logs.push(log.title.clone());

            // Record in statistics
            if !statistics.logs_found.contains(&log.title) {
                statistics.logs_found.push(log.title.clone());
            }

            // Show the log entry as a long notification
            notifications.send(ShowNotification {
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
    sub_query: Query<&GlobalTransform, With<Submarine>>,
    hazard_query: Query<(&GlobalTransform, &HazardZone)>,
    mut damage_events: EventWriter<SubmarineDamaged>,
    mut notifications: EventWriter<ShowNotification>,
    mut warned_thermal: Local<bool>,
    mut warned_current: Local<bool>,
) {
    let Ok(sub_gt) = sub_query.get_single() else { return };
    let sub_pos = sub_gt.translation().truncate();

    for (hazard_gt, hazard) in hazard_query.iter() {
        let hazard_pos = hazard_gt.translation().truncate();
        let dist = sub_pos.distance(hazard_pos);

        if dist > hazard.radius {
            continue;
        }

        match &hazard.hazard_type {
            HazardType::ThermalVent => {
                let damage = hazard.damage_per_second * time.delta_seconds();
                if damage > 0.01 {
                    damage_events.send(SubmarineDamaged {
                        source: DamageSource::Fire,
                        amount: damage,
                        position: Some(hazard_pos),
                        direction: Some((hazard_pos - sub_pos).normalize_or_zero()),
                    });
                }

                if !*warned_thermal {
                    *warned_thermal = true;
                    notifications.send(ShowNotification {
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
                    notifications.send(ShowNotification {
                        message: "Strong current! Navigation affected!".into(),
                        notification_type: NotificationType::Warning,
                        duration: 3.0,
                    });
                }
            }
        }
    }

    // Reset warnings when sub moves away from all hazards
    let near_any = hazard_query.iter().any(|(gt, hz)| {
        sub_pos.distance(gt.translation().truncate()) < hz.radius
    });
    if !near_any {
        *warned_thermal = false;
        *warned_current = false;
    }
}
