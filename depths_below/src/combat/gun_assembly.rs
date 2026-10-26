//! Guns built from parts.
//!
//! A gun is a Firing Piece plus the parts it can reach. In front, along the
//! way it faces: an optional Mantlet, then a straight line of Barrels, the
//! last of which may be a Muzzle Brake; Rail Magnets sit beside the barrel.
//! Behind and beside it, touching it or each other: Gauge Increasers,
//! Breech Coolers, Recoil Absorbers, Loaders, Clip Racks and Shell Feeders.
//! A straight run of Loader blocks (along the way they face) is one loader,
//! 1 m long per block; Clip Racks count toward the loader they touch.
//!
//! Worked out per ship whenever its layout changes (building::layout), and
//! written into the firing piece's BaseWeaponStats so the existing firing,
//! tuning and enhancer systems use it unchanged. The numbers come from
//! combat::gun_math, the same formulas the Gun Deck preview shows.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use std::collections::VecDeque;

use crate::building::layout::{BuiltFrom, LayoutEpoch, LayoutVersion};
use crate::building::multiblock::components::BaseWeaponStats;
use crate::components::{DestroyedModule, Module, ModuleType, WeaponMount};

use super::gun_math::{self, GunParts, GunStats, Loader};

/// What the parts around a firing piece add up to. On the firing piece.
#[derive(Component, Clone, Debug, Default)]
pub struct GunAssembly {
    pub parts: GunParts,
    pub stats: GunStats,
    /// Every block that belongs to this gun, the firing piece included.
    pub members: Vec<Entity>,
    /// Problems worth telling the builder, in plain words.
    pub problems: Vec<String>,
    /// Ship-local cell the round leaves from: just past the last barrel.
    pub muzzle_cell: IVec2,
    pub feeders: u32,
}

/// DEPTHS_GUN_TRACE=1: log each gun as it is assembled, and each shot.
pub fn trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("DEPTHS_GUN_TRACE").ok().as_deref() == Some("1"))
}

/// Game units per second for each metre per second of muzzle velocity.
/// 850 m/s -- a full burn -- flies at 2,550 u/s, between the old cannon and
/// railgun speeds once the range they reach is taken into account.
pub const GAME_SPEED_PER_MPS: f32 = 3.0;
/// The arc a spinal gun can fire in: it has no turret, you aim the ship.
pub const SPINAL_ARC_DEG: f32 = 8.0;

pub fn range_for(stats: &GunStats) -> f32 {
    4000.0 + stats.velocity_mps * 4.0
}

/// Projectile caliber scale (combat::new_projectiles::caliber_scale), from
/// gauge: a 150 mm gun is 1.0, a 500 mm one 3.3.
pub fn caliber_for(stats: &GunStats) -> f32 {
    (stats.gauge_mm / 150.0).clamp(0.4, 3.4)
}

/// Penetration in the units ammo and armour use (steel plate is 15-80).
pub fn penetration_for(stats: &GunStats) -> f32 {
    stats.penetration_mm * 0.25
}

fn is_body(t: ModuleType) -> bool {
    matches!(
        t,
        ModuleType::GaugeIncreaser
            | ModuleType::Loader
            | ModuleType::ClipRack
            | ModuleType::ShellFeeder
            | ModuleType::BreechCooler
            | ModuleType::RecoilAbsorber
    )
}

