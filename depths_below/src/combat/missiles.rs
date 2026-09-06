use bevy::prelude::*;
use crate::celestial::components::{GravityAffected, GravityForce};
use super::targeting::{TargetSelection, FireGroupState, FireGroup};
use super::new_projectiles::MissileProjectile;
use crate::vfx::particles::Particle;
use super::*;

// ============================================================================
// MISSILE SYSTEM
// Cold-gas eject -> motor burn -> proportional-navigation guidance.
// Bay chain length determines missile size. Decoyable. Shootable.
// ============================================================================

/// Cold-gas eject speed. The missile leaves the tube on a gas charge, not its
/// own motor — ~3 cells/s, slow enough to read as a "pop" rather than a shot.
const EJECT_SPEED: f32 = 200.0;

/// Seconds of coast before the motor lights, per launcher. At EJECT_SPEED
/// that is ~90-100 units for the big tubes: clear of the hull and its
/// outboard plating before the burn starts, which is what makes the silo
/// obstruction rule mean something.
const EJECT_TIME_HEAVY: f32 = 0.5;
const EJECT_TIME_GUIDED: f32 = 0.4;
const EJECT_TIME_CLUSTER: f32 = 0.2;

/// Base motor thrust. Ignition has to read as a kick, not a drift.
const BOOST_THRUST: f32 = 700.0;

/// Seconds the motor burns. Thrust x burn time is the missile's whole speed
/// budget: 900 x 1.6 = 1440 u/s on top of the eject charge.
const BURN_TIME: f32 = 1.4;

/// Speed ceiling. Turn radius goes with the SQUARE of speed, so this is the
/// single biggest lever on whether a missile can corner: dropping the cap
/// from 2200 to 1200 cuts the circle it flies to under a third of its old
/// size before agility is touched at all.
const MISSILE_MAX_SPEED: f32 = 1200.0;

/// Radians of steering authority a fresh missile carries. Every radian it
/// turns is deducted; at zero it coasts. Enough for ~8s of hard cornering,
/// so only a missile genuinely fighting an evasive target runs itself dry.
const RESERVE_TURN: f32 = 25.0;

/// Hard ceiling on turn RATE, independent of the lateral-acceleration cap.
///
/// Turn rate is lateral accel / speed, so a missile that has just lit its
/// motor — still only doing eject speed — could legally pull 7 rad/s. That is
/// a donut, not a course correction, and it spent the entire steering budget
/// in the first second and a half of flight.
const MAX_TURN_RATE: f32 = 4.0;

/// Heading-error gain for the pure-pursuit fallback. Proportional, so a
/// missile only slightly off the target eases on instead of slamming full
/// deflection and sawing back the other way.
const PURSUIT_GAIN: f32 = 2.5;

/// Proportional-navigation gain. 3 flies a visible curve onto the target; 5
/// snaps almost straight; below 2 it wanders and misses.
const PN_GAIN: f32 = 3.0;

/// Max lateral acceleration by launcher, in units/s².
///
/// The number to reason about is TURN RADIUS, r = v²/a, because that is what
/// you actually see. At the 1200 u/s cap a Guided pulls a ~275-unit circle
/// (~4 cells) and a Heavy a ~450-unit one (~7 cells). The previous values
/// gave the Heavy a 29-cell circle: technically guided, and in practice it
/// sailed past everything it was aimed at.
const LATERAL_GUIDED: f32 = 5200.0;
const LATERAL_HEAVY: f32 = 3200.0;

/// Inside this range the seeker is allowed a harder turn — the terminal dive.
const TERMINAL_RANGE: f32 = 300.0;
const TERMINAL_AGILITY_BONUS: f32 = 1.6;

/// Seconds before self-destruct, per launcher. Heavy carries the most: range
/// 9600 at ~1400 u/s average is ~7s, plus margin to hook round for a second
/// pass after an overshoot.
const LIFE_HEAVY: f32 = 9.0;
const LIFE_GUIDED: f32 = 8.0;
const LIFE_CLUSTER: f32 = 5.0;

