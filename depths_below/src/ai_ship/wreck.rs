use bevy::prelude::*;
use rand::Rng;

use crate::components::*;
use crate::events::*;
use super::components::*;

/// Secondary explosions marching across a fresh wreck for a second or two
/// after the kill, ending in one final boom. Pure spectacle — pops don't
/// deal damage (the ship is already dead), they sell the death.
#[derive(Component)]
pub struct DeathRattle {
    pub timer: Timer,
    pub remaining: u32,
}

/// Ticks death rattles: random block pops with debris, then the final boom.
pub fn update_death_rattle(
    time: Res<Time>,
    mut commands: Commands,
    mut rattle_query: Query<(Entity, &mut DeathRattle, Option<&Velocity>)>,
    children_query: Query<&Children>,
    block_pos_query: Query<(&GlobalTransform, &Sprite), Or<(With<Module>, With<HullSegment>)>>,
    mut boom_events: MessageWriter<AiModuleExploded>,
) {
    let mut rng = rand::thread_rng();
    for (ship_entity, mut rattle, velocity) in rattle_query.iter_mut() {
        rattle.timer.tick(time.delta());
        if !rattle.timer.is_finished() { continue; }

        // Pick a random surviving block to pop at
        let pop_pos = children_query.get(ship_entity).ok().and_then(|children| {
            let blocks: Vec<(Vec2, Color)> = children.iter()
                .filter_map(|c| block_pos_query.get(c).ok())
                .map(|(gt, sprite)| (gt.translation().truncate(), sprite.color))
                .collect();
            if blocks.is_empty() { None } else {
                Some(blocks[rng.gen_range(0..blocks.len())])
            }
        });
        let Some((pos, color)) = pop_pos else {
            commands.entity(ship_entity).try_remove::<DeathRattle>();
            continue;
        };

        let inherited = velocity.map(|v| v.0 * 0.6).unwrap_or(Vec2::ZERO);

        if rattle.remaining > 1 {
            // Secondary pop: flash + chunk spray + attenuated crunch
            crate::combat::spawn_hit_effect(&mut commands, pos, Color::srgb(1.0, 0.6, 0.15), rng.gen_range(30.0..60.0));
            crate::vfx::debris::spawn_chunks(&mut commands, &mut rng, pos, color, inherited);
            boom_events.write(AiModuleExploded { position: pos, blast_damage: 20.0 });
            rattle.remaining -= 1;
            rattle.timer = Timer::from_seconds(rng.gen_range(0.15..0.4), TimerMode::Once);
        } else {
            // Final boom — big flash, big spray, deep audio layer
            crate::combat::spawn_hit_effect(&mut commands, pos, Color::srgb(1.0, 0.5, 0.1), 140.0);
            for _ in 0..3 {
                crate::vfx::debris::spawn_chunks(&mut commands, &mut rng, pos, color, inherited);
            }
            boom_events.write(AiModuleExploded { position: pos, blast_damage: 100.0 });
            commands.entity(ship_entity).try_remove::<DeathRattle>();
        }
    }
}

/// When an AI ship is destroyed, turn it into a wreck IN PLACE — keep the
/// hull/module hierarchy intact (attach Wreck/PointOfInterest/AiShipWreck to
/// the existing root) instead of despawning it and spawning an unrelated
/// generic gray sprite. That old approach erased the actual battle damage
/// the player had just fought for and replaced it with an abstract blob;
/// this way the derelict really is the ship you shot up, sitting there for
/// F-key salvage (world::salvage_wreck_system already works off any entity
/// with Wreck + PointOfInterest, no changes needed there). Remaining
/// modules are deactivated and darkened — the reactor's dead, nothing on
/// this hulk still has power — but nothing is despawned.
pub fn ai_ship_death_system(
    mut commands: Commands,
    mut destroyed_events: MessageReader<AiShipDestroyed>,
    children_query: Query<&Children>,
    mut module_query: Query<(&mut Module, &mut Sprite, Has<DestroyedModule>), Without<HullSegment>>,
    mut hull_query: Query<(&mut Sprite, &HullSegment, Has<HullDestroyed>), Without<Module>>,
    registry: Res<crate::building::ModuleRegistry>,
    mut currency: ResMut<crate::resources::Currency>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for event in destroyed_events.read() {
        let loot = match event.ship_type {
            AiShipType::VoidTitan => 30,    // legendary hoard
            AiShipType::Dreadnought => 20,  // colossal wreck
            AiShipType::Leviathan => 6,
            AiShipType::AbyssalCult => 4,
            AiShipType::Drowned => 8,     // rare old loot
            AiShipType::PressureKing => 5,
            AiShipType::GlassEye => 7,    // intel data
            AiShipType::IronTide => 10,    // massive wreck
            AiShipType::Blackwater => 6,
            AiShipType::RustSwarm => 2,    // junk
        };

        commands.entity(event.entity).try_insert((
            AiShipWreck {
                ship_type: event.ship_type,
                loot_remaining: loot,
            },
            PointOfInterest {
                poi_type: PoiType::Wreck,
                discovered: true,
            },
            Wreck {
                loot_remaining: loot,
                is_explored: false,
            },
        ));

        // Power off and darken every block — dead reactor, no power,
        // nothing left running. Blocks that survived the fight (not
        // individually destroyed) pay out half their build cost in
        // credits — salvage value for the parts that are still intact.
        const WRECK_TINT: Color = Color::srgb(0.18, 0.18, 0.2);
        let mut salvage_value = 0.0_f32;
        let mut block_count = 0u32;
        if let Ok(children) = children_query.get(event.entity) {
            block_count = children.iter().count() as u32;
            for child in children.iter() {
                if let Ok((mut module, mut sprite, is_destroyed)) = module_query.get_mut(child) {
                    if !is_destroyed {
                        salvage_value += registry.get(module.module_type).cost as f32 * 0.5;
                    }
                    module.is_active = false;
                    sprite.color = WRECK_TINT;
                } else if let Ok((mut sprite, hull, is_destroyed)) = hull_query.get_mut(child) {
                    if !is_destroyed {
                        salvage_value += hull.material.cost() as f32 * 0.5;
                    }
                    sprite.color = WRECK_TINT;
                }
            }
        }

        // Bigger ships rattle longer before the final boom.
        commands.entity(event.entity).try_insert(DeathRattle {
            timer: Timer::from_seconds(0.2, TimerMode::Once),
            remaining: (block_count / 5).clamp(3, 10),
        });

        let type_name = match event.ship_type {
            AiShipType::VoidTitan => "Void Titan",
            AiShipType::Dreadnought => "Dreadnought",
            AiShipType::Leviathan => "Leviathan Rider",
            AiShipType::AbyssalCult => "Abyssal Cult",
            AiShipType::Drowned => "Drowned",
            AiShipType::PressureKing => "Pressure King",
            AiShipType::GlassEye => "Glass Eye",
            AiShipType::IronTide => "Iron Tide",
            AiShipType::Blackwater => "Blackwater",
            AiShipType::RustSwarm => "Rust Swarm",
        };

        let mut message = format!("{} vessel destroyed! Wreck can be salvaged (F).", type_name);
        if salvage_value > 0.0 {
            let payout = salvage_value.round() as u32;
            currency.credits += payout;
            message = format!("{} destroyed! +{}c for intact salvage. Wreck can be looted (F).", type_name, payout);
        }

        notifications.write(ShowNotification {
            message,
            notification_type: NotificationType::Success,
            duration: 4.0,
        });
    }
}
