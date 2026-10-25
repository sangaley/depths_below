use bevy::prelude::*;

use crate::components::*;
use super::components::*;

/// Top speed for harbour traffic inside the harbour.
const HARBOUR_SPEED: f32 = 350.0;
/// How far past a starport's hull the harbour reaches: the guard fleet
/// patrols within 8 km of it, convoys berth 4 km off it.
const HARBOUR_REACH: f32 = 12_000.0;
/// Past the harbour the limit lifts gradually (u/s per unit out), so a fleet
/// leaving it speeds up instead of lurching.
const HARBOUR_EASE: f32 = 0.25;

/// Speed limit for a ship `distance` from the centre of a starport whose hull
/// reaches `extent`. A Gilded Throne patrol at 2,000 u/s wheeled its formation
/// into itself and shoved hulls into whatever sat by the station.
pub fn harbour_speed_cap(distance: f32, extent: f32) -> f32 {
    HARBOUR_SPEED + (distance - extent - HARBOUR_REACH).max(0.0) * HARBOUR_EASE
}

/// Top speed per square root of thrust-per-block. A ship's speed comes from
/// how much engine it has for its size, like the player's thrust over mass.
/// It used to be the raw sum of its engines' thrust, which never looked at
/// the hull: the fleet classes carry 5-16 engines, and a 160-block raider
/// with five of them ran at 2,500 u/s -- 3,300 fleeing -- and a throne barge
/// at 5,800. The root squeezes the range: a heavy hull with little engine is
/// slow, a light one with lots is quick, and neither is absurd.
const AI_SPEED_PER_ROOT_TPB: f32 = 180.0;

/// No AI ship goes faster than this, whatever it's doing.
const AI_MAX_SPEED: f32 = 1_100.0;

/// Base speed for a ship with `thrust` from its live engines over `blocks`.
/// The faction's character is applied softened (its root): the old
/// multipliers were tuned against raw thrust and would stretch the range
/// back out.
pub fn ai_top_speed(thrust: f32, blocks: usize, faction_mult: f32) -> f32 {
    let per_block = thrust / blocks.max(1) as f32;
    AI_SPEED_PER_ROOT_TPB * per_block.max(0.0).sqrt() * faction_mult.max(0.0).sqrt()
}

/// How fast an AI ship can swing its nose round, in rad/s, by size. Small
/// ships turn quicker than big ones; nothing whips about. Turning used to
/// ease toward the target heading with no limit at all, so whenever the
/// heading it wanted changed -- a target circling, a formation slot moving,
/// a swerve round a rock -- the ship snapped across in a fraction of a
/// second, and a ship fighting at close range jittered left and right. The
/// player's own hull turns ~0.86 rad/s.
pub fn ai_turn_rate(blocks: usize) -> f32 {
    (19.0 / (blocks.max(1) as f32).sqrt()).clamp(0.35, 1.8)
}

/// Turn `rotation` toward `target_angle`, easing in but never faster than
/// `max_rate` rad/s.
fn turn_toward(rotation: Quat, target_angle: f32, blend: f32, max_rate: f32, dt: f32) -> Quat {
    let current = rotation.to_euler(EulerRot::ZYX).0;
    let diff = (target_angle - current + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    let step = (diff * blend).clamp(-max_rate * dt, max_rate * dt);
    Quat::from_rotation_z(current + step)
}

/// DEPTHS_STANDOFF_HULLS=1 (filming, for now): hold the firing distance hull
/// to hull, adding the target's own size -- otherwise a big target is hugged.
fn target_reach(target: Option<&AiShipTarget>, hulls: &Query<&crate::ship::collision::Collider>) -> f32 {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var("DEPTHS_STANDOFF_HULLS").ok().as_deref() == Some("1")) {
        return 0.0;
    }
    target.and_then(|t| t.entity).and_then(|e| hulls.get(e).ok()).map_or(0.0, |c| c.bound_radius)
}

