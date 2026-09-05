use bevy::prelude::*;
use crate::components::*;
use crate::resources::*;
use crate::events::*;
use crate::states::{GameState, CombatSet, SpatialSet};

// Re-use the public HitEffect from ship::damage
use crate::ship::damage::HitEffect;

mod weapons;
pub(crate) mod projectiles;
mod mines;
mod effects;
pub mod shields;
pub mod turrets;
pub mod targeting;
pub mod new_projectiles;
pub mod missiles;
pub mod point_defense;
pub mod severance;
pub mod chain_reactions;
pub mod combat_features;
pub mod energy_weapons;
pub mod ammo_types;
pub mod recoil;
pub mod limits;
pub mod impact;

/// Dev switch: no weapon consumes ammunition while true (player and AI).
/// Ammo economy comes back when the combat loop is tuned.
pub const INFINITE_AMMO: bool = true;

/// Floating damage number that drifts upward and fades out
#[derive(Component)]
pub struct FloatingDamage {
    pub timer: Timer,
    pub velocity: f32,
}

// Helper functions to get effective weapon stats (CalculatedStats or base Weapon)
pub(crate) fn get_weapon_damage(calculated: Option<&CalculatedStats>, weapon: &Weapon) -> f32 {
    calculated
        .and_then(|c| c.weapon.as_ref())
        .map(|w| w.damage)
        .unwrap_or(weapon.damage)
}

pub(crate) fn get_weapon_range(calculated: Option<&CalculatedStats>, weapon: &Weapon) -> f32 {
    calculated
        .and_then(|c| c.weapon.as_ref())
        .map(|w| w.range)
        .unwrap_or(weapon.range)
}

pub(crate) fn get_weapon_fire_rate(calculated: Option<&CalculatedStats>, weapon: &Weapon) -> f32 {
    calculated
        .and_then(|c| c.weapon.as_ref())
        .map(|w| w.fire_rate)
        .unwrap_or(weapon.fire_rate)
}

/// Projectile speed base. Was 600 — at the weapon ranges now reachable
/// (lifetime derives from range instead of a fixed timer, see
/// projectiles::spawn_projectile), that meant a max-range shot (9600) took
/// 16+ seconds to arrive. 1800 gets a max-range bullet there in ~3.5s.
pub(crate) const PROJECTILE_SPEED: f32 = 1800.0;
/// Projectile collision radius
pub(crate) const PROJECTILE_RADIUS: f32 = 12.0;
/// Creature collision radius
pub(crate) const CREATURE_RADIUS: f32 = 24.0;
/// Ship collision radius (for enemy projectiles)
pub(crate) const SUBMARINE_RADIUS: f32 = 60.0;

/// Spawn a visual hit-flash sprite at the given position.
pub(crate) fn spawn_hit_effect(commands: &mut Commands, position: Vec2, color: Color, size: f32) {
    commands.spawn((
        (Sprite {
                color,
                custom_size: Some(Vec2::splat(size)),
                ..default()
            }, Transform::from_xyz(position.x, position.y, 0.6)),
        HitEffect {
            timer: Timer::from_seconds(0.2, TimerMode::Once),
        },
    ));
}

/// Sparks thrown off an impact, sprayed along `dir`.
///
/// Directional on purpose. A symmetric puff says "something happened here"; a
/// fan says "it went THAT way", and for a ricochet the direction IS the
/// information — it's the only way to see that a round left along a new
/// heading instead of simply failing to do damage.
///
/// `energy` (0..1) scales count and spread: a grazing skip throws a long thin
/// streak, a near-square one that barely turned throws a short hot burst.
pub(crate) fn spawn_impact_sparks(
    commands: &mut Commands,
    position: Vec2,
    dir: Vec2,
    energy: f32,
    count: usize,
) {
    let dir = dir.normalize_or_zero();
    if dir == Vec2::ZERO {
        return;
    }
    let energy = energy.clamp(0.0, 1.0);
    for i in 0..count {
        // Grazing hits stay tight to the new heading; blunt ones scatter.
        let spread = (rand::random::<f32>() - 0.5) * (1.4 - energy * 0.9);
        let heading = Vec2::from_angle(spread).rotate(dir);
        let speed = 180.0 + rand::random::<f32>() * (260.0 + energy * 420.0);
        let life = 0.14 + rand::random::<f32>() * 0.26;
        // A few white-hot ones among the orange so the spray has depth
        // instead of reading as one flat colour.
        let hot = i % 3 == 0;
        commands.spawn((
            Sprite {
                color: if hot {
                    Color::srgb(1.0, 0.97, 0.86)
                } else {
                    Color::srgb(1.0, 0.68, 0.24)
                },
                custom_size: Some(Vec2::new(if hot { 6.0 } else { 4.0 }, 1.8)),
                ..default()
            },
            Transform {
                translation: position.extend(0.62),
                // Streaks lie along their own flight, so the spray reads as
                // motion rather than as confetti.
                rotation: Quat::from_rotation_z(heading.y.atan2(heading.x)),
                ..default()
            },
            crate::vfx::particles::Particle {
                velocity: heading * speed,
                lifetime: life,
                max_lifetime: life,
                fade: true,
                shrink: true,
            },
        ));
    }
}

