use bevy::prelude::*;
use crate::celestial::components::{GravityAffected, GravityForce};
use super::targeting::{TargetSelection, FireGroupState, FireGroup, lead_prediction::*};
use super::*;

// ============================================================================
// NEW PROJECTILE SYSTEM
// Every projectile is a real entity: position, velocity, gravity-affected.
// Kinetic rounds have penetration. Angled hits ricochet.
// ============================================================================

/// A projectile entity
#[derive(Component)]
pub struct Projectile {
    pub damage: f32,
    pub speed: f32,
    pub lifetime: f32,
    pub max_lifetime: f32,
    pub owner: Entity,
    pub damage_type: ProjectileDamageType,
    pub penetration: f32,     // How much armor it can go through
    pub has_penetrated: bool, // Already went through one layer
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProjectileDamageType {
    Kinetic,        // AP rounds — penetrates
    Explosive,      // HE rounds — area on impact
    Incendiary,     // Sets fires
    EmpRound,       // Disables modules
}

/// Missile entity — has guidance and fuel
#[derive(Component)]
pub struct MissileProjectile {
    pub damage: f32,
    pub target: Option<Entity>,
    pub burn_fuel: f32,        // Fuel for main engine
    pub reserve_fuel: f32,     // Fuel for course corrections
    pub thrust: f32,
    pub tracking_agility: f32, // How fast it can turn (rad/s)
    pub armed: bool,           // Needs to travel min distance before arming
    pub arm_distance: f32,
    pub traveled: f32,
    pub blast_radius: f32,
    pub owner: Entity,
}

/// Visual trail marker for projectiles
#[derive(Component)]
pub struct ProjectileTrail {
    pub color: Color,
    pub width: f32,
    pub fade_time: f32,
}

// ============================================================================
// WEAPON FIRING SYSTEM — uses fire groups + lead prediction
// ============================================================================

/// Main weapon firing system: reads fire groups, aims with lead prediction, spawns projectiles
pub fn fire_weapons_system(
    time: Res<Time>,
    fire_state: Res<FireGroupState>,
    selection: Res<TargetSelection>,
    sub_query: Query<(&Transform, &SubmarinePhysics, &Velocity), With<Submarine>>,
    mut weapon_query: Query<(
        Entity, &Module, &mut Weapon, &mut WeaponCooldown,
        &GlobalTransform, &FireGroup,
        Option<&crate::building::customization::parameters::ModuleCustomization>,
    ), Without<DestroyedModule>>,
    target_transform_query: Query<&Transform, Without<Submarine>>,
    target_velocity_query: Query<&Velocity, Without<Submarine>>,
    targeting_computer_query: Query<&Module, Without<DestroyedModule>>,
    mut commands: Commands,
) {
    let Ok((_sub_transform, _sub_physics, sub_velocity)) = sub_query.get_single() else { return };
    let _dt = time.delta_seconds();

    // Build module position list for adjacency checks
    let all_modules: Vec<(IVec2, ModuleType, bool)> = targeting_computer_query.iter()
        .map(|m| (m.grid_position, m.module_type, m.is_active))
        .collect();

    for (entity, module, mut weapon, mut cooldown, global_transform, fire_group, customization) in weapon_query.iter_mut() {
        if !module.is_active { continue; }

        // Tick cooldown
        cooldown.timer.tick(time.delta());
        if !cooldown.timer.finished() { continue; }

        // Check if this weapon's fire group is active
        let group_firing = fire_state.firing[fire_group.group as usize % 4];
        if !group_firing { continue; }

        // Need a target for auto-fire weapons
        let Some(target_entity) = selection.target else { continue; };
        let Ok(target_transform) = target_transform_query.get(target_entity) else { continue; };

        // Check ammo
        if weapon.ammo <= 0 { continue; }

        // Check range
        let weapon_pos = global_transform.translation().truncate();
        let target_pos = target_transform.translation.truncate();
        let distance = weapon_pos.distance(target_pos);
        if distance > weapon.range { continue; }

        // Get target velocity for lead prediction
        let target_vel = target_velocity_query.get(target_entity)
            .map(|v| v.0)
            .unwrap_or(Vec2::ZERO);

        // Determine prediction tier — Targeting Computer must be ADJACENT to this weapon
        let has_adjacent_tc = crate::combat::targeting::lead_prediction::check_adjacent_targeting_computer(
            module.grid_position, &all_modules,
        );
        let tier = get_weapon_prediction_tier(module, customization, has_adjacent_tc);

        // Calculate projectile speed based on weapon type
        let proj_speed = match module.module_type {
            ModuleType::Railgun => 1200.0,    // Very fast
            ModuleType::Coilgun => 800.0,     // Fast
            ModuleType::Cannon => 600.0,      // Medium
            ModuleType::Gatling => 500.0,     // Medium-slow
            _ => 600.0,
        };

        // Get shooter velocity for relative prediction
        let shooter_vel = sub_velocity.0;

        // Calculate lead — accounts for shooter velocity, degrades with distance
        let prediction = calculate_lead(
            weapon_pos,
            shooter_vel,
            target_pos,
            target_vel,
            Vec2::ZERO,
            proj_speed,
            tier,
            weapon.range,
        );

        // Apply accuracy spread — worse at longer range
        let aim_point = apply_accuracy_spread(
            weapon_pos,
            prediction.aim_point,
            prediction.distance_accuracy * 0.85, // Distance degrades accuracy
            10.0, // Max spread degrees at worst accuracy
        );

        // Fire!
        let direction = (aim_point - weapon_pos).normalize_or_zero();
        cooldown.timer.reset();
        weapon.ammo = weapon.ammo.saturating_sub(1);

        // Determine damage type based on module type
        let damage_type = match module.module_type {
            ModuleType::Railgun => ProjectileDamageType::Kinetic,
            ModuleType::Cannon => ProjectileDamageType::Kinetic,
            ModuleType::Coilgun => ProjectileDamageType::Kinetic,
            ModuleType::Gatling => ProjectileDamageType::Kinetic,
            _ => ProjectileDamageType::Kinetic,
        };

        // Spawn projectile(s)
        let burst_count = match module.module_type {
            ModuleType::Coilgun => 3,  // 3-round burst
            ModuleType::Gatling => 1,  // Continuous stream (high fire rate handles it)
            _ => 1,
        };

        for _i in 0..burst_count {
            let spread_offset = if burst_count > 1 {
                Vec2::new(
                    (rand::random::<f32>() - 0.5) * 3.0,
                    (rand::random::<f32>() - 0.5) * 3.0,
                )
            } else {
                Vec2::ZERO
            };

            let vel = direction * proj_speed + spread_offset;

            // Visual size based on weapon
            let (size, color) = match module.module_type {
                ModuleType::Railgun => (Vec2::new(16.0, 3.0), Color::rgb(0.5, 0.6, 1.0)),
                ModuleType::Cannon => (Vec2::new(10.0, 4.0), Color::rgb(0.9, 0.7, 0.3)),
                ModuleType::Coilgun => (Vec2::new(8.0, 3.0), Color::rgb(0.6, 0.7, 0.9)),
                ModuleType::Gatling => (Vec2::new(6.0, 2.0), Color::rgb(1.0, 0.8, 0.3)),
                _ => (Vec2::new(8.0, 3.0), Color::rgb(0.8, 0.8, 0.4)),
            };

            let angle = vel.y.atan2(vel.x);

            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color,
                        custom_size: Some(size),
                        ..default()
                    },
                    transform: Transform {
                        translation: Vec3::new(weapon_pos.x, weapon_pos.y, 0.5),
                        rotation: Quat::from_rotation_z(angle),
                        ..default()
                    },
                    ..default()
                },
                Projectile {
                    damage: weapon.damage,
                    speed: proj_speed,
                    lifetime: 4.0,
                    max_lifetime: 4.0,
                    owner: entity,
                    damage_type,
                    penetration: match module.module_type {
                        ModuleType::Railgun => 80.0,   // Goes through almost anything
                        ModuleType::Cannon => 40.0,    // Decent penetration
                        ModuleType::Coilgun => 25.0,   // Light penetration
                        ModuleType::Gatling => 10.0,   // Barely penetrates
                        _ => 20.0,
                    },
                    has_penetrated: false,
                },
                Velocity(vel),
                GravityAffected { mass: 0.5 }, // Projectiles affected by gravity (slightly)
                GravityForce::default(),
            ));
        }

        // Muzzle flash
        let flash_color = match module.module_type {
            ModuleType::Railgun => Color::rgb(0.5, 0.6, 1.0),
            ModuleType::Cannon => Color::rgb(0.9, 0.7, 0.3),
            ModuleType::Coilgun => Color::rgb(0.6, 0.7, 0.9),
            ModuleType::Gatling => Color::rgb(1.0, 0.8, 0.3),
            _ => Color::rgb(0.8, 0.8, 0.4),
        };
        spawn_hit_effect(&mut commands, weapon_pos + direction * 30.0, flash_color, 12.0);
    }
}

