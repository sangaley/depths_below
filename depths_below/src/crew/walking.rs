//! Crew walking about inside the ship.
//!
//! Split the way the AI ships are: `plan_crew_destinations` and
//! `plan_crew_paths` think on a timer, `walk_crew` moves every frame. A* over
//! a whole ship is far too expensive to redo per frame, and stepping has to be
//! smooth, so the two cannot share a cadence.
//!
//! Crew walk in ship-LOCAL space as children of the ship, so the hull can spin
//! and manoeuvre underneath them without any of it reaching this file.
//!
//! Deliberately does NOT touch `CrewMember::state`. `CrewState::Moving` exists
//! and is unused, but `crew_repair_system` and `crew_rebuild_system` both count
//! `Idle` crew as work capacity, so flipping walkers to `Moving` would quietly
//! slow repair and reconstruction. Whether walking should cost you that is a
//! real design question, and it gets answered when walking gains teeth — not
//! as a side effect of making crew visible. Until then the presence of
//! `CrewPath` is what "this person is walking" means.

use bevy::prelude::*;

use crate::ai_ship::components::OwnedByAiShip;
use crate::building::{footprints, grid_to_local, local_to_grid, ShipGrid};
use crate::components::*;
use crate::crew::eva_salvage::EvaSalvaging;
use crate::crew::navigation::{find_path, NavGrid};

/// World units per second. A cell is 66 units, so this is a touch under a
/// cell a second — brisk enough not to look broken, slow enough that crossing
/// the ship is visibly a journey.
const CREW_WALK_SPEED: f32 = 50.0;

/// How close counts as standing on the waypoint.
const ARRIVE: f32 = 3.0;

/// Seconds between planning passes.
const PLAN_INTERVAL: f32 = 0.5;

/// Where this crew member is trying to get to, in ship-local cells.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrewDestination(pub IVec2);

/// The route there. Inserted when planned, removed on arrival — same idiom as
/// `MigrationPath`, so "is this person walking" is a component query rather
/// than another flag on `CrewMember`.
#[derive(Component)]
pub struct CrewPath {
    pub cells: Vec<IVec2>,
    /// Index of the cell currently being walked toward.
    pub index: usize,
    /// `NavGrid::version` this route was planned against. When the ship
    /// changes shape the route is replanned rather than trusted.
    pub nav_version: u32,
}

impl CrewPath {
    fn target(&self) -> Option<IVec2> {
        self.cells.get(self.index).copied()
    }

    fn destination(&self) -> Option<IVec2> {
        self.cells.last().copied()
    }
}

#[derive(Resource)]
pub struct CrewPlanTimer(Timer);

impl Default for CrewPlanTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(PLAN_INTERVAL, TimerMode::Repeating))
    }
}

/// A crew member's post, as a cell they can actually stand on.
///
/// Multi-cell modules cover several cells and the origin is not guaranteed to
/// be one a person can occupy, so this picks the first passable cell of the
/// footprint rather than assuming.
fn station_cell(nav: &NavGrid, module: &Module) -> Option<IVec2> {
    let footprint = footprints::footprint_override(module.module_type);
    let cells = ShipGrid::cells_for(module.grid_position, module.size, module.rotation, footprint);
    cells
        .iter()
        .copied()
        .find(|c| nav.passable(*c))
        .or_else(|| cells.first().copied().and_then(|c| nav.nearest_passable(c)))
}