/// Spawn a floating word that drifts upward and fades out — the same channel
/// as a damage number, for outcomes that aren't a number. A round that skips
/// off a plate did something, and reporting it as "-3" reads as a bad hit
/// rather than as a deflection.
pub(crate) fn spawn_floating_label(commands: &mut Commands, position: Vec2, text: &str, color: Color) {
    commands.spawn((
        Text2d::new(text.to_string()),
        TextFont { font_size: FontSize::Px(16.0), ..default() },
        TextColor(color),
        Transform::from_xyz(position.x, position.y + 20.0, 1.0),
        FloatingDamage {
            timer: Timer::from_seconds(0.9, TimerMode::Once),
            velocity: 34.0,
        },
    ));
}

/// Spawn a floating damage number that drifts upward and fades out.
pub(crate) fn spawn_floating_damage(commands: &mut Commands, position: Vec2, damage: f32, color: Color) {
    commands.spawn((
        Text2d::new(format!("-{}", damage as i32)),
        TextFont { font_size: FontSize::Px(18.0), ..default() },
        TextColor(color),
        Transform::from_xyz(position.x, position.y + 20.0, 1.0),
        FloatingDamage {
            timer: Timer::from_seconds(0.8, TimerMode::Once),
            velocity: 40.0,
        },
    ));
}

/// Apply random angular spread based on accuracy (0..1). Returns adjusted target position.
pub(crate) fn apply_accuracy_spread(origin: Vec2, target_pos: Vec2, accuracy: f32, max_spread_degrees: f32) -> Vec2 {
    let spread = (1.0 - accuracy) * max_spread_degrees;
    let angle_offset = (rand::random::<f32>() - 0.5) * spread.to_radians();
    let dir = (target_pos - origin).normalize_or_zero();
    let rotated_dir = Vec2::new(
        dir.x * angle_offset.cos() - dir.y * angle_offset.sin(),
        dir.x * angle_offset.sin() + dir.y * angle_offset.cos(),
    );
    let dist = origin.distance(target_pos);
    origin + rotated_dir * dist
}

/// Checks whether a target direction falls within a weapon's firing arc
pub(crate) fn is_in_firing_arc(
    ship_rotation: f32,
    module_rotation: &Rotation,
    mount: &WeaponMount,
    direction_to_target: Vec2,
) -> bool {
    use std::f32::consts::FRAC_PI_2;
    match mount.mount_type {
        MountType::Turret => true,
        MountType::Fixed => {
            let module_angle = ship_rotation + module_rotation.to_radians();
            let weapon_forward = Vec2::new(module_angle.cos(), module_angle.sin());
            let dot = weapon_forward.dot(direction_to_target.normalize_or_zero());
            dot >= (mount.firing_arc / 2.0).to_radians().cos()
        }
        MountType::Broadside => {
            let perp = Vec2::new(
                (ship_rotation + FRAC_PI_2).cos(),
                (ship_rotation + FRAC_PI_2).sin(),
            );
            let dot = perp.dot(direction_to_target.normalize_or_zero()).abs();
            dot >= (mount.firing_arc / 2.0).to_radians().cos()
        }
    }
}

