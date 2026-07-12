use bevy::prelude::*;
use super::new_projectiles::Projectile;
use super::new_projectiles::MissileProjectile;

// ============================================================================
// ENTITY LIMITS
// Prevents runaway entity spawning from breaking performance.
// ============================================================================

/// Max projectiles alive at once (kinetic + missiles combined)
pub const MAX_PROJECTILES: usize = 1000;

/// Max blocks on the player ship. Was 250 — a real ship had already grown to
/// 305 (evidently from before this cap was enforced everywhere), so the
/// player was permanently locked out of building with no way back under the
/// limit short of deleting ~55+ blocks. Raised to give real headroom above
/// what's already been built, while still bounding entity count.
pub const MAX_SHIP_BLOCKS: usize = 500;

/// System: despawn oldest projectiles if over the limit
pub fn enforce_projectile_limit(
    mut commands: Commands,
    projectile_query: Query<(Entity, &Projectile)>,
    missile_query: Query<(Entity, &MissileProjectile)>,
) {
    let proj_count = projectile_query.iter().count();
    let missile_count = missile_query.iter().count();
    let total = proj_count + missile_count;

    if total <= MAX_PROJECTILES { return; }

    // Despawn excess projectiles (oldest first — lowest lifetime remaining)
    let excess = total - MAX_PROJECTILES;
    let mut to_remove: Vec<(Entity, f32)> = projectile_query.iter()
        .map(|(e, p)| (e, p.lifetime))
        .collect();
    to_remove.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    for (entity, _) in to_remove.iter().take(excess) {
        commands.entity(*entity).despawn();
    }
}
