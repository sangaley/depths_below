//! Lasers built from parts (the Laser Lab preview's rules, tools/laser-lab).
//!
//! A Laser Core is the hub; Couplers extend it. Cavities touching the core,
//! a coupler or a counted cavity store charge; Pumps beside a counted cavity
//! fill them from ship power. Q-switches, Frequency Doublers and
//! Destabilisers touching the core or a coupler shape what comes out:
//!
//! - Q-switches: none is one steady beam (0.75 damage a charge, intensity
//!   60); 1-4 pulse at 1, 2, 4, 8 a second (1.0 a charge, intensity 40).
//! - Doublers: +15 intensity each (four count); intensity against a block's
//!   armour decides how much of the beam burns in.
//! - Destabilisers: the bank lets go of `1 - 0.75^(1 + n)` of its charge a
//!   second while firing.
//!
//! Emitters fire it: on the hull touching the core or a coupler, or on a
//! turret whose ring has a Beam Relay beside it that touches them. Lenses in
//! a straight line in front of an emitter keep the beam strong further out.
//! The beam is instant and stops at the first block on its line: a steady
//! beam works at one block until it goes, a pulse that kills a block carries
//! what's left into the next, up to six deep. An emitter on point defence
//! fires only at missiles its ship's warners have spotted.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;

use crate::components::{DestroyedModule, HullSegment, Module, ModuleType, Ship};

/// Charge one cavity stores.
pub const CAVITY_CHARGE: f32 = 100.0;
/// Charge a second one pump adds.
pub const PUMP_RATE: f32 = 12.0;
/// What a pump draws from the ship while its bank fills: lasers are paid
/// for in power. A full bank's pumps only idle.
pub const PUMP_POWER_MW: f32 = 60.0;
pub const PUMP_IDLE_MW: f32 = 2.0;
/// How far point defence reaches for a missile.
pub const PD_REACH: f32 = 3500.0;
/// How deep a pulse carries.
pub const PULSE_DEPTH: usize = 6;
/// Cells are 66 units across.
const CELL: f32 = 66.0;

// ------------------------------------------------------------ the rules ---

/// What a bank's parts make of it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BankStats {
    pub storage: f32,
    pub drain: f32,
    /// Pulses a second; 0 is a steady beam.
    pub shots: u32,
    pub damage_per_charge: f32,
    pub intensity: f32,
    pub wavelength_nm: f32,
}

pub fn bank_stats(cavities: u32, q_switches: u32, doublers: u32, destabilisers: u32) -> BankStats {
    let q = q_switches.min(4);
    let d = doublers.min(4);
    BankStats {
        storage: CAVITY_CHARGE * cavities as f32,
        drain: 1.0 - 0.75f32.powi(destabilisers as i32 + 1),
        shots: [0, 1, 2, 4, 8][q as usize],
        damage_per_charge: if q == 0 { 0.75 } else { 1.0 },
        intensity: if q == 0 { 60.0 } else { 40.0 } + 15.0 * d as f32,
        wavelength_nm: 1064.0 / (1.0 + d as f32),
    }
}

/// How much of the beam is left after `distance` units.
pub fn reaching(distance: f32, lenses: u32, point_defence: bool) -> f32 {
    let l = 3000.0 * (1.0 + lenses as f32).sqrt() * if point_defence { 2.0 } else { 1.0 };
    (-distance / l).exp()
}

/// Furthest a laser bothers firing: where 5% of it is left.
pub fn max_range(lenses: u32) -> f32 {
    3.0 * 3000.0 * (1.0 + lenses as f32).sqrt()
}

/// A hull-mounted emitter's arc, degrees either side of where it faces.
pub fn hull_half_arc_deg(lenses: u32) -> f32 {
    150.0 / (4.0 + lenses as f32)
}

/// Share of a beam that burns into a block with this armour.
pub fn burn_share(intensity: f32, thickness: f32) -> f32 {
    if thickness <= 0.0 { 1.0 } else { (intensity / thickness).clamp(0.1, 1.0) }
}

/// A beam's colour from its wavelength: infrared drawn red, then green,
/// violet, white-violet.
pub fn beam_colour(nm: f32) -> Color {
    if nm >= 1000.0 {
        Color::srgb(1.0, 0.28, 0.22)
    } else if nm >= 500.0 {
        Color::srgb(0.35, 1.0, 0.47)
    } else if nm >= 330.0 {
        Color::srgb(0.59, 0.43, 1.0)
    } else if nm >= 250.0 {
        Color::srgb(0.75, 0.6, 1.0)
    } else {
        Color::srgb(0.92, 0.85, 1.0)
    }
}

