pub mod build_ui;
pub mod damage_overlay;
pub mod windows;
pub mod theme;
pub mod cursor;

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
            .init_resource::<windows::framework::WindowZCounter>()
            .init_resource::<windows::tooltip::TooltipState>()
            .init_resource::<windows::notification_log::NotificationHistory>()
            .init_resource::<PendingWarpTarget>()
            .add_systems(Startup, (setup_ui, cursor::setup_custom_cursor))
            .add_systems(
                Update,
                (
                    cursor::update_custom_cursor,
                    update_hud,
                    update_hud_secondary,
                    update_celestial_hud,
                    handle_notifications,
                    update_notifications,
                    handle_menu_input,
                    // Floating window systems
                    windows::framework::window_drag_system,
                    windows::framework::window_close_system,
                    windows::framework::window_collapse_system,
                    windows::framework::window_button_hover_system,
                    // Tooltip
                    windows::tooltip::tooltip_system,
                    windows::tooltip::tooltip_position_system,
                    // Notification log
                    windows::notification_log::record_notifications,
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
                    // Floating windows (exploring)
                    windows::minimap::toggle_minimap,
                    windows::minimap::update_minimap,
                    windows::notification_log::toggle_notification_log,
                    // Radial menu
                    windows::radial_menu::spawn_radial_on_right_click,
                    windows::radial_menu::update_radial_menu,
                    windows::radial_menu::radial_menu_input,
                ).run_if(in_state(GameState::Exploring)),
            )
            // Inspection & customization — the window is opened while docked/building,
            // so its buttons must stay responsive in both StationDocked and Exploring
            // (it can be left open across the undock transition).
            .add_systems(
                Update,
                (
                    windows::inspection::slot_button_click,
                    windows::inspection::slot_button_hover,
                    windows::inspection::customize_button_hover,
                    windows::inspection::preset_button_click,
                    windows::inspection::preset_button_hover,
                    windows::inspection::custom_preset_button_click,
                    windows::inspection::custom_preset_button_hover,
                    windows::inspection::save_build_button_click,
                    windows::inspection::save_build_button_hover,
                    windows::customization::slider_click_system,
                    windows::customization::undo_button_hover,
                ).run_if(
                    in_state(GameState::Exploring).or_else(in_state(GameState::StationDocked))
                ),
            )
            // Weapon tuning — dock-side workshop only. Windows close on undock.
            .init_resource::<windows::tuning::ActiveSliderDrag>()
            .add_systems(
                Update,
                (
                    windows::tuning::right_click_open_tuning,
                    windows::tuning::tuning_slider_drag,
                    windows::tuning::ammo_button_click,
                    windows::tuning::reset_tuning_click,
                    windows::tuning::tuning_window_refresh,
                ).run_if(in_state(GameState::StationDocked)),
            )
            .add_systems(OnExit(GameState::StationDocked), windows::tuning::despawn_tuning_windows)
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
            // Map-click warp destination + G-hold warp dash (while exploring)
            .add_systems(
                Update,
                (
                    map_click_system,
                    warp_dash_input,
                    execute_warp_dash,
                ).run_if(in_state(GameState::Exploring)),
            )
            // Upgrade shop (at surface base)
            .add_systems(
                Update,
                (
                    toggle_upgrade_shop,
                    upgrade_shop_input,
                ).run_if(in_state(GameState::StationDocked)),
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
                    build_ui::update_controls_help.run_if(
                        in_state(GameState::StationDocked).or_else(in_state(GameState::Exploring))
                    ),
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
                ).run_if(in_state(GameState::StationDocked)),
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
pub struct ThrusterText;

#[derive(Component)]
pub struct AmmoText;

/// Column container in the AMMO HUD slot — holds one AmmoLineText child per weapon.
/// A single Text node with embedded "\n" wasn't a reliable way to show
/// multiple weapons in a fixed-height flex row; separate stacked nodes are.
#[derive(Component)]
pub struct AmmoLinesContainer;

#[derive(Component)]
pub struct AmmoLineText;

#[derive(Component)]
pub struct NoiseText;

#[derive(Component)]
pub struct CreditsText;

/// Dedicated top-right money counter — separate from the CRED entry buried
/// in the crowded top stat bar, so credits are visible at a glance.
#[derive(Component)]
pub struct TopRightCreditsText;

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

/// Marker for star system info display
#[derive(Component)]
pub struct SystemInfoText;

/// Marker for gravity pull indicator
#[derive(Component)]
pub struct GravityIndicatorText;

/// Marker for map/inventory overlay
#[derive(Component)]
pub struct MapOverlay;

/// The clickable world-map square within the overlay — tagged so the click
/// handler can find it and convert cursor position to world coordinates.
#[derive(Component)]
struct MapPanel;

/// Gold crosshair marking the currently selected warp destination.
#[derive(Component)]
struct PendingWarpMarker;

/// World position the player last clicked on the map — persists across
/// opening/closing the map (it's a resource, not tied to the overlay's
/// entities) so the selection sticks until they pick a new one.
#[derive(Resource, Default)]
pub struct PendingWarpTarget(pub Option<Vec2>);

/// On the ship while a long-range warp dash is charging. Target position and
/// fuel cost are locked in at charge-start; hold G to keep charging, release
/// early to cancel.
#[derive(Component)]
pub struct MapWarpCharging {
    pub charge_timer: Timer,
    pub target_pos: Vec2,
    pub fuel_cost: f32,
}

const WARP_DASH_FUEL_PER_1000: f32 = 1.0;
const WARP_DASH_BASE_CHARGE: f32 = 2.0;
const WARP_DASH_DISTANCE_PER_SECOND: f32 = 15_000.0;
/// Stop this far short of the exact clicked point — avoids ever materializing
/// inside whatever's sitting there (a station, a boss hull, etc).
const WARP_DASH_ARRIVAL_BUFFER: f32 = 3000.0;

fn warp_dash_fuel_cost(distance: f32) -> f32 {
    (distance / 1000.0) * WARP_DASH_FUEL_PER_1000
}

fn warp_dash_charge_time(distance: f32) -> f32 {
    WARP_DASH_BASE_CHARGE + distance / WARP_DASH_DISTANCE_PER_SECOND
}

/// Helper to spawn a HUD bar (background + fill)
fn spawn_hud_bar(parent: &mut ChildSpawnerCommands, kind: HudBarKind, width: f32, color: Color) {
    parent.spawn((Node {
            width: Val::Px(width),
            height: Val::Px(4.0),
            ..default()
        }, BackgroundColor(Color::srgba(0.10, 0.12, 0.18, 0.8)))).with_children(|bar_bg| {
        bar_bg.spawn((
            (Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                }, BackgroundColor(color)),
            HudBar { kind },
        ));
    });
}

/// Short per-weapon label for the ammo HUD breakdown — full module names
/// ("Heavy Missile Launcher") don't fit next to a clip count.
fn ammo_hud_abbrev(module_type: ModuleType) -> &'static str {
    match module_type {
        ModuleType::Railgun => "RG",
        ModuleType::Cannon => "CN",
        ModuleType::Coilgun => "CG",
        ModuleType::Gatling => "GT",
        ModuleType::Laser => "LS",
        ModuleType::IonDisruptor => "ION",
        ModuleType::HeavyMissile => "HM",
        ModuleType::GuidedMissile => "GM",
        ModuleType::ClusterRocket => "CR",
        ModuleType::EMPPulse => "EMP",
        _ => "WPN",
    }
}

/// Helper to spawn a HUD group with label — uses theme colors
fn spawn_hud_group(parent: &mut ChildSpawnerCommands, label: &str, label_color: Color, children: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent.spawn((Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(2.0), Val::Px(2.0)),
            row_gap: Val::Px(1.0),
            ..default()
        })).with_children(|group| {
        group.spawn((Text::new(label), TextFont { font_size: FontSize::Px(theme::ThemeFonts::TINY), ..default() }, TextColor(label_color)));
        children(group);
    });
}

/// Helper to spawn a HUD separator
fn spawn_hud_separator(parent: &mut ChildSpawnerCommands) {
    parent.spawn((Node { width: Val::Px(1.0), height: Val::Px(28.0), ..default() }, BackgroundColor(theme::ThemeColors::HUD_SEPARATOR)));
}

/// Sets up the UI — themed, clean layout
fn setup_ui(mut commands: Commands) {
    use theme::*;

    commands.spawn((
        (Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            }),
        HudRoot,
    )).with_children(|parent| {
        // ===== TOP-RIGHT MONEY COUNTER =====
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(ThemeSpacing::SM),
                right: Val::Px(ThemeSpacing::LG),
                padding: UiRect::axes(Val::Px(ThemeSpacing::MD), Val::Px(ThemeSpacing::XS)),
                column_gap: Val::Px(ThemeSpacing::XS),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(ThemeColors::HUD_BG),
        )).with_children(|money| {
            money.spawn((Text::new("¤"), TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() }, TextColor(ThemeColors::ACCENT_YELLOW)));
            money.spawn((
                (Text::new("500"), TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() }, TextColor(ThemeColors::ACCENT_YELLOW)),
                TopRightCreditsText,
            ));
        });

        // ===== TOP BAR — Ship Vitals =====
        parent.spawn((Node {
                width: Val::Percent(100.0),
                padding: UiRect::new(Val::Px(ThemeSpacing::LG), Val::Px(ThemeSpacing::LG), Val::Px(ThemeSpacing::SM), Val::Px(ThemeSpacing::SM)),
                column_gap: Val::Px(ThemeSpacing::XS),
                align_items: AlignItems::Center,
                ..default()
            }, BackgroundColor(ThemeColors::HUD_BG))).with_children(|top_bar| {
            // -- SYSTEM + ZONE --
            spawn_hud_group(top_bar, "SYS", ThemeColors::ACCENT_PURPLE, |group| {
                group.spawn((
                    (Text::new("System-0"), TextFont { font_size: FontSize::Px(ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::ACCENT_PURPLE)),
                    SystemInfoText,
                ));
                group.spawn((
                    (Text::new("Station Orbit"), TextFont { font_size: FontSize::Px(ThemeFonts::TINY), ..default() }, TextColor(ThemeColors::ACCENT_BLUE)),
                    DepthZoneText,
                ));
            });

            spawn_hud_separator(top_bar);

            // -- GRAVITY --
            spawn_hud_group(top_bar, "GRAV", ThemeColors::TEXT_MUTED, |group| {
                group.spawn((
                    (Text::new(""), TextFont { font_size: FontSize::Px(ThemeFonts::CAPTION), ..default() }, TextColor(ThemeColors::TEXT_SECONDARY)),
                    GravityIndicatorText,
                ));
            });

            spawn_hud_separator(top_bar);

            // -- HULL --
            spawn_hud_group(top_bar, "HULL", ThemeColors::ACCENT_GREEN, |group| {
                group.spawn((
                    (Text::new("100%"), TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() }, TextColor(ThemeColors::ACCENT_GREEN)),
                    HullText,
                ));
                spawn_hud_bar(group, HudBarKind::Hull, 56.0, ThemeColors::ACCENT_GREEN);
            });

            // O2 group removed 2026-07-15 — crew oxygen is gone by design
            // (room air/decompression physics remain, but there's no life
            // support stat for the player to watch anymore).

            spawn_hud_separator(top_bar);

            // -- POWER --
            spawn_hud_group(top_bar, "PWR", ThemeColors::ACCENT_YELLOW, |group| {
                group.spawn((
                    (Text::new("0/0"), TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() }, TextColor(ThemeColors::ACCENT_YELLOW)),
                    PowerText,
                ));
            });

            // -- FUEL --
            spawn_hud_group(top_bar, "FUEL", ThemeColors::ACCENT_ORANGE, |group| {
                group.spawn((
                    (Text::new("100%"), TextFont { font_size: FontSize::Px(ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::ACCENT_ORANGE)),
                    FuelText,
                ));
                spawn_hud_bar(group, HudBarKind::Fuel, 44.0, ThemeColors::ACCENT_ORANGE);
            });

            spawn_hud_separator(top_bar);

            // -- THRUSTERS --
            spawn_hud_group(top_bar, "THRS", ThemeColors::ACCENT_BLUE, |group| {
                group.spawn((
                    (Text::new("50%"), TextFont { font_size: FontSize::Px(ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::ACCENT_BLUE)),
                    ThrusterText,
                ));
            });

            // -- AMMO --
            spawn_hud_group(top_bar, "AMMO", ThemeColors::ACCENT_ORANGE, |group| {
                group.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    AmmoLinesContainer,
                ));
            });

            // -- NOISE --
            spawn_hud_group(top_bar, "NOISE", ThemeColors::TEXT_MUTED, |group| {
                group.spawn((
                    (Text::new("0"), TextFont { font_size: FontSize::Px(ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::TEXT_SECONDARY)),
                    NoiseText,
                ));
            });

            spawn_hud_separator(top_bar);

            // -- CREDITS --
            spawn_hud_group(top_bar, "CRED", ThemeColors::ACCENT_YELLOW, |group| {
                group.spawn((
                    (Text::new("500"), TextFont { font_size: FontSize::Px(ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::ACCENT_YELLOW)),
                    CreditsText,
                ));
            });

            // -- CREW --
            spawn_hud_group(top_bar, "CREW", ThemeColors::ACCENT_GREEN, |group| {
                group.spawn((
                    (Text::new("0/0"), TextFont { font_size: FontSize::Px(ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::ACCENT_GREEN)),
                    CrewText,
                ));
            });

            // -- DISTANCE (replaces old DEPTH) --
            spawn_hud_group(top_bar, "DIST", ThemeColors::TEXT_MUTED, |group| {
                group.spawn((
                    (Text::new("0"), TextFont { font_size: FontSize::Px(ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::TEXT_PRIMARY)),
                    DepthText,
                ));
            });
        });

        // ===== NOTIFICATION CONTAINER =====
        parent.spawn((
            (Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(ThemeSpacing::LG),
                    top: Val::Px(48.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(ThemeSpacing::SM),
                    max_width: Val::Px(360.0),
                    ..default()
                }),
            NotificationContainer,
        ));

        // ===== BOTTOM BAR — Controls =====
        parent.spawn((Node {
                width: Val::Percent(100.0),
                height: Val::Px(24.0),
                padding: UiRect::new(Val::Px(ThemeSpacing::XL), Val::Px(ThemeSpacing::XL), Val::Px(ThemeSpacing::SM), Val::Px(ThemeSpacing::SM)),
                align_items: AlignItems::Center,
                ..default()
            }, BackgroundColor(ThemeColors::HUD_BG))).with_children(|bar| {
            bar.spawn((
                (Text::new("WASD Move  Q/E Thrust  SPACE Fire  Z Radar  V Warp  N Map  L Log  B Build  ESC Pause"), TextFont { font_size: FontSize::Px(ThemeFonts::CAPTION), ..default() }, TextColor(ThemeColors::TEXT_MUTED)),
                build_ui::ControlsHelpText,
            ));
        });
    });
}

