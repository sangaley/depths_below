use bevy::prelude::*;
use crate::components::{Ship, ShipPhysics, Velocity, Weapon};
use crate::events::{ShowNotification, NotificationType};
use crate::resources::{OxygenState, FuelState};
use crate::states::GameState;
use super::station_types::{station_type, station_type_name};

// ============================================================================
// HOME STATION — a physical base near the spawn point.
// Fly within range and press F to dock: the ship parks at the berth (origin)
// and the game returns to StationDocked, i.e. full build mode. This is the
// "stop and build" anchor the world was missing — before this, build mode
// was only reachable once, at game start.
// ============================================================================

#[derive(Component)]
pub struct HomeStation;

/// Remote resupply outposts scattered through the rings — fly close, press F,
/// and O2/fuel/ammo refill. No build mode (only Haven has a shipyard), but
/// they turn long expeditions from one-way trips into routes.
#[derive(Component)]
pub struct ResupplyOutpost;

/// Positions of the remote outposts, spread across different rings/directions
/// and out to increasing distance from Haven Station — a few close-in for
/// early expeditions, then waypoints reaching out toward the faction
/// territories (see ai_ship::components::faction_territories, 30k-350k out)
/// so long trips into hostile space have somewhere to resupply along the way.
pub const OUTPOST_POSITIONS: [Vec2; 12] = [
    Vec2::new(4200.0, -2800.0),
    Vec2::new(-5600.0, 3400.0),
    Vec2::new(-2400.0, -8800.0),
    Vec2::new(9500.0, 5200.0),
    Vec2::new(26000.0, 16000.0),
    Vec2::new(-31000.0, -17000.0),
    Vec2::new(48000.0, -34000.0),
    Vec2::new(-58000.0, 27000.0),
    Vec2::new(82000.0, -58000.0),
    Vec2::new(-105000.0, -68000.0),
    Vec2::new(135000.0, 88000.0),
    Vec2::new(-165000.0, -118000.0),
];

/// Marker for the HUD arrow that points to the nearest station/outpost.
#[derive(Component)]
pub struct BaseArrow;

/// World position of the station structure. The ship's build berth is at the
/// origin, so the station sits below-left of it — close enough to see from
/// spawn, far enough not to overlap the build grid. NOTE: must have y <= 0;
/// update_depth clamps the ship to y <= 0, so anything above the origin line
/// is unreachable.
pub const STATION_POS: Vec2 = Vec2::new(-700.0, -450.0);
const DOCK_RANGE: f32 = 400.0;
const OUTPOST_RANGE: f32 = 350.0;

/// Nearest station within docking/resupply range, as a contract-board index
/// (0 = Haven, 1..=N = OUTPOST_POSITIONS in order). None if the ship isn't
/// close enough to any station right now. Used to pick which station's
/// bounty board the mission board UI shows, and to gate claiming rewards.
pub fn nearest_station_index(pos: Vec2) -> Option<usize> {
    if pos.distance(STATION_POS) < DOCK_RANGE {
        return Some(0);
    }
    OUTPOST_POSITIONS.iter()
        .position(|p| pos.distance(*p) < OUTPOST_RANGE)
        .map(|i| i + 1)
}

/// Where the ship parks when docking (same as its initial spawn point).
const BERTH_POS: Vec2 = Vec2::new(0.0, -50.0);

