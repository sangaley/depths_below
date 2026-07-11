use bevy::prelude::*;

use crate::components::*;
use crate::events::*;
use super::components::*;

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
        if let Ok(children) = children_query.get(event.entity) {
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

        let type_name = match event.ship_type {
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
