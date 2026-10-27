//! Decoys and chaff against designed missiles (combat::missile_design).
//!
//! A ship with a live Decoy Launcher throws heat decoys, and one with a Chaff
//! Dispenser throws chaff, every few seconds while an enemy designed missile
//! is inbound. Each is thrown clear of the hull, off to the side the threat
//! is coming from, so a missile that takes it flies past the ship rather than
//! into it. Every locked seeker of the matching kind -- heat for a decoy,
//! radar for chaff -- that comes near one rolls once against it
//! (`MissileStats::fool_chance`); fooled, it chases the decoy and is lost
//! when the decoy burns out. A command link or a dumb nose has no seeker to
//! fool. Both blocks did nothing at all before this.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use super::missile_design::MissileHead;
use super::missiles::BuiltMissile;
use crate::components::{DestroyedModule, Module, ModuleType, Ship, Velocity};
use crate::combat::new_projectiles::MissileProjectile;

/// Inbound missiles inside this range set the launchers off, and a decoy
/// fools a seeker inside this range of it.
pub const DECOY_RANGE: f32 = 3500.0;
/// Seconds between throws, per launcher.
pub const DECOY_INTERVAL: f32 = 4.0;
/// Seconds a decoy lasts.
pub const DECOY_LIFE: f32 = 4.0;

#[derive(Component, Debug)]
pub struct MissileDecoy {
    /// A heat decoy (fools heat seekers); otherwise chaff (fools radar).
    pub heat: bool,
    pub life: f32,
    /// The ship that threw it: its own missiles ignore it.
    pub ship: Entity,
    /// Missiles that have already rolled against this one.
    pub rolled: Vec<Entity>,
}

fn fools(heat: bool, head: MissileHead) -> bool {
    (heat && head == MissileHead::Heat) || (!heat && head == MissileHead::Radar)
}

/// Throw decoys and chaff while enemy designed missiles are inbound.
#[allow(clippy::type_complexity)]
pub fn throw_decoys(
    time: Res<Time>,
    mut commands: Commands,
    ships: Query<(Entity, &GlobalTransform, &Children, Option<&Velocity>), Or<(With<Ship>, With<crate::ai_ship::components::AiShip>)>>,
    launchers: Query<(Entity, &Module, &GlobalTransform), Without<DestroyedModule>>,
    missiles: Query<(&Transform, &MissileProjectile, &BuiltMissile)>,
    mut ready_at: Local<HashMap<Entity, f32>>,
) {
    let now = time.elapsed_secs();
    for (ship, ship_gt, children, velocity) in ships.iter() {
        let here = ship_gt.translation().truncate();
        for (launcher, module, gt) in children.iter().filter_map(|c| launchers.get(c).ok()) {
            let heat = match module.module_type {
                ModuleType::DecoyLauncher => true,
                ModuleType::ChaffDispenser => false,
                _ => continue,
            };
            if module.health <= 0.0 || ready_at.get(&launcher).is_some_and(|t| *t > now) {
                continue;
            }
            let inbound: Vec<Vec2> = missiles
                .iter()
                .filter(|(_, m, b)| m.owner_ship != Some(ship) && fools(heat, b.head))
                .map(|(t, _, _)| t.translation.truncate())
                .filter(|p| p.distance(here) < DECOY_RANGE)
                .collect();
            if inbound.is_empty() {
                continue;
            }
            ready_at.insert(launcher, now + DECOY_INTERVAL);
            if super::gun_assembly::trace() {
                info!("DECOY thrown by {:?}: {}, {} inbound", ship, if heat { "heat decoy" } else { "chaff" }, inbound.len());
            }
            // Off to one side of the line the missiles are coming in on.
            let toward = (inbound.iter().sum::<Vec2>() / inbound.len() as f32 - here).normalize_or(Vec2::X);
            let side = if rand::random::<bool>() { 1.0 } else { -1.0 };
            let across = toward.perp() * side;
            let at = gt.translation().truncate() + toward * 400.0 + across * 900.0;
            let drift = velocity.map(|v| v.0).unwrap_or(Vec2::ZERO) + across * 180.0 - toward * 100.0;
            commands.spawn((
                Sprite {
                    color: if heat { Color::srgba(1.0, 0.75, 0.35, 0.95) } else { Color::srgba(0.7, 0.76, 0.86, 0.35) },
                    custom_size: Some(if heat { Vec2::splat(18.0) } else { Vec2::splat(160.0) }),
                    ..default()
                },
                Transform::from_translation(at.extend(0.4)),
                Velocity(drift),
                MissileDecoy { heat, life: DECOY_LIFE, ship, rolled: Vec::new() },
            ));
        }
    }
}

/// Every locked seeker of the matching kind near a decoy rolls once against it.
pub fn seduce_seekers(
    mut decoys: Query<(Entity, &Transform, &mut MissileDecoy)>,
    mut missiles: Query<(Entity, &Transform, &MissileProjectile, &mut BuiltMissile)>,
) {
    for (decoy, decoy_tf, mut d) in decoys.iter_mut() {
        let at = decoy_tf.translation.truncate();
        for (m, tf, projectile, mut b) in missiles.iter_mut() {
            if b.decoy.is_some() || !b.locked || !fools(d.heat, b.head) || projectile.owner_ship == Some(d.ship) {
                continue;
            }
            if d.rolled.contains(&m) || tf.translation.truncate().distance(at) > DECOY_RANGE {
                continue;
            }
            d.rolled.push(m);
            if rand::random::<f32>() < b.stats.fool_chance {
                b.decoy = Some(decoy);
                if super::gun_assembly::trace() {
                    info!("MISSILE fooled by {}", if d.heat { "a heat decoy" } else { "chaff" });
                }
            }
        }
    }
}

/// Decoys drift, slow and burn out.
pub fn age_decoys(time: Res<Time>, mut commands: Commands, mut decoys: Query<(Entity, &mut Transform, &mut Velocity, &mut MissileDecoy, &mut Sprite)>) {
    let dt = time.delta_secs();
    for (e, mut tf, mut v, mut d, mut sprite) in decoys.iter_mut() {
        d.life -= dt;
        if d.life <= 0.0 {
            commands.entity(e).try_despawn();
            continue;
        }
        tf.translation += (v.0 * dt).extend(0.0);
        v.0 *= (-0.6 * dt).exp();
        let a = (d.life / DECOY_LIFE).clamp(0.0, 1.0);
        sprite.color.set_alpha(if d.heat { 0.95 * a } else { 0.35 * a });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoys_fool_heat_seekers_and_chaff_fools_radar() {
        assert!(fools(true, MissileHead::Heat));
        assert!(!fools(true, MissileHead::Radar));
        assert!(fools(false, MissileHead::Radar));
        assert!(!fools(false, MissileHead::Heat));
        // Nothing to fool on a command link or a dumb nose.
        for heat in [true, false] {
            assert!(!fools(heat, MissileHead::CommandLink));
            assert!(!fools(heat, MissileHead::DumbNose));
        }
    }
}