/// Spawn the station structure once (guarded). Composite sprites in the same
/// flat style as block visuals — no external asset needed.
pub fn spawn_home_station(
    mut commands: Commands,
    existing: Query<(), With<HomeStation>>,
) {
    if !existing.is_empty() {
        return;
    }

    let root = commands.spawn((
        Transform::from_xyz(STATION_POS.x, STATION_POS.y, 0.05),
        Visibility::default(),
        HomeStation,
    )).id();

    let mut add = |size: Vec2, color: Color, pos: Vec3| {
        let child = commands.spawn((
            Sprite { color, custom_size: Some(size), ..default() },
            Transform::from_translation(pos),
        )).id();
        commands.entity(root).add_child(child);
    };

    // Central hub
    add(Vec2::new(220.0, 220.0), Color::srgb(0.16, 0.18, 0.26), Vec3::ZERO);
    add(Vec2::new(180.0, 180.0), Color::srgb(0.22, 0.25, 0.35), Vec3::new(0.0, 0.0, 0.01));
    // Four arms
    add(Vec2::new(360.0, 46.0), Color::srgb(0.20, 0.22, 0.30), Vec3::new(0.0, 0.0, 0.005));
    add(Vec2::new(46.0, 360.0), Color::srgb(0.20, 0.22, 0.30), Vec3::new(0.0, 0.0, 0.005));
    // Docking pads at the arm tips
    for (x, y) in [(190.0, 0.0), (-190.0, 0.0), (0.0, 190.0), (0.0, -190.0)] {
        add(Vec2::new(56.0, 56.0), Color::srgb(0.28, 0.32, 0.44), Vec3::new(x, y, 0.01));
        add(Vec2::new(30.0, 30.0), Color::srgb(0.85, 0.70, 0.25), Vec3::new(x, y, 0.02));
    }
    // Lit windows on the hub
    for (x, y) in [(-50.0, 40.0), (0.0, 40.0), (50.0, 40.0), (-50.0, -40.0), (0.0, -40.0), (50.0, -40.0)] {
        add(Vec2::new(14.0, 10.0), Color::srgb(0.95, 0.85, 0.45), Vec3::new(x, y, 0.02));
    }

    // Name label above the structure
    let label = commands.spawn((
        Text2d::new("HAVEN STATION"),
        TextFont { font_size: FontSize::Px(28.0), ..default() },
        TextColor(Color::srgba(0.7, 0.8, 1.0, 0.8)),
        Transform::from_xyz(0.0, 190.0, 0.03),
    )).id();
    commands.entity(root).add_child(label);

    // --- Remote resupply outposts: smaller single-pad structures ---
    for (i, pos) in OUTPOST_POSITIONS.iter().enumerate() {
        let outpost = commands.spawn((
            Transform::from_xyz(pos.x, pos.y, 0.05),
            Visibility::default(),
            ResupplyOutpost,
        )).id();

        let mut add_part = |size: Vec2, color: Color, offset: Vec3| {
            let child = commands.spawn((
                Sprite { color, custom_size: Some(size), ..default() },
                Transform::from_translation(offset),
            )).id();
            commands.entity(outpost).add_child(child);
        };

        add_part(Vec2::new(130.0, 130.0), Color::srgb(0.18, 0.20, 0.26), Vec3::ZERO);
        add_part(Vec2::new(100.0, 100.0), Color::srgb(0.24, 0.27, 0.35), Vec3::new(0.0, 0.0, 0.01));
        add_part(Vec2::new(44.0, 44.0), Color::srgb(0.85, 0.70, 0.25), Vec3::new(0.0, 0.0, 0.02));
        add_part(Vec2::new(200.0, 24.0), Color::srgb(0.20, 0.22, 0.30), Vec3::new(0.0, -80.0, 0.005));

        let type_name = station_type_name(station_type(i + 1));
        let outpost_label = commands.spawn((
            Text2d::new(format!("OUTPOST {} — {}", i + 1, type_name.to_uppercase())),
            TextFont { font_size: FontSize::Px(20.0), ..default() },
            TextColor(Color::srgba(0.7, 0.8, 1.0, 0.7)),
            Transform::from_xyz(0.0, 110.0, 0.03),
        )).id();
        commands.entity(outpost).add_child(outpost_label);
    }

    // --- Nearest-base arrow: floats near the ship, points home ---
    let arrow_root = commands.spawn((
        Transform::from_xyz(0.0, 0.0, 5.0),
        Visibility::Hidden,
        BaseArrow,
    )).id();
    let shaft = commands.spawn((
        Sprite { color: Color::srgba(0.5, 0.8, 1.0, 0.8), custom_size: Some(Vec2::new(34.0, 6.0)), ..default() },
        Transform::from_xyz(-8.0, 0.0, 0.0),
    )).id();
    let head = commands.spawn((
        Sprite { color: Color::srgba(0.6, 0.9, 1.0, 0.9), custom_size: Some(Vec2::new(14.0, 14.0)), ..default() },
        Transform {
            translation: Vec3::new(14.0, 0.0, 0.0),
            rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_4),
            ..default()
        },
    )).id();
    commands.entity(arrow_root).add_children(&[shaft, head]);
}