/// Updates celestial HUD elements: system name, gravity pull, nearest star distance
pub fn update_celestial_hud(
    galaxy: Res<crate::celestial::resources::GalaxyState>,
    ship_query: Query<&Transform, With<Ship>>,
    star_query: Query<(&Transform, &crate::celestial::components::CelestialBody), With<crate::celestial::components::Star>>,
    bh_query: Query<(&Transform, &crate::celestial::components::CelestialBody), With<crate::celestial::components::BlackHole>>,
    gravity_query: Query<&crate::celestial::components::GravityForce, With<Ship>>,
    mut system_text_query: Query<&mut Text, (With<SystemInfoText>, Without<GravityIndicatorText>)>,
    mut gravity_text_query: Query<(&mut Text, &mut TextColor), (With<GravityIndicatorText>, Without<SystemInfoText>)>,
) {
    // System name
    if let Ok(mut text) = system_text_query.single_mut() {
        text.0 = format!("System-{}", galaxy.current_system);
    }

    let ship_pos = ship_query.single()
        .map(|t| t.translation.truncate())
        .unwrap_or(Vec2::ZERO);

    // Gravity indicator
    if let Ok((mut text, mut text_color)) = gravity_text_query.single_mut() {
        let gravity_force = gravity_query.single()
            .map(|gf| gf.0.length())
            .unwrap_or(0.0);

        if gravity_force > 10.0 {
            // Find what's pulling us
            let nearest_star = star_query.iter()
                .map(|(t, body)| (t.translation.truncate().distance(ship_pos), &body.name))
                .min_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

            let nearest_bh = bh_query.iter()
                .map(|(t, body)| (t.translation.truncate().distance(ship_pos), &body.name))
                .min_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

            let source_name = match (nearest_star, nearest_bh) {
                (Some((sd, sn)), Some((bd, bn))) => if bd < sd { bn.as_str() } else { sn.as_str() },
                (Some((_, n)), None) => n.as_str(),
                (None, Some((_, n))) => n.as_str(),
                _ => "Unknown",
            };

            let intensity = if gravity_force > 400.0 {
                "EXTREME"
            } else if gravity_force > 200.0 {
                "Strong"
            } else if gravity_force > 50.0 {
                "Moderate"
            } else {
                "Weak"
            };

            text.0 = format!("Grav: {} ({})", intensity, source_name);
            text_color.0 = if gravity_force > 400.0 {
                Color::srgb(1.0, 0.0, 0.0)
            } else if gravity_force > 200.0 {
                Color::srgb(1.0, 1.0, 0.0)
            } else {
                Color::srgb(0.8, 0.4, 0.3)
            };
        } else {
            text.0 = String::new();
        }
    }
}

/// Returns the space zone name for a given distance
// Thresholds must match world::depth_to_zone (radial distance rings)
fn depth_zone_name(depth: f32) -> &'static str {
    if depth < 600.0 { "Station Orbit" }
    else if depth < 3000.0 { "Near Space" }
    else if depth < 8000.0 { "Asteroid Belt" }
    else if depth < 16000.0 { "Deep Space" }
    else if depth < 30000.0 { "Nebula" }
    else { "Black Hole Proximity" }
}

/// Updates HUD text and bars
pub fn update_hud(
    depth_state: Res<DepthState>,
    power_state: Res<PowerState>,
    hull_state: Res<HullState>,
    time: Res<Time>,
    mut depth_query: Query<(&mut Text, &mut TextColor), (With<DepthText>, Without<PowerText>, Without<OxygenText>, Without<HullText>, Without<DepthZoneText>)>,
    mut depth_zone_query: Query<&mut Text, (With<DepthZoneText>, Without<DepthText>, Without<PowerText>, Without<OxygenText>, Without<HullText>)>,
    mut power_query: Query<(&mut Text, &mut TextColor), (With<PowerText>, Without<DepthText>, Without<OxygenText>, Without<HullText>, Without<DepthZoneText>)>,
    mut hull_query: Query<(&mut Text, &mut TextColor), (With<HullText>, Without<DepthText>, Without<PowerText>, Without<OxygenText>, Without<DepthZoneText>)>,
    mut bar_query: Query<(&HudBar, &mut Node, &mut BackgroundColor)>,
) {
    // Depth
    if let Ok((mut text, mut text_color)) = depth_query.single_mut() {
        text.0 = format!("{:.0}m", depth_state.current_depth);
        text_color.0 = if depth_state.current_depth > 1000.0 {
            Color::srgb(1.0, 0.4, 0.4)
        } else if depth_state.current_depth > 500.0 {
            Color::srgb(0.7, 0.7, 1.0)
        } else {
            Color::WHITE
        };
    }
    if let Ok(mut text) = depth_zone_query.single_mut() {
        text.0 = depth_zone_name(depth_state.current_depth).to_string();
    }

    // Power
    if let Ok((mut text, mut text_color)) = power_query.single_mut() {
        let gen = power_state.total_power_generation;
        let con = power_state.total_power_consumption;
        text.0 = format!("{:.0}/{:.0}", gen, con);
        if power_state.power_balance < 0.0 {
            // Blink red when power deficit
            let blink = (time.elapsed_secs() * 4.0).sin() > 0.0;
            text_color.0 = if blink { Color::srgb(1.0, 0.0, 0.0) } else { Color::srgb(0.6, 0.2, 0.2) };
        } else {
            text_color.0 = Color::srgb(1.0, 1.0, 0.0);
        }
    }

    // Hull
    let hull_pct = hull_state.hull_integrity;
    let hull_pct_i = (hull_pct * 100.0) as i32;
    if let Ok((mut text, mut text_color)) = hull_query.single_mut() {
        text.0 = format!("{}%", hull_pct_i);
        if hull_pct_i < 20 {
            let blink = (time.elapsed_secs() * 5.0).sin() > 0.0;
            text_color.0 = if blink { Color::srgb(1.0, 0.0, 0.0) } else { Color::srgb(0.5, 0.1, 0.1) };
        } else if hull_pct_i < 50 {
            text_color.0 = Color::srgb(1.0, 1.0, 0.0);
        } else {
            text_color.0 = Color::srgb(0.0, 1.0, 0.0);
        }
    }

    // Update HUD bars
    for (bar, mut style, mut bg) in bar_query.iter_mut() {
        let (pct, color) = match bar.kind {
            HudBarKind::Hull => {
                let c = if hull_pct < 0.3 { Color::srgb(1.0, 0.0, 0.0) } else if hull_pct < 0.6 { Color::srgb(1.0, 1.0, 0.0) } else { Color::srgb(0.0, 1.0, 0.0) };
                (hull_pct, c)
            }
            // Oxygen bar removed with crew O2 (its HUD group no longer spawns)
            HudBarKind::Oxygen => continue,
            HudBarKind::Fuel => continue, // handled in update_hud_secondary
        };
        style.width = Val::Percent(pct * 100.0);
        *bg = color.into();
    }
}

