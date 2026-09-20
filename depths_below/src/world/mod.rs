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
            .init_resource::<MarketEvents>()
            .init_resource::<home_base::SystemStations>()
            // Stations follow whichever system is streamed in, and have to
            // exist while docked too — the game opens docked at Haven.
            .add_systems(OnEnter(GameState::StationDocked), home_base::spawn_base_arrow)
            .add_systems(
                Update,
                (
                    home_base::refresh_system_stations,
                    home_base::sync_station_entities,
                )
                    .chain()
                    .run_if(in_state(GameState::Exploring).or_else(in_state(GameState::StationDocked))),
            )
            .add_systems(
                Update,
                (
                    update_chunks,
                    check_depth_zone_change,
                    update_biome,
                    tick_market_events,
                    // Both claim the shared F press (resources::InteractPress),
                    // so they must sit behind the salvage handler: crew on the
                    // hull, and a wreck nearer than the station, outrank
                    // docking. Without the ordering the winner would be
                    // whichever Bevy happened to schedule first.
                    check_docking_proximity
                        .after(crate::crew::eva_salvage::order_salvage_detail),
                    home_base::station_docking
                        .after(crate::crew::eva_salvage::order_salvage_detail),
                    home_base::update_base_arrow,
                    apply_hazard_damage,
                )
                    .run_if(in_state(GameState::Exploring)),
            )
            // Must run AFTER transform propagation. In Update, a point of
            // interest spawned this frame still has the default GlobalTransform
            // — the world origin — so every log-bearing wreck in the galaxy
            // read as sitting 50 units from the ship and was "discovered" on
            // the frame it spawned. That is why the player was handed a log
            // before touching a control, and it had nothing to do with where
            // the wreck actually was.
            .add_systems(
                PostUpdate,
                // Both of these compare GlobalTransform, so both must run
                // after propagation. check_poi_discovery was left behind in
                // Update when discover_log_entries was moved, and had exactly
                // the same bug: a point of interest spawned this frame still
                // carries the default GlobalTransform — the world origin —
                // and the ship starts 50 units from it, so every streamed POI
                // read as adjacent and was instantly "discovered". That meant
                // toast spam and contract objectives completing on their own.
                (discover_log_entries, check_poi_discovery)
                    .after(bevy::transform::TransformSystems::Propagate)
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

        // Root-to-center; the ship's own hull spans several hundred units,
        // so discovery triggers as the hull gets near, not once the root
        // is parked on top of the POI.
        if dist < 700.0 {
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
    mut commands: Commands,
    fx: Res<crate::vfx::effect_textures::EffectTextures>,
    mut press: ResMut<crate::resources::InteractPress>,
    ship_query: Query<&GlobalTransform, With<Ship>>,
    poi_query: Query<(Entity, &GlobalTransform, &PointOfInterest)>,
    mut docking_events: MessageWriter<DockingStarted>,
    mut notifications: MessageWriter<ShowNotification>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if !press.pending() {
        return;
    }

    let Ok(ship_gt) = ship_query.single() else { return };
    let ship_pos = ship_gt.translation().truncate();

    for (entity, poi_gt, poi) in poi_query.iter() {
        if poi.poi_type != PoiType::Settlement {
            continue;
        }

        let dist = ship_pos.distance(poi_gt.translation().truncate());
        if dist < 900.0 {
            // Claim only now that we know we're actually docking here.
            if !press.claim() {
                return;
            }
            crate::vfx::particles::spawn_dock_pulse(&mut commands, &fx, ship_pos, 180.0);
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

// NOTE: wreck looting moved to crew::eva_salvage — F now dispatches a
// crew salvage detail instead of teleporting loot into the hold.

/// MARKET EVENTS — every few minutes an outpost may develop a shortage
/// and pay well over the odds for one good for a while. Prices resolve
/// through resources::live_item_price, so the docking menu reflects the
/// premium automatically; expiry is silent (the notification names the
/// duration up front).
fn tick_market_events(
    time: Res<Time>,
    mut events: ResMut<MarketEvents>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    use rand::Rng;
    let dt = time.delta_secs();
    events.active.retain_mut(|e| {
        e.remaining -= dt;
        e.remaining > 0.0
    });

    events.next_roll -= dt;
    if events.next_roll > 0.0 {
        return;
    }
    let mut rng = rand::thread_rng();
    events.next_roll = rng.gen_range(180.0..300.0);
    if events.active.len() >= 2 {
        return;
    }

    let station_idx = rng.gen_range(1..=12usize);
    let goods = [
        ItemType::ScrapMetal,
        ItemType::Crystal,
        ItemType::BioSample,
        ItemType::FuelCell,
        ItemType::RareAlloy,
        ItemType::AncientArtifact,
        ItemType::AmmoCrate,
    ];
    let item = goods[rng.gen_range(0..goods.len())];
    let sell_mult = rng.gen_range(1.6..2.0_f32);
    let remaining = rng.gen_range(300.0..480.0_f32);

    let type_name = station_types::station_type_name(station_types::station_type(station_idx));
    notifications.write(ShowNotification {
        message: format!(
            "MARKET: Outpost {} ({}) short on {} - paying {:.0}% for ~{:.0} min!",
            station_idx,
            type_name,
            item.name(),
            sell_mult * 100.0,
            remaining / 60.0
        ),
        notification_type: NotificationType::Info,
        duration: 6.0,
    });
    events.active.push(MarketEvent { station_idx, item, sell_mult, remaining });
}

/// Discover log entries when near POIs that have them
fn discover_log_entries(
    ship_query: Query<&GlobalTransform, With<Ship>>,
    // Deliberately does NOT require PointOfInterest. That component only
    // exists on the chunk layer, which generates in a narrow band of world Y
    // around the origin — so requiring it meant a log could only ever be read
    // near Haven, and celestial derelicts out in the galaxy were invisible to
    // this system no matter what they carried.
    log_query: Query<(&GlobalTransform, &LogEntry), Without<Ship>>,
    mut statistics: ResMut<Statistics>,
    mut log_queue: ResMut<crate::narrative::reader::LogQueue>,
    mut finale: ResMut<crate::narrative::FinaleFound>,
) {
    let Ok(ship_gt) = ship_query.single() else { return };
    let ship_pos = ship_gt.translation().truncate();

    for (poi_gt, log) in log_query.iter() {
        let poi_pos = poi_gt.translation().truncate();
        let dist = ship_pos.distance(poi_pos);

        // Statistics.logs_found is the only record of what has been read.
        //
        // This used to dedupe against a system `Local` as well, which was a
        // quiet disaster: a Local outlives the run, and reset_for_new_game has
        // no way to clear one. So after a single New Expedition every log read
        // in the previous run was permanently unreadable — the finale
        // included, which made the ending unreachable on any second run until
        // the process was restarted. logs_found IS reset, so it is the right
        // and only source of truth.
        if dist < 500.0 && !statistics.logs_found.contains(&log.title) {
            statistics.logs_found.push(log.title.clone());

            // The ending keys on *finding* the finale, not on holding it, so
            // that loading a save from after the ending does not replay it.
            if log.title == crate::narrative::logs::FINALE_TITLE {
                finale.0 = true;
            }
            // Goes to the reader, not a toast. Several entries run past two
            // hundred characters and the toast is 340px wide with an eight
            // second life — they were unreadable by construction.
            log_queue.push(log.title.clone(), log.text.clone());
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