/// Where a ship heading for a point `to_dest` away should point its nose.
///
/// Far off, at the point. Close in, the bearing to the point swings with every
/// few units of error, so an escort sitting on its formation slot -- the slot
/// moving under it as the leader turns -- nodded its nose left and right all
/// the time. Near the point the ship faces the way it is actually moving, and
/// holds its heading when it isn't moving, easing back onto the bearing as the
/// distance opens.
pub(crate) fn heading_for(to_dest: Vec2, velocity: Vec2, current: f32) -> f32 {
    let dist = to_dest.length();
    let bearing = to_dest.y.atan2(to_dest.x);
    if dist >= NEAR {
        return bearing;
    }
    // Moving briskly toward the point: face the way it's moving. Stopped, or
    // drifting past the point after overshooting it: keep the heading it has
    // rather than spinning round to face backwards.
    let toward = velocity.length() > 150.0 && velocity.dot(to_dest) > 0.0;
    let base = if toward { velocity.y.atan2(velocity.x) } else { current };
    let k = (dist / NEAR).clamp(0.0, 1.0);
    base + wrap(bearing - base) * k * k
}

/// Within this of its point, a ship stops aiming its nose at the point.
const NEAR: f32 = 1_200.0;

fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Where an escort points: the formation's heading near its slot, the
/// bearing to the slot when it's far off and has ground to make up.
pub(crate) fn escort_heading(face: f32, to_dest: Vec2, velocity: Vec2, current: f32) -> f32 {
    const FAR: f32 = 3_000.0;
    let dist = to_dest.length();
    if dist >= FAR {
        return heading_for(to_dest, velocity, current);
    }
    let k = ((dist - NEAR) / (FAR - NEAR)).clamp(0.0, 1.0);
    let far = to_dest.y.atan2(to_dest.x);
    face + wrap(far - face) * k * k
}

/// Ease the heading a ship steers for toward `raw`, so a target that flips
/// back and forth -- a slot jittering round the ship, avoidance picking a
/// side and dropping it -- averages out instead of wagging the nose.
pub(crate) fn smooth_heading(prev: Option<f32>, raw: f32, dt: f32) -> f32 {
    const SETTLE: f32 = 0.5; // seconds
    match prev {
        None => raw,
        Some(p) => p + wrap(raw - p) * (1.0 - (-dt / SETTLE).exp()),
    }
}