/// Updates secondary HUD elements: Fuel, Thrusters, Ammo, Noise, Credits, Crew
pub fn update_hud_secondary(
    fuel_state: Res<FuelState>,
    noise_state: Res<NoiseState>,
    currency: Res<Currency>,
    staffing_state: Res<StaffingState>,
    time: Res<Time>,
    ship_query: Query<Entity, With<Ship>>,
    thruster_query: Query<(&Thruster, &ChildOf)>,
    weapon_query: Query<(&Weapon, &Module, &ChildOf)>,
    mut fuel_query: Query<(&mut Text, &mut TextColor), (With<FuelText>, Without<ThrusterText>, Without<AmmoText>, Without<NoiseText>, Without<CreditsText>, Without<CrewText>)>,
    mut thruster_text_query: Query<(&mut Text, &mut TextColor), (With<ThrusterText>, Without<FuelText>, Without<AmmoText>, Without<NoiseText>, Without<CreditsText>, Without<CrewText>)>,
    mut ammo_ui: (
        Query<(Entity, Option<&Children>), With<AmmoLinesContainer>>,
        Query<&AmmoLineText>,
        Commands,
        Local<Vec<(u32, u32)>>,
    ),
    mut noise_query: Query<(&mut Text, &mut TextColor), (With<NoiseText>, Without<FuelText>, Without<ThrusterText>, Without<AmmoText>, Without<CreditsText>, Without<CrewText>)>,
    mut credits_query: Query<&mut Text, (With<CreditsText>, Without<FuelText>, Without<ThrusterText>, Without<AmmoText>, Without<NoiseText>, Without<CrewText>)>,
    mut top_right_credits_query: Query<&mut Text, (With<TopRightCreditsText>, Without<CreditsText>, Without<FuelText>, Without<ThrusterText>, Without<AmmoText>, Without<NoiseText>, Without<CrewText>)>,
    mut crew_query_hud: Query<(&mut Text, &mut TextColor), (With<CrewText>, Without<FuelText>, Without<ThrusterText>, Without<AmmoText>, Without<NoiseText>, Without<CreditsText>)>,
    mut bar_query: Query<(&HudBar, &mut Node, &mut BackgroundColor)>,
) {
    let (ammo_container_query, ammo_line_query, mut commands, mut last_ammo_snapshot) = ammo_ui;
    let Ok(player_ship) = ship_query.single() else { return };

    // Fuel
    let fuel_pct = if fuel_state.max_fuel > 0.0 {
        fuel_state.current_fuel / fuel_state.max_fuel
    } else { 1.0 };
    let fuel_pct_i = (fuel_pct * 100.0) as i32;
    if let Ok((mut text, mut text_color)) = fuel_query.single_mut() {
        text.0 = format!("{}%", fuel_pct_i);
        if fuel_pct_i < 15 {
            let blink = (time.elapsed_secs() * 4.0).sin() > 0.0;
            text_color.0 = if blink { Color::srgb(1.0, 0.0, 0.0) } else { Color::srgb(0.5, 0.1, 0.1) };
        } else if fuel_pct_i < 30 {
            text_color.0 = Color::srgb(1.0, 1.0, 0.0);
        } else {
            text_color.0 = Color::srgb(1.0, 0.6, 0.2);
        }
    }

    // Update fuel bar
    for (bar, mut style, mut bg) in bar_query.iter_mut() {
        if bar.kind == HudBarKind::Fuel {
            style.width = Val::Percent(fuel_pct * 100.0);
            *bg = if fuel_pct < 0.25 { Color::srgb(1.0, 0.0, 0.0) } else { Color::srgb(1.0, 0.6, 0.2) }.into();
        }
    }

    // Thrusters
    if let Ok((mut text, mut text_color)) = thruster_text_query.single_mut() {
        let outputs: Vec<f32> = thruster_query.iter()
            .filter(|(_, parent)| parent.parent() == player_ship)
            .map(|(t, _)| t.current_output)
            .collect();
        if outputs.is_empty() {
            text.0 = "N/A".to_string();
            text_color.0 = Color::srgb(0.5, 0.5, 0.5);
        } else {
            let avg = outputs.iter().sum::<f32>() / outputs.len() as f32;
            let pct = (avg * 100.0) as i32;
            text.0 = format!("{}%", pct);
            text_color.0 = Color::srgb(0.3, 0.5, 1.0);
        }
    }

    // Ammo — one line per weapon on the player's own ship, not a
    // world-wide total (this used to sum every ship's weapons, player and
    // AI alike, into one misleading number). Rendered as separate stacked
    // UI nodes rather than "\n" inside one Text — a single Text node in
    // this fixed-height top-bar row wasn't reliably showing more than the
    // first line for a 7-weapon loadout.
    if let Ok((container, children)) = ammo_container_query.single() {
        let mut entries: Vec<(ModuleType, u32, u32)> = Vec::new();
        for (weapon, module, parent) in weapon_query.iter() {
            if parent.parent() != player_ship { continue; }
            entries.push((module.module_type, weapon.ammo, weapon.max_ammo));
        }
        let snapshot: Vec<(u32, u32)> = entries.iter().map(|(_, a, m)| (*a, *m)).collect();

        if snapshot != *last_ammo_snapshot {
            *last_ammo_snapshot = snapshot;

            // Clear old lines
            if let Some(children) = children {
                for child in children.iter() {
                    if ammo_line_query.get(child).is_ok() {
                        commands.entity(child).despawn();
                    }
                }
            }

            if entries.is_empty() {
                commands.entity(container).with_children(|c| {
                    c.spawn((
                        (Text::new("N/A"), TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY), ..default() }, TextColor(Color::srgb(0.5, 0.5, 0.5))),
                        AmmoLineText,
                    ));
                });
            } else {
                commands.entity(container).with_children(|c| {
                    for (module_type, ammo, max_ammo) in entries {
                        let pct = if max_ammo > 0 { ammo as f32 / max_ammo as f32 } else { 1.0 };
                        let color = if pct < 0.3 { Color::srgb(1.0, 0.0, 0.0) } else { Color::srgb(0.9, 0.7, 0.3) };
                        c.spawn((
                            (Text::new(format!("{} {}/{}", ammo_hud_abbrev(module_type), ammo, max_ammo)),
                                TextFont { font_size: FontSize::Px(theme::ThemeFonts::TINY), ..default() }, TextColor(color)),
                            AmmoLineText,
                        ));
                    }
                });
            }
        }
    }

    // Noise
    if let Ok((mut text, mut text_color)) = noise_query.single_mut() {
        let noise = noise_state.noise_level as i32;
        text.0 = format!("{}", noise);
        text_color.0 = if noise > 80 {
            Color::srgb(1.0, 0.0, 0.0)
        } else if noise > 50 {
            Color::srgb(1.0, 1.0, 0.0)
        } else {
            Color::srgb(0.5, 0.5, 0.5)
        };
    }

    // Credits
    if let Ok(mut text) = credits_query.single_mut() {
        text.0 = format!("{}", currency.credits);
    }
    if let Ok(mut text) = top_right_credits_query.single_mut() {
        text.0 = format!("{}", currency.credits);
    }

    // Crew staffing
    if let Ok((mut text, mut text_color)) = crew_query_hud.single_mut() {
        text.0 = format!("{}/{}", staffing_state.total_crew, staffing_state.total_berths);
        text_color.0 = if staffing_state.total_crew > staffing_state.total_berths {
            Color::srgb(1.0, 0.0, 0.0)
        } else {
            Color::srgb(0.7, 0.9, 0.7)
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
    mut notification_events: MessageReader<ShowNotification>,
    container_query: Query<Entity, With<NotificationContainer>>,
    existing_toasts: Query<(Entity, &Text), With<NotificationToast>>,
    mut recent_messages: Local<Vec<(String, f32)>>,
    time: Res<Time>,
) {
    let Ok(container) = container_query.single() else { return };

    // Clean up expired dedup entries
    let now = time.elapsed_secs();
    recent_messages.retain(|(_, t)| now - *t < NOTIFICATION_DEDUP_SECS);

    // Count existing toasts
    let mut toast_count = existing_toasts.iter().count();

    for event in notification_events.read() {
        // Skip duplicate messages within the dedup window
        if recent_messages.iter().any(|(msg, _)| msg == &event.message) {
            continue;
        }

        // Cap max visible notifications - remove oldest if at limit
        if toast_count >= MAX_NOTIFICATIONS {
            if let Some((oldest_entity, _)) = existing_toasts.iter().next() {
                commands.entity(oldest_entity).despawn();
                toast_count -= 1;
            }
        }

        let (color, bg_color, prefix) = match event.notification_type {
            NotificationType::Danger => (
                theme::ThemeColors::STATUS_DANGER,
                theme::ThemeColors::NOTIF_DANGER_BG,
                "! ",
            ),
            NotificationType::Warning => (
                theme::ThemeColors::STATUS_WARN,
                theme::ThemeColors::NOTIF_WARN_BG,
                "* ",
            ),
            NotificationType::Success => (
                theme::ThemeColors::ACCENT_GREEN,
                theme::ThemeColors::NOTIF_SUCCESS_BG,
                "+ ",
            ),
            NotificationType::Info => (
                theme::ThemeColors::TEXT_PRIMARY,
                theme::ThemeColors::NOTIF_INFO_BG,
                "",
            ),
        };
        let msg = format!("{}{}", prefix, event.message);
        commands.spawn((
            Text::new(&msg), TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY), ..default() }, TextColor(color), Node { margin: UiRect::bottom(Val::Px(theme::ThemeSpacing::XS)),
                padding: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(5.0), Val::Px(5.0)),
                max_width: Val::Px(340.0),
                ..default() }, BackgroundColor(bg_color),
            NotificationToast { timer: Timer::from_seconds(event.duration, TimerMode::Once) },
        )).insert(ChildOf(container));

        recent_messages.push((event.message.clone(), now));
        toast_count += 1;
    }
}

/// Fades and despawns notification toasts
fn update_notifications(
    mut commands: Commands,
    time: Res<Time>,
    mut toast_query: Query<(Entity, &mut NotificationToast, &mut TextColor)>,
) {
    for (entity, mut toast, mut text_color) in toast_query.iter_mut() {
        toast.timer.tick(time.delta());
        let remaining = toast.timer.remaining_secs() / toast.timer.duration().as_secs_f32();
        if remaining < 0.3 {
            let alpha = remaining / 0.3;
            text_color.0.set_alpha(alpha);
        }
        if toast.timer.is_finished() {
            commands.entity(entity).despawn();
        }
    }
}