/// Points each crew member at their post.
///
/// Runs after `auto_assign_crew`, reading the assignments it made rather than
/// reaching into it — that system's per-ship bucketing and priority ordering
/// is doing enough already, and destinations are derivable from its output.
///
/// Crew with no post keep whatever destination they had; sending idle hands
/// somewhere in particular is a role behaviour, not a movement one.
pub fn plan_crew_destinations(
    mut commands: Commands,
    stations: Query<(&Module, &CrewStation, &ChildOf), Without<OwnedByAiShip>>,
    crew: Query<(Entity, &ChildOf), (With<CrewMember>, Without<EvaSalvaging>, Without<OwnedByAiShip>)>,
    existing: Query<&CrewDestination>,
    navs: Query<&NavGrid>,
) {
    for (module, station, parent) in stations.iter() {
        let Some(assigned) = station.assigned_crew else { continue };
        // The station and its operator must belong to the same ship. Crew
        // queries in this codebase have leaked across ships more than once.
        let Ok((entity, crew_parent)) = crew.get(assigned) else { continue };
        if crew_parent.parent() != parent.parent() {
            continue;
        }
        let Ok(nav) = navs.get(parent.parent()) else { continue };
        let Some(cell) = station_cell(nav, module) else { continue };

        if existing.get(entity).is_ok_and(|d| d.0 == cell) {
            continue;
        }
        commands.entity(entity).try_insert(CrewDestination(cell));
    }
}

/// Plans a route for anyone whose destination they haven't reached.
///
/// A crew member with no route to their post is left with no `CrewPath` and
/// simply stays put. That is a real answer, not a failure: the section they
/// need is cut off. Making it *matter* comes later; for now they just don't
/// walk through walls to get there.
pub fn plan_crew_paths(
    mut commands: Commands,
    time: Res<Time>,
    mut timer: ResMut<CrewPlanTimer>,
    navs: Query<&NavGrid>,
    crew: Query<
        (Entity, &Transform, &ChildOf, &CrewDestination, Option<&CrewPath>),
        (With<CrewMember>, Without<EvaSalvaging>, Without<OwnedByAiShip>),
    >,
) {
    timer.0.tick(time.delta());
    if !timer.0.just_finished() {
        return;
    }

    for (entity, transform, parent, destination, path) in crew.iter() {
        let Ok(nav) = navs.get(parent.parent()) else { continue };
        let destination = destination.0;

        // A route that still ends where we want to go, planned against the
        // ship's current shape, is still good.
        if let Some(path) = path {
            if path.nav_version == nav.version && path.destination() == Some(destination) {
                continue;
            }
        }

        let here = local_to_grid(transform.translation.truncate());
        if here == destination {
            commands.entity(entity).try_remove::<CrewPath>();
            continue;
        }

        // Standing somewhere that stopped being deck — a block was shot out
        // from under them. Walk from the nearest place they could be instead.
        let Some(from) = nav.nearest_passable(here) else {
            commands.entity(entity).try_remove::<CrewPath>();
            continue;
        };

        match find_path(nav, from, destination) {
            Some(cells) => {
                commands
                    .entity(entity)
                    .try_insert(CrewPath { cells, index: 0, nav_version: nav.version });
            }
            None => {
                commands.entity(entity).try_remove::<CrewPath>();
            }
        }
    }
}

/// Steps everyone along their route. Per-frame and deliberately dumb — all
/// the thinking happened in the planner.
pub fn walk_crew(
    mut commands: Commands,
    time: Res<Time>,
    mut crew: Query<
        (Entity, &mut Transform, &mut CrewPath, &CrewMember),
        (Without<EvaSalvaging>, Without<OwnedByAiShip>),
    >,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut path, member) in crew.iter_mut() {
        // The unconscious and the panicking don't report for duty.
        if member.health <= 0.0 || member.state == CrewState::Panicking {
            continue;
        }
        let Some(target_cell) = path.target() else {
            commands.entity(entity).try_remove::<CrewPath>();
            continue;
        };

        let target = grid_to_local(target_cell);
        let pos = transform.translation.truncate();
        let delta = target - pos;
        let distance = delta.length();

        if distance <= ARRIVE {
            transform.translation.x = target.x;
            transform.translation.y = target.y;
            path.index += 1;
            if path.index >= path.cells.len() {
                commands.entity(entity).try_remove::<CrewPath>();
            }
            continue;
        }

        let step = (CREW_WALK_SPEED * dt).min(distance);
        let dir = delta / distance;
        transform.translation.x += dir.x * step;
        transform.translation.y += dir.y * step;
    }
}