/// One ship's guns, from its live modules: (firing piece, assembly).
/// Pure, so it can be tested without a world.
pub fn assemble(modules: &[(Entity, Module)]) -> Vec<(Entity, GunAssembly)> {
    let mut at: HashMap<IVec2, (Entity, ModuleType, crate::components::Rotation)> = HashMap::new();
    for (e, m) in modules {
        at.insert(m.grid_position, (*e, m.module_type, m.rotation));
    }
    let mut firing: Vec<(Entity, IVec2, crate::components::Rotation)> = modules
        .iter()
        .filter(|(_, m)| m.module_type == ModuleType::FiringPiece)
        .map(|(e, m)| (*e, m.grid_position, m.rotation))
        .collect();
    // Deterministic: the same ship always assembles the same way.
    firing.sort_by_key(|(_, c, _)| (c.x, c.y));

    let mut claimed: HashSet<IVec2> = HashSet::new();
    let mut out: Vec<(Entity, GunAssembly)> = Vec::new();
    let mut barrel_lines: HashMap<Entity, (Vec<IVec2>, bool, bool)> = HashMap::new();

    // 1. Barrel lines first: they're unambiguous, straight out the front.
    for &(fp, cell, rot) in &firing {
        claimed.insert(cell);
        let dir = rot.facing_offset();
        let mut c = cell + dir;
        let mut mantlet = false;
        if matches!(at.get(&c), Some((_, ModuleType::Mantlet, _))) && !claimed.contains(&c) {
            mantlet = true;
            claimed.insert(c);
            c += dir;
        }
        let mut barrel = Vec::new();
        let mut brake = false;
        while let Some((_, t, _)) = at.get(&c) {
            if claimed.contains(&c) || !matches!(t, ModuleType::GunBarrel | ModuleType::MuzzleBrake) {
                break;
            }
            claimed.insert(c);
            barrel.push(c);
            if *t == ModuleType::MuzzleBrake {
                brake = true;
                break;
            }
            c += dir;
        }
        barrel_lines.insert(fp, (barrel, mantlet, brake));
    }

    // 2. Body parts: a breadth-first search from every firing piece at once,
    // so a part touching two guns belongs to the nearer one.
    let mut owner: HashMap<IVec2, Entity> = HashMap::new();
    let mut queue: VecDeque<(IVec2, Entity)> = VecDeque::new();
    for &(fp, cell, _) in &firing {
        owner.insert(cell, fp);
        queue.push_back((cell, fp));
    }
    while let Some((c, fp)) = queue.pop_front() {
        for step in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
            let n = c + step;
            if owner.contains_key(&n) || claimed.contains(&n) {
                continue;
            }
            if let Some((_, t, _)) = at.get(&n) {
                if is_body(*t) {
                    owner.insert(n, fp);
                    queue.push_back((n, fp));
                }
            }
        }
    }

    for &(fp, cell, rot) in &firing {
        let (barrel, mantlet, brake) = barrel_lines.remove(&fp).unwrap_or_default();
        let dir = rot.facing_offset();
        let mine: Vec<IVec2> = owner.iter().filter(|(c, e)| **e == fp && **c != cell).map(|(c, _)| *c).collect();
        let kind = |c: &IVec2| at.get(c).map(|(_, t, _)| *t);
        let mut members = vec![fp];
        members.extend(mine.iter().filter_map(|c| at.get(c).map(|(e, _, _)| *e)));
        members.extend(barrel.iter().filter_map(|c| at.get(c).map(|(e, _, _)| *e)));
        if mantlet {
            if let Some((e, _, _)) = at.get(&(cell + dir)) {
                members.push(*e);
            }
        }

        // Rail magnets: beside any barrel cell.
        let side = IVec2::new(-dir.y, dir.x);
        let mut magnets = 0;
        for b in &barrel {
            for s in [side, -side] {
                let c = *b + s;
                if kind(&c) == Some(ModuleType::RailMagnet) && claimed.insert(c) {
                    magnets += 1;
                    if let Some((e, _, _)) = at.get(&c) {
                        members.push(*e);
                    }
                }
            }
        }

        let count = |t: ModuleType| mine.iter().filter(|c| kind(c) == Some(t)).count() as u32;
        let gauge = count(ModuleType::GaugeIncreaser);
        let coolers = count(ModuleType::BreechCooler);
        let absorbers = count(ModuleType::RecoilAbsorber);
        let feeders = count(ModuleType::ShellFeeder);

        // Loaders: straight runs of Loader blocks along the way they face.
        let loader_cells: HashSet<IVec2> = mine.iter().copied().filter(|c| kind(c) == Some(ModuleType::Loader)).collect();
        let mut seen: HashSet<IVec2> = HashSet::new();
        let mut runs: Vec<Vec<IVec2>> = Vec::new();
        let mut sorted: Vec<IVec2> = loader_cells.iter().copied().collect();
        sorted.sort_by_key(|c| (c.x, c.y));
        for start in sorted {
            if seen.contains(&start) {
                continue;
            }
            let axis = at.get(&start).map(|(_, _, r)| r.facing_offset()).unwrap_or(IVec2::X).abs();
            let same_axis = |c: &IVec2| {
                loader_cells.contains(c) && at.get(c).map(|(_, _, r)| r.facing_offset().abs()) == Some(axis)
            };
            // Walk back to the start of the run, then forward along it.
            let mut head = start;
            while same_axis(&(head - axis)) && !seen.contains(&(head - axis)) {
                head -= axis;
            }
            let mut run = Vec::new();
            let mut c = head;
            while same_axis(&c) && !seen.contains(&c) {
                seen.insert(c);
                run.push(c);
                c += axis;
            }
            runs.push(run);
        }
        // Clip racks: each belongs to the loader it touches, or reaches
        // through other racks.
        let clip_cells: HashSet<IVec2> = mine.iter().copied().filter(|c| kind(c) == Some(ModuleType::ClipRack)).collect();
        let mut clip_owner: HashMap<IVec2, usize> = HashMap::new();
        let mut q: VecDeque<(IVec2, usize)> = VecDeque::new();
        for (i, run) in runs.iter().enumerate() {
            for c in run {
                q.push_back((*c, i));
            }
        }
        while let Some((c, i)) = q.pop_front() {
            for step in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                let n = c + step;
                if clip_cells.contains(&n) && !clip_owner.contains_key(&n) {
                    clip_owner.insert(n, i);
                    q.push_back((n, i));
                }
            }
        }
        let loaders: Vec<Loader> = runs
            .iter()
            .enumerate()
            .map(|(i, run)| {
                let clips = clip_owner.values().filter(|o| **o == i).count() as f32;
                Loader { length_m: run.len() as u32, clips: clips / run.len().max(1) as f32 }
            })
            .collect();

        let parts = GunParts {
            gauge_increasers: gauge,
            loaders,
            barrel_m: barrel.len() as u32,
            coolers,
            absorbers,
            rail_magnets: magnets,
            muzzle_brake: brake,
        };
        let stats = gun_math::stats(&parts);

        let mut problems = Vec::new();
        if barrel.is_empty() {
            problems.push("No barrel. Lay Barrels in a straight line out of the firing piece's front.".to_string());
        } else if (barrel.len() as f32) < stats.burn_m {
            problems.push(format!(
                "The barrel is {:.1} m short of a full burn, so shells leave at {:.0}% speed.",
                stats.burn_m - barrel.len() as f32,
                100.0 * stats.velocity_mps / gun_math::FULL_BURN_VELOCITY
            ));
        }
        if parts.loaders.is_empty() {
            problems.push("No loader. Put Loader blocks behind or beside the firing piece.".to_string());
        } else if stats.working_loaders == 0 {
            problems.push(format!(
                "Every loader is too short for a {:.2} m shell. Lengthen one or drop gauge.",
                stats.shell_m
            ));
        }
        if gauge > gun_math::MAX_GAUGE_INCREASERS {
            problems.push(format!("{} gauge increasers; only {} count.", gauge, gun_math::MAX_GAUGE_INCREASERS));
        }
        if stats.rpm > 0.0 && stats.rpm == stats.rpm_heat && stats.rpm_heat < stats.rpm_loaders {
            problems.push("Heat, not the loaders, sets the pace. Another Breech Cooler would let it fire faster.".to_string());
        }

        let muzzle_cell = barrel.last().copied().unwrap_or(cell + if mantlet { dir } else { IVec2::ZERO }) + dir;
        out.push((fp, GunAssembly { parts, stats, members, problems, muzzle_cell, feeders }));
    }
    out
}

