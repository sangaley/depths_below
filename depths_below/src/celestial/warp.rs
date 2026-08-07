use bevy::prelude::*;
use crate::components::{Ship, Velocity};
use crate::events::{ShowNotification, NotificationType};
use crate::resources::FuelState;
use super::components::*;
use super::resources::*;
use super::events::*;
use super::galaxy;

// Interstellar warp cost/time, scaled by galaxy-map distance (t = dist /
// GALAXY_RADIUS, 0..1) — deliberately separate from the G-key local warp
// dash (ui/mod.rs's WARP_DASH_* constants): that's a same-system reposition
// bounded by a ~600k-unit local map, this is a jump across the whole
// galaxy, orders of magnitude larger. Strawman numbers, tune by feel.
const INTERSTELLAR_BASE_CHARGE: f32 = 1.0; // seconds
const INTERSTELLAR_MAX_EXTRA_CHARGE: f32 = 3.0; // up to 4s at the far edge — was 6-60s, way too slow
const INTERSTELLAR_BASE_FUEL: f32 = 80.0;
const INTERSTELLAR_MAX_EXTRA_FUEL: f32 = 420.0; // up to 500 at the far edge

// pub(crate): reused by ui/mod.rs for the galaxy map's cost/charge preview
// so the sidebar shows the exact numbers the jump will actually use.
pub(crate) fn interstellar_charge_time(t: f32) -> f32 {
    INTERSTELLAR_BASE_CHARGE + t.clamp(0.0, 1.0) * INTERSTELLAR_MAX_EXTRA_CHARGE
}

pub(crate) fn interstellar_fuel_cost(t: f32) -> f32 {
    INTERSTELLAR_BASE_FUEL + t.clamp(0.0, 1.0) * INTERSTELLAR_MAX_EXTRA_FUEL
}

pub(crate) fn target_galaxy_pos(galaxy_map: &GalaxyMap, target: GalaxyWarpTarget) -> Option<Vec2> {
    match target {
        GalaxyWarpTarget::System(id) => galaxy_map.systems.iter().find(|s| s.id == id).map(|s| s.galaxy_pos),
        GalaxyWarpTarget::BlindPoint(pos) => Some(pos),
    }
}

