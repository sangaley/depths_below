use bevy::prelude::*;
use crate::components::*;
use crate::resources::*;
use crate::states::GameState;
use crate::events::*;

/// Updates depth and noise state.
/// engine_query is scoped to the player's own ship (via ChildOf) — Engine is
/// shared with AI ships, so unscoped this summed every AI ship's engine
/// noise in the world into the player's own NoiseState (the same
/// cross-ship-contamination pattern fixed everywhere else this session).
pub fn update_ship_state(
    ship_query: Query<(Entity, &Depth), With<Ship>>,
    engine_query: Query<(&Engine, &Module, &ChildOf)>,
    mut depth_state: ResMut<DepthState>,
    mut noise_state: ResMut<NoiseState>,
) {
    let Ok((player_ship, depth)) = ship_query.single() else { return };

    // Update depth
    depth_state.current_depth = depth.0;

    // Calculate noise level from the player's own active engines
    let noise: f32 = engine_query
        .iter()
        .filter(|(_, module, parent)| module.is_active && parent.parent() == player_ship)
        .map(|(engine, _, _)| engine.noise_level)
        .sum();

    noise_state.noise_level = noise;
}

/// Checks game over conditions
pub fn check_game_over(
    hull_state: Res<HullState>,
    oxygen_state: Res<OxygenState>,
    crew_query: Query<&CrewMember>,
    mut death_cause: ResMut<DeathCause>,
    mut next_state: ResMut<NextState<GameState>>,
    mut notifications: MessageWriter<ShowNotification>,
    mut o2_depleted_timer: Local<f32>,
    session_timer: Res<ExploringSessionTimer>,
    time: Res<Time>,
) {
    // Grace period: allow 3 seconds for all initialization systems to complete
    // (crew spawning, hull setup, etc. may take a frame to flush commands)
    if session_timer.elapsed < 3.0 {
        return;
    }

    let crew_count = crew_query.iter().count();
    let all_crew_dead = crew_count == 0 || crew_query.iter().all(|c| c.health <= 0.0);
    let hull_destroyed = hull_state.hull_integrity <= 0.0;

    // Phase 3.5: Oxygen depletion game over after 30 seconds at zero
    if oxygen_state.current_oxygen <= 0.0 {
        *o2_depleted_timer += time.delta_secs();
    } else {
        *o2_depleted_timer = 0.0;
    }
    let o2_game_over = *o2_depleted_timer > 30.0;

    // Attribute the death to the most recent damage source if it was fresh
    // (within 20s) — "hull destroyed" alone doesn't tell the player anything.
    let recent_damage = death_cause.last_damage.as_ref()
        .filter(|(_, at)| time.elapsed_secs_f64() - at < 20.0)
        .map(|(desc, _)| desc.clone());
    let attribution = recent_damage
        .map(|d| format!(" Cause: {}.", d))
        .unwrap_or_default();

    if all_crew_dead {
        death_cause.cause = Some(format!("All crew died.{}", attribution));
        notifications.write(ShowNotification {
            message: "All crew lost. The ship drifts silently into the void...".into(),
            notification_type: NotificationType::Danger,
            duration: 5.0,
        });
        next_state.set(GameState::GameOver);
    } else if hull_destroyed {
        death_cause.cause = Some(format!("Hull integrity reached zero.{}", attribution));
        notifications.write(ShowNotification {
            message: "Hull integrity zero! The ship is breaking apart!".into(),
            notification_type: NotificationType::Danger,
            duration: 5.0,
        });
        next_state.set(GameState::GameOver);
    } else if o2_game_over {
        death_cause.cause = Some("Crew suffocated — oxygen held at zero for 30 seconds. Check scrubber count, staffing, and power.".to_string());
        notifications.write(ShowNotification {
            message: "Life support failure! No oxygen remaining!".into(),
            notification_type: NotificationType::Danger,
            duration: 5.0,
        });
        next_state.set(GameState::GameOver);
    }
}

