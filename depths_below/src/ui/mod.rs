pub mod build_ui;
pub mod damage_overlay;

use std::collections::HashMap;

use bevy::prelude::*;
use crate::states::{GameState, BuildState};
use crate::resources::*;
use crate::events::*;
use crate::components::*;
use crate::camera::MainCamera;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<PrePauseState>()
            .init_resource::<CustomizationState>()
            .init_resource::<ComponentPlacementState>()
            .init_resource::<PieceCustomizationState>()
            .add_systems(Startup, setup_ui)
            .add_systems(
                Update,
                (
                    update_hud,
                    update_hud_secondary,
                    handle_notifications,
                    update_notifications,
                    handle_menu_input,
                ),
            )
            // Main menu
            .add_systems(OnEnter(GameState::MainMenu), spawn_main_menu)
            .add_systems(OnExit(GameState::MainMenu), despawn_main_menu)
            // Game Over screen
            .add_systems(OnEnter(GameState::GameOver), spawn_game_over_screen)
            .add_systems(OnExit(GameState::GameOver), despawn_game_over_screen)
            .add_systems(
                Update,
                game_over_input.run_if(in_state(GameState::GameOver)),
            )
            // Pause menu
            .add_systems(OnEnter(GameState::Paused), spawn_pause_menu)
            .add_systems(OnExit(GameState::Paused), despawn_pause_menu)
            .add_systems(
                Update,
                (
                    toggle_module_panel,
                    module_panel_input,
                    save_load_input,
                ).run_if(in_state(GameState::Paused)),
            )
            // Docked state
            .add_systems(OnEnter(GameState::Docked), spawn_docking_menu)
            .add_systems(OnExit(GameState::Docked), despawn_docking_menu)
            .add_systems(
                Update,
                docking_menu_input.run_if(in_state(GameState::Docked)),
            )
            // Game event notifications (while exploring)
            .add_systems(
                Update,
                (
                    handle_game_event_notifications,
                    update_hull_warning_overlay,
                ).run_if(in_state(GameState::Exploring)),
            )
            // Damage overlay (while exploring) — chained for correct ordering
            .add_systems(
                Update,
                (
                    damage_overlay::toggle_damage_overlay,
                    damage_overlay::spawn_overlay_legend.after(damage_overlay::toggle_damage_overlay),
                    damage_overlay::despawn_overlay_legend.after(damage_overlay::toggle_damage_overlay),
                    damage_overlay::update_damage_overlay.after(damage_overlay::spawn_overlay_legend),
                    damage_overlay::cleanup_damage_overlay.after(damage_overlay::toggle_damage_overlay),
                ).run_if(in_state(GameState::Exploring)),
            )
            // Clean up overlay legend/sprites on state transitions
            .add_systems(OnEnter(GameState::GameOver), damage_overlay::cleanup_overlay_on_exit)
            .add_systems(OnEnter(GameState::MainMenu), damage_overlay::cleanup_overlay_on_exit)
            // Crew menu toggle (while exploring)
            .add_systems(
                Update,
                (
                    toggle_crew_menu,
                    toggle_map_overlay,
                    crew_menu_assign_input,
                ).run_if(in_state(GameState::Exploring)),
            )
            // Upgrade shop (at surface base)
            .add_systems(
                Update,
                (
                    toggle_upgrade_shop,
                    upgrade_shop_input,
                ).run_if(in_state(GameState::SurfaceBase)),
            )
            // Build UI: ghost preview
            .add_systems(OnEnter(BuildState::Placing), build_ui::spawn_build_ghost)
            .add_systems(OnExit(BuildState::Placing), build_ui::despawn_build_ghost)
            // Build UI: delete highlight
            .add_systems(OnEnter(BuildState::Deleting), build_ui::spawn_delete_highlight)
            .add_systems(OnExit(BuildState::Deleting), build_ui::despawn_delete_highlight)
            // Build UI: info panel (spawn when entering build mode, despawn when leaving)
            .add_systems(OnExit(BuildState::Inactive), (
                build_ui::spawn_build_panel,
                build_ui::spawn_build_grid_lines,
                build_ui::spawn_module_outlines,
                build_ui::spawn_power_indicators,
            ))
            .add_systems(OnEnter(BuildState::Inactive), (
                build_ui::despawn_build_panel,
                build_ui::despawn_build_grid_lines,
                build_ui::despawn_module_outlines,
                build_ui::despawn_power_indicators,
            ))
            // Build UI: update systems
            .add_systems(
                Update,
                (
                    build_ui::update_build_ghost.run_if(in_state(BuildState::Placing)),
                    build_ui::update_delete_highlight.run_if(in_state(BuildState::Deleting)),
                    build_ui::update_build_panel.run_if(
                        in_state(BuildState::Placing)
                            .or_else(in_state(BuildState::Deleting)),
                    ),
                    build_ui::build_panel_click.run_if(
                        in_state(BuildState::Placing)
                            .or_else(in_state(BuildState::Deleting)),
                    ),
                    build_ui::update_build_info.run_if(
                        in_state(BuildState::Placing)
                            .or_else(in_state(BuildState::Deleting)),
                    ),
                    build_ui::update_controls_help.run_if(in_state(GameState::SurfaceBase)),
                    build_ui::update_module_tooltip.run_if(in_state(BuildState::Placing)),
                ),
            )
            // Customization panel systems
            .add_systems(
                Update,
                (
                    build_ui::spawn_customization_panel,
                    build_ui::update_customization_panel,
                    build_ui::handle_customization_input,
                ).run_if(in_state(GameState::SurfaceBase)),
            )
            // Component placement panel systems
            .add_systems(
                Update,
                (
                    build_ui::spawn_component_placement_panel,
                    build_ui::handle_component_placement_input,
                    build_ui::update_component_palette_visuals,
                    build_ui::update_component_grid_visuals,
                    build_ui::update_context_menu_visuals,
                    build_ui::handle_component_placement_keyboard,
                    build_ui::show_piece_context_menu,
                    build_ui::handle_context_menu_input,
                    build_ui::spawn_piece_customization_panel,
                    build_ui::handle_piece_customization_keyboard,
                ).run_if(in_state(BuildState::PlacingComponent)),
            );
    }
}

#[derive(Component)]
struct HudRoot;

#[derive(Component)]
pub struct DepthText;

#[derive(Component)]
pub struct PowerText;

#[derive(Component)]
pub struct OxygenText;

#[derive(Component)]
pub struct HullText;

#[derive(Component)]
pub struct FuelText;

#[derive(Component)]
pub struct BallastText;

#[derive(Component)]
pub struct AmmoText;

#[derive(Component)]
pub struct NoiseText;

#[derive(Component)]
pub struct CreditsText;

#[derive(Component)]
pub struct CrewText;

/// Marker for a HUD bar fill element
#[derive(Component)]
pub struct HudBar {
    pub kind: HudBarKind,
}

#[derive(Clone, Copy, PartialEq)]
pub enum HudBarKind {
    Hull,
    Oxygen,
    Fuel,
}

/// Marker for the depth zone indicator
#[derive(Component)]
pub struct DepthZoneText;

/// Marker for map/inventory overlay
#[derive(Component)]
pub struct MapOverlay;

/// Helper to spawn a HUD bar (background + fill)
fn spawn_hud_bar(parent: &mut ChildBuilder, kind: HudBarKind, width: f32, color: Color) {
    parent.spawn(NodeBundle {
        style: Style {
            width: Val::Px(width),
            height: Val::Px(8.0),
            ..default()
        },
        background_color: Color::rgba(0.15, 0.15, 0.15, 0.8).into(),
        ..default()
    }).with_children(|bar_bg| {
        bar_bg.spawn((
            NodeBundle {
                style: Style {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                background_color: color.into(),
                ..default()
            },
            HudBar { kind },
        ));
    });
}

/// Helper to spawn a HUD group with label
fn spawn_hud_group(parent: &mut ChildBuilder, label: &str, label_color: Color, children: impl FnOnce(&mut ChildBuilder)) {
    parent.spawn(NodeBundle {
        style: Style {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(Val::Px(6.0)),
            row_gap: Val::Px(2.0),
            ..default()
        },
        ..default()
    }).with_children(|group| {
        group.spawn(TextBundle::from_section(label, TextStyle {
            font_size: 10.0, color: label_color, ..default()
        }));
        children(group);
    });
}

/// Sets up the UI
fn setup_ui(mut commands: Commands) {
    commands.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            },
            ..default()
        },
        HudRoot,
    )).with_children(|parent| {
        // ===== TOP BAR =====
        parent.spawn(NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(4.0), Val::Px(4.0)),
                column_gap: Val::Px(4.0),
                align_items: AlignItems::Center,
                ..default()
            },
            background_color: Color::rgba(0.02, 0.03, 0.08, 0.85).into(),
            ..default()
        }).with_children(|top_bar| {
            // -- DEPTH --
            spawn_hud_group(top_bar, "DEPTH", Color::rgb(0.4, 0.4, 0.5), |group| {
                group.spawn((
                    TextBundle::from_section("0m", TextStyle {
                        font_size: 18.0, color: Color::WHITE, ..default()
                    }),
                    DepthText,
                ));
                group.spawn((
                    TextBundle::from_section("Surface", TextStyle {
                        font_size: 10.0, color: Color::rgb(0.4, 0.7, 1.0), ..default()
                    }),
                    DepthZoneText,
                ));
            });

            // Separator
            top_bar.spawn(NodeBundle {
                style: Style { width: Val::Px(1.0), height: Val::Px(30.0), ..default() },
                background_color: Color::rgba(0.3, 0.3, 0.4, 0.5).into(),
                ..default()
            });

            // -- HULL --
            spawn_hud_group(top_bar, "HULL", Color::rgb(0.4, 0.5, 0.4), |group| {
                group.spawn((
                    TextBundle::from_section("100%", TextStyle {
                        font_size: 18.0, color: Color::GREEN, ..default()
                    }),
                    HullText,
                ));
                spawn_hud_bar(group, HudBarKind::Hull, 60.0, Color::GREEN);
            });

            // Separator
            top_bar.spawn(NodeBundle {
                style: Style { width: Val::Px(1.0), height: Val::Px(30.0), ..default() },
                background_color: Color::rgba(0.3, 0.3, 0.4, 0.5).into(),
                ..default()
            });

            // -- O2 --
            spawn_hud_group(top_bar, "O2", Color::rgb(0.3, 0.5, 0.5), |group| {
                group.spawn((
                    TextBundle::from_section("100%", TextStyle {
                        font_size: 18.0, color: Color::CYAN, ..default()
                    }),
                    OxygenText,
                ));
                spawn_hud_bar(group, HudBarKind::Oxygen, 60.0, Color::CYAN);
            });

            // Separator
            top_bar.spawn(NodeBundle {
                style: Style { width: Val::Px(1.0), height: Val::Px(30.0), ..default() },
                background_color: Color::rgba(0.3, 0.3, 0.4, 0.5).into(),
                ..default()
            });

            // -- POWER --
            spawn_hud_group(top_bar, "POWER", Color::rgb(0.5, 0.5, 0.3), |group| {
                group.spawn((
                    TextBundle::from_section("0/0", TextStyle {
                        font_size: 18.0, color: Color::YELLOW, ..default()
                    }),
                    PowerText,
                ));
            });

            // Separator
            top_bar.spawn(NodeBundle {
                style: Style { width: Val::Px(1.0), height: Val::Px(30.0), ..default() },
                background_color: Color::rgba(0.3, 0.3, 0.4, 0.5).into(),
                ..default()
            });

            // -- FUEL --
            spawn_hud_group(top_bar, "FUEL", Color::rgb(0.5, 0.4, 0.3), |group| {
                group.spawn((
                    TextBundle::from_section("100%", TextStyle {
                        font_size: 18.0, color: Color::rgb(1.0, 0.6, 0.2), ..default()
                    }),
                    FuelText,
                ));
                spawn_hud_bar(group, HudBarKind::Fuel, 50.0, Color::rgb(1.0, 0.6, 0.2));
            });

            // Separator
            top_bar.spawn(NodeBundle {
                style: Style { width: Val::Px(1.0), height: Val::Px(30.0), ..default() },
                background_color: Color::rgba(0.3, 0.3, 0.4, 0.5).into(),
                ..default()
            });

            // -- BALLAST --
            spawn_hud_group(top_bar, "BLST", Color::rgb(0.3, 0.4, 0.5), |group| {
                group.spawn((
                    TextBundle::from_section("50%", TextStyle {
                        font_size: 16.0, color: Color::rgb(0.3, 0.5, 1.0), ..default()
                    }),
                    BallastText,
                ));
            });

            // -- AMMO --
            spawn_hud_group(top_bar, "AMMO", Color::rgb(0.5, 0.4, 0.3), |group| {
                group.spawn((
                    TextBundle::from_section("-/-", TextStyle {
                        font_size: 16.0, color: Color::rgb(0.9, 0.7, 0.3), ..default()
                    }),
                    AmmoText,
                ));
            });

            // -- NOISE --
            spawn_hud_group(top_bar, "NOISE", Color::rgb(0.4, 0.4, 0.4), |group| {
                group.spawn((
                    TextBundle::from_section("0", TextStyle {
                        font_size: 16.0, color: Color::rgb(0.6, 0.6, 0.6), ..default()
                    }),
                    NoiseText,
                ));
            });

            // Separator
            top_bar.spawn(NodeBundle {
                style: Style { width: Val::Px(1.0), height: Val::Px(30.0), ..default() },
                background_color: Color::rgba(0.3, 0.3, 0.4, 0.5).into(),
                ..default()
            });

            // -- CREDITS --
            spawn_hud_group(top_bar, "CRED", Color::rgb(0.5, 0.5, 0.3), |group| {
                group.spawn((
                    TextBundle::from_section("500", TextStyle {
                        font_size: 16.0, color: Color::rgb(0.9, 0.8, 0.3), ..default()
                    }),
                    CreditsText,
                ));
            });

            // -- CREW --
            spawn_hud_group(top_bar, "CREW", Color::rgb(0.4, 0.5, 0.4), |group| {
                group.spawn((
                    TextBundle::from_section("0/0", TextStyle {
                        font_size: 16.0, color: Color::rgb(0.7, 0.9, 0.7), ..default()
                    }),
                    CrewText,
                ));
            });
        });

        // ===== NOTIFICATION CONTAINER (right side) =====
        parent.spawn((
            NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    right: Val::Px(10.0),
                    top: Val::Px(60.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    max_width: Val::Px(380.0),
                    ..default()
                },
                ..default()
            },
            NotificationContainer,
        ));

        // ===== BOTTOM BAR =====
        parent.spawn(NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Px(28.0),
                padding: UiRect::new(Val::Px(12.0), Val::Px(12.0), Val::Px(6.0), Val::Px(6.0)),
                align_items: AlignItems::Center,
                ..default()
            },
            background_color: Color::rgba(0.02, 0.03, 0.08, 0.75).into(),
            ..default()
        }).with_children(|parent| {
            parent.spawn((
                TextBundle::from_section(
                    "WASD: Move | Q/E: Ballast | Space: Fire | Z: Sonar | C: Crew | M: Map | ESC: Pause",
                    TextStyle { font_size: 13.0, color: Color::rgb(0.45, 0.50, 0.55), ..default() },
                ),
                build_ui::ControlsHelpText,
            ));
        });
    });
}

