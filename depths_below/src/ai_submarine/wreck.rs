use bevy::prelude::*;

use crate::components::*;
use crate::events::*;
use super::components::*;

/// When an AI submarine is destroyed, despawn it and spawn a wreck entity
pub fn ai_sub_death_system(
    mut commands: Commands,
    mut destroyed_events: EventReader<AiSubDestroyed>,
    asset_server: Res<AssetServer>,
    mut notifications: EventWriter<ShowNotification>,
) {
    for event in destroyed_events.iter() {
        // Despawn the AI submarine and all children
        commands.entity(event.entity).despawn_recursive();

        let loot = match event.sub_type {
            AiSubType::Leviathan => 6,
            AiSubType::AbyssalCult => 4,
            AiSubType::Drowned => 8,     // rare old loot
            AiSubType::PressureKing => 5,
            AiSubType::GlassEye => 7,    // intel data
            AiSubType::IronTide => 10,    // massive wreck
            AiSubType::Blackwater => 6,
            AiSubType::RustSwarm => 2,    // junk
        };

        let wreck_color = Color::rgb(0.3, 0.3, 0.35); // Grayscale

        // Spawn wreck entity
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: wreck_color,
                    custom_size: Some(Vec2::new(200.0, 80.0)),
                    ..default()
                },
                transform: Transform {
                    translation: Vec3::new(event.position.x, event.position.y, -0.05),
                    rotation: Quat::from_rotation_z(0.15), // Slight tilt
                    ..default()
                },
                texture: asset_server.load(crate::sprite_map::effect_sprite_path("submarine_body")),
                ..default()
            },
            AiSubWreck {
                sub_type: event.sub_type,
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

        let type_name = match event.sub_type {
            AiSubType::Leviathan => "Leviathan Rider",
            AiSubType::AbyssalCult => "Abyssal Cult",
            AiSubType::Drowned => "Drowned",
            AiSubType::PressureKing => "Pressure King",
            AiSubType::GlassEye => "Glass Eye",
            AiSubType::IronTide => "Iron Tide",
            AiSubType::Blackwater => "Blackwater",
            AiSubType::RustSwarm => "Rust Swarm",
        };

        notifications.send(ShowNotification {
            message: format!("{} vessel destroyed! Wreck detected on radar.", type_name),
            notification_type: NotificationType::Success,
            duration: 4.0,
        });
    }
}