// ------------------------------------------------------------ assembly ---

/// The parts of one bank, found from its core over one ship's hull deck.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BankLayout {
    pub network: HashSet<IVec2>,
    pub cavities: Vec<IVec2>,
    pub pumps: Vec<IVec2>,
    pub q_switches: u32,
    pub doublers: u32,
    pub destabilisers: u32,
    pub emitters: Vec<IVec2>,
    pub relays: Vec<IVec2>,
}

const N4: [IVec2; 4] = [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y];

/// Everything that belongs to the core at `core`, by cell.
pub fn lay_out_bank(cells: &HashMap<IVec2, ModuleType>, core: IVec2) -> BankLayout {
    let kind = |c: IVec2| cells.get(&c).copied();
    let mut out = BankLayout::default();
    out.network.insert(core);
    let mut queue = vec![core];
    while let Some(c) = queue.pop() {
        for d in N4 {
            let n = c + d;
            if kind(n) == Some(ModuleType::LaserCoupler) && out.network.insert(n) {
                queue.push(n);
            }
        }
    }
    let touches_net = |c: IVec2| N4.iter().any(|d| out.network.contains(&(c + *d)));
    let mut cav: HashSet<IVec2> = HashSet::new();
    let mut queue: Vec<IVec2> = cells
        .iter()
        .filter(|(c, t)| **t == ModuleType::LaserCavity && touches_net(**c))
        .map(|(c, _)| *c)
        .collect();
    cav.extend(queue.iter().copied());
    while let Some(c) = queue.pop() {
        for d in N4 {
            let n = c + d;
            if kind(n) == Some(ModuleType::LaserCavity) && cav.insert(n) {
                queue.push(n);
            }
        }
    }
    for (c, t) in cells {
        match t {
            ModuleType::LaserPump if N4.iter().any(|d| cav.contains(&(*c + *d))) => out.pumps.push(*c),
            ModuleType::QSwitch if touches_net(*c) => out.q_switches += 1,
            ModuleType::FrequencyDoubler if touches_net(*c) => out.doublers += 1,
            ModuleType::LaserDestabiliser if touches_net(*c) => out.destabilisers += 1,
            ModuleType::LaserEmitter if touches_net(*c) => out.emitters.push(*c),
            ModuleType::BeamRelay if touches_net(*c) => out.relays.push(*c),
            _ => {}
        }
    }
    out.cavities = cav.into_iter().collect();
    out.cavities.sort_by_key(|c| (c.x, c.y));
    out.pumps.sort_by_key(|c| (c.x, c.y));
    out.emitters.sort_by_key(|c| (c.x, c.y));
    out
}

/// Lenses in a straight line out of the front of an emitter.
pub fn count_lenses(cells: &HashMap<IVec2, ModuleType>, emitter: IVec2, facing: IVec2) -> u32 {
    let mut n = 0;
    let mut c = emitter + facing;
    while cells.get(&c) == Some(&ModuleType::LaserLens) {
        n += 1;
        c += facing;
    }
    n
}

/// A laser, on its core.
#[derive(Component, Clone, Debug, Default)]
pub struct LaserBank {
    pub stats: BankStats,
    pub charge: f32,
    pub pumps: Vec<Entity>,
    pub emitters: Vec<Entity>,
    /// What it has let go since its last pulse.
    pub pending: f32,
    pub pulse_t: f32,
    pub cavities: u32,
}

/// An emitter linked to a bank.
#[derive(Component, Clone, Copy, Debug)]
pub struct LaserEmitter {
    pub bank: Entity,
    pub lenses: u32,
}

