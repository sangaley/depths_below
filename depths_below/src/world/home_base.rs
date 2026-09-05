use bevy::prelude::*;
use crate::components::{Ship, ShipPhysics, Velocity, Weapon};
use crate::events::{ShowNotification, NotificationType};
use crate::resources::{OxygenState, FuelState};
use crate::states::GameState;
use super::station_types::{StationType, station_type, station_type_name};

// ============================================================================
// STATIONS
// Every star system carries STATIONS_PER_SYSTEM full stations, spread wide
// around that system's own space. They are all "full" stations: dock with F
// and you get the shipyard (build mode), the shop, the bounty board and the
// hiring hall, exactly like Haven — the old model had thirteen stations
// crammed into Haven's system alone, twelve of them resupply-only blobs you
// could do nothing with but sell cargo, and every other system in the galaxy
// had none at all.
//
// Haven (system 0, slot 0) keeps its fixed position: it's where the ship
// spawns and where the berth is.
// ============================================================================

/// Marker for Haven specifically — the one station with a name of its own,
/// still used for its bigger collision hull and its "home" framing.
#[derive(Component)]
pub struct HomeStation;

/// Any station structure in the loaded system. `index` is the global station
/// index used for contract boards and pricing (see station_index).
#[derive(Component)]
pub struct Station {
    pub index: usize,
    pub system_id: u32,
}

/// Stations per star system. Two is deliberately sparse: a station is a
/// destination worth flying to, not scenery.
pub const STATIONS_PER_SYSTEM: usize = 2;

/// Upper bound on system ids the contract-board index space reserves room
/// for (galaxy::SYSTEM_COUNT is 30 today; boards are generated lazily, so
/// over-reserving costs an empty Vec entry each).
pub const MAX_SYSTEMS: usize = 64;

/// Size of the global station index space — see contracts::STATION_COUNT.
pub const TOTAL_STATION_SLOTS: usize = MAX_SYSTEMS * STATIONS_PER_SYSTEM;

/// Haven Station's fixed world position. The ship's build berth sits just
/// up-right of it, which is also where the ship spawns at game start.
pub const STATION_POS: Vec2 = Vec2::new(-700.0, -450.0);

/// Where a ship parks relative to the station it docks at.
const BERTH_OFFSET: Vec2 = Vec2::new(700.0, 400.0);

// Measured from the SHIP ROOT to the station center. Stations and hulls are
// solid (see ship::collision), and the root of a big ship can sit ~800 units
// behind its own nose — a tighter range is physically unreachable with a
// large hull parked against the station.
pub const DOCK_RANGE: f32 = 1800.0;

/// One station's identity and placement. Derived deterministically from the
/// system it belongs to, so it's identical every time that system loads and
/// needs no save data of its own.
#[derive(Clone, Debug)]
pub struct StationSite {
    /// Global index — contract board, prices, faction reputation.
    pub index: usize,
    pub system_id: u32,
    pub pos: Vec2,
    pub name: String,
    pub kind: StationType,
}

/// Global station index for a (system, slot) pair. Haven is 0.
pub fn station_index(system_id: u32, slot: usize) -> usize {
    system_id as usize * STATIONS_PER_SYSTEM + slot
}

/// Mirrors the names galaxy::generate_galaxy_map gives its systems, so a
/// station name can be derived from its index alone (no galaxy lookup).
pub fn system_display_name(system_id: u32) -> String {
    if system_id == 0 { "Haven".to_string() } else { format!("System-{:02}", system_id) }
}

/// Display name for a global station index.
pub fn station_display_name(index: usize) -> String {
    if index == 0 {
        return "Haven Station".to_string();
    }
    let system_id = (index / STATIONS_PER_SYSTEM) as u32;
    format!("{} {}", system_display_name(system_id), station_type_name(station_type(index)))
}

/// Deterministic station layout for one system. Stations are pushed far out
/// from the system center and away from each other — a golden-angle spread
/// keyed by (system, slot) means no two stations in a system share a
/// direction, and no two systems put theirs in the same relative spot.
pub fn station_sites(system_id: u32, local_center: Vec2) -> Vec<StationSite> {
    (0..STATIONS_PER_SYSTEM)
        .map(|slot| {
            let index = station_index(system_id, slot);
            let pos = if index == 0 {
                // Haven keeps its fixed spot: the ship spawns beside it.
                STATION_POS
            } else {
                let n = index as f32;
                let angle = n * 2.399963; // golden angle, radians
                // 180k-420k out: past the planets and the asteroid field, far
                // enough apart that two stations never share a screen.
                let radius = 180_000.0 + ((n * 0.6180339).fract()) * 240_000.0;
                local_center + Vec2::new(angle.cos(), angle.sin()) * radius
            };
            StationSite {
                index,
                system_id,
                pos,
                name: station_display_name(index),
                kind: station_type(index),
            }
        })
        .collect()
}

