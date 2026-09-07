//! Air as a fluid: pressure per tile, flow toward holes, and the drag that
//! flow puts on anyone standing in it.
//!
//! The old model drained each breached room independently at a flat rate
//! (`room.air_level -= 0.15 * dt`), so air had no direction and a sealed
//! compartment beside a hole in vacuum kept its air forever. Here every
//! interior tile carries its own pressure, neighbours inside one room
//! equalise, and holes pull to vacuum — so a hole at the bow empties the bow
//! first and the stern feeds it. That travelling wave is the whole point.
//!
//! `Room::air_level` survives as the mean of its tiles: `ship::fire` and
//! `crew_emergency_dispatch` read it and there is no reason to disturb them.
//!
//! Player ship only, like the `RoomMap` this is derived from. The house rule
//! is to key cell maps by `(Entity, IVec2)` (see `HeatNetworkState`), but that
//! buys nothing until room detection itself runs per-ship.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::building::rooms::RoomMap;
use crate::building::local_to_grid;
use crate::components::*;
use crate::events::*;

/// How fast a tile fully open to space empties, as a fraction per second.
/// Exponential, so it is violent while full and tapers as it empties — which
/// is where "the pull depends on how much air is left" comes from for free.
const VENT_CONDUCTANCE: f32 = 0.9;

/// How fast neighbouring tiles in one room equalise. Tuned against
/// `VENT_CONDUCTANCE` so a compartment feeds a hole faster than the hole can
/// drain a single tile — otherwise you get a dry tile at the breach and a
/// still-full room behind it, which reads as a bug rather than as suction.
const DIFFUSE_RATE: f32 = 2.5;

/// World units per second of body drag per unit of air flow.
///
/// A unit conversion, not a difficulty dial. Crew walk at `CREW_WALK_SPEED`
/// (50 u/s), and a fresh hole against full pressure produces flow near 0.9, so
/// 70.0 puts the draught at ~63 u/s — it takes them. By half pressure it is
/// ~31 u/s and they can walk out again.
const SUCTION_COUPLING: f32 = 70.0;

/// Flow above which being dragged starts to tell on someone.
const PANIC_FLOW: f32 = 0.5;

/// Morale lost per second while caught in a draught that strong. Sized so
/// roughly five seconds of it breaks a crew member who started sound, which
/// is long enough that a glancing tug on the way past costs nothing.
const PANIC_MORALE_DRAIN: f32 = 15.0;

/// Flow at a hole above which a body goes through it.
const EJECT_FLOW: f32 = 0.6;

/// Per-tile air inside the player's ship, in ship-LOCAL cells.
#[derive(Resource, Default)]
pub struct AirField {
    /// 1.0 = full atmosphere, 0.0 = vacuum.
    pub pressure: HashMap<IVec2, f32>,
    /// Where the air is going and how hard, in pressure-fraction per second.
    /// Rebuilt every frame by `vent_air_at_breaches` and `diffuse_air`.
    pub flow: HashMap<IVec2, Vec2>,
    /// Hull cells that are open, and how wide (0..1).
    ///
    /// Remembered rather than recomputed from what is standing, because a
    /// destroyed plate is despawned half a second after it dies: derive this
    /// from presence and the hole heals itself when the wreckage clears.
    /// Cleared per cell when a live plate exists there again.
    pub holes: HashMap<IVec2, f32>,
    /// Interior tiles that touch a hole: which way out, and total conductance.
    pub vents: HashMap<IVec2, (Vec2, f32)>,
}

impl AirField {
    /// Mean pressure across a set of tiles. Empty set reads as full, so a ship
    /// with no detected rooms never looks like it is suffocating.
    pub fn mean(&self, tiles: &[IVec2]) -> f32 {
        if tiles.is_empty() {
            return 1.0;
        }
        let sum: f32 = tiles.iter().map(|t| self.pressure.get(t).copied().unwrap_or(1.0)).sum();
        sum / tiles.len() as f32
    }
}

/// Seeds new interior tiles and forgets tiles that stopped being interior.
///
/// Also clears `flow`, which the two systems after this one accumulate into.
pub fn sync_air_tiles(
    ship_query: Query<Entity, With<Ship>>,
    hull_query: Query<(&HullSegment, Option<&HullDestroyed>, &ChildOf)>,
    room_map: Res<RoomMap>,
    mut air: ResMut<AirField>,
) {
    let Ok(player_ship) = ship_query.single() else { return };
    air.flow.clear();

    for room in room_map.rooms.iter() {
        for &tile in &room.tiles {
            air.pressure.entry(tile).or_insert(room.air_level);
        }
    }

    // A tile that was shot away stops holding air. Without this the map grows
    // forever and destroyed cells keep feeding pressure into their neighbours.
    air.pressure.retain(|tile, _| room_map.tile_to_room.contains_key(tile));

    // Holes and the vents they open are computed once here; venting, the room
    // summary and crew suction all read the result rather than each rebuilding
    // it from a full hull query.
    refresh_holes(&mut air, &hull_query, player_ship);
    air.vents = compute_vents(&room_map, &air.holes);
}

