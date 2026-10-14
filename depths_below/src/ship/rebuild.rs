use bevy::prelude::*;
use crate::components::*;
use crate::events::*;
use crate::resources::*;
use crate::building::{GridOccupancy, ModuleRegistry};
use crate::building::footprints;

// ============================================================================
// GHOST REBUILD — the ship remembers its own design.
//
// When a player block is destroyed (queued for removal), a ghost of what
// stood there is recorded: faint outline sprites on the hull, an entry in
// the RebuildQueue. Idle crew reconstruct ghosts one at a time in flight,
// paying ScrapMetal per block. Docking and rebuilding manually over a ghost
// cell simply clears it.
// ============================================================================

/// Seconds of work (at one idle crew) to rebuild a hull block.
const HULL_WORK: f32 = 5.0;
/// Seconds of work (at one idle crew) to rebuild a module.
const MODULE_WORK: f32 = 10.0;
/// ScrapMetal per hull block.
const HULL_SCRAP: u32 = 2;
/// Module scrap cost: max(2, cost / this divisor).
const MODULE_SCRAP_DIVISOR: u32 = 20;

#[derive(Clone, Copy, Debug)]
pub enum GhostKind {
    Hull { layer: HullLayer, material: HullMaterial },
    Module { module_type: ModuleType, rotation: Rotation },
}

pub struct GhostBlock {
    pub grid_position: IVec2,
    pub kind: GhostKind,
    /// Every grid cell the block covered (multi-cell modules included) —
    /// cleanup drops the ghost if ANY of them gets built over manually.
    pub cells: Vec<IVec2>,
    /// Seconds of crew work accumulated.
    pub progress: f32,
    pub work_needed: f32,
    pub scrap_cost: u32,
    /// Scrap is paid once, when work on this ghost begins.
    pub scrap_paid: bool,
    /// Ghost outline sprites (children of the ship).
    pub sprites: Vec<Entity>,
}

#[derive(Resource, Default)]
pub struct RebuildQueue {
    pub ghosts: Vec<GhostBlock>,
}

/// Marker for ghost outline sprites.
#[derive(Component)]
pub struct RebuildGhostSprite;

fn ghost_name(kind: &GhostKind) -> String {
    match kind {
        GhostKind::Hull { material, .. } => format!("{} hull", material.name()),
        GhostKind::Module { module_type, .. } => module_type.name().to_string(),
    }
}

/// Records a rebuild ghost for every PLAYER block queued for destruction
/// removal. AI ships get no ghosts — their wrecks are salvage, not repairs.
pub fn record_rebuild_ghosts(
    mut commands: Commands,
    fresh: Query<
        (Entity, &ChildOf, Option<&Module>, Option<&HullSegment>),
        Added<PendingRemoval>,
    >,
    ship_query: Query<Entity, With<Ship>>,
    registry: Res<ModuleRegistry>,
    mut queue: ResMut<RebuildQueue>,
) {
    let Ok(ship) = ship_query.single() else { return };

    for (_entity, parent, module, hull) in fresh.iter() {
        if parent.parent() != ship {
            continue;
        }

        let (grid_position, kind, work_needed, scrap_cost, cells, color) =
            if let Some(module) = module {
                let def = registry.get(module.module_type);
                let footprint = footprints::footprint_override(module.module_type);
                let cells = GridOccupancy::cells_for(
                    module.grid_position, def.size, module.rotation, footprint,
                );
                (
                    module.grid_position,
                    GhostKind::Module {
                        module_type: module.module_type,
                        rotation: module.rotation,
                    },
                    MODULE_WORK,
                    (def.cost / MODULE_SCRAP_DIVISOR).max(2),
                    cells,
                    def.color,
                )
            } else if let Some(hull) = hull {
                let cells = GridOccupancy::cells_for(
                    hull.grid_position, IVec2::ONE, Rotation::North, None,
                );
                (
                    hull.grid_position,
                    GhostKind::Hull {
                        layer: hull.hull_layer,
                        material: hull.material,
                    },
                    HULL_WORK,
                    HULL_SCRAP,
                    cells,
                    Color::WHITE,
                )
            } else {
                continue;
            };

        // One ghost per origin cell — repeated destruction of the same spot
        // (e.g. rebuilt then shot off again) must not stack entries.
        if queue.ghosts.iter().any(|g| g.grid_position == grid_position) {
            continue;
        }

        // Faint outline where the block stood — subtle, reads as a memory
        let ghost_color = color.with_alpha(0.15);
        let mut sprites = Vec::new();
        for &cell in &cells {
            let sprite = commands.spawn((
                (Sprite {
                        color: ghost_color,
                        custom_size: Some(Vec2::splat(58.0)),
                        ..default()
                    }, Transform::from_xyz(
                        cell.x as f32 * 66.0,
                        cell.y as f32 * 66.0 - 33.0,
                        0.05,
                    )),
                RebuildGhostSprite,
                ChildOf(ship),
            )).id();
            sprites.push(sprite);
        }

        queue.ghosts.push(GhostBlock {
            grid_position,
            kind,
            cells: cells.to_vec(),
            progress: 0.0,
            work_needed,
            scrap_cost,
            scrap_paid: false,
            sprites,
        });
    }
}