/// Returns the depth zone name for a given depth
fn depth_zone_name(depth: f32) -> &'static str {
    if depth < 50.0 { "Surface" }
    else if depth < 200.0 { "Shallows" }
    else if depth < 500.0 { "Twilight Zone" }
    else if depth < 1000.0 { "Dark Zone" }
    else if depth < 2000.0 { "Abyss" }
    else { "Hadal Trench" }
}

/// Updates HUD text and bars
pub fn update_hud(
    depth_state: Res<DepthState>,
    power_state: Res<PowerState>,
    oxygen_state: Res<OxygenState>,
    hull_state: Res<HullState>,
    time: Res<Time>,
    mut depth_query: Query<&mut Text, (With<DepthText>, Without<PowerText>, Without<OxygenText>, Without<HullText>, Without<DepthZoneText>)>,
    mut depth_zone_query: Query<&mut Text, (With<DepthZoneText>, Without<DepthText>, Without<PowerText>, Without<OxygenText>, Without<HullText>)>,
    mut power_query: Query<&mut Text, (With<PowerText>, Without<DepthText>, Without<OxygenText>, Without<HullText>, Without<DepthZoneText>)>,
    mut oxygen_query: Query<&mut Text, (With<OxygenText>, Without<DepthText>, Without<PowerText>, Without<HullText>, Without<DepthZoneText>)>,
    mut hull_query: Query<&mut Text, (With<HullText>, Without<DepthText>, Without<PowerText>, Without<OxygenText>, Without<DepthZoneText>)>,
    mut bar_query: Query<(&HudBar, &mut Style, &mut BackgroundColor)>,
) {
    // Depth
    if let Ok(mut text) = depth_query.get_single_mut() {
        text.sections[0].value = format!("{:.0}m", depth_state.current_depth);
        text.sections[0].style.color = if depth_state.current_depth > 1000.0 {
            Color::rgb(1.0, 0.4, 0.4)
        } else if depth_state.current_depth > 500.0 {
            Color::rgb(0.7, 0.7, 1.0)
        } else {
            Color::WHITE
        };
    }
    if let Ok(mut text) = depth_zone_query.get_single_mut() {
        text.sections[0].value = depth_zone_name(depth_state.current_depth).to_string();
    }

    // Power
    if let Ok(mut text) = power_query.get_single_mut() {
        let gen = power_state.total_power_generation;
        let con = power_state.total_power_consumption;
        text.sections[0].value = format!("{:.0}/{:.0}", gen, con);
        if power_state.power_balance < 0.0 {
            // Blink red when power deficit
            let blink = (time.elapsed_seconds() * 4.0).sin() > 0.0;
            text.sections[0].style.color = if blink { Color::RED } else { Color::rgb(0.6, 0.2, 0.2) };
        } else {
            text.sections[0].style.color = Color::YELLOW;
        }
    }

    // Oxygen
    let o2_pct = if oxygen_state.max_oxygen > 0.0 {
        oxygen_state.current_oxygen / oxygen_state.max_oxygen
    } else { 1.0 };
    let o2_pct_i = (o2_pct * 100.0) as i32;
    if let Ok(mut text) = oxygen_query.get_single_mut() {
        text.sections[0].value = format!("{}%", o2_pct_i);
        if o2_pct_i < 20 {
            let blink = (time.elapsed_seconds() * 5.0).sin() > 0.0;
            text.sections[0].style.color = if blink { Color::RED } else { Color::rgb(0.5, 0.1, 0.1) };
        } else if o2_pct_i < 50 {
            text.sections[0].style.color = Color::YELLOW;
        } else {
            text.sections[0].style.color = Color::CYAN;
        }
    }

    // Hull
    let hull_pct = hull_state.hull_integrity;
    let hull_pct_i = (hull_pct * 100.0) as i32;
    if let Ok(mut text) = hull_query.get_single_mut() {
        text.sections[0].value = format!("{}%", hull_pct_i);
        if hull_pct_i < 20 {
            let blink = (time.elapsed_seconds() * 5.0).sin() > 0.0;
            text.sections[0].style.color = if blink { Color::RED } else { Color::rgb(0.5, 0.1, 0.1) };
        } else if hull_pct_i < 50 {
            text.sections[0].style.color = Color::YELLOW;
        } else {
            text.sections[0].style.color = Color::GREEN;
        }
    }

    // Update HUD bars
    for (bar, mut style, mut bg) in bar_query.iter_mut() {
        let (pct, color) = match bar.kind {
            HudBarKind::Hull => {
                let c = if hull_pct < 0.3 { Color::RED } else if hull_pct < 0.6 { Color::YELLOW } else { Color::GREEN };
                (hull_pct, c)
            }
            HudBarKind::Oxygen => {
                let c = if o2_pct < 0.3 { Color::RED } else if o2_pct < 0.5 { Color::YELLOW } else { Color::CYAN };
                (o2_pct, c)
            }
            HudBarKind::Fuel => continue, // handled in update_hud_secondary
        };
        style.width = Val::Percent(pct * 100.0);
        *bg = color.into();
    }
}

/// Updates secondary HUD elements: Fuel, Ballast, Ammo, Noise, Credits, Crew
pub fn update_hud_secondary(
    fuel_state: Res<FuelState>,
    noise_state: Res<NoiseState>,
    currency: Res<Currency>,
    staffing_state: Res<StaffingState>,
    time: Res<Time>,
    ballast_query: Query<&Ballast>,
    weapon_query: Query<&Weapon>,
    mut fuel_query: Query<&mut Text, (With<FuelText>, Without<BallastText>, Without<AmmoText>, Without<NoiseText>, Without<CreditsText>, Without<CrewText>)>,
    mut ballast_text_query: Query<&mut Text, (With<BallastText>, Without<FuelText>, Without<AmmoText>, Without<NoiseText>, Without<CreditsText>, Without<CrewText>)>,
    mut ammo_query: Query<&mut Text, (With<AmmoText>, Without<FuelText>, Without<BallastText>, Without<NoiseText>, Without<CreditsText>, Without<CrewText>)>,
    mut noise_query: Query<&mut Text, (With<NoiseText>, Without<FuelText>, Without<BallastText>, Without<AmmoText>, Without<CreditsText>, Without<CrewText>)>,
    mut credits_query: Query<&mut Text, (With<CreditsText>, Without<FuelText>, Without<BallastText>, Without<AmmoText>, Without<NoiseText>, Without<CrewText>)>,
    mut crew_query_hud: Query<&mut Text, (With<CrewText>, Without<FuelText>, Without<BallastText>, Without<AmmoText>, Without<NoiseText>, Without<CreditsText>)>,
    mut bar_query: Query<(&HudBar, &mut Style, &mut BackgroundColor)>,
) {
    // Fuel
    let fuel_pct = if fuel_state.max_fuel > 0.0 {
        fuel_state.current_fuel / fuel_state.max_fuel
    } else { 1.0 };
    let fuel_pct_i = (fuel_pct * 100.0) as i32;
    if let Ok(mut text) = fuel_query.get_single_mut() {
        text.sections[0].value = format!("{}%", fuel_pct_i);
        if fuel_pct_i < 15 {
            let blink = (time.elapsed_seconds() * 4.0).sin() > 0.0;
            text.sections[0].style.color = if blink { Color::RED } else { Color::rgb(0.5, 0.1, 0.1) };
        } else if fuel_pct_i < 30 {
            text.sections[0].style.color = Color::YELLOW;
        } else {
            text.sections[0].style.color = Color::rgb(1.0, 0.6, 0.2);
        }
    }

    // Update fuel bar
    for (bar, mut style, mut bg) in bar_query.iter_mut() {
        if bar.kind == HudBarKind::Fuel {
            style.width = Val::Percent(fuel_pct * 100.0);
            *bg = if fuel_pct < 0.25 { Color::RED } else { Color::rgb(1.0, 0.6, 0.2) }.into();
        }
    }

    // Ballast
    if let Ok(mut text) = ballast_text_query.get_single_mut() {
        let ballasts: Vec<f32> = ballast_query.iter().map(|b| b.current_level).collect();
        if ballasts.is_empty() {
            text.sections[0].value = "N/A".to_string();
            text.sections[0].style.color = Color::GRAY;
        } else {
            let avg = ballasts.iter().sum::<f32>() / ballasts.len() as f32;
            let pct = (avg * 100.0) as i32;
            text.sections[0].value = format!("{}%", pct);
            text.sections[0].style.color = Color::rgb(0.3, 0.5, 1.0);
        }
    }

    // Ammo
    if let Ok(mut text) = ammo_query.get_single_mut() {
        let mut total_ammo = 0u32;
        let mut total_max = 0u32;
        for weapon in weapon_query.iter() {
            total_ammo += weapon.ammo;
            total_max += weapon.max_ammo;
        }
        if total_max == 0 {
            text.sections[0].value = "N/A".to_string();
            text.sections[0].style.color = Color::GRAY;
        } else {
            text.sections[0].value = format!("{}/{}", total_ammo, total_max);
            let pct = total_ammo as f32 / total_max as f32;
            if pct < 0.15 {
                let blink = (time.elapsed_seconds() * 3.0).sin() > 0.0;
                text.sections[0].style.color = if blink { Color::RED } else { Color::rgb(0.5, 0.1, 0.1) };
            } else if pct < 0.3 {
                text.sections[0].style.color = Color::RED;
            } else {
                text.sections[0].style.color = Color::rgb(0.9, 0.7, 0.3);
            }
        }
    }

    // Noise
    if let Ok(mut text) = noise_query.get_single_mut() {
        let noise = noise_state.noise_level as i32;
        text.sections[0].value = format!("{}", noise);
        text.sections[0].style.color = if noise > 80 {
            Color::RED
        } else if noise > 50 {
            Color::YELLOW
        } else {
            Color::rgb(0.5, 0.5, 0.5)
        };
    }

    // Credits
    if let Ok(mut text) = credits_query.get_single_mut() {
        text.sections[0].value = format!("{}", currency.credits);
    }

    // Crew staffing
    if let Ok(mut text) = crew_query_hud.get_single_mut() {
        text.sections[0].value = format!("{}/{}", staffing_state.total_crew, staffing_state.total_berths);
        text.sections[0].style.color = if staffing_state.total_crew > staffing_state.total_berths {
            Color::RED
        } else {
            Color::rgb(0.7, 0.9, 0.7)
        };
    }
}

