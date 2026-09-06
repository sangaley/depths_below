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
    /// Where this round was at the END of the previous frame.
    ///
    /// Projectiles don't travel, they teleport: `move_projectiles` adds
    /// `velocity * dt` once per frame. A railgun does 9000 u/s = 150 units
    /// per frame at 60fps, and hit detection only looked at the round's
    /// CURRENT position with a 45-unit radius — a 90-unit window on a
    /// 150-unit stride. The 60-unit blind spot in between was never tested,
    /// and blocks are 66 units apart, so fast rounds flew straight through
    /// solid ships. Worse for APFSDS (x1.5 speed → 225-unit stride) and
    /// worse again whenever the framerate dropped, i.e. during big fights.
    ///
    /// Hit tests sweep `prev_pos -> current` instead, so there is no gap
    /// left to hide in regardless of speed or frame time.
    pub prev_pos: Vec2,
    /// Ricochets so far. Bounded by MAX_BOUNCES: a round caught in a concave
    /// notch between two sloped plates would otherwise skip off the same
    /// corner every frame forever.
    pub bounces: u8,
}

/// How many times one round may skip before it's spent.
pub const MAX_BOUNCES: u8 = 2;

/// Speed cap on a deflected round.
///
/// A ricochet sheds most of its energy, and the fraction alone wasn't enough:
/// a railgun does 9000 u/s, so even at 45% it left at 4000 and was off the
/// screen before the next frame. The deflection is the feedback — you're meant
/// to SEE the round leave along a new heading — and it's worth nothing if it
/// lasts one frame. Capped to something you can follow.
pub const RICOCHET_MAX_SPEED: f32 = 1500.0;