/// Idle crew reconstruct ghosts one at a time (FIFO), paying scrap up front.
/// Runs while exploring — this is field construction, dock rebuilding stays
/// manual in build mode.
pub fn crew_rebuild_system(
    mut commands: Commands,
    time: Res<Time>,
    // The player's hands only. AI crew are CrewMembers too, spawned Idle and
    // never changed, so every enemy crew in the world was rebuilding our hull.
    crew_query: Query<(Entity, &CrewMember), Without<crate::ai_ship::components::OwnedByAiShip>>,
    stations: Query<&CrewStation>,
    mut queue: ResMut<RebuildQueue>,
    mut inventory: ResMut<Inventory>,
    mut place_hull_events: MessageWriter<PlaceHullRequest>,
    mut place_module_events: MessageWriter<PlaceModuleRequest>,
    mut notifications: MessageWriter<ShowNotification>,
    mut stall_notified: Local<bool>,
) {
    if queue.ghosts.is_empty() {
        *stall_notified = false;
        return;
    }

    // Idle is not "free": a posted hand keeps CrewState::Idle (posting never
    // touches the state), so the reactor's operator was rebuilding too. Only
    // the unposted count.
    let posted: std::collections::HashSet<Entity> =
        stations.iter().filter_map(|s| s.assigned_crew).collect();
    let idle_crew = crew_query.iter()
        .filter(|(e, c)| c.state == CrewState::Idle && c.health > 0.0 && !posted.contains(e))
        .count();
    if idle_crew == 0 {
        return;
    }

    let ghost = &mut queue.ghosts[0];

    // Materials up front: no scrap, no reconstruction.
    if !ghost.scrap_paid {
        if !inventory.remove_item(ItemType::ScrapMetal, ghost.scrap_cost) {
            if !*stall_notified {
                *stall_notified = true;
                notifications.write(ShowNotification {
                    message: format!(
                        "Rebuild waiting on ScrapMetal ({} needed for {})",
                        ghost.scrap_cost,
                        ghost_name(&ghost.kind)
                    ),
                    notification_type: NotificationType::Warning,
                    duration: 4.0,
                });
            }
            return;
        }
        ghost.scrap_paid = true;
        *stall_notified = false;
    }

    ghost.progress += idle_crew as f32 * time.delta_secs();
    if ghost.progress < ghost.work_needed {
        return;
    }

    // Done — respawn the block. `free: true`: it was paid for in scrap.
    let ghost = queue.ghosts.remove(0);
    match ghost.kind {
        GhostKind::Hull { layer, material } => {
            place_hull_events.write(PlaceHullRequest {
                layer,
                material,
                grid_position: ghost.grid_position,
                free: true,
            });
        }
        GhostKind::Module { module_type, rotation } => {
            place_module_events.write(PlaceModuleRequest {
                module_type,
                grid_position: ghost.grid_position,
                rotation,
                custom_name: None,
                subcomponents: None,
            extras: None,
                free: true,
            });
        }
    }
    for sprite in ghost.sprites {
        commands.entity(sprite).try_despawn();
    }
    notifications.write(ShowNotification {
        message: format!(
            "Crew rebuilt {} (-{} scrap)",
            ghost_name(&ghost.kind),
            ghost.scrap_cost
        ),
        notification_type: NotificationType::Success,
        duration: 2.5,
    });
}

/// While docked: drop any ghost whose origin cell got built over manually.
/// (Occupancy only rebuilds while docked, so this check is gated there.)
pub fn cleanup_built_over_ghosts(
    mut commands: Commands,
    occupancy: Res<GridOccupancy>,
    mut queue: ResMut<RebuildQueue>,
) {
    queue.ghosts.retain(|ghost| {
        if ghost.cells.iter().any(|c| occupancy.cells.contains_key(c)) {
            for &sprite in &ghost.sprites {
                commands.entity(sprite).try_despawn();
            }
            false
        } else {
            true
        }
    });
}

#[cfg(test)]
mod rebuild_crew_tests {
    use super::*;
    use crate::ai_ship::components::OwnedByAiShip;
    use std::time::Duration;

    fn hand() -> CrewMember {
        CrewMember {
            name: "Ash".into(),
            health: 100.0,
            max_health: 100.0,
            oxygen: 100.0,
            morale: 100.0,
            state: CrewState::Idle,
        }
    }

    /// Only our unposted hands work the ghost: not an enemy's crew, and not
    /// the operator standing at a post (who still reads as Idle).
    #[test]
    fn only_our_free_hands_rebuild() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<Inventory>()
            .add_message::<PlaceHullRequest>()
            .add_message::<PlaceModuleRequest>()
            .add_message::<ShowNotification>()
            .add_systems(Update, crew_rebuild_system);
        app.insert_resource(RebuildQueue {
            ghosts: vec![GhostBlock {
                grid_position: IVec2::ZERO,
                kind: GhostKind::Hull { layer: HullLayer::Outer, material: HullMaterial::Steel },
                cells: vec![IVec2::ZERO],
                progress: 0.0,
                work_needed: 100.0,
                scrap_cost: 0,
                scrap_paid: true,
                sprites: Vec::new(),
            }],
        });

        let raider = app.world_mut().spawn_empty().id();
        app.world_mut().spawn(hand());
        let operator = app.world_mut().spawn(hand()).id();
        app.world_mut().spawn(CrewStation { priority: 5, assigned_crew: Some(operator), manually_assigned: false });
        for _ in 0..3 {
            app.world_mut().spawn((hand(), OwnedByAiShip { root: raider }));
        }

        app.world_mut().resource_mut::<Time>().advance_by(Duration::from_secs(1));
        app.update();
        let progress = app.world().resource::<RebuildQueue>().ghosts[0].progress;
        assert!((progress - 1.0).abs() < 1e-4, "one free hand should give 1.0 work/s, got {progress}");
    }
}