/// Maximum number of notifications visible at once
const MAX_NOTIFICATIONS: usize = 6;
/// Minimum seconds between duplicate notifications
const NOTIFICATION_DEDUP_SECS: f32 = 3.0;

/// Spawns toast notifications from events, with deduplication and cap
fn handle_notifications(
    mut commands: Commands,
    mut notification_events: EventReader<ShowNotification>,
    container_query: Query<Entity, With<NotificationContainer>>,
    existing_toasts: Query<(Entity, &Text), With<NotificationToast>>,
    mut recent_messages: Local<Vec<(String, f32)>>,
    time: Res<Time>,
) {
    let Ok(container) = container_query.get_single() else { return };

    // Clean up expired dedup entries
    let now = time.elapsed_seconds();
    recent_messages.retain(|(_, t)| now - *t < NOTIFICATION_DEDUP_SECS);

    // Count existing toasts
    let mut toast_count = existing_toasts.iter().count();

    for event in notification_events.iter() {
        // Skip duplicate messages within the dedup window
        if recent_messages.iter().any(|(msg, _)| msg == &event.message) {
            continue;
        }

        // Cap max visible notifications - remove oldest if at limit
        if toast_count >= MAX_NOTIFICATIONS {
            if let Some((oldest_entity, _)) = existing_toasts.iter().next() {
                commands.entity(oldest_entity).despawn_recursive();
                toast_count -= 1;
            }
        }

        let (color, bg_color, prefix) = match event.notification_type {
            NotificationType::Danger => (
                Color::rgb(1.0, 0.3, 0.3),
                Color::rgba(0.4, 0.05, 0.05, 0.85),
                "[!] ",
            ),
            NotificationType::Warning => (
                Color::rgb(1.0, 0.8, 0.2),
                Color::rgba(0.3, 0.25, 0.02, 0.8),
                "[*] ",
            ),
            NotificationType::Success => (
                Color::rgb(0.3, 1.0, 0.3),
                Color::rgba(0.02, 0.2, 0.05, 0.8),
                "[+] ",
            ),
            NotificationType::Info => (
                Color::rgb(0.85, 0.9, 0.95),
                Color::rgba(0.05, 0.08, 0.15, 0.8),
                "",
            ),
        };
        let msg = format!("{}{}", prefix, event.message);
        commands.spawn((
            TextBundle::from_section(&msg, TextStyle {
                font_size: 15.0, color, ..default()
            }).with_style(Style {
                margin: UiRect::bottom(Val::Px(3.0)),
                padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(4.0), Val::Px(4.0)),
                ..default()
            }).with_background_color(bg_color),
            NotificationToast { timer: Timer::from_seconds(event.duration, TimerMode::Once) },
        )).set_parent(container);

        recent_messages.push((event.message.clone(), now));
        toast_count += 1;
    }
}

/// Fades and despawns notification toasts
fn update_notifications(
    mut commands: Commands,
    time: Res<Time>,
    mut toast_query: Query<(Entity, &mut NotificationToast, &mut Text)>,
) {
    for (entity, mut toast, mut text) in toast_query.iter_mut() {
        toast.timer.tick(time.delta());
        let remaining = toast.timer.remaining_secs() / toast.timer.duration().as_secs_f32();
        if remaining < 0.3 {
            let alpha = remaining / 0.3;
            for section in text.sections.iter_mut() {
                let c = section.style.color;
                section.style.color = Color::rgba(c.r(), c.g(), c.b(), alpha);
            }
        }
        if toast.timer.finished() {
            commands.entity(entity).despawn_recursive();
        }
    }
}

/// Handles menu input
fn handle_menu_input(
    keyboard: Res<Input<KeyCode>>,
    current_state: Res<State<GameState>>,
    build_state: Res<State<BuildState>>,
    customization_state: Res<CustomizationState>,
    mut next_state: ResMut<NextState<GameState>>,
    mut pre_pause: ResMut<PrePauseState>,
    mut load_events: EventWriter<LoadGameRequest>,
    module_panel: Query<Entity, With<ModulePanelOverlay>>,
    upgrade_shop: Query<Entity, With<UpgradeShopOverlay>>,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        // Don't open pause menu if upgrade shop overlay is open (it handles ESC itself)
        if !upgrade_shop.is_empty() {
            return;
        }
        match current_state.get() {
            GameState::Exploring | GameState::SurfaceBase => {
                pre_pause.0 = Some(*current_state.get());
                next_state.set(GameState::Paused);
            }
            GameState::Docked => {
                next_state.set(GameState::Exploring);
            }
            GameState::Paused => {
                let target = pre_pause.0.unwrap_or(GameState::Exploring);
                next_state.set(target);
            }
            _ => {}
        }
    }

    // Load from main menu: L+1/2/3/0
    if *current_state.get() == GameState::MainMenu && keyboard.pressed(KeyCode::L) {
        if keyboard.just_pressed(KeyCode::Key1) {
            load_events.send(LoadGameRequest { slot: 0 });
        }
        if keyboard.just_pressed(KeyCode::Key2) {
            load_events.send(LoadGameRequest { slot: 1 });
        }
        if keyboard.just_pressed(KeyCode::Key3) {
            load_events.send(LoadGameRequest { slot: 2 });
        }
        if keyboard.just_pressed(KeyCode::Key0) {
            load_events.send(LoadGameRequest { slot: 99 });
        }
    }

    // Don't process Enter for state transitions while module panel, building, or customizing is active
    let is_building = *build_state.get() != BuildState::Inactive;
    let is_customizing = customization_state.active;

    if keyboard.just_pressed(KeyCode::Return)
        && module_panel.is_empty()
        && upgrade_shop.is_empty()
        && !is_building
        && !is_customizing
    {
        match current_state.get() {
            GameState::MainMenu => next_state.set(GameState::SurfaceBase),
            GameState::SurfaceBase => next_state.set(GameState::Exploring),
            _ => {}
        }
    }
}

// ============================================================================
// GAME EVENT NOTIFICATIONS
// ============================================================================

/// Reads from currently-silent events and sends ShowNotification
fn handle_game_event_notifications(
    mut power_events: EventReader<PowerStateChanged>,
    mut oxygen_events: EventReader<OxygenStateChanged>,
    mut breach_events: EventReader<HullBreached>,
    mut crew_damage_events: EventReader<CrewDamaged>,
    crew_query: Query<&CrewMember>,
    weapon_query: Query<&Weapon>,
    mut notifications: EventWriter<ShowNotification>,
    mut low_ammo_warned: Local<bool>,
) {
    // Power state changes
    for event in power_events.iter() {
        if event.is_critical {
            notifications.send(ShowNotification {
                message: "WARNING: Power deficit! Systems failing!".into(),
                notification_type: NotificationType::Danger,
                duration: 4.0,
            });
        } else {
            notifications.send(ShowNotification {
                message: "Power restored. Systems nominal.".into(),
                notification_type: NotificationType::Success,
                duration: 3.0,
            });
        }
    }

    // Oxygen state changes
    for event in oxygen_events.iter() {
        if event.is_critical {
            notifications.send(ShowNotification {
                message: format!("OXYGEN CRITICAL! ({:.0}%) Crew suffocating!", event.new_level * 100.0),
                notification_type: NotificationType::Danger,
                duration: 4.0,
            });
        }
    }

    // Hull breaches
    for event in breach_events.iter() {
        notifications.send(ShowNotification {
            message: format!("HULL BREACH! Flooding in progress! (Severity: {:.0}%)", event.severity * 100.0),
            notification_type: NotificationType::Danger,
            duration: 4.0,
        });
    }

    // Crew damage
    for event in crew_damage_events.iter() {
        if let Ok(crew) = crew_query.get(event.crew) {
            notifications.send(ShowNotification {
                message: format!("{} taking damage! ({:?}, -{:.0})", crew.name, event.source, event.amount),
                notification_type: NotificationType::Warning,
                duration: 2.5,
            });
        }
    }

    // Low ammo warning (Phase 3.2)
    let any_low_ammo = weapon_query.iter().any(|w| {
        w.max_ammo > 0 && w.ammo > 0 && (w.ammo as f32) < (w.max_ammo as f32) * 0.25
    });
    if any_low_ammo && !*low_ammo_warned {
        *low_ammo_warned = true;
        notifications.send(ShowNotification {
            message: "Low ammo! Weapons below 25% capacity.".into(),
            notification_type: NotificationType::Warning,
            duration: 3.0,
        });
    } else if !any_low_ammo {
        *low_ammo_warned = false;
    }
}

// ============================================================================
// CREW MANAGEMENT MENU (C key)
// ============================================================================