/// Refreshes the remembered holes from hull damage.
///
/// Deliberately NOT geometric. Treating "no block in that cell" as open space
/// looks right and is catastrophically wrong here: the pristine starter design
/// has 51 interior tiles with no plating beyond them, so the ship would vent
/// from 51 places the moment it launched. Absence of a plate is not evidence
/// of a hole on a ship that was never fully plated -- only damage is.
///
/// Also not keyed off `HullSegment::is_depressurized`: that flag is set from
/// room air level by `update_decompression`, so venting through it would make
/// every tile of a half-empty room its own hole and run away.
fn refresh_holes(
    air: &mut AirField,
    hull: &Query<(&HullSegment, Option<&HullDestroyed>, &ChildOf)>,
    player_ship: Entity,
) {
    for (segment, destroyed, parent) in hull.iter() {
        if parent.parent() != player_ship {
            continue;
        }
        let pos = segment.grid_position;
        if destroyed.is_some() {
            air.holes.insert(pos, 1.0);
            continue;
        }
        // The segment's own breach state, set by `mark_breached_hull` from a
        // HullBreached event and worn down by crew sealing
        // (`crew_repair_system` drives depressurization_level to zero, then
        // clears the flag and starts paying scrap for the plate itself).
        //
        // Deriving this from health instead would deadlock that loop: hull
        // repair is gated behind the breach being sealed, so a hole computed
        // from low health could never close.
        if segment.is_depressurized && segment.depressurization_level > 0.0 {
            air.holes.insert(pos, segment.depressurization_level.clamp(0.0, 1.0));
        } else {
            air.holes.remove(&pos);
        }
    }
}

/// Opens a hole when `ship::damage` reports one.
///
/// A breach has to be an edge, not a standing condition. Recomputing it from
/// "health is under 30%" re-opens the hole the frame after crew finish sealing
/// it, because sealing is free damage control and does not restore the plate --
/// the crew patch the hole first and only then start spending scrap on health.
pub fn mark_breached_hull(
    mut breaches: MessageReader<HullBreached>,
    mut hull: Query<&mut HullSegment>,
) {
    for breach in breaches.read() {
        let Ok(mut segment) = hull.get_mut(breach.segment) else { continue };
        if segment.is_depressurized {
            continue; // already open; don't undo the crew's progress on it
        }
        segment.is_depressurized = true;
        segment.depressurization_level = 1.0;
    }
}

/// Which interior tiles touch a hole, which way it lies, and how wide.
fn compute_vents(room_map: &RoomMap, holes: &HashMap<IVec2, f32>) -> HashMap<IVec2, (Vec2, f32)> {
    let mut vents: HashMap<IVec2, (Vec2, f32)> = HashMap::new();
    for tile in room_map.tile_to_room.keys() {
        for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
            let neighbor = *tile + offset;
            if room_map.tile_to_room.contains_key(&neighbor) {
                continue; // interior, handled by diffusion
            }
            let Some(&conductance) = holes.get(&neighbor) else { continue };
            let entry = vents.entry(*tile).or_insert((Vec2::ZERO, 0.0));
            entry.0 += offset.as_vec2() * conductance;
            // Conductance sums, so a tile with three holes on it empties three
            // times as fast. That is where "size of the hole" enters.
            entry.1 += conductance;
        }
    }
    vents
}

/// Air leaves through holes, proportionally to how much is still there.
pub fn vent_air_at_breaches(time: Res<Time>, mut air: ResMut<AirField>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    let vents: Vec<(IVec2, Vec2, f32)> = air
        .vents
        .iter()
        .map(|(&tile, &(dir, conductance))| (tile, dir, conductance))
        .collect();

    for (tile, direction, conductance) in vents {
        let Some(pressure) = air.pressure.get_mut(&tile) else { continue };
        if *pressure <= 0.0 {
            continue;
        }
        let drained = (*pressure * VENT_CONDUCTANCE * conductance * dt).min(*pressure);
        *pressure -= drained;
        if let Some(dir) = direction.try_normalize() {
            *air.flow.entry(tile).or_insert(Vec2::ZERO) += dir * (drained / dt);
        }
    }
}

