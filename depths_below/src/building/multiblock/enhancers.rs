use bevy::prelude::*;
use crate::components::*;
use crate::resources::*;
use crate::events::*;
use crate::celestial::components::GravityAffected;
use super::components::*;

// ============================================================================
// ENHANCER SYSTEMS
// Each advanced module scans adjacent tiles and applies its effect.
// Effects are recalculated each frame (idempotent via the stat reset systems).
// ============================================================================

/// Applies all adjacency-based enhancer effects for weapons, hull, and structural modules.
pub fn apply_weapon_enhancers(
    module_query: Query<&Module, Without<DestroyedModule>>,
    mut weapon_query: Query<(&Module, &mut Weapon), Without<DestroyedModule>>,
    mut cascade_query: Query<(&Module, &mut CascadeRisk)>,
) {
    // Collect enhancer positions and types
    let enhancers: Vec<(IVec2, ModuleType)> = module_query.iter()
        .filter(|m| m.is_active)
        .map(|m| (m.grid_position, m.module_type))
        .collect();

    for (pos, module_type) in &enhancers {
        match module_type {
            ModuleType::MuzzleBrake => {
                for (wm, mut weapon) in weapon_query.iter_mut() {
                    if is_adjacent(pos, &wm.grid_position) {
                        weapon.damage *= 1.05;
                    }
                }
                for (cm, mut cascade) in cascade_query.iter_mut() {
                    if is_adjacent(pos, &cm.grid_position) {
                        cascade.cascade_chance *= 0.85;
                    }
                }
            }
            ModuleType::RecoilAbsorber => {
                for (cm, mut cascade) in cascade_query.iter_mut() {
                    if is_adjacent(pos, &cm.grid_position) {
                        cascade.cascade_chance *= 0.70;
                    }
                }
            }
            ModuleType::BoreEvacuator => {
                for (wm, mut weapon) in weapon_query.iter_mut() {
                    if is_adjacent(pos, &wm.grid_position)
                        && matches!(wm.module_type, ModuleType::Cannon | ModuleType::Railgun | ModuleType::Coilgun | ModuleType::Gatling)
                    {
                        weapon.fire_rate *= 1.20;
                    }
                }
            }
            ModuleType::MagneticAccelerator => {
                for (wm, mut weapon) in weapon_query.iter_mut() {
                    if is_adjacent(pos, &wm.grid_position)
                        && matches!(wm.module_type, ModuleType::Railgun | ModuleType::Coilgun)
                    {
                        weapon.range *= 1.40;
                    }
                }
            }
            ModuleType::FocusingArray => {
                for (wm, mut weapon) in weapon_query.iter_mut() {
                    if is_adjacent(pos, &wm.grid_position)
                        && matches!(wm.module_type, ModuleType::Laser | ModuleType::PlasmaCaster | ModuleType::IonDisruptor)
                    {
                        weapon.range *= 1.30;
                    }
                }
            }
            ModuleType::WarheadBay => {
                for (wm, mut weapon) in weapon_query.iter_mut() {
                    if is_adjacent(pos, &wm.grid_position)
                        && matches!(wm.module_type, ModuleType::HeavyMissile | ModuleType::GuidedMissile | ModuleType::ClusterRocket)
                    {
                        weapon.max_ammo += 8;
                    }
                }
            }
            ModuleType::VibrationDamper => {
                for (wm, mut weapon) in weapon_query.iter_mut() {
                    if is_adjacent(pos, &wm.grid_position) {
                        weapon.damage *= 1.10;
                    }
                }
            }
            ModuleType::ReinforcedJoint => {
                for (cm, mut cascade) in cascade_query.iter_mut() {
                    if is_adjacent(pos, &cm.grid_position) {
                        cascade.cascade_chance *= 0.60;
                    }
                }
            }
            _ => {}
        }
    }
}

/// Applies hull and structural enhancers (radiation hardening, hull reinforce, structural brace)
pub fn apply_hull_enhancers(
    module_query: Query<&Module, Without<DestroyedModule>>,
    mut hull_query: Query<&mut HullSegment>,
) {
    let enhancers: Vec<(IVec2, ModuleType)> = module_query.iter()
        .filter(|m| m.is_active)
        .map(|m| (m.grid_position, m.module_type))
        .collect();

    for (pos, module_type) in &enhancers {
        match module_type {
            ModuleType::RadiationHardening => {
                for mut hull in hull_query.iter_mut() {
                    if is_adjacent(pos, &hull.grid_position) {
                        hull.radiation_shielding *= 1.50;
                    }
                }
            }
            ModuleType::HullReinforcePlate | ModuleType::StructuralBrace => {
                let bonus_mult = if *module_type == ModuleType::HullReinforcePlate { 1.30 } else { 1.25 };
                for mut hull in hull_query.iter_mut() {
                    if is_adjacent(pos, &hull.grid_position) {
                        hull.max_health *= bonus_mult;
                    }
                }
            }
            _ => {}
        }
    }
}

