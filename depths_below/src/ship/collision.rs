use bevy::prelude::*;

use crate::ai_ship::components::AiShip;
use crate::camera::CameraState;
use crate::celestial::components::{CelestialBody, CelestialBodyType};
use crate::celestial::poi::{SpacePoi, SpacePoiType};
use crate::components::{HullSegment, Module, Ship, ShipPhysics, Velocity};
use crate::spatial::SpatialGrid;
use crate::world::home_base::{HomeStation, ResupplyOutpost};

// ============================================================================
// PHYSICAL COLLISION — ships, stations, asteroids, planets, stars.
// Everything solid carries a Collider (one or more circles in local space);
// anything with a Velocity is pushed and bounced, everything else is an
// immovable obstacle. Projectiles/missiles keep their own hit systems.
// ============================================================================

/// Bounce energy kept on impact (0 = dead stop, 1 = perfect bounce).
const RESTITUTION: f32 = 0.22;
/// Fraction of remaining overlap corrected per frame — softens de-penetration
/// so contacts settle instead of snapping.
const CORRECTION: f32 = 0.45;
/// Per-frame cap on positional correction. A warp that drops the ship inside
/// a planet slides it to the surface over a few frames instead of teleporting.
const MAX_PUSH: f32 = 350.0;
/// Relative approach speed above which an impact sparks and kicks the camera.
const IMPACT_FX_SPEED: f32 = 250.0;
/// Covers a block's corners from its center (33 * sqrt2) plus a little skin.
const BLOCK_SLACK: f32 = 48.0;
/// AI ships have no ShipPhysics; estimate mass from block count. Tuned so a
/// starter-sized hull (~150 blocks) lands near the player's 1200.
const BLOCK_MASS: f32 = 8.0;
/// Colliders bigger than this (planets, stars) skip the grid and are checked
/// against every ship directly — there are only a handful per system, and a
/// star-sized query radius would defeat the grid entirely.
const HUGE_RADIUS: f32 = 1500.0;

/// Physical collision body: one or more solid circles in entity-local space.
/// Static obstacles are a single circle with infinite mass; ships get a few
/// circles fitted along their long axis so a wedge hull doesn't collide like
/// one giant ball around its nose.
#[derive(Component)]
pub struct Collider {
    /// (local center, radius) — rotated/translated by the entity's Transform.
    pub circles: Vec<(Vec2, f32)>,
    /// Bounding circle enclosing all sub-circles, for the broad phase.
    pub bound_center: Vec2,
    pub bound_radius: f32,
    /// f32::INFINITY = immovable.
    pub mass: f32,
}

impl Collider {
    fn circle(radius: f32, mass: f32) -> Self {
        Self {
            circles: vec![(Vec2::ZERO, radius)],
            bound_center: Vec2::ZERO,
            bound_radius: radius,
            mass,
        }
    }

    fn inv_mass(&self) -> f32 {
        if self.mass.is_finite() { 1.0 / self.mass } else { 0.0 }
    }
}

fn world_point(transform: &Transform, local: Vec2) -> Vec2 {
    transform.translation.truncate() + (transform.rotation * local.extend(0.0)).truncate()
}

/// Broad-phase index over all colliders, rebuilt each frame. Small colliders
/// go in the grid; planet/star-sized ones are kept in a flat list.
#[derive(Resource)]
pub struct ColliderGrid {
    grid: SpatialGrid,
    max_small: f32,
    huge: Vec<Entity>,
}

impl Default for ColliderGrid {
    fn default() -> Self {
        Self { grid: SpatialGrid::new(512.0), max_small: 0.0, huge: Vec::new() }
    }
}