/// Z for crew sprites. Above modules (0.2, or 0.4 for overhanging barrels)
/// so a crew member crossing machinery is visible, and above the damage
/// overlay's 0.5 so they don't disappear under it when it's toggled on.
pub const CREW_Z: f32 = 0.6;

/// Every cell covered by the ship's crew quarters.
pub fn quarters_cells<'a>(modules: impl Iterator<Item = &'a Module>) -> Vec<IVec2> {
    let mut cells = Vec::new();
    for module in modules {
        let footprint = footprints::footprint_override(module.module_type);
        cells.extend(ShipGrid::cells_for(
            module.grid_position,
            module.size,
            module.rotation,
            footprint,
        ));
    }
    cells.sort_by_key(|c| (c.x, c.y));
    cells
}

/// Ship-local spawn point for the nth hand coming aboard: spread across the
/// bunks rather than stacked in one cell, so a fresh crew reads as a crew.
///
/// A ship with no quarters at all falls back to the origin; the walking
/// planner will pull them onto real deck on its next pass.
pub fn berth_position(quarters: &[IVec2], index: usize) -> Vec3 {
    match quarters.get(index % quarters.len().max(1)) {
        Some(&cell) => grid_to_local(cell).extend(CREW_Z),
        None => Vec3::new(0.0, 0.0, CREW_Z),
    }
}

#[cfg(test)]
mod walking_tests {
    use super::*;
    use crate::crew::navigation::rebuild_nav_grids;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    fn module_at(module_type: ModuleType, cell: IVec2) -> Module {
        Module {
            module_type,
            health: 100.0,
            max_health: 100.0,
            power_consumption: 0.0,
            power_generation: 0.0,
            is_active: true,
            grid_position: cell,
            size: IVec2::ONE,
            rotation: Rotation::North,
        }
    }

    fn hull_at(cell: IVec2) -> (HullSegment, Transform) {
        (
            HullSegment { grid_position: cell, hull_layer: HullLayer::Hallway, ..default() },
            Transform::from_translation(grid_to_local(cell).extend(0.1)),
        )
    }

    /// A one-row ship: hallway from (0,0) to (len-1, 0), crew at one end, a
    /// station at the other.
    fn corridor_ship(app: &mut App, len: i32) -> (Entity, Entity, IVec2) {
        let ship = app.world_mut().spawn(Ship).id();
        for x in 0..len {
            let cell = IVec2::new(x, 0);
            app.world_mut().spawn(hull_at(cell)).insert(ChildOf(ship));
        }

        let start = IVec2::new(0, 0);
        let post = IVec2::new(len - 1, 0);
        let crew = app
            .world_mut()
            .spawn((
                CrewMember {
                    name: "Chen".into(),
                    health: 100.0,
                    max_health: 100.0,
                    oxygen: 100.0,
                    morale: 100.0,
                    state: CrewState::Idle,
                },
                Transform::from_translation(grid_to_local(start).extend(CREW_Z)),
            ))
            .insert(ChildOf(ship))
            .id();

        app.world_mut()
            .spawn((
                module_at(ModuleType::Gatling, post),
                CrewStation { priority: 6, assigned_crew: Some(crew), manually_assigned: false },
            ))
            .insert(ChildOf(ship));

        (ship, crew, post)
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<CrewPlanTimer>();
        app.add_systems(
            Update,
            (rebuild_nav_grids, plan_crew_destinations, plan_crew_paths, walk_crew).chain(),
        );
        app
    }

