use bevy::prelude::*;
use crate::components::*;
use crate::combat::new_projectiles::Projectile;
use crate::combat::new_projectiles::MissileProjectile;

// ============================================================================
// RECOIL SYSTEM — Newton's Third Law
// Every shot pushes the ship backward. Force = projectile mass × velocity.
// Ship recoil = force / ship mass. No friction in space — it accumulates.
// Unbalanced weapons cause spin. Recoil Absorber reduces it.
// ============================================================================

/// Applied each frame a weapon fires — accumulates recoil force
#[derive(Resource, Default)]
pub struct RecoilAccumulator {
    /// Linear force (pushes ship)
    pub force: Vec2,
    /// Torque (spins ship from off-center weapons)
    pub torque: f32,
}

/// Rotational inertia, as a multiple of mass.
///
/// A ship is not a point, so a shot off the centreline yaws it. Tuned so a
/// railgun ~300 units off-axis kicks about 0.15 rad/s — a visible slew that
/// the helm shrugs off, not a spin.
const MOMENT_OF_INERTIA: f32 = 15_000.0;

/// Speed a missile actually leaves the tube at, for recoil purposes. Mirrors
/// `missiles::EJECT_SPEED` — the gas charge is what pushes back on the ship,
/// not the motor that lights later, well clear of the hull.
const EJECT_RECOIL_SPEED: f32 = 200.0;

/// System: when projectiles are spawned, calculate recoil and apply to ship.
/// Uses Newton's third law: F_recoil = mass_projectile × velocity_projectile
pub fn apply_weapon_recoil(
    mut recoil: ResMut<RecoilAccumulator>,
    mut ship_query: Query<(&mut Velocity, &mut ShipPhysics), With<Ship>>,
) {
    let Ok((mut velocity, mut physics)) = ship_query.single_mut() else { return };

    if recoil.force.length_squared() < 0.001 && recoil.torque.abs() < 0.001 {
        return;
    }

    // The accumulator holds an IMPULSE, not a sustained force: it is filled
    // by the rounds that spawned this frame and cleared below. Scaling it by
    // dt made every shot's kick proportional to frame time and about sixty
    // times too small — a railgun moved the ship 0.1 u/s instead of 7.5, so
    // recoil existed on paper and was imperceptible in the seat.
    velocity.0 += recoil.force / physics.mass;

    // Real angular recoil. This used to fake spin by nudging the ship
    // sideways, because the system had no mutable ShipPhysics to write to —
    // so an off-centre gun shoved the hull instead of turning it.
    physics.angular_velocity += recoil.torque / (physics.mass * MOMENT_OF_INERTIA);

    // Clear accumulator for next frame
    recoil.force = Vec2::ZERO;
    recoil.torque = 0.0;
}

/// System: calculate recoil when kinetic projectiles are spawned
pub fn accumulate_projectile_recoil(
    new_projectiles: Query<(&Projectile, &Velocity, &Transform), Added<Projectile>>,
    _weapon_query: Query<(&Module, &GlobalTransform), With<Weapon>>,
    ship_query: Query<&Transform, With<Ship>>,
    recoil_absorber_query: Query<&Module, Without<DestroyedModule>>,
    mut recoil: ResMut<RecoilAccumulator>,
) {
    let Ok(ship_transform) = ship_query.single() else { return };
    let ship_center = ship_transform.translation.truncate();

    // Check for recoil absorbers (reduce recoil)
    let absorber_count = recoil_absorber_query.iter()
        .filter(|m| m.module_type == ModuleType::RecoilAbsorber && m.is_active)
        .count();
    let absorber_reduction = 1.0 - (absorber_count as f32 * 0.25).min(0.75); // Max 75% reduction

    for (projectile, proj_velocity, proj_transform) in new_projectiles.iter() {
        // Projectile momentum = damage as proxy for mass × velocity
        // Heavier rounds (more damage) create more recoil
        let projectile_mass = projectile.damage * 0.01; // Scale factor
        let proj_speed = proj_velocity.0.length();

        // Recoil force = opposite of projectile direction × momentum
        let proj_dir = proj_velocity.0.normalize_or_zero();
        let recoil_force = -proj_dir * projectile_mass * proj_speed;

        // Apply absorber reduction
        let final_force = recoil_force * absorber_reduction;

        recoil.force += final_force;

        // Calculate torque from off-center firing
        let fire_pos = proj_transform.translation.truncate();
        let offset = fire_pos - ship_center;
        // Cross product in 2D = offset.x * force.y - offset.y * force.x
        let torque = offset.x * final_force.y - offset.y * final_force.x;
        recoil.torque += torque;
    }
}

/// System: calculate recoil when missiles are launched (smaller than kinetic)
pub fn accumulate_missile_recoil(
    new_missiles: Query<(&MissileProjectile, &Velocity), Added<MissileProjectile>>,
    mut recoil: ResMut<RecoilAccumulator>,
    recoil_absorber_query: Query<&Module, Without<DestroyedModule>>,
) {
    let absorber_count = recoil_absorber_query.iter()
        .filter(|m| m.module_type == ModuleType::RecoilAbsorber && m.is_active)
        .count();
    let absorber_reduction = 1.0 - (absorber_count as f32 * 0.25).min(0.75);

    for (missile, _missile_velocity) in new_missiles.iter() {
        // Missile launch recoil is smaller than kinetic (slow launch).
        //
        // Off the EJECT charge and the tube's heading, not off the missile's
        // absolute velocity: a missile inherits the ship's own motion, so
        // reading its raw speed billed the launcher for however fast the hull
        // was already travelling — a full-throttle salvo kicked far harder
        // than the same salvo at rest, in whatever direction the ship was
        // already going.
        let missile_mass = missile.damage * 0.005;
        recoil.force += -missile.launch_dir * missile_mass * EJECT_RECOIL_SPEED * absorber_reduction;
    }
}
