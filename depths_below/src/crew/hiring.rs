use bevy::prelude::*;
use rand::Rng;

use crate::components::*;
use crate::events::*;
use crate::resources::*;
use crate::states::GameState;
use crate::world::home_base;

// ============================================================================
// HIRING BOARD — station recruitment panel, same idiom as the contracts
// mission board (J). Quarters modules auto-fill their bunks when BUILT,
// so this board's role is replacing casualties: deaths leave empty
// bunks, and this is where you fill them for credits without buying
// another barracks. H near/at a station to open.
// ============================================================================

pub struct Candidate {
    pub name: String,
    pub role: &'static str,
    pub cost: u32,
}

#[derive(Resource, Default)]
pub struct HiringBoardOpen(pub bool);

#[derive(Resource, Default)]
pub struct HiringSelection(pub usize);

#[derive(Resource, Default)]
pub struct HiringPool {
    pub station_idx: Option<usize>,
    pub candidates: Vec<Candidate>,
}

#[derive(Component)]
pub struct HiringPanel;

#[derive(Component)]
pub struct HiringContent;

const ROLES: [&str; 8] = [
    "ex-Navy gunner",
    "salvage diver",
    "void-born drifter",
    "deck engineer",
    "shipwright's apprentice",
    "asteroid prospector",
    "decommissioned marine",
    "freighter hand",
];

const NAMES: [&str; 16] = [
    "Vega", "Osei", "Lindqvist", "Aoki", "Mercer", "Duval", "Ramaswamy", "Kade",
    "Willow", "Stross", "Imani", "Costa", "Brun", "Ferro", "Solano", "Pike",
];

fn ensure_pool(pool: &mut HiringPool, station_idx: usize) {
    if pool.station_idx == Some(station_idx) && !pool.candidates.is_empty() {
        return;
    }
    let mut rng = rand::thread_rng();
    pool.station_idx = Some(station_idx);
    pool.candidates = (0..rng.gen_range(3..=5))
        .map(|_| Candidate {
            name: NAMES[rng.gen_range(0..NAMES.len())].to_string(),
            role: ROLES[rng.gen_range(0..ROLES.len())],
            cost: 150 + rng.gen_range(0..5) * 25,
        })
        .collect();
}

pub fn toggle_hiring_board(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut open: ResMut<HiringBoardOpen>,
    mut pool: ResMut<HiringPool>,
    mut selection: ResMut<HiringSelection>,
    existing: Query<Entity, With<HiringPanel>>,
    game_state: Res<State<GameState>>,
    ship_query: Query<&Transform, With<Ship>>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    if !keyboard.just_pressed(KeyCode::KeyH) {
        return;
    }

    if open.0 {
        open.0 = false;
        for entity in existing.iter() {
            commands.entity(entity).despawn();
        }
        return;
    }

    // Same reach rule as the mission board: docked at Haven = station 0,
    // otherwise whichever station is in range.
    let station = if *game_state.get() == GameState::StationDocked {
        Some(0)
    } else {
        ship_query.single().ok()
            .and_then(|t| home_base::nearest_station_index(t.translation.truncate()))
    };
    let Some(station) = station else {
        notifications.write(ShowNotification {
            message: "No station in range — recruits wait at stations.".into(),
            notification_type: NotificationType::Warning,
            duration: 3.0,
        });
        return;
    };

    ensure_pool(&mut pool, station);
    selection.0 = 0;
    open.0 = true;

    commands.spawn((
        (Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(20.0),
                top: Val::Percent(12.0),
                width: Val::Percent(60.0),
                height: Val::Percent(70.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(16.0)),
                ..default()
            }, BackgroundColor(Color::srgba(0.02, 0.05, 0.15, 0.95)), ZIndex(100)),
        HiringPanel,
    )).with_children(|parent| {
        parent.spawn((
            Text::new(format!("CREW FOR HIRE — STATION {}", station)),
            TextFont { font_size: FontSize::Px(28.0), ..default() },
            TextColor(Color::WHITE),
            Node { margin: UiRect::bottom(Val::Px(12.0)), ..default() },
        ));
        parent.spawn((
            (Node {
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::clip(),
                    flex_grow: 1.0,
                    ..default()
                }),
            HiringContent,
        ));
        parent.spawn((
            Text::new("Up/Down: Select | Enter: Hire | H: Close"),
            TextFont { font_size: FontSize::Px(14.0), ..default() },
            TextColor(Color::srgb(0.5, 0.5, 0.5)),
            Node { margin: UiRect::top(Val::Px(8.0)), ..default() },
        ));
    });
}

