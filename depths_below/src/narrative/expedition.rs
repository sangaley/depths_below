//! What the player is for.
//!
//! Until now nothing in the game named an objective. The menu said "Build your
//! ship. Explore the void. Survive." and the tutorial ended on "push deeper",
//! which is a direction, not a reason. A player finished training and had no
//! idea what they were supposed to do.
//!
//! The answer was already sitting in the game: twenty-three logs left by an
//! expedition that went out before you and did not come back. They are already
//! tiered by distance, so reading them in order *is* going outward. The trail
//! existed; nobody had ever told the player it was a trail.
//!
//! It also sets the reveal up correctly. You spend the game following the last
//! expedition's records, and the point of the story is that the last
//! expedition was you.

use bevy::prelude::*;

use crate::resources::Statistics;
use crate::states::GameState;
use crate::ui::theme::{ThemeColors, ThemeFonts};

use super::logs::{LOG_ENTRIES, MAX_TIER};

/// How far down the trail this build goes.
///
/// The demo stops partway: the player reads the opening records, the voice has
/// started to go wrong, and then the trail runs out. Raising this to MAX_TIER
/// is the whole difference between the demo and the full game, so it is one
/// constant rather than a scatter of conditions.
pub const TRAIL_MAX_TIER: u8 = 1;

/// Progress along the trail, derived rather than stored.
#[derive(Resource, Debug, Default)]
pub struct Expedition {
    pub found: usize,
    pub total: usize,
    /// True once every record this build carries has been recovered.
    pub trail_exhausted: bool,
    /// Latched so the closing message is said once, not every frame.
    wall_announced: bool,
}

/// Records that count toward the trail in this build.
fn trail_total() -> usize {
    LOG_ENTRIES.iter().filter(|e| e.tier <= TRAIL_MAX_TIER).count()
}

fn update_expedition(
    stats: Res<Statistics>,
    mut exp: ResMut<Expedition>,
    mut notifications: MessageWriter<crate::events::ShowNotification>,
) {
    exp.total = trail_total();
    exp.found = LOG_ENTRIES
        .iter()
        .filter(|e| e.tier <= TRAIL_MAX_TIER)
        .filter(|e| stats.logs_found.iter().any(|f| f == e.title))
        .count();

    exp.trail_exhausted = exp.total > 0 && exp.found >= exp.total;

    if exp.trail_exhausted && !exp.wall_announced {
        exp.wall_announced = true;
        // The wall. Deliberately not a failure and not a victory — the records
        // simply stop, and where they stop is where this build stops.
        notifications.write(crate::events::ShowNotification {
            message: "That is every record recovered. The expedition went further than \
                      its logs did — past charted space, where nothing is written down."
                .into(),
            notification_type: crate::events::NotificationType::Success,
            duration: 12.0,
        });
    }
}

#[derive(Component)]
struct ExpeditionHudText;

fn spawn_expedition_hud(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(92.0),
            left: Val::Px(14.0),
            ..default()
        },
        Text::new(""),
        TextFont { font_size: FontSize::Px(ThemeFonts::CAPTION), ..default() },
        TextColor(ThemeColors::TEXT_MUTED),
        ZIndex(6),
        ExpeditionHudText,
    ));
}

fn update_expedition_hud(
    exp: Res<Expedition>,
    state: Res<State<GameState>>,
    mut hud: Query<(&mut Text, &mut TextColor, &mut Visibility), With<ExpeditionHudText>>,
) {
    let Ok((mut text, mut colour, mut vis)) = hud.single_mut() else { return };

    // Only while actually out there. It has no business over the menu or the
    // ending.
    let showing = matches!(*state.get(), GameState::Exploring | GameState::StationDocked);
    *vis = if showing { Visibility::Inherited } else { Visibility::Hidden };
    if !showing {
        return;
    }

    let want = if exp.trail_exhausted {
        "EXPEDITION  the records end here".to_string()
    } else {
        format!("EXPEDITION  {} of {} records recovered", exp.found, exp.total)
    };
    if **text != want {
        **text = want;
    }
    colour.0 = if exp.trail_exhausted { ThemeColors::ACCENT_ORANGE } else { ThemeColors::TEXT_MUTED };
}

fn clear_on_new_run(mut exp: ResMut<Expedition>) {
    *exp = Expedition::default();
}

pub struct ExpeditionPlugin;

impl Plugin for ExpeditionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Expedition>()
            .add_systems(Startup, spawn_expedition_hud)
            .add_systems(Update, (update_expedition, update_expedition_hud).chain())
            .add_systems(OnEnter(GameState::MainMenu), clear_on_new_run);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The trail must have entries in this build, or the objective reads
    /// "0 of 0" and tells the player nothing.
    #[test]
    fn the_trail_has_records() {
        assert!(trail_total() > 0);
    }

    /// And it must stop short of the end, or the demo gives away the finale.
    /// The whole point of the wall is that the good part is still unseen.
    #[test]
    fn the_demo_stops_short_of_the_finale() {
        assert!(TRAIL_MAX_TIER < MAX_TIER, "the demo trail reaches the last tier");
        let beyond = LOG_ENTRIES.iter().filter(|e| e.tier > TRAIL_MAX_TIER).count();
        assert!(beyond > 0, "nothing is held back for the full game");
    }

    /// Long enough to be a thread, short enough to finish in a sitting.
    #[test]
    fn the_trail_is_a_demo_length() {
        let n = trail_total();
        assert!((4..=14).contains(&n), "{n} records is the wrong size for a demo trail");
    }

    /// A fresh run starts at nothing found and no wall announced.
    #[test]
    fn a_fresh_run_starts_empty() {
        let e = Expedition::default();
        assert_eq!(e.found, 0);
        assert!(!e.trail_exhausted);
        assert!(!e.wall_announced);
    }
}