// ============================================================================
// PROJECTILE MOVEMENT — gravity-affected, lifetime-limited
// ============================================================================

/// Move projectiles, apply gravity, tick lifetime, despawn expired
pub fn move_projectiles(
    time: Res<Time>,
    mut commands: Commands,
    mut proj_query: Query<(Entity, &mut Projectile, &mut Transform, &mut Velocity, &GravityForce)>,
) {
    let dt = time.delta_seconds();

    for (entity, mut proj, mut transform, mut velocity, gravity) in proj_query.iter_mut() {
        // Apply gravity to velocity
        velocity.0 += gravity.0 * dt * 0.5; // Projectiles resist gravity more than ships

        // Move
        transform.translation.x += velocity.0.x * dt;
        transform.translation.y += velocity.0.y * dt;

        // Rotate to face movement direction
        if velocity.0.length_squared() > 1.0 {
            let angle = velocity.0.y.atan2(velocity.0.x);
            transform.rotation = Quat::from_rotation_z(angle);
        }

        // Age
        proj.lifetime -= dt;
        if proj.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

// ============================================================================
// PROJECTILE COLLISION — damage on hit, penetration, ricochet
// ============================================================================

/// Check projectile collisions with creatures and ships
pub fn check_projectile_hits(
    mut commands: Commands,
    proj_query: Query<(Entity, &Projectile, &Transform, &Velocity)>,
    mut creature_query: Query<(Entity, &Transform, &mut Creature), Without<Submarine>>,
    _notifications: EventWriter<ShowNotification>,
) {
    for (proj_entity, proj, proj_transform, _proj_vel) in proj_query.iter() {
        let proj_pos = proj_transform.translation.truncate();

        for (_creature_entity, creature_transform, mut creature) in creature_query.iter_mut() {
            if creature.health <= 0.0 { continue; }

            let creature_pos = creature_transform.translation.truncate();
            let hit_radius = match creature.creature_type {
                CreatureType::Leviathan => 90.0,
                CreatureType::Stalker => 30.0,
                CreatureType::ParasiteSwarm => 15.0,
                CreatureType::VoidDrifter => 12.0,
            };

            let dist = proj_pos.distance(creature_pos);
            if dist > hit_radius { continue; }

            // HIT!
            creature.health -= proj.damage;

            // Impact spark
            spawn_hit_effect(&mut commands, proj_pos, Color::rgb(1.0, 0.8, 0.3), 8.0);
            spawn_floating_damage(&mut commands, proj_pos, proj.damage, Color::rgb(1.0, 0.4, 0.2));

            // Despawn projectile (unless it penetrates)
            if proj.penetration < 30.0 || proj.has_penetrated {
                commands.entity(proj_entity).despawn();
            }
            // High penetration projectiles continue through

            break; // One hit per frame per projectile
        }
    }
}
