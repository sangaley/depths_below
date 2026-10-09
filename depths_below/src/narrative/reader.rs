//! A card you can actually read a log on.
//!
//! Logs used to arrive as an eight-second notification toast: 340px wide, body
//! text, capped at six on screen at once, and silently deduped if two said the
//! same thing within three seconds. Several entries run past two hundred
//! characters. They were being delivered in a widget that could not hold them
//! and then taken away before they could be finished.
//!
//! This is the tutorial card's shape, deliberately — same placement, same
//! dismiss idiom — because the game already taught the player that a centred
//! card near the top is something to read.

use bevy::prelude::*;

use crate::states::GameState;
use crate::ui::theme::{ThemeColors, ThemeFonts, ThemeSpacing};

/// Logs waiting to be read, oldest first.
///
/// A queue rather than a single slot: flying into a cluster of derelicts can
/// turn up two entries within a second of each other, and the toast path used
/// to drop the second one on the floor.
#[derive(Resource, Default)]
pub struct LogQueue {
    pub pending: Vec<(String, String)>,
}

impl LogQueue {
    pub fn push(&mut self, title: String, text: String) {
        self.pending.push((title, text));
    }
}

#[derive(Component)]
struct LogCardRoot;
#[derive(Component)]
struct LogCardTitle;
#[derive(Component)]
struct LogCardBody;
#[derive(Component)]
struct LogCardFooter;

fn spawn_log_card(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(110.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            // Above the tutorial card's 50: if training is still running when a
            // log turns up, the log is the thing that just happened.
            ZIndex(55),
            Visibility::Hidden,
            LogCardRoot,
        ))
        .with_children(|wrapper| {
            wrapper
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        width: Val::Px(520.0),
                        max_width: Val::Percent(92.0),
                        padding: UiRect::all(Val::Px(ThemeSpacing::LG)),
                        row_gap: Val::Px(ThemeSpacing::SM),
                        ..default()
                    },
                    BackgroundColor(ThemeColors::BG_CARD),
                ))
                .with_children(|card| {
                    card.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(2.0),
                            margin: UiRect::bottom(Val::Px(ThemeSpacing::XS)),
                            ..default()
                        },
                        BackgroundColor(ThemeColors::ACCENT_ORANGE),
                    ));
                    card.spawn((
                        Text::new("RECOVERED LOG"),
                        TextFont { font_size: FontSize::Px(ThemeFonts::CAPTION), ..default() },
                        TextColor(ThemeColors::ACCENT_ORANGE),
                    ));
                    card.spawn((
                        Text::new(""),
                        TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() },
                        TextColor(ThemeColors::TEXT_PRIMARY),
                        LogCardTitle,
                    ));
                    card.spawn((
                        Text::new(""),
                        TextFont { font_size: FontSize::Px(ThemeFonts::BODY), ..default() },
                        TextColor(ThemeColors::TEXT_SECONDARY),
                        LogCardBody,
                    ));
                    card.spawn((
                        Text::new("[Space] close"),
                        TextFont { font_size: FontSize::Px(ThemeFonts::CAPTION), ..default() },
                        TextColor(ThemeColors::TEXT_MUTED),
                        Node { margin: UiRect::top(Val::Px(ThemeSpacing::XS)), ..default() },
                        LogCardFooter,
                    ));
                });
        });
}

/// Shows the head of the queue, and pops it when the player closes it.
///
/// Space closes, not Escape — Escape pauses, and a reader that ate the pause
/// key would be worse than the toast it replaces.
#[allow(clippy::type_complexity)]
fn drive_log_card(
    mut queue: ResMut<LogQueue>,
    keys: Res<ButtonInput<KeyCode>>,
    mut root: Query<&mut Visibility, With<LogCardRoot>>,
    mut texts: ParamSet<(
        Query<&mut Text, With<LogCardTitle>>,
        Query<&mut Text, With<LogCardBody>>,
        Query<&mut Text, With<LogCardFooter>>,
    )>,
    mut showing: Local<Option<String>>,
) {
    let Ok(mut vis) = root.single_mut() else { return };

    let Some((title, body)) = queue.pending.first().cloned() else {
        *vis = Visibility::Hidden;
        *showing = None;
        return;
    };

    *vis = Visibility::Visible;

    // Only rewrite the card when the entry actually changes.
    if showing.as_deref() != Some(title.as_str()) {
        if let Ok(mut t) = texts.p0().single_mut() { **t = title.clone(); }
        if let Ok(mut t) = texts.p1().single_mut() { **t = body.clone(); }
        let remaining = queue.pending.len().saturating_sub(1);
        let footer = if remaining > 0 {
            format!("[Space] close  ({remaining} more recovered)")
        } else {
            "[Space] close".to_string()
        };
        if let Ok(mut t) = texts.p2().single_mut() { **t = footer; }
        *showing = Some(title);
    }

    if keys.just_pressed(KeyCode::Space) {
        queue.pending.remove(0);
        *showing = None;
    }
}

fn hide_log_card(mut root: Query<&mut Visibility, With<LogCardRoot>>) {
    if let Ok(mut v) = root.single_mut() {
        *v = Visibility::Hidden;
    }
}

/// Nothing should still be on screen once the run is over.
fn clear_log_queue(mut queue: ResMut<LogQueue>) {
    queue.pending.clear();
}

pub struct LogReaderPlugin;

impl Plugin for LogReaderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LogQueue>()
            .add_systems(Startup, spawn_log_card)
            .add_systems(Update, drive_log_card.run_if(in_state(GameState::Exploring)))
            .add_systems(OnEnter(GameState::GameOver), clear_log_queue)
            .add_systems(OnEnter(GameState::MainMenu), clear_log_queue)
            .add_systems(OnEnter(GameState::Truth), clear_log_queue)
            // The card only *drives* while Exploring, so without this it stays
            // on screen unattended once the state changes — it was still sitting
            // over the ending, offering "[Space] close".
            .add_systems(OnExit(GameState::Exploring), hide_log_card);
    }
}
