use bevy::prelude::*;

use crate::ai_ship::components::{AiShip, AiShipState, OwnedByAiShip};
use crate::components::*;
use crate::events::*;
use super::selection::{TargetSelection, TargetType};

// ============================================================================
// AIM LOCK — right-click a block on an enemy ship and the guns work on THAT
// block until you say otherwise. Right-click empty space to drop the lock and
// go back to manual fire at the cursor.
//
// Deliberately not the old Tab-lock: that picked the ship for you and aimed
// at its core, which meant the player never chose where to hit. Here the
// player picks the spot — a gun mount, an engine, or just a patch of hull —
// and the whole battery works it.
// ============================================================================

/// How close to a block the cursor has to be to grab it. Blocks are 66 units
/// across, so this is forgiving without reaching onto a neighbouring ship.
const PICK_RADIUS: f32 = 50.0;

/// Beyond this the lock still holds but the guns stop auto-firing — no
/// dumping magazines at something a screen and a half away.
const AUTO_FIRE_SLACK: f32 = 1.15;

#[derive(Resource, Default)]
pub struct AimLock {
    /// The enemy ship root that owns the locked block.
    pub ship: Option<Entity>,
    /// The specific block (hull tile or module) under fire.
    pub block: Option<Entity>,
    /// The locked block's world position, refreshed every frame by
    /// maintain_aim_lock. Firing systems read this instead of re-querying, so
    /// adding lock support to a weapon is one line.
    pub point: Vec2,
    /// Set on frames where a right-click was spent locking or unlocking, so
    /// the radial menu doesn't pop at the same time.
    pub click_consumed: bool,
}

impl AimLock {
    pub fn is_locked(&self) -> bool {
        self.ship.is_some() && self.block.is_some()
    }

    /// World point the guns should converge on, if anything is locked.
    pub fn aim_point(&self) -> Option<Vec2> {
        self.is_locked().then_some(self.point)
    }

    fn clear(&mut self) {
        self.ship = None;
        self.block = None;
    }
}

/// Right-click: lock the block under the cursor, or clear the lock if the
/// click landed on empty space.
pub fn aim_lock_input(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    mut lock: ResMut<AimLock>,
    mut selection: ResMut<TargetSelection>,
    blocks: Query<(Entity, &GlobalTransform, &OwnedByAiShip, Option<&Module>, Option<&HullSegment>)>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    lock.click_consumed = false;
    if !mouse.just_pressed(MouseButton::Right) { return; }

    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_gt)) = camera_query.single() else { return };
    let Some(cursor) = window.cursor_position()
        .and_then(|p| camera.viewport_to_world_2d(cam_gt, p).ok())
    else { return };

    // Nearest LIVE block to the cursor. Dead blocks are skipped so clicking a
    // hole in the hull grabs the armour beside it rather than a corpse tile
    // no shot will ever register on.
    let mut best: Option<(Entity, Entity, Vec2, f32, String)> = None;
    for (entity, gt, owned, module, hull) in blocks.iter() {
        let alive = match (module, hull) {
            (Some(m), _) => m.health > 0.0,
            (_, Some(h)) => h.health > 0.0,
            _ => continue,
        };
        if !alive { continue; }

        let pos = gt.translation().truncate();
        let dist = cursor.distance(pos);
        if dist > PICK_RADIUS { continue; }
        if best.as_ref().is_some_and(|(_, _, _, d, _)| *d <= dist) { continue; }

        let name = match (module, hull) {
            (Some(m), _) => m.module_type.name().to_string(),
            _ => "hull plating".to_string(),
        };
        best = Some((entity, owned.root, pos, dist, name));
    }

    lock.click_consumed = true;

    match best {
        Some((block, ship, pos, _, name)) => {
            lock.ship = Some(ship);
            lock.block = Some(block);
            lock.point = pos;
            // Everything that already homes/targets by ship (missiles, ion,
            // EMP) keeps working off the existing selection — the lock only
            // adds which BLOCK the direct-fire guns converge on.
            selection.target = Some(ship);
            selection.target_type = TargetType::Ship;
            notifications.write(ShowNotification {
                message: format!("Guns on {} — right-click empty space to release.", name),
                notification_type: NotificationType::Info,
                duration: 2.5,
            });
        }
        None => {
            // Empty space: drop everything and hand the guns back to the mouse.
            if lock.is_locked() || selection.target.is_some() {
                lock.clear();
                selection.target = None;
                selection.target_type = TargetType::None;
                notifications.write(ShowNotification {
                    message: "Weapons free — manual fire.".into(),
                    notification_type: NotificationType::Info,
                    duration: 2.0,
                });
            } else {
                // Nothing was locked, so this click isn't ours — let the
                // radial menu have it.
                lock.click_consumed = false;
            }
        }
    }
}

