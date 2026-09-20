//! Narrative layer: what the game is about, told through what it already
//! simulates rather than through cutscenes.

pub mod logs;
pub mod doppelganger;
pub mod reader;
pub mod truth;

use bevy::prelude::*;

use crate::celestial::resources::{GalaxyMap, SystemDiscovery, SystemStreamingManager};
use crate::resources::Statistics;
use crate::states::GameState;

/// How far along the story is, and how bad it is where the player is standing.
///
/// Deliberately **derived, never stored**. Everything it reads is already in
/// the save — which systems have been visited, which logs have been read, and
/// where the player is — so there is no new field to persist, no migration,
/// and no way for it to drift out of step with the run. It also means loading
/// an old save produces the right value immediately rather than a zeroed one.
///
/// This matters for a second reason. `handle_save_request` is at Bevy's
/// 16-parameter limit and says so in a comment, so a new saved resource is not
/// a free change. Deriving sidesteps that entirely.
/// Raised the moment the finale entry is *read*, and only then.
///
/// The ending used to trigger on `Statistics.logs_found` containing the
/// finale, which is a state rather than an event: loading any save made after
/// the ending would replay the whole sequence, and a new expedition that
/// inherited the old run's statistics would fire it in the first second.
/// Finding it is the moment that means something.
#[derive(Resource, Debug, Default)]
pub struct FinaleFound(pub bool);

#[derive(Resource, Debug, Default)]
pub struct CascadeState {
    /// 0.0 at the first launch, 1.0 at the far edge with everything read.
    /// Progression: how far the player has come, and it never goes down.
    pub level: f32,
    /// Local intensity: the log tier of the system the player is in right
    /// now. Flying home lowers this; it is "how bad is it *here*", not "how
    /// far along am I".
    pub ring: u8,
    /// Fraction of the corpus read, 0.0..1.0.
    pub read: f32,
    /// Furthest galaxy distance any visited system sits at, over the galaxy
    /// radius. The high-water mark, so retreating does not rewind the story.
    pub reach: f32,
}

impl CascadeState {
    /// Debug override, in the house style (`DEPTHS_MOVETEST`,
    /// `DEPTHS_SKIP_MENU`, `DEPTHS_AI_VS_AI_TEST`). Without it, seeing a late
    /// beat costs an hour of flying, which is how late beats end up untested.
    ///
    /// `DEPTHS_CASCADE=0.8` pins the level; ring follows unless the player is
    /// somewhere that says otherwise.
    fn override_level() -> Option<f32> {
        std::env::var("DEPTHS_CASCADE").ok()?.parse::<f32>().ok().map(|v| v.clamp(0.0, 1.0))
    }
}

/// Weighting between "how far out have you been" and "how much have you read".
/// Distance leads because it is the axis the player actually steers.
const REACH_WEIGHT: f32 = 0.6;
const READ_WEIGHT: f32 = 0.4;

fn update_cascade(
    galaxy: Res<GalaxyMap>,
    streaming: Res<SystemStreamingManager>,
    stats: Res<Statistics>,
    mut cascade: ResMut<CascadeState>,
) {
    let radius = crate::celestial::galaxy::GALAXY_RADIUS.max(1.0);

    // High-water mark over everywhere the player has actually been. Located
    // but unvisited systems do not count: seeing a light is not going there.
    let reach = galaxy
        .systems
        .iter()
        .filter(|s| matches!(s.discovery, SystemDiscovery::Visited))
        .map(|s| s.galaxy_pos.length() / radius)
        .fold(0.0f32, f32::max)
        .clamp(0.0, 1.0);

    let total = logs::LOG_ENTRIES.len().max(1) as f32;
    let read = (stats.logs_found.len() as f32 / total).clamp(0.0, 1.0);

    cascade.reach = reach;
    cascade.read = read;
    cascade.level = CascadeState::override_level()
        .unwrap_or_else(|| (REACH_WEIGHT * reach + READ_WEIGHT * read).clamp(0.0, 1.0));

    // Where the player is standing right now. A blind warp into empty space
    // has no system, so fall back to raw distance from Haven.
    cascade.ring = match streaming.loaded_system.and_then(|id| galaxy.systems.iter().find(|s| s.id == id)) {
        Some(sys) => logs::tier_for_danger(sys.danger_tier),
        None => {
            let d = streaming.current_galaxy_pos.length() / radius;
            ((d * (logs::MAX_TIER as f32 + 1.0)) as u8).min(logs::MAX_TIER)
        }
    };
}

pub struct NarrativePlugin;

impl Plugin for NarrativePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FinaleFound>();
        app.add_plugins(reader::LogReaderPlugin);
        app.add_plugins(truth::TruthPlugin);
        app.add_plugins(doppelganger::DoppelgangerPlugin);
        app.init_resource::<CascadeState>().add_systems(
            Update,
            update_cascade.run_if(in_state(GameState::Exploring).or_else(in_state(GameState::StationDocked))),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reading everything without leaving home must not finish the story, and
    /// flying to the edge without reading anything must not either. Both
    /// halves have to contribute or one of them is decoration.
    #[test]
    fn neither_axis_alone_completes_the_arc() {
        assert!((REACH_WEIGHT + READ_WEIGHT - 1.0).abs() < f32::EPSILON);
        assert!(REACH_WEIGHT < 1.0 && READ_WEIGHT < 1.0);
        assert!(REACH_WEIGHT > READ_WEIGHT, "distance is the axis the player steers");
    }

    /// The opening must read as zero. If a fresh run starts part-way up the
    /// scale, the quiet first hour the story depends on never happens.
    #[test]
    fn a_fresh_run_is_silent() {
        let c = CascadeState::default();
        assert_eq!(c.level, 0.0);
        assert_eq!(c.ring, 0);
    }
}