/// Neighbouring tiles inside one room equalise.
///
/// Same shape as `heat::diffuse_heat`: snapshot, accumulate deltas into a
/// `Vec`, apply afterwards — otherwise the result depends on hash iteration
/// order. Exchange is same-room only, which is what makes a sealed bulkhead
/// stop the flow for nothing: room detection already splits rooms at one.
pub fn diffuse_air(time: Res<Time>, room_map: Res<RoomMap>, mut air: ResMut<AirField>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    let prev = air.pressure.clone();
    let mut deltas: Vec<(IVec2, f32)> = Vec::new();
    let mut flows: Vec<(IVec2, Vec2)> = Vec::new();

    for (&pos, &pressure) in prev.iter() {
        let Some(&room) = room_map.tile_to_room.get(&pos) else { continue };
        for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
            let neighbor = pos + offset;
            // Different room means a wall between them, so nothing crosses.
            if room_map.tile_to_room.get(&neighbor) != Some(&room) {
                continue;
            }
            let Some(&neighbor_pressure) = prev.get(&neighbor) else { continue };
            let delta = (pressure - neighbor_pressure) * DIFFUSE_RATE * dt;
            if delta > 0.0 {
                deltas.push((pos, -delta));
                deltas.push((neighbor, delta));
                flows.push((pos, offset.as_vec2() * (delta / dt)));
            }
        }
    }

    for (pos, delta) in deltas {
        if let Some(pressure) = air.pressure.get_mut(&pos) {
            *pressure = (*pressure + delta).clamp(0.0, 1.0);
        }
    }
    for (pos, flow) in flows {
        *air.flow.entry(pos).or_insert(Vec2::ZERO) += flow;
    }
}

/// Publishes the tile field back as room air level, which is what the rest of
/// the game reads.
///
/// `is_breached` is derived from whether the room actually has a hole on it
/// rather than latched from a `RoomDepressurized` event, so it clears itself
/// when the hull is repaired.
pub fn sync_room_air(air: Res<AirField>, mut room_map: ResMut<RoomMap>) {
    for room in room_map.rooms.iter_mut() {
        room.air_level = air.mean(&room.tiles);
        room.is_breached = room.tiles.iter().any(|t| air.vents.contains_key(t));
    }
}

// ============================================================================
// WHAT THE AIR DOES TO PEOPLE
// ============================================================================

/// A body on its way out of the ship, in WORLD space.
///
/// Not a crew member: the crew entity is despawned by `handle_crew_death` the
/// moment it dies, precisely so no staffing or routing system has to learn to
/// skip it. This is the stand-in that tumbles away where you can see it.
#[derive(Component)]
pub struct EjectedBody {
    pub velocity: Vec2,
    pub spin: f32,
    pub life: f32,
}

/// Air drags anyone standing in it, and takes them out through the hole.
///
/// Runs after `walk_crew`, so the two compose: a crew member walking at
/// `CREW_WALK_SPEED` into a draught of 63 u/s still loses ground, and wins it
/// back once the compartment has partly emptied. Nothing here reads
/// `GlobalTransform` — crew live in ship-local space and so does the air.
pub fn crew_suction(
    mut commands: Commands,
    time: Res<Time>,
    air: Res<AirField>,
    ship_query: Query<&Velocity, With<Ship>>,
    mut crew: Query<
        (Entity, &mut Transform, &GlobalTransform, &mut CrewMember),
        (
            Without<crate::crew::eva_salvage::EvaSalvaging>,
            Without<crate::ai_ship::components::OwnedByAiShip>,
        ),
    >,
    mut deaths: MessageWriter<CrewDied>,
) {
    let Ok(ship_velocity) = ship_query.single() else { return };
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    for (entity, mut transform, global, mut member) in crew.iter_mut() {
        if member.health <= 0.0 {
            continue;
        }
        let cell = local_to_grid(transform.translation.truncate());
        let Some(&flow) = air.flow.get(&cell) else { continue };
        let strength = flow.length();
        if strength < f32::EPSILON {
            continue;
        }

        // Out through the hole. Only on a tile that actually has one, so a
        // strong draught mid-corridor shoves you about but cannot delete you.
        if strength >= EJECT_FLOW && air.vents.contains_key(&cell) {
            let world = global.translation();
            let outward = flow.normalize_or_zero();
            commands.spawn((
                Sprite {
                    color: Color::srgb(0.8, 0.6, 0.5),
                    custom_size: Some(Vec2::new(16.0, 16.0)),
                    ..default()
                },
                Transform::from_translation(world),
                EjectedBody {
                    velocity: ship_velocity.0 + outward * (strength * SUCTION_COUPLING),
                    spin: if outward.x >= 0.0 { 4.0 } else { -4.0 },
                    life: 6.0,
                },
            ));
            deaths.write(CrewDied {
                crew: entity,
                name: member.name.clone(),
                cause: CrewDamageSource::Decompression,
            });
            // handle_crew_death despawns them, but not until it next runs.
            // Zeroing health here is what stops this firing again next frame
            // and spawning a second body for the same person.
            member.health = 0.0;
            continue;
        }

        transform.translation.x += flow.x * SUCTION_COUPLING * dt;
        transform.translation.y += flow.y * SUCTION_COUPLING * dt;

        // Being dragged toward a hole is frightening, and morale is how this
        // game already says so: `update_crew_ai` panics anyone under 20 and
        // calms them again over 30, and `update_crew_needs` regenerates it
        // once they are safe.
        //
        // Setting `CrewState::Panicking` here directly does not work -- that
        // same system clears it on the next frame for anyone above morale 30,
        // which is nearly everyone, so the flag flickered on and off and
        // `walk_crew` skipped them on alternating frames. Going through morale
        // means a few seconds in a strong draught genuinely breaks someone,
        // and a brief tug does not.
        if strength >= PANIC_FLOW {
            member.morale = (member.morale - PANIC_MORALE_DRAIN * dt).max(0.0);
        }
    }
}