/// Closest approach between point `p` and segment `a -> b`.
/// Returns (distance, t) where t is 0 at `a` and 1 at `b`.
///
/// t is what orders hits along the round's path: the block it crossed FIRST
/// wins, not the one whose centre happens to sit nearest the segment.
pub fn segment_closest(a: Vec2, b: Vec2, p: Vec2) -> (f32, f32) {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq < 1e-6 {
        return (a.distance(p), 0.0);
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    (a.lerp(b, t).distance(p), t)
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

/// Missile entity — cold-gas eject, then motor burn, then guided flight.
///
/// The three phases are what separate a missile from a bullet: it leaves the
/// tube on a gas charge committed to the silo heading (`eject_time`), lights
/// the motor (`burn_fuel`), and only then starts steering (`reserve_fuel`).
/// By ignition the geometry is already off, so the seeker has to fly a curve
/// to fix it — which is the whole point.
#[derive(Component)]
pub struct MissileProjectile {
    pub damage: f32,
    pub target: Option<Entity>,
    /// SECONDS of main-motor burn remaining. Was a raw fuel quantity drained
    /// at a thrust-dependent rate, which made burn duration an accident of
    /// two unrelated numbers; as a time it is the thing you actually tune.
    pub burn_fuel: f32,
    /// RADIANS of steering authority remaining. Each frame's turn is
    /// subtracted, so a missile that fights a hard crossing shot runs out of
    /// corrections and coasts — the reason a Heavy can be baited into a miss.
    pub reserve_fuel: f32,
    pub thrust: f32,
    /// Maximum lateral acceleration in units/s². Divided by current speed to
    /// get a turn rate, so a missile that is going fast turns wide — the
    /// reason overshoot happens at all.
    pub max_lateral: f32,
    pub armed: bool,           // Needs to travel min distance before arming
    pub arm_distance: f32,
    pub traveled: f32,
    pub blast_radius: f32,
    pub owner: Entity,
    /// Seconds of cold-gas coast left before the motor lights. While this is
    /// positive the missile does not thrust and does not steer.
    pub eject_time: f32,
    /// The silo heading it was ejected along — held rigidly during the coast.
    pub launch_dir: Vec2,
    /// Seconds until self-destruct. Replaces the old trick of running
    /// `burn_fuel` negative and using it as a coast timer, which fought any
    /// attempt to give the motor a real burn duration.
    pub life: f32,
    /// Inside this range the seeker gets a harder turn limit — terminal dive.
    pub terminal_range: f32,
    /// Position last frame — swept-hit anchor, same role as `Projectile::prev_pos`.
    pub prev_pos: Vec2,
    /// The ship that launched it, when it was launched by a real tube.
    ///
    /// Unlike every other projectile in the game, a missile is NOT allowed
    /// through its parent hull — it is the deliberate exception. This is how
    /// it finds the hull to test. `None` for the plasma and EMP rounds that
    /// borrow this component, which keep the ordinary pass-through.
    pub owner_ship: Option<Entity>,
    /// Ship-local cell of the tube it left.
    ///
    /// Identified by CELL and not by entity: `update_ship_grids` writes hull
    /// after modules, so a launcher sitting on a hull tile — which is all of
    /// them — resolves to the hull segment in the grid, never to the weapon.
    /// Comparing entities meant a missile treated its own launcher's cell as
    /// solid hull and detonated the instant it spawned.
    pub launch_cell: IVec2,
}

impl Default for MissileProjectile {
    fn default() -> Self {
        Self {
            damage: 0.0,
            target: None,
            burn_fuel: 0.0,
            reserve_fuel: 0.0,
            thrust: 0.0,
            max_lateral: 0.0,
            armed: false,
            arm_distance: 80.0,
            traveled: 0.0,
            blast_radius: 40.0,
            owner: Entity::PLACEHOLDER,
            eject_time: 0.0,
            launch_dir: Vec2::X,
            life: 6.0,
            terminal_range: 300.0,
            prev_pos: Vec2::ZERO,
            owner_ship: None,
            launch_cell: IVec2::MAX,
        }
    }
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
        Entity, &Module, Has<CrewStation>, Option<&ModuleEfficiency>, &mut Weapon, &mut WeaponCooldown,
        &GlobalTransform, &FireGroup, &WeaponMount, &ChildOf,
        Option<&crate::building::customization::parameters::ModuleCustomization>,
        Option<&crate::building::customization::tuning::WeaponTuning>,
        Option<&crate::building::customization::tuning::SelectedAmmo>,
        Option<&ModuleTemperature>,
        Option<&crate::combat::targeting::AutoAimPoint>,
    ), Without<DestroyedModule>>,
    target_transform_query: Query<&Transform, Without<Ship>>,
    target_velocity_query: Query<&Velocity, Without<Ship>>,
    targeting_computer_query: Query<&Module, Without<DestroyedModule>>,
    windows_query: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    input_state: Res<crate::resources::InputState>,
    aim_lock: Res<crate::combat::targeting::AimLock>,
    mut fired_events: MessageWriter<crate::events::WeaponFired>,
    mut commands: Commands,
    debug_tuning: Res<crate::debug::DebugTuning>,
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

    for (entity, module, has_station, staffing, mut weapon, mut cooldown, global_transform, fire_group, mount, parent, customization, tuning, selected_ammo, temp, auto_aim) in weapon_query.iter_mut() {
        // A gun with nobody on it does not fire.
        if !crate::combat::weapon_is_crewed(has_station, staffing) {
            continue;
        }
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
        cooldown.timer.tick(time.delta().mul_f32(debug_tuning.fire_rate_mult));
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
        // A right-click lock names a specific BLOCK — that's the aim point,
        // capped to range like any other. Falls through to the ship-level
        // selection and then the cursor when nothing is locked.
        let (target_pos, target_vel) = if let Some(point) = aim_lock.aim_point() {
            let to_point = point - weapon_pos;
            let aim = if to_point.length() > weapon.range {
                weapon_pos + to_point.normalize_or_zero() * weapon.range
            } else {
                point
            };
            let vel = aim_lock.ship
                .and_then(|e| target_velocity_query.get(e).ok())
                .map(|v| v.0)
                .unwrap_or(Vec2::ZERO);
            (aim, vel)
        } else if let Some(auto) = auto_aim {
            // Nothing locked: this gun has been handed its own block on the
            // engaged ship (targeting::auto_engage). Every gun gets a
            // different one, which is what stops a battery from drilling a
            // single tile while the rest of the hull goes untouched.
            let to_point = auto.point - weapon_pos;
            let aim = if to_point.length() > weapon.range {
                weapon_pos + to_point.normalize_or_zero() * weapon.range
            } else {
                auto.point
            };
            let vel = target_velocity_query.get(auto.ship)
                .map(|v| v.0)
                .unwrap_or(Vec2::ZERO);
            (aim, vel)
        } else if let Some(target_entity) = selection.target {
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
                    KineticAmmoType::Incendiary
                    | KineticAmmoType::PlasmaSlug
                    | KineticAmmoType::NaniteCanister => ProjectileDamageType::Incendiary,
                    // A neutron shell does nothing electrical, but of the four
                    // damage types this is the one that means "the hull is
                    // fine, what's inside it isn't".
                    KineticAmmoType::EMPShell | KineticAmmoType::NeutronShell =>
                        ProjectileDamageType::EmpRound,
                    KineticAmmoType::APHE | KineticAmmoType::HEFrag | KineticAmmoType::Flak
                    | KineticAmmoType::Antimatter | KineticAmmoType::Singularity =>
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
                ModuleType::Railgun => (Vec2::new(90.0, 9.0), Color::srgb(0.2, 0.5, 1.0)),   // long blue streak
                ModuleType::Cannon => (Vec2::new(38.0, 22.0), Color::srgb(1.0, 0.45, 0.05)), // big orange shell
                ModuleType::Coilgun => (Vec2::new(24.0, 10.0), Color::srgb(0.6, 0.8, 1.0)),
                ModuleType::Gatling => (Vec2::new(17.0, 7.0), Color::srgb(1.0, 0.85, 0.2)),
                _ => (Vec2::new(17.0, 7.0), Color::srgb(0.8, 0.8, 0.4)),
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
                    damage: weapon.damage * ammo_damage_mult * debug_tuning.damage_mult,
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
                    prev_pos: weapon_pos,
                    bounces: 0,
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

        // Remember where the round was before this step. check_projectile_hits
        // runs immediately after this system and sweeps prev_pos -> current,
        // so nothing can slip through the gap between two frames.
        proj.prev_pos = transform.translation.truncate();

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
    // Transform and Sprite went MUTABLE here (a bounced round is stepped clear
    // of the plate and recoloured), which collides with the &Transform in the
    // creature and AI-ship queries below. Bevy can't infer that a projectile is
    // never a creature or a ship, so say so.
    mut proj_query: Query<
        (Entity, &mut Projectile, &mut Transform, &mut Velocity, &mut Sprite),
        (Without<Creature>, Without<Ship>, Without<crate::ai_ship::components::AiShip>),
    >,
    mut creature_query: Query<(&Transform, &mut Creature), Without<Ship>>,
    creature_grid: Res<crate::spatial::CreatureGrid>,
    mut ai_ship_query: Query<
        (Entity, &Transform, &GlobalTransform, &Children, &mut crate::combat::shields::ShipShield),
        With<crate::ai_ship::components::AiShip>,
    >,
    mut ai_module_query: Query<(&mut Module, &GlobalTransform), Without<DestroyedModule>>,
    mut ai_hull_query: Query<(&mut HullSegment, &GlobalTransform), Without<crate::components::HullDestroyed>>,
    // Neutron shells hurt the people, not the plate — see AmmoHitBehavior::Irradiate.
    mut crew_query: Query<&mut crate::components::CrewMember>,
    owner_parent_query: Query<&ChildOf>,
    grid_query: Query<&crate::building::ShipGrid>,
    block_query: Query<&crate::building::Block>,
    mut ai_damage_events: MessageWriter<crate::events::AiShipDamaged>,
    _notifications: MessageWriter<ShowNotification>,
) {
    'projectiles: for (proj_entity, mut proj, mut proj_transform, mut proj_vel, mut proj_sprite) in proj_query.iter_mut() {
        // Spent this frame: move_projectiles already queued its despawn. Both
        // systems sit in the same tuple and queue commands before the flush, so
        // resolving a hit on it here queues a SECOND despawn for the same
        // entity and the flush logs "Entity despawned: ... is invalid". Bevy
        // absorbs it, but it's noise that would mask a real one.
        if proj.lifetime <= 0.0 {
            continue;
        }
        let proj_pos = proj_transform.translation.truncate();
        // A weapon's own ship is never a valid target for its own shot,
        // regardless of aim — belt-and-suspenders on top of firing-arc and
        // per-ship query scoping.
        let owner_ship = owner_parent_query.get(proj.owner).ok().map(|p| p.parent());

        // === AI SHIPS: shield first, then block-by-block hull damage ===
        for (ai_entity, ai_transform, ai_gt, children, mut shield) in ai_ship_query.iter_mut() {
            if Some(ai_entity) == owner_ship { continue; }
            // Bubble is centered on the blocks' centroid, not the root
            let center = shield.world_center(ai_transform);
            let dist_to_ship = proj_pos.distance(center);

            // Directional shield: only the facing arc intercepts; flank/rear
            // slips past — and so does a phase slug, which is out of phase
            // with the matter the bubble is built to stop.
            let phased = proj.ammo.is_some_and(|a| a.ignores_shields());
            if !phased && shield.is_up() && dist_to_ship < shield.radius && shield.covers_arc(proj_pos - center) {
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
                // NARROW PHASE: the grid IS the hitbox. Sweep the segment
                // from where the round WAS last frame to where it is now
                // through this ship's cell space (building::ShipGrid::walk);
                // the blocks it passed through come back in order, nearest
                // first. Two bugs die here: fast rounds no longer tunnel
                // between samples (a railgun/APFSDS step is 2-3 cells), and
                // occupancy is cell membership — no 45-unit circles over-
                // covering flat faces and leaking through corner seams.
                let Ok(grid) = grid_query.get(ai_entity) else { continue };
                let inv = ai_gt.affine().inverse();
                let to_cell = |world: Vec2| {
                    let p = inv.transform_point3(world.extend(0.0)).truncate();
                    Vec2::new(p.x / 66.0, (p.y + 33.0) / 66.0)
                };
                let (cell_from, cell_to) = (to_cell(proj.prev_pos), to_cell(proj_pos));
                // World-space heading, for effects: dir_local is in the
                // target's cell space and would spray along the wrong axis on
                // any ship that isn't sitting at zero rotation.
                let dir_local_world = proj_vel.0.normalize_or_zero();
                // Direction in the SHIP's own cell space — the hull's heading
                // is already baked into `inv`, so angling the ship angles
                // every plate on that side without any new per-block data.
                let dir_local = (cell_to - cell_from).normalize_or_zero();
                // Walk the cells, then ask each block's SHAPE what the round
                // actually met inside it. A wedge fills half its cell, so a
                // round crossing the hollow corner passes through and the walk
                // carries on to whatever is behind — the block no longer gets
                // to claim its diagonal from every direction.
                let mut found = None;
                for step in grid.walk(cell_from, cell_to) {
                    // A penetrator is still inside the block it just went
                    // through; keep walking to the one behind it.
                    if Some(step.entity) == proj.last_hit { continue; }
                    let block = block_query
                        .get(step.entity)
                        .copied()
                        .unwrap_or(crate::building::Block::module(step.cell));
                    let entry = cell_from + dir_local * step.t_enter;
                    let exit = cell_from + dir_local * (step.t_enter + step.span);
                    if let Some(hit) = crate::combat::impact::clip_to_shape(
                        &block, step.entry_face, step.cell, entry, exit,
                    ) {
                        found = Some((step, block, hit));
                        break;
                    }
                }
                let Some((step, block, surface)) = found else { continue };
                let obl = crate::combat::impact::obliquity(
                    surface.normal, dir_local, &block, proj.ammo, proj.caliber,
                );

                // Primary hit: damage the struck block, remember where.
                let mut penetrated = false;
                let primary: Option<(Entity, Vec2)> = if let Ok((_, gt)) = ai_module_query.get(step.entity) {
                    // A cell holding armour resolves to the plate (hull wins
                    // the cell in update_ship_grids), so reaching a module
                    // means nothing covers it: fully exposed, takes everything.
                    let hit_pos = gt.translation().truncate();
                    let impact = crate::combat::impact::resolve_impact(proj.damage, &block, surface.span, obl, Some(0.0));
                    penetrated = impact.outcome == crate::combat::impact::ImpactOutcome::Penetrated;
                    ai_module_query.get_mut(step.entity).ok().map(|(mut module, _)| {
                        module.health = (module.health - impact.to_block).max(0.0);
                        spawn_hit_effect(&mut commands, hit_pos, Color::srgb(1.0, 0.6, 0.2), 12.0);
                        // Biting hits spray back along the round's own path —
                        // debris coming out of the hole, not off it.
                        spawn_impact_sparks(&mut commands, hit_pos, -dir_local_world, 0.2, 5);
                        spawn_floating_damage(&mut commands, hit_pos, impact.to_block, Color::srgb(1.0, 0.8, 0.3));
                        (step.entity, hit_pos)
                    })
                } else {
                    // ARMOUR EXPOSURE — a module still under live plating only
                    // takes the share of the round its ammo can drive through;
                    // the plate spends the rest. The walk lands on the plate
                    // (it owns the cell), so the module beneath is found by
                    // footprint, not by which sprite centre happened to be a
                    // hair nearer the round.
                    let covered = children.iter().find(|child| {
                        ai_module_query.get(*child).ok().is_some_and(|(module, _)| {
                            let footprint = crate::building::footprints::footprint_override(module.module_type);
                            crate::building::ShipGrid::cells_for(module.grid_position, module.size, module.rotation, footprint)
                                .contains(&step.cell)
                        })
                    });
                    let pass = match covered {
                        Some(_) => crate::combat::ammo_types::armor_pass_through(proj.ammo),
                        None => 0.0,
                    };
                    let impact = crate::combat::impact::resolve_impact(proj.damage, &block, surface.span, obl, Some(pass));
                    penetrated = impact.outcome == crate::combat::impact::ImpactOutcome::Penetrated;
                    let hull_hit = ai_hull_query.get_mut(step.entity).ok().map(|(mut hull, gt)| {
                        hull.health = (hull.health - impact.to_block).max(0.0);
                        let hit_pos = gt.translation().truncate();
                        spawn_hit_effect(&mut commands, hit_pos, Color::srgb(1.0, 0.5, 0.2), 16.0);
                        spawn_impact_sparks(&mut commands, hit_pos, -dir_local_world, 0.2, 6);
                        spawn_floating_damage(&mut commands, hit_pos, impact.to_block, Color::srgb(1.0, 0.3, 0.3));
                        (step.entity, hit_pos)
                    });
                    if let (Some(module_entity), true) = (covered, impact.through > 0.0) {
                        if let Ok((mut module, gt)) = ai_module_query.get_mut(module_entity) {
                            module.health = (module.health - impact.through).max(0.0);
                            spawn_floating_damage(&mut commands, gt.translation().truncate(), impact.through, Color::srgb(1.0, 0.8, 0.3));
                        }
                    }
                    hull_hit
                };

                let Some((hit_entity, hit_pos)) = primary else { continue };

                // SPALL — the plate's inner face letting go. Not the round's
                // own effect (that's AmmoHitBehavior below); this is the
                // ARMOUR failing, which is what makes a breach categorically
                // worse than a dent.
                //
                // HESH is the exception that proves the rule: through_solid
                // means it spalls WITHOUT getting through, which is its whole
                // identity and the reason its penetration is 0 by design. It's
                // the answer to a sloped hull you can't punch.
                let spall = crate::combat::ammo_types::spall(proj.ammo);
                if penetrated || spall.through_solid {
                    spall_blocks(
                        &mut commands, grid, &mut ai_module_query, &mut ai_hull_query,
                        spall, step.cell, dir_local, proj.damage, hit_entity,
                        hit_pos, dir_local_world,
                    );
                }

                // RICOCHET — the round skipped instead of biting. It's still
                // flying, so deflect it rather than despawning: a bounce can
                // find another block on this same ship, which is what makes a
                // concave hull a shot trap and a convex one shed fire.
                if obl.ricochet {
                    proj.bounces += 1;
                    proj.last_hit = Some(hit_entity);
                    // No floating word here. The sparks and the round's own
                    // new heading carry it, and a label per bounce buried the
                    // screen in text during a real fight. The angle still
                    // lives in the aim-lock readout, where it's one line on
                    // the block you're actually working on.
                    // TODO(audio): wants a hard metallic skip. No suitable
                    // asset — assets/audio/impacts has explosions only — and
                    // picking one is a taste call, so it's left unwired rather
                    // than filled with something wrong.
                    spawn_hit_effect(&mut commands, hit_pos, Color::srgb(0.95, 0.95, 0.85), 10.0);
                    if proj.bounces > MAX_BOUNCES {
                        commands.entity(proj_entity).despawn();
                        continue 'projectiles;
                    }
                    // Plate normal back out into world space — the ship's
                    // heading is in ai_gt, so this follows the hull as it turns.
                    let n_local = surface.normal.normalize_or_zero();
                    let n = ai_gt.affine()
                        .transform_vector3(n_local.extend(0.0))
                        .truncate()
                        .normalize_or_zero();
                    let v = proj_vel.0.normalize_or_zero();
                    if n != Vec2::ZERO && v != Vec2::ZERO {
                        // A mirror bounce looks wrong: real ricochets skid
                        // ALONG the plate and shed energy. Blend mirror toward
                        // the tangent by how grazing the hit was.
                        let mirror = v - 2.0 * v.dot(n) * n;
                        let tangent = (v - v.dot(n) * n).normalize_or_zero();
                        let skid = 0.35 + 0.45 * (1.0 - obl.cos_impact);
                        let scatter = (rand::random::<f32>() - 0.5) * 0.17; // ±~5°
                        let out = Vec2::from_angle(scatter)
                            .rotate(mirror.lerp(tangent, skid).normalize_or_zero());
                        let kept = proj_vel.0.length() * (0.45 + 0.40 * (1.0 - obl.cos_impact));
                        proj_vel.0 = out * kept.min(RICOCHET_MAX_SPEED);
                        proj.damage *= 0.10 + 0.30 * (1.0 - obl.cos_impact);

                        // Sparks along the NEW heading. This is the whole
                        // point: the round already changed direction, but a
                        // small pale flash gave no way to see that it had, so
                        // a deflection looked like a shot that simply failed.
                        let graze = 1.0 - obl.cos_impact;
                        spawn_impact_sparks(&mut commands, hit_pos, out, graze, 9 + (graze * 7.0) as usize);
                        // ...and the round itself goes hot, so it can be
                        // followed off the plate instead of vanishing into the
                        // background as a dim shape travelling somewhere new.
                        proj_sprite.color = Color::srgb(1.0, 0.85, 0.55);

                        // Step the round clear of the plate it just skipped
                        // off. Without this it restarts next frame still
                        // inside the ship, finds another block immediately,
                        // and burns its two bounces within a few frames — so
                        // it never visibly goes anywhere. `last_hit` only
                        // protects it from re-hitting the SAME block.
                        let clear = out * 70.0;
                        proj_transform.translation.x += clear.x;
                        proj_transform.translation.y += clear.y;
                        proj.prev_pos = proj_transform.translation.truncate();
                    }
                    ai_damage_events.write(crate::events::AiShipDamaged {
                        target: ai_entity,
                        source: crate::events::DamageSource::Explosion,
                        amount: 0.0,
                        position: Some(hit_pos),
                        direction: None,
                        attacker: owner_ship,
                    });
                    continue 'projectiles;
                }

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
                        Implode { crush_damage, crush_radius } => {
                            // Same reach as a blast, but every block in it
                            // takes the FULL crush — an implosion has no
                            // falloff, it just closes. Splash already applies
                            // one flat figure, which is exactly right here.
                            let radius = crush_radius * proj.caliber;
                            splash_blocks(
                                &mut commands, children, &mut ai_module_query, &mut ai_hull_query,
                                hit_entity, hit_pos, radius, crush_damage,
                            );
                            spawn_hit_effect(&mut commands, hit_pos, Color::srgb(0.4, 0.2, 0.6), radius);
                        }
                        Irradiate { dose, crew_affected } => {
                            // The hull is left alone on purpose. AI crew carry
                            // no Transform (see ai_ship::crew), so there is no
                            // position to measure a radius against — the cone
                            // is expressed as a headcount instead, taken off
                            // the ship's living crew.
                            let mut left = crew_affected;
                            for child in children.iter() {
                                if left == 0 { break; }
                                if let Ok(mut crew) = crew_query.get_mut(child) {
                                    if crew.health <= 0.0 { continue; }
                                    crew.health = (crew.health - dose).max(0.0);
                                    left -= 1;
                                }
                            }
                            // Dosed crew stop staffing their station, which is
                            // where the damage actually lands: an unmanned
                            // gun runs at zero efficiency (crew::compute_module_efficiency).
                            spawn_hit_effect(&mut commands, hit_pos, Color::srgb(0.8, 1.0, 0.6), 90.0);
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
                    attacker: owner_ship,
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
                // A well pulls in whatever is swimming past it, same as frag
                // catches a swarm — the reason it's worth firing at a shoal.
                Implode { crush_radius, crush_damage } => Some((crush_radius * proj.caliber, crush_damage * 0.5)),
                _ => None,
            }
        });

// Broad phase has to cover the whole step, not the end of it: the
        // grid query is centred on the segment's midpoint and widened by half
        // its length. Small creatures are where the old point test hurt most
        // — a VoidDrifter's 12-unit radius is a 24-unit window against a
        // railgun's 150-unit stride, so roughly five shots in six missed a
        // target they were aimed straight at.
        let sweep_mid = (proj.prev_pos + proj_pos) * 0.5;
        let sweep_half = proj.prev_pos.distance(proj_pos) * 0.5;
        for (creature_entity, _) in creature_grid.0.nearby(sweep_mid, sweep_half + MAX_CREATURE_HIT_RADIUS) {
            let Ok((creature_transform, mut creature)) = creature_query.get_mut(creature_entity) else { continue };
            if creature.health <= 0.0 { continue; }

            let creature_pos = creature_transform.translation.truncate();
            let hit_radius = match creature.creature_type {
                CreatureType::Leviathan => 90.0,
                CreatureType::Stalker => 30.0,
                CreatureType::ParasiteSwarm => 15.0,
                CreatureType::VoidDrifter => 12.0,
            };

            let (dist, _) = segment_closest(proj.prev_pos, proj_pos, creature_pos);
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
/// Fragments off the back of a breached plate, driven INWARD along the round's
/// path in a cone.
///
/// Deliberately not `splash_blocks`. That's a radius — it treats a bulkhead as
/// empty air and sprays the same in every direction. Spall is directional and
/// it STOPS at the first thing it meets, so what's behind the plate matters:
/// a reactor sitting right behind thin armour is in real danger, and a layer
/// of junk in front of it genuinely shields it.
///
/// Each fragment is a short walk on the same grid hit resolution uses, so it
/// respects the ship's actual layout rather than a distance check.
fn spall_blocks(
    commands: &mut Commands,
    grid: &crate::building::ShipGrid,
    module_query: &mut Query<(&mut Module, &GlobalTransform), Without<DestroyedModule>>,
    hull_query: &mut Query<(&mut HullSegment, &GlobalTransform), Without<crate::components::HullDestroyed>>,
    profile: crate::combat::ammo_types::SpallProfile,
    from_cell: IVec2,
    dir_local: Vec2,
    damage: f32,
    exclude: Entity,
    world_at: Vec2,
    world_dir: Vec2,
) {
    if profile.fragments == 0 || damage <= 0.0 || dir_local == Vec2::ZERO {
        return;
    }
    let origin = from_cell.as_vec2();
    let half = profile.cone_degrees.to_radians();
    for i in 0..profile.fragments {
        // Spread evenly across the cone rather than randomly, so a narrow
        // profile reads as a focused jet instead of three coin flips.
        let t = if profile.fragments == 1 {
            0.0
        } else {
            (i as f32 / (profile.fragments - 1) as f32) * 2.0 - 1.0
        };
        let jitter = (rand::random::<f32>() - 0.5) * 0.15;
        let heading = Vec2::from_angle(t * half + jitter).rotate(dir_local);
        for step in grid.walk(origin, origin + heading * profile.reach) {
            if step.entity == exclude { continue; }
            let frag = damage * profile.damage_frac;
            if let Ok((mut module, gt)) = module_query.get_mut(step.entity) {
                module.health = (module.health - frag).max(0.0);
                spawn_floating_damage(commands, gt.translation().truncate(), frag, Color::srgb(0.95, 0.85, 0.6));
            } else if let Ok((mut hull, gt)) = hull_query.get_mut(step.entity) {
                hull.health = (hull.health - frag).max(0.0);
                spawn_floating_damage(commands, gt.translation().truncate(), frag, Color::srgb(0.95, 0.85, 0.6));
            } else {
                continue;
            }
            // Stops at the first thing it hits — that's the whole point.
            break;
        }
    }
    // Sparks blowing INWARD, so a breach reads differently from a bounce
    // (which sprays back out along the round's new heading).
    spawn_impact_sparks(commands, world_at, world_dir, 0.5, 4 + profile.fragments as usize);
}

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
            attacker: None,
        });
        if burning.remaining <= 0.0 {
            commands.entity(entity).remove::<BlockBurning>();
        }
    }
}

#[cfg(test)]
mod sweep_tests {
    use super::*;

    // Real numbers from the bug: cells are 66 units, the per-block hit radius
    // is 45.0, and base_projectile_speed puts a Railgun at 9000 u/s — 150
    // units per frame at 60fps, or 225 with APFSDS's 1.5x multiplier.
    const HIT_RADIUS: f32 = 45.0;
    const RAILGUN_STEP: f32 = 150.0;
    const APFSDS_STEP: f32 = 225.0;

    /// The tunneling case. A block 100 units along a 150-unit step was 100
    /// away when the frame began and 50 away when it ended — outside the
    /// 45-unit radius at BOTH sample points, so the old point test never saw
    /// it and the round flew through solid armour.
    #[test]
    fn fast_round_does_not_tunnel_through_a_block() {
        let (prev, cur) = (Vec2::ZERO, Vec2::new(RAILGUN_STEP, 0.0));
        let block = Vec2::new(100.0, 0.0);

        // What the old point test saw at each frame boundary: nothing.
        assert!(prev.distance(block) > HIT_RADIUS);
        assert!(cur.distance(block) > HIT_RADIUS);

        // What the sweep sees: a dead-centre hit, two thirds along.
        let (dist, t) = segment_closest(prev, cur, block);
        assert!(dist < HIT_RADIUS, "swept test still missed the block");
        assert!((t - 100.0 / RAILGUN_STEP).abs() < 1e-4);
    }

    /// APFSDS is the worst offender — highest speed, and the round whose whole
    /// purpose is punching armour. A 225-unit stride leaves a 135-unit blind
    /// spot, and cells are 66 apart, so consecutive cells could both vanish.
    #[test]
    fn apfsds_stride_covers_every_cell_it_crosses() {
        let (prev, cur) = (Vec2::ZERO, Vec2::new(APFSDS_STEP, 0.0));
        for block_x in [66.0, 132.0, 198.0] {
            let (dist, _) = segment_closest(prev, cur, Vec2::new(block_x, 0.0));
            assert!(dist < HIT_RADIUS, "cell at {block_x} was skipped");
        }
    }

    /// Ranked by entry order, not proximity. Over a long step the round should
    /// strike the outer plate it crossed first; the old "nearest centre to the
    /// current position" rank could pick a block BEHIND that plate.
    #[test]
    fn hits_are_ordered_by_entry_not_proximity() {
        let (prev, cur) = (Vec2::ZERO, Vec2::new(APFSDS_STEP, 0.0));
        let plate = Vec2::new(40.0, 10.0);
        let core = Vec2::new(200.0, 0.0);

        let (_, t_plate) = segment_closest(prev, cur, plate);
        let (_, t_core) = segment_closest(prev, cur, core);
        assert!(t_plate < t_core, "outer plate must be struck before the core");

        // ...and the old rank would have chosen the core.
        assert!(cur.distance(core) < cur.distance(plate));
    }

    /// A just-spawned round has a zero-length segment. It must degrade to a
    /// plain point test rather than dividing by zero.
    #[test]
    fn zero_length_step_falls_back_to_a_point_test() {
        let p = Vec2::new(10.0, 20.0);
        let (dist, t) = segment_closest(p, p, Vec2::new(10.0, 50.0));
        assert!((dist - 30.0).abs() < 1e-4);
        assert_eq!(t, 0.0);
    }

    /// Closest approach is clamped to the segment: a block behind the muzzle or
    /// beyond this frame's end is not hit by this step.
    #[test]
    fn sweep_does_not_extend_past_the_segment() {
        let (prev, cur) = (Vec2::ZERO, Vec2::new(RAILGUN_STEP, 0.0));

        let (d_behind, t_behind) = segment_closest(prev, cur, Vec2::new(-200.0, 0.0));
        assert_eq!(t_behind, 0.0);
        assert!(d_behind > HIT_RADIUS);

        let (d_ahead, t_ahead) = segment_closest(prev, cur, Vec2::new(400.0, 0.0));
        assert_eq!(t_ahead, 1.0);
        assert!(d_ahead > HIT_RADIUS);
    }
}