/// Handles menu input
fn handle_menu_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    current_state: Res<State<GameState>>,
    build_state: Res<State<BuildState>>,
    customization_state: Res<CustomizationState>,
    mission_board_open: Res<crate::contracts::MissionBoardOpen>,
    mut next_state: ResMut<NextState<GameState>>,
    mut pre_pause: ResMut<PrePauseState>,
    mut load_events: MessageWriter<LoadGameRequest>,
    module_panel: Query<Entity, With<ModulePanelOverlay>>,
    upgrade_shop: Query<Entity, With<UpgradeShopOverlay>>,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        // Don't open pause menu if upgrade shop overlay is open (it handles ESC itself)
        if !upgrade_shop.is_empty() {
            return;
        }
        match current_state.get() {
            GameState::Exploring | GameState::StationDocked => {
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
    if *current_state.get() == GameState::MainMenu && keyboard.pressed(KeyCode::KeyL) {
        if keyboard.just_pressed(KeyCode::Digit1) {
            load_events.write(LoadGameRequest { slot: 0 });
        }
        if keyboard.just_pressed(KeyCode::Digit2) {
            load_events.write(LoadGameRequest { slot: 1 });
        }
        if keyboard.just_pressed(KeyCode::Digit3) {
            load_events.write(LoadGameRequest { slot: 2 });
        }
        if keyboard.just_pressed(KeyCode::Digit0) {
            load_events.write(LoadGameRequest { slot: 99 });
        }
    }

    // Don't process Enter for state transitions while module panel, building,
    // customizing, or the mission board is active — the mission board also
    // binds Enter to "accept contract", and without this guard accepting a
    // contract simultaneously launched the ship out of the station.
    let is_building = *build_state.get() != BuildState::Inactive;
    let is_customizing = customization_state.active;

    if keyboard.just_pressed(KeyCode::Enter)
        && module_panel.is_empty()
        && upgrade_shop.is_empty()
        && !is_building
        && !is_customizing
        && !mission_board_open.0
    {
        match current_state.get() {
            GameState::MainMenu => next_state.set(GameState::StationDocked),
            GameState::StationDocked => next_state.set(GameState::Exploring),
            _ => {}
        }
    }
}

// ============================================================================
// GAME EVENT NOTIFICATIONS
// ============================================================================

/// Reads from currently-silent events and sends ShowNotification
fn handle_game_event_notifications(
    mut power_events: MessageReader<PowerStateChanged>,
    mut oxygen_events: MessageReader<OxygenStateChanged>,
    mut breach_events: MessageReader<HullBreached>,
    mut crew_damage_events: MessageReader<CrewDamaged>,
    crew_query: Query<&CrewMember>,
    weapon_query: Query<&Weapon>,
    mut notifications: MessageWriter<ShowNotification>,
    mut low_ammo_warned: Local<bool>,
) {
    // Power state changes
    for event in power_events.read() {
        if event.is_critical {
            notifications.write(ShowNotification {
                message: "WARNING: Power deficit! Systems failing!".into(),
                notification_type: NotificationType::Danger,
                duration: 4.0,
            });
        } else {
            notifications.write(ShowNotification {
                message: "Power restored. Systems nominal.".into(),
                notification_type: NotificationType::Success,
                duration: 3.0,
            });
        }
    }

    // Hull breaches
    for event in breach_events.read() {
        notifications.write(ShowNotification {
            message: format!("HULL BREACH! Decompression in progress! (Severity: {:.0}%)", event.severity * 100.0),
            notification_type: NotificationType::Danger,
            duration: 4.0,
        });
    }

    // Crew damage
    for event in crew_damage_events.read() {
        if let Ok(crew) = crew_query.get(event.crew) {
            notifications.write(ShowNotification {
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
        notifications.write(ShowNotification {
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
    keyboard: Res<ButtonInput<KeyCode>>,
    existing_menu: Query<Entity, With<CrewMenuOverlay>>,
    crew_query: Query<(Entity, &CrewMember)>,
    station_query: Query<(&CrewStation, &Module)>,
    staffing_state: Res<StaffingState>,
) {
    if !keyboard.just_pressed(KeyCode::KeyC) {
        return;
    }

    // Toggle off if already open
    if let Ok(entity) = existing_menu.single() {
        commands.entity(entity).despawn();
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
        (Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                top: Val::Px(60.0),
                width: Val::Px(380.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(6.0),
                ..default()
            }, BackgroundColor(Color::srgba(0.0, 0.0, 0.1, 0.85))),
        CrewMenuOverlay,
    )).with_children(|parent| {
        parent.spawn((Text::new(format!("CREW MANAGEMENT - {}/{} berths - {}/{} stations",
                staffing_state.total_crew, staffing_state.total_berths,
                staffing_state.staffed_stations, staffing_state.total_stations)), TextFont { font_size: FontSize::Px(20.0), ..default() }, TextColor(Color::WHITE)));

        for (entity, crew) in crew_query.iter() {
            let status = if crew.health <= 0.0 {
                "DEAD".to_string()
            } else if let Some(grid) = crew_assignments.get(&entity) {
                format!("{:?} -> ({},{})", crew.state, grid.x, grid.y)
            } else {
                format!("{:?} (Idle)", crew.state)
            };

            parent.spawn((Text::new(format!("{} | HP:{:.0} Morale:{:.0} | {}",
                    crew.name, crew.health, crew.morale, status)), TextFont { font_size: FontSize::Px(15.0), ..default() }, TextColor(Color::srgb(0.8, 0.8, 0.8))));
        }

        parent.spawn((Text::new("Press C to close"), TextFont { font_size: FontSize::Px(12.0), ..default() }, TextColor(Color::srgb(0.25, 0.25, 0.25))));
    });
}

/// Stub for crew assignment input — press 1 to manually assign idle crew to first unstaffed weapon
fn crew_menu_assign_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    crew_query: Query<(Entity, &CrewMember)>,
    mut station_query: Query<(&mut CrewStation, &Module), With<Weapon>>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    // Press 1 to assign first idle crew to first unstaffed weapon station
    if keyboard.just_pressed(KeyCode::Digit1) {
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
            notifications.write(ShowNotification {
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

/// World-units-to-map-pixels scale. Covers -MAP_WORLD_RANGE..MAP_WORLD_RANGE
/// on each axis, centered on world origin (where the player starts) — big
/// enough to fit every faction territory (25k-175k out) and the first star
/// system (star at ~492k out, planets orbiting another 25k-45k+ beyond it).
const MAP_WORLD_RANGE: f32 = 600_000.0;

/// Converts a world position to a pixel offset within a square map panel of
/// the given size (top-left origin, Y flipped since world +Y is up but UI
/// +Y is down).
fn world_to_map_px(world_pos: Vec2, panel_size: f32) -> (f32, f32) {
    let half = panel_size / 2.0;
    let x = half + (world_pos.x / MAP_WORLD_RANGE) * half;
    let y = half - (world_pos.y / MAP_WORLD_RANGE) * half;
    (x.clamp(0.0, panel_size), y.clamp(0.0, panel_size))
}

/// Bundles the map's world-data queries into one SystemParam — Bevy caps how
/// many parameters a single system function can take (16), and
/// toggle_map_overlay's own params plus these pushed it past that.
#[derive(bevy::ecs::system::SystemParam)]
struct MapWorldData<'w, 's> {
    ai_ship_query: Query<'w, 's, &'static Transform, With<crate::ai_ship::components::AiShip>>,
    sim: Res<'w, crate::ai_ship::components::WorldSimulation>,
    star_query: Query<'w, 's, &'static Transform, With<crate::celestial::components::Star>>,
    planet_query: Query<'w, 's, &'static Transform, With<crate::celestial::components::Planet>>,
    bounty_ship_query: Query<'w, 's, (&'static Transform, &'static crate::ai_ship::components::BountyTarget), With<crate::ai_ship::components::AiShip>>,
    contract_state: Res<'w, crate::contracts::ContractState>,
}

/// Plain-data snapshot of everything the map needs to render — decoupled
/// from SystemParams so both toggle_map_overlay (open) and map_click_system
/// (re-render after picking a destination) can build the exact same UI
/// without duplicating the layout code.
struct MapSnapshot {
    panel_size: f32,
    player_pos: Vec2,
    pending_target: Option<Vec2>,
    current_fuel: f32,
    stars: Vec<Vec2>,
    planets: Vec<Vec2>,
    hostiles: Vec<Vec2>,
    bounties: Vec<Vec2>,
    wrecks_found: usize,
    caves_found: usize,
    settlements_found: usize,
    inventory_items: Vec<(String, u32)>,
    inventory_weight: (f32, f32),
    logs_found: Vec<String>,
}

fn spawn_map_overlay(commands: &mut Commands, snap: &MapSnapshot) {
    let panel_size = snap.panel_size;
    commands.spawn((
        (Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(20.0),
                padding: UiRect::all(Val::Px(20.0)),
                ..default()
            }, BackgroundColor(Color::srgba(0.0, 0.0, 0.05, 0.97)), ZIndex(50)),
        MapOverlay,
    )).with_children(|parent| {
        // Left column: map panel + legend stacked underneath it.
        parent.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            flex_shrink: 0.0,
            ..default()
        }).with_children(|col| {
        // Solar system map: fixed world-anchored frame (not recentered on
        // the player) so position relative to the whole map is legible.
        // Clickable — click anywhere to set a warp destination (see
        // map_click_system); Interaction is what makes Bevy track hover/press
        // state on this node at all.
        col.spawn((
            Node {
                width: Val::Px(panel_size),
                height: Val::Px(panel_size),
                position_type: PositionType::Relative,
                overflow: Overflow::clip(),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.08, 1.0)),
            Interaction::None,
            MapPanel,
        )).with_children(|map| {
            // Star(s)
            for star_pos in &snap.stars {
                let (x, y) = world_to_map_px(*star_pos, panel_size);
                map.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(x - 7.0),
                        top: Val::Px(y - 7.0),
                        width: Val::Px(14.0),
                        height: Val::Px(14.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(1.0, 0.9, 0.4)),
                ));
            }
            // Planets
            for planet_pos in &snap.planets {
                let (x, y) = world_to_map_px(*planet_pos, panel_size);
                map.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(x - 3.5),
                        top: Val::Px(y - 3.5),
                        width: Val::Px(7.0),
                        height: Val::Px(7.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.5, 0.6, 0.8)),
                ));
            }
            // Stations: Haven + resupply outposts
            for station_pos in std::iter::once(crate::world::home_base::STATION_POS)
                .chain(crate::world::home_base::OUTPOST_POSITIONS.iter().copied())
            {
                let (x, y) = world_to_map_px(station_pos, panel_size);
                map.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(x - 4.0),
                        top: Val::Px(y - 4.0),
                        width: Val::Px(8.0),
                        height: Val::Px(8.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.25, 1.0, 0.35)),
                ));
            }
            // Hostiles: real (in render range) + still-off-screen simulated
            for hostile_pos in &snap.hostiles {
                let (x, y) = world_to_map_px(*hostile_pos, panel_size);
                map.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(x - 2.0),
                        top: Val::Px(y - 2.0),
                        width: Val::Px(4.0),
                        height: Val::Px(4.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(1.0, 0.15, 0.15)),
                ));
            }
            // Active bounty targets, highlighted on top of the generic red
            // hostile dot at the same spot — this is specifically "your" hunt.
            for bounty_pos in &snap.bounties {
                let (x, y) = world_to_map_px(*bounty_pos, panel_size);
                map.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(x - 4.0),
                        top: Val::Px(y - 4.0),
                        width: Val::Px(8.0),
                        height: Val::Px(8.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(1.0, 0.2, 0.85)),
                ));
            }

            // Pending warp destination, if one is selected — gold crosshair
            if let Some(target) = snap.pending_target {
                let (x, y) = world_to_map_px(target, panel_size);
                map.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(x - 6.0),
                        top: Val::Px(y - 1.0),
                        width: Val::Px(12.0),
                        height: Val::Px(2.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(1.0, 0.85, 0.1)),
                    PendingWarpMarker,
                ));
                map.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(x - 1.0),
                        top: Val::Px(y - 6.0),
                        width: Val::Px(2.0),
                        height: Val::Px(12.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(1.0, 0.85, 0.1)),
                    PendingWarpMarker,
                ));
            }

            // Player marker on top, slightly bigger so it's easy to find
            let (px, py) = world_to_map_px(snap.player_pos, panel_size);
            map.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(px - 3.5),
                    top: Val::Px(py - 3.5),
                    width: Val::Px(7.0),
                    height: Val::Px(7.0),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.3, 0.9, 1.0)),
            ));
        });

        // Color key legend, directly under the map
        col.spawn(Node {
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(14.0),
            row_gap: Val::Px(4.0),
            width: Val::Px(panel_size),
            ..default()
        }).with_children(|legend| {
            let entries: &[(Color, &str)] = &[
                (Color::srgb(0.3, 0.9, 1.0), "You"),
                (Color::srgb(0.25, 1.0, 0.35), "Station"),
                (Color::srgb(1.0, 0.15, 0.15), "Hostile"),
                (Color::srgb(1.0, 0.2, 0.85), "Bounty target"),
                (Color::srgb(1.0, 0.9, 0.4), "Star"),
                (Color::srgb(0.5, 0.6, 0.8), "Planet"),
                (Color::srgb(1.0, 0.85, 0.1), "Warp target"),
            ];
            for (color, label) in entries {
                legend.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(5.0),
                    ..default()
                }).with_children(|row| {
                    row.spawn((
                        Node { width: Val::Px(9.0), height: Val::Px(9.0), ..default() },
                        BackgroundColor(*color),
                    ));
                    row.spawn((Text::new(*label), TextFont { font_size: FontSize::Px(12.0), ..default() }, TextColor(Color::srgb(0.75, 0.75, 0.8))));
                });
            }
        });
        });

        // Sidebar: inventory / discovered locations / logs
        parent.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                width: Val::Px(300.0),
                height: Val::Percent(90.0),
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(6.0),
                overflow: Overflow::clip_y(),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.1, 0.85)),
        )).with_children(|parent| {
            parent.spawn((Text::new("MAP & INVENTORY"), TextFont { font_size: FontSize::Px(22.0), ..default() }, TextColor(Color::WHITE)));

            // Warp dash: destination + projected cost
            parent.spawn((Text::new("--- Warp Dash ---"), TextFont { font_size: FontSize::Px(18.0), ..default() }, TextColor(Color::srgb(1.0, 0.85, 0.1))));
            if let Some(target) = snap.pending_target {
                let dist = snap.player_pos.distance(target);
                let fuel_cost = warp_dash_fuel_cost((dist - WARP_DASH_ARRIVAL_BUFFER).max(0.0));
                let charge_time = warp_dash_charge_time((dist - WARP_DASH_ARRIVAL_BUFFER).max(0.0));
                let can_afford = snap.current_fuel >= fuel_cost;
                let cost_color = if can_afford { Color::srgb(0.7, 0.9, 1.0) } else { Color::srgb(1.0, 0.4, 0.4) };
                parent.spawn((Text::new(format!("Target: {:.0} units away", dist)), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::WHITE)));
                parent.spawn((Text::new(format!("Cost: {:.0} fuel ({:.0} available)", fuel_cost, snap.current_fuel)), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(cost_color)));
                parent.spawn((Text::new(format!("Charge time: {:.0}s", charge_time)), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::srgb(0.7, 0.9, 1.0))));
                parent.spawn((Text::new("Close map (M), hold G to charge and jump."), TextFont { font_size: FontSize::Px(13.0), ..default() }, TextColor(Color::srgb(0.6, 0.9, 0.6))));
            } else {
                parent.spawn((Text::new("Click the map to set a destination."), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::srgb(0.6, 0.6, 0.6))));
            }

            // Discovered locations
            parent.spawn((Text::new("--- Discovered ---"), TextFont { font_size: FontSize::Px(18.0), ..default() }, TextColor(Color::srgb(0.6, 0.7, 0.9))));
            parent.spawn((Text::new(format!("Wrecks found: {}", snap.wrecks_found)), TextFont { font_size: FontSize::Px(16.0), ..default() }, TextColor(Color::srgb(0.8, 0.6, 0.4))));
            parent.spawn((Text::new(format!("Caves found: {}", snap.caves_found)), TextFont { font_size: FontSize::Px(16.0), ..default() }, TextColor(Color::srgb(0.6, 0.6, 0.6))));
            parent.spawn((Text::new(format!("Settlements: {}", snap.settlements_found)), TextFont { font_size: FontSize::Px(16.0), ..default() }, TextColor(Color::srgb(0.4, 0.8, 0.4))));

            // Inventory
            parent.spawn((Text::new("--- Inventory ---"), TextFont { font_size: FontSize::Px(18.0), ..default() }, TextColor(Color::srgb(1.0, 1.0, 0.0))));

            if snap.inventory_items.is_empty() {
                parent.spawn((Text::new("(empty)"), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::srgb(0.5, 0.5, 0.5))));
            } else {
                for (name, count) in &snap.inventory_items {
                    parent.spawn((Text::new(format!("{}: x{}", name, count)), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::WHITE)));
                }
            }

            parent.spawn((Text::new(format!("Weight: {:.0}/{:.0}", snap.inventory_weight.0, snap.inventory_weight.1)), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::srgb(0.5, 0.5, 0.5))));

            // Logs found
            if !snap.logs_found.is_empty() {
                parent.spawn((Text::new("--- Logs ---"), TextFont { font_size: FontSize::Px(18.0), ..default() }, TextColor(Color::srgb(0.0, 1.0, 1.0))));
                for log in &snap.logs_found {
                    parent.spawn((Text::new(log.clone()), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::srgb(0.7, 0.7, 0.8))));
                }
            }

            parent.spawn((Text::new("Press M to close"), TextFont { font_size: FontSize::Px(12.0), ..default() }, TextColor(Color::srgb(0.25, 0.25, 0.25))));
        });
    });
}

