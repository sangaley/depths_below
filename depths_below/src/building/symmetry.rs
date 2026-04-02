use bevy::prelude::*;
use crate::components::*;
use crate::events::*;
use crate::states::BuildState;

// ============================================================================
// SYMMETRY MODE
// Toggle with Y key during build mode.
// Places mirror copy at (-x, y) with mirrored rotation.
// Visual center line shown when active.
// ============================================================================

/// Resource tracking symmetry state
#[derive(Resource, Default)]
pub struct SymmetryState {
    pub enabled: bool,
}

/// Marker for the symmetry center line visual
#[derive(Component)]
pub struct SymmetryCenterLine;

/// Toggle symmetry with Y key
pub fn toggle_symmetry(
    keyboard: Res<Input<KeyCode>>,
    mut symmetry: ResMut<SymmetryState>,
    mut commands: Commands,
    existing_line: Query<Entity, With<SymmetryCenterLine>>,
    mut notifications: EventWriter<ShowNotification>,
    current_state: Res<State<BuildState>>,
) {
    if *current_state.get() == BuildState::Inactive { return; }

    if keyboard.just_pressed(KeyCode::Y) {
        symmetry.enabled = !symmetry.enabled;

        if symmetry.enabled {
            // Spawn center line
            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color: Color::rgba(0.3, 0.6, 1.0, 0.2),
                        custom_size: Some(Vec2::new(2.0, 5000.0)), // Tall vertical line
                        ..default()
                    },
                    transform: Transform::from_xyz(0.0, 0.0, 0.3),
                    ..default()
                },
                SymmetryCenterLine,
            ));
            notifications.send(ShowNotification {
                message: "Symmetry mode ON — modules mirror across center".into(),
                notification_type: NotificationType::Info,
                duration: 2.0,
            });
        } else {
            // Despawn center line
            for entity in existing_line.iter() {
                commands.entity(entity).despawn();
            }
            notifications.send(ShowNotification {
                message: "Symmetry mode OFF".into(),
                notification_type: NotificationType::Info,
                duration: 2.0,
            });
        }
    }
}

/// Get the mirrored position (reflect across X=0)
pub fn mirror_position(pos: IVec2) -> IVec2 {
    IVec2::new(-pos.x, pos.y)
}

/// Get the mirrored rotation
pub fn mirror_rotation(rotation: Rotation) -> Rotation {
    match rotation {
        Rotation::North => Rotation::North,   // Symmetric
        Rotation::South => Rotation::South,   // Symmetric
        Rotation::East => Rotation::West,     // Mirror
        Rotation::West => Rotation::East,     // Mirror
    }
}