/// Gives freshly spawned static world objects their collision circle.
/// Everything is matched by Added<> here so no spawn site needs to know
/// about collision.
pub fn attach_static_colliders(
    mut commands: Commands,
    celestial: Query<(Entity, &CelestialBody), Added<CelestialBody>>,
    haven: Query<Entity, Added<HomeStation>>,
    outposts: Query<Entity, Added<ResupplyOutpost>>,
    pois: Query<(Entity, &SpacePoi), (Added<SpacePoi>, Without<CelestialBody>)>,
) {
    for (entity, body) in celestial.iter() {
        let radius = match body.body_type {
            // Rock sprites don't fill their square; shrink so the contact
            // matches what the eye sees.
            CelestialBodyType::Asteroid => body.radius * 0.85,
            CelestialBodyType::Planet | CelestialBodyType::Star => body.radius,
            // Black holes consume ships (BeingConsumed spiral) — a solid rim
            // would break that. Debris is not worth bouncing off.
            CelestialBodyType::BlackHole | CelestialBodyType::Debris => continue,
        };
        commands.entity(entity).try_insert(Collider::circle(radius, f32::INFINITY));
    }
    // Haven: circle hugging the hub — the four arm tips poke out rather than
    // walling off the empty diagonals of the full cross shape.
    for entity in haven.iter() {
        commands.entity(entity).try_insert(Collider::circle(140.0, f32::INFINITY));
    }
    for entity in outposts.iter() {
        commands.entity(entity).try_insert(Collider::circle(85.0, f32::INFINITY));
    }
    for (entity, poi) in pois.iter() {
        let radius = match poi.poi_type {
            SpacePoiType::SpaceStation => 75.0,
            SpacePoiType::DerelictShip => 85.0,
            // Anomalies are readings, not matter; asteroid nodes are covered
            // by their CelestialBody above.
            _ => continue,
        };
        commands.entity(entity).try_insert(Collider::circle(radius, f32::INFINITY));
    }
}

/// Fits a handful of circles along the hull's longer axis. Buckets the block
/// positions, one circle per bucket — a wedge gets a small nose circle and a
/// wide stern circle instead of one giant ball.
fn fit_ship_collider(points: &[Vec2], mass: f32) -> Collider {
    if points.is_empty() {
        return Collider::circle(120.0, mass);
    }
    let mut min = points[0];
    let mut max = points[0];
    let mut sum = Vec2::ZERO;
    for p in points {
        min = min.min(*p);
        max = max.max(*p);
        sum += *p;
    }
    let centroid = sum / points.len() as f32;
    let span = max - min;
    let along_x = span.x >= span.y;
    let (long, short) = if along_x { (span.x, span.y) } else { (span.y, span.x) };
    let n = ((long / short.max(66.0)) * 2.0).round().clamp(1.0, 6.0) as usize;

    let lo = if along_x { min.x } else { min.y };
    let mut buckets: Vec<Vec<Vec2>> = vec![Vec::new(); n];
    for p in points {
        let c = if along_x { p.x } else { p.y };
        let idx = if long < 1.0 { 0 } else { (((c - lo) / long) * n as f32) as usize };
        buckets[idx.min(n - 1)].push(*p);
    }

    let mut circles = Vec::new();
    for bucket in buckets.iter().filter(|b| !b.is_empty()) {
        let center = bucket.iter().sum::<Vec2>() / bucket.len() as f32;
        let radius = bucket.iter().map(|p| p.distance(center)).fold(0.0, f32::max) + BLOCK_SLACK;
        circles.push((center, radius.max(70.0)));
    }
    let bound_radius = circles
        .iter()
        .map(|(c, r)| c.distance(centroid) + r)
        .fold(0.0, f32::max);

    Collider { circles, bound_center: centroid, bound_radius, mass }
}

/// (Re)builds ship colliders from their child blocks — on spawn and whenever
/// the block set changes (build edits, battle damage, severance). Covers the
/// player, AI ships, and wrecks (a dead AI ship keeps its entity and blocks).
pub fn refresh_ship_colliders(
    mut commands: Commands,
    ships: Query<
        (Entity, Option<&ShipPhysics>, Option<&Children>),
        (Or<(With<Ship>, With<AiShip>)>, Or<(Changed<Children>, Without<Collider>)>),
    >,
    blocks: Query<&Transform, Or<(With<Module>, With<HullSegment>)>>,
) {
    for (entity, physics, children) in ships.iter() {
        let points: Vec<Vec2> = children
            .map(|children| {
                children
                    .iter()
                    .filter_map(|child| blocks.get(child).ok())
                    .map(|t| t.translation.truncate())
                    .collect()
            })
            .unwrap_or_default();
        let mass = physics
            .map(|p| p.mass)
            .unwrap_or((points.len() as f32 * BLOCK_MASS).max(100.0));
        commands.entity(entity).try_insert(fit_ship_collider(&points, mass));
    }
}

pub fn rebuild_collider_grid(
    mut grid: ResMut<ColliderGrid>,
    colliders: Query<(Entity, &Transform, &Collider)>,
) {
    grid.grid.clear();
    grid.huge.clear();
    grid.max_small = 0.0;
    for (entity, transform, collider) in colliders.iter() {
        if collider.bound_radius > HUGE_RADIUS {
            grid.huge.push(entity);
        } else {
            grid.grid.insert(entity, world_point(transform, collider.bound_center));
            grid.max_small = grid.max_small.max(collider.bound_radius);
        }
    }
}