fn build_map_snapshot(
    windows: &Query<&Window>,
    player_pos: Vec2,
    pending_target: Option<Vec2>,
    fuel_state: &FuelState,
    discovered: &DiscoveredLocations,
    inventory: &Inventory,
    statistics: &Statistics,
    world_data: &MapWorldData,
) -> MapSnapshot {
    let (win_w, win_h) = windows.single().map(|w| (w.width(), w.height())).unwrap_or((1280.0, 800.0));
    let panel_size = (win_w.min(win_h) * 0.85).max(200.0);

    let mut hostiles: Vec<Vec2> = world_data.ai_ship_query.iter().map(|t| t.translation.truncate()).collect();
    hostiles.extend(
        world_data.sim.ships.iter()
            .filter(|s| !s.spawned && s.behavior != crate::ai_ship::components::SimBehavior::Dead)
            .map(|s| s.position)
    );

    let bounties: Vec<Vec2> = crate::contracts::bounty_nav::active_bounty_positions_with_id(
        &world_data.contract_state, &world_data.sim, &world_data.bounty_ship_query,
    ).into_iter().map(|(pos, _)| pos).collect();

    MapSnapshot {
        panel_size,
        player_pos,
        pending_target,
        current_fuel: fuel_state.current_fuel,
        stars: world_data.star_query.iter().map(|t| t.translation.truncate()).collect(),
        planets: world_data.planet_query.iter().map(|t| t.translation.truncate()).collect(),
        hostiles,
        bounties,
        wrecks_found: discovered.wrecks.len(),
        caves_found: discovered.caves.len(),
        settlements_found: discovered.settlements.len(),
        inventory_items: inventory.items.iter().map(|(item_type, count)| (item_type.name().to_string(), *count)).collect(),
        inventory_weight: (inventory.current_weight, inventory.max_capacity),
        logs_found: statistics.logs_found.clone(),
    }
}

fn toggle_map_overlay(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    existing: Query<Entity, With<MapOverlay>>,
    discovered: Res<DiscoveredLocations>,
    inventory: Res<Inventory>,
    statistics: Res<Statistics>,
    player_query: Query<&Transform, With<Ship>>,
    world_data: MapWorldData,
    pending: Res<PendingWarpTarget>,
    fuel_state: Res<FuelState>,
    windows: Query<&Window>,
    mut virtual_time: ResMut<Time<Virtual>>,
) {
    if !keyboard.just_pressed(KeyCode::KeyM) {
        return;
    }

    if let Ok(entity) = existing.single() {
        commands.entity(entity).despawn();
        virtual_time.unpause();
        return;
    }

    // Full-screen map pauses the simulation — ships, timers, damage, fuel
    // burn etc. all read Time<Virtual>, so this freezes everything except
    // UI input (which doesn't depend on Time) with no extra state juggling.
    virtual_time.pause();

    let player_pos = player_query.single().map(|t| t.translation.truncate()).unwrap_or(Vec2::ZERO);
    let snapshot = build_map_snapshot(&windows, player_pos, pending.0, &fuel_state, &discovered, &inventory, &statistics, &world_data);
    spawn_map_overlay(&mut commands, &snapshot);
}

/// Handles clicks on the map panel: converts cursor position to world
/// coordinates, sets that as the pending warp destination, and rebuilds the
/// entire overlay so the sidebar's cost/charge-time preview and the
/// crosshair both update in the same frame — patching just the crosshair
/// left the sidebar (the main feedback the player actually looks at) frozen
/// on "click the map to set a destination" forever, which read as "clicking
/// does nothing."
fn map_click_system(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    map_panel: Query<(&ComputedNode, &bevy::ui::UiGlobalTransform), With<MapPanel>>,
    existing: Query<Entity, With<MapOverlay>>,
    windows: Query<&Window>,
    mut pending: ResMut<PendingWarpTarget>,
    player_query: Query<&Transform, With<Ship>>,
    discovered: Res<DiscoveredLocations>,
    inventory: Res<Inventory>,
    statistics: Res<Statistics>,
    world_data: MapWorldData,
    fuel_state: Res<FuelState>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok((node, transform)) = map_panel.single() else { return };
    let Ok(window) = windows.single() else { return };
    // window.cursor_position() is logical pixels; ComputedNode/UiGlobalTransform
    // are physical — on a Retina Mac (2x) that alone silently broke every
    // click. Also: this was querying the classic 2D/3D `GlobalTransform`,
    // which Bevy 0.19 no longer keeps in sync for UI nodes at all (UI now
    // uses its own dedicated `UiGlobalTransform` — see picking_backend.rs)
    // — so panel_center was reading a stale/default value regardless of the
    // pixel-scale bug. Both are fixed by using UiGlobalTransform plus Bevy's
    // own `ComputedNode::normalize_point` hit-test helper instead of manual
    // rectangle math.
    let Some(cursor_pos) = window.cursor_position().map(|p| p * window.scale_factor()) else { return };

    let Some(norm) = node.normalize_point(*transform, cursor_pos) else { return };
    if norm.x.abs() > 0.5 || norm.y.abs() > 0.5 {
        return; // click landed outside the map panel (e.g. on the sidebar)
    }

    let world_x = norm.x * 2.0 * MAP_WORLD_RANGE;
    let world_y = -norm.y * 2.0 * MAP_WORLD_RANGE;
    let target = Vec2::new(world_x, world_y);
    pending.0 = Some(target);

    let player_pos = player_query.single().map(|t| t.translation.truncate()).unwrap_or(Vec2::ZERO);
    let dist = player_pos.distance(target);
    notifications.write(ShowNotification {
        message: format!("Warp target set — {:.0} units away.", dist),
        notification_type: NotificationType::Info,
        duration: 2.5,
    });

    // Full rebuild so the sidebar preview and crosshair are consistent.
    if let Ok(entity) = existing.single() {
        commands.entity(entity).despawn();
    }
    let snapshot = build_map_snapshot(&windows, player_pos, pending.0, &fuel_state, &discovered, &inventory, &statistics, &world_data);
    spawn_map_overlay(&mut commands, &snapshot);
}

/// G: hold to charge a warp dash toward the pending map destination. Release
/// early to cancel. No-ops (silently, via the Without<MapWarpCharging> guard)
/// if nothing's selected — same info notification either way explains why.
fn warp_dash_input(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    ship_query: Query<(Entity, &Transform), (With<Ship>, Without<MapWarpCharging>)>,
    mut charging_query: Query<(Entity, &mut MapWarpCharging), With<Ship>>,
    pending: Res<PendingWarpTarget>,
    fuel_state: Res<FuelState>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    if keyboard.just_pressed(KeyCode::KeyG) {
        let Ok((entity, transform)) = ship_query.single() else { return };
        let ship_pos = transform.translation.truncate();

        let Some(target) = pending.0 else {
            notifications.write(ShowNotification {
                message: "No warp destination set — open the map (M) and click one.".into(),
                notification_type: NotificationType::Warning,
                duration: 3.0,
            });
            return;
        };

        let dist = ship_pos.distance(target);
        if dist < WARP_DASH_ARRIVAL_BUFFER {
            notifications.write(ShowNotification {
                message: "Already at the warp destination.".into(),
                notification_type: NotificationType::Info,
                duration: 2.5,
            });
            return;
        }

        let jump_dist = dist - WARP_DASH_ARRIVAL_BUFFER;
        let fuel_cost = warp_dash_fuel_cost(jump_dist);
        if fuel_state.current_fuel < fuel_cost {
            notifications.write(ShowNotification {
                message: format!("Not enough fuel for the jump ({:.0} needed, {:.0} available).", fuel_cost, fuel_state.current_fuel),
                notification_type: NotificationType::Warning,
                duration: 3.0,
            });
            return;
        }

        let dir = (target - ship_pos).normalize_or_zero();
        let target_pos = target - dir * WARP_DASH_ARRIVAL_BUFFER;
        let charge_time = warp_dash_charge_time(jump_dist);

        commands.entity(entity).insert(MapWarpCharging {
            charge_timer: Timer::from_seconds(charge_time, TimerMode::Once),
            target_pos,
            fuel_cost,
        });

        notifications.write(ShowNotification {
            message: format!("Warp dash charging: {:.0} fuel, {:.0}s — hold G!", fuel_cost, charge_time),
            notification_type: NotificationType::Info,
            duration: charge_time + 1.0,
        });
        return;
    }

    if keyboard.just_released(KeyCode::KeyG) {
        if let Ok((entity, charging)) = charging_query.single() {
            if !charging.charge_timer.is_finished() {
                commands.entity(entity).remove::<MapWarpCharging>();
                notifications.write(ShowNotification {
                    message: "Warp dash cancelled.".into(),
                    notification_type: NotificationType::Info,
                    duration: 2.0,
                });
            }
        }
        return;
    }

    if let Ok((_, mut charging)) = charging_query.single_mut() {
        charging.charge_timer.tick(time.delta());
    }
}

/// Completes the jump once the charge finishes: teleport, kill momentum,
/// spend the fuel locked in at charge-start, clear the destination.
fn execute_warp_dash(
    mut commands: Commands,
    mut ship_query: Query<(Entity, &mut Transform, &mut Velocity, &MapWarpCharging), With<Ship>>,
    mut fuel_state: ResMut<FuelState>,
    mut pending: ResMut<PendingWarpTarget>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let Ok((entity, mut transform, mut velocity, charging)) = ship_query.single_mut() else { return };
    if !charging.charge_timer.is_finished() {
        return;
    }

    transform.translation.x = charging.target_pos.x;
    transform.translation.y = charging.target_pos.y;
    velocity.0 = Vec2::ZERO;
    fuel_state.current_fuel = (fuel_state.current_fuel - charging.fuel_cost).max(0.0);
    pending.0 = None;

    commands.entity(entity).remove::<MapWarpCharging>();

    notifications.write(ShowNotification {
        message: "Warp dash complete.".into(),
        notification_type: NotificationType::Success,
        duration: 4.0,
    });
}

// ============================================================================
// MAIN MENU SCREEN
// ============================================================================

fn spawn_main_menu(mut commands: Commands) {
    use theme::*;

    commands.spawn((
        (Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(ThemeSpacing::SECTION),
                ..default()
            }, BackgroundColor(ThemeColors::BG_VOID), ZIndex(100)),
        MainMenuOverlay,
    )).with_children(|parent| {
        // Title container
        parent.spawn((Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::new(Val::Px(80.0), Val::Px(80.0), Val::Px(ThemeSpacing::XXL), Val::Px(ThemeSpacing::XXL)),
                row_gap: Val::Px(ThemeSpacing::MD),
                ..default()
            })).with_children(|title_box| {
            // Top accent line
            title_box.spawn((Node { width: Val::Px(240.0), height: Val::Px(1.0), margin: UiRect::bottom(Val::Px(ThemeSpacing::LG)), ..default() }, BackgroundColor(ThemeColors::BORDER_BRIGHT)));

            title_box.spawn((Text::new("DEPTHS BELOW"), TextFont { font_size: FontSize::Px(ThemeFonts::DISPLAY), ..default() }, TextColor(ThemeColors::ACCENT_BLUE)));

            title_box.spawn((Text::new("Into the Void"), TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() }, TextColor(ThemeColors::TEXT_SECONDARY)));

            // Bottom accent line
            title_box.spawn((Node { width: Val::Px(240.0), height: Val::Px(1.0), margin: UiRect::top(Val::Px(ThemeSpacing::LG)), ..default() }, BackgroundColor(ThemeColors::BORDER_BRIGHT)));
        });

        // Actions container
        parent.spawn((Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(ThemeSpacing::LG),
                ..default()
            })).with_children(|actions| {
            // New game button
            actions.spawn((Node {
                    padding: UiRect::new(Val::Px(ThemeSpacing::XXL), Val::Px(ThemeSpacing::XXL), Val::Px(ThemeSpacing::MD), Val::Px(ThemeSpacing::MD)),
                    ..default()
                }, BackgroundColor(ThemeColors::BG_ELEVATED))).with_children(|btn| {
                btn.spawn((Text::new("ENTER — New Expedition"), TextFont { font_size: FontSize::Px(ThemeFonts::H2), ..default() }, TextColor(ThemeColors::TEXT_PRIMARY)));
            });

            // Saved games
            let slots = crate::meta::get_save_slots();
            let has_saves = slots.iter().any(|(_, info)| info.is_some());
            if has_saves {
                actions.spawn((Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        padding: UiRect::all(Val::Px(ThemeSpacing::XL)),
                        row_gap: Val::Px(ThemeSpacing::SM),
                        ..default()
                    }, BackgroundColor(ThemeColors::BG_CARD))).with_children(|save_box| {
                    save_box.spawn((Text::new("SAVED EXPEDITIONS"), TextFont { font_size: FontSize::Px(ThemeFonts::CAPTION), ..default() }, TextColor(ThemeColors::TEXT_MUTED)));

                    save_box.spawn((Node { width: Val::Px(180.0), height: Val::Px(1.0), ..default() }, BackgroundColor(ThemeColors::BORDER_SUBTLE)));

                    for (slot, info) in &slots {
                        if let Some(info) = info {
                            let label = if *slot == 99 { "Auto".to_string() } else { format!("Slot {}", slot + 1) };
                            let key = if *slot == 99 { "L+0" } else { match slot { 0 => "L+1", 1 => "L+2", 2 => "L+3", _ => "L+?" } };
                            let time_min = (info.play_time / 60.0) as i32;
                            let time_sec = (info.play_time % 60.0) as i32;
                            save_box.spawn((Text::new(format!("[{}]  {} — {:.0} distance, {}:{:02} played",
                                    key, label, info.depth, time_min, time_sec)), TextFont { font_size: FontSize::Px(ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::ACCENT_GREEN)));
                        }
                    }
                });
            }
        });

        // Tagline
        parent.spawn((Text::new("Build your ship. Explore the void. Survive."), TextFont { font_size: FontSize::Px(ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::TEXT_MUTED)));

        // Version / flavor
        parent.spawn((Text::new("The void remembers those who dare to venture deeper."), TextFont { font_size: FontSize::Px(ThemeFonts::BODY_SMALL), ..default() }, TextColor(Color::srgba(0.25, 0.28, 0.35, 0.6))));
    });
}