/// Toggles crew management overlay with C key
fn toggle_crew_menu(
    mut commands: Commands,
    keyboard: Res<Input<KeyCode>>,
    existing_menu: Query<Entity, With<CrewMenuOverlay>>,
    crew_query: Query<(Entity, &CrewMember)>,
    station_query: Query<(&CrewStation, &Module)>,
    staffing_state: Res<StaffingState>,
) {
    if !keyboard.just_pressed(KeyCode::C) {
        return;
    }

    // Toggle off if already open
    if let Ok(entity) = existing_menu.get_single() {
        commands.entity(entity).despawn_recursive();
        return;
    }

    // Build a map: crew entity -> assigned module grid position
    let mut crew_assignments: std::collections::HashMap<Entity, IVec2> = std::collections::HashMap::new();
    for (cs, module) in station_query.iter() {
        if let Some(crew_entity) = cs.assigned_crew {
            crew_assignments.insert(crew_entity, module.grid_position);
        }
    }

    // Spawn crew management panel
    commands.spawn((
        NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                top: Val::Px(60.0),
                width: Val::Px(380.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(6.0),
                ..default()
            },
            background_color: Color::rgba(0.0, 0.0, 0.1, 0.85).into(),
            ..default()
        },
        CrewMenuOverlay,
    )).with_children(|parent| {
        parent.spawn(TextBundle::from_section(
            format!("CREW MANAGEMENT - {}/{} berths - {}/{} stations",
                staffing_state.total_crew, staffing_state.total_berths,
                staffing_state.staffed_stations, staffing_state.total_stations),
            TextStyle { font_size: 20.0, color: Color::WHITE, ..default() },
        ));

        for (entity, crew) in crew_query.iter() {
            let status = if crew.health <= 0.0 {
                "DEAD".to_string()
            } else if let Some(grid) = crew_assignments.get(&entity) {
                format!("{:?} -> ({},{})", crew.state, grid.x, grid.y)
            } else {
                format!("{:?} (Idle)", crew.state)
            };

            parent.spawn(TextBundle::from_section(
                format!("{} | HP:{:.0} O2:{:.0} Morale:{:.0} | {}",
                    crew.name, crew.health, crew.oxygen, crew.morale, status),
                TextStyle { font_size: 15.0, color: Color::rgb(0.8, 0.8, 0.8), ..default() },
            ));
        }

        parent.spawn(TextBundle::from_section(
            "Press C to close",
            TextStyle { font_size: 12.0, color: Color::DARK_GRAY, ..default() },
        ));
    });
}

/// Stub for crew assignment input — press 1 to manually assign idle crew to first unstaffed weapon
fn crew_menu_assign_input(
    keyboard: Res<Input<KeyCode>>,
    crew_query: Query<(Entity, &CrewMember)>,
    mut station_query: Query<(&mut CrewStation, &Module), With<Weapon>>,
    mut notifications: EventWriter<ShowNotification>,
) {
    // Press 1 to assign first idle crew to first unstaffed weapon station
    if keyboard.just_pressed(KeyCode::Key1) {
        // Find crew entities already assigned to any station
        let assigned_crew: std::collections::HashSet<Entity> = station_query
            .iter()
            .filter_map(|(cs, _)| cs.assigned_crew)
            .collect();

        let first_idle = crew_query.iter()
            .find(|(entity, c)| c.health > 0.0 && !assigned_crew.contains(entity));

        let first_unstaffed = station_query.iter_mut()
            .find(|(cs, _)| cs.assigned_crew.is_none());

        if let (Some((crew_entity, crew)), Some((mut cs, _module))) = (first_idle, first_unstaffed) {
            cs.assigned_crew = Some(crew_entity);
            cs.manually_assigned = true;
            notifications.send(ShowNotification {
                message: format!("{} assigned to weapon", crew.name),
                notification_type: NotificationType::Success,
                duration: 2.0,
            });
        }
    }
}

// ============================================================================
// MAP / INVENTORY OVERLAY (M key)
// ============================================================================

fn toggle_map_overlay(
    mut commands: Commands,
    keyboard: Res<Input<KeyCode>>,
    existing: Query<Entity, With<MapOverlay>>,
    discovered: Res<DiscoveredLocations>,
    inventory: Res<Inventory>,
    statistics: Res<Statistics>,
) {
    if !keyboard.just_pressed(KeyCode::M) {
        return;
    }

    if let Ok(entity) = existing.get_single() {
        commands.entity(entity).despawn_recursive();
        return;
    }

    commands.spawn((
        NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                right: Val::Px(10.0),
                top: Val::Px(60.0),
                width: Val::Px(300.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(6.0),
                ..default()
            },
            background_color: Color::rgba(0.0, 0.0, 0.1, 0.85).into(),
            ..default()
        },
        MapOverlay,
    )).with_children(|parent| {
        parent.spawn(TextBundle::from_section("MAP & INVENTORY", TextStyle {
            font_size: 22.0, color: Color::WHITE, ..default()
        }));

        // Discovered locations
        parent.spawn(TextBundle::from_section(
            format!("Wrecks found: {}", discovered.wrecks.len()),
            TextStyle { font_size: 16.0, color: Color::rgb(0.8, 0.6, 0.4), ..default() },
        ));
        parent.spawn(TextBundle::from_section(
            format!("Caves found: {}", discovered.caves.len()),
            TextStyle { font_size: 16.0, color: Color::rgb(0.6, 0.6, 0.6), ..default() },
        ));
        parent.spawn(TextBundle::from_section(
            format!("Settlements: {}", discovered.settlements.len()),
            TextStyle { font_size: 16.0, color: Color::rgb(0.4, 0.8, 0.4), ..default() },
        ));

        // Inventory
        parent.spawn(TextBundle::from_section("--- Inventory ---", TextStyle {
            font_size: 18.0, color: Color::YELLOW, ..default()
        }));

        if inventory.items.is_empty() {
            parent.spawn(TextBundle::from_section("(empty)", TextStyle {
                font_size: 14.0, color: Color::GRAY, ..default()
            }));
        } else {
            for (item_type, count) in &inventory.items {
                parent.spawn(TextBundle::from_section(
                    format!("{}: x{}", item_type.name(), count),
                    TextStyle { font_size: 14.0, color: Color::WHITE, ..default() },
                ));
            }
        }

        parent.spawn(TextBundle::from_section(
            format!("Weight: {:.0}/{:.0}", inventory.current_weight, inventory.max_capacity),
            TextStyle { font_size: 14.0, color: Color::GRAY, ..default() },
        ));

        // Logs found
        if !statistics.logs_found.is_empty() {
            parent.spawn(TextBundle::from_section("--- Logs ---", TextStyle {
                font_size: 18.0, color: Color::CYAN, ..default()
            }));
            for log in &statistics.logs_found {
                parent.spawn(TextBundle::from_section(log, TextStyle {
                    font_size: 14.0, color: Color::rgb(0.7, 0.7, 0.8), ..default()
                }));
            }
        }

        parent.spawn(TextBundle::from_section(
            "Press M to close",
            TextStyle { font_size: 12.0, color: Color::DARK_GRAY, ..default() },
        ));
    });
}

// ============================================================================
// MAIN MENU SCREEN
// ============================================================================

fn spawn_main_menu(mut commands: Commands) {
    commands.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                ..default()
            },
            background_color: Color::rgba(0.01, 0.03, 0.10, 0.97).into(),
            z_index: ZIndex::Global(100),
            ..default()
        },
        MainMenuOverlay,
    )).with_children(|parent| {
        // Title container with subtle border
        parent.spawn(NodeBundle {
            style: Style {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::new(Val::Px(60.0), Val::Px(60.0), Val::Px(30.0), Val::Px(30.0)),
                row_gap: Val::Px(8.0),
                ..default()
            },
            background_color: Color::rgba(0.03, 0.06, 0.18, 0.6).into(),
            ..default()
        }).with_children(|title_box| {
            // Decorative line
            title_box.spawn(NodeBundle {
                style: Style { width: Val::Px(200.0), height: Val::Px(1.0), margin: UiRect::bottom(Val::Px(10.0)), ..default() },
                background_color: Color::rgba(0.2, 0.5, 0.8, 0.4).into(),
                ..default()
            });

            title_box.spawn(TextBundle::from_section("DEPTHS BELOW", TextStyle {
                font_size: 72.0, color: Color::rgb(0.25, 0.65, 1.0), ..default()
            }));

            title_box.spawn(TextBundle::from_section("Submarine Survival", TextStyle {
                font_size: 18.0, color: Color::rgb(0.35, 0.45, 0.55), ..default()
            }));

            // Decorative line
            title_box.spawn(NodeBundle {
                style: Style { width: Val::Px(200.0), height: Val::Px(1.0), margin: UiRect::top(Val::Px(10.0)), ..default() },
                background_color: Color::rgba(0.2, 0.5, 0.8, 0.4).into(),
                ..default()
            });
        });

        // Spacer
        parent.spawn(NodeBundle {
            style: Style { height: Val::Px(30.0), ..default() },
            ..default()
        });

        // Actions container
        parent.spawn(NodeBundle {
            style: Style {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(12.0),
                ..default()
            },
            ..default()
        }).with_children(|actions| {
            actions.spawn(TextBundle::from_section("[ENTER]  New Expedition", TextStyle {
                font_size: 22.0, color: Color::rgb(0.8, 0.9, 1.0), ..default()
            }));

            // Show available saves for loading
            let slots = crate::meta::get_save_slots();
            let has_saves = slots.iter().any(|(_, info)| info.is_some());
            if has_saves {
                actions.spawn(NodeBundle {
                    style: Style { height: Val::Px(10.0), ..default() },
                    ..default()
                });

                actions.spawn(NodeBundle {
                    style: Style {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        padding: UiRect::new(Val::Px(20.0), Val::Px(20.0), Val::Px(12.0), Val::Px(12.0)),
                        row_gap: Val::Px(6.0),
                        ..default()
                    },
                    background_color: Color::rgba(0.05, 0.08, 0.15, 0.5).into(),
                    ..default()
                }).with_children(|save_box| {
                    save_box.spawn(TextBundle::from_section("SAVED EXPEDITIONS", TextStyle {
                        font_size: 14.0, color: Color::rgb(0.4, 0.6, 0.8), ..default()
                    }));

                    for (slot, info) in &slots {
                        if let Some(info) = info {
                            let label = if *slot == 99 { "Auto".to_string() } else { format!("Slot {}", slot + 1) };
                            let key = if *slot == 99 { "L+0" } else { match slot { 0 => "L+1", 1 => "L+2", 2 => "L+3", _ => "L+?" } };
                            let time_min = (info.play_time / 60.0) as i32;
                            let time_sec = (info.play_time % 60.0) as i32;
                            save_box.spawn(TextBundle::from_section(
                                format!("[{}]  {} - {:.0}m depth, {}:{:02} played",
                                    key, label, info.depth, time_min, time_sec),
                                TextStyle { font_size: 16.0, color: Color::rgb(0.6, 0.8, 0.6), ..default() },
                            ));
                        }
                    }
                });
            }
        });

        // Spacer
        parent.spawn(NodeBundle {
            style: Style { height: Val::Px(40.0), ..default() },
            ..default()
        });

        // Controls hint
        parent.spawn(NodeBundle {
            style: Style {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(4.0),
                ..default()
            },
            ..default()
        }).with_children(|hints| {
            hints.spawn(TextBundle::from_section(
                "Build your submarine. Explore the depths. Survive.",
                TextStyle { font_size: 14.0, color: Color::rgb(0.35, 0.4, 0.45), ..default() },
            ));
        });
    });
}