/// Finds overlapping pairs (broad phase via the grid) and resolves each with
/// a positional push plus an impulse along the contact normal, split by mass.
/// Static obstacles just don't move (infinite mass); everything shares the
/// same math.
pub fn resolve_collisions(
    grid: Res<ColliderGrid>,
    mut bodies: Query<(Entity, &mut Transform, Option<&mut Velocity>, &Collider)>,
    player: Query<(), With<Ship>>,
    mut camera: ResMut<CameraState>,
    mut commands: Commands,
) {
    // Pass 1 (read-only): collect candidate pairs. Only dynamic bodies (ones
    // with a Velocity) initiate; dynamic-dynamic pairs are deduped by entity
    // order so each is resolved once.
    let mut contacts: Vec<(Entity, Entity)> = Vec::new();
    for (entity, transform, velocity, collider) in bodies.iter() {
        if velocity.is_none() {
            continue;
        }
        let center = world_point(transform, collider.bound_center);
        let candidates = grid
            .grid
            .nearby(center, collider.bound_radius + grid.max_small)
            .map(|(other, _)| other)
            .chain(grid.huge.iter().copied());
        for other in candidates {
            if other == entity {
                continue;
            }
            let Ok((_, other_transform, other_velocity, other_collider)) = bodies.get(other) else {
                continue;
            };
            if other_velocity.is_some() && other < entity {
                continue;
            }
            let other_center = world_point(other_transform, other_collider.bound_center);
            if center.distance(other_center) < collider.bound_radius + other_collider.bound_radius {
                contacts.push((entity, other));
            }
        }
    }

    // Pass 2: resolve. Overlaps are recomputed from current transforms so a
    // chain of contacts in one frame doesn't act on stale positions.
    for (a, b) in contacts {
        let Ok([(_, mut ta, va, ca), (_, mut tb, vb, cb)]) = bodies.get_many_mut([a, b]) else {
            continue;
        };

        // Deepest overlapping circle pair decides the contact.
        let mut best: Option<(f32, Vec2, Vec2, f32)> = None; // (pen, world_a, world_b, radius_a)
        for (offset_a, radius_a) in &ca.circles {
            let wa = world_point(&ta, *offset_a);
            for (offset_b, radius_b) in &cb.circles {
                let wb = world_point(&tb, *offset_b);
                let pen = radius_a + radius_b - wa.distance(wb);
                if pen > 0.0 && best.map(|(bp, ..)| pen > bp).unwrap_or(true) {
                    best = Some((pen, wa, wb, *radius_a));
                }
            }
        }
        let Some((pen, wa, wb, radius_a)) = best else { continue };

        let delta = wb - wa;
        let normal = if delta.length_squared() > 1e-6 { delta.normalize() } else { Vec2::X };
        let (inv_a, inv_b) = (ca.inv_mass(), cb.inv_mass());
        let total_inv = inv_a + inv_b;
        if total_inv <= 0.0 {
            continue;
        }

        // Impulse: kill approach speed along the normal, keep a small bounce.
        let vel_a = va.as_ref().map(|v| v.0).unwrap_or(Vec2::ZERO);
        let vel_b = vb.as_ref().map(|v| v.0).unwrap_or(Vec2::ZERO);
        let approach = (vel_a - vel_b).dot(normal);
        if approach > 0.0 {
            let impulse = (1.0 + RESTITUTION) * approach / total_inv;
            if let Some(mut v) = va {
                v.0 -= normal * impulse * inv_a;
            }
            if let Some(mut v) = vb {
                v.0 += normal * impulse * inv_b;
            }
        }

        // Positional correction, split by mass.
        let push = (pen * CORRECTION).min(MAX_PUSH);
        ta.translation -= (normal * push * (inv_a / total_inv)).extend(0.0);
        tb.translation += (normal * push * (inv_b / total_inv)).extend(0.0);

        // Hard hits get a small scrape flash; the player also feels a kick.
        if approach > IMPACT_FX_SPEED {
            let contact = wa + normal * (radius_a - pen * 0.5);
            crate::combat::spawn_hit_effect(
                &mut commands,
                contact,
                Color::srgba(0.75, 0.82, 0.95, 0.85),
                (approach / 70.0).clamp(8.0, 18.0),
            );
            if player.contains(a) || player.contains(b) {
                let kick = ((approach - IMPACT_FX_SPEED) / 80.0).clamp(1.5, 8.0);
                camera.shake_intensity = (camera.shake_intensity + kick).min(10.0);
            }
        }
    }
}
