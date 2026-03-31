use bevy::prelude::*;
use crate::components::*;
use crate::resources::*;
use crate::building::{ModuleRegistry, GridOccupancy};
use crate::building::multiblock::components::*;
use crate::ui::windows::framework::*;
use crate::ui::theme::*;

// ============================================================================
// BUILD INFO SYSTEMS
// Cost summary, module hover tooltips, center of mass indicator,
// power/heat overlays.
// ============================================================================

/// Marker for the cost summary floating window
#[derive(Component)]
pub struct CostSummaryWindow;

/// Marker for the center of mass crosshair
#[derive(Component)]
pub struct CenterOfMassIndicator;

/// Marker for power overlay sprites
#[derive(Component)]
pub struct PowerOverlayTile;

/// Marker for heat overlay sprites
#[derive(Component)]
pub struct HeatOverlayTile;

// ============================================================================
// COST SUMMARY WINDOW — shows total ship stats
// ============================================================================

/// Toggle cost summary with I key during build mode
pub fn toggle_cost_summary(
    mut commands: Commands,
    keyboard: Res<Input<KeyCode>>,
    existing: Query<Entity, With<CostSummaryWindow>>,
    module_query: Query<&Module, Without<DestroyedModule>>,
    hull_query: Query<&HullSegment>,
    weapon_query: Query<(&Module, &Weapon)>,
    engine_query: Query<&Engine>,
    registry: Res<ModuleRegistry>,
    currency: Res<Currency>,
) {
    if !keyboard.just_pressed(KeyCode::I) { return; }

    // Toggle off if exists
    if let Ok(entity) = existing.get_single() {
        commands.entity(entity).despawn_recursive();
        return;
    }

    // Calculate stats
    let mut total_cost = 0u32;
    let mut total_power_gen = 0.0_f32;
    let mut total_power_use = 0.0_f32;
    let mut total_hull_hp = 0.0_f32;
    let mut module_count = 0u32;
    let mut weapon_count = 0u32;
    let mut total_dps = 0.0_f32;
    let mut total_thrust = 0.0_f32;

    for module in module_query.iter() {
        let def = registry.get(module.module_type);
        total_cost += def.cost;
        total_power_gen += def.power_generation;
        total_power_use += def.power_consumption;
        module_count += 1;
    }

    for hull in hull_query.iter() {
        total_hull_hp += hull.max_health;
        total_cost += hull.material.cost();
    }

    for (module, weapon) in weapon_query.iter() {
        weapon_count += 1;
        total_dps += weapon.damage * weapon.fire_rate;
    }

    for engine in engine_query.iter() {
        total_thrust += engine.thrust;
    }

    let power_balance = total_power_gen - total_power_use;

    // Spawn floating window
    let content = spawn_floating_window(
        &mut commands,
        "cost_summary",
        "Ship Summary",
        Vec2::new(260.0, 320.0),
        Vec2::new(20.0, 200.0),
    );
    commands.entity(content).insert(CostSummaryWindow);

    // Stats rows
    let stats = [
        ("Modules", format!("{}", module_count), ThemeColors::TEXT_PRIMARY),
        ("Total Cost", format!("{}c", total_cost), ThemeColors::ACCENT_YELLOW),
        ("Credits Left", format!("{}c", currency.credits), if currency.credits > 100 { ThemeColors::ACCENT_GREEN } else { ThemeColors::ACCENT_RED }),
        ("", String::new(), ThemeColors::TEXT_MUTED), // Spacer
        ("Power Gen", format!("+{:.0}", total_power_gen), ThemeColors::ACCENT_YELLOW),
        ("Power Use", format!("-{:.0}", total_power_use), ThemeColors::ACCENT_ORANGE),
        ("Power Balance", format!("{:.0}", power_balance), if power_balance >= 0.0 { ThemeColors::ACCENT_GREEN } else { ThemeColors::ACCENT_RED }),
        ("", String::new(), ThemeColors::TEXT_MUTED),
        ("Hull HP", format!("{:.0}", total_hull_hp), ThemeColors::ACCENT_GREEN),
        ("Weapons", format!("{}", weapon_count), ThemeColors::ACCENT_RED),
        ("Est. DPS", format!("{:.1}", total_dps), ThemeColors::ACCENT_ORANGE),
        ("Total Thrust", format!("{:.0}", total_thrust), ThemeColors::ACCENT_BLUE),
    ];

    for (label, value, color) in &stats {
        if label.is_empty() {
            // Divider
            let div = commands.spawn(NodeBundle {
                style: Style { width: Val::Percent(100.0), height: Val::Px(1.0), margin: UiRect::vertical(Val::Px(2.0)), ..default() },
                background_color: ThemeColors::BORDER_SUBTLE.into(),
                ..default()
            }).id();
            commands.entity(content).add_child(div);
        } else {
            spawn_window_row(&mut commands, content, label, value, ThemeColors::TEXT_SECONDARY, *color);
        }
    }
}

// ============================================================================
// CENTER OF MASS INDICATOR
// ============================================================================