fn despawn_main_menu(
    mut commands: Commands,
    query: Query<Entity, With<MainMenuOverlay>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

// ============================================================================
// GAME OVER SCREEN
// ============================================================================

fn spawn_game_over_screen(
    mut commands: Commands,
    statistics: Res<Statistics>,
    victory_state: Res<VictoryState>,
    death_cause: Res<crate::resources::DeathCause>,
) {
    use theme::*;

    let is_victory = victory_state.achieved;

    commands.spawn((
        (Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(ThemeSpacing::XXL),
                ..default()
            }, BackgroundColor(ThemeColors::BG_VOID)),
        GameOverOverlay,
    )).with_children(|parent| {
        // Title
        if is_victory {
            parent.spawn((Text::new("VICTORY"), TextFont { font_size: FontSize::Px(ThemeFonts::DISPLAY), ..default() }, TextColor(ThemeColors::ACCENT_GREEN)));
            parent.spawn((Text::new("You reached the deepest void and uncovered the truth."), TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() }, TextColor(ThemeColors::TEXT_TITLE)));
        } else {
            parent.spawn((Text::new("LOST IN SPACE"), TextFont { font_size: FontSize::Px(ThemeFonts::DISPLAY), ..default() }, TextColor(ThemeColors::ACCENT_RED)));
            parent.spawn((Text::new("The void claims another vessel."), TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() }, TextColor(ThemeColors::TEXT_SECONDARY)));

            // What actually killed the player — the single most useful line
            // on this screen.
            if let Some(cause) = &death_cause.cause {
                parent.spawn((
                    Text::new(cause.clone()),
                    TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() },
                    TextColor(ThemeColors::ACCENT_ORANGE),
                ));
            }
        }

        // Stats panel
        parent.spawn((Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                padding: UiRect::all(Val::Px(ThemeSpacing::XXL)),
                row_gap: Val::Px(ThemeSpacing::MD),
                ..default()
            }, BackgroundColor(ThemeColors::BG_CARD))).with_children(|stats| {
            stats.spawn((Text::new("EXPEDITION LOG"), TextFont { font_size: FontSize::Px(ThemeFonts::CAPTION), ..default() }, TextColor(ThemeColors::TEXT_MUTED)));

            stats.spawn((Node { width: Val::Px(200.0), height: Val::Px(1.0), ..default() }, BackgroundColor(ThemeColors::BORDER_SUBTLE)));

            let time_min = (statistics.play_time_seconds / 60.0) as i32;
            let time_sec = (statistics.play_time_seconds % 60.0) as i32;

            let stat_items = [
                (format!("Max Distance     {:.0}", statistics.max_depth_reached), ThemeColors::ACCENT_BLUE),
                (format!("Time Survived    {}:{:02}", time_min, time_sec), ThemeColors::TEXT_PRIMARY),
                (format!("Creatures Slain  {}", statistics.creatures_killed), ThemeColors::ACCENT_ORANGE),
                (format!("Crew Lost        {}", statistics.crew_lost), ThemeColors::ACCENT_RED),
            ];

            for (text, color) in stat_items {
                stats.spawn((Text::new(text), TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() }, TextColor(color)));
            }

            if !statistics.logs_found.is_empty() {
                stats.spawn((Text::new(format!("Logs Found       {}", statistics.logs_found.len())), TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() }, TextColor(ThemeColors::ACCENT_CYAN)));
            }
        });

        // Return prompt
        parent.spawn((Node {
                padding: UiRect::new(Val::Px(ThemeSpacing::XXL), Val::Px(ThemeSpacing::XXL), Val::Px(ThemeSpacing::MD), Val::Px(ThemeSpacing::MD)),
                ..default()
            }, BackgroundColor(ThemeColors::BG_ELEVATED))).with_children(|btn| {
            btn.spawn((Text::new("ENTER — Return to Station"), TextFont { font_size: FontSize::Px(ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::TEXT_PRIMARY)));
        });
    });
}

fn despawn_game_over_screen(
    mut commands: Commands,
    query: Query<Entity, With<GameOverOverlay>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

fn game_over_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if keyboard.just_pressed(KeyCode::Enter) {
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
        (Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(10.0),
                ..default()
            }, BackgroundColor(theme::ThemeColors::BG_VOID), ZIndex(100)),
        PauseMenuOverlay,
    )).with_children(|parent| {
        // Header
        parent.spawn((Text::new("PAUSED"), TextFont { font_size: FontSize::Px(theme::ThemeFonts::H1), ..default() }, TextColor(theme::ThemeColors::TEXT_TITLE)));

        // Vitals line
        let hull_pct = (hull_state.hull_integrity * 100.0) as i32;
        parent.spawn((Text::new(format!(
                "Distance: {:.0}m  Hull: {}%  Power: {:.0}/{:.0}",
                depth_state.current_depth, hull_pct,
                power_state.total_power_generation, power_state.total_power_consumption,
            )), TextFont { font_size: FontSize::Px(18.0), ..default() }, TextColor(Color::srgb(0.8, 0.8, 0.8))));

        // Module counts by category
        for cat in ModuleCategory::ALL {
            let total = cat_total.get(cat).copied().unwrap_or(0);
            if total == 0 { continue; }
            let active = cat_active.get(cat).copied().unwrap_or(0);
            let color = if active == total {
                Color::srgb(0.0, 1.0, 0.0)
            } else if active > 0 {
                Color::srgb(1.0, 1.0, 0.0)
            } else {
                Color::srgb(1.0, 0.0, 0.0)
            };
            parent.spawn((Text::new(format!("  {}: {}/{} active", cat.name(), active, total)), TextFont { font_size: FontSize::Px(16.0), ..default() }, TextColor(color)));
        }

        // Save/Load section
        parent.spawn((Text::new("--- SAVE/LOAD ---"), TextFont { font_size: FontSize::Px(18.0), ..default() }, TextColor(Color::srgb(0.6, 0.8, 1.0))));

        // Show save slot info
        let slots = crate::meta::get_save_slots();
        for (slot, info) in &slots {
            let label = if *slot == 99 {
                "Auto-save".to_string()
            } else {
                format!("Slot {}", slot + 1)
            };

            let status = if let Some(info) = info {
                format!("{}: Distance {:.0}m, {:.0}s played, Hull {:.0}%",
                    label, info.depth, info.play_time, info.hull_integrity * 100.0)
            } else {
                format!("{}: [Empty]", label)
            };

            let key = if *slot == 99 {
                "L+0: Load".to_string()
            } else {
                format!("F{}: Save  |  L+{}: Load", slot + 1, slot + 1)
            };

            parent.spawn((Text::new(format!("  {} ({})", status, key)), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(if info.is_some() { Color::srgb(0.7, 0.9, 0.7) } else { Color::srgb(0.5, 0.5, 0.5) })));
        }

        // Hint
        parent.spawn((Text::new("ESC: Resume | P: Modules | F1-F3: Save | L+1-3: Load"), TextFont { font_size: FontSize::Px(16.0), ..default() }, TextColor(Color::srgb(0.5, 0.5, 0.5))));
    });
}

fn despawn_pause_menu(
    mut commands: Commands,
    query: Query<Entity, With<PauseMenuOverlay>>,
    panel_query: Query<Entity, With<ModulePanelOverlay>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
    for entity in panel_query.iter() {
        commands.entity(entity).despawn();
    }
}

// ============================================================================
// SAVE/LOAD INPUT (while paused)
// ============================================================================

/// Handle F1-F3 to save, L+1-3 to load (also L+0 for auto-save)
fn save_load_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut save_events: MessageWriter<SaveGameRequest>,
    mut load_events: MessageWriter<LoadGameRequest>,
) {
    let l_held = keyboard.pressed(KeyCode::KeyL);

    // Save: F1, F2, F3
    if !l_held {
        if keyboard.just_pressed(KeyCode::F1) {
            save_events.write(SaveGameRequest { slot: 0 });
        }
        if keyboard.just_pressed(KeyCode::F2) {
            save_events.write(SaveGameRequest { slot: 1 });
        }
        if keyboard.just_pressed(KeyCode::F3) {
            save_events.write(SaveGameRequest { slot: 2 });
        }
    }

    // Load: L+1, L+2, L+3, L+0 (auto-save)
    if l_held {
        if keyboard.just_pressed(KeyCode::Digit1) {
            load_events.write(LoadGameRequest { slot: 0 });
        }
        if keyboard.just_pressed(KeyCode::Digit2) {
            load_events.write(LoadGameRequest { slot: 1 });
        }
        if keyboard.just_pressed(KeyCode::Digit3) {
            load_events.write(LoadGameRequest { slot: 2 });
        }
        if keyboard.just_pressed(KeyCode::Digit0) {
            load_events.write(LoadGameRequest { slot: 99 }); // Auto-save slot
        }
    }
}

// ============================================================================
// MODULE MANAGEMENT PANEL (P key while paused)
// ============================================================================

/// Toggles the module management panel on/off with P key
fn toggle_module_panel(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    existing_panel: Query<Entity, With<ModulePanelOverlay>>,
    module_query: Query<(Entity, &Module)>,
) {
    if !keyboard.just_pressed(KeyCode::KeyP) {
        return;
    }

    info!("P pressed - toggling module panel");

    // Toggle off if already open
    if let Ok(entity) = existing_panel.single() {
        info!("Closing module panel");
        commands.entity(entity).despawn();
        return;
    }

    // Collect modules grouped by category
    let mut by_cat: HashMap<ModuleCategory, Vec<(Entity, &Module)>> = HashMap::new();
    for (entity, module) in module_query.iter() {
        by_cat.entry(module.module_type.category()).or_default().push((entity, module));
    }

    info!("Opening module panel, {} modules found", module_query.iter().count());

    commands.spawn((
        (Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                top: Val::Px(60.0),
                width: Val::Px(400.0),
                max_height: Val::Percent(80.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(4.0),
                ..default()
            }, BackgroundColor(Color::srgba(0.0, 0.05, 0.15, 0.95)), ZIndex(110)),
        ModulePanelOverlay,
        ModuleListSelection(0),
    )).with_children(|parent| {
        parent.spawn((Text::new("MODULE MANAGEMENT"), TextFont { font_size: FontSize::Px(22.0), ..default() }, TextColor(Color::WHITE)));

        let mut row_index: usize = 0;
        for cat in ModuleCategory::ALL {
            let Some(modules) = by_cat.get(cat) else { continue };

            // Category header
            parent.spawn((Text::new(format!("--- {} ---", cat.name())), TextFont { font_size: FontSize::Px(16.0), ..default() }, TextColor(Color::srgb(1.0, 1.0, 0.0))));

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
                    Color::srgb(0.0, 1.0, 0.0)
                } else {
                    Color::srgb(0.6, 0.3, 0.3)
                };

                parent.spawn((
                    (Text::new(&text), TextFont { font_size: FontSize::Px(15.0), ..default() }, TextColor(color)),
                    ModuleListItem(entity),
                ));
                row_index += 1;
            }
        }

        if row_index == 0 {
            parent.spawn((Text::new("No modules installed"), TextFont { font_size: FontSize::Px(16.0), ..default() }, TextColor(Color::srgb(0.5, 0.5, 0.5))));
        }

        parent.spawn((Text::new("Up/Down: Select  Enter: Toggle  P: Close"), TextFont { font_size: FontSize::Px(12.0), ..default() }, TextColor(Color::srgb(0.25, 0.25, 0.25))));
    });
}