pub fn hiring_board_input(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    open: Res<HiringBoardOpen>,
    mut pool: ResMut<HiringPool>,
    mut selection: ResMut<HiringSelection>,
    mut currency: ResMut<Currency>,
    staffing: Res<StaffingState>,
    crew_query: Query<&CrewMember>,
    ship_query: Query<Entity, With<Ship>>,
    mut roster: ResMut<CrewRoster>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    if !open.0 || pool.candidates.is_empty() {
        return;
    }
    let count = pool.candidates.len();

    if keyboard.just_pressed(KeyCode::ArrowUp) {
        selection.0 = if selection.0 == 0 { count - 1 } else { selection.0 - 1 };
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) {
        selection.0 = (selection.0 + 1) % count;
    }
    if selection.0 >= count {
        selection.0 = 0;
    }

    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }

    let alive = crew_query.iter().filter(|c| c.health > 0.0).count() as u32;
    if alive >= staffing.total_berths {
        notifications.write(ShowNotification {
            message: "No empty bunks — build more quarters first.".into(),
            notification_type: NotificationType::Warning,
            duration: 3.0,
        });
        return;
    }
    let candidate = &pool.candidates[selection.0];
    if currency.credits < candidate.cost {
        notifications.write(ShowNotification {
            message: format!("Not enough credits (need {}c, have {}c)", candidate.cost, currency.credits),
            notification_type: NotificationType::Warning,
            duration: 2.5,
        });
        return;
    }
    let Ok(ship) = ship_query.single() else { return };

    let candidate = pool.candidates.remove(selection.0);
    currency.credits -= candidate.cost;

    let crew = commands
        .spawn((
            (
                Sprite {
                    color: Color::srgb(0.8, 0.6, 0.5),
                    custom_size: Some(Vec2::new(16.0, 16.0)),
                    ..default()
                },
                Transform::from_xyz(alive as f32 * 14.0 - 40.0, -20.0, 0.5),
            ),
            CrewMember {
                name: candidate.name.clone(),
                health: 100.0,
                max_health: 100.0,
                oxygen: 100.0,
                morale: 90.0,
                state: CrewState::Idle,
            },
        ))
        .insert(ChildOf(ship))
        .id();
    roster.members.push(crew);

    notifications.write(ShowNotification {
        message: format!(
            "{} ({}) signed on for {}c. ({}/{} berths)",
            candidate.name, candidate.role, candidate.cost, alive + 1, staffing.total_berths
        ),
        notification_type: NotificationType::Success,
        duration: 3.0,
    });
}

/// Rebuilds the candidate list whenever the pool or cursor changes.
pub fn update_hiring_display(
    mut commands: Commands,
    open: Res<HiringBoardOpen>,
    pool: Res<HiringPool>,
    selection: Res<HiringSelection>,
    staffing: Res<StaffingState>,
    crew_query: Query<&CrewMember>,
    content_query: Query<Entity, With<HiringContent>>,
) {
    if !open.0 {
        return;
    }
    if !(pool.is_changed() || selection.is_changed() || open.is_changed()) {
        return;
    }
    let Ok(content) = content_query.single() else { return };

    let alive = crew_query.iter().filter(|c| c.health > 0.0).count() as u32;
    commands.entity(content).despawn_related::<Children>();
    commands.entity(content).with_children(|parent| {
        parent.spawn((
            Text::new(format!("Bunks: {}/{}", alive, staffing.total_berths)),
            TextFont { font_size: FontSize::Px(16.0), ..default() },
            TextColor(if alive < staffing.total_berths {
                Color::srgb(0.5, 0.9, 0.5)
            } else {
                Color::srgb(0.8, 0.6, 0.3)
            }),
            Node { margin: UiRect::bottom(Val::Px(10.0)), ..default() },
        ));

        if pool.candidates.is_empty() {
            parent.spawn((
                Text::new("No one's looking for work here right now."),
                TextFont { font_size: FontSize::Px(16.0), ..default() },
                TextColor(Color::srgb(0.6, 0.6, 0.6)),
            ));
        }
        for (i, candidate) in pool.candidates.iter().enumerate() {
            let selected = i == selection.0;
            let cursor = if selected { "> " } else { "  " };
            parent.spawn((
                Text::new(format!("{}{} — {}  [{}c]", cursor, candidate.name, candidate.role, candidate.cost)),
                TextFont { font_size: FontSize::Px(18.0), ..default() },
                TextColor(if selected { Color::WHITE } else { Color::srgb(0.7, 0.7, 0.75) }),
                Node { margin: UiRect::bottom(Val::Px(4.0)), ..default() },
            ));
        }
    });
}