/// Tumble ejected bodies away and fade them out.
pub fn tumble_ejected_bodies(
    mut commands: Commands,
    time: Res<Time>,
    mut bodies: Query<(Entity, &mut Transform, &mut Sprite, &mut EjectedBody)>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut sprite, mut body) in bodies.iter_mut() {
        body.life -= dt;
        if body.life <= 0.0 {
            commands.entity(entity).try_despawn();
            continue;
        }
        let velocity = body.velocity;
        transform.translation.x += velocity.x * dt;
        transform.translation.y += velocity.y * dt;
        transform.rotate_z(body.spin * dt);
        // Fade over the last two seconds rather than blinking out.
        sprite.color.set_alpha((body.life / 2.0).min(1.0));
    }
}

#[cfg(test)]
mod air_tests {
    use super::*;
    use crate::building::rooms::Room;

    /// A corridor of `len` interior cells along +X, walled by inner hull on
    /// both ends unless `open_at` names a cell to leave open to space.
    fn corridor(app: &mut App, len: i32, open_at: Option<i32>) -> Entity {
        let ship = app.world_mut().spawn(Ship).id();

        let tiles: Vec<IVec2> = (0..len).map(|x| IVec2::new(x, 0)).collect();
        let mut room_map = RoomMap::default();
        for (i, &t) in tiles.iter().enumerate() {
            let _ = i;
            room_map.tile_to_room.insert(t, 0);
        }
        room_map.rooms.push(Room {
            id: 0,
            tiles: tiles.clone(),
            air_level: 1.0,
            is_breached: false,
            has_power: false,
        });
        app.insert_resource(room_map);

        // Cap both ends. `open_at` names the cap that has been holed -- the
        // breached state `mark_breached_hull` would have put on it, since
        // these tests drive the field directly rather than through damage.
        for x in [-1, len] {
            let breached = open_at == Some(x);
            app.world_mut().spawn((
                HullSegment {
                    health: if breached { 0.0 } else { 100.0 },
                    max_health: 100.0,
                    radiation_shielding: 0.0,
                    is_depressurized: breached,
                    depressurization_level: if breached { 1.0 } else { 0.0 },
                    hull_layer: HullLayer::Inner,
                    material: HullMaterial::Steel,
                    grid_position: IVec2::new(x, 0),
                },
                ChildOf(ship),
            ));
        }
        ship
    }

    /// Deliberately no `MinimalPlugins`: its `TimePlugin` rewrites `Time` from
    /// the wall clock on every update, so a hand-advanced delta is discarded
    /// and every step runs at a few microseconds. The rates here are all
    /// per-second, which made the whole simulation measure as nearly frozen.
    /// These tests own the clock instead.
    fn sim_app() -> App {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<AirField>();
        app.add_systems(
            Update,
            (sync_air_tiles, vent_air_at_breaches, diffuse_air, sync_room_air).chain(),
        );
        app
    }

    fn step(app: &mut App, seconds: f32, steps: u32) {
        for _ in 0..steps {
            let delta = std::time::Duration::from_secs_f32(seconds / steps as f32);
            app.world_mut().resource_mut::<Time>().advance_by(delta);
            app.update();
        }
    }

    fn pressure(app: &App, x: i32) -> f32 {
        app.world().resource::<AirField>().pressure[&IVec2::new(x, 0)]
    }

    /// A sealed compartment must not leak. The old model drained any room
    /// flagged breached at a flat rate regardless of whether it still had a
    /// hole on it.
    #[test]
    fn sealed_room_holds_its_air() {
        let mut app = sim_app();
        corridor(&mut app, 4, None);
        step(&mut app, 5.0, 50);

        for x in 0..4 {
            assert!(
                pressure(&app, x) > 0.99,
                "tile {x} lost air with no hole in the hull: {}",
                pressure(&app, x)
            );
        }
    }

    /// The whole point: air nearest the hole goes first and the far end feeds
    /// it, so there is a gradient rather than a uniform fade.
    #[test]
    fn air_empties_from_the_hole_outward() {
        let mut app = sim_app();
        corridor(&mut app, 5, Some(-1)); // hole past the x=0 end
        step(&mut app, 0.5, 25);

        let near = pressure(&app, 0);
        let far = pressure(&app, 4);
        assert!(near < far, "expected a gradient, got near={near} far={far}");
        assert!(near < 0.95, "tile at the hole barely drained: {near}");
        assert!(far > near + 0.02, "gradient too flat to read: near={near} far={far}");
    }