/// Moves AI ships toward their nav destination, reading child Engine components for thrust.
pub fn ai_ship_movement_system(
    time: Res<Time>,
    mut ai_ships: Query<(
        Entity,
        &mut Transform,
        &mut Velocity,
        &AiShipBehavior,
        &mut AiShipNav,
        &AiShipState,
        &AiShipType,
        &Children,
        Option<&crate::ship::collision::Collider>,
        Option<&super::fleet::FleetMember>,
        Has<super::standing_spawns::PlayerEscort>,
        Option<&AiShipTarget>,
    ), With<AiShip>>,
    // The target's own size, for DEPTHS_STANDOFF_HULLS (see below).
    hulls: Query<&crate::ship::collision::Collider>,
    engine_query: Query<(&Engine, &Module, &OwnedByAiShip)>,
    weapon_query: Query<(&Weapon, &Module, &OwnedByAiShip), Without<Engine>>,
    // Terrain look-ahead so ships swing around rocks/stations on their way
    // instead of grinding along everything that's solid now.
    collider_grid: Res<crate::ship::collision::ColliderGrid>,
    terrain: Query<
        (&Transform, &crate::ship::collision::Collider),
        crate::ship::collision::TerrainFilter,
    >,
    // The player's hull is in the way too: a fleet crossing near you flowed
    // into it, and an escort could wedge itself between the Pincer's claws.
    player: Query<(&Transform, &crate::ship::collision::Collider), (With<Ship>, Without<AiShip>)>,
    faction_stations: Res<crate::diplomacy::stations::FactionStations>,
) {
    let dt = time.delta_secs();
    let player_obstacle = player.single().ok().map(|(tf, c)| {
        (crate::ship::collision::world_point(tf, c.bound_center), c.bound_radius)
    });

    for (_entity, mut transform, mut velocity, behavior, mut nav, state, ship_type, children, collider, member, escort, target) in ai_ships.iter_mut() {
        if *behavior == AiShipBehavior::Dead || state.is_destroyed {
            velocity.0 = Vec2::ZERO;
            continue;
        }

        // Sum thrust from child engines — tracking the undamaged total too,
        // so we know how crippled the drive section is.
        let mut total_thrust = 0.0_f32;
        let mut max_possible_thrust = 0.0_f32;
        for child in children.iter() {
            if let Ok((engine, module, _owned)) = engine_query.get(child) {
                max_possible_thrust += engine.thrust;
                if module.is_active && module.health > 0.0 {
                    let efficiency = module.health / module.max_health;
                    total_thrust += engine.thrust * efficiency;
                }
            }
        }

        // DEATH SPIRALS — a ship with mangled engines can't hold a heading.
        // Thrust asymmetry shows up as a slow sinusoidal weave injected into
        // its steering; the worse the drive damage, the wider and drunker
        // the wander. Phase from the entity index so a damaged squadron
        // doesn't wobble in lockstep.
        let engine_integrity = if max_possible_thrust > 0.0 {
            (total_thrust / max_possible_thrust).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let wobble_angle = if engine_integrity < 0.85 {
            let hurt = 1.0 - engine_integrity;
            let phase = (_entity.to_bits() % 97) as f32;
            (time.elapsed_secs() * 2.3 + phase).sin() * hurt * hurt * 1.1
        } else {
            0.0
        };

        // Speed multiplier per behavior
        let behavior_mult = match behavior {
            AiShipBehavior::Patrolling | AiShipBehavior::Idle => 0.6,
            AiShipBehavior::FollowingTradeRoute => 0.8,
            AiShipBehavior::Engaging => 1.0,
            AiShipBehavior::Fleeing | AiShipBehavior::EvadingCreature => 1.3,
            AiShipBehavior::Salvaging => 0.3,
            AiShipBehavior::Dead => 0.0,
        };

        // In formation, the leader holds back so its escorts can keep up,
        // and an escort out of place hurries to its slot.
        let to_slot = nav.destination.map_or(0.0, |d| d.distance(transform.translation.truncate()));
        let behavior_mult = match (member, behavior) {
            (Some(m), AiShipBehavior::Patrolling) if m.leader => behavior_mult * 0.75,
            // An escort keeps up with you: you fly at full speed, it doesn't
            // dawdle at patrol pace.
            (Some(_), AiShipBehavior::Patrolling) if escort => behavior_mult * 1.6,
            (Some(_), AiShipBehavior::Patrolling) if to_slot > 1_500.0 => behavior_mult * 1.35,
            _ => behavior_mult,
        };
        // An escort settles into its slot rather than overshooting it.
        let settle = (escort || member.is_some_and(|m| !m.leader)) && *behavior == AiShipBehavior::Patrolling;

        // Faction-specific speed characteristics
        let faction_mult = match ship_type {
            AiShipType::TheSilence => 1.6,      // fastest ship in the game
            AiShipType::RecursiveKingdom => 1.3,      // fast but erratic
            AiShipType::GildedThrone => 1.2,     // quick tactical ship
            AiShipType::StellarPreserve => 0.9,      // creature-towed, moderate
            AiShipType::SynthesisCollective => 1.0,    // average
            AiShipType::BrokenChoir => 0.7,        // sluggish, damaged engines
            AiShipType::CorpseStars => 0.8,   // heavy but powerful engines
            AiShipType::TerranHegemony => 0.6,       // slow battleship
            AiShipType::EternalHegemony => 0.4,    // colossal, lumbering
            AiShipType::Shepherd => 0.35,     // barely moves, but it doesn't need to
            AiShipType::MerchantPrinces => 0.9,   // laden freight
            AiShipType::FreeHaulersCompact => 0.8, // pushing a load
        };

        let blocks = children.len();
        let max_speed = (ai_top_speed(total_thrust, blocks, faction_mult) * behavior_mult).min(AI_MAX_SPEED);
        let turn_rate = ai_turn_rate(blocks);
        // Harbour traffic goes slow. A ship going about its business near a
        // faction starport keeps to harbour speed; one fighting or running
        // for its life doesn't.
        let harbour_bound = matches!(
            behavior,
            AiShipBehavior::Patrolling | AiShipBehavior::Idle | AiShipBehavior::FollowingTradeRoute | AiShipBehavior::Salvaging
        );
        let max_speed = match faction_stations.post {
            Some((_, port, extent)) if harbour_bound => {
                max_speed.min(harbour_speed_cap(transform.translation.truncate().distance(port), extent))
            }
            _ => max_speed,
        };

        // Frame-rate-independent smoothing fraction: closes this much of the
        // remaining gap (to a target velocity or a target angle) per second.
        // The old `(80.0 * dt).min(1.0)` saturated to a full 1.0 snap at any
        // normal framerate — velocity teleported to its new value every
        // single frame while rotation eased in smoothly, so the ship visibly
        // slid in a direction its nose wasn't pointed at. This converges
        // continuously instead, same shape at 30fps or 300fps.
        let vel_blend = 1.0 - (-6.0_f32 * dt).exp();
        let turn_blend = 1.0 - (-4.5_f32 * dt).exp();

        if let Some(destination) = nav.destination {
            let pos = transform.translation.truncate();
            let own_radius = collider.map(|c| c.bound_radius).unwrap_or(300.0);
            // Room kept round your hull, wider the faster this ship is going:
            // its velocity eases onto a new heading, so a hull at 1,500 u/s
            // carries on a few hundred units before a swerve takes.
            let player_obstacle = player_obstacle
                .map(|(center, radius)| (center, radius + (velocity.0.length() * 0.3).clamp(200.0, 800.0)));
            // A formation slot or waypoint that lands on you is taken just
            // outside your hull instead. A patrol sweeping past a parked
            // ship had escorts flying their slots straight through it.
            let destination = match player_obstacle {
                Some((center, radius)) if *behavior != AiShipBehavior::Engaging => {
                    let keep = radius + own_radius;
                    if destination.distance(center) < keep {
                        center + (destination - center).normalize_or((pos - center).normalize_or(Vec2::X)) * keep
                    } else {
                        destination
                    }
                }
                _ => destination,
            };
            // Drive damage skews the perceived heading — the ship weaves
            // toward where its broken engines are dragging it.
            let to_dest = Vec2::from_angle(wobble_angle).rotate(destination - pos);
            let dist = to_dest.length();

            // Combat standoff: while engaging, hold a firing distance from the
            // target instead of flying into (and through) it. Below the band:
            // back off. Inside the band: orbit sideways. Beyond it: approach.
            // Recursive Kingdom keeps its point-blank ramming — that IS their faction,
            // regardless of what it's carrying. Everyone else holds at their
            // own longest-range active weapon (85% of its range, so they
            // fight solidly inside their own reach instead of right at the
            // ragged edge) — a ship stripped down to short-range guns closes
            // in, one carrying a sniper weapon hangs back, instead of every
            // ship in a faction using the same fixed distance regardless of
            // loadout. Falls back to the old faction defaults only if a ship
            // somehow has no active weapons (e.g. mid-repair).
            let max_weapon_range = children.iter()
                .filter_map(|c| weapon_query.get(c).ok())
                .filter(|(_, module, _)| module.is_active && module.health > 0.0)
                .map(|(weapon, _, _)| weapon.range)
                .fold(0.0_f32, f32::max);

            // A CLOSE standoff. The old 0.85×(weapon range) parked long-range
            // factions 6000-7000u out, where their slow shells never landed
            // and any approach just made them kite backwards forever — the
            // "they look at me, then run away, and never hit me" report.
            // Fights now happen at ~half weapon range, hard-capped at 3000 so
            // even a railgun ship closes to a distance where its rounds
            // actually connect and you can plainly see you're being shot at.
            // Recursive Kingdom still rams point-blank (that IS their identity).
            let standoff = if *behavior == AiShipBehavior::Engaging {
                match ship_type {
                    AiShipType::RecursiveKingdom => 0.0,
                    _ if max_weapon_range > 0.0 => (max_weapon_range * 0.55).clamp(900.0, 3000.0),
                    AiShipType::Shepherd | AiShipType::EternalHegemony => 3000.0,
                    AiShipType::TerranHegemony | AiShipType::CorpseStars => 2600.0,
                    AiShipType::GildedThrone => 2200.0,
                    _ => 1800.0,
                }
            } else {
                0.0
            };
            // Measured from the ship's centre, so a hull kilometres across
            // held its "distance" with its own bow in your hull, shoving you
            // across the system. The standoff starts where its hull ends.
            let standoff = if standoff > 0.0 {
                standoff + collider.map_or(0.0, |c| c.bound_radius) + target_reach(target, &hulls)
            } else {
                0.0
            };

            if standoff > 0.0 && dist < standoff * 1.3 && dist > 1.0 {
                let direction = to_dest / dist; // toward the player
                // Hold and shoot instead of orbiting. Each ship parks at its own
                // range (jittered so they don't all sit at one perfect radius)
                // and STATION-KEEPS there: it only closes if pushed too far out,
                // eases back only if crowded too close — otherwise it holds and
                // fires. No perpetual circling.
                let bits = _entity.to_bits();
                let jitter = 0.8 + ((bits % 13) as f32 / 13.0) * 0.35; // 0.8..1.15
                let hold = standoff * jitter;
                let radial = if dist > hold * 1.1 {
                    direction * max_speed * 0.5 // drift in
                } else if dist < hold * 0.7 {
                    -direction * max_speed * 0.4 // ease back
                } else {
                    Vec2::ZERO // in the pocket — hold position
                };
                // A faint sideways sway so it's not a frozen statue — a small
                // fraction of the old orbit, not a full circle.
                let phase = (bits % 211) as f32;
                let sway_sign = if bits & 1 == 0 { 1.0 } else { -1.0 };
                let tangent = Vec2::new(-direction.y, direction.x) * sway_sign;
                let sway = tangent * max_speed * 0.1 * (time.elapsed_secs() * 0.4 + phase).sin();
                velocity.0 = velocity.0.lerp(radial + sway, vel_blend);

                // Face the player — you're fine with this, and it reads natural
                // for a ship holding station and trading fire.
                let target_angle = direction.y.atan2(direction.x);
                nav.steer = Some(target_angle);
                transform.rotation = turn_toward(transform.rotation, target_angle, turn_blend, turn_rate, dt);
            } else if dist > 5.0 {
                // Bend the heading around any terrain blocking the way.
                let direction = crate::ship::collision::steer_around(
                    &collider_grid,
                    &terrain,
                    pos,
                    to_dest / dist,
                    dist,
                    own_radius,
                );
                // Not while fighting you: a ship engaging holds its own
                // standoff, and a rammer means to hit.
                let direction = match player_obstacle {
                    Some((center, radius)) if *behavior != AiShipBehavior::Engaging => {
                        crate::ship::collision::steer_clear_of(pos, direction, dist, own_radius, center, radius)
                    }
                    _ => direction,
                };

                // Gradually accelerate toward desired velocity
                let arrive = if settle { (dist / 600.0).min(1.0) } else { 1.0 };
                let desired_vel = direction * max_speed * arrive;
                velocity.0 = velocity.0.lerp(desired_vel, vel_blend);

                // Face where it's going -- or, close to the point, where it's
                // actually moving (heading_for); an escort near its slot
                // faces the way the formation does. Smoothed (smooth_heading).
                let current = transform.rotation.to_euler(EulerRot::ZYX).0;
                let raw = match nav.face.filter(|_| settle) {
                    Some(face) => escort_heading(face, direction * dist, velocity.0, current),
                    None => heading_for(direction * dist, velocity.0, current),
                };
                let target_angle = smooth_heading(nav.steer, raw, dt);
                nav.steer = Some(target_angle);
                transform.rotation = turn_toward(transform.rotation, target_angle, turn_blend, turn_rate, dt);
            } else {
                // Near destination, slow down
                velocity.0 *= (1.0 - 3.0 * dt).max(0.0);
            }
        } else {
            // No destination, drift to stop
            velocity.0 *= (1.0 - 2.0 * dt).max(0.0);
        }

        // Apply drag
        let drag = 0.5 * dt;
        velocity.0 *= 1.0 - drag;

        // Apply velocity to position
        transform.translation.x += velocity.0.x * dt;
        transform.translation.y += velocity.0.y * dt;
    }
}

/// Updates AI ship depth from Y position
pub fn ai_thruster_system(
    mut ai_ships: Query<(&Transform, &mut AiShipState), With<AiShip>>,
) {
    for (transform, mut state) in ai_ships.iter_mut() {
        // Depth = negative Y (deeper = more negative Y = higher depth value)
        state.depth = (-transform.translation.y / 10.0).max(0.0);
    }
}

/// Consumes fuel based on engine activity
pub fn ai_fuel_system(
    time: Res<Time>,
    mut ai_ships: Query<(&mut AiShipState, &AiShipBehavior, &Children), With<AiShip>>,
    engine_query: Query<(&Engine, &Module, &OwnedByAiShip)>,
) {
    let dt = time.delta_secs();

    for (mut state, behavior, children) in ai_ships.iter_mut() {
        if *behavior == AiShipBehavior::Dead {
            continue;
        }

        let mut fuel_consumption = 0.0_f32;
        for child in children.iter() {
            if let Ok((engine, module, _)) = engine_query.get(child) {
                if module.is_active && module.health > 0.0 {
                    fuel_consumption += engine.fuel_consumption;
                }
            }
        }

        // Reduce consumption when idle/patrolling
        let consumption_mult = match behavior {
            AiShipBehavior::Idle => 0.1,
            AiShipBehavior::Patrolling | AiShipBehavior::Salvaging => 0.5,
            AiShipBehavior::FollowingTradeRoute => 0.7,
            _ => 1.0,
        };

        state.fuel = (state.fuel - fuel_consumption * consumption_mult * dt).max(0.0);
    }
}

#[cfg(test)]
mod harbour_tests {
    use super::*;

    /// Inside the harbour everyone keeps to harbour speed; the limit lifts
    /// smoothly outside it, and is gone a system's width away.
    #[test]
    fn harbour_traffic_slows_near_the_starport() {
        let extent = 2_300.0;
        assert_eq!(harbour_speed_cap(0.0, extent), HARBOUR_SPEED);
        assert_eq!(harbour_speed_cap(extent + HARBOUR_REACH, extent), HARBOUR_SPEED);
        let mut last = HARBOUR_SPEED;
        for step in 1..100 {
            let cap = harbour_speed_cap(extent + HARBOUR_REACH + step as f32 * 200.0, extent);
            assert!(cap > last && cap - last <= 200.0 * HARBOUR_EASE + 0.01, "the limit jumps at {step}");
            last = cap;
        }
        assert!(harbour_speed_cap(100_000.0, extent) > 10_000.0);
    }
}

#[cfg(test)]
mod heading_tests {
    use super::*;

    /// An escort riding on its slot: the slot jitters a few dozen units round
    /// it every tick while the fleet cruises east. Its nose must stay east,
    /// not swing to each new bearing.
    #[test]
    fn an_escort_on_its_slot_does_not_nod() {
        let cruising = Vec2::new(300.0, 0.0);
        for (dx, dy) in [(30.0, 25.0), (-20.0, 30.0), (25.0, -30.0), (-30.0, -20.0)] {
            let h = heading_for(Vec2::new(dx, dy), cruising, 0.0);
            assert!(h.abs() < 0.05, "slot at ({dx},{dy}) swung the nose to {h}");
        }
    }

    /// Stopped near its point, a ship holds the heading it has.
    #[test]
    fn a_ship_at_rest_by_its_point_holds_heading() {
        let h = heading_for(Vec2::new(0.0, 40.0), Vec2::ZERO, 0.7);
        assert!((h - 0.7).abs() < 0.01, "{h}");
    }

    /// An escort sitting on its slot faces the formation's way, wherever the
    /// slot jitters to.
    #[test]
    fn an_escort_near_its_slot_faces_the_formation() {
        let face = 1.0;
        for to_slot in [Vec2::new(200.0, -150.0), Vec2::new(-300.0, 250.0), Vec2::new(0.0, 900.0)] {
            let h = escort_heading(face, to_slot, Vec2::new(-80.0, 30.0), 0.2);
            assert!((h - face).abs() < 0.05, "slot {to_slot} pointed it to {h}");
        }
        // Far behind, it heads for the slot.
        let far = escort_heading(face, Vec2::new(0.0, -6_000.0), Vec2::ZERO, 0.0);
        assert!((far + std::f32::consts::FRAC_PI_2).abs() < 1e-3, "{far}");
    }

    /// A target flipping between two sides every frame averages out: the
    /// heading steered for barely moves.
    #[test]
    fn a_flipping_target_does_not_wag_the_steering() {
        let mut steer = Some(0.0);
        let mut lo = 0.0f32;
        let mut hi = 0.0f32;
        for k in 0..240 {
            let raw = if k % 2 == 0 { 0.6 } else { -0.6 };
            let s = smooth_heading(steer, raw, 1.0 / 60.0);
            steer = Some(s);
            if k > 60 {
                lo = lo.min(s);
                hi = hi.max(s);
            }
        }
        assert!(hi - lo < 0.05, "steering swung {lo}..{hi}");
    }

    /// Far from the point, it still faces the point.
    #[test]
    fn far_off_it_faces_the_point() {
        let h = heading_for(Vec2::new(0.0, 5_000.0), Vec2::new(300.0, 0.0), 0.0);
        assert!((h - std::f32::consts::FRAC_PI_2).abs() < 1e-4, "{h}");
    }
}

#[cfg(test)]
mod ai_speed_tests {
    use super::*;

    /// The Silence's five-engine Listener: 160 blocks, 2000 thrust, the
    /// fastest faction. It ran at 2,560 u/s; it stays the quickest thing out
    /// there but under the ceiling, and fleeing (x1.3) too.
    #[test]
    fn the_fastest_raider_stays_under_the_ceiling() {
        let engage = ai_top_speed(2000.0, 161, 1.6);
        assert!(engage > 600.0 && engage * 1.3 <= AI_MAX_SPEED * 1.2, "{engage}");
    }

    /// A barge of 1,444 blocks with fifteen engines is not a racer.
    #[test]
    fn a_big_hull_is_slower_than_a_light_one_with_the_same_engines() {
        let barge = ai_top_speed(6000.0, 1444, 1.2);
        let courier = ai_top_speed(2000.0, 164, 1.2);
        assert!(barge < courier, "{barge} vs {courier}");
        assert!(barge < 600.0, "barge {barge}");
    }

    #[test]
    fn small_ships_turn_quicker_but_nothing_whips_round() {
        assert!(ai_turn_rate(150) > ai_turn_rate(1500));
        assert!(ai_turn_rate(10) <= 1.8 && ai_turn_rate(100_000) >= 0.35);
    }

    /// A half-turn target is reached at the capped rate, not in one frame.
    #[test]
    fn a_turn_is_capped_per_second() {
        let dt = 1.0 / 60.0;
        let mut rot = Quat::IDENTITY;
        for _ in 0..30 {
            rot = turn_toward(rot, std::f32::consts::PI * 0.9, 1.0, 1.0, dt);
        }
        let angle = rot.to_euler(EulerRot::ZYX).0;
        assert!((angle - 0.5).abs() < 0.02, "half a second at 1 rad/s turns 0.5 rad, got {angle}");
    }
}