/// Point the arrow from the ship toward the nearest station/outpost; hidden
/// when already close to one.
pub fn update_base_arrow(
    ship_query: Query<&Transform, (With<Ship>, Without<BaseArrow>)>,
    mut arrow_query: Query<(&mut Transform, &mut Visibility), With<BaseArrow>>,
) {
    let Ok(ship_transform) = ship_query.single() else { return };
    let Ok((mut arrow_transform, mut vis)) = arrow_query.single_mut() else { return };
    let ship_pos = ship_transform.translation.truncate();

    let nearest = std::iter::once(STATION_POS)
        .chain(OUTPOST_POSITIONS.iter().copied())
        .min_by(|a, b| {
            ship_pos.distance_squared(*a)
                .partial_cmp(&ship_pos.distance_squared(*b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(STATION_POS);

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

/// Dock at remote outposts: fly within range, press F to open the docking
/// menu priced for this outpost (title, goods prices, and service discounts
/// all resolve from nearest_station_index). This used to be a one-keypress
/// bundle that sold ALL cargo instantly and force-resupplied — fine before
/// per-item selling existed, but it dumped goods the player was routing to
/// a better-paying station without asking, and never opened a menu at the
/// very structures the trade routes run between. The menu covers everything
/// the bundle did, with choices.
pub fn outpost_docking(
    keyboard: Res<ButtonInput<KeyCode>>,
    ship_query: Query<&Transform, With<Ship>>,
    mut next_state: ResMut<NextState<GameState>>,
    mut notifications: MessageWriter<ShowNotification>,
    mut prompted: Local<bool>,
) {
    let Ok(ship_transform) = ship_query.single() else { return };
    let ship_pos = ship_transform.translation.truncate();

    let Some(outpost_i) = OUTPOST_POSITIONS.iter().position(|p| ship_pos.distance(*p) < OUTPOST_RANGE) else {
        *prompted = false;
        return;
    };
    let station_idx = outpost_i + 1;

    if !*prompted {
        *prompted = true;
        notifications.write(ShowNotification {
            message: format!(
                "Outpost {} ({}) in range — press F to dock & trade",
                station_idx, station_type_name(station_type(station_idx))
            ),
            notification_type: NotificationType::Info,
            duration: 4.0,
        });
    }

    if keyboard.just_pressed(KeyCode::KeyF) {
        next_state.set(GameState::Docked);
    }
}

/// While exploring: prompt when near the station, dock with F.
/// Docking parks the ship at the berth, kills its momentum, and re-enters
/// build mode (StationDocked).
pub fn home_station_docking(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut ship_query: Query<(Entity, &mut Transform, &mut Velocity, &mut ShipPhysics), With<Ship>>,
    mut weapon_query: Query<(&mut Weapon, &ChildOf)>,
    mut oxygen_state: ResMut<OxygenState>,
    mut fuel_state: ResMut<FuelState>,
    mut notifications: MessageWriter<ShowNotification>,
    mut next_state: ResMut<NextState<GameState>>,
    mut prompted: Local<bool>,
) {
    let Ok((ship_entity, mut transform, mut velocity, mut physics)) = ship_query.single_mut() else { return };
    let ship_pos = transform.translation.truncate();
    let dist = ship_pos.distance(STATION_POS);

    if dist > DOCK_RANGE {
        *prompted = false;
        return;
    }

    if !*prompted {
        *prompted = true;
        notifications.write(ShowNotification {
            message: "Haven Station in range — press F to dock and build".into(),
            notification_type: NotificationType::Info,
            duration: 4.0,
        });
    }

    if keyboard.just_pressed(KeyCode::KeyF) {
        transform.translation.x = BERTH_POS.x;
        transform.translation.y = BERTH_POS.y;
        // Square the ship up with the build grid — modules are placed in
        // unrotated grid space, so a tilted ship would misalign the ghost.
        transform.rotation = Quat::IDENTITY;
        physics.rotation = 0.0;
        velocity.0 = Vec2::ZERO;
        physics.angular_velocity = 0.0;
        physics.throttle = 0.0;

        // Home base resupplies everything — docking is safety.
        oxygen_state.current_oxygen = oxygen_state.max_oxygen;
        fuel_state.current_fuel = fuel_state.max_fuel;
        for (mut weapon, parent) in weapon_query.iter_mut() {
            if parent.parent() == ship_entity {
                weapon.ammo = weapon.max_ammo;
            }
        }

        notifications.write(ShowNotification {
            message: "Docked at Haven Station — O2 and fuel resupplied. B: build | Enter: launch".into(),
            notification_type: NotificationType::Success,
            duration: 5.0,
        });
        next_state.set(GameState::StationDocked);
    }
}