/// Re-assemble a ship's guns when its layout changes, and hand the numbers
/// to the firing piece's weapon.
pub fn assemble_guns(
    mut commands: Commands,
    ships: Query<(Entity, &LayoutVersion, Option<&Children>)>,
    modules: Query<&Module, Without<DestroyedModule>>,
    mut firing: Query<(&mut BaseWeaponStats, &mut WeaponMount)>,
    epoch: Res<LayoutEpoch>,
    mut seen_epoch: Local<Option<LayoutEpoch>>,
    mut built: Local<BuiltFrom>,
) {
    if *seen_epoch == Some(*epoch) {
        return;
    }
    *seen_epoch = Some(*epoch);
    for (ship, version, children) in ships.iter() {
        if !built.is_stale(ship, version) {
            continue;
        }
        built.mark(ship, version);
        let Some(children) = children else { continue };
        let mods: Vec<(Entity, Module)> = children
            .iter()
            .filter_map(|c| modules.get(c).ok().map(|m| (c, m.clone())))
            .filter(|(_, m)| m.module_type.category() == crate::components::ModuleCategory::Guns)
            .collect();
        if mods.is_empty() {
            continue;
        }
        for (fp, assembly) in assemble(&mods) {
            if let Ok((mut base, mut mount)) = firing.get_mut(fp) {
                base.damage = assembly.stats.damage;
                base.range = range_for(&assembly.stats);
                base.fire_rate = (assembly.stats.rpm / 60.0).max(0.0);
                mount.firing_arc = SPINAL_ARC_DEG;
            }
            if trace() {
                let s = &assembly.stats;
                info!(
                    "GUN assembled {:?}: {:.0} mm, shell {:.2} m, {} of {} loaders working, barrel {} m (burn {:.1}), {:.0} m/s, {:.0} dmg, {:.1} rpm (loaders {:.1}, heat {:.1}), spread {:.2} deg, {} parts, problems: {:?}",
                    fp, s.gauge_mm, s.shell_m, s.working_loaders, assembly.parts.loaders.len(), assembly.parts.barrel_m, s.burn_m,
                    s.velocity_mps, s.damage, s.rpm, s.rpm_loaders, s.rpm_heat, s.spread_deg, assembly.members.len(), assembly.problems
                );
            }
            commands.entity(fp).try_insert(assembly);
        }
    }
    built.retain(|e| ships.contains(e));
}

