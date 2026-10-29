//! DEPTHS_COMBAT_REPORT=1: a line every five seconds on how a fight stands,
//! for the combat test runs (tools/combat-lab): the player's hull, power,
//! crew and blocks, and every enemy ship in sensor range -- its class, hull,
//! distance and what it's doing. Nothing at all without the switch.

use bevy::prelude::*;

use crate::ai_ship::components::{AiShip, AiShipBehavior, AiShipClass, AiShipState, AiShipType, OwnedByAiShip};
use crate::components::{CrewMember, DestroyedModule, HullDestroyed, HullSegment, Module, Ship};

/// Seconds between report lines.
const EVERY_S: f32 = 5.0;

pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("DEPTHS_COMBAT_REPORT").is_some())
}

#[allow(clippy::too_many_arguments)]
pub fn combat_report(
    time: Res<Time>,
    mut next: Local<f32>,
    mut start_blocks: Local<Option<usize>>,
    hull: Res<crate::resources::HullState>,
    power: Res<crate::resources::PowerState>,
    player: Query<(&Transform, Option<&Children>), With<Ship>>,
    blocks: Query<(), (Or<(With<Module>, With<HullSegment>)>, Without<DestroyedModule>, Without<HullDestroyed>)>,
    crew: Query<(), (With<CrewMember>, Without<OwnedByAiShip>)>,
    enemies: Query<(&Transform, &AiShipType, Option<&AiShipClass>, &AiShipState, &AiShipBehavior), With<AiShip>>,
) {
    if !on() {
        return;
    }
    let now = time.elapsed_secs();
    if now < *next {
        return;
    }
    *next = now + EVERY_S;
    let Ok((at, children)) = player.single() else { return };
    let at = at.translation.truncate();
    let alive = children.map_or(0, |c| c.iter().filter(|e| blocks.contains(*e)).count());
    let start = *start_blocks.get_or_insert(alive);
    let mut seen: Vec<String> = enemies
        .iter()
        .filter_map(|(tf, faction, class, state, behavior)| {
            let d = tf.translation.truncate().distance(at);
            (d <= crate::camera::SENSOR_RANGE * 1.5).then(|| {
                format!(
                    "{:?}/{} hull={:.2} d={:.0} {}",
                    faction,
                    class.map_or("-", |c| c.0),
                    state.hull_integrity,
                    d,
                    if state.is_destroyed { "DESTROYED".to_string() } else { format!("{behavior:?}") }
                )
            })
        })
        .collect();
    seen.sort();
    info!(
        "COMBAT t={:.0} player hull={:.2} blocks={}/{} power={:.0}/{:.0} crew={} | {}",
        now,
        hull.hull_integrity,
        alive,
        start,
        power.total_power_generation,
        power.total_power_consumption,
        crew.iter().count(),
        seen.join(" ; ")
    );
}