/// Work every ship's lasers out again when its layout changes.
#[allow(clippy::type_complexity)]
pub fn assemble_lasers(
    mut commands: Commands,
    ships: Query<(Entity, &crate::building::layout::LayoutVersion, Option<&Children>), Or<(With<Ship>, With<crate::ai_ship::components::AiShip>)>>,
    turrets: Query<(&crate::building::layout::LayoutVersion, &super::turret_deck::TurretBody, Option<&Children>)>,
    modules: Query<(Entity, &Module), (Without<DestroyedModule>, Without<super::turret_deck::PendingTurretMount>)>,
    rings: Query<&Module>,
    mut banks: Query<&mut LaserBank>,
    emitters: Query<Entity, With<LaserEmitter>>,
    epoch: Res<crate::building::layout::LayoutEpoch>,
    mut seen: Local<Option<crate::building::layout::LayoutEpoch>>,
    mut built: Local<crate::building::layout::BuiltFrom>,
) {
    if *seen == Some(*epoch) {
        return;
    }
    *seen = Some(*epoch);
    for (ship, version, children) in ships.iter() {
        let on_it: Vec<Entity> = children.into_iter().flatten().copied().filter(|c| turrets.contains(*c)).collect();
        let stale = built.is_stale(ship, version) || on_it.iter().any(|t| turrets.get(*t).is_ok_and(|(v, _, _)| built.is_stale(*t, v)));
        if !stale {
            continue;
        }
        built.mark(ship, version);
        for t in &on_it {
            if let Ok((v, _, _)) = turrets.get(*t) {
                built.mark(*t, v);
            }
        }
        let laser = |c: &Entity| modules.get(*c).ok().filter(|(_, m)| m.module_type.category() == crate::components::ModuleCategory::Lasers);
        let hull: Vec<(Entity, &Module)> = children.into_iter().flatten().filter_map(laser).collect();
        let cells: HashMap<IVec2, ModuleType> = hull.iter().map(|(_, m)| (m.grid_position, m.module_type)).collect();
        let at: HashMap<IVec2, Entity> = hull.iter().map(|(e, m)| (m.grid_position, *e)).collect();
        // Unlink every emitter first; those still fed are linked again below.
        for e in hull.iter().map(|(e, _)| *e).filter(|e| emitters.contains(*e)) {
            commands.entity(e).try_remove::<LaserEmitter>();
        }
        let turret_parts: Vec<(Entity, &super::turret_deck::TurretBody, Vec<(Entity, &Module)>)> = on_it
            .iter()
            .filter_map(|t| turrets.get(*t).ok().map(|(_, body, kids)| (*t, body, kids.into_iter().flatten().filter_map(laser).collect())))
            .collect();
        for (_, _, parts) in &turret_parts {
            for (e, _) in parts {
                if emitters.contains(*e) {
                    commands.entity(*e).try_remove::<LaserEmitter>();
                }
            }
        }
        for (core, core_module) in hull.iter().filter(|(_, m)| m.module_type == ModuleType::LaserCore) {
            let layout = lay_out_bank(&cells, core_module.grid_position);
            let stats = bank_stats(layout.cavities.len() as u32, layout.q_switches, layout.doublers, layout.destabilisers);
            let mut linked = Vec::new();
            for cell in &layout.emitters {
                let Some(e) = at.get(cell) else { continue };
                let facing = hull.iter().find(|(x, _)| x == e).map(|(_, m)| m.rotation.facing_offset()).unwrap_or(IVec2::Y);
                commands.entity(*e).try_insert(LaserEmitter { bank: *core, lenses: count_lenses(&cells, *cell, facing) });
                linked.push(*e);
            }
            // Up to the turrets, through a relay beside each ring.
            for (_, body, parts) in &turret_parts {
                let ring_cells = rings.get(body.ring).map(super::turret_deck::ring_cells).unwrap_or_default();
                let fed = layout.relays.iter().any(|r| N4.iter().any(|d| ring_cells.contains(&(*r + *d))));
                if !fed {
                    continue;
                }
                let up: HashMap<IVec2, ModuleType> = parts.iter().map(|(_, m)| (m.grid_position, m.module_type)).collect();
                for (e, m) in parts.iter().filter(|(_, m)| m.module_type == ModuleType::LaserEmitter) {
                    let lenses = count_lenses(&up, m.grid_position, m.rotation.facing_offset());
                    commands.entity(*e).try_insert(LaserEmitter { bank: *core, lenses });
                    linked.push(*e);
                }
            }
            let pumps: Vec<Entity> = layout.pumps.iter().filter_map(|c| at.get(c).copied()).collect();
            let charge = banks.get_mut(*core).map(|b| b.charge.min(stats.storage)).unwrap_or(stats.storage);
            commands.entity(*core).try_insert(LaserBank {
                stats,
                charge,
                pumps,
                emitters: linked,
                pending: 0.0,
                pulse_t: 0.0,
                cavities: layout.cavities.len() as u32,
            });
            if super::gun_assembly::trace() {
                info!(
                    "LASER assembled {:?}: {} cavities, {} pumps, {} Q, {} doublers, {} destabilisers, {} emitters -> {:.0} charge, {} pulses/s, intensity {:.0}",
                    core, layout.cavities.len(), layout.pumps.len(), layout.q_switches, layout.doublers, layout.destabilisers,
                    layout.emitters.len(), stats.storage, stats.shots, stats.intensity
                );
            }
        }
    }
    built.retain(|e| ships.contains(e) || turrets.contains(e));
}