fn despawn_main_menu(
    mut commands: Commands,
    query: Query<Entity, With<MainMenuOverlay>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

// ============================================================================
// GAME OVER SCREEN
// ============================================================================

fn spawn_game_over_screen(
    mut commands: Commands,
    statistics: Res<Statistics>,
    victory_state: Res<VictoryState>,
) {
    let is_victory = victory_state.achieved;
    let bg_color = if is_victory {
        Color::rgba(0.01, 0.05, 0.03, 0.92)
    } else {
        Color::rgba(0.08, 0.02, 0.02, 0.92)
    };

    commands.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                ..default()
            },
            background_color: bg_color.into(),
            ..default()
        },
        GameOverOverlay,
    )).with_children(|parent| {
        // Title section
        if is_victory {
            parent.spawn(TextBundle::from_section("VICTORY", TextStyle {
                font_size: 68.0, color: Color::rgb(0.3, 1.0, 0.3), ..default()
            }));
            parent.spawn(NodeBundle { style: Style { height: Val::Px(8.0), ..default() }, ..default() });
            parent.spawn(TextBundle::from_section(
                "You reached the deepest abyss and uncovered the truth.",
                TextStyle { font_size: 20.0, color: Color::rgb(0.7, 0.9, 1.0), ..default() },
            ));
            parent.spawn(TextBundle::from_section(
                "The ocean remembers those who dare to descend.",
                TextStyle { font_size: 16.0, color: Color::rgb(0.4, 0.6, 0.7), ..default() },
            ));
        } else {
            parent.spawn(TextBundle::from_section("LOST AT SEA", TextStyle {
                font_size: 68.0, color: Color::rgb(0.8, 0.15, 0.1), ..default()
            }));
            parent.spawn(NodeBundle { style: Style { height: Val::Px(4.0), ..default() }, ..default() });
            parent.spawn(TextBundle::from_section(
                "The deep claims another vessel.",
                TextStyle { font_size: 18.0, color: Color::rgb(0.5, 0.35, 0.35), ..default() },
            ));
        }

        parent.spawn(NodeBundle { style: Style { height: Val::Px(24.0), ..default() }, ..default() });

        // Stats panel
        parent.spawn(NodeBundle {
            style: Style {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                padding: UiRect::new(Val::Px(30.0), Val::Px(30.0), Val::Px(16.0), Val::Px(16.0)),
                row_gap: Val::Px(8.0),
                ..default()
            },
            background_color: Color::rgba(0.05, 0.05, 0.08, 0.6).into(),
            ..default()
        }).with_children(|stats| {
            stats.spawn(TextBundle::from_section("EXPEDITION LOG", TextStyle {
                font_size: 14.0, color: Color::rgb(0.4, 0.5, 0.6), ..default()
            }));

            // Decorative line
            stats.spawn(NodeBundle {
                style: Style { width: Val::Px(180.0), height: Val::Px(1.0), margin: UiRect::bottom(Val::Px(4.0)), ..default() },
                background_color: Color::rgba(0.3, 0.4, 0.5, 0.3).into(),
                ..default()
            });

            let time_min = (statistics.play_time_seconds / 60.0) as i32;
            let time_sec = (statistics.play_time_seconds % 60.0) as i32;

            let stat_items = [
                (format!("Max Depth        {:.0}m", statistics.max_depth_reached), Color::rgb(0.5, 0.7, 1.0)),
                (format!("Time Survived    {}:{:02}", time_min, time_sec), Color::WHITE),
                (format!("Creatures Slain  {}", statistics.creatures_killed), Color::rgb(0.9, 0.6, 0.3)),
                (format!("Crew Lost        {}", statistics.crew_lost), Color::rgb(0.9, 0.4, 0.4)),
            ];

            for (text, color) in stat_items {
                stats.spawn(TextBundle::from_section(text, TextStyle {
                    font_size: 20.0, color, ..default()
                }));
            }

            if !statistics.logs_found.is_empty() {
                stats.spawn(TextBundle::from_section(
                    format!("Logs Found       {}", statistics.logs_found.len()),
                    TextStyle { font_size: 20.0, color: Color::CYAN, ..default() },
                ));
            }
        });

        parent.spawn(NodeBundle { style: Style { height: Val::Px(30.0), ..default() }, ..default() });

        parent.spawn(TextBundle::from_section(
            "[ENTER]  Return to Surface",
            TextStyle { font_size: 18.0, color: Color::rgb(0.45, 0.5, 0.55), ..default() },
        ));
    });
}

fn despawn_game_over_screen(
    mut commands: Commands,
    query: Query<Entity, With<GameOverOverlay>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

fn game_over_input(
    keyboard: Res<Input<KeyCode>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if keyboard.just_pressed(KeyCode::Return) {
        next_state.set(GameState::MainMenu);
    }
}

// ============================================================================
// PAUSE MENU
// ============================================================================

fn spawn_pause_menu(
    mut commands: Commands,
    depth_state: Res<DepthState>,
    power_state: Res<PowerState>,
    oxygen_state: Res<OxygenState>,
    hull_state: Res<HullState>,
    module_query: Query<&Module>,
) {
    // Count modules per category and active status
    let mut cat_total: HashMap<ModuleCategory, usize> = HashMap::new();
    let mut cat_active: HashMap<ModuleCategory, usize> = HashMap::new();
    for module in module_query.iter() {
        let cat = module.module_type.category();
        *cat_total.entry(cat).or_insert(0) += 1;
        if module.is_active {
            *cat_active.entry(cat).or_insert(0) += 1;
        }
    }

    info!("Spawning pause menu, modules found: {}", module_query.iter().count());

    commands.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(10.0),
                ..default()
            },
            background_color: Color::rgba(0.0, 0.0, 0.0, 0.85).into(),
            z_index: ZIndex::Global(100),
            ..default()
        },
        PauseMenuOverlay,
    )).with_children(|parent| {
        // Header
        parent.spawn(TextBundle::from_section("== PAUSED ==", TextStyle {
            font_size: 52.0, color: Color::WHITE, ..default()
        }));

        // Vitals line
        let o2_pct = if oxygen_state.max_oxygen > 0.0 {
            (oxygen_state.current_oxygen / oxygen_state.max_oxygen * 100.0) as i32
        } else { 100 };
        let hull_pct = (hull_state.hull_integrity * 100.0) as i32;
        parent.spawn(TextBundle::from_section(
            format!(
                "Depth: {:.0}m  Hull: {}%  O2: {}%  Power: {:.0}/{:.0}",
                depth_state.current_depth, hull_pct, o2_pct,
                power_state.total_power_generation, power_state.total_power_consumption,
            ),
            TextStyle { font_size: 18.0, color: Color::rgb(0.8, 0.8, 0.8), ..default() },
        ));

        // Module counts by category
        for cat in ModuleCategory::ALL {
            let total = cat_total.get(cat).copied().unwrap_or(0);
            if total == 0 { continue; }
            let active = cat_active.get(cat).copied().unwrap_or(0);
            let color = if active == total {
                Color::GREEN
            } else if active > 0 {
                Color::YELLOW
            } else {
                Color::RED
            };
            parent.spawn(TextBundle::from_section(
                format!("  {}: {}/{} active", cat.name(), active, total),
                TextStyle { font_size: 16.0, color, ..default() },
            ));
        }

        // Save/Load section
        parent.spawn(TextBundle::from_section(
            "--- SAVE/LOAD ---",
            TextStyle { font_size: 18.0, color: Color::rgb(0.6, 0.8, 1.0), ..default() },
        ));

        // Show save slot info
        let slots = crate::meta::get_save_slots();
        for (slot, info) in &slots {
            let label = if *slot == 99 {
                "Auto-save".to_string()
            } else {
                format!("Slot {}", slot + 1)
            };

            let status = if let Some(info) = info {
                format!("{}: Depth {:.0}m, {:.0}s played, Hull {:.0}%",
                    label, info.depth, info.play_time, info.hull_integrity * 100.0)
            } else {
                format!("{}: [Empty]", label)
            };

            let key = if *slot == 99 {
                "L+0: Load".to_string()
            } else {
                format!("F{}: Save  |  L+{}: Load", slot + 1, slot + 1)
            };

            parent.spawn(TextBundle::from_section(
                format!("  {} ({})", status, key),
                TextStyle {
                    font_size: 14.0,
                    color: if info.is_some() { Color::rgb(0.7, 0.9, 0.7) } else { Color::GRAY },
                    ..default()
                },
            ));
        }

        // Hint
        parent.spawn(TextBundle::from_section(
            "ESC: Resume | P: Modules | F1-F3: Save | L+1-3: Load",
            TextStyle { font_size: 16.0, color: Color::GRAY, ..default() },
        ));
    });
}

fn despawn_pause_menu(
    mut commands: Commands,
    query: Query<Entity, With<PauseMenuOverlay>>,
    panel_query: Query<Entity, With<ModulePanelOverlay>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
    for entity in panel_query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

// ============================================================================
// SAVE/LOAD INPUT (while paused)
// ============================================================================

/// Handle F1-F3 to save, L+1-3 to load (also L+0 for auto-save)
fn save_load_input(
    keyboard: Res<Input<KeyCode>>,
    mut save_events: EventWriter<SaveGameRequest>,
    mut load_events: EventWriter<LoadGameRequest>,
) {
    let l_held = keyboard.pressed(KeyCode::L);

    // Save: F1, F2, F3
    if !l_held {
        if keyboard.just_pressed(KeyCode::F1) {
            save_events.send(SaveGameRequest { slot: 0 });
        }
        if keyboard.just_pressed(KeyCode::F2) {
            save_events.send(SaveGameRequest { slot: 1 });
        }
        if keyboard.just_pressed(KeyCode::F3) {
            save_events.send(SaveGameRequest { slot: 2 });
        }
    }

    // Load: L+1, L+2, L+3, L+0 (auto-save)
    if l_held {
        if keyboard.just_pressed(KeyCode::Key1) {
            load_events.send(LoadGameRequest { slot: 0 });
        }
        if keyboard.just_pressed(KeyCode::Key2) {
            load_events.send(LoadGameRequest { slot: 1 });
        }
        if keyboard.just_pressed(KeyCode::Key3) {
            load_events.send(LoadGameRequest { slot: 2 });
        }
        if keyboard.just_pressed(KeyCode::Key0) {
            load_events.send(LoadGameRequest { slot: 99 }); // Auto-save slot
        }
    }
}

// ============================================================================
// MODULE MANAGEMENT PANEL (P key while paused)
// ============================================================================

/// Toggles the module management panel on/off with P key
fn toggle_module_panel(
    mut commands: Commands,
    keyboard: Res<Input<KeyCode>>,
    existing_panel: Query<Entity, With<ModulePanelOverlay>>,
    module_query: Query<(Entity, &Module)>,
) {
    if !keyboard.just_pressed(KeyCode::P) {
        return;
    }

    info!("P pressed - toggling module panel");

    // Toggle off if already open
    if let Ok(entity) = existing_panel.get_single() {
        info!("Closing module panel");
        commands.entity(entity).despawn_recursive();
        return;
    }

    // Collect modules grouped by category
    let mut by_cat: HashMap<ModuleCategory, Vec<(Entity, &Module)>> = HashMap::new();
    for (entity, module) in module_query.iter() {
        by_cat.entry(module.module_type.category()).or_default().push((entity, module));
    }

    info!("Opening module panel, {} modules found", module_query.iter().count());

    commands.spawn((
        NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                top: Val::Px(60.0),
                width: Val::Px(400.0),
                max_height: Val::Percent(80.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(4.0),
                ..default()
            },
            background_color: Color::rgba(0.0, 0.05, 0.15, 0.95).into(),
            z_index: ZIndex::Global(110),
            ..default()
        },
        ModulePanelOverlay,
        ModuleListSelection(0),
    )).with_children(|parent| {
        parent.spawn(TextBundle::from_section("MODULE MANAGEMENT", TextStyle {
            font_size: 22.0, color: Color::WHITE, ..default()
        }));

        let mut row_index: usize = 0;
        for cat in ModuleCategory::ALL {
            let Some(modules) = by_cat.get(cat) else { continue };

            // Category header
            parent.spawn(TextBundle::from_section(
                format!("--- {} ---", cat.name()),
                TextStyle { font_size: 16.0, color: Color::YELLOW, ..default() },
            ));

            for &(entity, module) in modules {
                let status = if module.is_active { "[ON] " } else { "[OFF]" };
                let pwr = if module.power_generation > 0.0 {
                    format!("Pwr:+{:.0}", module.power_generation)
                } else if module.power_consumption > 0.0 {
                    format!("Pwr:-{:.0}", module.power_consumption)
                } else {
                    "Pwr:0".to_string()
                };
                let cursor = if row_index == 0 { "> " } else { "  " };
                let text = format!(
                    "{}{} {} - HP:{:.0}/{:.0} {}",
                    cursor, status, module.module_type.name(),
                    module.health, module.max_health, pwr,
                );
                let color = if module.is_active {
                    Color::GREEN
                } else {
                    Color::rgb(0.6, 0.3, 0.3)
                };

                parent.spawn((
                    TextBundle::from_section(&text, TextStyle {
                        font_size: 15.0, color, ..default()
                    }),
                    ModuleListItem(entity),
                ));
                row_index += 1;
            }
        }

        if row_index == 0 {
            parent.spawn(TextBundle::from_section("No modules installed", TextStyle {
                font_size: 16.0, color: Color::GRAY, ..default()
            }));
        }

        parent.spawn(TextBundle::from_section(
            "Up/Down: Select  Enter: Toggle  P: Close",
            TextStyle { font_size: 12.0, color: Color::DARK_GRAY, ..default() },
        ));
    });
}

