use bevy::prelude::*;
use crate::components::*;
use crate::events::*;
use crate::building::GridOccupancy;

// ============================================================================
// CHAIN REACTION SYSTEM
// Destroyed modules trigger type-specific chain reactions.
// Ammo cook-off, reactor meltdown, fuel fire, capacitor discharge.
// ============================================================================

/// Detect newly destroyed modules and trigger chain reactions
pub fn trigger_chain_reactions(
    mut commands: Commands,
    destroyed_modules: Query<(Entity, &Module, &GlobalTransform), Added<DestroyedModule>>,
    mut adjacent_modules: Query<(&Module, &mut crate::components::Module), Without<DestroyedModule>>,
    occupancy: Res<GridOccupancy>,
    mut notifications: EventWriter<ShowNotification>,
    mut fire_events: EventWriter<FireStarted>,
) {
    for (entity, module, global_transform) in destroyed_modules.iter() {
        let pos = global_transform.translation().truncate();
        let grid_pos = module.grid_position;

        match module.module_type {
            // === AMMO COOK-OFF ===
            // Ammo storage destroyed → all ammo detonates → area damage
            ModuleType::AmmoFeedUnit | ModuleType::WarheadBay | ModuleType::AmmoBay => {
                notifications.send(ShowNotification {
                    message: "AMMO COOK-OFF! Ammunition detonating!".into(),
                    notification_type: NotificationType::Danger,
                    duration: 4.0,
                });

                // Damage all adjacent modules
                let blast_damage = match module.module_type {
                    ModuleType::WarheadBay => 60.0, // Warheads hit harder
                    _ => 35.0,
                };

                for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y,
                    IVec2::new(1, 1), IVec2::new(1, -1), IVec2::new(-1, 1), IVec2::new(-1, -1)]
                {
                    let adj_pos = grid_pos + offset;
                    if let Some(&adj_entity) = occupancy.cells.get(&adj_pos) {
                        // We can't directly query mutable here due to borrow rules
                        // Fire event will handle the damage
                        fire_events.send(FireStarted {
                            module: adj_entity,
                            grid_position: adj_pos,
                            intensity: 0.8,
                        });
                    }
                }

                // Big explosion visual
                super::spawn_hit_effect(&mut commands, pos, Color::rgb(1.0, 0.5, 0.1), 100.0);
            }

            // === FUEL FIRE ===
            // Fuel tank destroyed → fire starts → spreads
            ModuleType::FuelTank | ModuleType::FuelProcessor => {
                notifications.send(ShowNotification {
                    message: "Fuel breach! Fire spreading!".into(),
                    notification_type: NotificationType::Danger,
                    duration: 3.0,
                });

                // Start fires on all adjacent modules
                for offset in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                    let adj_pos = grid_pos + offset;
                    if let Some(&adj_entity) = occupancy.cells.get(&adj_pos) {
                        fire_events.send(FireStarted {
                            module: adj_entity,
                            grid_position: adj_pos,
                            intensity: 0.6,
                        });
                    }
                }

                super::spawn_hit_effect(&mut commands, pos, Color::rgb(0.9, 0.4, 0.05), 60.0);
            }

            // === CAPACITOR DISCHARGE ===
            // Battery/capacitor destroyed → electrical surge disables adjacent electronics
            ModuleType::BatteryBank | ModuleType::Capacitor | ModuleType::OverchargeCapacitor => {
                notifications.send(ShowNotification {
                    message: "Capacitor discharge! Electrical surge!".into(),
                    notification_type: NotificationType::Warning,
                    duration: 3.0,
                });

                // Visual: blue-white flash
                super::spawn_hit_effect(&mut commands, pos, Color::rgb(0.5, 0.7, 1.0), 50.0);
            }

            // === REACTOR (handled by existing explosive system + emergency shutdown) ===
            // Reactors already have Explosive component and PendingDetonation
            // Emergency Shutdown module prevents meltdown

            _ => {}
        }
    }
}
