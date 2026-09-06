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
pub mod ai_ship;
mod celestial;
mod vfx;
mod spatial;
mod autoplay;
mod demo;
mod audio;
mod debug;
mod gamepad;
mod tutorial;

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
use ai_ship::AiShipPlugin;
use contracts::ContractsPlugin;
use celestial::CelestialPlugin;
use vfx::VfxPlugin;
use spatial::SpatialPlugin;
use autoplay::AutoplayPlugin;
use demo::DemoPlugin;
use audio::GameAudioPlugin;
use debug::DebugPlugin;
use gamepad::GamepadPlugin;
use tutorial::TutorialPlugin;

fn main() {
    App::new()
        // Default Bevy plugins (windowing, rendering, input, etc.)
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Depths Below - Into the Void".into(),
                resolution: (1280, 720).into(),
                ..default()
            }),
            ..default()
        }))

        // Game state
        .init_state::<GameState>()

        // Global resources
        .init_resource::<InputState>()
        .init_resource::<crate::resources::InteractPress>()
        // Ahead of every F handler, so they all see the same single press.
        .add_systems(First, crate::resources::refresh_interact_press)
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
            // AbyssHorrorPlugin parked in src/parked/ (see parked/README.md) —
            // built around creatures watching/fleeing you, pointless while
            // creature spawning is off. Move it back and re-add here to revive.
            AiShipPlugin,
            ContractsPlugin,
            CelestialPlugin,
            VfxPlugin,
        ))
        .add_plugins(SpatialPlugin)
        .add_plugins(DemoPlugin)
        .add_plugins(AutoplayPlugin)
        .add_plugins(DebugPlugin)
        .add_plugins(GameAudioPlugin)
        .add_plugins(GamepadPlugin)
        .add_plugins(TutorialPlugin)

        .run();
}
