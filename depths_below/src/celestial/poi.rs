use bevy::prelude::*;
use rand::Rng;
use crate::components::*;
use crate::events::*;
use crate::resources::*;
use super::components::*;

// ============================================================================
// SPACE POINTS OF INTEREST
// Derelicts, anomalies, resource nodes, space stations.
// Spawned per star system during generation.
// ============================================================================

/// Types of space POIs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpacePoiType {
    DerelictShip,       // Lootable wreck
    AsteroidNode,       // Mineable resource deposit
    Anomaly,            // Strange readings — story trigger
    SpaceStation,       // Trading outpost
    DebrisField,        // Scattered salvage
    SignalSource,       // Distress signal or trap
}

impl SpacePoiType {
    /// The chunk-layer equivalent, for anything that speaks in `PoiType`.
    ///
    /// The two point-of-interest systems grew up separately and never met:
    /// contracts, discovery and the map all speak `PoiType`, which only ever
    /// existed on the chunk layer near the world origin, while `SpacePoi` is
    /// what actually exists in all thirty-one systems. So Explore contracts
    /// could only be completed within a few thousand units of Haven, and the
    /// expedition trail sends people the other way.
    pub fn as_poi_type(self) -> Option<PoiType> {
        match self {
            SpacePoiType::DerelictShip | SpacePoiType::DebrisField => Some(PoiType::Wreck),
            SpacePoiType::Anomaly | SpacePoiType::SignalSource => Some(PoiType::Ruins),
            SpacePoiType::SpaceStation => Some(PoiType::Settlement),
            // A rock is not a place.
            SpacePoiType::AsteroidNode => None,
        }
    }
}

/// Component marking a space POI
#[derive(Component)]
pub struct SpacePoi {
    pub poi_type: SpacePoiType,
    pub looted: bool,
    /// Whether the ship has been close enough to log it.
    ///
    /// Separate from `looted` on purpose: that one gates whether there is
    /// anything left to take, and reusing it to mean "seen" would have made
    /// every derelict in the galaxy unlootable the moment you flew past it.
    pub discovered: bool,
    pub name: String,
    pub loot_value: u32,
}