/// Passive utility effects: signal jammer, fuel injector, inertial dampener
pub fn apply_utility_enhancers(
    module_query: Query<&Module, Without<DestroyedModule>>,
    mut noise_state: ResMut<NoiseState>,
    mut fuel_state: ResMut<FuelState>,
    mut gravity_query: Query<&mut GravityAffected, With<Submarine>>,
) {
    let mut has_signal_jammer = false;
    let mut fuel_injector_count = 0u32;
    let mut dampener_count = 0u32;

    for module in module_query.iter() {
        if !module.is_active { continue; }
        match module.module_type {
            ModuleType::SignalJammer => has_signal_jammer = true,
            ModuleType::FuelInjector => fuel_injector_count += 1,
            ModuleType::InertialDampener => dampener_count += 1,
            ModuleType::GravityCompensator => dampener_count += 2, // Stronger version
            _ => {}
        }
    }

    if has_signal_jammer {
        noise_state.noise_level *= 0.60;
    }

    if fuel_injector_count > 0 {
        // Diminishing returns: first = 20%, second = 10%, etc.
        let reduction = 1.0 - (fuel_injector_count as f32 * 0.15).min(0.40);
        fuel_state.fuel_consumption_rate *= reduction;
    }

    if dampener_count > 0 {
        if let Ok(mut gravity) = gravity_query.get_single_mut() {
            // Higher mass = less acceleration from same force
            let resistance = 1.0 + dampener_count as f32 * 0.30;
            gravity.mass *= resistance;
        }
    }
}

/// Emergency O2 cache: auto-deploys when oxygen hits critical
pub fn emergency_o2_system(
    mut commands: Commands,
    oxygen_state: Res<OxygenState>,
    module_query: Query<(Entity, &Module), Without<DestroyedModule>>,
    mut notifications: EventWriter<ShowNotification>,
    mut deployed: Local<bool>,
) {
    if oxygen_state.current_oxygen > 5.0 {
        *deployed = false;
        return;
    }
    if *deployed { return; }

    for (entity, module) in module_query.iter() {
        if module.module_type != ModuleType::EmergencyO2Cache || !module.is_active { continue; }

        *deployed = true;
        commands.entity(entity).insert(DestroyedModule {
            original_type: module.module_type,
        });
        notifications.send(ShowNotification {
            message: "EMERGENCY O2 CACHE DEPLOYED! 60 seconds of air!".into(),
            notification_type: NotificationType::Danger,
            duration: 4.0,
        });
        return;
    }
}

/// Emergency shutdown: auto-SCRAM reactor before meltdown
pub fn emergency_shutdown_system(
    mut commands: Commands,
    reactor_query: Query<(Entity, &Module, &Reactor), Without<DestroyedModule>>,
    shutdown_query: Query<(Entity, &Module), Without<DestroyedModule>>,
    mut notifications: EventWriter<ShowNotification>,
) {
    for (_reactor_entity, reactor_module, reactor) in reactor_query.iter() {
        if reactor.heat < reactor.max_heat * 0.90 { continue; }

        for (shutdown_entity, shutdown_module) in shutdown_query.iter() {
            if shutdown_module.module_type != ModuleType::EmergencyShutdown || !shutdown_module.is_active { continue; }
            if !is_adjacent(&reactor_module.grid_position, &shutdown_module.grid_position) { continue; }

            // Consume the shutdown module (one-time use)
            commands.entity(shutdown_entity).insert(DestroyedModule {
                original_type: shutdown_module.module_type,
            });

            notifications.send(ShowNotification {
                message: "EMERGENCY SCRAM! Reactor cooled before meltdown!".into(),
                notification_type: NotificationType::Danger,
                duration: 5.0,
            });
            return;
        }
    }
}

/// Afterburner: Shift+W for temporary thrust boost
pub fn afterburner_system(
    time: Res<Time>,
    keyboard: Res<Input<KeyCode>>,
    module_query: Query<&Module, Without<DestroyedModule>>,
    mut engine_query: Query<&mut Engine>,
    mut notifications: EventWriter<ShowNotification>,
    mut active_timer: Local<f32>,
    mut cooldown: Local<f32>,
) {
    let dt = time.delta_seconds();

    if *active_timer > 0.0 { *active_timer -= dt; }
    if *cooldown > 0.0 { *cooldown -= dt; }

    let has_afterburner = module_query.iter()
        .any(|m| m.module_type == ModuleType::Afterburner && m.is_active);

    if !has_afterburner { return; }

    if keyboard.pressed(KeyCode::ShiftLeft) && keyboard.pressed(KeyCode::W)
        && *cooldown <= 0.0 && *active_timer <= 0.0
    {
        *active_timer = 5.0;
        *cooldown = 30.0;
        notifications.send(ShowNotification {
            message: "AFTERBURNER ENGAGED!".into(),
            notification_type: NotificationType::Warning,
            duration: 2.0,
        });
    }

    if *active_timer > 0.0 {
        for mut engine in engine_query.iter_mut() {
            engine.thrust *= 3.0;
        }
    }
}

fn is_adjacent(a: &IVec2, b: &IVec2) -> bool {
    let diff = *a - *b;
    (diff.x.abs() + diff.y.abs()) == 1
}
