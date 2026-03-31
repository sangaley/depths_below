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
    mut events: EventReader<ShowNotification>,
    time: Res<Time>,
) {
    for event in events.iter() {
        history.entries.push(NotificationEntry {
            message: event.message.clone(),
            notification_type: event.notification_type,
            timestamp: time.elapsed_seconds(),
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
    keyboard: Res<Input<KeyCode>>,
    existing: Query<Entity, With<NotificationLogWindow>>,
    history: Res<NotificationHistory>,
) {
    if !keyboard.just_pressed(KeyCode::L) {
        return;
    }

    if let Ok(entity) = existing.get_single() {
        commands.entity(entity).despawn_recursive();
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
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::ColumnReverse, // Newest at bottom
                overflow: Overflow::clip_y(),
                max_height: Val::Px(260.0),
                ..default()
            },
            ..default()
        },
        NotificationLogContent,
    )).id();

    // Populate with history (last 30 entries)
    let start = history.entries.len().saturating_sub(30);
    for entry in &history.entries[start..] {
        let color = match entry.notification_type {
            NotificationType::Info => Color::rgb(0.5, 0.7, 0.8),
            NotificationType::Warning => Color::rgb(0.8, 0.7, 0.3),
            NotificationType::Danger => Color::rgb(0.9, 0.3, 0.3),
            NotificationType::Success => Color::rgb(0.3, 0.8, 0.4),
        };

        let minutes = (entry.timestamp / 60.0) as u32;
        let seconds = (entry.timestamp % 60.0) as u32;

        let row = commands.spawn(
            NodeBundle {
                style: Style {
                    width: Val::Percent(100.0),
                    padding: UiRect::vertical(Val::Px(1.0)),
                    ..default()
                },
                ..default()
            },
        ).id();

        let text = commands.spawn(
            TextBundle::from_section(
                format!("[{:02}:{:02}] {}", minutes, seconds, entry.message),
                TextStyle {
                    font_size: 11.0,
                    color,
                    ..default()
                },
            ),
        ).id();

        commands.entity(row).add_child(text);
        commands.entity(scroll_area).add_child(row);
    }

    commands.entity(content).add_child(scroll_area);
}
