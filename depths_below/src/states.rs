use bevy::prelude::*;

/// Main game states
#[derive(States, Debug, Clone, Copy, Eq, PartialEq, Hash, Default)]
pub enum GameState {
    #[default]
    MainMenu,
    Loading,
    StationDocked,  // At station, full building mode
    Exploring,      // In space, playing
    Docked,         // At outpost or wreck
    Paused,
    GameOver,
}

/// Sub-states for more granular control
#[derive(States, Debug, Clone, Copy, Eq, PartialEq, Hash, Default)]
pub enum BuildState {
    #[default]
    Inactive,
    Placing,            // Placing a new module
    Moving,             // Moving existing module
    Connecting,         // Connecting power/systems
    Deleting,           // Removing modules
    PlacingComponent,   // Placing components within a module's internal grid
    CustomizingPiece,   // Customizing a specific component piece
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SubmarineSet {
    Input,
    Movement,
    Physics,
    Power,
    Heat,
    Oxygen,
    Hull,
    State,
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum CombatSet {
    WeaponFire,
    Cleanup,
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SonarSet {
    Input,
    Update,
    Visibility,
}
