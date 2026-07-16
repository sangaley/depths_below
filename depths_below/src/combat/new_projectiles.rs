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
    /// Loaded round type — drives on-hit behavior (blast, EMP, burn, ...).
    /// None for AI shots and legacy paths → plain single-block damage.
    pub ammo: Option<crate::combat::ammo_types::KineticAmmoType>,
    /// Caliber scale of the firing weapon — shrinks/grows the ammo's on-hit
    /// EFFECTS (blast radius, EMP duration, burn time). Damage numbers
    /// already scale through proj.damage; without this a gatling EMP round
    /// disabled as wide and as long as a cannon's, at 10x the fire rate.
    pub caliber: f32,
    /// Block already damaged by this round — a penetrator passing through a
    /// block is still inside its hit radius next frame; without this it
    /// would hit the same block twice instead of the one behind it.
    pub last_hit: Option<Entity>,
}

/// A block set on fire by incendiary rounds — ticks damage until it burns out.
#[derive(Component)]
pub struct BlockBurning {
    pub dps: f32,
    pub remaining: f32,
    /// Owning AI ship — burn ticks report here so aggregate hull integrity
    /// (process_ai_ship_damage_system) stays in sync.
    pub ship: Entity,
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
    power_state: Res<crate::resources::PowerState>,
    selection: Res<TargetSelection>,
    ship_query: Query<(Entity, &Transform, &ShipPhysics, &Velocity), With<Ship>>,
    mut weapon_query: Query<(
        Entity, &Module, &mut Weapon, &mut WeaponCooldown,
        &GlobalTransform, &FireGroup, &WeaponMount, &ChildOf,
        Option<&crate::building::customization::parameters::ModuleCustomization>,
        Option<&crate::building::customization::tuning::WeaponTuning>,
        Option<&crate::building::customization::tuning::SelectedAmmo>,
        Option<&ModuleTemperature>,
    ), Without<DestroyedModule>>,
    target_transform_query: Query<&Transform, Without<Ship>>,
    target_velocity_query: Query<&Velocity, Without<Ship>>,
    targeting_computer_query: Query<&Module, Without<DestroyedModule>>,
    windows_query: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    input_state: Res<crate::resources::InputState>,
    mut fired_events: MessageWriter<crate::events::WeaponFired>,
    mut commands: Commands,
) {
    let Ok((player_ship, ship_transform, ship_physics, ship_velocity)) = ship_query.single() else { return };
    let _dt = time.delta_secs();

    // Weapons need power: a grid in deficit (e.g. shield surging under fire)
    // silences the guns until the balance recovers.
    if power_state.power_balance < 0.0 {
        return;
    }

    // Cursor world position — dumb-fire fallback when no target is selected.
    let cursor_world: Option<Vec2> = windows_query.single().ok()
        .and_then(|w| w.cursor_position())
        .and_then(|c| {
            camera_query.single().ok()
                .and_then(|(cam, gt)| cam.viewport_to_world_2d(gt, c).ok())
        });
    // Controller right-stick aim beats the mouse while it owns aim (see
    // InputState.gamepad_aim): dumb-fire at a point projected out along
    // the stick direction.
    let cursor_world = input_state.gamepad_aim
        .map(|dir| ship_transform.translation.truncate() + dir * 2000.0)
        .or(cursor_world);

    // Build module position list for adjacency checks
    let all_modules: Vec<(IVec2, ModuleType, bool)> = targeting_computer_query.iter()
        .map(|m| (m.grid_position, m.module_type, m.is_active))
        .collect();

    for (entity, module, mut weapon, mut cooldown, global_transform, fire_group, mount, parent, customization, tuning, selected_ammo, temp) in weapon_query.iter_mut() {
        // Player ship only: this query has no ownership filter on its own, and
        // AI ships carry the exact same Weapon/FireGroup/WeaponMount
        // components (shared spawn_module path). Unscoped, holding Space
        // also fired every AI ship's default-group weapon at the player's
        // current target/cursor — including an AI ship shooting itself when
        // the player had it targeted.
        if parent.parent() != player_ship { continue; }
        // True kinetic weapons only. This loop has no type match arms below
        // for anything else — every OTHER weapon type (Laser, PlasmaCaster,
        // IonDisruptor, EMPPulse, the three missile types, MiningDrill,
        // TractorBeam) fell through the `_ =>` wildcard arms and phantom-fired
        // a generic small yellow bullet from this system IN ADDITION to
        // whatever their own dedicated firing system did (or, for
        // PlasmaCaster, didn't — it has no dedicated system at all). That
        // stray shot is what made the laser look broken: the real beam
        // can't hit ships (separate bug), but this phantom bullet could,
        // muddying what was actually happening.
        if !matches!(module.module_type,
            ModuleType::Cannon | ModuleType::Railgun | ModuleType::Coilgun | ModuleType::Gatling
        ) {
            continue;
        }
        if !module.is_active { continue; }

        // Tick cooldown BEFORE the thermal gate. Gating first froze the
        // timer while hot — and generate_heat treats a running cooldown as
        // "recently fired", so a hot gun kept generating heat forever and
        // never came back (one burst → permanently stuck red).
        cooldown.timer.tick(time.delta());
        if !cooldown.timer.is_finished() { continue; }

        // Thermal throttle — same gate the laser uses. Overtuned guns heat
        // past this under sustained fire and stutter until they cool.
        if let Some(temp) = temp {
            if temp.current >= temp.max_temp * 0.95 { continue; }
        }

        // Check if this weapon's fire group is active
        let group_firing = fire_state.firing[fire_group.group as usize % 4];
        if !group_firing { continue; }

        // Check ammo
        if !crate::combat::INFINITE_AMMO && weapon.ammo <= 0 { continue; }

        let weapon_pos = global_transform.translation().truncate();

        // Aim at the selected target if there is one; otherwise dumb-fire at
        // the cursor. Never silently skip on range — an out-of-range target
        // just means the shot is capped at max range and falls short, which
        // is visible feedback instead of a gun that refuses to fire.
        let (target_pos, target_vel) = if let Some(target_entity) = selection.target {
            let Ok(target_transform) = target_transform_query.get(target_entity) else { continue };
            let mut target_pos = target_transform.translation.truncate();
            let to_target = target_pos - weapon_pos;
            if to_target.length() > weapon.range {
                target_pos = weapon_pos + to_target.normalize_or_zero() * weapon.range;
            }
            let vel = target_velocity_query.get(target_entity)
                .map(|v| v.0)
                .unwrap_or(Vec2::ZERO);
            (target_pos, vel)
        } else if let Some(cursor) = cursor_world {
            // Cap the aim point to weapon range along the cursor direction
            let to_cursor = cursor - weapon_pos;
            let aim = if to_cursor.length() > weapon.range {
                weapon_pos + to_cursor.normalize_or_zero() * weapon.range
            } else {
                cursor
            };
            (aim, Vec2::ZERO)
        } else {
            continue;
        };

        // Fixed/broadside mounts can't swivel outside their arc — without this
        // check a forward-facing cannon would fire at a target behind the
        // ship, sending the shot straight through the hull it's mounted on.
        if !is_in_firing_arc(ship_physics.rotation, &module.rotation, mount, target_pos - weapon_pos) {
            continue;
        }

        // Determine prediction tier — Targeting Computer must be ADJACENT to this weapon
        let has_adjacent_tc = crate::combat::targeting::lead_prediction::check_adjacent_targeting_computer(
            module.grid_position, &all_modules,
        );
        let tier = get_weapon_prediction_tier(module, customization, has_adjacent_tc);

        // Muzzle speed: per-type base (see tuning.rs — shared with the tuning
        // window's live readout) scaled by the velocity slider and the loaded
        // ammo's own velocity profile (APFSDS darts fly, HEAT crawls).
        let tuning_vel = tuning.map(|t| t.velocity).unwrap_or(1.0);
        let ammo_vel = selected_ammo.map(|a| a.0.velocity_mult()).unwrap_or(1.0);
        let proj_speed =
            crate::building::customization::tuning::base_projectile_speed(module.module_type)
            * tuning_vel * ammo_vel;

        // Get shooter velocity for relative prediction
        let shooter_vel = ship_velocity.0;

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
        if !crate::combat::INFINITE_AMMO {
            weapon.ammo = weapon.ammo.saturating_sub(1);
        }
        fired_events.write(crate::events::WeaponFired {
            weapon_type: module.module_type,
            position: weapon_pos,
            from_player: true,
        });

        // Loaded ammo drives damage type, per-round damage, and penetration.
        // Without a SelectedAmmo (AI ships, pre-tuning saves) everything
        // falls back to the old per-weapon-type behavior.
        use crate::combat::ammo_types::KineticAmmoType;
        let (damage_type, ammo_damage_mult, penetration) = match selected_ammo.map(|a| a.0) {
            Some(ammo) => (
                match ammo {
                    KineticAmmoType::Incendiary => ProjectileDamageType::Incendiary,
                    KineticAmmoType::EMPShell => ProjectileDamageType::EmpRound,
                    KineticAmmoType::APHE | KineticAmmoType::HEFrag | KineticAmmoType::Flak =>
                        ProjectileDamageType::Explosive,
                    _ => ProjectileDamageType::Kinetic,
                },
                ammo.damage_mult(),
                ammo.penetration(),
            ),
            None => (
                ProjectileDamageType::Kinetic,
                1.0,
                match module.module_type {
                    ModuleType::Railgun => 80.0,   // Goes through almost anything
                    ModuleType::Cannon => 40.0,    // Decent penetration
                    ModuleType::Coilgun => 25.0,   // Light penetration
                    ModuleType::Gatling => 10.0,   // Barely penetrates
                    _ => 20.0,
                },
            ),
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

            // Projectiles inherit the ship's own velocity — calculate_lead's
            // aim point assumes this (it solves using target_vel - shooter_vel
            // as the relative velocity, which only converges to a correct
            // intercept if the shot's world-frame speed is proj_speed +
            // shooter_vel). Without this, aiming while moving fast dragged
            // the computed aim point back toward the ship itself, collapsing
            // the shot's direction to near-zero — kinetic rounds barely
            // moved while sustained-thrusting toward a target.
            let vel = direction * proj_speed + shooter_vel + spread_offset;

            // Visual size/color based on weapon — sized and colored to read
            // clearly at gameplay zoom instead of every shot looking like
            // the same small yellow sliver.
            let (size, base_color) = match module.module_type {
                ModuleType::Railgun => (Vec2::new(50.0, 4.0), Color::srgb(0.2, 0.5, 1.0)),   // long blue streak
                ModuleType::Cannon => (Vec2::new(20.0, 12.0), Color::srgb(1.0, 0.45, 0.05)), // big orange shell
                ModuleType::Coilgun => (Vec2::new(12.0, 5.0), Color::srgb(0.6, 0.8, 1.0)),
                ModuleType::Gatling => (Vec2::new(8.0, 3.0), Color::srgb(1.0, 0.85, 0.2)),
                _ => (Vec2::new(8.0, 3.0), Color::srgb(0.8, 0.8, 0.4)),
            };
            // Loaded ammo recolors the round (AP brass, EMP blue, ...) so a
            // mixed loadout reads at a glance; size stays per-weapon.
            let color = selected_ammo.map(|a| a.0.color()).unwrap_or(base_color);

            let angle = vel.y.atan2(vel.x);

            commands.spawn((
                (Sprite {
                        color,
                        custom_size: Some(size),
                        ..default()
                    }, Transform {
                        translation: Vec3::new(weapon_pos.x, weapon_pos.y, 0.5),
                        rotation: Quat::from_rotation_z(angle),
                        ..default()
                    }),
                Projectile {
                    damage: weapon.damage * ammo_damage_mult,
                    speed: proj_speed,
                    lifetime: 4.0,
                    max_lifetime: 4.0,
                    owner: entity,
                    damage_type,
                    penetration,
                    has_penetrated: false,
                    ammo: selected_ammo.map(|a| a.0),
                    caliber: caliber_scale(module.module_type),
                    last_hit: None,
                },
                Velocity(vel),
                GravityAffected { mass: 0.5 }, // Projectiles affected by gravity (slightly)
                GravityForce::default(),
            ));
        }

        // Muzzle flash
        let flash_color = match module.module_type {
            ModuleType::Railgun => Color::srgb(0.2, 0.5, 1.0),
            ModuleType::Cannon => Color::srgb(1.0, 0.45, 0.05),
            ModuleType::Coilgun => Color::srgb(0.6, 0.8, 1.0),
            ModuleType::Gatling => Color::srgb(1.0, 0.85, 0.2),
            _ => Color::srgb(0.8, 0.8, 0.4),
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
    let dt = time.delta_secs();

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

/// Largest creature hit radius (Leviathan) — used to size the spatial grid query margin.
const MAX_CREATURE_HIT_RADIUS: f32 = 90.0;

/// Check projectile collisions with creatures and ships.
/// Uses the creature spatial grid to only distance-check creatures near each
/// projectile instead of every creature in the world.
pub fn check_projectile_hits(
    mut commands: Commands,
    mut proj_query: Query<(Entity, &mut Projectile, &Transform, &Velocity)>,
    mut creature_query: Query<(&Transform, &mut Creature), Without<Ship>>,
    creature_grid: Res<crate::spatial::CreatureGrid>,
    mut ai_ship_query: Query<
        (Entity, &Transform, &Children, &mut crate::combat::shields::ShipShield),
        With<crate::ai_ship::components::AiShip>,
    >,
    mut ai_module_query: Query<(&mut Module, &GlobalTransform), Without<DestroyedModule>>,
    mut ai_hull_query: Query<(&mut HullSegment, &GlobalTransform), Without<crate::components::HullDestroyed>>,
    owner_parent_query: Query<&ChildOf>,
    mut ai_damage_events: MessageWriter<crate::events::AiShipDamaged>,
    _notifications: MessageWriter<ShowNotification>,
) {
    'projectiles: for (proj_entity, mut proj, proj_transform, proj_vel) in proj_query.iter_mut() {
        let proj_pos = proj_transform.translation.truncate();
        // A weapon's own ship is never a valid target for its own shot,
        // regardless of aim — belt-and-suspenders on top of firing-arc and
        // per-ship query scoping.
        let owner_ship = owner_parent_query.get(proj.owner).ok().map(|p| p.parent());

        // === AI SHIPS: shield first, then block-by-block hull damage ===
        for (ai_entity, ai_transform, children, mut shield) in ai_ship_query.iter_mut() {
            if Some(ai_entity) == owner_ship { continue; }
            // Bubble is centered on the blocks' centroid, not the root
            let center = shield.world_center(ai_transform);
            let dist_to_ship = proj_pos.distance(center);

            // Shield bubble intercepts anything crossing its radius
            if shield.is_up() && dist_to_ship < shield.radius {
                shield.absorb(proj.damage);
                spawn_hit_effect(&mut commands, proj_pos, Color::srgb(0.5, 0.8, 1.0), 14.0);
                commands.entity(proj_entity).despawn();
                continue 'projectiles;
            }

            // Shield down: hit the nearest block within impact range — hull
            // segment or module, whichever is actually closer to the impact.
            // This is the single authoritative hit resolution for AI ships;
            // process_ai_ship_damage_system used to redo its own separate
            // "nearest hull segment on the whole ship, no radius limit"
            // search off the same event, which routinely landed on some
            // unrelated block far from the actual shot — the block you aimed
            // at wasn't the one that took damage. It now only recalculates
            // aggregate integrity from whatever this system already did.
            // Scan bound follows the ship's real extent (shield radius is
            // computed from it) — a fixed bound left long hulls unhittable.
            if dist_to_ship < shield.radius + 60.0 {
                // Skip proj.last_hit: a penetrator that just went through a
                // block is still inside its 45-unit radius next frame.
                let mut best_module: Option<(Entity, f32)> = None;
                for child in children.iter() {
                    if Some(child) == proj.last_hit { continue; }
                    if let Ok((_, gt)) = ai_module_query.get(child) {
                        let d = proj_pos.distance(gt.translation().truncate());
                        if d < 45.0 && best_module.map(|(_, bd)| d < bd).unwrap_or(true) {
                            best_module = Some((child, d));
                        }
                    }
                }
                let mut best_hull: Option<(Entity, f32)> = None;
                for child in children.iter() {
                    if Some(child) == proj.last_hit { continue; }
                    if let Ok((_, gt)) = ai_hull_query.get(child) {
                        let d = proj_pos.distance(gt.translation().truncate());
                        if d < 45.0 && best_hull.map(|(_, bd)| d < bd).unwrap_or(true) {
                            best_hull = Some((child, d));
                        }
                    }
                }

                let hit_module = matches!((best_module, best_hull), (Some((_, md)), Some((_, hd))) if md <= hd)
                    || (best_module.is_some() && best_hull.is_none());

                // Primary hit: damage the struck block, remember where.
                let primary: Option<(Entity, Vec2)> = if hit_module {
                    let target = best_module.unwrap().0;
                    ai_module_query.get_mut(target).ok().map(|(mut module, gt)| {
                        module.health = (module.health - proj.damage).max(0.0);
                        let hit_pos = gt.translation().truncate();
                        spawn_hit_effect(&mut commands, hit_pos, Color::srgb(1.0, 0.6, 0.2), 12.0);
                        spawn_floating_damage(&mut commands, hit_pos, proj.damage, Color::srgb(1.0, 0.8, 0.3));
                        (target, hit_pos)
                    })
                } else if let Some((hull_entity, _)) = best_hull {
                    ai_hull_query.get_mut(hull_entity).ok().map(|(mut hull, gt)| {
                        hull.health = (hull.health - proj.damage).max(0.0);
                        let hit_pos = gt.translation().truncate();
                        spawn_hit_effect(&mut commands, hit_pos, Color::srgb(1.0, 0.5, 0.2), 16.0);
                        spawn_floating_damage(&mut commands, hit_pos, proj.damage, Color::srgb(1.0, 0.3, 0.3));
                        (hull_entity, hit_pos)
                    })
                } else {
                    None
                };

                let Some((hit_entity, hit_pos)) = primary else { continue };

                // === AMMO ON-HIT BEHAVIOR — finally consumes the
                // AmmoHitBehavior table that ammo_types.rs has defined all
                // along. `penetrates` decides whether the round survives
                // this hit and flies on into the block behind.
                let mut penetrates = false;
                if let Some(ammo) = proj.ammo {
                    use crate::combat::ammo_types::AmmoHitBehavior::*;
                    match ammo.hit_behavior(proj.damage) {
                        Penetrate { damage_falloff, .. } => {
                            // AP/APFSDS: continue into the next block with
                            // reduced energy (one extra layer for now).
                            if !proj.has_penetrated {
                                penetrates = true;
                                proj.has_penetrated = true;
                                proj.last_hit = Some(hit_entity);
                                proj.damage *= 1.0 - damage_falloff;
                            }
                        }
                        PenetrateExplode { blast_damage, blast_radius, .. }
                        | SurfaceExplode { blast_damage, blast_radius, .. } => {
                            let radius = blast_radius * proj.caliber;
                            splash_blocks(
                                &mut commands, children, &mut ai_module_query, &mut ai_hull_query,
                                hit_entity, hit_pos, radius, blast_damage,
                            );
                            spawn_hit_effect(&mut commands, hit_pos, Color::srgb(1.0, 0.5, 0.1), radius);
                        }
                        ProximityBurst { fragment_damage, fragment_radius, .. } => {
                            let radius = fragment_radius * proj.caliber;
                            splash_blocks(
                                &mut commands, children, &mut ai_module_query, &mut ai_hull_query,
                                hit_entity, hit_pos, radius, fragment_damage,
                            );
                            spawn_hit_effect(&mut commands, hit_pos, Color::srgb(1.0, 0.9, 0.4), radius);
                        }
                        EMPDisable { disable_radius, disable_duration } => {
                            let radius = disable_radius * proj.caliber;
                            let duration = disable_duration * proj.caliber;
                            for child in children.iter() {
                                if let Ok((module, gt)) = ai_module_query.get(child) {
                                    if !module.is_active { continue; }
                                    if hit_pos.distance(gt.translation().truncate()) < radius {
                                        commands.entity(child).try_insert(
                                            crate::combat::energy_weapons::IonDisabled { timer: duration }
                                        );
                                    }
                                }
                            }
                            spawn_hit_effect(&mut commands, hit_pos, Color::srgb(0.4, 0.5, 0.95), radius);
                        }
                        Ignite { fire_duration, fire_intensity } => {
                            commands.entity(hit_entity).try_insert(BlockBurning {
                                // proj.damage already carries the incendiary's
                                // low direct-damage multiplier; the burn is
                                // where the real damage lives.
                                dps: proj.damage * fire_intensity,
                                remaining: fire_duration * proj.caliber,
                                ship: ai_entity,
                            });
                        }
                        Shockwave { shockwave_damage, shockwave_radius, .. } => {
                            // HESH: the block BEHIND the armor takes the spall,
                            // straight along the round's flight direction.
                            let dir = proj_vel.0.normalize_or_zero();
                            let behind = hit_pos + dir * 66.0 * shockwave_radius * 0.75;
                            let mut best: Option<(Entity, f32)> = None;
                            for child in children.iter() {
                                if child == hit_entity { continue; }
                                let block_pos = ai_module_query.get(child).map(|(_, gt)| gt.translation().truncate())
                                    .or_else(|_| ai_hull_query.get(child).map(|(_, gt)| gt.translation().truncate()));
                                if let Ok(block_pos) = block_pos {
                                    let d = behind.distance(block_pos);
                                    if d < 50.0 && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                                        best = Some((child, d));
                                    }
                                }
                            }
                            if let Some((spall_entity, _)) = best {
                                if let Ok((mut module, gt)) = ai_module_query.get_mut(spall_entity) {
                                    module.health = (module.health - shockwave_damage).max(0.0);
                                    spawn_floating_damage(&mut commands, gt.translation().truncate(), shockwave_damage, Color::srgb(0.9, 0.9, 0.5));
                                } else if let Ok((mut hull, gt)) = ai_hull_query.get_mut(spall_entity) {
                                    hull.health = (hull.health - shockwave_damage).max(0.0);
                                    spawn_floating_damage(&mut commands, gt.translation().truncate(), shockwave_damage, Color::srgb(0.9, 0.9, 0.5));
                                }
                            }
                        }
                        // HEAT: its 1.8× damage + 70 pen already rode in on
                        // proj.damage at spawn; the angle-sensitivity part of
                        // the shaped-charge fantasy needs hit normals the
                        // grid doesn't give us yet.
                        ShapedCharge { .. } => {}
                    }
                }

                ai_damage_events.write(crate::events::AiShipDamaged {
                    target: ai_entity,
                    source: crate::events::DamageSource::Explosion,
                    amount: 0.0, // damage already applied directly above — this is bookkeeping only
                    position: Some(hit_pos),
                    direction: None,
                });
                if !penetrates {
                    commands.entity(proj_entity).despawn();
                }
                continue 'projectiles;
            }
        }

        // Fragmenting rounds splash other creatures around the impact —
        // HE-Frag/Flak's whole identity ("great vs swarms") vs single-target AP.
        let creature_splash: Option<(f32, f32)> = proj.ammo.and_then(|ammo| {
            use crate::combat::ammo_types::AmmoHitBehavior::*;
            match ammo.hit_behavior(proj.damage) {
                SurfaceExplode { blast_radius, fragment_damage, .. } => Some((blast_radius * proj.caliber, fragment_damage)),
                ProximityBurst { fragment_radius, fragment_damage, .. } => Some((fragment_radius * proj.caliber, fragment_damage)),
                _ => None,
            }
        });

        for (creature_entity, _) in creature_grid.0.nearby(proj_pos, MAX_CREATURE_HIT_RADIUS) {
            let Ok((creature_transform, mut creature)) = creature_query.get_mut(creature_entity) else { continue };
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
            drop(creature);

            // Impact spark
            spawn_hit_effect(&mut commands, proj_pos, Color::srgb(1.0, 0.8, 0.3), 8.0);
            spawn_floating_damage(&mut commands, proj_pos, proj.damage, Color::srgb(1.0, 0.4, 0.2));

            // Fragment splash to everything else in the burst radius
            if let Some((radius, frag_damage)) = creature_splash {
                for (other_entity, _) in creature_grid.0.nearby(proj_pos, radius) {
                    if other_entity == creature_entity { continue; }
                    let Ok((other_transform, mut other)) = creature_query.get_mut(other_entity) else { continue };
                    if other.health <= 0.0 { continue; }
                    let other_pos = other_transform.translation.truncate();
                    if proj_pos.distance(other_pos) < radius {
                        other.health -= frag_damage;
                        spawn_floating_damage(&mut commands, other_pos, frag_damage, Color::srgb(1.0, 0.7, 0.3));
                    }
                }
                spawn_hit_effect(&mut commands, proj_pos, Color::srgb(1.0, 0.6, 0.15), radius);
            }

            // Despawn projectile (unless it penetrates)
            if proj.penetration < 30.0 || proj.has_penetrated {
                commands.entity(proj_entity).despawn();
            }
            // High penetration projectiles continue through

            break; // One hit per frame per projectile
        }
    }
}

/// How big a round each kinetic weapon actually throws — scales the ammo's
/// on-hit effects. A gatling firing APHE is a hail of small grenades; a
/// cannon firing it is a shell.
pub fn caliber_scale(module_type: ModuleType) -> f32 {
    match module_type {
        ModuleType::Gatling => 0.45,
        ModuleType::Coilgun => 0.75,
        ModuleType::Cannon => 1.0,
        ModuleType::Railgun => 1.25,
        _ => 1.0,
    }
}

/// Blast damage to every block within `radius` of the impact, except the
/// primary block (it already took the direct hit).
fn splash_blocks(
    commands: &mut Commands,
    children: &Children,
    module_query: &mut Query<(&mut Module, &GlobalTransform), Without<DestroyedModule>>,
    hull_query: &mut Query<(&mut HullSegment, &GlobalTransform), Without<crate::components::HullDestroyed>>,
    exclude: Entity,
    center: Vec2,
    radius: f32,
    damage: f32,
) {
    for child in children.iter() {
        if child == exclude { continue; }
        if let Ok((mut module, gt)) = module_query.get_mut(child) {
            let pos = gt.translation().truncate();
            if center.distance(pos) < radius {
                module.health = (module.health - damage).max(0.0);
                spawn_floating_damage(commands, pos, damage, Color::srgb(1.0, 0.55, 0.2));
            }
        } else if let Ok((mut hull, gt)) = hull_query.get_mut(child) {
            let pos = gt.translation().truncate();
            if center.distance(pos) < radius {
                hull.health = (hull.health - damage).max(0.0);
                spawn_floating_damage(commands, pos, damage, Color::srgb(1.0, 0.45, 0.25));
            }
        }
    }
}

/// Ticks incendiary burn on blocks: damage over time until the fire burns
/// out. Reports zero-amount AiShipDamaged so aggregate hull integrity
/// (process_ai_ship_damage_system) recalculates from the burned health.
pub fn tick_burning_blocks(
    time: Res<Time>,
    mut commands: Commands,
    mut burning_query: Query<(Entity, &mut BlockBurning, Option<&mut Module>, Option<&mut HullSegment>)>,
    mut ai_damage_events: MessageWriter<crate::events::AiShipDamaged>,
) {
    let dt = time.delta_secs();
    for (entity, mut burning, module, hull) in burning_query.iter_mut() {
        burning.remaining -= dt;
        let tick_damage = burning.dps * dt;
        if let Some(mut module) = module {
            module.health = (module.health - tick_damage).max(0.0);
        } else if let Some(mut hull) = hull {
            hull.health = (hull.health - tick_damage).max(0.0);
        }
        ai_damage_events.write(crate::events::AiShipDamaged {
            target: burning.ship,
            source: crate::events::DamageSource::Fire,
            amount: 0.0,
            position: None,
            direction: None,
        });
        if burning.remaining <= 0.0 {
            commands.entity(entity).remove::<BlockBurning>();
        }
    }
}
