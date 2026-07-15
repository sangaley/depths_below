use bevy::prelude::*;

mod components;
mod resources;
mod events;
mod states;
mod ship;
mod world;
mod creatures;
mod crew;
mod building;
mod ui;
mod meta;
mod contracts;
mod combat;
mod radar;
mod camera;
mod sprite_map;
mod abyss_horror;
pub mod ai_ship;
mod celestial;
mod vfx;
mod spatial;
mod demo;
mod audio;

use states::GameState;
use events::EventsPlugin;
use resources::InputState;
use ship::ShipPlugin;
use world::WorldPlugin;
use creatures::CreaturePlugin;
use crew::CrewPlugin;
use building::BuildingPlugin;
use ui::UiPlugin;
use meta::MetaPlugin;
use combat::CombatPlugin;
use radar::RadarPlugin;
use camera::CameraPlugin;
#[allow(unused_imports)]
use abyss_horror::AbyssHorrorPlugin;
use ai_ship::AiShipPlugin;
use contracts::ContractsPlugin;
use celestial::CelestialPlugin;
use vfx::VfxPlugin;
use spatial::SpatialPlugin;
use demo::DemoPlugin;
use audio::GameAudioPlugin;

fn main() {
    App::new()
        // Default Bevy plugins (windowing, rendering, input, etc.)
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Depths Below — Into the Void".into(),
                resolution: (1280, 720).into(),
                ..default()
            }),
            ..default()
        }))

        // Game state
        .init_state::<GameState>()

        // Global resources
        .init_resource::<InputState>()
        .insert_resource(ClearColor(Color::srgb(0.05, 0.15, 0.35)))

        // Events
        .add_plugins(EventsPlugin)

        // Our game plugins
        .add_plugins((
            ShipPlugin,
            WorldPlugin,
            CreaturePlugin,
            CrewPlugin,
            BuildingPlugin,
            UiPlugin,
            MetaPlugin,
            CombatPlugin,
            RadarPlugin,
            CameraPlugin,
            // AbyssHorrorPlugin disabled — it's built around real creatures
            // "watching"/fleeing you (see abyss_horror.rs); with creature
            // spawning off (see creatures::spawn_creatures) it would just be
            // false scares (phantom blips, glitches) with nothing behind
            // them. Re-add here when creatures come back.
            // AbyssHorrorPlugin,
            AiShipPlugin,
            ContractsPlugin,
            CelestialPlugin,
            VfxPlugin,
        ))
        .add_plugins(SpatialPlugin)
        .add_plugins(DemoPlugin)
        .add_plugins(GameAudioPlugin)

        .run();
}