/// Keeps the lock pointed at something real: refreshes the aim point, walks
/// to the neighbouring block when the locked one is shot away (so sustained
/// fire keeps working the same hole instead of silently dropping), and
/// releases when the ship is dead or gone.
pub fn maintain_aim_lock(
    mut lock: ResMut<AimLock>,
    mut selection: ResMut<TargetSelection>,
    blocks: Query<(Entity, &GlobalTransform, &OwnedByAiShip, Option<&Module>, Option<&HullSegment>)>,
    ships: Query<&AiShipState, With<AiShip>>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let Some(ship) = lock.ship else { return };

    // Ship gone (despawned) or out of the fight — nothing left to aim at.
    let ship_alive = ships.get(ship).map(|s| !s.is_destroyed).unwrap_or(false);
    if !ship_alive {
        lock.clear();
        selection.target = None;
        selection.target_type = TargetType::None;
        notifications.write(ShowNotification {
            message: "Target out of the fight — weapons free.".into(),
            notification_type: NotificationType::Info,
            duration: 2.0,
        });
        return;
    }

    let live_block = lock.block.and_then(|b| blocks.get(b).ok()).filter(|(_, _, _, module, hull)| {
        match (module, hull) {
            (Some(m), _) => m.health > 0.0,
            (_, Some(h)) => h.health > 0.0,
            _ => false,
        }
    });

    if let Some((_, gt, _, _, _)) = live_block {
        lock.point = gt.translation().truncate();
        return;
    }

    // Locked block is gone: keep hammering the same spot by grabbing the
    // nearest surviving block on that ship.
    let last_point = lock.point;
    let next = blocks.iter()
        .filter(|(_, _, owned, module, hull)| {
            owned.root == ship && match (module, hull) {
                (Some(m), _) => m.health > 0.0,
                (_, Some(h)) => h.health > 0.0,
                _ => false,
            }
        })
        .map(|(entity, gt, _, _, _)| (entity, gt.translation().truncate()))
        .min_by(|a, b| {
            last_point.distance_squared(a.1)
                .partial_cmp(&last_point.distance_squared(b.1))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    match next {
        Some((entity, pos)) => {
            lock.block = Some(entity);
            lock.point = pos;
        }
        None => {
            lock.clear();
            selection.target = None;
            selection.target_type = TargetType::None;
        }
    }
}

/// Whether the battery should be auto-firing at the lock this frame: locked,
/// and the target inside the reach of the longest gun aboard (plus a little
/// slack) so a lock across the system doesn't empty the magazines.
pub fn auto_fire_engaged(
    lock: &AimLock,
    ship_pos: Vec2,
    max_weapon_range: f32,
) -> bool {
    lock.aim_point()
        .is_some_and(|p| ship_pos.distance(p) <= max_weapon_range * AUTO_FIRE_SLACK)
}

/// Draws the lock reticle: a box on the locked block, in the same idiom as
/// the target bracket but tighter, so it reads as "this block" not "this ship".
#[derive(Component)]
pub struct AimLockMarker;

pub fn draw_aim_lock(
    mut commands: Commands,
    lock: Res<AimLock>,
    existing: Query<Entity, With<AimLockMarker>>,
    blocks: Query<(Option<&Module>, Option<&HullSegment>)>,
) {
    for entity in existing.iter() {
        commands.entity(entity).despawn();
    }
    let Some(point) = lock.aim_point() else { return };

    // Four corner ticks around the block.
    const HALF: f32 = 22.0;
    const TICK: f32 = 9.0;
    const THICK: f32 = 2.0;
    let color = Color::srgb(1.0, 0.35, 0.3);
    for (sx, sy) in [(-1.0, 1.0), (1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
        let corner = point + Vec2::new(sx * HALF, sy * HALF);
        commands.spawn((
            Sprite { color, custom_size: Some(Vec2::new(TICK, THICK)), ..default() },
            Transform::from_xyz(corner.x - sx * TICK * 0.5, corner.y, 6.0),
            AimLockMarker,
        ));
        commands.spawn((
            Sprite { color, custom_size: Some(Vec2::new(THICK, TICK)), ..default() },
            Transform::from_xyz(corner.x, corner.y - sy * TICK * 0.5, 6.0),
            AimLockMarker,
        ));
    }

    // Condition of the block under fire, as a thin bar below the reticle —
    // without it there's no way to tell whether the plate is nearly off or
    // you've been chewing a fresh one for ten seconds.
    let frac = lock.block
        .and_then(|b| blocks.get(b).ok())
        .and_then(|(module, hull)| match (module, hull) {
            (Some(m), _) if m.max_health > 0.0 => Some((m.health / m.max_health).clamp(0.0, 1.0)),
            (_, Some(h)) if h.max_health > 0.0 => Some((h.health / h.max_health).clamp(0.0, 1.0)),
            _ => None,
        });
    let Some(frac) = frac else { return };

    const BAR_W: f32 = 44.0;
    const BAR_H: f32 = 3.0;
    let bar_y = point.y - HALF - 7.0;
    commands.spawn((
        Sprite { color: Color::srgba(0.0, 0.0, 0.0, 0.55), custom_size: Some(Vec2::new(BAR_W, BAR_H)), ..default() },
        Transform::from_xyz(point.x, bar_y, 6.0),
        AimLockMarker,
    ));
    let fill = BAR_W * frac;
    commands.spawn((
        Sprite {
            color: if frac > 0.5 {
                Color::srgb(0.9, 0.75, 0.3)
            } else {
                Color::srgb(1.0, 0.4, 0.25)
            },
            custom_size: Some(Vec2::new(fill.max(1.0), BAR_H)),
            ..default()
        },
        Transform::from_xyz(point.x - (BAR_W - fill) * 0.5, bar_y, 6.01),
        AimLockMarker,
    ));
}
