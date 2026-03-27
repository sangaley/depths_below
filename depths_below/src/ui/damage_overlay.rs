use bevy::prelude::*;
use crate::components::*;
use crate::events::*;

/// Marker for the overlay legend UI node
#[derive(Component)]
pub(crate) struct DamageOverlayLegend;

/// Toggles the damage overlay on/off when O is pressed during Exploring
pub fn toggle_damage_overlay(
    keyboard: Res<Input<KeyCode>>,
    mut commands: Commands,
    sub_query: Query<(Entity, Option<&DamageOverlayVisible>), With<Submarine>>,
    mut notifications: EventWriter<ShowNotification>,
) {
    if !keyboard.just_pressed(KeyCode::O) {
        return;
    }

    let Ok((entity, overlay)) = sub_query.get_single() else { return };

    if overlay.is_some() {
        commands.entity(entity).remove::<DamageOverlayVisible>();
        notifications.send(ShowNotification {
            message: "Damage overlay OFF".into(),
            notification_type: NotificationType::Info,
            duration: 1.0,
        });
    } else {
        commands.entity(entity).insert(DamageOverlayVisible);
        notifications.send(ShowNotification {
            message: "Damage overlay ON — green=OK, yellow=damaged, red=critical, gray=destroyed".into(),
            notification_type: NotificationType::Info,
            duration: 2.5,
        });
    }
}

/// Spawns the legend UI when overlay becomes visible
pub fn spawn_overlay_legend(
    mut commands: Commands,
    sub_query: Query<&DamageOverlayVisible, Added<DamageOverlayVisible>>,
    existing_legend: Query<Entity, With<DamageOverlayLegend>>,
) {
    if sub_query.is_empty() {
        return;
    }
    // Don't double-spawn
    if !existing_legend.is_empty() {
        return;
    }

    // Use representative colors from the actual gradient at midpoints
    let entries = [
        ("100-60%", damage_color_smooth(0.80)),
        (" 60-30%", damage_color_smooth(0.45)),
        (" 30-1% ", damage_color_smooth(0.15)),
        ("   0%  ", damage_color_smooth(0.00)),
    ];

    commands.spawn((
        NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                right: Val::Px(10.0),
                top: Val::Px(60.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(6.0)),
                row_gap: Val::Px(3.0),
                ..default()
            },
            background_color: Color::rgba(0.0, 0.0, 0.0, 0.7).into(),
            ..default()
        },
        DamageOverlayLegend,
    )).with_children(|parent| {
        // Title
        parent.spawn(TextBundle::from_section(
            "DMG OVERLAY",
            TextStyle {
                font_size: 12.0,
                color: Color::WHITE,
                ..default()
            },
        ));
        for (label, color) in entries {
            // Make swatch fully opaque so it's visible in the legend
            let swatch_color = match color {
                Color::Rgba { red, green, blue, .. } => Color::rgba(red, green, blue, 0.9),
                other => other,
            };
            parent.spawn(NodeBundle {
                style: Style {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(4.0),
                    ..default()
                },
                ..default()
            }).with_children(|row| {
                // Color swatch
                row.spawn(NodeBundle {
                    style: Style {
                        width: Val::Px(10.0),
                        height: Val::Px(10.0),
                        ..default()
                    },
                    background_color: swatch_color.into(),
                    ..default()
                });
                // Label
                row.spawn(TextBundle::from_section(
                    label,
                    TextStyle {
                        font_size: 11.0,
                        color: Color::rgba(0.8, 0.8, 0.8, 1.0),
                        ..default()
                    },
                ));
            });
        }
    });
}

/// Despawns the legend UI when overlay is removed or submarine is gone.
/// Also handles cleanup on state transitions (game over, etc).
pub fn despawn_overlay_legend(
    mut commands: Commands,
    sub_query: Query<(Entity, Option<&DamageOverlayVisible>), With<Submarine>>,
    legend_query: Query<Entity, With<DamageOverlayLegend>>,
) {
    if legend_query.is_empty() {
        return; // Nothing to clean up
    }

    // If no submarine exists or submarine has no overlay, remove legend
    let should_remove = match sub_query.get_single() {
        Ok((_, Some(_))) => false, // Submarine exists with overlay — keep legend
        _ => true,                 // No sub or no overlay — remove legend
    };

    if should_remove {
        for entity in legend_query.iter() {
            commands.entity(entity).despawn_recursive();
        }
    }
}