/// Pumps fill their bank while they're up and powered, drawing hard on the
/// ship's power until it's full. Short of power, the player's banks fill
/// only as fast as the reactors can feed everything aboard.
pub fn charge_lasers(
    time: Res<Time>,
    mut banks: Query<(&mut LaserBank, &ChildOf)>,
    mut modules: Query<&mut Module, Without<DestroyedModule>>,
    player: Query<(), With<Ship>>,
    power: Option<Res<crate::resources::PowerState>>,
) {
    let dt = time.delta_secs();
    let supplied = power.map_or(1.0, |p| {
        if p.power_balance < 0.0 && p.total_power_consumption > 0.0 {
            (p.total_power_generation / p.total_power_consumption).clamp(0.0, 1.0)
        } else {
            1.0
        }
    });
    for (mut bank, parent) in banks.iter_mut() {
        let full = bank.charge >= bank.stats.storage - 0.01;
        let draw = if full { PUMP_IDLE_MW } else { PUMP_POWER_MW };
        let mut live = 0;
        for p in bank.pumps.clone() {
            let Ok(mut m) = modules.get_mut(p) else { continue };
            if m.power_consumption != draw {
                m.power_consumption = draw;
            }
            if m.health > 0.0 && m.is_active {
                live += 1;
            }
        }
        let fed = if player.contains(parent.parent()) { supplied } else { 1.0 };
        let storage = bank.stats.storage;
        bank.charge = (bank.charge + PUMP_RATE * live as f32 * fed * dt).min(storage);
    }
}

// --------------------------------------------------------------- firing ---

/// One emitter's shot this frame, resolved by `resolve_laser_shots`.
#[derive(Message, Clone, Copy, Debug)]
pub struct LaserShot {
    pub ship: Entity,
    pub from: Vec2,
    pub dir: Vec2,
    pub energy: f32,
    pub pulse: bool,
    pub lenses: u32,
    pub stats: BankStats,
    /// On point defence: the missile it's burning.
    pub missile: Option<Entity>,
}