    /// Advances the clock so the planner's timer fires and the mover gets a
    /// real delta.
    ///
    /// `Time::advance_by` is no good here: `TimePlugin` rewrites `Time<()>`
    /// from the real clock at the top of every frame, so a manual advance is
    /// overwritten before any system sees it. `TimeUpdateStrategy` is the
    /// supported way to drive the clock by hand.
    fn tick(app: &mut App, seconds: f32) {
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            seconds,
        )));
        app.update();
    }

    fn crew_cell(app: &App, crew: Entity) -> IVec2 {
        local_to_grid(crew_local(app, crew))
    }

    fn crew_local(app: &App, crew: Entity) -> Vec2 {
        app.world().get::<Transform>(crew).unwrap().translation.truncate()
    }

    /// Runs long enough for a destination to be inserted, the planner's timer
    /// to fire, and the resulting route to become visible. Each of those is a
    /// command insertion, so each needs a frame boundary — in play that lag is
    /// two frames of a walk that takes seconds.
    fn settle(app: &mut App) {
        for _ in 0..4 {
            tick(app, 0.3);
        }
    }

    #[test]
    fn a_crew_member_walks_to_their_post() {
        let mut app = test_app();
        let (_, crew, post) = corridor_ship(&mut app, 6);

        settle(&mut app);
        assert_eq!(
            app.world().get::<CrewDestination>(crew).map(|d| d.0),
            Some(post),
            "the assignment never became a destination"
        );
        assert!(app.world().get::<CrewPath>(crew).is_some(), "no route was planned");

        // Five cells at 66 units each, 50 units a second — about 6.6s.
        for _ in 0..100 {
            tick(&mut app, 0.1);
        }

        assert_eq!(crew_cell(&app, crew), post, "crew never reached their station");
        assert!(
            app.world().get::<CrewPath>(crew).is_none(),
            "the route should be dropped on arrival"
        );
    }

    /// The point of walking: it takes time. If a crew member is at their post
    /// on the first frame, nothing downstream can ever cost travel.
    #[test]
    fn arriving_is_not_instant() {
        let mut app = test_app();
        let (_, crew, post) = corridor_ship(&mut app, 6);
        let start_x = crew_local(&app, crew).x;
        let post_x = grid_to_local(post).x;

        settle(&mut app);
        tick(&mut app, 0.5);

        let x = crew_local(&app, crew).x;
        assert!(x > start_x, "never left the bunks");
        assert!(x < post_x, "crossed five cells before the walk could be seen");
    }

    /// A post with no route to it is not a crash and not a teleport — the
    /// crew member simply stays where they are.
    #[test]
    fn an_unreachable_post_strands_the_crew_where_they_stand() {
        let mut app = test_app();
        let ship = app.world_mut().spawn(Ship).id();

        // Two decks with no deck between them.
        for cell in [IVec2::new(0, 0), IVec2::new(4, 0)] {
            app.world_mut().spawn(hull_at(cell)).insert(ChildOf(ship));
        }

        let crew = app
            .world_mut()
            .spawn((
                CrewMember {
                    name: "Okafor".into(),
                    health: 100.0,
                    max_health: 100.0,
                    oxygen: 100.0,
                    morale: 100.0,
                    state: CrewState::Idle,
                },
                Transform::from_translation(grid_to_local(IVec2::new(0, 0)).extend(CREW_Z)),
            ))
            .insert(ChildOf(ship))
            .id();

        app.world_mut()
            .spawn((
                module_at(ModuleType::Gatling, IVec2::new(4, 0)),
                CrewStation { priority: 6, assigned_crew: Some(crew), manually_assigned: false },
            ))
            .insert(ChildOf(ship));

        for _ in 0..20 {
            tick(&mut app, 0.6);
        }

        assert!(app.world().get::<CrewPath>(crew).is_none(), "routed through vacuum");
        assert_eq!(crew_cell(&app, crew), IVec2::new(0, 0), "crew moved without a route");
    }
}