/// Cleanup legend on game over / main menu transitions
pub fn cleanup_overlay_on_exit(
    mut commands: Commands,
    legend_query: Query<Entity, With<DamageOverlayLegend>>,
    overlay_query: Query<Entity, With<DamageOverlaySprite>>,
) {
    for entity in legend_query.iter() {
        commands.entity(entity).despawn_recursive();
    }
    for entity in overlay_query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

/// Updates damage overlay sprites on modules and hull segments when visible
pub fn update_damage_overlay(
    sub_query: Query<&Children, (With<Submarine>, With<DamageOverlayVisible>)>,
    module_query: Query<(Entity, &Module), Without<DamageOverlaySprite>>,
    hull_query: Query<(Entity, &HullSegment), (Without<Module>, Without<DamageOverlaySprite>)>,
    mut overlay_sprites: Query<(&Parent, &mut Sprite), With<DamageOverlaySprite>>,
    existing_parents: Query<&Parent, With<DamageOverlaySprite>>,
    mut commands: Commands,
) {
    let Ok(children) = sub_query.get_single() else { return };

    // Collect which parent entities already have overlays
    let mut parents_with_overlay: bevy::utils::HashSet<Entity> = bevy::utils::HashSet::new();
    for parent in existing_parents.iter() {
        parents_with_overlay.insert(parent.get());
    }

    // Update existing overlay colors
    for (parent, mut sprite) in overlay_sprites.iter_mut() {
        if let Ok((_, module)) = module_query.get(parent.get()) {
            let ratio = health_ratio(module.health, module.max_health);
            sprite.color = damage_color_smooth(ratio);
        } else if let Ok((_, hull)) = hull_query.get(parent.get()) {
            let ratio = health_ratio(hull.health, hull.max_health);
            sprite.color = damage_color_smooth(ratio);
        }
    }

    // Spawn overlays for entities that don't have one yet
    for &child in children.iter() {
        if parents_with_overlay.contains(&child) {
            continue;
        }

        if let Ok((entity, module)) = module_query.get(child) {
            let ratio = health_ratio(module.health, module.max_health);
            let visual_size = rotated_size(module.size, module.rotation);
            spawn_overlay(&mut commands, entity, damage_color_smooth(ratio), visual_size);
        } else if let Ok((entity, hull)) = hull_query.get(child) {
            let ratio = health_ratio(hull.health, hull.max_health);
            spawn_overlay(&mut commands, entity, damage_color_smooth(ratio), IVec2::ONE);
        }
    }
}

/// Removes all overlay sprites when the overlay is toggled off
pub fn cleanup_damage_overlay(
    sub_query: Query<&Submarine, Without<DamageOverlayVisible>>,
    overlay_query: Query<Entity, With<DamageOverlaySprite>>,
    mut commands: Commands,
    mut cleaned: Local<bool>,
) {
    if sub_query.is_empty() {
        // No submarine without overlay — either no sub or overlay is active
        *cleaned = false;
        return;
    }

    // Submarine exists without DamageOverlayVisible — clean up once
    if *cleaned {
        return;
    }

    for entity in overlay_query.iter() {
        commands.entity(entity).despawn_recursive();
    }
    *cleaned = true;
}

// ============================================================================
// HELPERS
// ============================================================================

fn health_ratio(health: f32, max_health: f32) -> f32 {
    if max_health > 0.0 { (health / max_health).clamp(0.0, 1.0) } else { 1.0 }
}

/// Returns the visual size of a module accounting for rotation.
/// East/West rotations swap width and height.
fn rotated_size(size: IVec2, rotation: Rotation) -> IVec2 {
    match rotation {
        Rotation::North | Rotation::South => size,
        Rotation::East | Rotation::West => IVec2::new(size.y, size.x),
    }
}

fn spawn_overlay(commands: &mut Commands, parent: Entity, color: Color, size: IVec2) {
    let w = 60.0 + (size.x - 1).max(0) as f32 * 66.0;
    let h = 60.0 + (size.y - 1).max(0) as f32 * 66.0;

    let overlay = commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color,
                custom_size: Some(Vec2::new(w, h)),
                ..default()
            },
            transform: Transform::from_xyz(0.0, 0.0, 0.5),
            ..default()
        },
        DamageOverlaySprite,
    )).id();

    commands.entity(overlay).set_parent(parent);
}

/// Smooth color interpolation based on health ratio.
/// Lerps between green→yellow→red→gray for a continuous gradient.
fn damage_color_smooth(ratio: f32) -> Color {
    let alpha = 0.3;

    if ratio <= 0.0 {
        // Destroyed: dark gray
        return Color::rgba(0.3, 0.3, 0.3, 0.5);
    }

    if ratio <= 0.30 {
        // 0-30%: red to yellow
        let t = ratio / 0.30;
        return Color::rgba(
            0.9,
            lerp(0.1, 0.6, t),
            0.1,
            alpha + 0.15 * (1.0 - t), // more opaque at lower health
        );
    }

    if ratio <= 0.60 {
        // 30-60%: yellow to light green
        let t = (ratio - 0.30) / 0.30;
        return Color::rgba(
            lerp(0.9, 0.4, t),
            lerp(0.6, 0.8, t),
            lerp(0.1, 0.2, t),
            alpha,
        );
    }

    // 60-100%: green (fades out as health approaches full)
    let t = (ratio - 0.60) / 0.40;
    Color::rgba(
        lerp(0.3, 0.1, t),
        lerp(0.8, 0.7, t),
        lerp(0.2, 0.1, t),
        alpha * (1.0 - t * 0.6), // almost invisible at full health
    )
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