/// Decide which emitters fire, let the banks go, and send the shots.
#[allow(clippy::type_complexity)]
pub fn fire_lasers(
    time: Res<Time>,
    fire_state: Res<super::targeting::fire_groups::FireGroupState>,
    aim: Res<super::turret_deck::PlayerAimPoint>,
    mut banks: Query<(&mut LaserBank, &ChildOf)>,
    emitters: Query<(&LaserEmitter, &Module, &GlobalTransform, &ChildOf, Option<&super::targeting::fire_groups::FireGroup>, Has<super::targeting::fire_groups::InterceptMode>)>,
    turret_bodies: Query<&super::turret_deck::TurretBody>,
    ships: Query<(Has<Ship>, Option<&crate::ai_ship::components::AiShipTarget>, Option<&super::missile_defence::MissileWarning>)>,
    whereabouts: Query<&GlobalTransform>,
    missiles: Query<&Transform, With<super::new_projectiles::MissileProjectile>>,
    mut shots: MessageWriter<LaserShot>,
) {
    let dt = time.delta_secs();
    for (mut bank, core_parent) in banks.iter_mut() {
        let ship = core_parent.parent();
        let Ok((is_player, ai_target, warning)) = ships.get(ship) else { continue };
        // Who fires this frame, and along what line.
        let mut firing: Vec<(Vec2, Vec2, u32, Option<Entity>)> = Vec::new();
        let tell = super::gun_assembly::trace() && (time.elapsed_secs() % 2.0) < dt;
        for e in bank.emitters.clone() {
            let Ok((link, module, gt, parent, group, pd)) = emitters.get(e) else {
                if tell { info!("LASER emitter {:?}: not linked", e); }
                continue;
            };
            if module.health <= 0.0 || !module.is_active {
                if tell { info!("LASER emitter {:?}: down or unpowered (health {:.0}, active {})", e, module.health, module.is_active); }
                continue;
            }
            let facing = (gt.affine().matrix3 * Vec3::Y).truncate().normalize_or_zero();
            let muzzle = gt.translation().truncate() + facing * CELL * (link.lenses as f32 + 0.5);
            let on_turret = turret_bodies.get(parent.parent()).ok();
            // A turret has already turned to its mark: fire only once it's on.
            // A hull emitter bends its beam within its arc.
            let line_to = |mark: Vec2| -> Option<Vec2> {
                let want = (mark - muzzle).normalize_or_zero();
                if on_turret.is_some() {
                    (facing.dot(want) >= 4.0f32.to_radians().cos()).then_some(want)
                } else {
                    (facing.dot(want) >= hull_half_arc_deg(link.lenses).to_radians().cos()).then_some(want)
                }
            };
            if pd {
                // Point defence: the nearest missile the ship's warners see.
                let Some(w) = warning else { continue };
                let nearest = w
                    .spotted
                    .iter()
                    .filter_map(|m| missiles.get(*m).ok().map(|t| (*m, t.translation.truncate())))
                    .filter(|(_, p)| p.distance(muzzle) < PD_REACH)
                    .min_by(|a, b| a.1.distance(muzzle).total_cmp(&b.1.distance(muzzle)));
                if let Some((m, p)) = nearest {
                    if let Some(dir) = line_to(p) {
                        firing.push((muzzle, dir, link.lenses, Some(m)));
                    }
                }
                continue;
            }
            let mark = if is_player {
                if !fire_state.firing[group.map_or(0, |g| g.group) as usize % 4] {
                    continue;
                }
                match on_turret {
                    Some(body) => body.aim,
                    None => aim.first.or(aim.then),
                }
            } else {
                match on_turret {
                    Some(body) => body.aim,
                    None => ai_target.and_then(|t| t.entity.and_then(|x| whereabouts.get(x).ok()).map(|g| g.translation().truncate()).or(Some(t.position))),
                }
            };
            let Some(mark) = mark else {
                if tell { info!("LASER emitter {:?}: nothing to aim at", e); }
                continue;
            };
            if !is_player && mark.distance(muzzle) > max_range(link.lenses) {
                continue;
            }
            match line_to(mark) {
                Some(dir) => firing.push((muzzle, dir, link.lenses, None)),
                None => if tell {
                    info!("LASER emitter {:?}: mark off its arc (facing {:?}, mark dir {:?})", e, facing, (mark - muzzle).normalize_or_zero());
                },
            }
        }
        if tell {
            info!("LASER bank: {} emitters, {} firing, charge {:.0}/{:.0}", bank.emitters.len(), firing.len(), bank.charge, bank.stats.storage);
        }
        if firing.is_empty() {
            bank.pending = 0.0;
            continue;
        }
        let released = bank.charge * (1.0 - (1.0 - bank.stats.drain).powf(dt));
        bank.charge -= released;
        let stats = bank.stats;
        let send = |energy: f32, pulse: bool, shots: &mut MessageWriter<LaserShot>| {
            let each = energy / firing.len() as f32;
            for (from, dir, lenses, missile) in &firing {
                shots.write(LaserShot { ship, from: *from, dir: *dir, energy: each, pulse, lenses: *lenses, stats, missile: *missile });
            }
        };
        if stats.shots == 0 {
            send(released, false, &mut shots);
        } else {
            bank.pending += released;
            bank.pulse_t -= dt;
            if bank.pulse_t <= 0.0 {
                bank.pulse_t = (bank.pulse_t + 1.0 / stats.shots as f32).max(0.0);
                let energy = std::mem::take(&mut bank.pending);
                if energy > 0.5 {
                    send(energy, true, &mut shots);
                }
            }
        }
    }
}

/// A beam on screen: a glow under a hot core, fading out.
#[derive(Component)]
pub struct LaserBeamFx {
    pub life: f32,
    pub max: f32,
}

fn draw_beam(commands: &mut Commands, from: Vec2, to: Vec2, colour: Color, pulse: bool) {
    let mid = (from + to) / 2.0;
    let len = from.distance(to);
    let angle = (to - from).to_angle();
    let life = if pulse { 0.12 } else { 0.05 };
    let core = colour.mix(&Color::WHITE, 0.55);
    for (width, c, z) in [(if pulse { 16.0 } else { 10.0 }, colour.with_alpha(0.22), 0.6), (if pulse { 4.0 } else { 2.5 }, core, 0.61)] {
        commands.spawn((
            Sprite { color: c, custom_size: Some(Vec2::new(len, width)), ..default() },
            Transform { translation: mid.extend(z), rotation: Quat::from_rotation_z(angle), ..default() },
            LaserBeamFx { life, max: life },
        ));
    }
}