#[cfg(test)]
mod gun_assembly_tests {
    use super::*;
    use crate::components::Rotation;

    fn m(t: ModuleType, x: i32, y: i32, r: Rotation) -> Module {
        Module {
            module_type: t,
            health: 100.0,
            max_health: 100.0,
            power_consumption: 0.0,
            power_generation: 0.0,
            is_active: true,
            grid_position: IVec2::new(x, y),
            size: IVec2::ONE,
            rotation: r,
        }
    }

    /// Build a world-free module list from (type, x, y, rotation).
    fn ship(parts: &[(ModuleType, i32, i32, Rotation)]) -> Vec<(Entity, Module)> {
        parts
            .iter()
            .enumerate()
            .map(|(i, (t, x, y, r))| (Entity::from_raw_u32(i as u32 + 1).unwrap(), m(*t, *x, *y, *r)))
            .collect()
    }

    use ModuleType::*;
    use Rotation::*;

    /// A 170 mm gun facing the bow: two increasers, a 1 m loader with one
    /// rack, a cooler, and four barrels.
    fn small_gun() -> Vec<(Entity, Module)> {
        ship(&[
            (FiringPiece, 0, 0, East),
            (GaugeIncreaser, 0, 1, East),
            (GaugeIncreaser, 0, -1, East),
            (Loader, -1, 0, East),
            (ClipRack, -2, 0, East),
            (BreechCooler, -1, 1, East),
            (GunBarrel, 1, 0, East),
            (GunBarrel, 2, 0, East),
            (GunBarrel, 3, 0, East),
            (GunBarrel, 4, 0, East),
        ])
    }

    #[test]
    fn parts_around_a_firing_piece_make_one_gun() {
        let guns = assemble(&small_gun());
        assert_eq!(guns.len(), 1);
        let g = &guns[0].1;
        assert_eq!(g.parts.gauge_increasers, 2);
        assert_eq!(g.parts.barrel_m, 4);
        assert_eq!(g.parts.coolers, 1);
        assert_eq!(g.parts.loaders, vec![gun_math::Loader { length_m: 1, clips: 1.0 }]);
        assert_eq!(g.stats.gauge_mm, 170.0);
        assert!(g.stats.rpm > 0.0);
        assert_eq!(g.muzzle_cell, IVec2::new(5, 0));
        assert_eq!(g.members.len(), 10);
        assert!(g.problems.is_empty(), "{:?}", g.problems);
    }

    #[test]
    fn barrels_only_count_in_a_straight_line_out_the_front() {
        let mut s = small_gun();
        s.push((Entity::from_raw_u32(99).unwrap(), m(GunBarrel, 5, 1, East))); // off the line
        let g = &assemble(&s)[0].1;
        assert_eq!(g.parts.barrel_m, 4);
    }

    #[test]
    fn a_straight_run_of_loader_blocks_is_one_long_loader() {
        let s = ship(&[
            (FiringPiece, 0, 0, East),
            (Loader, -1, 0, East),
            (Loader, -2, 0, East),
            (Loader, -3, 0, East),
            (GunBarrel, 1, 0, East),
        ]);
        let g = &assemble(&s)[0].1;
        assert_eq!(g.parts.loaders, vec![gun_math::Loader { length_m: 3, clips: 0.0 }]);
    }

    #[test]
    fn a_part_between_two_guns_goes_to_the_nearer() {
        let s = ship(&[
            (FiringPiece, 0, 0, East),
            (GaugeIncreaser, 0, 1, East),
            (GaugeIncreaser, 0, 2, East),
            (FiringPiece, 0, 4, East),
            (GaugeIncreaser, 0, 3, East),
        ]);
        let guns = assemble(&s);
        let gauges: Vec<u32> = guns.iter().map(|(_, g)| g.parts.gauge_increasers).collect();
        assert_eq!(gauges, vec![2, 1]);
    }

    #[test]
    fn rail_magnets_beside_the_barrel_add_speed() {
        let mut s = small_gun();
        s.push((Entity::from_raw_u32(98).unwrap(), m(RailMagnet, 2, 1, East)));
        s.push((Entity::from_raw_u32(97).unwrap(), m(RailMagnet, 3, -1, East)));
        let plain = &assemble(&small_gun())[0].1;
        let railed = &assemble(&s)[0].1;
        assert_eq!(railed.parts.rail_magnets, 2);
        assert!(railed.stats.velocity_mps > plain.stats.velocity_mps);
    }

    #[test]
    fn a_gun_with_nothing_to_load_it_says_so() {
        let s = ship(&[(FiringPiece, 0, 0, North), (GunBarrel, 0, 1, North)]);
        let g = &assemble(&s)[0].1;
        assert_eq!(g.stats.rpm, 0.0);
        assert!(g.problems.iter().any(|p| p.contains("No loader")));
    }
}
