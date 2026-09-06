//! What the guns shoot at when the player hasn't locked anything by hand.
//!
//! Free aim sent every gun to the same cursor point, so a battery drilled one
//! tile at a time and a hull came apart in a single neat hole. Each gun now
//! picks its OWN block on the engaged ship and holds it for a moment before
//! rolling another, which spreads a salvo across the silhouette the way a
//! broadside should look.
//!
//! A right-click lock still overrides all of this — see [`AimLock`]. This is
//! only what happens in its absence.

use bevy::prelude::*;
use rand::Rng;

use crate::ai_ship::components::{AiShip, AiShipWreck};
use crate::building::ShipGrid;
use crate::components::*;

use super::{AimLock, TargetSelection};

/// Furthest away an unlocked battery will pick a fight on its own. Matches
/// the ship-target range in `cycle_target`, so what `\` can select and what
/// the guns will engage unprompted are the same set.
const AUTO_ENGAGE_RANGE: f32 = 9000.0;

/// How long one gun stays on one block before rolling another. Randomised per
/// gun so a battery drifts across the hull instead of every barrel jumping to
/// a new tile on the same frame.
const REAIM_MIN: f32 = 0.4;
const REAIM_MAX: f32 = 1.1;

/// The block one gun is working on while nothing is manually locked.
#[derive(Component)]
pub struct AutoAimPoint {
    /// The block itself, so a gun can tell when the tile it was drilling dies.
    pub block: Entity,
    /// The ship that owns it — lead prediction needs its velocity.
    pub ship: Entity,
    /// Refreshed every frame from the block's own transform, so the aim
    /// tracks a moving target rather than a stale world point.
    pub point: Vec2,
    ttl: f32,
}

/// Assigns each player gun a block on the engaged enemy.
///
/// Runs before the firing systems and writes only a component, so the firing
/// system reads its aim point out of the query it already has. That matters:
/// `fire_weapons_system` sits on Bevy's 16-parameter ceiling and cannot take
/// another query.
pub fn assign_auto_aim(
    time: Res<Time>,
    mut commands: Commands,
    aim_lock: Res<AimLock>,
    selection: Res<TargetSelection>,
    player: Query<(Entity, &Transform), With<Ship>>,
    // Without<AiShipWreck> is what makes the "still a live ship" filter below
    // actually mean that: a wreck is the same entity as the ship that died and
    // keeps its AiShip component, so an unfiltered query has the whole battery
    // drilling derelicts out to AUTO_ENGAGE_RANGE without the player touching
    // a key.
    enemies: Query<(Entity, &Transform, &ShipGrid), (With<AiShip>, Without<AiShipWreck>, Without<Ship>)>,
    block_pos: Query<&GlobalTransform>,
    mut weapons: Query<(Entity, &ChildOf, Option<&mut AutoAimPoint>), (With<Weapon>, Without<DestroyedModule>)>,
) {
    let Ok((player_ship, player_tf)) = player.single() else { return };

    // A manual lock owns the whole battery. Leave the guns alone.
    if aim_lock.is_locked() {
        return;
    }

    let player_pos = player_tf.translation.truncate();

    // Whatever `\` last selected, as long as it is still a live ship;
    // otherwise the nearest one in range.
    let engaged = selection
        .target
        .filter(|e| enemies.contains(*e))
        .or_else(|| {
            enemies
                .iter()
                .map(|(e, t, _)| (e, player_pos.distance(t.translation.truncate())))
                .filter(|(_, d)| *d <= AUTO_ENGAGE_RANGE)
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(e, _)| e)
        });

    let Some(engaged) = engaged else {
        // Nothing worth shooting. Drop the stale points so the guns fall back
        // to the cursor instead of holding aim on a ship that is gone.
        for (entity, _, point) in weapons.iter() {
            if point.is_some() {
                commands.entity(entity).remove::<AutoAimPoint>();
            }
        }
        return;
    };

    let Ok((_, _, grid)) = enemies.get(engaged) else { return };
    // ShipGrid holds LIVE blocks only — destroyed ones drop out of it — so a
    // gun can never be assigned a tile that has already been shot away.
    // Multi-cell modules appear once per cell they occupy, which is a bias
    // worth keeping: a bigger block is a bigger thing to hit.
    let live: Vec<Entity> = grid.cells.values().copied().collect();
    if live.is_empty() {
        return;
    }

    let dt = time.delta_secs();
    let mut rng = rand::thread_rng();

    for (entity, parent, point) in weapons.iter_mut() {
        if parent.parent() != player_ship {
            continue;
        }

        match point {
            Some(mut p) => {
                p.ttl -= dt;
                let stale = p.ship != engaged || !live.contains(&p.block);
                if p.ttl <= 0.0 || stale {
                    p.block = live[rng.gen_range(0..live.len())];
                    p.ship = engaged;
                    p.ttl = rng.gen_range(REAIM_MIN..REAIM_MAX);
                }
                if let Ok(gt) = block_pos.get(p.block) {
                    p.point = gt.translation().truncate();
                }
            }
            None => {
                let block = live[rng.gen_range(0..live.len())];
                let Ok(gt) = block_pos.get(block) else { continue };
                commands.entity(entity).try_insert(AutoAimPoint {
                    block,
                    ship: engaged,
                    point: gt.translation().truncate(),
                    ttl: rng.gen_range(REAIM_MIN..REAIM_MAX),
                });
            }
        }
    }
}
