use bevy::prelude::*;
use crate::events::{ShowNotification, NotificationType};
use super::framework::*;

// ============================================================================
// NOTIFICATION LOG — scrollable history of all game notifications
// ============================================================================

#[derive(Component)]
pub struct NotificationLogWindow;

#[derive(Component)]
pub struct NotificationLogContent;

/// Stores notification history
#[derive(Resource)]
pub struct NotificationHistory {
    pub entries: Vec<NotificationEntry>,
    pub max_entries: usize,
}

pub struct NotificationEntry {
    pub message: String,
    pub notification_type: NotificationType,
    pub timestamp: f32,
}

impl Default for NotificationHistory {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            max_entries: 100,
        }
    }
}

/// Record notifications into history
pub fn record_notifications(
    mut history: ResMut<NotificationHistory>,
    mut events: MessageReader<ShowNotification>,
    time: Res<Time>,
) {
    for event in events.read() {
        history.entries.push(NotificationEntry {
            message: event.message.clone(),
            notification_type: event.notification_type,
            timestamp: time.elapsed_secs(),
        });

        // Trim to max
        while history.entries.len() > history.max_entries {
            history.entries.remove(0);
        }
    }
}

/// Toggle notification log with L key
pub fn toggle_notification_log(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<(Entity, &FloatingWindow)>,
    history: Res<NotificationHistory>,
) {
    if !keyboard.just_pressed(KeyCode::KeyL) {
        return;
    }

    // By the window root, as with the map: the marker sits on the content
    // node, and despawning only that left an empty framed window behind.
    if let Some((entity, _)) = windows.iter().find(|(_, w)| w.id == "notif_log") {
        commands.entity(entity).despawn();
        return;
    }

    let content = spawn_floating_window(
        &mut commands,
        "notif_log",
        "Notification Log",
        Vec2::new(350.0, 300.0),
        Vec2::new(900.0, 50.0),
    );

    // Find root window and mark it
    commands.entity(content).insert(NotificationLogWindow);

    // Scrollable content
    let scroll_area = commands.spawn((
        (Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::ColumnReverse, 
                overflow: Overflow::clip_y(),
                max_height: Val::Px(260.0),
                ..default()
            }),
        NotificationLogContent,
    )).id();

    // Populate with history (last 30 entries)
    let start = history.entries.len().saturating_sub(30);
    for entry in &history.entries[start..] {
        let color = match entry.notification_type {
            NotificationType::Info => Color::srgb(0.5, 0.7, 0.8),
            NotificationType::Warning => Color::srgb(0.8, 0.7, 0.3),
            NotificationType::Danger => Color::srgb(0.9, 0.3, 0.3),
            NotificationType::Success => Color::srgb(0.3, 0.8, 0.4),
        };

        let minutes = (entry.timestamp / 60.0) as u32;
        let seconds = (entry.timestamp % 60.0) as u32;

        let row = commands.spawn(
            (Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::vertical(Val::Px(1.0)),
                    ..default()
                }),
        ).id();

        let text = commands.spawn(
            (Text::new(format!("[{:02}:{:02}] {}", minutes, seconds, entry.message)), TextFont { font_size: FontSize::Px(11.0), ..default() }, TextColor(color)),
        ).id();

        commands.entity(row).add_child(text);
        commands.entity(scroll_area).add_child(row);
    }

    commands.entity(content).add_child(scroll_area);
}

#[cfg(test)]
mod toggle_tests {
    use super::*;

    #[test]
    fn l_opens_the_log_and_l_closes_all_of_it() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<NotificationHistory>();
        app.add_systems(Update, toggle_notification_log);
        let press = |app: &mut App| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::KeyL);
            keys.clear();
            keys.press(KeyCode::KeyL);
            app.update();
        };
        let nodes = |app: &mut App| app.world_mut().query::<&Node>().iter(app.world()).count();

        press(&mut app);
        assert!(nodes(&mut app) > 0, "L did not open the log");
        press(&mut app);
        assert_eq!(nodes(&mut app), 0, "the window frame outlived the close");
    }
}