/// V key initiates warp charge toward whatever the galaxy map has set as
/// the pending target (PendingGalaxyWarpTarget) — a known system, or a
/// blind point in space with nothing confirmed there (the galaxy map is
/// clickable anywhere, not just discovered pips — see ui/mod.rs). Press
/// once and the charge runs on its own — no need to hold the key through
/// what can be up to a 60s charge; press V again to cancel early. Charge
/// time and fuel cost both scale with galaxy-map distance to the target.
pub fn warp_input_system(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    galaxy_map: Res<GalaxyMap>,
    streaming: Res<SystemStreamingManager>,
    pending: Res<PendingGalaxyWarpTarget>,
    mut fuel_state: ResMut<FuelState>,
    ship_query: Query<Entity, (With<Ship>, Without<WarpCharging>)>,
    mut charging_query: Query<(Entity, &mut WarpCharging), With<Ship>>,
    mut notifications: MessageWriter<ShowNotification>,
    mut jump_events: MessageWriter<WarpJumpStarted>,
) {
    // V starts the charge and it runs on its own from there — no need to
    // hold the key through the whole (up to 60s) charge, unlike the G-key
    // local dash. A second press while already charging cancels instead.
    if keyboard.just_pressed(KeyCode::KeyV) {
        if let Ok((entity, _)) = charging_query.single() {
            commands.entity(entity).remove::<WarpCharging>();
            notifications.write(ShowNotification {
                message: "Warp cancelled.".into(),
                notification_type: NotificationType::Info,
                duration: 2.0,
            });
        } else if let Ok(ship_entity) = ship_query.single() {
            let Some(target) = pending.0 else {
                notifications.write(ShowNotification {
                    message: "No warp destination set — open the map (M), Tab to the galaxy view, and click anywhere.".into(),
                    notification_type: NotificationType::Warning,
                    duration: 3.0,
                });
                return;
            };
            let Some(target_pos) = target_galaxy_pos(&galaxy_map, target) else { return };

            let dist = streaming.current_galaxy_pos.distance(target_pos);
            if dist < 1.0 {
                notifications.write(ShowNotification {
                    message: "Already there.".into(),
                    notification_type: NotificationType::Info,
                    duration: 2.5,
                });
                return;
            }

            let t = dist / galaxy::GALAXY_RADIUS;
            let fuel_cost = interstellar_fuel_cost(t);
            if fuel_state.current_fuel < fuel_cost {
                notifications.write(ShowNotification {
                    message: format!("Not enough fuel for the jump ({:.0} needed, {:.0} available).", fuel_cost, fuel_state.current_fuel),
                    notification_type: NotificationType::Warning,
                    duration: 3.0,
                });
                return;
            }
            let charge_time = interstellar_charge_time(t);
            let target_desc = match target {
                GalaxyWarpTarget::System(id) => galaxy_map.systems.iter().find(|s| s.id == id).map(|s| s.name.clone()).unwrap_or_default(),
                GalaxyWarpTarget::BlindPoint(_) => "uncharted space".to_string(),
            };

            commands.entity(ship_entity).insert(WarpCharging {
                target,
                fuel_cost,
                charge_timer: Timer::from_seconds(charge_time, TimerMode::Once),
            });

            notifications.write(ShowNotification {
                message: format!("Warp drive charging toward {}... ({:.0}s, press V again to cancel)", target_desc, charge_time),
                notification_type: NotificationType::Info,
                duration: charge_time + 1.0,
            });
        }
    }

    // Tick charge timer
    if let Ok((entity, mut charging)) = charging_query.single_mut() {
        charging.charge_timer.tick(time.delta());

        // Progress notifications
        let pct = charging.charge_timer.fraction();
        if pct > 0.5 && pct < 0.55 {
            notifications.write(ShowNotification {
                message: "Warp drive at 50%...".into(),
                notification_type: NotificationType::Warning,
                duration: 1.5,
            });
        }

        // Jump when charge completes
        if charging.charge_timer.is_finished() {
            let target = charging.target;
            fuel_state.current_fuel = (fuel_state.current_fuel - charging.fuel_cost).max(0.0);
            commands.entity(entity).remove::<WarpCharging>();

            jump_events.write(WarpJumpStarted { target });
        }
    }
}