/// Updates inventory max_capacity based on cargo hold modules
pub fn update_inventory_capacity(
    cargo_query: Query<(&CargoHold, &Module)>,
    mut inventory: ResMut<Inventory>,
) {
    let base_capacity = 50.0f32;
    let cargo_bonus: f32 = cargo_query
        .iter()
        .filter(|(_, module)| module.is_active)
        .map(|(cargo, _)| cargo.capacity)
        .sum();
    inventory.max_capacity = base_capacity + cargo_bonus;
}

/// Checks if the player has achieved victory (reached 2500m depth + found final log)
pub fn check_victory(
    depth_state: Res<DepthState>,
    statistics: Res<Statistics>,
    mut victory_state: ResMut<VictoryState>,
    mut next_state: ResMut<NextState<GameState>>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    if victory_state.achieved {
        return;
    }

    // Victory requires reaching 2200m+ depth AND finding the final log.
    // The [UNTITLED] log spawns at depth_level 20-23 (2000-2300m range).
    if depth_state.current_depth >= 2200.0
        && statistics.logs_found.iter().any(|l| l == "[UNTITLED]")
    {
        victory_state.achieved = true;
        notifications.write(ShowNotification {
            message: "You have reached the deepest point and uncovered the final truth. VICTORY!".into(),
            notification_type: NotificationType::Success,
            duration: 8.0,
        });
        next_state.set(GameState::GameOver);
    }
}

/// Resets victory state when returning to surface
pub fn reset_victory_state(
    mut victory_state: ResMut<VictoryState>,
    mut death_cause: ResMut<DeathCause>,
) {
    victory_state.achieved = false;
    death_cause.cause = None;
    death_cause.last_damage = None;
}

/// Tracks statistics during gameplay
pub fn update_statistics(
    time: Res<Time>,
    depth_state: Res<DepthState>,
    mut statistics: ResMut<Statistics>,
) {
    statistics.play_time_seconds += time.delta_secs();

    if depth_state.current_depth > statistics.max_depth_reached {
        statistics.max_depth_reached = depth_state.current_depth;
    }
}

/// Ticks the exploring session timer each frame
pub fn tick_session_timer(time: Res<Time>, mut timer: ResMut<ExploringSessionTimer>) {
    timer.elapsed += time.delta_secs();
}

/// Resets the exploring session timer (called on OnEnter(Exploring))
pub fn reset_session_timer(mut timer: ResMut<ExploringSessionTimer>) {
    timer.elapsed = 0.0;
}

/// Cleans up all game entities when returning to main menu (after game over or restart).
/// This prevents stale entities from accumulating across play sessions.
pub fn cleanup_game_entities(
    mut commands: Commands,
    ships: Query<Entity, With<Ship>>,
    creatures: Query<Entity, With<Creature>>,
    chunks: Query<Entity, With<Chunk>>,
    pois: Query<Entity, With<PointOfInterest>>,
    mut chunk_manager: ResMut<ChunkManager>,
    mut roster: ResMut<CrewRoster>,
    mut depth_state: ResMut<DepthState>,
    mut oxygen_state: ResMut<OxygenState>,
    mut hull_state: ResMut<HullState>,
    mut fuel_state: ResMut<FuelState>,
    mut noise_state: ResMut<NoiseState>,
) {
    // Despawn ship and all children (modules, hull segments, crew)
    for entity in ships.iter() {
        commands.entity(entity).despawn();
    }
    // Despawn creatures
    for entity in creatures.iter() {
        commands.entity(entity).despawn();
    }
    // Despawn chunks (and their children: POIs, decorations)
    for entity in chunks.iter() {
        commands.entity(entity).despawn();
    }
    // Despawn any orphaned POIs
    for entity in pois.iter() {
        commands.entity(entity).despawn();
    }
    // Reset chunk manager
    chunk_manager.loaded_chunks.clear();
    // Reset crew roster
    roster.members.clear();
    // Reset resources to defaults
    depth_state.current_depth = 0.0;
    oxygen_state.current_oxygen = 1000.0;
    oxygen_state.max_oxygen = 1000.0;
    hull_state.hull_integrity = 1.0;
    hull_state.max_radiation_shielding = 0.0;
    fuel_state.current_fuel = fuel_state.max_fuel;
    noise_state.noise_level = 0.0;

    info!("Game entities cleaned up for restart");
}