/// Show/update center of mass crosshair during build mode
pub fn update_center_of_mass(
    mut commands: Commands,
    module_query: Query<(&Module, &GlobalTransform), Without<DestroyedModule>>,
    hull_query: Query<(&HullSegment, &GlobalTransform)>,
    existing: Query<Entity, With<CenterOfMassIndicator>>,
    current_state: Res<State<crate::states::BuildState>>,
) {
    // Only show during build mode
    if *current_state.get() == crate::states::BuildState::Inactive {
        for entity in existing.iter() {
            commands.entity(entity).despawn();
        }
        return;
    }

    // Calculate center of mass
    let mut total_mass = 0.0_f32;
    let mut weighted_pos = Vec2::ZERO;

    for (module, gt) in module_query.iter() {
        let mass = match module.module_type.category() {
            ModuleCategory::Power => 3.0,      // Reactors are heavy
            ModuleCategory::Weapons => 2.0,
            ModuleCategory::Storage => 2.5,
            _ => 1.0,
        };
        let pos = gt.translation().truncate();
        weighted_pos += pos * mass;
        total_mass += mass;
    }

    for (hull, gt) in hull_query.iter() {
        let mass = hull.material.health_multiplier(); // Heavier materials = more mass
        let pos = gt.translation().truncate();
        weighted_pos += pos * mass;
        total_mass += mass;
    }

    if total_mass < 0.01 { return; }

    let com = weighted_pos / total_mass;

    // Despawn old indicator
    for entity in existing.iter() {
        commands.entity(entity).despawn();
    }

    // Spawn crosshair at center of mass
    let off_center = com.length();
    let color = if off_center < 30.0 {
        Color::rgba(0.3, 0.8, 0.4, 0.5) // Green — well balanced
    } else if off_center < 80.0 {
        Color::rgba(0.8, 0.7, 0.2, 0.5) // Yellow — slightly off
    } else {
        Color::rgba(0.8, 0.2, 0.2, 0.5) // Red — unbalanced
    };

    // Horizontal line
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color,
                custom_size: Some(Vec2::new(20.0, 2.0)),
                ..default()
            },
            transform: Transform::from_xyz(com.x, com.y, 0.8),
            ..default()
        },
        CenterOfMassIndicator,
    ));
    // Vertical line
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color,
                custom_size: Some(Vec2::new(2.0, 20.0)),
                ..default()
            },
            transform: Transform::from_xyz(com.x, com.y, 0.8),
            ..default()
        },
        CenterOfMassIndicator,
    ));
}

// ============================================================================
// POWER OVERLAY
// ============================================================================

/// Toggle power overlay with P+O keys (hold P then press O)
pub fn toggle_power_overlay(
    mut commands: Commands,
    keyboard: Res<Input<KeyCode>>,
    module_query: Query<(&Module, &GlobalTransform), Without<DestroyedModule>>,
    power_graph: Res<crate::resources::PowerGraph>,
    existing: Query<Entity, With<PowerOverlayTile>>,
    mut active: Local<bool>,
) {
    // Toggle with F2
    if keyboard.just_pressed(KeyCode::F2) {
        *active = !*active;

        if !*active {
            for entity in existing.iter() {
                commands.entity(entity).despawn();
            }
            return;
        }
    }

    if !*active { return; }

    // Despawn old overlay
    for entity in existing.iter() {
        commands.entity(entity).despawn();
    }

    // Draw power state per module
    for (module, gt) in module_query.iter() {
        let pos = gt.translation().truncate();
        let is_powered = power_graph.powered_tiles.contains(&module.grid_position);

        let color = if module.power_generation > 0.0 {
            Color::rgba(0.9, 0.8, 0.2, 0.25) // Yellow = generator
        } else if is_powered && module.is_active {
            Color::rgba(0.2, 0.8, 0.3, 0.20) // Green = powered + active
        } else if is_powered {
            Color::rgba(0.3, 0.5, 0.7, 0.15) // Blue = powered but inactive
        } else {
            Color::rgba(0.8, 0.2, 0.2, 0.30) // Red = no power
        };

        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color,
                    custom_size: Some(Vec2::splat(60.0)),
                    ..default()
                },
                transform: Transform::from_xyz(pos.x, pos.y, 0.7),
                ..default()
            },
            PowerOverlayTile,
        ));
    }
}

// ============================================================================
// HEAT MAP OVERLAY
// ============================================================================

/// Toggle heat overlay with F3
pub fn toggle_heat_overlay(
    mut commands: Commands,
    keyboard: Res<Input<KeyCode>>,
    temp_query: Query<(&Module, &ModuleTemperature, &GlobalTransform), Without<DestroyedModule>>,
    existing: Query<Entity, With<HeatOverlayTile>>,
    mut active: Local<bool>,
) {
    if keyboard.just_pressed(KeyCode::F3) {
        *active = !*active;
        if !*active {
            for entity in existing.iter() {
                commands.entity(entity).despawn();
            }
            return;
        }
    }

    if !*active { return; }

    // Despawn old overlay
    for entity in existing.iter() {
        commands.entity(entity).despawn();
    }

    for (module, temp, gt) in temp_query.iter() {
        let pos = gt.translation().truncate();
        let heat_ratio = (temp.current / temp.max_temp).clamp(0.0, 1.0);

        // Blue → Yellow → Red gradient
        let color = if heat_ratio < 0.3 {
            Color::rgba(0.1, 0.2, 0.6, 0.15) // Cool blue
        } else if heat_ratio < 0.6 {
            Color::rgba(0.7, 0.6, 0.1, 0.20) // Warm yellow
        } else if heat_ratio < 0.85 {
            Color::rgba(0.8, 0.3, 0.1, 0.25) // Hot orange
        } else {
            Color::rgba(0.9, 0.1, 0.1, 0.35) // Critical red
        };

        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color,
                    custom_size: Some(Vec2::splat(60.0)),
                    ..default()
                },
                transform: Transform::from_xyz(pos.x, pos.y, 0.7),
                ..default()
            },
            HeatOverlayTile,
        ));
    }
}

// ============================================================================
// MODULE SEARCH FILTER
// ============================================================================

/// Resource for module search state
#[derive(Resource, Default)]
pub struct ModuleSearchState {
    pub query: String,
    pub is_active: bool,
}

/// Marker for the search input UI
#[derive(Component)]
pub struct ModuleSearchInput;