/// How near a hostile has to be before the crew stop doing chores and stay at
/// their posts. Generous on purpose — you should not have damage-control teams
/// strolling into the open while something is still shooting at you.
const COMBAT_RANGE: f32 = 2600.0;

#[derive(Resource)]
pub struct CrewErrandTimer(Timer);

impl Default for CrewErrandTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(1.0, TimerMode::Repeating))
    }
}

/// Out of combat, hands with no post go and fix things.
///
/// Only crew nobody has assigned to a station take errands — the reactor keeps
/// its operator while the spare hands patch the hull. That is the whole reason
/// the engine room stopped needing five people: it freed the hands this uses.
///
/// Nothing here touches `CrewMember::state`. An idle crew member already mends
/// what's within reach at half rate (`IDLE_REPAIR_POWER`), so walking them to
/// the damage is enough to get breaches sealed and plating patched between
/// fights — basic damage control, not a full repair crew.
pub fn plan_repair_errands(
    mut commands: Commands,
    time: Res<Time>,
    mut timer: ResMut<CrewErrandTimer>,
    ships: Query<(Entity, &GlobalTransform, &NavGrid), (With<Ship>, Without<OwnedByAiShip>)>,
    hostiles: Query<&GlobalTransform, With<crate::ai_ship::components::AiShip>>,
    stations: Query<&CrewStation>,
    hulls: Query<(&HullSegment, &ChildOf), Without<HullDestroyed>>,
    modules: Query<(&Module, &ChildOf), Without<DestroyedModule>>,
    crew: Query<
        (Entity, &Transform, &ChildOf),
        (With<CrewMember>, Without<EvaSalvaging>, Without<OwnedByAiShip>),
    >,
) {
    timer.0.tick(time.delta());
    if !timer.0.just_finished() {
        return;
    }
    let Ok((ship, ship_gt, nav)) = ships.single() else { return };

    let ship_pos = ship_gt.translation().truncate();
    let in_combat = hostiles
        .iter()
        .any(|h| h.translation().truncate().distance(ship_pos) < COMBAT_RANGE);
    if in_combat {
        return;
    }

    // Breaches first — air is leaving. Then anything simply damaged.
    let mut jobs: Vec<(u8, IVec2)> = Vec::new();
    for (hull, parent) in hulls.iter() {
        if parent.parent() != ship {
            continue;
        }
        if hull.is_depressurized && hull.depressurization_level > 0.0 {
            jobs.push((0, hull.grid_position));
        } else if hull.health < hull.max_health {
            jobs.push((1, hull.grid_position));
        }
    }
    for (module, parent) in modules.iter() {
        if parent.parent() == ship && module.health < module.max_health && module.health > 0.0 {
            jobs.push((1, module.grid_position));
        }
    }
    if jobs.is_empty() {
        return;
    }

    let posted: std::collections::HashSet<Entity> =
        stations.iter().filter_map(|s| s.assigned_crew).collect();

    // One job per hand, nearest first — otherwise every spare crew member
    // walks to the same breach and the rest of the damage goes untouched.
    let mut claimed: std::collections::HashSet<IVec2> = std::collections::HashSet::new();
    for (entity, transform, parent) in crew.iter() {
        if parent.parent() != ship || posted.contains(&entity) {
            continue;
        }
        let here = local_to_grid(transform.translation.truncate());

        let best = jobs
            .iter()
            .filter(|(_, cell)| !claimed.contains(cell))
            .filter_map(|(severity, cell)| {
                // Stand next to the damage, not inside it — a breached hull
                // cell is not somewhere a person fits.
                nav.nearest_passable(*cell).map(|stand| {
                    let d = (here - stand).abs();
                    (*severity, d.x + d.y, *cell, stand)
                })
            })
            .min_by_key(|(severity, dist, _, _)| (*severity, *dist));

        let Some((_, _, job_cell, stand)) = best else { continue };
        claimed.insert(job_cell);
        commands.entity(entity).try_insert(CrewDestination(stand));
    }
}
