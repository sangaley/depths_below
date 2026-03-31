use bevy::prelude::*;

mod components;
mod resources;
mod events;
mod states;
mod submarine;
mod world;
mod creatures;
mod crew;
mod building;
mod ui;
mod meta;
mod contracts;
mod combat;
mod sonar;
mod camera;
mod sprite_map;
mod abyss_horror;
pub mod ai_submarine;
mod celestial;
mod vfx;

use states::GameState;
use events::EventsPlugin;
use resources::InputState;
use submarine::SubmarinePlugin;
use world::WorldPlugin;
use creatures::CreaturePlugin;
use crew::CrewPlugin;
use building::BuildingPlugin;
use ui::UiPlugin;
use meta::MetaPlugin;
use combat::CombatPlugin;
use sonar::SonarPlugin;
use camera::CameraPlugin;
use abyss_horror::AbyssHorrorPlugin;
use ai_submarine::AiSubmarinePlugin;
use contracts::ContractsPlugin;
use celestial::CelestialPlugin;
use vfx::VfxPlugin;

fn main() {
    App::new()
        // Default Bevy plugins (windowing, rendering, input, etc.)
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Depths Below — Into the Void".into(),
                resolution: (1280., 720.).into(),
                ..default()
            }),
            ..default()
        }))

        // Game state
        .add_state::<GameState>()

        // Global resources
        .init_resource::<InputState>()
        .insert_resource(ClearColor(Color::rgb(0.05, 0.15, 0.35)))

        // Events
        .add_plugins(EventsPlugin)

        // Our game plugins
        .add_plugins((
            SubmarinePlugin,
            WorldPlugin,
            CreaturePlugin,
            CrewPlugin,
            BuildingPlugin,
            UiPlugin,
            MetaPlugin,
            CombatPlugin,
            SonarPlugin,
            CameraPlugin,
            AbyssHorrorPlugin,
            AiSubmarinePlugin,
            ContractsPlugin,
            CelestialPlugin,
            VfxPlugin,
        ))

        .run();
}