pub fn fade_laser_beams(time: Res<Time>, mut commands: Commands, mut beams: Query<(Entity, &mut LaserBeamFx, &mut Sprite)>) {
    for (e, mut b, mut sprite) in beams.iter_mut() {
        b.life -= time.delta_secs();
        if b.life <= 0.0 {
            commands.entity(e).try_despawn();
        } else {
            let a = sprite.color.alpha();
            sprite.color.set_alpha(a * (b.life / b.max).clamp(0.0, 1.0).sqrt().max(0.2));
        }
    }
}

/// Walk each shot's beam and burn what it meets.
#[allow(clippy::type_complexity)]
pub fn resolve_laser_shots(
    mut commands: Commands,
    mut shots: MessageReader<LaserShot>,
    mut ships: Query<(Entity, &Transform, &GlobalTransform, &Children, &mut super::shields::ShipShield, Has<Ship>), Or<(With<crate::ai_ship::components::AiShip>, With<Ship>)>>,
    turret_layers: Query<&GlobalTransform, With<super::turret_deck::TurretBody>>,
    grids: (Query<&crate::building::ShipGrid>, Query<&crate::building::RoofGrid>),
    block_query: Query<&crate::building::Block>,
    mut modules: Query<&mut Module, Without<DestroyedModule>>,
    mut hulls: Query<&mut HullSegment, Without<crate::components::HullDestroyed>>,
    mut missiles: Query<(&Transform, Option<&mut super::missiles::BuiltMissile>), With<super::new_projectiles::MissileProjectile>>,
    mut ai_damage: MessageWriter<crate::events::AiShipDamaged>,
    mut player_hits: super::new_projectiles::PlayerHitEffects,
    terrain: (Res<crate::ship::collision::ColliderGrid>, Query<(&Transform, &crate::ship::collision::Collider), crate::ship::collision::TerrainFilter>),
) {
    for shot in shots.read() {
        let colour = beam_colour(shot.stats.wavelength_nm);
        // Point defence: straight onto the missile.
        if let Some(m) = shot.missile {
            let Ok((t, built)) = missiles.get_mut(m) else { continue };
            let at = t.translation.truncate();
            let dmg = shot.energy * shot.stats.damage_per_charge * reaching(at.distance(shot.from), shot.lenses, true);
            let down = match built {
                Some(mut b) => {
                    b.hp -= dmg;
                    b.hp <= 0.0
                }
                None => true,
            };
            draw_beam(&mut commands, shot.from, at, colour, shot.pulse);
            if down {
                commands.entity(m).try_despawn();
                super::spawn_hit_effect(&mut commands, at, Color::srgb(1.0, 0.7, 0.4), 15.0);
                if super::gun_assembly::trace() {
                    info!("LASER downed a missile");
                }
            }
            continue;
        }

        let reach = max_range(shot.lenses);
        let mut end = shot.from + shot.dir * reach;
        // Rock is cover.
        if let Some((_, p)) = crate::ship::collision::terrain_segment_hit(&terrain.0, &terrain.1, shot.from, end, 0.0) {
            end = p;
        }
        // The nearest ship whose bubble or blocks the beam meets.
        struct Hit {
            ship: Entity,
            dist: f32,
            shield: bool,
            layer: Option<(Entity, bool, bool)>,
            steps: Vec<(Entity, f32)>,
        }
        let mut best: Option<Hit> = None;
        let span = shot.from.distance(end);
        for (ship, tf, gt, children, shield, _) in ships.iter() {
            if ship == shot.ship {
                continue;
            }
            let centre = shield.world_center(tf);
            let along = (centre - shot.from).dot(shot.dir);
            if along < -shield.radius || along > span + shield.radius {
                continue;
            }
            let off = (shot.from + shot.dir * along).distance(centre);
            if off > shield.radius + CELL {
                continue;
            }
            if shield.is_up() && off < shield.radius {
                let entry = along - (shield.radius * shield.radius - off * off).max(0.0).sqrt();
                let hit_at = shot.from + shot.dir * entry.max(0.0);
                if shield.covers_arc(hit_at - centre) && best.as_ref().is_none_or(|b| entry < b.dist) {
                    best = Some(Hit { ship, dist: entry.max(0.0), shield: true, layer: None, steps: Vec::new() });
                    continue;
                }
            }
            // The hull, each turret over it, each turret's roof over that:
            // the beam meets whichever it reaches first (ties go up).
            let layers = std::iter::once((ship, false, false, *gt)).chain(
                children
                    .iter()
                    .filter_map(|c| turret_layers.get(c).ok().map(|g| (c, *g)))
                    .flat_map(|(c, g)| [(c, true, false, g), (c, true, true, g)]),
            );
            for (layer, is_turret, roof, lgt) in layers {
                let grid = if roof { grids.1.get(layer).map(|r| &r.0) } else { grids.0.get(layer) };
                let Ok(grid) = grid else { continue };
                let inv = lgt.affine().inverse();
                let half = if is_turret { 0.0 } else { 33.0 };
                let to_cell = |w: Vec2| {
                    let p = inv.transform_point3(w.extend(0.0)).truncate();
                    Vec2::new(p.x / CELL, (p.y + half) / CELL)
                };
                let (a, b) = (to_cell(shot.from), to_cell(end));
                let dir_local = (b - a).normalize_or_zero();
                let mut steps = Vec::new();
                for step in grid.walk(a, b) {
                    if is_turret && modules.get(step.entity).is_ok_and(|m| matches!(m.module_type, ModuleType::GunBarrel | ModuleType::MuzzleBrake)) {
                        continue;
                    }
                    let block = block_query.get(step.entity).copied().unwrap_or(crate::building::Block::module(step.cell));
                    let entry = a + dir_local * step.t_enter;
                    let exit = a + dir_local * (step.t_enter + step.span);
                    if crate::combat::impact::clip_to_shape(&block, step.entry_face, step.cell, entry, exit).is_some() {
                        steps.push((step.entity, block.thickness));
                        if steps.len() >= PULSE_DEPTH {
                            break;
                        }
                    }
                }
                let Some(first) = grid.walk(a, b).into_iter().find(|s| steps.first().is_some_and(|(e, _)| *e == s.entity)) else { continue };
                let dist = first.t_enter * CELL;
                let nearer = best.as_ref().is_none_or(|h| if is_turret { dist <= h.dist + 1.0 } else { dist < h.dist });
                if nearer {
                    best = Some(Hit { ship, dist, shield: false, layer: Some((layer, is_turret, roof)), steps });
                }
            }
        }
        let Some(hit) = best else {
            if super::gun_assembly::trace() && rand::random::<f32>() < 0.02 {
                info!("LASER miss: nothing on the line from {:?} toward {:?}", shot.from, shot.dir);
            }
            draw_beam(&mut commands, shot.from, end, colour, shot.pulse);
            continue;
        };
        let hit_at = shot.from + shot.dir * hit.dist;
        draw_beam(&mut commands, shot.from, hit_at, colour, shot.pulse);
        let mut left = shot.energy * shot.stats.damage_per_charge * reaching(hit.dist, shot.lenses, false);
        let is_player = ships.get(hit.ship).is_ok_and(|s| s.5);
        if is_player && player_hits.god_mode() {
            continue;
        }
        let dealt_total = left;
        if super::gun_assembly::trace() && (shot.pulse || rand::random::<f32>() < 0.02) {
            info!(
                "LASER hit {:?} at {:.0} u{}: {:.0} damage{}",
                hit.ship, hit.dist, if hit.shield { " (shield)" } else { "" }, left,
                if shot.pulse { " (pulse)" } else { "" }
            );
        }
        if hit.shield {
            if let Ok((_, _, _, _, mut shield, _)) = ships.get_mut(hit.ship) {
                shield.absorb(left);
            }
            super::spawn_hit_effect(&mut commands, hit_at, colour.with_alpha(0.6), 8.0);
        } else {
            // Burn in: a steady beam works on the first block; a pulse that
            // kills one carries what's left on.
            let depth = if shot.pulse { PULSE_DEPTH } else { 1 };
            for (entity, thickness) in hit.steps.iter().take(depth) {
                if left <= 0.0 {
                    break;
                }
                let share = burn_share(shot.stats.intensity, *thickness);
                let mut took = 0.0;
                let mut killed = false;
                if let Ok(mut m) = modules.get_mut(*entity) {
                    took = (left * share).min(m.health);
                    m.health -= took;
                    killed = m.health <= 0.0;
                } else if let Ok(mut h) = hulls.get_mut(*entity) {
                    took = (left * share).min(h.health);
                    h.health -= took;
                    killed = h.health <= 0.0;
                    if is_player && hit.layer.is_some_and(|(_, turret, _)| !turret) {
                        player_hits.breach_if_holed(*entity, &mut h);
                    }
                }
                left -= took / share;
                if !killed {
                    break;
                }
            }
            if shot.pulse {
                super::spawn_hit_effect(&mut commands, hit_at, colour, 12.0);
            } else if rand::random::<f32>() < 0.3 {
                super::spawn_hit_effect(&mut commands, hit_at, colour.with_alpha(0.7), 6.0);
            }
        }
        if is_player {
            player_hits.report(dealt_total, hit_at, shot.dir, false);
        } else {
            ai_damage.write(crate::events::AiShipDamaged {
                target: hit.ship,
                source: crate::events::DamageSource::Gunfire,
                amount: dealt_total,
                position: Some(hit_at),
                direction: Some(shot.dir),
                attacker: Some(shot.ship),
            });
        }
    }
}

