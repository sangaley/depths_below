use bevy::prelude::*;
use crate::components::*;
use crate::events::*;
use crate::resources::*;
use crate::states::BuildState;

// ============================================================================
// UNDO/REDO SYSTEM
// Tracks all build actions in a history stack.
// Ctrl+Z undoes, Ctrl+Y or Ctrl+Shift+Z redoes.
// ============================================================================

/// A single build action that can be undone/redone
#[derive(Clone, Debug)]
pub enum BuildAction {
    PlaceModule {
        entity: Entity,
        module_type: ModuleType,
        grid_position: IVec2,
        rotation: Rotation,
        cost: u32,
    },
    PlaceHull {
        entity: Entity,
        layer: HullLayer,
        material: HullMaterial,
        grid_position: IVec2,
        cost: u32,
    },
    RemoveModule {
        module_type: ModuleType,
        grid_position: IVec2,
        rotation: Rotation,
        refund: u32,
    },
}

/// Resource tracking build history
#[derive(Resource)]
pub struct BuildHistory {
    pub undo_stack: Vec<BuildAction>,
    pub redo_stack: Vec<BuildAction>,
    pub max_history: usize,
}

impl Default for BuildHistory {
    fn default() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_history: 50,
        }
    }
}

impl BuildHistory {
    /// Record a new action (clears redo stack)
    pub fn record(&mut self, action: BuildAction) {
        self.undo_stack.push(action);
        self.redo_stack.clear();
        if self.undo_stack.len() > self.max_history {
            self.undo_stack.remove(0);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

/// System: listen for Ctrl+Z / Ctrl+Y and execute undo/redo
pub fn undo_redo_input(
    keyboard: Res<Input<KeyCode>>,
    mut history: ResMut<BuildHistory>,
    mut commands: Commands,
    mut currency: ResMut<Currency>,
    _module_query: Query<(Entity, &Module)>,
    _hull_query: Query<(Entity, &HullSegment)>,
    mut notifications: EventWriter<ShowNotification>,
    _current_state: Res<State<BuildState>>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !ctrl { return; }

    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);

    // Ctrl+Z = Undo
    if keyboard.just_pressed(KeyCode::Z) && !shift {
        if let Some(action) = history.undo_stack.pop() {
            match &action {
                BuildAction::PlaceModule { entity, cost, module_type, .. } => {
                    // Undo a placement = remove the module, refund cost
                    commands.entity(*entity).despawn_recursive();
                    currency.credits += cost;
                    notifications.send(ShowNotification {
                        message: format!("Undo: removed {} (+{}c)", module_type.name(), cost),
                        notification_type: NotificationType::Info,
                        duration: 2.0,
                    });
                }
                BuildAction::PlaceHull { entity, cost, .. } => {
                    commands.entity(*entity).despawn_recursive();
                    currency.credits += cost;
                    notifications.send(ShowNotification {
                        message: format!("Undo: removed hull (+{}c)", cost),
                        notification_type: NotificationType::Info,
                        duration: 2.0,
                    });
                }
                BuildAction::RemoveModule { refund, module_type, .. } => {
                    // Undo a removal = we can't respawn it perfectly, just refund
                    // Full undo of removal would require storing all component data
                    currency.credits -= refund; // Take back the refund
                    notifications.send(ShowNotification {
                        message: format!("Undo: {} removal reversed (-{}c)", module_type.name(), refund),
                        notification_type: NotificationType::Info,
                        duration: 2.0,
                    });
                }
            }
            history.redo_stack.push(action);
        }
    }

    // Ctrl+Y or Ctrl+Shift+Z = Redo
    if keyboard.just_pressed(KeyCode::Y) || (keyboard.just_pressed(KeyCode::Z) && shift) {
        if let Some(action) = history.redo_stack.pop() {
            match &action {
                BuildAction::PlaceModule { cost, module_type, .. } => {
                    // Redo a placement — deduct cost (module was already despawned by undo)
                    // Full redo would require respawning — for now just track credits
                    if currency.credits >= *cost {
                        currency.credits -= cost;
                        notifications.send(ShowNotification {
                            message: format!("Redo: {} (-{}c)", module_type.name(), cost),
                            notification_type: NotificationType::Info,
                            duration: 2.0,
                        });
                    }
                }
                _ => {}
            }
            history.undo_stack.push(action);
        }
    }
}