/// Handles Up/Down/Enter input on the module panel
fn module_panel_input(
    keyboard: Res<Input<KeyCode>>,
    mut panel_query: Query<&mut ModuleListSelection, With<ModulePanelOverlay>>,
    mut item_query: Query<(&ModuleListItem, &mut Text)>,
    mut module_query: Query<&mut Module>,
    mut notifications: EventWriter<ShowNotification>,
) {
    let Ok(mut selection) = panel_query.get_single_mut() else { return };

    let items: Vec<Entity> = item_query.iter().map(|(item, _)| item.0).collect();
    let count = items.len();
    if count == 0 { return; }

    let old_idx = selection.0;
    let mut changed = false;

    if keyboard.just_pressed(KeyCode::Up) {
        selection.0 = if old_idx == 0 { count - 1 } else { old_idx - 1 };
        changed = true;
    }
    if keyboard.just_pressed(KeyCode::Down) {
        selection.0 = if old_idx + 1 >= count { 0 } else { old_idx + 1 };
        changed = true;
    }

    // Toggle is_active on Enter
    if keyboard.just_pressed(KeyCode::Return) {
        let target_entity = items[selection.0];
        if let Ok(mut module) = module_query.get_mut(target_entity) {
            module.is_active = !module.is_active;
            let state_str = if module.is_active { "ON" } else { "OFF" };
            notifications.send(ShowNotification {
                message: format!("{} turned {}", module.module_type.name(), state_str),
                notification_type: NotificationType::Info,
                duration: 2.0,
            });
            changed = true;
        }
    }

    if !changed { return; }

    // Rebuild text for all rows
    let new_idx = selection.0;
    for (i, (item, mut text)) in item_query.iter_mut().enumerate() {
        let Ok(module) = module_query.get(item.0) else { continue };
        let cursor = if i == new_idx { "> " } else { "  " };
        let status = if module.is_active { "[ON] " } else { "[OFF]" };
        let pwr = if module.power_generation > 0.0 {
            format!("Pwr:+{:.0}", module.power_generation)
        } else if module.power_consumption > 0.0 {
            format!("Pwr:-{:.0}", module.power_consumption)
        } else {
            "Pwr:0".to_string()
        };
        text.sections[0].value = format!(
            "{}{} {} - HP:{:.0}/{:.0} {}",
            cursor, status, module.module_type.name(),
            module.health, module.max_health, pwr,
        );
        text.sections[0].style.color = if module.is_active {
            Color::GREEN
        } else {
            Color::rgb(0.6, 0.3, 0.3)
        };
    }
}

// ============================================================================
// DOCKING / TRADING MENU (GameState::Docked)
// ============================================================================

/// Service definitions for the docking menu
struct DockingService {
    name: &'static str,
    description: String,
    cost: u32,
    available: bool,
}

fn get_docking_services(
    hull_state: &HullState,
    oxygen_state: &OxygenState,
    fuel_state: &FuelState,
    weapon_query: &Query<&Weapon, Without<Creature>>,
    crew_count: usize,
    total_berths: u32,
    inventory: &Inventory,
) -> Vec<DockingService> {
    let hull_damage = 1.0 - hull_state.hull_integrity;
    let hull_repair_cost = (hull_damage * 500.0) as u32;

    let o2_missing = oxygen_state.max_oxygen - oxygen_state.current_oxygen;
    let o2_cost = (o2_missing * 2.0) as u32;

    // Count weapons that need ammo
    let mut ammo_needed = 0u32;
    for weapon in weapon_query.iter() {
        if weapon.ammo < weapon.max_ammo {
            ammo_needed += weapon.max_ammo - weapon.ammo;
        }
    }
    let ammo_cost = ammo_needed * 5;

    let hire_cost = 200 + (crew_count as u32) * 50;

    // Sell value: count total sellable items
    let mut sell_value = 0u32;
    for (item_type, count) in &inventory.items {
        let price = match item_type {
            ItemType::ScrapMetal => 10,
            ItemType::Crystal => 25,
            ItemType::BioSample => 15,
            ItemType::FuelCell => 20,
            ItemType::RareAlloy => 50,
            ItemType::AncientArtifact => 100,
            ItemType::AmmoCrate => 30,
        };
        sell_value += price * count;
    }

    let fuel_missing = fuel_state.max_fuel - fuel_state.current_fuel;
    let fuel_cost = (fuel_missing * 0.5) as u32;

    vec![
        DockingService {
            name: "Repair Hull",
            description: format!("Restore hull to 100% (Damage: {:.0}%)", hull_damage * 100.0),
            cost: hull_repair_cost,
            available: hull_damage > 0.01,
        },
        DockingService {
            name: "Refill Oxygen",
            description: format!("Refill O2 tanks ({:.0}/{:.0})", oxygen_state.current_oxygen, oxygen_state.max_oxygen),
            cost: o2_cost,
            available: o2_missing > 1.0,
        },
        DockingService {
            name: "Refuel",
            description: format!("Fill fuel tanks ({:.0}/{:.0}) - FuelCells used first", fuel_state.current_fuel, fuel_state.max_fuel),
            cost: fuel_cost,
            available: fuel_missing > 1.0,
        },
        DockingService {
            name: "Rearm Weapons",
            description: format!("Resupply {} rounds - AmmoCrates used first", ammo_needed),
            cost: ammo_cost,
            available: ammo_needed > 0,
        },
        DockingService {
            name: "Hire Crew",
            description: format!("Recruit crew ({}/{} berths)", crew_count, total_berths),
            cost: hire_cost,
            available: (crew_count as u32) < total_berths,
        },
        DockingService {
            name: "Sell Cargo",
            description: format!("Sell all inventory for {} credits", sell_value),
            cost: 0,
            available: sell_value > 0,
        },
        DockingService {
            name: "Repair Modules",
            description: "Restore all damaged modules to full health".to_string(),
            cost: 0, // Calculated dynamically in the input handler
            available: true, // Checked dynamically
        },
        DockingService {
            name: "Undock",
            description: "Return to exploring".to_string(),
            cost: 0,
            available: true,
        },
    ]
}

fn spawn_docking_menu(
    mut commands: Commands,
    hull_state: Res<HullState>,
    oxygen_state: Res<OxygenState>,
    fuel_state: Res<FuelState>,
    weapon_query: Query<&Weapon, Without<Creature>>,
    crew_query: Query<&CrewMember>,
    inventory: Res<Inventory>,
    currency: Res<Currency>,
    staffing_state: Res<StaffingState>,
) {
    let crew_count = crew_query.iter().count();
    let services = get_docking_services(&hull_state, &oxygen_state, &fuel_state, &weapon_query, crew_count, staffing_state.total_berths, &inventory);

    commands.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                ..default()
            },
            background_color: Color::rgba(0.02, 0.05, 0.12, 0.92).into(),
            z_index: ZIndex::Global(100),
            ..default()
        },
        DockingOverlay,
        DockingMenuSelection(0),
    )).with_children(|parent| {
        parent.spawn(TextBundle::from_section("SETTLEMENT", TextStyle {
            font_size: 48.0, color: Color::rgb(0.4, 0.8, 0.6), ..default()
        }));

        parent.spawn(TextBundle::from_section(
            format!("Credits: {}", currency.credits),
            TextStyle { font_size: 22.0, color: Color::YELLOW, ..default() },
        ));

        parent.spawn(TextBundle::from_section("", TextStyle {
            font_size: 8.0, ..default()
        }));

        for (i, service) in services.iter().enumerate() {
            let cursor = if i == 0 { "> " } else { "  " };
            let cost_str = if service.cost > 0 {
                format!(" [{}c]", service.cost)
            } else {
                String::new()
            };

            let color = if !service.available {
                Color::rgb(0.4, 0.4, 0.4)
            } else if i == 0 {
                Color::WHITE
            } else {
                Color::rgb(0.8, 0.8, 0.8)
            };

            parent.spawn((
                TextBundle::from_sections([
                    TextSection::new(
                        format!("{}{}{}\n", cursor, service.name, cost_str),
                        TextStyle { font_size: 20.0, color, ..default() },
                    ),
                    TextSection::new(
                        format!("    {}", service.description),
                        TextStyle { font_size: 14.0, color: Color::rgb(0.6, 0.6, 0.7), ..default() },
                    ),
                ]),
                DockingServiceItem(i),
            ));
        }

        parent.spawn(TextBundle::from_section("", TextStyle {
            font_size: 8.0, ..default()
        }));

        parent.spawn(TextBundle::from_section(
            "Up/Down: Select | Enter: Purchase | ESC: Undock",
            TextStyle { font_size: 14.0, color: Color::DARK_GRAY, ..default() },
        ));
    });
}