impl LaserBank {
    /// How full it is, for the weapons rack.
    pub fn fill(&self) -> f32 {
        if self.stats.storage > 0.0 { self.charge / self.stats.storage } else { 0.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(rows: &[&str]) -> (HashMap<IVec2, ModuleType>, IVec2) {
        let mut cells = HashMap::new();
        let mut core = IVec2::ZERO;
        let h = rows.len() as i32;
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                let c = IVec2::new(x as i32, h - 1 - y as i32);
                let t = match ch {
                    'C' => { core = c; ModuleType::LaserCore }
                    'K' => ModuleType::LaserCoupler,
                    'V' => ModuleType::LaserCavity,
                    'P' => ModuleType::LaserPump,
                    'Q' => ModuleType::QSwitch,
                    'D' => ModuleType::FrequencyDoubler,
                    'X' => ModuleType::LaserDestabiliser,
                    'E' => ModuleType::LaserEmitter,
                    'L' => ModuleType::LaserLens,
                    'R' => ModuleType::BeamRelay,
                    _ => continue,
                };
                cells.insert(c, t);
            }
        }
        (cells, core)
    }

    /// The Laser Lab's Cutter: 16 cavities, 10 pumps, steady beam.
    #[test]
    fn the_cutter_lays_out_as_in_the_lab() {
        let (cells, core) = sheet(&[
            ".........",
            ".PVVVVVP.",
            ".PV.X.VP.",
            ".PVKCDVP.",
            ".PV.X.VP.",
            ".PVVVVVP.",
            ".........",
        ]);
        let b = lay_out_bank(&cells, core);
        assert_eq!((b.cavities.len(), b.pumps.len(), b.q_switches, b.doublers, b.destabilisers), (16, 10, 0, 1, 2));
        let s = bank_stats(16, 0, 1, 2);
        assert_eq!(s.storage, 1600.0);
        assert_eq!(s.shots, 0);
        assert_eq!(s.intensity, 75.0);
        assert!((s.drain - 0.578).abs() < 0.001);
    }