/// Component for mineable asteroids
#[derive(Component)]
pub struct MineableResource {
    pub resource_remaining: f32,
    pub resource_type: ResourceNodeType,
    pub extraction_rate: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum ResourceNodeType {
    MetalOre,
    RareCrystal,
    FuelDeposit,
    ExoticMatter,
}

/// Spawn POIs when a new star system is generated
pub fn spawn_system_pois(
    commands: &mut Commands,
    system_center: Vec2,
    system_id: u32,
    planet_positions: &[Vec2],
    rng: &mut impl Rng,
    danger_tier: f32,
) {
    // Which band of the log corpus this system is allowed to hold. Derelicts
    // and anomalies are the carriers: they exist in every system, unlike the
    // chunk-layer points of interest that logs used to hang on.
    let log_tier = crate::narrative::logs::tier_for_danger(danger_tier);
    // Derelict ships (1-3 per system)
    let derelict_count = rng.gen_range(1..=3);
    for i in 0..derelict_count {
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let dist = rng.gen_range(30_000.0..80_000.0);
        let pos = system_center + Vec2::new(angle.cos() * dist, angle.sin() * dist);

        let poi = commands.spawn((
            (Sprite {
                    color: Color::srgb(0.35, 0.30, 0.28),
                    custom_size: Some(Vec2::new(200.0, 80.0)),
                    ..default()
                }, Transform::from_xyz(pos.x, pos.y, -0.3)),
            SpacePoi {
                poi_type: SpacePoiType::DerelictShip,
                looted: false,
                discovered: false,
                name: format!("Derelict-{}-{}", system_id, i),
                loot_value: rng.gen_range(50..200),
            },
            StarSystemMember { system_id },
        )).id();
        // Not every hulk has something to read. Keyed by system and index so
        // the same derelict always carries the same entry across reloads.
        if rng.gen::<f32>() < 0.55 {
            let key = (system_id as u64) << 8 | i as u64;
            if let Some(entry) = crate::narrative::logs::pick_log(log_tier, key) {
                commands.entity(poi).insert(LogEntry {
                    title: entry.title.to_string(),
                    text: entry.text.to_string(),
                    depth_hint: 0.0,
                });
            }
        }
    }

    // Asteroid resource nodes used to be spawned separately here, clustered
    // near planets — now every decorative asteroid (spawning::spawn_asteroid_field,
    // called for every system including warp jumps) carries its own
    // MineableResource directly, so this duplicate/invisible-to-the-player
    // node type is gone. planet_positions is still used below (space station).

    // Anomaly (0-1 per system, rare)
    if rng.gen::<f32>() < 0.4 {
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let dist = rng.gen_range(50_000.0..100_000.0);
        let pos = system_center + Vec2::new(angle.cos() * dist, angle.sin() * dist);

        let poi = commands.spawn((
            (Sprite {
                    color: Color::srgba(0.5, 0.3, 0.8, 0.6),
                    custom_size: Some(Vec2::splat(300.0)),
                    ..default()
                }, Transform::from_xyz(pos.x, pos.y, -0.3)),
            SpacePoi {
                poi_type: SpacePoiType::Anomaly,
                looted: false,
                discovered: false,
                name: format!("Anomaly-{}", system_id),
                loot_value: rng.gen_range(100..500),
            },
            StarSystemMember { system_id },
        )).id();
        // The type comment has said "story trigger" since it was written and
        // nothing ever read it. An anomaly always carries a log, and always
        // the deepest band its system is allowed.
        let key = (system_id as u64) << 8 | 0xA1;
        if let Some(entry) = crate::narrative::logs::pick_log(log_tier, key) {
            commands.entity(poi).insert(LogEntry {
                title: entry.title.to_string(),
                text: entry.text.to_string(),
                depth_hint: 0.0,
            });
        }
    }

    // Space station (1 per system, near a planet)
    if let Some(planet_pos) = planet_positions.first() {
        let station_offset = Vec2::new(
            rng.gen_range(-8_000.0..8_000.0),
            rng.gen_range(-8_000.0..8_000.0),
        );
        let pos = *planet_pos + station_offset;

        commands.spawn((
            (Sprite {
                    color: Color::srgb(0.45, 0.50, 0.55),
                    custom_size: Some(Vec2::splat(150.0)),
                    ..default()
                }, Transform::from_xyz(pos.x, pos.y, -0.2)),
            SpacePoi {
                poi_type: SpacePoiType::SpaceStation,
                looted: false,
                discovered: false,
                name: format!("Station-{}", system_id),
                loot_value: 0,
            },
            StarSystemMember { system_id },
        ));
    }
}

/// Mining system: when ship is near a MineableResource and has a Mining Drill, extract resources
pub fn mining_system(
    time: Res<Time>,
    ship_query: Query<&Transform, With<Ship>>,
    drill_query: Query<&Module, Without<DestroyedModule>>,
    mut resource_query: Query<(&Transform, &mut MineableResource, &mut SpacePoi, Option<&CelestialBody>), Without<Ship>>,
    mut inventory: ResMut<Inventory>,
    mut notifications: MessageWriter<ShowNotification>,
    mut last_notify: Local<f32>,
) {
    let Ok(ship_transform) = ship_query.single() else { return };
    let ship_pos = ship_transform.translation.truncate();
    let dt = time.delta_secs();

    // Check if ship has active mining drill
    let has_drill = drill_query.iter()
        .any(|m| m.module_type == ModuleType::MiningDrill && m.is_active);
    if !has_drill { return; }

    *last_notify += dt;

    for (res_transform, mut resource, mut poi, body) in resource_query.iter_mut() {
        let dist = ship_pos.distance(res_transform.translation.truncate());

        // Mining range, measured ship root to asteroid center. Rocks are
        // solid now, so the reachable minimum is the rock's radius plus most
        // of the ship's own length — scale the range with the rock so big
        // asteroids stay mineable.
        let range = 1200.0 + body.map(|b| b.radius).unwrap_or(0.0);
        if dist > range || resource.resource_remaining <= 0.0 { continue; }

        let extracted = resource.extraction_rate * dt;
        resource.resource_remaining -= extracted;

        // Convert to inventory items
        let item = match resource.resource_type {
            ResourceNodeType::MetalOre => ItemType::ScrapMetal,
            ResourceNodeType::RareCrystal => ItemType::Crystal,
            ResourceNodeType::FuelDeposit => ItemType::FuelCell,
            ResourceNodeType::ExoticMatter => ItemType::RareAlloy,
        };

        // Add to inventory every ~2 seconds worth of extraction
        if resource.resource_remaining % 10.0 < extracted {
            inventory.add_item(item, 1);
        }

        // Notify periodically
        if *last_notify > 3.0 {
            *last_notify = 0.0;
            notifications.write(ShowNotification {
                message: format!("Mining {:?}... {:.0} remaining", resource.resource_type, resource.resource_remaining),
                notification_type: NotificationType::Info,
                duration: 2.0,
            });
        }

        if resource.resource_remaining <= 0.0 {
            poi.looted = true;
            notifications.write(ShowNotification {
                message: format!("{} depleted", poi.name),
                notification_type: NotificationType::Info,
                duration: 3.0,
            });
        }
    }
}

/// Loot derelict ships when close
pub fn loot_derelict_system(
    ship_query: Query<&Transform, With<Ship>>,
    mut poi_query: Query<(&Transform, &mut SpacePoi), Without<Ship>>,
    mut currency: ResMut<Currency>,
    mut notifications: MessageWriter<ShowNotification>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if !keyboard.just_pressed(KeyCode::KeyE) { return; }

    let Ok(ship_transform) = ship_query.single() else { return };
    let ship_pos = ship_transform.translation.truncate();

    for (poi_transform, mut poi) in poi_query.iter_mut() {
        if poi.looted { continue; }
        if !matches!(poi.poi_type, SpacePoiType::DerelictShip | SpacePoiType::Anomaly) { continue; }

        let dist = ship_pos.distance(poi_transform.translation.truncate());
        // Root-to-center; derelicts are solid now, so leave room for the
        // ship's own hull between root and contact point.
        if dist > 1400.0 { continue; }

        poi.looted = true;
        currency.credits += poi.loot_value;
        notifications.write(ShowNotification {
            message: format!("Looted {}! +{}c", poi.name, poi.loot_value),
            notification_type: NotificationType::Success,
            duration: 3.0,
        });
        return;
    }
}