fn despawn_docking_menu(
    mut commands: Commands,
    query: Query<Entity, With<DockingOverlay>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

fn docking_menu_input(
    keyboard: Res<Input<KeyCode>>,
    mut commands: Commands,
    mut menu_query: Query<&mut DockingMenuSelection, With<DockingOverlay>>,
    mut item_query: Query<(&DockingServiceItem, &mut Text)>,
    mut hull_state: ResMut<HullState>,
    mut oxygen_state: ResMut<OxygenState>,
    mut fuel_state: ResMut<FuelState>,
    mut weapon_query: Query<&mut Weapon, Without<Creature>>,
    mut currency: ResMut<Currency>,
    mut inventory: ResMut<Inventory>,
    crew_query: Query<&CrewMember>,
    mut notifications: EventWriter<ShowNotification>,
    mut next_state: ResMut<NextState<GameState>>,
    mut hull_query: Query<&mut HullSegment>,
    staffing_state: Res<StaffingState>,
    mut module_query: Query<&mut Module>,
) {
    let Ok(mut selection) = menu_query.get_single_mut() else { return };

    let service_count = 8usize;
    let old_idx = selection.0;
    let mut changed = false;

    if keyboard.just_pressed(KeyCode::Up) {
        selection.0 = if old_idx == 0 { service_count - 1 } else { old_idx - 1 };
        changed = true;
    }
    if keyboard.just_pressed(KeyCode::Down) {
        selection.0 = if old_idx + 1 >= service_count { 0 } else { old_idx + 1 };
        changed = true;
    }

    if keyboard.just_pressed(KeyCode::Return) {
        let crew_count = crew_query.iter().count();
        let weapon_read_query_hack: Vec<_> = weapon_query.iter().map(|w| (w.ammo, w.max_ammo)).collect();

        match selection.0 {
            0 => {
                // Repair Hull
                let hull_damage = 1.0 - hull_state.hull_integrity;
                let cost = (hull_damage * 500.0) as u32;
                if hull_damage < 0.01 {
                    notifications.send(ShowNotification {
                        message: "Hull already at full integrity".into(),
                        notification_type: NotificationType::Info,
                        duration: 2.0,
                    });
                } else if currency.credits >= cost {
                    currency.credits -= cost;
                    hull_state.hull_integrity = 1.0;
                    // Also repair all hull segments
                    for mut segment in hull_query.iter_mut() {
                        segment.health = segment.max_health;
                        segment.is_flooded = false;
                        segment.flood_level = 0.0;
                    }
                    notifications.send(ShowNotification {
                        message: format!("Hull repaired! (-{}c)", cost),
                        notification_type: NotificationType::Success,
                        duration: 3.0,
                    });
                    changed = true;
                } else {
                    notifications.send(ShowNotification {
                        message: format!("Not enough credits (need {}c, have {}c)", cost, currency.credits),
                        notification_type: NotificationType::Warning,
                        duration: 2.0,
                    });
                }
            }
            1 => {
                // Refill Oxygen
                let o2_missing = oxygen_state.max_oxygen - oxygen_state.current_oxygen;
                let cost = (o2_missing * 2.0) as u32;
                if o2_missing < 1.0 {
                    notifications.send(ShowNotification {
                        message: "Oxygen tanks are full".into(),
                        notification_type: NotificationType::Info,
                        duration: 2.0,
                    });
                } else if currency.credits >= cost {
                    currency.credits -= cost;
                    oxygen_state.current_oxygen = oxygen_state.max_oxygen;
                    notifications.send(ShowNotification {
                        message: format!("Oxygen refilled! (-{}c)", cost),
                        notification_type: NotificationType::Success,
                        duration: 3.0,
                    });
                    changed = true;
                } else {
                    notifications.send(ShowNotification {
                        message: format!("Not enough credits (need {}c, have {}c)", cost, currency.credits),
                        notification_type: NotificationType::Warning,
                        duration: 2.0,
                    });
                }
            }
            2 => {
                // Refuel - first consume FuelCells from inventory (free), then charge for rest
                let fuel_missing = fuel_state.max_fuel - fuel_state.current_fuel;
                if fuel_missing < 1.0 {
                    notifications.send(ShowNotification {
                        message: "Fuel tanks are full".into(),
                        notification_type: NotificationType::Info,
                        duration: 2.0,
                    });
                } else {
                    let mut fuel_added = 0.0f32;
                    // Each FuelCell provides 50 fuel
                    let fuel_cells = inventory.items.get(&ItemType::FuelCell).copied().unwrap_or(0);
                    let cells_needed = ((fuel_missing / 50.0).ceil() as u32).min(fuel_cells);
                    if cells_needed > 0 {
                        let fuel_from_cells = (cells_needed as f32 * 50.0).min(fuel_missing);
                        fuel_state.current_fuel += fuel_from_cells;
                        fuel_added += fuel_from_cells;
                        inventory.remove_item(ItemType::FuelCell, cells_needed);
                        notifications.send(ShowNotification {
                            message: format!("Used {} FuelCells (+{:.0} fuel)", cells_needed, fuel_from_cells),
                            notification_type: NotificationType::Info,
                            duration: 2.0,
                        });
                    }

                    let remaining_missing = fuel_state.max_fuel - fuel_state.current_fuel;
                    if remaining_missing > 1.0 {
                        let cost = (remaining_missing * 0.5) as u32;
                        if currency.credits >= cost {
                            currency.credits -= cost;
                            fuel_state.current_fuel = fuel_state.max_fuel;
                            notifications.send(ShowNotification {
                                message: format!("Fuel tanks refilled! (-{}c)", cost),
                                notification_type: NotificationType::Success,
                                duration: 3.0,
                            });
                        } else {
                            notifications.send(ShowNotification {
                                message: format!("Not enough credits for full refuel (need {}c)", cost),
                                notification_type: NotificationType::Warning,
                                duration: 2.0,
                            });
                        }
                    } else if fuel_added > 0.0 {
                        notifications.send(ShowNotification {
                            message: "Fuel tanks full from FuelCells!".into(),
                            notification_type: NotificationType::Success,
                            duration: 2.0,
                        });
                    }
                    changed = true;
                }
            }
            3 => {
                // Rearm Weapons - AmmoCrates provide 10 rounds each (free), rest costs credits
                let mut ammo_needed = 0u32;
                for &(ammo, max_ammo) in &weapon_read_query_hack {
                    if ammo < max_ammo {
                        ammo_needed += max_ammo - ammo;
                    }
                }
                if ammo_needed == 0 {
                    notifications.send(ShowNotification {
                        message: "All weapons fully loaded".into(),
                        notification_type: NotificationType::Info,
                        duration: 2.0,
                    });
                } else {
                    // Use AmmoCrates first (each crate = 10 rounds)
                    let ammo_crates = inventory.items.get(&ItemType::AmmoCrate).copied().unwrap_or(0);
                    let crates_needed = ((ammo_needed as f32 / 10.0).ceil() as u32).min(ammo_crates);
                    let ammo_from_crates = (crates_needed * 10).min(ammo_needed);
                    if crates_needed > 0 {
                        inventory.remove_item(ItemType::AmmoCrate, crates_needed);
                        notifications.send(ShowNotification {
                            message: format!("Used {} AmmoCrates (+{} rounds)", crates_needed, ammo_from_crates),
                            notification_type: NotificationType::Info,
                            duration: 2.0,
                        });
                    }

                    let remaining_ammo = ammo_needed - ammo_from_crates;
                    let cost = remaining_ammo * 5;
                    if remaining_ammo > 0 && currency.credits < cost {
                        notifications.send(ShowNotification {
                            message: format!("Not enough credits for full rearm (need {}c)", cost),
                            notification_type: NotificationType::Warning,
                            duration: 2.0,
                        });
                    } else {
                        currency.credits -= cost;
                        for mut weapon in weapon_query.iter_mut() {
                            weapon.ammo = weapon.max_ammo;
                        }
                        let msg = if cost > 0 {
                            format!("Weapons rearmed! {} rounds (-{}c)", ammo_needed, cost)
                        } else {
                            format!("Weapons rearmed from AmmoCrates! {} rounds", ammo_needed)
                        };
                        notifications.send(ShowNotification {
                            message: msg,
                            notification_type: NotificationType::Success,
                            duration: 3.0,
                        });
                    }
                    changed = true;
                }
            }
            4 => {
                // Hire Crew — gated by available berths
                let total_berths = staffing_state.total_berths as usize;
                if crew_count >= total_berths {
                    notifications.send(ShowNotification {
                        message: "No available berths! Build more quarters.".into(),
                        notification_type: NotificationType::Warning,
                        duration: 2.0,
                    });
                } else {
                    let cost = 200 + (crew_count as u32) * 50;
                    if currency.credits >= cost {
                        currency.credits -= cost;
                        let crew_names = ["Morgan", "Rivera", "Chen", "Volkov", "Okafor", "Tanaka", "Andersen", "Reyes",
                                          "Park", "Santos", "Becker", "Ito", "Larsen", "Novak", "Gupta", "Patel"];
                        let name = crew_names[crew_count % crew_names.len()].to_string();

                        // Spawn with SpriteBundle; reconcile_hired_crew system
                        // will parent to submarine and add to CrewRoster
                        commands.spawn((
                            SpriteBundle {
                                sprite: Sprite {
                                    color: Color::rgb(0.8, 0.6, 0.5),
                                    custom_size: Some(Vec2::new(16.0, 16.0)),
                                    ..default()
                                },
                                transform: Transform::from_xyz(
                                    (crew_count as f32 - 3.5) * 20.0,
                                    0.0,
                                    0.5,
                                ),
                                ..default()
                            },
                            CrewMember {
                                name: name.clone(),
                                health: 100.0,
                                max_health: 100.0,
                                oxygen: 100.0,
                                morale: 80.0,
                                state: CrewState::Idle,
                            },
                        ));

                        notifications.send(ShowNotification {
                            message: format!("{} joined the crew! (-{}c) ({}/{} berths)",
                                name, cost, crew_count + 1, total_berths),
                            notification_type: NotificationType::Success,
                            duration: 3.0,
                        });
                        changed = true;
                    } else {
                        notifications.send(ShowNotification {
                            message: format!("Not enough credits (need {}c, have {}c)", cost, currency.credits),
                            notification_type: NotificationType::Warning,
                            duration: 2.0,
                        });
                    }
                }
            }
            5 => {
                // Sell Cargo
                let mut total_value = 0u32;
                let mut items_sold = Vec::new();
                for (item_type, count) in &inventory.items {
                    let price = match item_type {
                        ItemType::ScrapMetal => 10,
                        ItemType::Crystal => 25,
                        ItemType::BioSample => 15,
                        ItemType::FuelCell => 20,
                        ItemType::RareAlloy => 50,
                        ItemType::AncientArtifact => 100,
                        ItemType::AmmoCrate => 30,
                    };
                    let value = price * count;
                    total_value += value;
                    items_sold.push((*item_type, *count));
                }

                if total_value == 0 {
                    notifications.send(ShowNotification {
                        message: "No cargo to sell".into(),
                        notification_type: NotificationType::Info,
                        duration: 2.0,
                    });
                } else {
                    currency.credits += total_value;
                    inventory.items.clear();
                    inventory.current_weight = 0.0;
                    notifications.send(ShowNotification {
                        message: format!("Sold all cargo for {}c!", total_value),
                        notification_type: NotificationType::Success,
                        duration: 3.0,
                    });
                    changed = true;
                }
            }
            6 => {
                // Repair Modules
                let mut total_damage = 0.0f32;
                for module in module_query.iter() {
                    if module.health < module.max_health {
                        total_damage += module.max_health - module.health;
                    }
                }
                let cost = (total_damage * 5.0) as u32;
                if total_damage < 0.1 {
                    notifications.send(ShowNotification {
                        message: "All modules at full health".into(),
                        notification_type: NotificationType::Info,
                        duration: 2.0,
                    });
                } else if currency.credits >= cost {
                    currency.credits -= cost;
                    for mut module in module_query.iter_mut() {
                        module.health = module.max_health;
                        if !module.is_active && module.health > 0.0 {
                            module.is_active = true;
                        }
                    }
                    notifications.send(ShowNotification {
                        message: format!("All modules repaired! (-{}c)", cost),
                        notification_type: NotificationType::Success,
                        duration: 3.0,
                    });
                    changed = true;
                } else {
                    notifications.send(ShowNotification {
                        message: format!("Not enough credits (need {}c, have {}c)", cost, currency.credits),
                        notification_type: NotificationType::Warning,
                        duration: 2.0,
                    });
                }
            }
            7 => {
                // Undock
                next_state.set(GameState::Exploring);
                notifications.send(ShowNotification {
                    message: "Undocking...".into(),
                    notification_type: NotificationType::Info,
                    duration: 2.0,
                });
                return;
            }
            _ => {}
        }
    }

    if !changed { return; }

    // Rebuild menu text to reflect updated state
    let crew_count = crew_query.iter().count();
    let weapon_data: Vec<_> = weapon_query.iter().map(|w| (w.ammo, w.max_ammo)).collect();

    let hull_damage = 1.0 - hull_state.hull_integrity;
    let hull_repair_cost = (hull_damage * 500.0) as u32;
    let o2_missing = oxygen_state.max_oxygen - oxygen_state.current_oxygen;
    let o2_cost = (o2_missing * 2.0) as u32;
    let mut ammo_needed = 0u32;
    for &(ammo, max_ammo) in &weapon_data {
        if ammo < max_ammo {
            ammo_needed += max_ammo - ammo;
        }
    }
    let ammo_cost = ammo_needed * 5;
    let hire_cost = 200 + (crew_count as u32) * 50;
    let mut sell_value = 0u32;
    for (item_type, count) in &inventory.items {
        let price = match item_type {
            ItemType::ScrapMetal => 10,
            ItemType::Crystal => 25,
            ItemType::BioSample => 15,
            ItemType::FuelCell => 20,
            ItemType::RareAlloy => 50,
            ItemType::AncientArtifact => 100,
            ItemType::AmmoCrate => 30,
        };
        sell_value += price * count;
    }

    let fuel_missing = fuel_state.max_fuel - fuel_state.current_fuel;
    let fuel_cost = (fuel_missing * 0.5) as u32;

    let new_idx = selection.0;
    let service_info: Vec<(&str, String, u32, bool)> = vec![
        ("Repair Hull", format!("Restore hull to 100% (Damage: {:.0}%)", hull_damage * 100.0), hull_repair_cost, hull_damage > 0.01),
        ("Refill Oxygen", format!("Refill O2 tanks ({:.0}/{:.0})", oxygen_state.current_oxygen, oxygen_state.max_oxygen), o2_cost, o2_missing > 1.0),
        ("Refuel", format!("Fill fuel tanks ({:.0}/{:.0}) - FuelCells used first", fuel_state.current_fuel, fuel_state.max_fuel), fuel_cost, fuel_missing > 1.0),
        ("Rearm Weapons", format!("Resupply {} rounds - AmmoCrates used first", ammo_needed), ammo_cost, ammo_needed > 0),
        ("Hire Crew", format!("Recruit crew ({}/{} berths)", crew_count, staffing_state.total_berths), hire_cost, (crew_count as u32) < staffing_state.total_berths),
        ("Sell Cargo", format!("Sell all inventory for {} credits", sell_value), 0, sell_value > 0),
        ("Repair Modules", {
            let mut total_module_damage = 0.0f32;
            for module in module_query.iter() {
                if module.health < module.max_health {
                    total_module_damage += module.max_health - module.health;
                }
            }
            format!("Restore all modules ({:.0} HP to repair)", total_module_damage)
        }, {
            let mut total_module_damage = 0.0f32;
            for module in module_query.iter() {
                if module.health < module.max_health {
                    total_module_damage += module.max_health - module.health;
                }
            }
            (total_module_damage * 5.0) as u32
        }, module_query.iter().any(|m| m.health < m.max_health)),
        ("Undock", "Return to exploring".to_string(), 0, true),
    ];

    for (item, mut text) in item_query.iter_mut() {
        let idx = item.0;
        if idx >= service_info.len() { continue; }
        let (name, desc, cost, available) = &service_info[idx];

        let cursor = if idx == new_idx { "> " } else { "  " };
        let cost_str = if *cost > 0 { format!(" [{}c]", cost) } else { String::new() };
        let color = if !available {
            Color::rgb(0.4, 0.4, 0.4)
        } else if idx == new_idx {
            Color::WHITE
        } else {
            Color::rgb(0.8, 0.8, 0.8)
        };

        text.sections[0].value = format!("{}{}{}\n", cursor, name, cost_str);
        text.sections[0].style.color = color;
        text.sections[1].value = format!("    {}", desc);
    }
}

// ============================================================================
// UPGRADE SHOP (U key at surface base)
// ============================================================================

struct UpgradeDef {
    name: &'static str,
    cost: u32,
    unlock_category: &'static str, // "hull_types" or "modules"
    unlock_key: &'static str,
    description: &'static str,
}

const UPGRADE_DEFS: &[UpgradeDef] = &[
    UpgradeDef { name: "Titanium Hull", cost: 800, unlock_category: "hull_types", unlock_key: "titanium", description: "Depth rating: 500m" },
    UpgradeDef { name: "Composite Hull", cost: 2000, unlock_category: "hull_types", unlock_key: "composite", description: "Depth rating: 1000m" },
    UpgradeDef { name: "Abyssal Alloy Hull", cost: 5000, unlock_category: "hull_types", unlock_key: "abyssal_alloy", description: "Depth rating: 2500m" },
    UpgradeDef { name: "Advanced Sonar Package", cost: 600, unlock_category: "modules", unlock_key: "advanced_sonar", description: "Unlocks advanced sonar modules" },
    UpgradeDef { name: "Heavy Weapons Package", cost: 1200, unlock_category: "modules", unlock_key: "heavy_weapons", description: "Unlocks heavy weapon modules" },
    UpgradeDef { name: "Silent Drive Technology", cost: 1500, unlock_category: "modules", unlock_key: "silent_drive", description: "Unlocks silent propulsion" },
];

fn is_upgrade_owned(upgrade: &UpgradeDef, unlocks: &Unlocks) -> bool {
    let list = match upgrade.unlock_category {
        "hull_types" => &unlocks.hull_types,
        "modules" => &unlocks.modules,
        _ => &unlocks.upgrades,
    };
    list.contains(&upgrade.unlock_key.to_string())
}

fn toggle_upgrade_shop(
    mut commands: Commands,
    keyboard: Res<Input<KeyCode>>,
    existing: Query<Entity, With<UpgradeShopOverlay>>,
    currency: Res<Currency>,
    unlocks: Res<Unlocks>,
    build_state: Res<State<BuildState>>,
) {
    if !keyboard.just_pressed(KeyCode::U) {
        return;
    }

    // Don't open shop while in build mode
    if *build_state.get() != BuildState::Inactive {
        return;
    }

    // Toggle off if already open
    if let Ok(entity) = existing.get_single() {
        commands.entity(entity).despawn_recursive();
        return;
    }

    commands.spawn((
        NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                ..default()
            },
            background_color: Color::rgba(0.02, 0.05, 0.12, 0.92).into(),
            z_index: ZIndex::Global(100),
            ..default()
        },
        UpgradeShopOverlay,
        UpgradeShopSelection(0),
    )).with_children(|parent| {
        parent.spawn(TextBundle::from_section("UPGRADE SHOP", TextStyle {
            font_size: 48.0, color: Color::rgb(0.4, 0.8, 1.0), ..default()
        }));

        parent.spawn(TextBundle::from_section(
            format!("Credits: {}", currency.credits),
            TextStyle { font_size: 22.0, color: Color::YELLOW, ..default() },
        ));

        parent.spawn(TextBundle::from_section("", TextStyle {
            font_size: 8.0, ..default()
        }));

        for (i, upgrade) in UPGRADE_DEFS.iter().enumerate() {
            let owned = is_upgrade_owned(upgrade, &unlocks);
            let cursor = if i == 0 { "> " } else { "  " };

            let (label, color) = if owned {
                (format!("{}{} [OWNED]", cursor, upgrade.name), Color::rgb(0.4, 0.7, 0.4))
            } else {
                (format!("{}{} [{}c]", cursor, upgrade.name, upgrade.cost),
                 if i == 0 { Color::WHITE } else { Color::rgb(0.8, 0.8, 0.8) })
            };

            parent.spawn((
                TextBundle::from_sections([
                    TextSection::new(
                        format!("{}\n", label),
                        TextStyle { font_size: 20.0, color, ..default() },
                    ),
                    TextSection::new(
                        format!("    {}", upgrade.description),
                        TextStyle { font_size: 14.0, color: Color::rgb(0.6, 0.6, 0.7), ..default() },
                    ),
                ]),
                UpgradeShopItem(i),
            ));
        }

        parent.spawn(TextBundle::from_section("", TextStyle {
            font_size: 8.0, ..default()
        }));

        parent.spawn(TextBundle::from_section(
            "Up/Down: Select | Enter: Purchase | U/ESC: Close",
            TextStyle { font_size: 14.0, color: Color::DARK_GRAY, ..default() },
        ));
    });
}