/// Fire missile weapons when their fire group is active
pub fn fire_missiles_system(
    time: Res<Time>,
    fire_state: Res<FireGroupState>,
    power_state: Res<crate::resources::PowerState>,
    selection: Res<TargetSelection>,
    aim_lock: Res<super::targeting::AimLock>,
    ship_query: Query<(Entity, &ShipPhysics, &Transform, &Velocity), With<Ship>>,
    mut weapon_query: Query<(
        // WeaponMount is gone from here deliberately: a missile leaves down
        // its own tube regardless of arc, so nothing consults the mount.
        Entity, &Module, Has<CrewStation>, Option<&ModuleEfficiency>, &mut Weapon, &mut WeaponCooldown,
        &GlobalTransform, &FireGroup, &ChildOf,
        Option<&crate::building::customization::tuning::WeaponTuning>,
        Option<&ModuleTemperature>,
    ), Without<DestroyedModule>>,
    // One query, not three: everything a missile may chase, and the lookup
    // for whatever is already locked. Bevy caps a system at 16 parameters
    // and this one was at the ceiling.
    target_query: Query<
        (Entity, &Transform, Option<&Creature>),
        (Without<Ship>, Or<(With<crate::ai_ship::components::AiShip>, With<Creature>)>),
    >,
    machine_stats: Query<&crate::building::multiblock::components::MachineStats>,
    mut fuel_state: ResMut<crate::resources::FuelState>,
    windows_query: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    input_state: Res<crate::resources::InputState>,
    mut commands: Commands,
    mut notifications: MessageWriter<ShowNotification>,
    mut fired_events: MessageWriter<crate::events::WeaponFired>,
) {
    let Ok((player_ship, ship_physics, ship_transform, ship_velocity)) = ship_query.single() else { return };

    // Weapons need power — grid deficit silences launchers too.
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
    // InputState.gamepad_aim).
    let cursor_world = input_state.gamepad_aim
        .map(|dir| ship_transform.translation.truncate() + dir * 2000.0)
        .or(cursor_world);

    // Everything a missile could reasonably chase, gathered once. Cheap —
    // a handful of AI ships and whatever creatures are loaded — and reused
    // by every launcher below.
    let candidates: Vec<(Entity, Vec2)> = target_query
        .iter()
        .filter(|(_, _, creature)| creature.is_none_or(|c| c.health > 0.0))
        .map(|(e, t, _)| (e, t.translation.truncate()))
        .collect();

    for (entity, module, has_station, staffing, mut weapon, mut cooldown, global_transform, fire_group, parent, tuning, temp) in weapon_query.iter_mut() {
        // A gun with nobody on it does not fire.
        if !crate::combat::weapon_is_crewed(has_station, staffing) {
            continue;
        }
        // Player ship only — see fire_weapons_system for why this matters:
        // AI ships carry identical missile-bay components and would
        // otherwise launch whenever the player fires, homing on the
        // player's own target selection.
        if parent.parent() != player_ship { continue; }
        // Only missile-type weapons
        if !matches!(module.module_type,
            ModuleType::HeavyMissile | ModuleType::GuidedMissile | ModuleType::ClusterRocket
        ) { continue; }

        if !module.is_active { continue; }

        // Tick before the thermal gate — see fire_weapons_system: gating
        // first freezes the timer, which generate_heat reads as "recently
        // fired", locking the launcher hot forever.
        cooldown.timer.tick(time.delta());
        if !cooldown.timer.is_finished() { continue; }

        // Thermal throttle — same gate the laser/kinetics use.
        if let Some(temp) = temp {
            if temp.current >= temp.max_temp * 0.95 { continue; }
        }

        let group_firing = fire_state.firing[fire_group.group as usize % 4];
        if !group_firing { continue; }

        if !crate::combat::INFINITE_AMMO && weapon.ammo <= 0 { continue; }

        let weapon_pos = global_transform.translation().truncate();

        // Homing target, in the order the player expects to be obeyed.
        //
        // Right-click block lock is the documented primary way to aim the
        // battery (see combat::targeting::aim_lock) and every other weapon
        // honours it — missiles read only `selection.target`, the middle-click
        // ship lock, so the normal aiming flow produced unguided rockets that
        // flew at the cursor. Homing follows the locked SHIP rather than the
        // locked block: a block's Transform is ship-local, and the warhead's
        // blast radius covers the difference anyway.
        let locked = aim_lock.ship.filter(|_| aim_lock.is_locked());
        // Nothing locked: seek on the player's behalf rather than throwing a
        // guided weapon away as a dumb rocket. Candidates are scored by
        // distance to the CURSOR, so where you point still decides which
        // enemy gets the salvo, but only ones inside this launcher's range
        // are eligible — a seeker aimed at something it cannot reach is just
        // a slower miss.
        let auto = cursor_world.and_then(|cursor| {
            candidates.iter()
                .filter(|(_, pos)| weapon_pos.distance(*pos) <= weapon.range)
                .min_by(|(_, a), (_, b)| {
                    cursor.distance_squared(*a)
                        .partial_cmp(&cursor.distance_squared(*b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(e, _)| *e)
        });
        // Only the seeker's target is decided here. Where the missile is
        // POINTED is no longer a function of the target at all — it leaves
        // down the tube either way — so a launcher with nothing to chase
        // still fires, and flies out unguided.
        let homing_target = locked.or(selection.target).or(auto)
            .filter(|e| target_query.get(*e).is_ok());

        // The tube points where the tube points.
        //
        // Launch direction used to be the aim direction bent back to the edge
        // of the firing arc, so a target anywhere inside the cone meant the
        // missile left already angled onto it — nothing about the launch said
        // "silo". Ejecting along the module's own facing instead makes every
        // launch come out of the tube the way the tube is welded on, whatever
        // the ship is doing, and leaves the turn onto the target to the
        // seeker where it belongs. It is also inherently safe in the way the
        // arc clamp was trying to be: a missile that leaves along its own
        // tube never leaves backwards through the hull.
        //
        // The +FRAC_PI_2 is not a fudge: module art in this game is drawn
        // pointing +Y, not +X (turrets::aim_turrets subtracts the same
        // quarter turn for barrels). Rotation::East is -PI/2, so an
        // East-mounted launcher RENDERS pointing +X — the bow — which is why
        // every layout puts bow tubes at East and stern engines at West.
        // Without this the missile left 90 degrees off the tube it visibly
        // came out of.
        let launch_dir = Vec2::from_angle(ship_physics.rotation + module.rotation.facing_angle());

        // Bay chain length is missile size — see MachineStats.
        let bay_count = machine_stats.get(entity)
            .map(|s| s.barrel_count.max(1))
            .unwrap_or(1);
        // Ship fuel pays for the LAUNCH; the missile's own burn and steering
        // authority are properties of the missile, not of how much fuel the
        // ship happened to have.
        let fuel_cost = 5.0 * bay_count as f32;
        if fuel_state.current_fuel < fuel_cost {
            notifications.write(ShowNotification {
                message: "No fuel for missile launch!".into(),
                notification_type: NotificationType::Warning,
                duration: 2.0,
            });
            continue;
        }
        fuel_state.current_fuel -= fuel_cost;

        cooldown.timer.reset();
        if !crate::combat::INFINITE_AMMO {
            weapon.ammo = weapon.ammo.saturating_sub(1);
        }
        fired_events.write(crate::events::WeaponFired {
            weapon_type: module.module_type,
            position: weapon_pos,
            from_player: true,
        });

        launch_missiles(&mut commands, &MissileLaunch {
            weapon: entity,
            ship: player_ship,
            launch_cell: module.grid_position,
            muzzle: weapon_pos,
            launch_dir,
            ship_velocity: ship_velocity.0,
            target: homing_target,
            module_type: module.module_type,
            damage: weapon.damage,
            bays: bay_count,
            thrust_mult: tuning.map(|t| t.velocity).unwrap_or(1.0),
        });

        // Launch notification for heavy missiles
        if module.module_type == ModuleType::HeavyMissile {
            notifications.write(ShowNotification {
                message: "Heavy missile launched!".into(),
                notification_type: NotificationType::Warning,
                duration: 1.5,
            });
        }
    }
}

/// Everything one launcher needs to put a missile in the air.
///
/// Shared by the player's fire_missiles_system and the AI's gunnery loop.
/// Enemy launchers used to route through the legacy straight-line projectile
/// path, so an "enemy missile" was a fast red bullet — none of the eject,
/// guidance or collision behaviour applied to them at all.
pub struct MissileLaunch {
    pub weapon: Entity,
    pub ship: Entity,
    pub launch_cell: IVec2,
    pub muzzle: Vec2,
    /// Tube heading in WORLD space: ship rotation + the module's facing.
    pub launch_dir: Vec2,
    pub ship_velocity: Vec2,
    pub target: Option<Entity>,
    pub module_type: ModuleType,
    pub damage: f32,
    pub bays: u32,
    pub thrust_mult: f32,
}

/// Spawn a launcher's salvo. Returns how many bodies left the tube.
pub fn launch_missiles(commands: &mut Commands, l: &MissileLaunch) -> u32 {
    let size_mult = l.bays.max(1) as f32;
    let bulk = size_mult.sqrt();
    let (max_lateral, eject_time, life) = match l.module_type {
        ModuleType::GuidedMissile => (LATERAL_GUIDED / bulk, EJECT_TIME_GUIDED, LIFE_GUIDED),
        ModuleType::HeavyMissile => (LATERAL_HEAVY / bulk, EJECT_TIME_HEAVY, LIFE_HEAVY),
        ModuleType::ClusterRocket => (0.0, EJECT_TIME_CLUSTER, LIFE_CLUSTER),
        _ => (LATERAL_HEAVY / bulk, EJECT_TIME_HEAVY, LIFE_HEAVY),
    };
    let missile_damage = l.damage * size_mult;
    let volley = match l.module_type {
        ModuleType::ClusterRocket => (3.0 * size_mult).min(8.0) as u32,
        _ => 1,
    };
    let visual_w = 24.0 + size_mult * 8.0;
    let visual_h = 10.0 + size_mult * 4.0;

    for i in 0..volley {
        // Fan the volley by ROTATING the launch heading, not by adding a
        // world-space offset — that shoved every rocket the same way
        // regardless of which way the tube pointed.
        let spread_angle = if volley > 1 {
            (i as f32 - (volley - 1) as f32 / 2.0) * 0.12
        } else {
            0.0
        };
        let (sin, cos) = spread_angle.sin_cos();
        let dir = Vec2::new(
            l.launch_dir.x * cos - l.launch_dir.y * sin,
            l.launch_dir.x * sin + l.launch_dir.y * cos,
        );
        // Inherit the ship's own velocity, or a ship at speed rear-ends its
        // own salvo.
        let vel = dir * EJECT_SPEED + l.ship_velocity;
        let per_missile = if volley > 1 { missile_damage / volley as f32 } else { missile_damage };

        commands.spawn((
            (Sprite {
                    color: Color::srgb(0.8, 0.3, 0.2),
                    custom_size: Some(Vec2::new(visual_w, visual_h)),
                    ..default()
                }, Transform {
                    translation: Vec3::new(l.muzzle.x, l.muzzle.y, 0.5),
                    rotation: Quat::from_rotation_z(dir.y.atan2(dir.x)),
                    ..default()
                }),
            MissileProjectile {
                damage: per_missile,
                target: if max_lateral > 0.0 { l.target } else { None },
                burn_fuel: BURN_TIME * bulk,
                reserve_fuel: RESERVE_TURN,
                thrust: BOOST_THRUST * l.thrust_mult / bulk,
                max_lateral,
                arm_distance: 80.0,
                blast_radius: 30.0 + size_mult * 20.0,
                owner: l.weapon,
                eject_time,
                launch_dir: dir,
                life,
                terminal_range: TERMINAL_RANGE,
                prev_pos: l.muzzle,
                owner_ship: Some(l.ship),
                launch_cell: l.launch_cell,
                ..default()
            },
            MissileTrail::default(),
            Velocity(vel),
            GravityAffected { mass: 2.0 + size_mult },
            GravityForce::default(),
        ));
    }
    volley
}

/// Per-missile exhaust bookkeeping.
///
/// Kept off `MissileProjectile` so a plasma bolt — which borrows that
/// component purely for blast resolution — does not trail rocket smoke.
/// The timers are per-missile rather than a shared `Local`, so a volley
/// staggers its puffs instead of emitting every particle on one frame.
#[derive(Component)]
pub struct MissileTrail {
    flame: f32,
    smoke: f32,
    ignited: bool,
}

impl Default for MissileTrail {
    fn default() -> Self {
        // Staggered start so a cluster salvo does not pulse in lockstep.
        Self { flame: rand::random::<f32>() * 0.02, smoke: rand::random::<f32>() * 0.03, ignited: false }
    }
}

/// Distance from a missile's centre back to its nozzle.
const NOZZLE_OFFSET: f32 = 14.0;

/// Exhaust, smoke and the one-shot ignition flash.
///
/// The smoke is the load-bearing part: it is emitted with almost no velocity
/// of its own, so it hangs where the missile passed and draws the whole curve
/// on screen. Without it the flight path only exists for as long as the
/// missile is standing on it.
pub fn spawn_missile_trails(
    time: Res<Time>,
    mut commands: Commands,
    fx: Res<crate::vfx::effect_textures::EffectTextures>,
    mut query: Query<(&MissileProjectile, &Transform, &Velocity, &mut MissileTrail)>,
) {
    let dt = time.delta_secs();

    for (missile, transform, velocity, mut trail) in query.iter_mut() {
        let pos = transform.translation.truncate();
        // While ejecting the missile is pointed down the tube even though its
        // velocity carries the ship's motion, so the plume has to follow the
        // same heading the sprite does or it blows out of the wrong end.
        let heading = if missile.eject_time > 0.0 {
            missile.launch_dir
        } else {
            velocity.0.normalize_or_zero()
        };
        if heading == Vec2::ZERO {
            continue;
        }
        let nozzle = pos - heading * NOZZLE_OFFSET;

        // === COLD GAS: pale, slow, thin. The motor is not lit yet. ===
        if missile.eject_time > 0.0 {
            trail.flame -= dt;
            if trail.flame <= 0.0 {
                trail.flame = 0.045;
                let drift = Vec2::from_angle((rand::random::<f32>() - 0.5) * 0.9).rotate(-heading);
                let life = 0.3 + rand::random::<f32>() * 0.2;
                commands.spawn((
                    Sprite {
                        image: fx.puff(),
                        color: Color::srgba(0.82, 0.86, 0.92, 0.5),
                        custom_size: Some(Vec2::splat(10.0 + rand::random::<f32>() * 7.0)),
                        ..default()
                    },
                    Transform::from_xyz(nozzle.x, nozzle.y, 0.45),
                    Particle::wisp(drift * (50.0 + rand::random::<f32>() * 40.0), life, 0.5, true),
                ));
            }
            continue;
        }

        // === IGNITION: one-shot flash out the back. ===
        if !trail.ignited {
            trail.ignited = true;
            spawn_hit_effect(&mut commands, nozzle, Color::srgb(1.0, 0.86, 0.55), 30.0);
            spawn_impact_sparks(&mut commands, nozzle, -heading, 0.45, 10);
        }

        // === FLAME: only while the motor is actually burning. ===
        if missile.burn_fuel > 0.0 {
            trail.flame -= dt;
            if trail.flame <= 0.0 {
                trail.flame = 0.018;
                let spread = Vec2::from_angle((rand::random::<f32>() - 0.5) * 0.5).rotate(-heading);
                let life = 0.12 + rand::random::<f32>() * 0.14;
                let hot = rand::random::<f32>() < 0.4;
                commands.spawn((
                    Sprite {
                        color: if hot {
                            Color::srgba(1.0, 0.95, 0.75, 1.0)
                        } else {
                            Color::srgba(1.0, 0.55, 0.15, 0.95)
                        },
                        custom_size: Some(Vec2::splat(if hot { 7.0 } else { 10.0 })),
                        ..default()
                    },
                    Transform::from_xyz(nozzle.x, nozzle.y, 0.55),
                    Particle::wisp(spread * (200.0 + rand::random::<f32>() * 180.0), life, 1.0, true),
                ));
            }
        }

        // === SMOKE: hangs in place and marks the path. ===
        trail.smoke -= dt;
        if trail.smoke <= 0.0 {
            trail.smoke = 0.03;
            let life = 1.0 + rand::random::<f32>() * 0.6;
            let grey = 0.30 + rand::random::<f32>() * 0.18;
            commands.spawn((
                Sprite {
                    image: fx.puff(),
                    // Bigger than the solid quad this replaced. A soft puff's
                    // visible core is roughly the inner half of its footprint
                    // -- the rest is low-alpha rim that vanishes against the
                    // void -- so matched sizes would read as a fainter,
                    // thinner trail than the squares, not a softer one.
                    color: Color::srgba(grey, grey * 0.96, grey * 0.93, 0.5),
                    custom_size: Some(Vec2::splat(13.0 + rand::random::<f32>() * 10.0)),
                    ..default()
                },
                Transform::from_xyz(nozzle.x, nozzle.y, 0.44),
                // Almost no velocity: the trail should stay where it was laid
                // down, not chase the missile that laid it.
                Particle::wisp(
                    Vec2::from_angle(rand::random::<f32>() * std::f32::consts::TAU) * 14.0,
                    life,
                    0.5,
                    false,
                ),
            ));
        }
    }
}

/// Move and guide missiles: cold-gas eject, then motor burn, then guidance.
///
/// The eject phase is the whole trick. A missile that lights its motor in the
/// tube and steers immediately just flies at the target, which is what made
/// the old ones read as slow bullets. Committing the first ~90 units to the
/// silo heading means the seeker inherits a bad angle and has to fly a curve
/// out of it — and a sluggish one overshoots and comes back around.
pub fn move_missiles(
    time: Res<Time>,
    mut commands: Commands,
    mut missile_query: Query<(Entity, &mut MissileProjectile, &mut Transform, &mut Velocity, &GravityForce)>,
    target_query: Query<(&Transform, Option<&Velocity>), Without<MissileProjectile>>,
    hulls: Query<(&GlobalTransform, &crate::building::ShipGrid)>,
) {
    let dt = time.delta_secs();

    for (entity, mut missile, mut transform, mut velocity, gravity) in missile_query.iter_mut() {
        let pos = transform.translation.truncate();
        missile.prev_pos = pos;

        missile.life -= dt;
        if missile.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        missile.traveled += velocity.0.length() * dt;
        if !missile.armed && missile.traveled > missile.arm_distance {
            missile.armed = true;
        }

        // Still threading its own ship? Then it is in a launch channel and
        // holds its heading.
        //
        // This is the Forts behaviour: blocks to the LEFT and RIGHT of a tube
        // do not stop a missile, they guide it — it keeps going forward until
        // it is clear. Only something dead ahead stops it, and that is a
        // collision, handled in check_missile_hits. A fixed eject timer could
        // not express this: a tube at the bottom of a long channel would
        // start steering while still inside the hull and turn into a wall.
        let in_channel = missile.owner_ship.and_then(|ship| hulls.get(ship).ok())
            .is_some_and(|(ship_gt, grid)| {
                let cell = crate::building::world_to_cell(ship_gt, pos);
                let c = IVec2::new(cell.x.round() as i32, cell.y.round() as i32);
                // The cell it is in, or either shoulder of it — a missile
                // level with the hull is still leaving, not yet away.
                grid.contains(c)
                    || grid.contains(c + IVec2::X) || grid.contains(c + IVec2::NEG_X)
                    || grid.contains(c + IVec2::Y) || grid.contains(c + IVec2::NEG_Y)
            });

        // === EJECT: gas charge only. Motor cold, seeker caged. ===
        let ejecting = missile.eject_time > 0.0;
        if ejecting {
            missile.eject_time -= dt;
        } else {
            // === BURN: motor lit. ===
            if missile.burn_fuel > 0.0 {
                missile.burn_fuel -= dt;
                let forward = velocity.0.normalize_or_zero();
                velocity.0 += forward * missile.thrust * dt;
                let speed = velocity.0.length();
                if speed > MISSILE_MAX_SPEED {
                    velocity.0 *= MISSILE_MAX_SPEED / speed;
                }
            }

            // === GUIDANCE: proportional navigation. ===
            //
            // PN steers to null the rotation rate of the line of sight rather
            // than to point at the target, which is what produces a lead
            // curve instead of the tail-chase the old cross-product
            // controller flew. Commanded lateral acceleration is
            // N x (LOS rate) x (closing speed).
            if missile.reserve_fuel > 0.0 && missile.max_lateral > 0.0 && !in_channel {
                if let Some(target) = missile.target {
                    if let Ok((target_tf, target_vel)) = target_query.get(target) {
                        let speed = velocity.0.length();
                        let los = target_tf.translation.truncate() - pos;
                        let range = los.length();
                        if speed > 1.0 && range > 1.0 {
                            let los_dir = los / range;
                            let rel_vel = target_vel.map(|v| v.0).unwrap_or(Vec2::ZERO) - velocity.0;
                            let closing = -rel_vel.dot(los_dir);
                            let cap = if range < missile.terminal_range {
                                missile.max_lateral * TERMINAL_AGILITY_BONUS
                            } else {
                                missile.max_lateral
                            };
                            let max_turn = (cap / speed).min(MAX_TURN_RATE);

                            let turn_rate = if closing > 1.0 {
                                let los_rate = los_dir.perp_dot(rel_vel) / range;
                                let lateral = PN_GAIN * los_rate * closing;
                                (lateral / speed).clamp(-max_turn, max_turn)
                            } else {
                                // Overshot, or the target is outrunning us. PN
                                // has no closing speed to work with and would
                                // command nothing, leaving the missile to fly
                                // off into the dark. Fall back to pure pursuit
                                // — proportional on heading error, so it hooks
                                // round hard when the target is behind it and
                                // gently when it is nearly lined up.
                                let heading = velocity.0 / speed;
                                let angle_err = heading.perp_dot(los_dir)
                                    .atan2(heading.dot(los_dir));
                                (angle_err * PURSUIT_GAIN).clamp(-max_turn, max_turn)
                            };

                            let turn = turn_rate * dt;
                            let heading = velocity.0.y.atan2(velocity.0.x) + turn;
                            velocity.0 = Vec2::new(heading.cos(), heading.sin()) * speed;
                            // Every radian of correction is spent authority.
                            missile.reserve_fuel -= turn.abs();
                        }
                    }
                }
            }
        }

        velocity.0 += gravity.0 * dt;

        transform.translation.x += velocity.0.x * dt;
        transform.translation.y += velocity.0.y * dt;

        // Point down the TUBE while the gas charge carries it out, and along
        // the flight path once the motor and seeker take over. Facing purely
        // by velocity made a missile launched from a moving ship slide out
        // sideways: it inherits the hull's speed, which at anything above a
        // crawl swamps the 200 u/s eject and leaves the sprite pointing where
        // the ship is going rather than where the silo is aimed.
        let facing = if ejecting || in_channel { missile.launch_dir } else { velocity.0 };
        if facing.length_squared() > 1.0e-4 {
            transform.rotation = Quat::from_rotation_z(facing.y.atan2(facing.x));
        }
    }
}

/// Largest creature hit radius (Leviathan) — used to size the spatial grid query margin.
const MAX_CREATURE_HIT_RADIUS: f32 = 100.0;

/// Check missile hits — armed missiles explode on contact.
/// Uses the creature spatial grid to only distance-check nearby creatures.
pub fn check_missile_hits(
    mut commands: Commands,
    fx: Res<crate::vfx::effect_textures::EffectTextures>,
    missile_query: Query<(Entity, &MissileProjectile, &Transform)>,
    mut creature_query: Query<(&Transform, &mut Creature), Without<Ship>>,
    creature_grid: Res<crate::spatial::CreatureGrid>,
    mut ai_ship_query: Query<
        (Entity, &Transform, &Children, &mut crate::combat::shields::ShipShield),
        With<crate::ai_ship::components::AiShip>,
    >,
    mut ai_module_query: Query<(&mut Module, &GlobalTransform), Without<DestroyedModule>>,
    block_query: Query<&crate::building::Block>,
    owner_parent_query: Query<&ChildOf>,
    mut ai_damage_events: MessageWriter<crate::events::AiShipDamaged>,
    mut notifications: MessageWriter<ShowNotification>,
    parent_hulls: Query<(&GlobalTransform, &crate::building::ShipGrid)>,
    mut ship_damage_events: MessageWriter<crate::events::ShipDamaged>,
    player_query: Query<Entity, With<Ship>>,
) {
    'missiles: for (missile_entity, missile, missile_transform) in missile_query.iter() {
        // === HULLS ===
        //
        // Missiles are the exception to the rule that a round ignores the
        // ship that fired it. A warhead is a physical object leaving a tube
        // at walking pace, and flying it through armour looks absurd in a way
        // a hypersonic slug does not. The same walk serves both ends of that:
        // against its OWN hull it is the cook-off the silo warning promises,
        // and against the player's it is how an enemy missile lands at all —
        // the blast branch below only ever looks at AI ships.
        //
        // Checked BEFORE arming: the whole point is the ones that never get
        // clear of the tube.
        let player_ship = player_query.single().ok();
        let missile_pos_now = missile_transform.translation.truncate();
        for ship in [missile.owner_ship, player_ship].into_iter().flatten() {
            let Ok((ship_gt, grid)) = parent_hulls.get(ship) else { continue };
            let own = missile.owner_ship == Some(ship);
            let here = crate::building::world_to_cell(ship_gt, missile_pos_now);
            let cell = IVec2::new(here.x.round() as i32, here.y.round() as i32);
            let Some(hit) = grid.get(cell) else { continue };

            if own {
                // Not the tube it is leaving, and not its own plating: one
                // course of armour is a blow-out panel, exactly as it is for
                // the build-mode silo check. Identified by CELL, because
                // update_ship_grids writes hull after modules and a launcher
                // on a hull tile resolves to the hull segment.
                if cell == missile.launch_cell || hit == missile.owner {
                    continue;
                }
                if ai_module_query.get(hit).is_ok_and(|(m, _)| crate::building::is_blowout_panel(m.module_type)) {
                    continue;
                }
            }

            spawn_explosion(&mut commands, &fx, missile_pos_now, missile.blast_radius, Color::srgb(1.0, 0.45, 0.1));
            if let Ok((mut module, _)) = ai_module_query.get_mut(hit) {
                module.health = (module.health - missile.damage).max(0.0);
            }

            if Some(ship) == player_ship {
                // Direction points from the ship TOWARD the attacker — the
                // convention process_ship_damage walks inward along.
                let from = (missile.prev_pos - missile_pos_now).normalize_or_zero();
                ship_damage_events.write(crate::events::ShipDamaged {
                    source: crate::events::DamageSource::Explosion,
                    // Real amount when it is an ENEMY warhead: the player's
                    // damage pipeline applies it. Zero for our own cook-off,
                    // which was already applied to the block above.
                    amount: if own { 0.0 } else { missile.damage },
                    position: Some(missile_pos_now),
                    direction: Some(from),
                });
                notifications.write(ShowNotification {
                    message: if own {
                        "SILO OBSTRUCTED — WARHEAD COOK-OFF".into()
                    } else {
                        "MISSILE IMPACT".to_string()
                    },
                    notification_type: NotificationType::Danger,
                    duration: 2.5,
                });
            }
            commands.entity(missile_entity).despawn();
            continue 'missiles;
        }

        if !missile.armed { continue; }

        let missile_pos = missile_transform.translation.truncate();
        // A weapon's own ship is never a valid target for its own missile.
        let owner_ship = owner_parent_query.get(missile.owner).ok().map(|p| p.parent());

        // === AI SHIPS: shield absorbs the warhead; shield down = blast
        // damage to every block inside the blast radius ===
        for (ai_entity, ai_transform, children, mut shield) in ai_ship_query.iter_mut() {
            if Some(ai_entity) == owner_ship { continue; }
            let center = shield.world_center(ai_transform);
            let dist_to_ship = missile_pos.distance(center);

            if shield.is_up() && dist_to_ship < shield.radius && shield.covers_arc(missile_pos - center) {
                shield.absorb(missile.damage);
                spawn_explosion(&mut commands, &fx, missile_pos, missile.blast_radius * 0.7, Color::srgb(0.5, 0.8, 1.0));
                commands.entity(missile_entity).despawn();
                continue 'missiles;
            }

            if dist_to_ship < shield.radius + 60.0 {
                // Blast damage, but armour gets a say.
                //
                // This used to apply the FULL warhead to every module inside
                // the radius, ignoring plating, hull and distance alike — the
                // one place in combat where armour did not matter, so the
                // answer to a heavily belted Pressure King was the same as
                // for a bare Rust Swarm. Now each block soaks by its own
                // thickness through the same resolve_impact the guns use,
                // and damage falls off across the radius.
                //
                // Pass-through is high: a shaped charge is meant to defeat
                // plate that stops a bullet. High, not total — plating you
                // stand behind should still be worth having.
                const WARHEAD_PASS_THROUGH: f32 = 0.65;
                let radius = missile.blast_radius.max(50.0);
                let mut total_damage = 0.0;
                let mut hit_any = false;
                for child in children.iter() {
                    if let Ok((mut module, gt)) = ai_module_query.get_mut(child) {
                        let d = missile_pos.distance(gt.translation().truncate());
                        if d >= radius { continue; }
                        // Linear falloff: at the rim a warhead scorches, at
                        // the centre it guts.
                        let share = missile.damage * (1.0 - d / radius);
                        let block = block_query.get(child).copied()
                            .unwrap_or_else(|_| crate::building::Block::module(IVec2::ZERO));
                        let impact = crate::combat::impact::resolve_impact(
                            share,
                            &block,
                            1.0,
                            // A detonation has no incoming angle to speak of —
                            // it envelops the block rather than striking a
                            // face, so it never skips off.
                            crate::combat::impact::Obliquity::HEAD_ON,
                            Some(WARHEAD_PASS_THROUGH),
                        );
                        let dealt = impact.to_block + impact.through;
                        if dealt <= 0.0 { continue; }
                        module.health = (module.health - dealt).max(0.0);
                        total_damage += dealt;
                        hit_any = true;
                    }
                }
                if hit_any {
                    spawn_explosion(&mut commands, &fx, missile_pos, missile.blast_radius, Color::srgb(1.0, 0.5, 0.1));
                    spawn_floating_damage(&mut commands, missile_pos, total_damage, Color::srgb(1.0, 0.4, 0.1));
                    // amount: 0.0 — damage already applied directly above to
                    // every module in the blast radius. process_ai_ship_damage_system
                    // used to re-apply this same total again via its own
                    // distance-sort, double-damaging and often hitting
                    // different blocks than the ones actually in the blast.
                    ai_damage_events.write(crate::events::AiShipDamaged {
                        target: ai_entity,
                        source: crate::events::DamageSource::Explosion,
                        amount: 0.0,
                        position: Some(missile_pos),
                        direction: None,
                        attacker: owner_ship,
                    });
                    commands.entity(missile_entity).despawn();
                    continue 'missiles;
                }
            }
        }

        for (creature_entity, _) in creature_grid.0.nearby(missile_pos, MAX_CREATURE_HIT_RADIUS) {
            let Ok((creature_transform, mut creature)) = creature_query.get_mut(creature_entity) else { continue };
            if creature.health <= 0.0 { continue; }

            let creature_pos = creature_transform.translation.truncate();
            let dist = missile_pos.distance(creature_pos);
            let hit_radius = match creature.creature_type {
                CreatureType::Leviathan => 100.0,
                _ => 40.0,
            };

            if dist > hit_radius { continue; }

            // IMPACT!
            // Direct hit damage
            creature.health -= missile.damage;

            // Blast radius damage to nearby creatures
            // (handled by the explosion effect — could expand later)

            // Explosion visual
            spawn_explosion(&mut commands, &fx, missile_pos, missile.blast_radius, Color::srgb(1.0, 0.5, 0.1));
            spawn_floating_damage(&mut commands, missile_pos, missile.damage, Color::srgb(1.0, 0.3, 0.1));

            commands.entity(missile_entity).despawn();
            break;
        }
    }
}