    /// Two rooms with a wall between them exchange nothing, which is what
    /// makes sealing a bulkhead worth doing.
    #[test]
    fn air_does_not_cross_between_rooms() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<AirField>();
        app.add_systems(Update, (sync_air_tiles, diffuse_air, sync_room_air).chain());

        let mut room_map = RoomMap::default();
        room_map.tile_to_room.insert(IVec2::new(0, 0), 0);
        room_map.tile_to_room.insert(IVec2::new(1, 0), 1);
        room_map.rooms.push(Room {
            id: 0, tiles: vec![IVec2::new(0, 0)],
            air_level: 0.0, is_breached: false, has_power: false,
        });
        room_map.rooms.push(Room {
            id: 1, tiles: vec![IVec2::new(1, 0)],
            air_level: 1.0, is_breached: false, has_power: false,
        });
        app.insert_resource(room_map);
        app.world_mut().spawn(Ship);
        step(&mut app, 2.0, 20);

        assert!(
            pressure(&app, 1) > 0.99,
            "air crossed a wall into the vacuum next door: {}",
            pressure(&app, 1)
        );
    }

    /// Suction has to fall off as the compartment empties -- that taper is
    /// where "the pull depends on how much air is left" comes from, and it is
    /// the only thing stopping a near-empty room pinning crew forever.
    #[test]
    fn flow_weakens_as_the_room_empties() {
        let mut app = sim_app();
        corridor(&mut app, 5, Some(-1));

        step(&mut app, 0.1, 5);
        let early = app.world().resource::<AirField>().flow[&IVec2::new(0, 0)].length();
        step(&mut app, 4.0, 40);
        let late = app.world().resource::<AirField>().flow[&IVec2::new(0, 0)].length();

        assert!(
            late < early * 0.5,
            "flow did not taper as the room emptied: early={early} late={late}"
        );
    }

    /// Spawns a design as real entities under a ship and runs room detection
    /// plus the whole air pass over it.
    fn app_from_design(design: &crate::building::blueprint::Blueprint) -> App {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<AirField>();
        app.init_resource::<RoomMap>();
        app.add_message::<HullBreached>();
        app.add_systems(
            Update,
            (
                crate::building::rooms::update_room_map,
                mark_breached_hull,
                sync_air_tiles,
                vent_air_at_breaches,
                diffuse_air,
                sync_room_air,
            )
                .chain(),
        );

        let ship = app.world_mut().spawn(Ship).id();
        for cell in &design.hull_cells {
            app.world_mut().spawn((
                HullSegment {
                    health: 100.0,
                    max_health: 100.0,
                    radiation_shielding: 0.0,
                    is_depressurized: false,
                    depressurization_level: 0.0,
                    hull_layer: cell.layer,
                    material: cell.material,
                    grid_position: cell.grid_pos,
                },
                Transform::from_translation(
                    crate::building::grid_to_local(cell.grid_pos).extend(0.0),
                ),
                ChildOf(ship),
            ));
        }
        for module in &design.modules {
            app.world_mut().spawn((
                Module {
                    module_type: module.module_type,
                    health: 100.0,
                    max_health: 100.0,
                    power_consumption: 0.0,
                    power_generation: 0.0,
                    is_active: true,
                    grid_position: module.grid_pos,
                    size: IVec2::ONE,
                    rotation: module.rotation,
                },
                Transform::from_translation(
                    crate::building::grid_to_local(module.grid_pos).extend(0.0),
                ),
                ChildOf(ship),
            ));
        }
        app
    }

    /// The ship the player actually launches in must not be venting before a
    /// shot is fired.
    ///
    /// This is here because the obvious way to find a hole -- "no block in the
    /// cell next door means open space" -- is wrong on this ship. The shipped
    /// starter has 51 interior tiles with nothing plated beyond them, so that
    /// reading would have opened 51 holes at spawn and emptied the ship on the
    /// way out of the dock. Holes come from damage instead, and this is the
    /// test that says so.
    #[test]
    fn the_shipped_starter_design_is_airtight_at_spawn() {
        let design = crate::building::blueprint::load_design_file("designs/starter.json")
            .expect("designs/starter.json missing or unparseable");
        let mut app = app_from_design(&design);
        step(&mut app, 3.0, 30);

        let air = app.world().resource::<AirField>();
        assert!(
            !air.pressure.is_empty(),
            "no interior tiles detected — the test proves nothing"
        );
        assert!(
            air.vents.is_empty(),
            "{} tiles are venting on an undamaged ship: {:?}",
            air.vents.len(),
            air.vents.keys().take(8).collect::<Vec<_>>()
        );

        let worst = air
            .pressure
            .iter()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(cell, p)| (*cell, *p))
            .unwrap();
        assert!(
            worst.1 > 0.999,
            "tile {:?} lost air with the hull intact: {}",
            worst.0,
            worst.1
        );
    }

    /// The other half of airtight-at-spawn: once a plate is actually shot
    /// out, that ship had better start losing air through the gap.
    #[test]
    fn shooting_out_a_plate_vents_the_starter_ship() {
        let design = crate::building::blueprint::load_design_file("designs/starter.json")
            .expect("designs/starter.json missing or unparseable");
        let mut app = app_from_design(&design);
        step(&mut app, 0.1, 1); // one pass to detect rooms

        // Find a plate that actually borders interior space, so the test is
        // not silently holing an outboard armour slab that fronts nothing.
        let interior: Vec<IVec2> = app
            .world()
            .resource::<RoomMap>()
            .tile_to_room
            .keys()
            .copied()
            .collect();
        assert!(!interior.is_empty(), "no rooms detected on the starter ship");

        let mut target = None;
        let mut hulls = app.world_mut().query::<(Entity, &HullSegment)>();
        for (entity, segment) in hulls.iter(app.world()) {
            let pos = segment.grid_position;
            if interior.contains(&pos) {
                continue; // hallways are interior, not a wall to breach
            }
            if [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y]
                .iter()
                .any(|o| interior.contains(&(pos + *o)))
            {
                target = Some((entity, pos));
                break;
            }
        }
        let (plate, plate_cell) = target.expect("no hull plate borders any room");

        // Through the real path: ship::damage lowers the plate and reports a
        // HullBreached, and mark_breached_hull is what opens the hole.
        app.world_mut().get_mut::<HullSegment>(plate).unwrap().health = 0.0;
        app.world_mut()
            .resource_mut::<Messages<HullBreached>>()
            .write(HullBreached { segment: plate, severity: 1.0 });
        step(&mut app, 2.0, 20);

        let air = app.world().resource::<AirField>();
        assert!(
            air.holes.contains_key(&plate_cell),
            "a plate at zero health did not register as a hole"
        );
        assert!(
            !air.vents.is_empty(),
            "hull open at {plate_cell:?} but nothing is venting"
        );
        let drained = air.pressure.values().any(|p| *p < 0.95);
        assert!(drained, "hull is open and no tile lost meaningful air");
    }

    /// A sealed containment door has to actually divide the ship.
    ///
    /// It did not before: emergency bulkheads are MODULES and
    /// `fire::emergency_bulkhead_system` marked them `BulkheadSealed`, but
    /// `update_room_map` only ever collected sealed HULL segments, so the
    /// marker was read by nobody and the door held nothing back.
    #[test]
    fn a_sealed_door_module_splits_the_ship_in_two() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<AirField>();
        app.init_resource::<RoomMap>();
        app.add_systems(Update, crate::building::rooms::update_room_map);

        let ship = app.world_mut().spawn(Ship).id();

        // Two hallway cells with a valve module between them.
        for x in [0, 2] {
            app.world_mut().spawn((
                HullSegment {
                    health: 100.0,
                    max_health: 100.0,
                    radiation_shielding: 0.0,
                    is_depressurized: false,
                    depressurization_level: 0.0,
                    hull_layer: HullLayer::Hallway,
                    material: HullMaterial::Steel,
                    grid_position: IVec2::new(x, 0),
                },
                Transform::from_translation(
                    crate::building::grid_to_local(IVec2::new(x, 0)).extend(0.0),
                ),
                ChildOf(ship),
            ));
        }
        let valve = app
            .world_mut()
            .spawn((
                Module {
                    module_type: ModuleType::AirlockValve,
                    health: 100.0,
                    max_health: 100.0,
                    power_consumption: 0.0,
                    power_generation: 0.0,
                    is_active: true,
                    grid_position: IVec2::new(1, 0),
                    size: IVec2::ONE,
                    rotation: Rotation::North,
                },
                Transform::from_translation(
                    crate::building::grid_to_local(IVec2::new(1, 0)).extend(0.0),
                ),
                ChildOf(ship),
            ))
            .id();

        app.update();
        assert_eq!(
            app.world().resource::<RoomMap>().rooms.len(),
            1,
            "an open valve should join the two sides into one compartment"
        );

        app.world_mut().entity_mut(valve).insert(BulkheadSealed);
        app.update();

        let room_map = app.world().resource::<RoomMap>();
        assert_eq!(
            room_map.rooms.len(),
            2,
            "a sealed valve must divide the ship — it left {} room(s)",
            room_map.rooms.len()
        );
        assert_ne!(
            room_map.tile_to_room.get(&IVec2::new(0, 0)),
            room_map.tile_to_room.get(&IVec2::new(2, 0)),
            "both sides still resolve to the same compartment"
        );
    }

    /// And once divided, air must not cross it — the reason you would ever
    /// shut one.
    #[test]
    fn shutting_a_door_saves_the_compartment_behind_it() {
        let mut app = sim_app();
        // Six cells, holed past the x=0 end. Tiles 0..2 are the doomed side,
        // 3..5 the side we are trying to keep.
        corridor(&mut app, 6, Some(-1));
        step(&mut app, 4.0, 40);
        let unsealed_far = pressure(&app, 5);

        // Same again, with the halves detached the way a shut door leaves them.
        let mut app = sim_app();
        corridor(&mut app, 6, Some(-1));
        {
            let mut room_map = app.world_mut().resource_mut::<RoomMap>();
            room_map.tile_to_room.clear();
            room_map.rooms.clear();
            for (id, range) in [(0usize, 0..3), (1usize, 3..6)] {
                let tiles: Vec<IVec2> = range.map(|x| IVec2::new(x, 0)).collect();
                for &t in &tiles {
                    room_map.tile_to_room.insert(t, id);
                }
                room_map.rooms.push(crate::building::rooms::Room {
                    id,
                    tiles,
                    air_level: 1.0,
                    is_breached: false,
                    has_power: false,
                });
            }
        }
        step(&mut app, 4.0, 40);
        let sealed_far = pressure(&app, 5);

        assert!(
            sealed_far > 0.99,
            "the protected side lost air through a shut door: {sealed_far}"
        );
        assert!(
            unsealed_far < sealed_far - 0.1,
            "shutting the door made no difference: open={unsealed_far} shut={sealed_far}"
        );
    }

    /// Armour plating is not a room.
    ///
    /// Every module counted as interior space, and plating is a module, so the
    /// ship kept its atmosphere in its armour: 32 of the starter's 84 interior
    /// tiles were exterior plating and 7 of its 11 compartments contained
    /// nothing else. Once air became a fluid those pockets started venting
    /// compartments nobody could ever have stood in, and they inflated the air
    /// volume the whole simulation balances against.
    #[test]
    fn armour_plating_is_not_breathable_space() {
        let design = crate::building::blueprint::load_design_file("designs/starter.json")
            .expect("designs/starter.json missing or unparseable");
        let mut app = app_from_design(&design);
        step(&mut app, 0.1, 1);

        let plating: Vec<IVec2> = design
            .modules
            .iter()
            .filter(|m| !m.module_type.holds_atmosphere())
            .map(|m| m.grid_pos)
            .collect();
        assert!(!plating.is_empty(), "starter has no plating — test proves nothing");

        let room_map = app.world().resource::<RoomMap>();
        let leaked: Vec<&IVec2> = plating
            .iter()
            .filter(|c| room_map.tile_to_room.contains_key(c))
            .collect();
        assert!(
            leaked.is_empty(),
            "{} armour plates are being counted as room space: {:?}",
            leaked.len(),
            leaked.iter().take(6).collect::<Vec<_>>()
        );
    }

    /// fire.rs and crew_emergency_dispatch still read Room::air_level, so it
    /// has to keep tracking the tiles it summarises.
    #[test]
    fn room_air_level_tracks_the_tile_mean() {
        let mut app = sim_app();
        corridor(&mut app, 4, Some(-1));
        step(&mut app, 1.0, 20);

        let air = app.world().resource::<AirField>();
        let expected: f32 =
            (0..4).map(|x| air.pressure[&IVec2::new(x, 0)]).sum::<f32>() / 4.0;
        let room_map = app.world().resource::<RoomMap>();
        let actual = room_map.rooms[0].air_level;

        assert!(
            (actual - expected).abs() < 1e-5,
            "room air_level {actual} drifted from its tile mean {expected}"
        );
        assert!(room_map.rooms[0].is_breached, "room with a hole on it is not flagged breached");
    }
}