/// Clamps an aim direction into a mount's firing arc. Turrets pass the aim
/// through; fixed/broadside mounts return the nearest arc-edge direction
/// when the aim falls outside their cone. Used so launchers never silently
/// refuse to fire — an off-axis salvo is visible feedback that the mount
/// points elsewhere, a gun that eats the trigger press reads as broken.
pub(crate) fn clamp_to_firing_arc(
    ship_rotation: f32,
    module_rotation: &Rotation,
    mount: &WeaponMount,
    aim_dir: Vec2,
) -> Vec2 {
    use std::f32::consts::FRAC_PI_2;
    let forward = match mount.mount_type {
        MountType::Turret => return aim_dir,
        MountType::Fixed => {
            let angle = ship_rotation + module_rotation.to_radians();
            Vec2::new(angle.cos(), angle.sin())
        }
        MountType::Broadside => {
            // Two mirrored cones — clamp against the side nearer the aim
            let angle = ship_rotation + FRAC_PI_2;
            let side = Vec2::new(angle.cos(), angle.sin());
            if side.dot(aim_dir) >= 0.0 { side } else { -side }
        }
    };
    let half_arc = (mount.firing_arc / 2.0).to_radians();
    let offset = forward.angle_to(aim_dir);
    if offset.abs() <= half_arc {
        aim_dir
    } else {
        Vec2::from_angle(offset.clamp(-half_arc, half_arc)).rotate(forward)
    }
}

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<targeting::TargetSelection>()
            .init_resource::<targeting::AimLock>()
            .init_resource::<targeting::FireGroupState>()
            .init_resource::<recoil::RecoilAccumulator>()
            .configure_sets(Update, CombatSet::WeaponFire.after(SpatialSet::Update).run_if(in_state(GameState::Exploring)))
            .configure_sets(Update, CombatSet::Cleanup.after(CombatSet::WeaponFire).run_if(in_state(GameState::Exploring)))
            // Target selection + fire groups (always during exploring).
            // cycle_target (Tab) intentionally NOT registered — Tab is the
            // radar key, and having it also lock onto the nearest enemy was
            // an unwanted side effect. Guns free-aim at the cursor by default;
            // middle-click still opt-in locks a target for auto-aim + bracket.
            .add_systems(Update, (
                targeting::click_select_target,
                targeting::draw_target_bracket,
                // Lock input runs before the radial menu can claim the click,
                // and before fire_group_input so a lock made this frame fires
                // this frame.
                targeting::aim_lock_input
                    .before(crate::ui::windows::radial_menu::spawn_radial_on_right_click),
                targeting::maintain_aim_lock.after(targeting::aim_lock_input),
                targeting::draw_aim_lock.after(targeting::maintain_aim_lock),
                targeting::fire_group_input.after(targeting::maintain_aim_lock),
                // `\` picks which enemy the unlocked battery works on, and
                // assign_auto_aim then hands each gun its own block on it.
                // Ordered before the firing set so a switch takes effect on
                // the frame it was pressed.
                targeting::cycle_target,
                targeting::assign_auto_aim
                    .after(targeting::cycle_target)
                    .after(targeting::maintain_aim_lock)
                    .before(CombatSet::WeaponFire),
            ).run_if(in_state(GameState::Exploring)))
            // Shields: attach to player + AI ships, recharge, drive bubble visuals
            .add_systems(Update, (
                shields::attach_player_shield,
                shields::refresh_player_shield_skin,
                shields::refresh_player_shield_capacity.before(shields::update_shields),
                shields::attach_ai_shields,
                shields::refresh_shield_extents.before(shields::update_shields),
                shields::toggle_player_shield,
                shields::update_player_shield_arc,
                shields::update_ai_shield_arcs,
                shields::update_shield_segment,
                shields::update_shields,
                turrets::aim_turrets,
            ).run_if(in_state(GameState::Exploring)))
            // Player weapons: kinetic projectiles + missiles (new physics system)
            .add_systems(Update, (
                new_projectiles::fire_weapons_system,
                new_projectiles::move_projectiles,
                new_projectiles::check_projectile_hits,
                missiles::fire_missiles_system,
                missiles::move_missiles,
                missiles::check_missile_hits,
                point_defense::intercept_missiles,
                point_defense::pd_missile_collision,
            ).in_set(CombatSet::WeaponFire))
            // Creature/AI weapons: use original projectile system (different entity type)
            .add_systems(Update, (
                effects::creature_ranged_attack,
                projectiles::projectile_movement,
                projectiles::projectile_collision,
            ).in_set(CombatSet::WeaponFire))
            // Cleanup + limits
            .add_systems(Update, (
                effects::despawn_dead_creatures,
                effects::animate_floating_damage,
                crate::ship::damage::cleanup_hit_effects,
                limits::enforce_projectile_limit,
                new_projectiles::tick_burning_blocks,
            ).in_set(CombatSet::Cleanup))
            // Fire group assignment (build mode)
            // Severance + chain reactions
            .add_systems(
                Update,
                (
                    severance::check_section_severance,
                    severance::move_detached_sections,
                    severance::debris_collision,
                    chain_reactions::trigger_chain_reactions,
                ).run_if(in_state(GameState::Exploring)),
            )
            // Combat features: heat glow, damage arrows, weak points, boarding
            .add_systems(
                Update,
                (
                    combat_features::weapon_heat_visual,
                    combat_features::spawn_damage_indicators,
                    combat_features::update_damage_indicators,
                    combat_features::attach_weak_points,
                    combat_features::update_weak_point_visuals,
                    combat_features::parasite_boarding,
                    combat_features::boarded_parasite_damage,
                    combat_features::crew_fights_boarders,
                ).run_if(in_state(GameState::Exploring)),
            )
            // Recoil
            .add_systems(
                Update,
                (
                    recoil::accumulate_projectile_recoil,
                    recoil::accumulate_missile_recoil,
                    recoil::apply_weapon_recoil
                        .after(recoil::accumulate_projectile_recoil)
                        .after(recoil::accumulate_missile_recoil),
                ).run_if(in_state(GameState::Exploring)),
            )
            // Energy weapons
            .add_systems(
                Update,
                (
                    energy_weapons::fire_laser_system,
                    energy_weapons::fire_ion_system,
                    energy_weapons::update_ion_pulses,
                    energy_weapons::update_ion_disabled,
                    energy_weapons::fire_plasma_system,
                    energy_weapons::fire_emp_missiles,
                    energy_weapons::emp_detonation,
                ).run_if(in_state(GameState::Exploring)),
            )
            // Build mode combat tools
            .add_systems(
                Update,
                (
                    targeting::assign_fire_group,
                    point_defense::toggle_intercept_mode,
                ).run_if(in_state(GameState::StationDocked)),
            );
    }
}