/// Handles Up/Down/Enter input on the module panel
fn module_panel_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut panel_query: Query<&mut ModuleListSelection, With<ModulePanelOverlay>>,
    mut item_query: Query<(&ModuleListItem, &mut Text, &mut TextColor)>,
    mut module_query: Query<&mut Module>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let Ok(mut selection) = panel_query.single_mut() else { return };

    let items: Vec<Entity> = item_query.iter().map(|(item, _, _)| item.0).collect();
    let count = items.len();
    if count == 0 { return; }

    let old_idx = selection.0;
    let mut changed = false;

    if keyboard.just_pressed(KeyCode::ArrowUp) {
        selection.0 = if old_idx == 0 { count - 1 } else { old_idx - 1 };
        changed = true;
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) {
        selection.0 = if old_idx + 1 >= count { 0 } else { old_idx + 1 };
        changed = true;
    }

    // Toggle is_active on Enter
    if keyboard.just_pressed(KeyCode::Enter) {
        let target_entity = items[selection.0];
        if let Ok(mut module) = module_query.get_mut(target_entity) {
            module.is_active = !module.is_active;
            let state_str = if module.is_active { "ON" } else { "OFF" };
            notifications.write(ShowNotification {
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
    for (i, (item, mut text, mut text_color)) in item_query.iter_mut().enumerate() {
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
        text.0 = format!(
            "{}{} {} - HP:{:.0}/{:.0} {}",
            cursor, status, module.module_type.name(),
            module.health, module.max_health, pwr,
        );
        text_color.0 = if module.is_active {
            Color::srgb(0.0, 1.0, 0.0)
        } else {
            Color::srgb(0.6, 0.3, 0.3)
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
    station_idx: usize,
) -> Vec<DockingService> {
    let hull_damage = 1.0 - hull_state.hull_integrity;
    let hull_repair_full_cost = (hull_damage * 500.0) as u32;
    let scrap_have = inventory.items.get(&ItemType::ScrapMetal).copied().unwrap_or(0);
    let scrap_usable = (hull_repair_full_cost / 50).min(scrap_have);
    let hull_repair_cost = hull_repair_full_cost.saturating_sub(scrap_usable * 50);

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

    let hire_full_cost = 200 + (crew_count as u32) * 50;
    let bio_have = inventory.items.get(&ItemType::BioSample).copied().unwrap_or(0);
    let bio_usable = (hire_full_cost / 60).min(bio_have);
    let hire_cost = hire_full_cost.saturating_sub(bio_usable * 60);

    // Sell value: count total sellable items at this station's prices
    let mut sell_value = 0u32;
    for (item_type, count) in &inventory.items {
        sell_value += crate::resources::station_item_price(station_idx, *item_type) * count;
    }

    let fuel_missing = fuel_state.max_fuel - fuel_state.current_fuel;
    let fuel_cost = (fuel_missing * 0.5) as u32;

    vec![
        DockingService {
            name: "Repair Hull",
            description: format!("Restore hull to 100% (Damage: {:.0}%) - ScrapMetal used first", hull_damage * 100.0),
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
            description: format!("Recruit crew ({}/{} berths) - BioSample used first", crew_count, total_berths),
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
    ship_query: Query<&Transform, With<Ship>>,
) {
    let crew_count = crew_query.iter().count();
    let station_idx = ship_query.single().ok()
        .and_then(|t| crate::world::home_base::nearest_station_index(t.translation.truncate()))
        .unwrap_or(0);
    let services = get_docking_services(&hull_state, &oxygen_state, &fuel_state, &weapon_query, crew_count, staffing_state.total_berths, &inventory, station_idx);

    commands.spawn((
        (Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                ..default()
            }, BackgroundColor(theme::ThemeColors::BG_VOID), ZIndex(100)),
        DockingOverlay,
        DockingMenuSelection(0),
    )).with_children(|parent| {
        parent.spawn((Text::new("HAVEN STATION — SHIPYARD"), TextFont { font_size: FontSize::Px(theme::ThemeFonts::H1), ..default() }, TextColor(theme::ThemeColors::ACCENT_CYAN)));

        parent.spawn((Text::new(format!("Credits: {}", currency.credits)), TextFont { font_size: FontSize::Px(theme::ThemeFonts::H2), ..default() }, TextColor(theme::ThemeColors::ACCENT_YELLOW)));

        parent.spawn((Text::new(""), TextFont { font_size: FontSize::Px(8.0), ..default() }, TextColor(Color::WHITE)));

        for (i, service) in services.iter().enumerate() {
            let cursor = if i == 0 { "> " } else { "  " };
            let cost_str = if service.cost > 0 {
                format!(" [{}c]", service.cost)
            } else {
                String::new()
            };

            let color = if !service.available {
                Color::srgb(0.4, 0.4, 0.4)
            } else if i == 0 {
                Color::WHITE
            } else {
                Color::srgb(0.8, 0.8, 0.8)
            };

            parent.spawn((
                Text::new(format!("{}{}{}\n", cursor, service.name, cost_str)),
                TextFont { font_size: FontSize::Px(20.0), ..default() },
                TextColor(color),
                DockingServiceItem(i),
            )).with_children(|section| {
                section.spawn((
                    TextSpan::new(format!("    {}", service.description)),
                    TextFont { font_size: FontSize::Px(14.0), ..default() },
                    TextColor(Color::srgb(0.6, 0.6, 0.7)),
                ));
            });
        }

        parent.spawn((Text::new(""), TextFont { font_size: FontSize::Px(8.0), ..default() }, TextColor(Color::WHITE)));

        parent.spawn((Text::new("Up/Down: Select | Enter: Purchase | ESC: Undock"), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::srgb(0.25, 0.25, 0.25))));
    });
}

fn despawn_docking_menu(
    mut commands: Commands,
    query: Query<Entity, With<DockingOverlay>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

fn docking_menu_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut menu_query: Query<&mut DockingMenuSelection, With<DockingOverlay>>,
    mut item_query: Query<(&DockingServiceItem, &mut Text, &mut TextColor, &Children)>,
    mut span_query: Query<&mut TextSpan>,
    econ_state: (ResMut<HullState>, ResMut<OxygenState>, ResMut<FuelState>, ResMut<Currency>, ResMut<Inventory>),
    mut weapon_query: Query<&mut Weapon, Without<Creature>>,
    crew_query: Query<&CrewMember>,
    mut notifications: MessageWriter<ShowNotification>,
    mut next_state: ResMut<NextState<GameState>>,
    mut hull_query: Query<&mut HullSegment>,
    staffing_state: Res<StaffingState>,
    mut module_query: Query<&mut Module>,
    ship_query: Query<&Transform, With<Ship>>,
) {
    let (mut hull_state, mut oxygen_state, mut fuel_state, mut currency, mut inventory) = econ_state;
    let Ok(mut selection) = menu_query.single_mut() else { return };

    let station_idx = ship_query.single().ok()
        .and_then(|t| crate::world::home_base::nearest_station_index(t.translation.truncate()))
        .unwrap_or(0);

    let service_count = 8usize;
    let old_idx = selection.0;
    let mut changed = false;

    if keyboard.just_pressed(KeyCode::ArrowUp) {
        selection.0 = if old_idx == 0 { service_count - 1 } else { old_idx - 1 };
        changed = true;
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) {
        selection.0 = if old_idx + 1 >= service_count { 0 } else { old_idx + 1 };
        changed = true;
    }

    if keyboard.just_pressed(KeyCode::Enter) {
        let crew_count = crew_query.iter().count();
        let weapon_read_query_hack: Vec<_> = weapon_query.iter().map(|w| (w.ammo, w.max_ammo)).collect();

        match selection.0 {
            0 => {
                // Repair Hull — ScrapMetal offsets cost (50c value each)
                // before credits, same pattern as Refuel/Rearm's FuelCell/
                // AmmoCrate offset. Checked atomically (compute scrap+credit
                // split, verify affordable, THEN consume both) rather than
                // spending scrap first — repair is all-or-nothing, unlike
                // fuel/ammo's partial fill, so a failed attempt must not
                // waste resources the player can't get back.
                let hull_damage = 1.0 - hull_state.hull_integrity;
                let full_cost = (hull_damage * 500.0) as u32;
                if hull_damage < 0.01 {
                    notifications.write(ShowNotification {
                        message: "Hull already at full integrity".into(),
                        notification_type: NotificationType::Info,
                        duration: 2.0,
                    });
                } else {
                    const SCRAP_VALUE: u32 = 50;
                    let scrap_have = inventory.items.get(&ItemType::ScrapMetal).copied().unwrap_or(0);
                    let scrap_used = (full_cost / SCRAP_VALUE).min(scrap_have);
                    let cost = full_cost.saturating_sub(scrap_used * SCRAP_VALUE);
                    if currency.credits >= cost {
                        if scrap_used > 0 {
                            inventory.remove_item(ItemType::ScrapMetal, scrap_used);
                        }
                        currency.credits -= cost;
                        hull_state.hull_integrity = 1.0;
                        // Also repair all hull segments
                        for mut segment in hull_query.iter_mut() {
                            segment.health = segment.max_health;
                            segment.is_depressurized = false;
                            segment.depressurization_level = 0.0;
                        }
                        let message = if scrap_used > 0 {
                            format!("Hull repaired! Used {} ScrapMetal (-{}c)", scrap_used, cost)
                        } else {
                            format!("Hull repaired! (-{}c)", cost)
                        };
                        notifications.write(ShowNotification {
                            message,
                            notification_type: NotificationType::Success,
                            duration: 3.0,
                        });
                        changed = true;
                    } else {
                        notifications.write(ShowNotification {
                            message: format!("Not enough credits (need {}c, have {}c)", cost, currency.credits),
                            notification_type: NotificationType::Warning,
                            duration: 2.0,
                        });
                    }
                }
            }
            1 => {
                // Refill Oxygen
                let o2_missing = oxygen_state.max_oxygen - oxygen_state.current_oxygen;
                let cost = (o2_missing * 2.0) as u32;
                if o2_missing < 1.0 {
                    notifications.write(ShowNotification {
                        message: "Oxygen tanks are full".into(),
                        notification_type: NotificationType::Info,
                        duration: 2.0,
                    });
                } else if currency.credits >= cost {
                    currency.credits -= cost;
                    oxygen_state.current_oxygen = oxygen_state.max_oxygen;
                    notifications.write(ShowNotification {
                        message: format!("Oxygen refilled! (-{}c)", cost),
                        notification_type: NotificationType::Success,
                        duration: 3.0,
                    });
                    changed = true;
                } else {
                    notifications.write(ShowNotification {
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
                    notifications.write(ShowNotification {
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
                        notifications.write(ShowNotification {
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
                            notifications.write(ShowNotification {
                                message: format!("Fuel tanks refilled! (-{}c)", cost),
                                notification_type: NotificationType::Success,
                                duration: 3.0,
                            });
                        } else {
                            notifications.write(ShowNotification {
                                message: format!("Not enough credits for full refuel (need {}c)", cost),
                                notification_type: NotificationType::Warning,
                                duration: 2.0,
                            });
                        }
                    } else if fuel_added > 0.0 {
                        notifications.write(ShowNotification {
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
                    notifications.write(ShowNotification {
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
                        notifications.write(ShowNotification {
                            message: format!("Used {} AmmoCrates (+{} rounds)", crates_needed, ammo_from_crates),
                            notification_type: NotificationType::Info,
                            duration: 2.0,
                        });
                    }

                    let remaining_ammo = ammo_needed - ammo_from_crates;
                    let cost = remaining_ammo * 5;
                    if remaining_ammo > 0 && currency.credits < cost {
                        notifications.write(ShowNotification {
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
                        notifications.write(ShowNotification {
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
                    notifications.write(ShowNotification {
                        message: "No available berths! Build more quarters.".into(),
                        notification_type: NotificationType::Warning,
                        duration: 2.0,
                    });
                } else {
                    // BioSample offsets hiring cost first (60c value each —
                    // medical/ration supplies for the new hire) — same
                    // atomic check-then-spend pattern as Repair Hull's
                    // ScrapMetal offset, since hiring is all-or-nothing too.
                    let full_cost = 200 + (crew_count as u32) * 50;
                    const BIOSAMPLE_VALUE: u32 = 60;
                    let bio_have = inventory.items.get(&ItemType::BioSample).copied().unwrap_or(0);
                    let bio_used = (full_cost / BIOSAMPLE_VALUE).min(bio_have);
                    let cost = full_cost.saturating_sub(bio_used * BIOSAMPLE_VALUE);
                    if currency.credits >= cost {
                        if bio_used > 0 {
                            inventory.remove_item(ItemType::BioSample, bio_used);
                        }
                        currency.credits -= cost;
                        let crew_names = ["Morgan", "Rivera", "Chen", "Volkov", "Okafor", "Tanaka", "Andersen", "Reyes",
                                          "Park", "Santos", "Becker", "Ito", "Larsen", "Novak", "Gupta", "Patel"];
                        let name = crew_names[crew_count % crew_names.len()].to_string();

                        // Spawn with SpriteBundle; reconcile_hired_crew system
                        // will parent to ship and add to CrewRoster
                        commands.spawn((
                            (Sprite {
                                    color: Color::srgb(0.8, 0.6, 0.5),
                                    custom_size: Some(Vec2::new(16.0, 16.0)),
                                    ..default()
                                }, Transform::from_xyz(
                                    (crew_count as f32 - 3.5) * 20.0,
                                    0.0,
                                    0.5,
                                )),
                            CrewMember {
                                name: name.clone(),
                                health: 100.0,
                                max_health: 100.0,
                                oxygen: 100.0,
                                morale: 80.0,
                                state: CrewState::Idle,
                            },
                        ));

                        let message = if bio_used > 0 {
                            format!("{} joined the crew! Used {} BioSample (-{}c) ({}/{} berths)",
                                name, bio_used, cost, crew_count + 1, total_berths)
                        } else {
                            format!("{} joined the crew! (-{}c) ({}/{} berths)",
                                name, cost, crew_count + 1, total_berths)
                        };
                        notifications.write(ShowNotification {
                            message,
                            notification_type: NotificationType::Success,
                            duration: 3.0,
                        });
                        changed = true;
                    } else {
                        notifications.write(ShowNotification {
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
                    let price = crate::resources::station_item_price(station_idx, *item_type);
                    let value = price * count;
                    total_value += value;
                    items_sold.push((*item_type, *count));
                }

                if total_value == 0 {
                    notifications.write(ShowNotification {
                        message: "No cargo to sell".into(),
                        notification_type: NotificationType::Info,
                        duration: 2.0,
                    });
                } else {
                    currency.credits += total_value;
                    inventory.items.clear();
                    inventory.current_weight = 0.0;
                    notifications.write(ShowNotification {
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
                    notifications.write(ShowNotification {
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
                    notifications.write(ShowNotification {
                        message: format!("All modules repaired! (-{}c)", cost),
                        notification_type: NotificationType::Success,
                        duration: 3.0,
                    });
                    changed = true;
                } else {
                    notifications.write(ShowNotification {
                        message: format!("Not enough credits (need {}c, have {}c)", cost, currency.credits),
                        notification_type: NotificationType::Warning,
                        duration: 2.0,
                    });
                }
            }
            7 => {
                // Undock
                next_state.set(GameState::Exploring);
                notifications.write(ShowNotification {
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
    let hull_repair_full_cost = (hull_damage * 500.0) as u32;
    let scrap_have = inventory.items.get(&ItemType::ScrapMetal).copied().unwrap_or(0);
    let scrap_usable = (hull_repair_full_cost / 50).min(scrap_have);
    let hull_repair_cost = hull_repair_full_cost.saturating_sub(scrap_usable * 50);
    let o2_missing = oxygen_state.max_oxygen - oxygen_state.current_oxygen;
    let o2_cost = (o2_missing * 2.0) as u32;
    let mut ammo_needed = 0u32;
    for &(ammo, max_ammo) in &weapon_data {
        if ammo < max_ammo {
            ammo_needed += max_ammo - ammo;
        }
    }
    let ammo_cost = ammo_needed * 5;
    let hire_full_cost = 200 + (crew_count as u32) * 50;
    let bio_have = inventory.items.get(&ItemType::BioSample).copied().unwrap_or(0);
    let bio_usable = (hire_full_cost / 60).min(bio_have);
    let hire_cost = hire_full_cost.saturating_sub(bio_usable * 60);
    let mut sell_value = 0u32;
    for (item_type, count) in &inventory.items {
        sell_value += crate::resources::station_item_price(station_idx, *item_type) * count;
    }

    let fuel_missing = fuel_state.max_fuel - fuel_state.current_fuel;
    let fuel_cost = (fuel_missing * 0.5) as u32;

    let new_idx = selection.0;
    let service_info: Vec<(&str, String, u32, bool)> = vec![
        ("Repair Hull", format!("Restore hull to 100% (Damage: {:.0}%) - ScrapMetal used first", hull_damage * 100.0), hull_repair_cost, hull_damage > 0.01),
        ("Refill Oxygen", format!("Refill O2 tanks ({:.0}/{:.0})", oxygen_state.current_oxygen, oxygen_state.max_oxygen), o2_cost, o2_missing > 1.0),
        ("Refuel", format!("Fill fuel tanks ({:.0}/{:.0}) - FuelCells used first", fuel_state.current_fuel, fuel_state.max_fuel), fuel_cost, fuel_missing > 1.0),
        ("Rearm Weapons", format!("Resupply {} rounds - AmmoCrates used first", ammo_needed), ammo_cost, ammo_needed > 0),
        ("Hire Crew", format!("Recruit crew ({}/{} berths) - BioSample used first", crew_count, staffing_state.total_berths), hire_cost, (crew_count as u32) < staffing_state.total_berths),
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

    for (item, mut text, mut text_color, children) in item_query.iter_mut() {
        let idx = item.0;
        if idx >= service_info.len() { continue; }
        let (name, desc, cost, available) = &service_info[idx];

        let cursor = if idx == new_idx { "> " } else { "  " };
        let cost_str = if *cost > 0 { format!(" [{}c]", cost) } else { String::new() };
        let color = if !available {
            Color::srgb(0.4, 0.4, 0.4)
        } else if idx == new_idx {
            Color::WHITE
        } else {
            Color::srgb(0.8, 0.8, 0.8)
        };

        text.0 = format!("{}{}{}\n", cursor, name, cost_str);
        text_color.0 = color;
        for child in children.iter() {
            if let Ok(mut span) = span_query.get_mut(child) {
                span.0 = format!("    {}", desc);
            }
        }
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
    UpgradeDef { name: "Titanium Hull", cost: 800, unlock_category: "hull_types", unlock_key: "titanium", description: "+50% hull strength" },
    UpgradeDef { name: "Composite Hull", cost: 2000, unlock_category: "hull_types", unlock_key: "composite", description: "+100% hull strength" },
    UpgradeDef { name: "Abyssal Alloy Hull", cost: 5000, unlock_category: "hull_types", unlock_key: "abyssal_alloy", description: "+200% hull strength" },
    UpgradeDef { name: "Advanced Radar Package", cost: 600, unlock_category: "modules", unlock_key: "advanced_radar", description: "Unlocks advanced radar modules" },
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
    keyboard: Res<ButtonInput<KeyCode>>,
    existing: Query<Entity, With<UpgradeShopOverlay>>,
    currency: Res<Currency>,
    unlocks: Res<Unlocks>,
    build_state: Res<State<BuildState>>,
) {
    if !keyboard.just_pressed(KeyCode::KeyU) {
        return;
    }

    // Don't open shop while in build mode
    if *build_state.get() != BuildState::Inactive {
        return;
    }

    // Toggle off if already open
    if let Ok(entity) = existing.single() {
        commands.entity(entity).despawn();
        return;
    }

    commands.spawn((
        (Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                ..default()
            }, BackgroundColor(Color::srgba(0.02, 0.05, 0.12, 0.92)), ZIndex(100)),
        UpgradeShopOverlay,
        UpgradeShopSelection(0),
    )).with_children(|parent| {
        parent.spawn((Text::new("UPGRADE SHOP"), TextFont { font_size: FontSize::Px(48.0), ..default() }, TextColor(Color::srgb(0.4, 0.8, 1.0))));

        parent.spawn((Text::new(format!("Credits: {}", currency.credits)), TextFont { font_size: FontSize::Px(22.0), ..default() }, TextColor(Color::srgb(1.0, 1.0, 0.0))));

        parent.spawn((Text::new(""), TextFont { font_size: FontSize::Px(8.0), ..default() }, TextColor(Color::WHITE)));

        for (i, upgrade) in UPGRADE_DEFS.iter().enumerate() {
            let owned = is_upgrade_owned(upgrade, &unlocks);
            let cursor = if i == 0 { "> " } else { "  " };

            let (label, color) = if owned {
                (format!("{}{} [OWNED]", cursor, upgrade.name), Color::srgb(0.4, 0.7, 0.4))
            } else {
                (format!("{}{} [{}c]", cursor, upgrade.name, upgrade.cost),
                 if i == 0 { Color::WHITE } else { Color::srgb(0.8, 0.8, 0.8) })
            };

            parent.spawn((
                Text::new(format!("{}\n", label)),
                TextFont { font_size: FontSize::Px(20.0), ..default() },
                TextColor(color),
                UpgradeShopItem(i),
            )).with_children(|section| {
                section.spawn((
                    TextSpan::new(format!("    {}", upgrade.description)),
                    TextFont { font_size: FontSize::Px(14.0), ..default() },
                    TextColor(Color::srgb(0.6, 0.6, 0.7)),
                ));
            });
        }

        parent.spawn((Text::new(""), TextFont { font_size: FontSize::Px(8.0), ..default() }, TextColor(Color::WHITE)));

        parent.spawn((Text::new("Up/Down: Select | Enter: Purchase | U/ESC: Close"), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::srgb(0.25, 0.25, 0.25))));
    });
}

fn upgrade_shop_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut shop_query: Query<(Entity, &mut UpgradeShopSelection), With<UpgradeShopOverlay>>,
    mut item_query: Query<(&UpgradeShopItem, &mut Text, &mut TextColor, &Children)>,
    mut span_query: Query<&mut TextSpan>,
    mut currency: ResMut<Currency>,
    mut unlocks: ResMut<Unlocks>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let Ok((shop_entity, mut selection)) = shop_query.single_mut() else { return };

    // Close on U or ESC
    if keyboard.just_pressed(KeyCode::KeyU) || keyboard.just_pressed(KeyCode::Escape) {
        commands.entity(shop_entity).despawn();
        return;
    }

    let count = UPGRADE_DEFS.len();
    let old_idx = selection.0;
    let mut changed = false;

    if keyboard.just_pressed(KeyCode::ArrowUp) {
        selection.0 = if old_idx == 0 { count - 1 } else { old_idx - 1 };
        changed = true;
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) {
        selection.0 = if old_idx + 1 >= count { 0 } else { old_idx + 1 };
        changed = true;
    }

    if keyboard.just_pressed(KeyCode::Enter) {
        let upgrade = &UPGRADE_DEFS[selection.0];
        if is_upgrade_owned(upgrade, &unlocks) {
            notifications.write(ShowNotification {
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
            notifications.write(ShowNotification {
                message: format!("Purchased {}! (-{}c)", upgrade.name, upgrade.cost),
                notification_type: NotificationType::Success,
                duration: 3.0,
            });
            changed = true;
        } else {
            notifications.write(ShowNotification {
                message: format!("Not enough credits (need {}c, have {}c)", upgrade.cost, currency.credits),
                notification_type: NotificationType::Warning,
                duration: 2.0,
            });
        }
    }

    if !changed { return; }

    // Rebuild text
    let new_idx = selection.0;
    for (item, mut text, mut text_color, children) in item_query.iter_mut() {
        let i = item.0;
        if i >= UPGRADE_DEFS.len() { continue; }
        let upgrade = &UPGRADE_DEFS[i];
        let owned = is_upgrade_owned(upgrade, &unlocks);
        let cursor = if i == new_idx { "> " } else { "  " };

        let (label, color) = if owned {
            (format!("{}{} [OWNED]", cursor, upgrade.name), Color::srgb(0.4, 0.7, 0.4))
        } else {
            (format!("{}{} [{}c]", cursor, upgrade.name, upgrade.cost),
             if i == new_idx { Color::WHITE } else { Color::srgb(0.8, 0.8, 0.8) })
        };

        text.0 = format!("{}\n", label);
        text_color.0 = color;
        for child in children.iter() {
            if let Ok(mut span) = span_query.get_mut(child) {
                span.0 = format!("    {}", upgrade.description);
            }
        }
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
        if let Ok((_, mut sprite, mut transform)) = overlay_query.single_mut() {
            // Pulse alpha and follow camera
            let alpha = 0.1 + 0.05 * (time.elapsed_secs() * 6.0).sin();
            sprite.color = Color::srgba(1.0, 0.0, 0.0, alpha);
            transform.translation = Vec3::new(camera_pos.x, camera_pos.y, 10.0);
        } else {
            // Spawn the overlay at camera position
            commands.spawn((
                (Sprite {
                        color: Color::srgba(1.0, 0.0, 0.0, 0.1),
                        custom_size: Some(Vec2::new(2560.0, 1440.0)),
                        ..default()
                    }, Transform::from_xyz(camera_pos.x, camera_pos.y, 10.0)),
                HullWarningOverlay,
            ));
        }
    } else {
        // Despawn if hull is healthy
        for (entity, _, _) in overlay_query.iter() {
            commands.entity(entity).despawn();
        }
    }
}