/// Execute the warp jump. A System target loads that system deterministically
/// (as before). A BlindPoint target either snaps to arriving at a real
/// system if one is within SNAP_TOLERANCE of it, or lands in genuinely
/// empty space — nothing spawns, loaded_system becomes None, but the
/// player still has a real local position and Warm neighbors keep ticking
/// based on proximity to that point. Either way, unloads the old system
/// first and refreshes Warm neighbors around the new position.
pub fn execute_warp_jump(
    mut commands: Commands,
    mut jump_events: MessageReader<WarpJumpStarted>,
    mut galaxy: ResMut<GalaxyState>,
    mut galaxy_map: ResMut<GalaxyMap>,
    mut streaming: ResMut<SystemStreamingManager>,
    mut pending: ResMut<PendingGalaxyWarpTarget>,
    celestial_query: Query<(Entity, &StarSystemMember)>,
    mut ship_query: Query<(&mut Transform, &mut Velocity), With<Ship>>,
    mut notifications: MessageWriter<ShowNotification>,
    mut completed_events: MessageWriter<WarpJumpCompleted>,
    textures: Res<crate::vfx::procedural_textures::CelestialTextures>,
    asset_server: Res<AssetServer>,
    time: Res<Time>,
    mut sim: ResMut<crate::ai_ship::components::WorldSimulation>,
) {
    for event in jump_events.read() {
        let Ok((mut ship_transform, mut ship_velocity)) = ship_query.single_mut() else {
            continue;
        };

        let now = time.elapsed_secs_f64();

        // Resolve the target into either a real system id (snapping a
        // close-enough blind point onto it) or a raw empty-space position.
        let resolved_system_id = match event.target {
            GalaxyWarpTarget::System(id) => Some(id),
            GalaxyWarpTarget::BlindPoint(pos) => galaxy::system_within_snap_tolerance(&galaxy_map, pos),
        };

        if let Some(old_id) = streaming.loaded_system {
            galaxy::unload_system(&mut commands, &celestial_query, &mut galaxy_map, old_id, now);
        }

        let (new_galaxy_pos, arrival_name) = if let Some(new_id) = resolved_system_id {
            let Some(system_info) = galaxy::load_system(&mut commands, &asset_server, &textures, &mut galaxy_map, new_id, now) else {
                continue;
            };
            let Some(def) = galaxy_map.systems.iter().find(|s| s.id == new_id) else { continue };
            let center = def.local_center;
            let galaxy_pos = def.galaxy_pos;
            let name = def.name.clone();

            // Land IN the faction's population cluster, not just "somewhere
            // safe near the star" — that flat 50k-100k distance used to put
            // the player nowhere near a weak faction's tight cluster (e.g.
            // RustSwarm's whole territory is only 15k radius), landing them
            // in what looked like empty space every time. The cluster is
            // offset clear of the star itself (see
            // celestial::galaxy::faction_cluster_center), so arriving near
            // it can't accidentally drop the player inside/on top of the
            // star regardless of faction or star size.
            let arrival_pos = if def.faction.is_some() {
                let cluster_center = galaxy::faction_cluster_center(def);
                let territory_radius = def.faction
                    .and_then(|f| crate::ai_ship::components::faction_territories().into_iter().find(|t| t.faction == f))
                    .map(|t| t.radius)
                    .unwrap_or(30_000.0);
                let arrival_angle = rand::random::<f32>() * std::f32::consts::TAU;
                cluster_center + Vec2::new(arrival_angle.cos(), arrival_angle.sin()) * territory_radius * 0.5
            } else {
                // Safe system (Haven) — no faction cluster, just clear the star.
                let safe_distance = system_info.star_entity.map(|_| 100_000.0).unwrap_or(50_000.0);
                let arrival_angle = rand::random::<f32>() * std::f32::consts::TAU;
                center + Vec2::new(arrival_angle.cos(), arrival_angle.sin()) * safe_distance
            };
            ship_transform.translation.x = arrival_pos.x;
            ship_transform.translation.y = arrival_pos.y;

            streaming.loaded_system = Some(new_id);
            galaxy.systems.push(system_info);
            galaxy.current_system = new_id;

            (galaxy_pos, name)
        } else {
            // Genuinely empty space — nothing to spawn, nothing to load.
            let GalaxyWarpTarget::BlindPoint(pos) = event.target else { continue };
            let arrival_pos = galaxy::blind_point_local_center(pos);
            ship_transform.translation.x = arrival_pos.x;
            ship_transform.translation.y = arrival_pos.y;

            streaming.loaded_system = None;

            (pos, "uncharted space".to_string())
        };

        ship_velocity.0 = Vec2::ZERO; // Kill momentum on arrival
        streaming.current_galaxy_pos = new_galaxy_pos;
        streaming.warm_systems = galaxy::nearest_neighbors_to_pos(&galaxy_map, new_galaxy_pos, streaming.loaded_system, galaxy::WARM_NEIGHBOR_COUNT);
        pending.0 = None;

        // First-ever visit populates a system's faction ships; a repeat
        // visit leaves whatever's left (however depleted by earlier combat)
        // exactly as it is — see ensure_system_population's doc comment.
        if let Some(new_id) = streaming.loaded_system {
            crate::ai_ship::simulation::ensure_system_population(&mut sim, &galaxy_map, new_id);
        }
        for warm_id in streaming.warm_systems.clone() {
            crate::ai_ship::simulation::ensure_system_population(&mut sim, &galaxy_map, warm_id);
        }

        completed_events.write(WarpJumpCompleted { system_id: streaming.loaded_system });

        notifications.write(ShowNotification {
            message: format!("Warp complete! Arrived at {}", arrival_name),
            notification_type: NotificationType::Success,
            duration: 4.0,
        });
    }
}

/// When warp completes, notify the player about the new system (skipped
/// for a blind landing in empty space — nothing to scan).
pub fn on_warp_complete(
    mut completed_events: MessageReader<WarpJumpCompleted>,
    galaxy: Res<GalaxyState>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for event in completed_events.read() {
        let Some(system_id) = event.system_id else { continue };
        if let Some(system) = galaxy.systems.iter().find(|s| s.id == system_id) {
            let planet_count = system.planet_entities.len();
            notifications.write(ShowNotification {
                message: format!("System scan: {} planets detected. Proceed with caution.", planet_count),
                notification_type: NotificationType::Info,
                duration: 5.0,
            });
        }
    }
}