    #[test]
    fn parts_off_the_network_count_for_nothing() {
        let (cells, core) = sheet(&[
            "P.V.Q",
            ".....",
            "..C..",
        ]);
        let b = lay_out_bank(&cells, core);
        assert!(b.cavities.is_empty() && b.pumps.is_empty() && b.q_switches == 0);
    }

    #[test]
    fn an_emitter_on_the_core_with_lenses_in_front() {
        let (cells, core) = sheet(&[
            "..L..",
            "..L..",
            "..E..",
            "..C..",
            ".VKR.",
        ]);
        let b = lay_out_bank(&cells, core);
        assert_eq!(b.emitters.len(), 1);
        assert_eq!(b.relays.len(), 1);
        assert_eq!(count_lenses(&cells, b.emitters[0], IVec2::Y), 2);
        assert_eq!(b.cavities.len(), 1, "a cavity on a coupler counts");
    }

    #[test]
    fn pulses_trade_depth_for_damage() {
        let steady = bank_stats(4, 0, 0, 0);
        let pulsed = bank_stats(4, 3, 0, 0);
        assert_eq!(pulsed.shots, 4);
        assert!(pulsed.damage_per_charge > steady.damage_per_charge);
        assert!(pulsed.intensity < steady.intensity);
        assert_eq!(bank_stats(4, 9, 9, 0).shots, 8, "four Q-switches count");
        assert_eq!(bank_stats(4, 0, 9, 0).intensity, 120.0);
    }

    #[test]
    fn armour_and_range() {
        assert_eq!(burn_share(40.0, 0.0), 1.0);
        assert_eq!(burn_share(40.0, 80.0), 0.5);
        assert_eq!(burn_share(10.0, 200.0), 0.1);
        assert!(reaching(4000.0, 6, false) > reaching(4000.0, 0, false));
        assert!(reaching(3000.0, 1, true) > reaching(3000.0, 1, false));
    }
}