/// The loaded system's stations. Rebuilt whenever the streamed system
/// changes (see refresh_system_stations) so every consumer — docking, the
/// map, radar, the contract board — reads one list instead of each deriving
/// its own.
#[derive(Resource, Default)]
pub struct SystemStations {
    pub system_id: Option<u32>,
    pub sites: Vec<StationSite>,
}

impl SystemStations {
    /// Nearest station to `pos` within docking range, if any.
    pub fn nearest_in_range(&self, pos: Vec2) -> Option<&StationSite> {
        self.sites
            .iter()
            .filter(|s| pos.distance(s.pos) < DOCK_RANGE)
            .min_by(|a, b| {
                pos.distance_squared(a.pos)
                    .partial_cmp(&pos.distance_squared(b.pos))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Contract-board index of the station in docking range, if any. Used to
    /// pick which board the mission board shows and to gate claiming rewards.
    pub fn nearest_index(&self, pos: Vec2) -> Option<usize> {
        self.nearest_in_range(pos).map(|s| s.index)
    }

    /// Closest station regardless of range — what the HUD arrow points at.
    pub fn closest(&self, pos: Vec2) -> Option<&StationSite> {
        self.sites.iter().min_by(|a, b| {
            pos.distance_squared(a.pos)
                .partial_cmp(&pos.distance_squared(b.pos))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    pub fn positions(&self) -> impl Iterator<Item = Vec2> + '_ {
        self.sites.iter().map(|s| s.pos)
    }
}

/// Keeps SystemStations pointed at whichever system is currently streamed in.
/// Falls back to Haven before the galaxy has been generated (the game opens
/// docked at Haven, which is a frame or two before celestial's OnEnter
/// (Exploring) generation runs) so Haven Station is never missing.
pub fn refresh_system_stations(
    streaming: Res<crate::celestial::resources::SystemStreamingManager>,
    galaxy_map: Res<crate::celestial::resources::GalaxyMap>,
    mut stations: ResMut<SystemStations>,
) {
    let current = streaming.loaded_system.or(if galaxy_map.systems.is_empty() { Some(0) } else { None });

    if stations.system_id == current && !(current.is_some() && stations.sites.is_empty()) {
        return;
    }

    stations.system_id = current;
    stations.sites = match current {
        Some(id) => {
            let center = galaxy_map
                .systems
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.local_center)
                .unwrap_or(crate::celestial::galaxy::HAVEN_LOCAL_CENTER);
            station_sites(id, center)
        }
        // Blind-warped into empty space: no system, no stations.
        None => Vec::new(),
    };
}

/// Spawns/despawns station structures so the world always shows exactly the
/// loaded system's stations. Reconciling every frame (rather than hooking
/// system load/unload) keeps warp, game start and save-load on one path.
pub fn sync_station_entities(
    mut commands: Commands,
    stations: Res<SystemStations>,
    existing: Query<(Entity, &Station)>,
) {
    let mut present: Vec<usize> = Vec::new();
    for (entity, station) in existing.iter() {
        let still_here = stations.system_id == Some(station.system_id)
            && stations.sites.iter().any(|s| s.index == station.index);
        if still_here {
            present.push(station.index);
        } else {
            commands.entity(entity).despawn();
        }
    }

    for site in stations.sites.iter() {
        if !present.contains(&site.index) {
            spawn_station(&mut commands, site);
        }
    }
}

/// Builds one station structure. Every station is a real installation now, so
/// they all share Haven's silhouette; Haven itself is drawn larger, and the
/// accent color codes the station type.
fn spawn_station(commands: &mut Commands, site: &StationSite) {
    let is_haven = site.index == 0;
    let scale = if is_haven { 1.0 } else { 0.8 };
    let accent = station_accent(site.kind);

    let root = commands
        .spawn((
            Transform::from_xyz(site.pos.x, site.pos.y, 0.05),
            Visibility::default(),
            Station { index: site.index, system_id: site.system_id },
        ))
        .id();
    if is_haven {
        commands.entity(root).insert(HomeStation);
    }

    let mut add = |size: Vec2, color: Color, pos: Vec3| {
        let child = commands
            .spawn((
                Sprite { color, custom_size: Some(size * scale), ..default() },
                Transform::from_translation(Vec3::new(pos.x * scale, pos.y * scale, pos.z)),
            ))
            .id();
        commands.entity(root).add_child(child);
    };

    // Central hub
    add(Vec2::new(220.0, 220.0), Color::srgb(0.16, 0.18, 0.26), Vec3::ZERO);
    add(Vec2::new(180.0, 180.0), Color::srgb(0.22, 0.25, 0.35), Vec3::new(0.0, 0.0, 0.01));
    // Four arms
    add(Vec2::new(360.0, 46.0), Color::srgb(0.20, 0.22, 0.30), Vec3::new(0.0, 0.0, 0.005));
    add(Vec2::new(46.0, 360.0), Color::srgb(0.20, 0.22, 0.30), Vec3::new(0.0, 0.0, 0.005));
    // Docking pads at the arm tips, lit in the station type's accent
    for (x, y) in [(190.0, 0.0), (-190.0, 0.0), (0.0, 190.0), (0.0, -190.0)] {
        add(Vec2::new(56.0, 56.0), Color::srgb(0.28, 0.32, 0.44), Vec3::new(x, y, 0.01));
        add(Vec2::new(30.0, 30.0), accent, Vec3::new(x, y, 0.02));
    }
    // Lit windows on the hub
    for (x, y) in [(-50.0, 40.0), (0.0, 40.0), (50.0, 40.0), (-50.0, -40.0), (0.0, -40.0), (50.0, -40.0)] {
        add(Vec2::new(14.0, 10.0), Color::srgb(0.95, 0.85, 0.45), Vec3::new(x, y, 0.02));
    }

    // Name plate: station name over its type
    let label = commands
        .spawn((
            Text2d::new(site.name.to_uppercase()),
            TextFont { font_size: FontSize::Px(if is_haven { 28.0 } else { 24.0 }), ..default() },
            TextColor(Color::srgba(0.7, 0.8, 1.0, 0.8)),
            Transform::from_xyz(0.0, 190.0 * scale, 0.03),
        ))
        .id();
    commands.entity(root).add_child(label);

    if !is_haven {
        let kind_label = commands
            .spawn((
                Text2d::new(station_type_name(site.kind).to_uppercase()),
                TextFont { font_size: FontSize::Px(16.0), ..default() },
                TextColor(Color::srgba(0.6, 0.7, 0.9, 0.6)),
                Transform::from_xyz(0.0, 165.0 * scale, 0.03),
            ))
            .id();
        commands.entity(root).add_child(kind_label);
    }
}

/// Accent color per station type — the same coding the map legend uses.
pub fn station_accent(kind: StationType) -> Color {
    match kind {
        StationType::Shipyard => Color::srgb(0.85, 0.70, 0.25),
        StationType::MiningColony => Color::srgb(0.80, 0.45, 0.25),
        StationType::TradeHub => Color::srgb(0.35, 0.85, 0.45),
        StationType::MilitaryOutpost => Color::srgb(0.85, 0.30, 0.30),
        StationType::ResearchOutpost => Color::srgb(0.40, 0.65, 1.00),
        StationType::RefuelDepot => Color::srgb(0.55, 0.80, 0.85),
    }
}

/// Marker for the HUD arrow that points to the nearest station.
#[derive(Component)]
pub struct BaseArrow;

/// Spawns the nearest-station arrow once. (Stations themselves are spawned by
/// sync_station_entities, which follows whichever system is loaded.)
pub fn spawn_base_arrow(mut commands: Commands, existing: Query<(), With<BaseArrow>>) {
    if !existing.is_empty() {
        return;
    }

    let arrow_root = commands
        .spawn((Transform::from_xyz(0.0, 0.0, 5.0), Visibility::Hidden, BaseArrow))
        .id();
    let shaft = commands
        .spawn((
            Sprite { color: Color::srgba(0.5, 0.8, 1.0, 0.8), custom_size: Some(Vec2::new(34.0, 6.0)), ..default() },
            Transform::from_xyz(-8.0, 0.0, 0.0),
        ))
        .id();
    let head = commands
        .spawn((
            Sprite { color: Color::srgba(0.6, 0.9, 1.0, 0.9), custom_size: Some(Vec2::new(14.0, 14.0)), ..default() },
            Transform {
                translation: Vec3::new(14.0, 0.0, 0.0),
                rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_4),
                ..default()
            },
        ))
        .id();
    commands.entity(arrow_root).add_children(&[shaft, head]);
}

/// Point the arrow from the ship toward the nearest station; hidden when
/// already close to one (or when there's no station at all — blind space).
pub fn update_base_arrow(
    stations: Res<SystemStations>,
    ship_query: Query<&Transform, (With<Ship>, Without<BaseArrow>)>,
    mut arrow_query: Query<(&mut Transform, &mut Visibility), With<BaseArrow>>,
) {
    let Ok(ship_transform) = ship_query.single() else { return };
    let Ok((mut arrow_transform, mut vis)) = arrow_query.single_mut() else { return };
    let ship_pos = ship_transform.translation.truncate();

    let Some(nearest) = stations.closest(ship_pos).map(|s| s.pos) else {
        *vis = Visibility::Hidden;
        return;
    };

    let dist = ship_pos.distance(nearest);
    if dist < 600.0 {
        *vis = Visibility::Hidden;
        return;
    }
    *vis = Visibility::Visible;

    let dir = (nearest - ship_pos).normalize_or_zero();
    let orbit = ship_pos + dir * 150.0;
    arrow_transform.translation.x = orbit.x;
    arrow_transform.translation.y = orbit.y;
    arrow_transform.rotation = Quat::from_rotation_z(dir.y.atan2(dir.x));
}

/// Fly within range of any station and press F to dock: the ship parks at
/// that station's berth, momentum dies, supplies top up and the game enters
/// StationDocked — build mode, shop, bounty board, hiring, everywhere.
/// (Before, only Haven did this; the twelve outposts opened a sell-only trade
/// menu instead.)
pub fn station_docking(
    keyboard: Res<ButtonInput<KeyCode>>,
    stations: Res<SystemStations>,
    mut ship_query: Query<(Entity, &mut Transform, &mut Velocity, &mut ShipPhysics), With<Ship>>,
    mut weapon_query: Query<(&mut Weapon, &ChildOf)>,
    mut oxygen_state: ResMut<OxygenState>,
    mut fuel_state: ResMut<FuelState>,
    mut notifications: MessageWriter<ShowNotification>,
    mut next_state: ResMut<NextState<GameState>>,
    mut prompted_for: Local<Option<usize>>,
) {
    let Ok((ship_entity, mut transform, mut velocity, mut physics)) = ship_query.single_mut() else { return };
    let ship_pos = transform.translation.truncate();

    let Some(site) = stations.nearest_in_range(ship_pos) else {
        *prompted_for = None;
        return;
    };

    if *prompted_for != Some(site.index) {
        *prompted_for = Some(site.index);
        notifications.write(ShowNotification {
            message: format!(
                "{} ({}) in range — press F to dock",
                site.name,
                station_type_name(site.kind)
            ),
            notification_type: NotificationType::Info,
            duration: 4.0,
        });
    }

    if keyboard.just_pressed(KeyCode::KeyF) {
        let berth = site.pos + BERTH_OFFSET;
        transform.translation.x = berth.x;
        transform.translation.y = berth.y;
        // Square the ship up with the build grid — modules are placed in
        // unrotated grid space, so a tilted ship would misalign the ghost.
        transform.rotation = Quat::IDENTITY;
        physics.rotation = 0.0;
        velocity.0 = Vec2::ZERO;
        physics.angular_velocity = 0.0;
        physics.throttle = 0.0;

        // Docking is safety: everything tops up.
        oxygen_state.current_oxygen = oxygen_state.max_oxygen;
        fuel_state.current_fuel = fuel_state.max_fuel;
        for (mut weapon, parent) in weapon_query.iter_mut() {
            if parent.parent() == ship_entity {
                weapon.ammo = weapon.max_ammo;
            }
        }

        notifications.write(ShowNotification {
            message: format!(
                "Docked at {} — O2 and fuel resupplied. B: build | U: shop | J: jobs | Enter: launch",
                site.name
            ),
            notification_type: NotificationType::Success,
            duration: 5.0,
        });
        next_state.set(GameState::StationDocked);
    }
}
