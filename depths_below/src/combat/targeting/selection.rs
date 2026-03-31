use bevy::prelude::*;
use crate::components::*;
use crate::events::*;
use crate::ai_submarine::components::AiSubmarine;

// ============================================================================
// TARGET SELECTION SYSTEM
// Tab cycles targets. Middle-click selects under cursor.
// Selected target gets bracket. All auto-fire weapons shoot at it.
// ============================================================================

/// Resource tracking the currently selected target
#[derive(Resource, Default)]
pub struct TargetSelection {
    pub target: Option<Entity>,
    pub target_type: TargetType,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TargetType {
    #[default]
    None,
    Creature,
    Ship,
}

/// Visual bracket around selected target
#[derive(Component)]
pub struct TargetBracket;

/// HUD element showing target info
#[derive(Component)]
pub struct TargetInfoText;

/// System: Tab cycles through valid targets (closest first)
pub fn cycle_target(
    keyboard: Res<Input<KeyCode>>,
    mut selection: ResMut<TargetSelection>,
    sub_query: Query<&Transform, With<Submarine>>,
    creature_query: Query<(Entity, &Transform, &Creature), Without<Submarine>>,
    ai_sub_query: Query<(Entity, &Transform), (With<AiSubmarine>, Without<Submarine>)>,
    mut notifications: EventWriter<ShowNotification>,
) {
    if !keyboard.just_pressed(KeyCode::Tab) { return; }

    let Ok(sub_transform) = sub_query.get_single() else { return };
    let sub_pos = sub_transform.translation.truncate();

    // Build sorted list of all valid targets by distance
    let mut targets: Vec<(Entity, f32, &str, TargetType)> = Vec::new();

    for (entity, transform, creature) in creature_query.iter() {
        if creature.health <= 0.0 { continue; }
        let dist = sub_pos.distance(transform.translation.truncate());
        if dist > 1500.0 { continue; } // Max targeting range
        let name = match creature.creature_type {
            CreatureType::VoidDrifter => "Void Drifter",
            CreatureType::Stalker => "Stalker",
            CreatureType::Leviathan => "LEVIATHAN",
            CreatureType::ParasiteSwarm => "Parasite",
        };
        targets.push((entity, dist, name, TargetType::Creature));
    }

    for (entity, transform) in ai_sub_query.iter() {
        let dist = sub_pos.distance(transform.translation.truncate());
        if dist > 2000.0 { continue; }
        targets.push((entity, dist, "Hostile Ship", TargetType::Ship));
    }

    // Sort by distance
    targets.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    if targets.is_empty() {
        selection.target = None;
        selection.target_type = TargetType::None;
        notifications.send(ShowNotification {
            message: "No targets in range".into(),
            notification_type: NotificationType::Info,
            duration: 1.5,
        });
        return;
    }

    // Find current target in list, cycle to next
    let current_idx = selection.target
        .and_then(|current| targets.iter().position(|(e, _, _, _)| *e == current));

    let next_idx = match current_idx {
        Some(i) => (i + 1) % targets.len(),
        None => 0,
    };

    let (entity, dist, name, target_type) = targets[next_idx];
    selection.target = Some(entity);
    selection.target_type = target_type;

    notifications.send(ShowNotification {
        message: format!("Target: {} ({:.0}m)", name, dist),
        notification_type: NotificationType::Warning,
        duration: 2.0,
    });
}

/// System: middle-click to select target under cursor
pub fn click_select_target(
    mouse: Res<Input<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    mut selection: ResMut<TargetSelection>,
    creature_query: Query<(Entity, &Transform, &Creature), Without<Submarine>>,
    ai_sub_query: Query<(Entity, &Transform), (With<AiSubmarine>, Without<Submarine>)>,
) {
    if !mouse.just_pressed(MouseButton::Middle) { return; }

    let Ok(window) = windows.get_single() else { return };
    let Ok((camera, camera_transform)) = camera_query.get_single() else { return };
    let Some(cursor) = window.cursor_position()
        .and_then(|p| camera.viewport_to_world_2d(camera_transform, p))
    else { return };

    let click_pos = cursor;
    let select_radius = 100.0;

    // Check creatures first
    let mut closest: Option<(Entity, f32, TargetType)> = None;
    for (entity, transform, creature) in creature_query.iter() {
        if creature.health <= 0.0 { continue; }
        let dist = click_pos.distance(transform.translation.truncate());
        if dist < select_radius {
            if closest.map_or(true, |(_, d, _)| dist < d) {
                closest = Some((entity, dist, TargetType::Creature));
            }
        }
    }

    // Check AI ships
    for (entity, transform) in ai_sub_query.iter() {
        let dist = click_pos.distance(transform.translation.truncate());
        if dist < select_radius {
            if closest.map_or(true, |(_, d, _)| dist < d) {
                closest = Some((entity, dist, TargetType::Ship));
            }
        }
    }

    if let Some((entity, _, target_type)) = closest {
        selection.target = Some(entity);
        selection.target_type = target_type;
    } else {
        // Click on nothing = deselect
        selection.target = None;
        selection.target_type = TargetType::None;
    }
}

/// System: draw bracket around selected target
pub fn draw_target_bracket(
    mut commands: Commands,
    selection: Res<TargetSelection>,
    existing_brackets: Query<Entity, With<TargetBracket>>,
    transform_query: Query<&Transform>,
    creature_query: Query<&Creature>,
) {
    // Despawn old brackets
    for entity in existing_brackets.iter() {
        commands.entity(entity).despawn();
    }

    let Some(target) = selection.target else { return };
    let Ok(transform) = transform_query.get(target) else {
        return;
    };
    let pos = transform.translation.truncate();

    // Get target size for bracket
    let size = if let Ok(creature) = creature_query.get(target) {
        match creature.creature_type {
            CreatureType::Leviathan => 180.0,
            CreatureType::Stalker => 60.0,
            _ => 30.0,
        }
    } else {
        80.0 // Ship default
    };

    let bracket_size = size + 20.0;
    let thickness = 2.0;
    let arm_length = bracket_size * 0.3;

    // Draw 4 corner brackets
    let corners = [
        (Vec2::new(-bracket_size / 2.0, bracket_size / 2.0), true, true),   // Top-left
        (Vec2::new(bracket_size / 2.0, bracket_size / 2.0), true, false),   // Top-right
        (Vec2::new(-bracket_size / 2.0, -bracket_size / 2.0), false, true), // Bottom-left
        (Vec2::new(bracket_size / 2.0, -bracket_size / 2.0), false, false), // Bottom-right
    ];

    let bracket_color = match selection.target_type {
        TargetType::Creature => Color::rgb(0.9, 0.3, 0.2),
        TargetType::Ship => Color::rgb(0.9, 0.6, 0.2),
        TargetType::None => Color::WHITE,
    };

    for (offset, is_top, is_left) in corners {
        // Horizontal arm
        let h_x = if is_left { offset.x + arm_length / 2.0 } else { offset.x - arm_length / 2.0 };
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: bracket_color,
                    custom_size: Some(Vec2::new(arm_length, thickness)),
                    ..default()
                },
                transform: Transform::from_xyz(pos.x + h_x, pos.y + offset.y, 1.0),
                ..default()
            },
            TargetBracket,
        ));

        // Vertical arm
        let v_y = if is_top { offset.y - arm_length / 2.0 } else { offset.y + arm_length / 2.0 };
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: bracket_color,
                    custom_size: Some(Vec2::new(thickness, arm_length)),
                    ..default()
                },
                transform: Transform::from_xyz(pos.x + offset.x, pos.y + v_y, 1.0),
                ..default()
            },
            TargetBracket,
        ));
    }
}