fn upgrade_shop_input(
    keyboard: Res<Input<KeyCode>>,
    mut commands: Commands,
    mut shop_query: Query<(Entity, &mut UpgradeShopSelection), With<UpgradeShopOverlay>>,
    mut item_query: Query<(&UpgradeShopItem, &mut Text)>,
    mut currency: ResMut<Currency>,
    mut unlocks: ResMut<Unlocks>,
    mut notifications: EventWriter<ShowNotification>,
) {
    let Ok((shop_entity, mut selection)) = shop_query.get_single_mut() else { return };

    // Close on U or ESC
    if keyboard.just_pressed(KeyCode::U) || keyboard.just_pressed(KeyCode::Escape) {
        commands.entity(shop_entity).despawn_recursive();
        return;
    }

    let count = UPGRADE_DEFS.len();
    let old_idx = selection.0;
    let mut changed = false;

    if keyboard.just_pressed(KeyCode::Up) {
        selection.0 = if old_idx == 0 { count - 1 } else { old_idx - 1 };
        changed = true;
    }
    if keyboard.just_pressed(KeyCode::Down) {
        selection.0 = if old_idx + 1 >= count { 0 } else { old_idx + 1 };
        changed = true;
    }

    if keyboard.just_pressed(KeyCode::Return) {
        let upgrade = &UPGRADE_DEFS[selection.0];
        if is_upgrade_owned(upgrade, &unlocks) {
            notifications.send(ShowNotification {
                message: format!("{} already owned!", upgrade.name),
                notification_type: NotificationType::Info,
                duration: 2.0,
            });
        } else if currency.credits >= upgrade.cost {
            currency.credits -= upgrade.cost;
            let list = match upgrade.unlock_category {
                "hull_types" => &mut unlocks.hull_types,
                "modules" => &mut unlocks.modules,
                _ => &mut unlocks.upgrades,
            };
            list.push(upgrade.unlock_key.to_string());
            notifications.send(ShowNotification {
                message: format!("Purchased {}! (-{}c)", upgrade.name, upgrade.cost),
                notification_type: NotificationType::Success,
                duration: 3.0,
            });
            changed = true;
        } else {
            notifications.send(ShowNotification {
                message: format!("Not enough credits (need {}c, have {}c)", upgrade.cost, currency.credits),
                notification_type: NotificationType::Warning,
                duration: 2.0,
            });
        }
    }

    if !changed { return; }

    // Rebuild text
    let new_idx = selection.0;
    for (item, mut text) in item_query.iter_mut() {
        let i = item.0;
        if i >= UPGRADE_DEFS.len() { continue; }
        let upgrade = &UPGRADE_DEFS[i];
        let owned = is_upgrade_owned(upgrade, &unlocks);
        let cursor = if i == new_idx { "> " } else { "  " };

        let (label, color) = if owned {
            (format!("{}{} [OWNED]", cursor, upgrade.name), Color::rgb(0.4, 0.7, 0.4))
        } else {
            (format!("{}{} [{}c]", cursor, upgrade.name, upgrade.cost),
             if i == new_idx { Color::WHITE } else { Color::rgb(0.8, 0.8, 0.8) })
        };

        text.sections[0].value = format!("{}\n", label);
        text.sections[0].style.color = color;
        text.sections[1].value = format!("    {}", upgrade.description);
    }
}

// ============================================================================
// LOW HULL WARNING OVERLAY
// ============================================================================

/// Marker for the hull warning overlay
#[derive(Component)]
struct HullWarningOverlay;

/// Pulses a red overlay when hull integrity drops below 30%
fn update_hull_warning_overlay(
    mut commands: Commands,
    time: Res<Time>,
    hull_state: Res<HullState>,
    mut overlay_query: Query<(Entity, &mut Sprite, &mut Transform), (With<HullWarningOverlay>, Without<MainCamera>)>,
    camera_query: Query<&Transform, (With<MainCamera>, Without<HullWarningOverlay>)>,
) {
    let critical = hull_state.hull_integrity < 0.3;

    if critical {
        let camera_pos = camera_query.iter().next().map(|t| t.translation).unwrap_or(Vec3::ZERO);
        if let Ok((_, mut sprite, mut transform)) = overlay_query.get_single_mut() {
            // Pulse alpha and follow camera
            let alpha = 0.1 + 0.05 * (time.elapsed_seconds() * 6.0).sin();
            sprite.color = Color::rgba(1.0, 0.0, 0.0, alpha);
            transform.translation = Vec3::new(camera_pos.x, camera_pos.y, 10.0);
        } else {
            // Spawn the overlay at camera position
            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color: Color::rgba(1.0, 0.0, 0.0, 0.1),
                        custom_size: Some(Vec2::new(2560.0, 1440.0)),
                        ..default()
                    },
                    transform: Transform::from_xyz(camera_pos.x, camera_pos.y, 10.0),
                    ..default()
                },
                HullWarningOverlay,
            ));
        }
    } else {
        // Despawn if hull is healthy
        for (entity, _, _) in overlay_query.iter() {
            commands.entity(entity).despawn_recursive();
        }
    }
}
