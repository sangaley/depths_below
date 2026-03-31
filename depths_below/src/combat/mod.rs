use bevy::prelude::*;
use crate::components::*;
use crate::resources::*;
use crate::events::*;
use crate::states::{GameState, CombatSet};

// Re-use the public HitEffect from submarine::damage
use crate::submarine::damage::HitEffect;

mod weapons;
pub(crate) mod projectiles;
mod mines;
mod effects;
pub mod targeting;
pub mod new_projectiles;
pub mod missiles;
pub mod point_defense;
pub mod severance;
pub mod chain_reactions;
pub mod combat_features;
pub mod energy_weapons;
pub mod ammo_types;

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

/// Projectile speed base
pub(crate) const PROJECTILE_SPEED: f32 = 600.0;
/// Projectile collision radius
pub(crate) const PROJECTILE_RADIUS: f32 = 12.0;
/// Creature collision radius
pub(crate) const CREATURE_RADIUS: f32 = 24.0;
/// Submarine collision radius (for enemy projectiles)
pub(crate) const SUBMARINE_RADIUS: f32 = 60.0;

/// Spawn a visual hit-flash sprite at the given position.
pub(crate) fn spawn_hit_effect(commands: &mut Commands, position: Vec2, color: Color, size: f32) {
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color,
                custom_size: Some(Vec2::splat(size)),
                ..default()
            },
            transform: Transform::from_xyz(position.x, position.y, 0.6),
            ..default()
        },
        HitEffect {
            timer: Timer::from_seconds(0.2, TimerMode::Once),
        },
    ));
}

/// Spawn a floating damage number that drifts upward and fades out.
pub(crate) fn spawn_floating_damage(commands: &mut Commands, position: Vec2, damage: f32, color: Color) {
    commands.spawn((
        Text2dBundle {
            text: Text::from_section(
                format!("-{}", damage as i32),
                TextStyle {
                    font_size: 18.0,
                    color,
                    ..default()
                },
            ),
            transform: Transform::from_xyz(position.x, position.y + 20.0, 1.0),
            ..default()
        },
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
    sub_rotation: f32,
    module_rotation: &Rotation,
    mount: &WeaponMount,
    direction_to_target: Vec2,
) -> bool {
    use std::f32::consts::FRAC_PI_2;
    match mount.mount_type {
        MountType::Turret => true,
        MountType::Fixed => {
            let module_angle = sub_rotation + module_rotation.to_radians();
            let weapon_forward = Vec2::new(module_angle.cos(), module_angle.sin());
            let dot = weapon_forward.dot(direction_to_target.normalize_or_zero());
            dot >= (mount.firing_arc / 2.0).to_radians().cos()
        }
        MountType::Broadside => {
            let perp = Vec2::new(
                (sub_rotation + FRAC_PI_2).cos(),
                (sub_rotation + FRAC_PI_2).sin(),
            );
            let dot = perp.dot(direction_to_target.normalize_or_zero()).abs();
            dot >= (mount.firing_arc / 2.0).to_radians().cos()
        }
    }
}

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<targeting::TargetSelection>()
            .init_resource::<targeting::FireGroupState>()
            .configure_set(Update, CombatSet::WeaponFire.run_if(in_state(GameState::Exploring)))
            .configure_set(Update, CombatSet::Cleanup.after(CombatSet::WeaponFire).run_if(in_state(GameState::Exploring)))
            // Target selection + fire groups (always during exploring)
            .add_systems(Update, (
                targeting::cycle_target,
                targeting::click_select_target,
                targeting::draw_target_bracket,
                targeting::fire_group_input,
            ).run_if(in_state(GameState::Exploring)))
            // New combat systems
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
            // Legacy weapon systems
            .add_systems(Update, (
                weapons::crew_weapon_system,
                weapons::manual_weapon_system,
                effects::creature_ranged_attack,
                projectiles::projectile_movement,
                projectiles::projectile_collision,
                mines::mine_system,
                mines::mine_explosion_system,
            ).in_set(CombatSet::WeaponFire))
            // Cleanup
            .add_systems(Update, (
                effects::despawn_dead_creatures,
                effects::animate_floating_damage,
                crate::submarine::damage::cleanup_hit_effects,
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
            // Energy weapons
            .add_systems(
                Update,
                (
                    energy_weapons::fire_laser_system,
                    energy_weapons::fire_ion_system,
                    energy_weapons::update_ion_pulses,
                    energy_weapons::update_ion_disabled,
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