// ============================================================================
// CONTAINMENT
// ============================================================================

/// Adjacent pressure at which a containment door decides to shut.
const SEAL_AT: f32 = 0.7;

/// And the pressure it wants back before opening again. The gap is deliberate:
/// with a live flow field the seal threshold is crossed and re-crossed
/// constantly, and a door with one threshold flaps open and shut every frame.
const UNSEAL_AT: f32 = 0.95;

/// How long a door warns before it shuts.
const CLOSING_DELAY: f32 = 2.0;

/// How long someone stays off duty running for it. Generous enough to cross a
/// compartment, short enough that being caught on the wrong side does not
/// retire them.
const FLEE_TIMEOUT: f32 = 8.0;

/// Automatic bulkheads and flood valves: shut on falling pressure, having
/// given anyone in the doomed compartment a moment to get out.
///
/// Replaces `fire::emergency_bulkhead_system`, which read the room-wide mean
/// air level. A mean lags badly here -- air nearest the hole goes first, so a
/// door beside the breach would wait for the whole compartment to average down
/// before reacting. Reading the pressure of the tiles it actually touches
/// makes it shut as the wave arrives.
pub fn auto_containment(
    mut commands: Commands,
    time: Res<Time>,
    air: Res<AirField>,
    room_map: Res<RoomMap>,
    ship_query: Query<Entity, With<Ship>>,
    mut doors: Query<
        (
            Entity,
            &Module,
            &ChildOf,
            Has<BulkheadSealed>,
            Option<&mut BulkheadClosing>,
        ),
        Without<DestroyedModule>,
    >,
    crew: Query<(Entity, &Transform), (With<CrewMember>, Without<crate::crew::eva_salvage::EvaSalvaging>)>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let Ok(player_ship) = ship_query.single() else { return };
    let dt = time.delta();

    for (entity, module, parent, sealed, closing) in doors.iter_mut() {
        if parent.parent() != player_ship || !module.module_type.is_containment_door() {
            continue;
        }
        if !module.is_active {
            continue;
        }

        let sides = [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y];
        let neighbours: Vec<(IVec2, f32)> = sides
            .iter()
            .map(|o| module.grid_position + *o)
            .filter(|c| room_map.tile_to_room.contains_key(c))
            .map(|c| (c, air.pressure.get(&c).copied().unwrap_or(1.0)))
            .collect();
        if neighbours.is_empty() {
            continue;
        }

        let worst = neighbours
            .iter()
            .copied()
            .fold((IVec2::ZERO, f32::MAX), |acc, n| if n.1 < acc.1 { n } else { acc });
        let best = neighbours
            .iter()
            .copied()
            .fold((IVec2::ZERO, f32::MIN), |acc, n| if n.1 > acc.1 { n } else { acc });

        if sealed {
            if worst.1 > UNSEAL_AT {
                commands.entity(entity).remove::<BulkheadSealed>();
                notifications.write(ShowNotification {
                    message: "Bulkhead reopened - pressure restored.".into(),
                    notification_type: NotificationType::Info,
                    duration: 2.0,
                });
            }
            continue;
        }

        if let Some(mut closing) = closing {
            closing.timer.tick(dt);
            if closing.timer.is_finished() {
                commands
                    .entity(entity)
                    .remove::<BulkheadClosing>()
                    .insert(BulkheadSealed);
                notifications.write(ShowNotification {
                    message: "Bulkhead sealed - compartment isolated.".into(),
                    notification_type: NotificationType::Danger,
                    duration: 3.0,
                });
            } else if worst.1 > UNSEAL_AT {
                // The leak was dealt with while the clock ran; stand down.
                commands.entity(entity).remove::<BulkheadClosing>();
            }
            continue;
        }

        if worst.1 >= SEAL_AT {
            continue;
        }

        // Shutting. Anyone on the losing side gets told to run for the door,
        // and `Fleeing` outranks their station assignment so the order sticks
        // long enough for them to move.
        commands.entity(entity).insert(BulkheadClosing {
            timer: Timer::from_seconds(CLOSING_DELAY, TimerMode::Once),
        });

        let doomed_room = room_map.tile_to_room.get(&worst.0).copied();
        let refuge = if best.1 > worst.1 { Some(best.0) } else { None };
        let mut running = 0u32;
        if let (Some(doomed), Some(refuge)) = (doomed_room, refuge) {
            for (crew_entity, transform) in crew.iter() {
                let cell = local_to_grid(transform.translation.truncate());
                if room_map.tile_to_room.get(&cell).copied() != Some(doomed) {
                    continue;
                }
                commands
                    .entity(crew_entity)
                    .insert((
                        Fleeing {
                            timer: Timer::from_seconds(FLEE_TIMEOUT, TimerMode::Once),
                        },
                        crate::crew::walking::CrewDestination(refuge),
                    ));
                running += 1;
            }
        }

        notifications.write(ShowNotification {
            message: if running > 0 {
                format!("BULKHEAD CLOSING - {running} crew running for it!")
            } else {
                "Bulkhead closing - decompression detected.".into()
            },
            notification_type: NotificationType::Warning,
            duration: CLOSING_DELAY,
        });
    }
}

/// Lets people stop running once they are somewhere with air.
pub fn clear_fleeing(
    mut commands: Commands,
    time: Res<Time>,
    air: Res<AirField>,
    mut fleeing: Query<(Entity, &Transform, &mut Fleeing)>,
) {
    let dt = time.delta();
    for (entity, transform, mut flee) in fleeing.iter_mut() {
        flee.timer.tick(dt);
        let cell = local_to_grid(transform.translation.truncate());
        let safe = air.pressure.get(&cell).copied().unwrap_or(1.0) > UNSEAL_AT;
        if safe || flee.timer.is_finished() {
            commands.entity(entity).remove::<Fleeing>();
        }
    }
}
