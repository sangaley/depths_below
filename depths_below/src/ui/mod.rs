pub mod build_ui;
pub mod damage_overlay;
pub mod windows;
pub mod theme;
pub mod cursor;
pub mod menu_buttons;

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use bevy::input::InputSystems;
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
            .init_resource::<windows::power_routing::PowerSliderDrag>()
            .init_resource::<windows::tooltip::TooltipState>()
            .init_resource::<windows::notification_log::NotificationHistory>()
            .init_resource::<PendingWarpTarget>()
            .init_resource::<MapViewMode>()
            .init_resource::<menu_buttons::GameSettings>()
            .init_resource::<menu_buttons::SettingsMenu>()
            // UiScale is normally inserted by Bevy's UiPlugin; init defensively
            // so apply_display_settings can always drive it.
            .init_resource::<bevy::ui::UiScale>()
            // Load persisted settings before the apply systems' first run.
            .add_systems(Startup, (setup_ui, cursor::setup_custom_cursor, menu_buttons::load_settings))
            // Clickable menu buttons + Settings overlay (main/pause/game-over).
            // These run in every state — the queries are empty unless a menu
            // is on screen, so there's no per-frame cost during play.
            .add_systems(
                Update,
                (
                    menu_buttons::menu_button_visuals,
                    menu_buttons::menu_button_dispatch,
                    menu_buttons::apply_audio_settings,
                    menu_buttons::apply_display_settings,
                    menu_buttons::save_settings,
                    menu_buttons::manage_settings_overlay,
                    menu_buttons::update_settings_values,
                ),
            )
            // Never let the Settings overlay leak into gameplay.
            .add_systems(OnExit(GameState::MainMenu), menu_buttons::close_settings_on_exit)
            .add_systems(OnExit(GameState::Paused), menu_buttons::close_settings_on_exit)
            // HUD toolbar: synthesize the key while a button is held (PreUpdate
            // after InputSystems, like the gamepad bridge); recolor on hover.
            .add_systems(PreUpdate, hud_action_button_press.after(InputSystems).run_if(in_state(GameState::Exploring)))
            .add_systems(Update, (
                hud_action_button_hover,
                toggle_flight_toolbar_visibility,
                weapon_rack_visibility,
                update_weapon_rack.run_if(in_state(GameState::Exploring)),
            ))
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
            // Quick-save (F5) / quick-load (F9) during normal play — no menu needed
            .add_systems(
                Update,
                quick_save_load_input.run_if(
                    in_state(GameState::Exploring)
                        .or_else(in_state(GameState::StationDocked))
                        .or_else(in_state(GameState::Paused)),
                ),
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
                    // Power routing window (Weapons/Shields/Engines)
                    windows::power_routing::toggle_power_window,
                    windows::power_routing::power_slider_drag,
                    windows::power_routing::power_preset_click,
                    windows::power_routing::power_button_hover,
                    windows::power_routing::power_window_refresh,
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
            // Cargo hold panel — Haven has no trade menu (that's remote
            // outposts, GameState::Docked) and the M inventory overlay is
            // flying-only, so the itemized hold was unreadable at the home
            // station. This shows it top-left while docked/building.
            .add_systems(OnEnter(GameState::StationDocked), spawn_station_cargo_panel)
            .add_systems(OnExit(GameState::StationDocked), despawn_station_cargo_panel)
            .add_systems(
                Update,
                update_station_cargo_panel.run_if(in_state(GameState::StationDocked)),
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
                    toggle_galaxy_view,
                    crew_duty_click,
                    refresh_crew_duty_labels,
                ).run_if(in_state(GameState::Exploring)),
            )
            // Map-click warp destination + G-hold warp dash (while exploring)
            .add_systems(
                Update,
                (
                    map_click_system,
                    galaxy_map_click_system,
                    warp_dash_input,
                    execute_warp_dash,
                ).run_if(in_state(GameState::Exploring)),
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
                    build_ui::scroll_item_slots.run_if(
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

/// A clickable HUD toolbar button that stands in for a keyboard shortcut.
/// While pressed, hud_action_button_press synthesizes `key` onto the shared
/// ButtonInput<KeyCode> (same trick gamepad.rs uses), so the existing toggle/
/// action systems fire with no changes.
#[derive(Component)]
struct HudActionButton {
    key: KeyCode,
}

/// The in-flight action toolbar (Map/Sys/Radar/Crew). Only shown while flying
/// (GameState::Exploring) — hidden when docked or in menus, since those panels
/// aren't the right controls there. See toggle_flight_toolbar_visibility.
#[derive(Component)]
struct FlightToolbar;

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

#[derive(Component)]
pub struct CrewText;

#[derive(Component)]
pub struct CargoText;

/// Root of the Haven cargo-hold panel (top-left while StationDocked).
#[derive(Component)]
struct StationCargoPanel;
/// The itemized cargo list text inside the Haven cargo-hold panel.
#[derive(Component)]
struct StationCargoBodyText;
/// The "used/capacity hold" total line inside the Haven cargo-hold panel.
#[derive(Component)]
struct StationCargoTotalText;

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
    Power,
    Thrust,
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

/// The clickable galaxy-scale map square — same idea as MapPanel, but for
/// the galaxy view. Separate marker since only one of the two is ever
/// spawned at a time (Local vs. Galaxy view) and their coordinate
/// conversions are entirely different scales.
#[derive(Component)]
struct GalaxyMapPanel;

/// Gold crosshair marking the currently selected warp destination.
#[derive(Component)]
struct PendingWarpMarker;

/// World position the player last clicked on the map — persists across
/// opening/closing the map (it's a resource, not tied to the overlay's
/// entities) so the selection sticks until they pick a new one.
#[derive(Resource, Default)]
pub struct PendingWarpTarget(pub Option<Vec2>);

/// Which panel the M-key overlay is currently showing — Local (the existing
/// current-system tactical view) or Galaxy (the new galaxy-scale starmap).
/// Always resets to Local when the overlay is freshly opened; Tab flips it
/// while open.
#[derive(Resource, Default, PartialEq, Eq, Clone, Copy)]
pub enum MapViewMode {
    #[default]
    Local,
    Galaxy,
}

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

/// A vital meter for the redesigned top bar: a "LABEL  value" row over a thin
/// severity-colored bar. `value` spawns the value node (so the caller attaches
/// the right marker, e.g. HullText); the bar fill carries HudBar so the existing
/// update systems drive its width/color.
fn spawn_meter(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    bar_kind: HudBarKind,
    bar_color: Color,
    value: impl FnOnce(&mut ChildSpawnerCommands),
) {
    use theme::*;
    parent.spawn(Node {
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(3.0),
        min_width: Val::Px(90.0),
        ..default()
    }).with_children(|m| {
        m.spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        }).with_children(|row| {
            row.spawn((Text::new(label), TextFont { font_size: FontSize::Px(ThemeFonts::TINY), ..default() }, TextColor(ThemeColors::TEXT_MUTED)));
            value(row);
        });
        m.spawn((Node { width: Val::Percent(100.0), height: Val::Px(5.0), ..default() },
            BackgroundColor(Color::srgba(0.04, 0.06, 0.11, 0.9)))).with_children(|track| {
            track.spawn((
                (Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(bar_color)),
                HudBar { kind: bar_kind },
            ));
        });
    });
}

/// A label-over-value stack for the nav / resources clusters. `value` spawns the
/// value line(s) so the caller attaches the right marker.
fn spawn_stack(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    align_end: bool,
    value: impl FnOnce(&mut ChildSpawnerCommands),
) {
    use theme::*;
    parent.spawn(Node {
        flex_direction: FlexDirection::Column,
        align_items: if align_end { AlignItems::FlexEnd } else { AlignItems::FlexStart },
        row_gap: Val::Px(1.0),
        ..default()
    }).with_children(|g| {
        g.spawn((Text::new(label), TextFont { font_size: FontSize::Px(ThemeFonts::TINY), ..default() }, TextColor(ThemeColors::TEXT_MUTED)));
        value(g);
    });
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
        // ===== TOP BAR — Ship Vitals =====
        // Credits used to also show as a separate top-right floating counter
        // (removed — same currency.credits value shown twice on screen at
        // once; the CRED group below is now the one place to look).
        parent.spawn((Node {
                width: Val::Percent(100.0),
                padding: UiRect::new(Val::Px(ThemeSpacing::LG), Val::Px(ThemeSpacing::LG), Val::Px(ThemeSpacing::SM), Val::Px(ThemeSpacing::SM)),
                column_gap: Val::Px(ThemeSpacing::XS),
                align_items: AlignItems::Center,
                ..default()
            }, BackgroundColor(ThemeColors::HUD_BG))).with_children(|top_bar| {
            // Three scannable clusters: ship vitals (severity meters) · nav
            // (system / depth / noise) · resources (credits / crew / cargo).
            // The nav cluster grows to push resources to the right edge.

            // ---- CLUSTER: ship vitals ----
            top_bar.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(14.0),
                padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                ..default()
            }).with_children(|c| {
                spawn_meter(c, "HULL", HudBarKind::Hull, ThemeColors::ACCENT_GREEN, |r| {
                    r.spawn((Text::new("100%"), TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() }, TextColor(ThemeColors::ACCENT_GREEN), HullText));
                });
                spawn_meter(c, "PWR", HudBarKind::Power, ThemeColors::ACCENT_YELLOW, |r| {
                    r.spawn((Text::new("0/0"), TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() }, TextColor(ThemeColors::ACCENT_YELLOW), PowerText));
                });
                spawn_meter(c, "FUEL", HudBarKind::Fuel, ThemeColors::ACCENT_ORANGE, |r| {
                    r.spawn((Text::new("100%"), TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() }, TextColor(ThemeColors::ACCENT_ORANGE), FuelText));
                });
                spawn_meter(c, "THRS", HudBarKind::Thrust, ThemeColors::ACCENT_BLUE, |r| {
                    r.spawn((Text::new("0%"), TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() }, TextColor(ThemeColors::ACCENT_BLUE), ThrusterText));
                });
            });

            // ---- CLUSTER: navigation (grows to push resources right) ----
            top_bar.spawn((Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(22.0),
                padding: UiRect::axes(Val::Px(18.0), Val::Px(6.0)),
                margin: UiRect::left(Val::Px(6.0)),
                border: UiRect::left(Val::Px(1.0)),
                ..default()
            }, BorderColor::all(ThemeColors::BORDER_SUBTLE))).with_children(|c| {
                spawn_stack(c, "SYS", false, |g| {
                    g.spawn((Text::new("System-0"), TextFont { font_size: FontSize::Px(ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::TEXT_TITLE), SystemInfoText));
                    g.spawn((Text::new("Station Orbit"), TextFont { font_size: FontSize::Px(ThemeFonts::CAPTION), ..default() }, TextColor(ThemeColors::TEXT_SECONDARY), DepthZoneText));
                });
                spawn_stack(c, "HAVEN", false, |g| {
                    g.spawn((Text::new("0.0 km"), TextFont { font_size: FontSize::Px(ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::TEXT_PRIMARY), DepthText));
                    g.spawn((Text::new(""), TextFont { font_size: FontSize::Px(ThemeFonts::CAPTION), ..default() }, TextColor(ThemeColors::TEXT_SECONDARY), GravityIndicatorText));
                });
                spawn_stack(c, "NOISE", false, |g| {
                    g.spawn((Text::new("0"), TextFont { font_size: FontSize::Px(ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::TEXT_SECONDARY), NoiseText));
                });
            });

            // ---- CLUSTER: resources (right) ----
            top_bar.spawn((Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(18.0),
                padding: UiRect::axes(Val::Px(16.0), Val::Px(6.0)),
                border: UiRect::left(Val::Px(1.0)),
                ..default()
            }, BorderColor::all(ThemeColors::BORDER_SUBTLE))).with_children(|c| {
                spawn_stack(c, "CRED", true, |g| {
                    g.spawn((Text::new("500"), TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() }, TextColor(ThemeColors::ACCENT_YELLOW), CreditsText));
                });
                spawn_stack(c, "CREW", true, |g| {
                    g.spawn((Text::new("0/0"), TextFont { font_size: FontSize::Px(ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::TEXT_PRIMARY), CrewText));
                });
                spawn_stack(c, "CARGO", true, |g| {
                    g.spawn((Text::new("0/0"), TextFont { font_size: FontSize::Px(ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::ACCENT_CYAN), CargoText));
                });
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
                // Immediately overwritten every frame by build_ui::update_controls_help
                // once GameState resolves — this is just the pre-first-frame fallback.
                (Text::new("Mouse: Aim | WASD: Move | Space: Fire | B: Build | ESC: Pause"), TextFont { font_size: FontSize::Px(ThemeFonts::CAPTION), ..default() }, TextColor(ThemeColors::TEXT_MUTED)),
                build_ui::ControlsHelpText,
            ));
        });

        // Clickable action toolbar — one button per common shortcut so the
        // player can click instead of memorizing keys. Absolute-positioned just
        // above the controls strip. Each button synthesizes its KeyCode (see
        // hud_action_button_press), so every existing toggle/action reacts with
        // no changes, exactly like the gamepad bridge.
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(34.0),
                left: Val::Px(10.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
            FlightToolbar,
        )).with_children(|toolbar| {
            // Only actions that open a panel/window belong here — momentary
            // actions (ping/warp/dock/shield) stay on their keys.
            // Only actions usable WHILE FLYING belong here. Build (docked-only)
            // and Jobs/mission board (a station service) move to the dock window.
            let actions: [(&str, &str, KeyCode); 5] = [
                ("Map",   "M",   KeyCode::KeyM),
                ("Sys",   "N",   KeyCode::KeyN),
                ("Radar", "Tab", KeyCode::Tab),
                ("Crew",  "C",   KeyCode::KeyC),
                ("Pwr",   "U",   KeyCode::KeyU),
            ];
            for (label, key_disp, key) in actions {
                toolbar.spawn((
                    Node {
                        padding: UiRect::new(Val::Px(16.0), Val::Px(16.0), Val::Px(10.0), Val::Px(10.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(2.0),
                        ..default()
                    },
                    BackgroundColor(ThemeColors::BG_ELEVATED),
                    BorderColor::all(ThemeColors::BORDER_DEFAULT),
                    Button,
                    Interaction::default(),
                    HudActionButton { key },
                )).with_children(|b| {
                    b.spawn((
                        Text::new(label),
                        TextFont { font_size: FontSize::Px(ThemeFonts::H3), ..default() },
                        TextColor(ThemeColors::TEXT_PRIMARY),
                    ));
                    b.spawn((
                        Text::new(key_disp),
                        TextFont { font_size: FontSize::Px(ThemeFonts::CAPTION), ..default() },
                        TextColor(ThemeColors::TEXT_MUTED),
                    ));
                });
            }
        });

        // ===== WEAPON RACK (bottom-center, flight only) =====
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(12.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-230.0)),
                width: Val::Px(460.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(ThemeColors::HUD_BG),
            BorderColor::all(ThemeColors::BORDER_DEFAULT),
            WeaponRackPanel,
        )).with_children(|rack| {
            rack.spawn((Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            }, BorderColor::all(ThemeColors::BORDER_SUBTLE))).with_children(|h| {
                h.spawn((Text::new("WEAPONS"), TextFont { font_size: FontSize::Px(ThemeFonts::TINY), ..default() }, TextColor(ThemeColors::TEXT_SECONDARY)));
            });
            rack.spawn((Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                row_gap: Val::Px(2.0),
                ..default()
            }, WeaponRackRoot));
        });
    });
}

/// Turn a HUD toolbar button press into a synthesized key press. Mirrors
/// gamepad.rs's bridge: build the set of keys whose button is currently held,
/// release ones no longer held, press the rest (idempotent). Runs in PreUpdate
/// after InputSystems so the synthesized `just_pressed` is seen by the toggle
/// systems that same frame.
fn hud_action_button_press(
    buttons: Query<(&Interaction, &HudActionButton)>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    mut emulated: Local<HashSet<KeyCode>>,
) {
    let mut desired: HashSet<KeyCode> = HashSet::new();
    for (interaction, btn) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            desired.insert(btn.key);
        }
    }
    for &key in emulated.iter() {
        if !desired.contains(&key) {
            keyboard.release(key);
        }
    }
    for &key in desired.iter() {
        keyboard.press(key);
    }
    *emulated = desired;
}

/// Hover/press color feedback for the HUD toolbar buttons.
fn hud_action_button_hover(
    mut buttons: Query<(&Interaction, &mut BackgroundColor), (With<HudActionButton>, Changed<Interaction>)>,
) {
    for (interaction, mut bg) in buttons.iter_mut() {
        *bg = theme::button_color_for_interaction(interaction).into();
    }
}

/// Show the flight toolbar only while flying — hide it when docked or in menus,
/// where Map/Sys/Radar/Crew aren't the relevant controls. Runs on state change.
fn toggle_flight_toolbar_visibility(
    state: Res<State<GameState>>,
    mut panels: Query<&mut Node, With<FlightToolbar>>,
) {
    if !state.is_changed() {
        return;
    }
    let show = *state.get() == GameState::Exploring;
    for mut node in panels.iter_mut() {
        node.display = if show { Display::Flex } else { Display::None };
    }
}

/// Weapon rack visibility: shown while flying, hidden when docked, and manually
/// toggleable with K (so it can be tucked away). Only writes when the effective
/// visibility changes, to avoid dirtying the layout every frame.
fn weapon_rack_visibility(
    keyboard: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    mut hidden: Local<bool>,
    mut last: Local<Option<bool>>,
    mut rack: Query<&mut Node, With<WeaponRackPanel>>,
) {
    if keyboard.just_pressed(KeyCode::KeyK) {
        *hidden = !*hidden;
    }
    let show = *state.get() == GameState::Exploring && !*hidden;
    if *last != Some(show) {
        *last = Some(show);
        for mut node in rack.iter_mut() {
            node.display = if show { Display::Flex } else { Display::None };
        }
    }
}

// ===== WEAPON RACK =====

/// The bottom-center weapon rack panel — a flight HUD element gated to Exploring
/// (shares toggle_flight_toolbar_visibility with the toolbar).
#[derive(Component)]
struct WeaponRackPanel;
/// Container the per-weapon rows are spawned into.
#[derive(Component)]
struct WeaponRackRoot;
/// One rack row, bound to a weapon module entity, caching its dynamic children.
#[derive(Component)]
struct WeaponRackRow {
    weapon: Entity,
    bar: Entity,
    ammo: Entity,
    state: Entity,
}
/// The reload-progress bar fill inside a rack row.
#[derive(Component)]
struct WeaponRackBarFill;
/// Marks the ammo/state text nodes so the update query stays narrow.
#[derive(Component)]
struct WeaponRackDynText;

fn weapon_rack_name(m: ModuleType) -> &'static str {
    match m {
        ModuleType::Railgun => "Railgun",
        ModuleType::Cannon => "Cannon",
        ModuleType::Coilgun => "Coilgun",
        ModuleType::Gatling => "Gatling",
        ModuleType::Laser => "Laser",
        ModuleType::PlasmaCaster => "Plasma Caster",
        ModuleType::IonDisruptor => "Ion Disruptor",
        ModuleType::HeavyMissile => "Heavy Missile",
        ModuleType::GuidedMissile => "Guided Missile",
        ModuleType::ClusterRocket => "Cluster Rocket",
        ModuleType::EMPPulse => "EMP Pulse",
        _ => "Weapon",
    }
}

/// Rebuilds/updates the weapon rack: one row per player weapon with a reload
/// bar (fills 0→ready), its fire-group tag, ammo count, and a READY / countdown
/// state. Rows are reconciled by weapon entity, so adding/removing a gun in the
/// yard updates the rack on next entry to flight.
fn update_weapon_rack(
    rack_root: Query<Entity, With<WeaponRackRoot>>,
    ship_query: Query<Entity, With<Ship>>,
    weapon_query: Query<(Entity, &Module, &Weapon, Option<&WeaponCooldown>, Option<&crate::combat::targeting::fire_groups::FireGroup>, &ChildOf)>,
    rows: Query<(Entity, &WeaponRackRow)>,
    mut texts: Query<(&mut Text, &mut TextColor), With<WeaponRackDynText>>,
    mut bars: Query<(&mut Node, &mut BackgroundColor), With<WeaponRackBarFill>>,
    mut commands: Commands,
) {
    use theme::*;
    let Ok(root) = rack_root.single() else { return };
    let Ok(player) = ship_query.single() else { return };

    struct W { e: Entity, name: &'static str, grp: u8, frac: f32, ready: bool, remaining: f32, ammo: u32, max: u32 }
    let mut ws: Vec<W> = Vec::new();
    for (e, module, weapon, cd, fg, parent) in weapon_query.iter() {
        if parent.parent() != player { continue; }
        let (frac, ready, remaining) = match cd {
            Some(cd) => (cd.timer.fraction(), cd.timer.is_finished(), cd.timer.remaining_secs()),
            None => (1.0, true, 0.0),
        };
        ws.push(W {
            e,
            name: weapon_rack_name(module.module_type),
            grp: fg.map(|f| f.group).unwrap_or(0),
            frac, ready, remaining,
            ammo: weapon.ammo, max: weapon.max_ammo,
        });
    }
    ws.sort_by(|a, b| a.grp.cmp(&b.grp).then(a.name.cmp(b.name)));

    // Update existing rows; despawn rows whose weapon is gone.
    let mut have: HashSet<Entity> = HashSet::new();
    for (row_e, row) in rows.iter() {
        if let Some(w) = ws.iter().find(|w| w.e == row.weapon) {
            have.insert(row.weapon);
            if let Ok((mut node, mut bg)) = bars.get_mut(row.bar) {
                node.width = Val::Percent((w.frac * 100.0).clamp(0.0, 100.0));
                *bg = if w.ready { ThemeColors::ACCENT_GREEN } else { ThemeColors::ACCENT_ORANGE }.into();
            }
            if let Ok((mut t, _)) = texts.get_mut(row.ammo) {
                t.0 = format!("{}/{}", w.ammo, w.max);
            }
            if let Ok((mut t, mut col)) = texts.get_mut(row.state) {
                if w.ready { t.0 = "READY".into(); col.0 = ThemeColors::ACCENT_GREEN; }
                else { t.0 = format!("{:.1}s", w.remaining); col.0 = ThemeColors::ACCENT_ORANGE; }
            }
        } else {
            commands.entity(row_e).despawn();
        }
    }
    // Spawn rows for weapons that don't have one yet.
    for w in ws.iter().filter(|w| !have.contains(&w.e)) {
        let grp = commands.spawn((
            Text::new(format!("{}", w.grp + 1)),
            Node { min_width: Val::Px(16.0), ..default() },
            TextFont { font_size: FontSize::Px(ThemeFonts::TINY), ..default() },
            TextColor(ThemeColors::TEXT_MUTED),
        )).id();
        let name = commands.spawn((
            Text::new(w.name),
            Node { flex_grow: 1.0, ..default() },
            TextFont { font_size: FontSize::Px(ThemeFonts::BODY_SMALL), ..default() },
            TextColor(ThemeColors::TEXT_PRIMARY),
        )).id();
        let bar_fill = commands.spawn((
            (Node { width: Val::Percent(w.frac * 100.0), height: Val::Percent(100.0), ..default() },
             BackgroundColor(if w.ready { ThemeColors::ACCENT_GREEN } else { ThemeColors::ACCENT_ORANGE })),
            WeaponRackBarFill,
        )).id();
        let bar_track = commands.spawn((
            Node { width: Val::Px(64.0), height: Val::Px(4.0), ..default() },
            BackgroundColor(Color::srgba(0.04, 0.06, 0.11, 0.9)),
        )).id();
        commands.entity(bar_track).add_child(bar_fill);
        let ammo = commands.spawn((
            Text::new(format!("{}/{}", w.ammo, w.max)),
            Node { min_width: Val::Px(48.0), ..default() },
            TextFont { font_size: FontSize::Px(ThemeFonts::CAPTION), ..default() },
            TextColor(ThemeColors::TEXT_SECONDARY),
            WeaponRackDynText,
        )).id();
        let state = commands.spawn((
            Text::new(if w.ready { "READY" } else { "..." }),
            Node { min_width: Val::Px(44.0), ..default() },
            TextFont { font_size: FontSize::Px(ThemeFonts::TINY), ..default() },
            TextColor(ThemeColors::ACCENT_GREEN),
            WeaponRackDynText,
        )).id();
        let row = commands.spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(10.0),
                padding: UiRect::axes(Val::Px(4.0), Val::Px(3.0)),
                ..default()
            },
            WeaponRackRow { weapon: w.e, bar: bar_fill, ammo, state },
        )).id();
        commands.entity(row).add_children(&[grp, name, bar_track, ammo, state]);
        commands.entity(root).add_child(row);
    }
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

/// Formats a world-unit range as kilometres. Every "depth" number in this game
/// is really the ship's radial distance from Haven Station (see
/// movement::update_depth) — it's a space game, not a submarine one, so the
/// readouts say how far out you are, in km. One decimal close in, where 100m
/// of drift still matters; whole km once you're far enough out that it doesn't.
pub fn format_range_km(units: f32) -> String {
    let km = units / 1000.0;
    if km < 10.0 { format!("{:.1} km", km) } else { format!("{:.0} km", km) }
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
    // Range from Haven Station
    if let Ok((mut text, mut text_color)) = depth_query.single_mut() {
        text.0 = format_range_km(depth_state.current_depth);
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
            HudBarKind::Power => {
                // Fill = consumption as a fraction of generation (how much of
                // the budget is used); red when in deficit, yellow otherwise.
                let gen = power_state.total_power_generation.max(0.001);
                let frac = (power_state.total_power_consumption / gen).clamp(0.0, 1.0);
                let c = if power_state.power_balance < 0.0 { Color::srgb(0.9, 0.2, 0.2) } else { Color::srgb(0.9, 0.75, 0.25) };
                (frac, c)
            }
            // Oxygen bar removed with crew O2 (its HUD group no longer spawns)
            HudBarKind::Oxygen => continue,
            HudBarKind::Fuel | HudBarKind::Thrust => continue, // handled in update_hud_secondary
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
    ship_query: Query<(Entity, &ShipPhysics), With<Ship>>,
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
    mut crew_query_hud: Query<(&mut Text, &mut TextColor), (With<CrewText>, Without<FuelText>, Without<ThrusterText>, Without<AmmoText>, Without<NoiseText>, Without<CreditsText>)>,
    mut bar_query: Query<(&HudBar, &mut Node, &mut BackgroundColor)>,
    // Bundled into one tuple param — this system is already at Bevy's
    // 16-param ceiling (see the ammo_ui tuple above for the same reason).
    mut cargo_ui: (Res<Inventory>, Query<(&mut Text, &mut TextColor), (With<CargoText>, Without<FuelText>, Without<ThrusterText>, Without<AmmoText>, Without<NoiseText>, Without<CreditsText>, Without<CrewText>)>),
) {
    let (ammo_container_query, ammo_line_query, mut commands, mut last_ammo_snapshot) = ammo_ui;
    let (inventory, mut cargo_query) = cargo_ui;
    let Ok((player_ship, physics)) = ship_query.single() else { return };

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

    // Main-drive throttle for the THRS meter + text. This used to read the
    // Q/E vertical thrusters, which are gone — so it sat at 0% (or "N/A") all
    // game. Now it's the W/S throttle the ship actually flies on: -1 full
    // reverse .. +1 full ahead, eased by ship_movement.
    let throttle = physics.throttle.clamp(-1.0, 1.0);
    let thrust_avg = throttle.abs();

    // Update fuel + thrust meter bars
    for (bar, mut style, mut bg) in bar_query.iter_mut() {
        match bar.kind {
            HudBarKind::Fuel => {
                style.width = Val::Percent(fuel_pct * 100.0);
                *bg = if fuel_pct < 0.25 { Color::srgb(1.0, 0.0, 0.0) } else { Color::srgb(1.0, 0.6, 0.2) }.into();
            }
            HudBarKind::Thrust => {
                style.width = Val::Percent((thrust_avg * 100.0).clamp(0.0, 100.0));
                // Amber while backing down, so reverse reads at a glance.
                *bg = if throttle < -0.02 { Color::srgb(1.0, 0.6, 0.2) } else { Color::srgb(0.3, 0.5, 1.0) }.into();
            }
            _ => {}
        }
    }

    // Throttle
    if let Ok((mut text, mut text_color)) = thruster_text_query.single_mut() {
        let pct = (thrust_avg * 100.0).round() as i32;
        if throttle < -0.02 {
            text.0 = format!("{}% REV", pct);
            text_color.0 = Color::srgb(1.0, 0.6, 0.2);
        } else {
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

    // Crew staffing
    if let Ok((mut text, mut text_color)) = crew_query_hud.single_mut() {
        text.0 = format!("{}/{}", staffing_state.total_crew, staffing_state.total_berths);
        text_color.0 = if staffing_state.total_crew > staffing_state.total_berths {
            Color::srgb(1.0, 0.0, 0.0)
        } else {
            Color::srgb(0.7, 0.9, 0.7)
        };
    }

    // Cargo hold — weight-based (matches Inventory.max_capacity, not an
    // item count), the same numbers the Map overlay's Inventory section and
    // sell/buy pricing already use.
    if let Ok((mut text, mut text_color)) = cargo_query.single_mut() {
        text.0 = format!("{:.0}/{:.0}", inventory.current_weight, inventory.max_capacity);
        text_color.0 = if inventory.current_weight >= inventory.max_capacity {
            Color::srgb(1.0, 0.0, 0.0)
        } else if inventory.max_capacity > 0.0 && inventory.current_weight / inventory.max_capacity > 0.85 {
            Color::srgb(1.0, 1.0, 0.0)
        } else {
            Color::srgb(0.6, 0.8, 1.0)
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

// ============================================================================
// HAVEN CARGO-HOLD PANEL
// A small itemized cargo list shown top-left while docked at the home station,
// where the trade menu (remote outposts) and the flying-only M inventory
// overlay both leave the hold unreadable. Single Text nodes updated per frame,
// same as the HUD readouts.
// ============================================================================

fn spawn_station_cargo_panel(mut commands: Commands) {
    use theme::*;
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(ThemeSpacing::LG),
                top: Val::Px(52.0),
                width: Val::Px(200.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(ThemeSpacing::XS),
                padding: UiRect::all(Val::Px(ThemeSpacing::MD)),
                ..default()
            },
            BackgroundColor(ThemeColors::BG_PANEL),
            ZIndex(40),
            StationCargoPanel,
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("CARGO HOLD"),
                TextFont { font_size: FontSize::Px(ThemeFonts::CAPTION), ..default() },
                TextColor(ThemeColors::TEXT_MUTED),
            ));
            panel.spawn((
                Node { width: Val::Percent(100.0), height: Val::Px(1.0), margin: UiRect::vertical(Val::Px(ThemeSpacing::XS)), ..default() },
                BackgroundColor(ThemeColors::BORDER_SUBTLE),
            ));
            panel.spawn((
                Text::new("— empty —"),
                TextFont { font_size: FontSize::Px(ThemeFonts::BODY_SMALL), ..default() },
                TextColor(ThemeColors::TEXT_SECONDARY),
                StationCargoBodyText,
            ));
            panel.spawn((
                Node { width: Val::Percent(100.0), height: Val::Px(1.0), margin: UiRect::vertical(Val::Px(ThemeSpacing::XS)), ..default() },
                BackgroundColor(ThemeColors::BORDER_SUBTLE),
            ));
            panel.spawn((
                Text::new("0/0 hold"),
                TextFont { font_size: FontSize::Px(ThemeFonts::CAPTION), ..default() },
                TextColor(ThemeColors::ACCENT_BLUE),
                StationCargoTotalText,
            ));
        });
}

fn update_station_cargo_panel(
    inventory: Res<Inventory>,
    mut body_q: Query<&mut Text, (With<StationCargoBodyText>, Without<StationCargoTotalText>)>,
    mut total_q: Query<(&mut Text, &mut TextColor), (With<StationCargoTotalText>, Without<StationCargoBodyText>)>,
) {
    // Rebuilt every frame (cheap — a handful of items on a cold station
    // screen); the `!=` guards below keep it from touching the Text unless the
    // hold actually changed. Sorted so the list is stable frame-to-frame
    // (HashMap iteration order is not).
    let mut lines: Vec<String> = inventory
        .items
        .iter()
        .filter(|(_, &count)| count > 0)
        .map(|(item, count)| format!("{}  x{}", item.name(), count))
        .collect();
    lines.sort();
    let body = if lines.is_empty() { "— empty —".to_string() } else { lines.join("\n") };

    if let Ok(mut text) = body_q.single_mut() {
        if text.0 != body {
            text.0 = body;
        }
    }
    if let Ok((mut text, mut color)) = total_q.single_mut() {
        text.0 = format!("{:.0}/{:.0} hold", inventory.current_weight, inventory.max_capacity);
        color.0 = if inventory.max_capacity > 0.0 && inventory.current_weight >= inventory.max_capacity {
            theme::ThemeColors::STATUS_DANGER
        } else {
            theme::ThemeColors::ACCENT_BLUE
        };
    }
}

fn despawn_station_cargo_panel(
    mut commands: Commands,
    query: Query<Entity, With<StationCargoPanel>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
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
    mut tutorial: ResMut<crate::tutorial::Tutorial>,
    mut settings_menu: ResMut<menu_buttons::SettingsMenu>,
    mut commands: Commands,
    module_panel: Query<Entity, With<ModulePanelOverlay>>,
    floating_windows: Query<(Entity, &windows::framework::FloatingWindow)>,
) {
    // Settings overlay is modal: while it's open, Escape closes it (and nothing
    // else) so it never falls through to resume/close the underlying menu.
    if settings_menu.open {
        if keyboard.just_pressed(KeyCode::Escape) {
            settings_menu.open = false;
        }
        return;
    }

    // The deep customization window has no toggle key, so Escape closes it here
    // (complementing its × button) before Escape can fall through to the pause
    // menu. Its floating-window id is prefixed "deep_" (see customization.rs).
    if keyboard.just_pressed(KeyCode::Escape) {
        let mut closed_any = false;
        for (entity, window) in floating_windows.iter() {
            if window.id.starts_with("deep_") {
                commands.entity(entity).despawn();
                closed_any = true;
            }
        }
        if closed_any {
            return;
        }
    }

    if keyboard.just_pressed(KeyCode::Escape) {
        match current_state.get() {
            // While build mode is active, Escape backs out of build layers
            // (paste → selection → build mode, see clipboard_input) instead
            // of opening the pause menu.
            GameState::StationDocked if *build_state.get() != BuildState::Inactive => {}
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
        && !is_building
        && !is_customizing
        && !mission_board_open.0
    {
        match current_state.get() {
            GameState::MainMenu => {
                // New expedition (not a load) — arm the guided tutorial.
                tutorial.begin();
                next_state.set(GameState::StationDocked);
            }
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
    existing_menu: Query<(Entity, &windows::framework::FloatingWindow)>,
    // Without<OwnedByAiShip>: player-only — AI ships carry CrewMember/
    // CrewStation now too (see ai_ship::crew), unscoped this menu would show
    // AI crew mixed into the player's own crew roster.
    crew_query: Query<(Entity, &CrewMember, Option<&CrewDuty>), Without<crate::ai_ship::components::OwnedByAiShip>>,
    station_query: Query<(&CrewStation, &Module), Without<crate::ai_ship::components::OwnedByAiShip>>,
    staffing_state: Res<StaffingState>,
) {
    if !keyboard.just_pressed(KeyCode::KeyC) {
        return;
    }

    const WINDOW_ID: &str = "crew_menu";

    // Toggle off if already open — despawn the window ROOT (found via
    // FloatingWindow.id), not the content entity spawn_floating_window
    // returns, so the title bar/border don't linger orphaned on screen.
    if let Some((entity, _)) = existing_menu.iter().find(|(_, w)| w.id == WINDOW_ID) {
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

    let content = windows::framework::spawn_floating_window(
        &mut commands,
        WINDOW_ID,
        "Crew Management",
        Vec2::new(460.0, 340.0),
        Vec2::new(10.0, 60.0),
    );

    commands.entity(content).with_children(|parent| {
        parent.spawn((
            Text::new(format!("{}/{} berths — {}/{} stations staffed",
                staffing_state.total_crew, staffing_state.total_berths,
                staffing_state.staffed_stations, staffing_state.total_stations)),
            TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY), ..default() },
            TextColor(theme::ThemeColors::TEXT_TITLE),
            Node { margin: UiRect::bottom(Val::Px(theme::ThemeSpacing::SM)), ..default() },
        ));
    });
    theme::spawn_divider(&mut commands, content);

    commands.entity(content).with_children(|parent| {
        parent.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(theme::ThemeSpacing::SM),
            ..default()
        }).with_children(|list| {
            for (entity, crew, duty) in crew_query.iter() {
                let duty = duty.copied().unwrap_or_default();
                let (status, dot_color) = if crew.health <= 0.0 {
                    ("DEAD".to_string(), theme::ThemeColors::STATUS_DANGER)
                } else if crew.state == CrewState::Panicking {
                    (format!("{:?}", crew.state), theme::ThemeColors::STATUS_WARN)
                } else if let Some(grid) = crew_assignments.get(&entity) {
                    (format!("{:?} → ({},{})", crew.state, grid.x, grid.y), theme::ThemeColors::STATUS_OK)
                } else {
                    ("Idle".to_string(), theme::ThemeColors::TEXT_MUTED)
                };

                list.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(theme::ThemeSpacing::MD),
                        padding: UiRect::all(Val::Px(theme::ThemeSpacing::SM)),
                        ..default()
                    },
                    BackgroundColor(theme::ThemeColors::BG_CARD),
                )).with_children(|row| {
                    // Status dot — one glance instead of parsing text
                    row.spawn((
                        Node { width: Val::Px(8.0), height: Val::Px(8.0), ..default() },
                        BackgroundColor(dot_color),
                    ));
                    row.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        row_gap: Val::Px(theme::ThemeSpacing::XS),
                        ..default()
                    }).with_children(|info| {
                        info.spawn((
                            Text::new(format!("{}  —  {}", crew.name, status)),
                            TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY_SMALL), ..default() },
                            TextColor(if crew.health <= 0.0 { theme::ThemeColors::TEXT_MUTED } else { theme::ThemeColors::TEXT_PRIMARY }),
                        ));
                        info.spawn((
                            Text::new(format!("HP {:.0}   Morale {:.0}", crew.health, crew.morale)),
                            TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() },
                            TextColor(theme::ThemeColors::TEXT_MUTED),
                        ));
                    });

                    // Standing order. Click to cycle — a real dropdown is a
                    // lot of machinery for six values, and one click per step
                    // beats one click to open plus one to choose.
                    row.spawn((
                        Node {
                            width: Val::Px(96.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            padding: UiRect::all(Val::Px(theme::ThemeSpacing::XS)),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(theme::ThemeColors::BG_ELEVATED),
                        BorderColor::all(theme::ThemeColors::BORDER_DEFAULT),
                        Button,
                        Interaction::default(),
                        CrewDutyButton { crew: entity },
                    )).with_children(|chip| {
                        chip.spawn((
                            Text::new(duty.label()),
                            TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() },
                            TextColor(if duty == CrewDuty::Auto {
                                theme::ThemeColors::TEXT_MUTED
                            } else {
                                theme::ThemeColors::TEXT_TITLE
                            }),
                            CrewDutyLabel { crew: entity },
                        ));
                    });
                });
            }
        });
    });
}

/// Stub for crew assignment input — press 1 to manually assign idle crew to first unstaffed weapon
/// Marks the clickable duty chip on a crew row.
#[derive(Component)]
pub struct CrewDutyButton {
    pub crew: Entity,
}

/// Marks the chip's text so it can be refreshed in place.
#[derive(Component)]
pub struct CrewDutyLabel {
    pub crew: Entity,
}

/// Clicking a crew member's chip cycles their standing order.
///
/// Replaces a "press 1 to assign the first idle hand to the first weapon"
/// stub that was never wired to the menu being open.
fn crew_duty_click(
    mut commands: Commands,
    buttons: Query<(&Interaction, &CrewDutyButton), Changed<Interaction>>,
    mut crew_query: Query<(&CrewMember, Option<&mut CrewDuty>)>,
    mut station_query: Query<&mut CrewStation>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for (interaction, button) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Ok((crew, duty)) = crew_query.get_mut(button.crew) else { continue };
        let next = duty.as_deref().copied().unwrap_or_default().next();
        match duty {
            Some(mut d) => *d = next,
            None => {
                commands.entity(button.crew).try_insert(next);
            }
        }

        // Drop the post they were holding so the next auto-assign pass can
        // honour the new order instead of leaving them where they were.
        for mut station in station_query.iter_mut() {
            if station.assigned_crew == Some(button.crew) {
                station.assigned_crew = None;
                station.manually_assigned = false;
            }
        }

        notifications.write(ShowNotification {
            message: format!("{} — {}", crew.name, next.label()),
            notification_type: NotificationType::Info,
            duration: 1.5,
        });
    }
}

/// Keeps the chips reading true while the window stays open.
fn refresh_crew_duty_labels(
    duties: Query<&CrewDuty>,
    mut labels: Query<(&CrewDutyLabel, &mut Text, &mut TextColor)>,
) {
    for (label, mut text, mut color) in labels.iter_mut() {
        let duty = duties.get(label.crew).copied().unwrap_or_default();
        let wanted = duty.label();
        if text.0 != wanted {
            text.0 = wanted.to_string();
        }
        color.0 = if duty == CrewDuty::Auto {
            theme::ThemeColors::TEXT_MUTED
        } else {
            theme::ThemeColors::TEXT_TITLE
        };
    }
}

// ============================================================================
// MAP / INVENTORY OVERLAY (M key)
// ============================================================================

/// World-units-to-map-pixels scale. Covers -MAP_WORLD_RANGE..MAP_WORLD_RANGE
/// on each axis around `center` — big enough to fit every faction territory
/// (25k-175k out) and a star system (star at ~50k-100k out from its own
/// local_center, planets orbiting another 25k-45k+ beyond that).
const MAP_WORLD_RANGE: f32 = 600_000.0;

/// Converts a world position to a pixel offset within a square map panel of
/// the given size (top-left origin, Y flipped since world +Y is up but UI
/// +Y is down). `center` is the map's fixed reference point — NOT the
/// player (see map_click_system's doc comment on why the frame doesn't
/// recenter every frame) but the CURRENT SYSTEM's local_center. Every
/// system's local_center lives somewhere in a multi-million-unit box (see
/// celestial::galaxy::generate_galaxy_map's LOCAL_RANGE), not clustered near
/// world origin like only-Haven-ever-existing used to guarantee — hardcoding
/// origin here left every non-Haven system's contents clamped to whichever
/// panel edge was closest, reading as a "glitch."
fn world_to_map_px(world_pos: Vec2, center: Vec2, panel_size: f32) -> (f32, f32) {
    let rel = world_pos - center;
    let half = panel_size / 2.0;
    let x = half + (rel.x / MAP_WORLD_RANGE) * half;
    let y = half - (rel.y / MAP_WORLD_RANGE) * half;
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
    streaming: Res<'w, crate::celestial::resources::SystemStreamingManager>,
    galaxy_map: Res<'w, crate::celestial::resources::GalaxyMap>,
    stations: Res<'w, crate::world::home_base::SystemStations>,
}

/// Plain-data snapshot of everything the map needs to render — decoupled
/// from SystemParams so both toggle_map_overlay (open) and map_click_system
/// (re-render after picking a destination) can build the exact same UI
/// without duplicating the layout code.
struct MapSnapshot {
    panel_size: f32,
    player_pos: Vec2,
    // Fixed reference point the local map is drawn around — see
    // world_to_map_px's doc comment.
    map_center: Vec2,
    pending_target: Option<Vec2>,
    current_fuel: f32,
    /// This system's stations — position, name and accent color. Every
    /// system has its own (see world::home_base::station_sites), so unlike
    /// the old fixed Haven-only list these are always the right ones for
    /// wherever the player actually is.
    stations: Vec<(Vec2, String, Color)>,
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
    /// Name of the system being shown, for the map header.
    system_name: String,
    /// Nearest station name + range, so the header answers "where am I?"
    /// without hunting for the green dot.
    nearest_station: Option<(String, f32)>,
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
            }, BackgroundColor(theme::ThemeColors::BG_VOID), ZIndex(50)),
        MapOverlay,
    )).with_children(|parent| {
        // Left column: header, map panel, legend.
        parent.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(theme::ThemeSpacing::MD),
            flex_shrink: 0.0,
            ..default()
        }).with_children(|col| {
        // Header strip: which system this is, plus where the nearest station
        // sits. The map used to open with no title at all — nothing on screen
        // said which of thirty systems you were looking at.
        col.spawn((
            Node {
                width: Val::Px(panel_size),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::axes(Val::Px(theme::ThemeSpacing::LG), Val::Px(theme::ThemeSpacing::MD)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::ThemeColors::BG_PANEL),
            BorderColor::all(theme::ThemeColors::BORDER_DEFAULT),
        )).with_children(|head| {
            head.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme::ThemeSpacing::XS),
                ..default()
            }).with_children(|left| {
                left.spawn((
                    Text::new(format!("LOCAL MAP — {}", snap.system_name.to_uppercase())),
                    TextFont { font_size: FontSize::Px(theme::ThemeFonts::H2), ..default() },
                    TextColor(theme::ThemeColors::TEXT_TITLE),
                ));
                let sub = match &snap.nearest_station {
                    Some((name, dist)) => format!("Nearest station: {} · {}", name, format_range_km(*dist)),
                    None => "No station in this system".to_string(),
                };
                left.spawn((
                    Text::new(sub),
                    TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() },
                    TextColor(theme::ThemeColors::TEXT_SECONDARY),
                ));
            });
            head.spawn((
                Text::new("TAB: GALAXY VIEW   ·   CLICK: SET WARP TARGET   ·   M: CLOSE"),
                TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() },
                TextColor(theme::ThemeColors::TEXT_MUTED),
            ));
        });
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
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::ThemeColors::HUD_BG),
            BorderColor::all(theme::ThemeColors::BORDER_DEFAULT),
            Interaction::None,
            MapPanel,
        )).with_children(|map| {
            // Star(s)
            for star_pos in &snap.stars {
                let (x, y) = world_to_map_px(*star_pos, snap.map_center, panel_size);
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
                let (x, y) = world_to_map_px(*planet_pos, snap.map_center, panel_size);
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
            // Stations of the system currently loaded — labelled, since two
            // per system means each one is a distinct destination worth
            // recognising rather than an anonymous green dot.
            for (station_pos, name, accent) in &snap.stations {
                let (x, y) = world_to_map_px(*station_pos, snap.map_center, panel_size);
                map.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(x - 5.0),
                        top: Val::Px(y - 5.0),
                        width: Val::Px(10.0),
                        height: Val::Px(10.0),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(*accent),
                    BorderColor::all(Color::srgba(0.9, 1.0, 0.95, 0.85)),
                ));
                map.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(x + 9.0),
                        top: Val::Px(y - 7.0),
                        ..default()
                    },
                    Text::new(name.clone()),
                    TextFont { font_size: FontSize::Px(theme::ThemeFonts::TINY), ..default() },
                    TextColor(Color::srgba(0.75, 0.95, 0.8, 0.9)),
                ));
            }
            // Hostiles: real (in render range) + still-off-screen simulated
            for hostile_pos in &snap.hostiles {
                let (x, y) = world_to_map_px(*hostile_pos, snap.map_center, panel_size);
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
                let (x, y) = world_to_map_px(*bounty_pos, snap.map_center, panel_size);
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
                let (x, y) = world_to_map_px(target, snap.map_center, panel_size);
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
            let (px, py) = world_to_map_px(snap.player_pos, snap.map_center, panel_size);
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
                    row.spawn((Text::new(*label), TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() }, TextColor(theme::ThemeColors::TEXT_SECONDARY)));
                });
            }
        });
        });

        // Sidebar: warp plan, cargo hold, survey log. Rebuilt around cards
        // with real hierarchy — it used to be one flat run of text lines
        // where the warp cost, the cargo list and the log entries all looked
        // exactly alike.
        parent.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                width: Val::Px(360.0),
                height: Val::Percent(92.0),
                padding: UiRect::all(Val::Px(theme::ThemeSpacing::LG)),
                row_gap: Val::Px(theme::ThemeSpacing::MD),
                overflow: Overflow::clip_y(),
                flex_shrink: 0.0,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::ThemeColors::BG_PANEL),
            BorderColor::all(theme::ThemeColors::BORDER_DEFAULT),
        )).with_children(|parent| {
            let section_header = |parent: &mut ChildSpawnerCommands, label: &str| {
                parent.spawn((
                    Text::new(label.to_uppercase()),
                    TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() },
                    TextColor(theme::ThemeColors::TEXT_MUTED),
                    Node { margin: UiRect::top(Val::Px(theme::ThemeSpacing::SM)), ..default() },
                ));
            };

            // ---- WARP PLAN ----
            section_header(parent, "Warp Plan");
            parent.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(theme::ThemeSpacing::XS),
                    padding: UiRect::all(Val::Px(theme::ThemeSpacing::MD)),
                    border: UiRect::left(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(theme::ThemeColors::BG_CARD),
                BorderColor::all(if snap.pending_target.is_some() {
                    theme::ThemeColors::ACCENT_YELLOW
                } else {
                    theme::ThemeColors::BORDER_SUBTLE
                }),
            )).with_children(|card| {
                let Some(target) = snap.pending_target else {
                    card.spawn((
                        Text::new("No destination set"),
                        TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY), ..default() },
                        TextColor(theme::ThemeColors::TEXT_SECONDARY),
                    ));
                    card.spawn((
                        Text::new("Click anywhere on the map to plot a jump."),
                        TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY_SMALL), ..default() },
                        TextColor(theme::ThemeColors::TEXT_MUTED),
                    ));
                    return;
                };

                let dist = snap.player_pos.distance(target);
                let fuel_cost = warp_dash_fuel_cost((dist - WARP_DASH_ARRIVAL_BUFFER).max(0.0));
                let charge_time = warp_dash_charge_time((dist - WARP_DASH_ARRIVAL_BUFFER).max(0.0));
                let can_afford = snap.current_fuel >= fuel_cost;

                card.spawn((
                    Text::new(format_range_km(dist)),
                    TextFont { font_size: FontSize::Px(theme::ThemeFonts::H2), ..default() },
                    TextColor(theme::ThemeColors::TEXT_PRIMARY),
                ));

                let mut stat = |card: &mut ChildSpawnerCommands, label: &str, value: String, color: Color| {
                    card.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        ..default()
                    }).with_children(|row| {
                        row.spawn((
                            Text::new(label),
                            TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY_SMALL), ..default() },
                            TextColor(theme::ThemeColors::TEXT_SECONDARY),
                        ));
                        row.spawn((
                            Text::new(value),
                            TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY), ..default() },
                            TextColor(color),
                        ));
                    });
                };
                stat(card, "Fuel", format!("{:.0} / {:.0}", fuel_cost, snap.current_fuel),
                    if can_afford { theme::ThemeColors::TEXT_PRIMARY } else { theme::ThemeColors::ACCENT_RED });
                stat(card, "Charge", format!("{:.0}s", charge_time), theme::ThemeColors::TEXT_PRIMARY);

                card.spawn((
                    Text::new(if can_afford {
                        "Close the map (M), then hold G to jump."
                    } else {
                        "Not enough fuel for this jump."
                    }),
                    TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY_SMALL), ..default() },
                    TextColor(if can_afford { theme::ThemeColors::ACCENT_GREEN } else { theme::ThemeColors::ACCENT_RED }),
                    Node { margin: UiRect::top(Val::Px(theme::ThemeSpacing::XS)), ..default() },
                ));
            });

            // ---- CARGO ----
            section_header(parent, "Cargo Hold");
            let (weight, capacity) = snap.inventory_weight;
            let fill = if capacity > 0.0 { (weight / capacity).clamp(0.0, 1.0) } else { 0.0 };
            let fill_color = if fill > 0.9 {
                theme::ThemeColors::ACCENT_RED
            } else if fill > 0.7 {
                theme::ThemeColors::ACCENT_ORANGE
            } else {
                theme::ThemeColors::ACCENT_CYAN
            };
            parent.spawn(Node {
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            }).with_children(|row| {
                row.spawn((
                    Text::new(format!("{:.0} / {:.0}", weight, capacity)),
                    TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY), ..default() },
                    TextColor(theme::ThemeColors::TEXT_PRIMARY),
                ));
                row.spawn((
                    Text::new(format!("{:.0}% full", fill * 100.0)),
                    TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() },
                    TextColor(theme::ThemeColors::TEXT_SECONDARY),
                ));
            });
            // Capacity bar — the old sidebar printed the raw weight numbers
            // and nothing else, so "nearly full" never registered until a
            // pickup silently failed.
            parent.spawn((
                Node { width: Val::Percent(100.0), height: Val::Px(6.0), ..default() },
                BackgroundColor(theme::ThemeColors::BG_INPUT),
            )).with_children(|track| {
                track.spawn((
                    Node { width: Val::Percent(fill * 100.0), height: Val::Percent(100.0), ..default() },
                    BackgroundColor(fill_color),
                ));
            });

            if snap.inventory_items.is_empty() {
                parent.spawn((
                    Text::new("Hold is empty"),
                    TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY_SMALL), ..default() },
                    TextColor(theme::ThemeColors::TEXT_MUTED),
                ));
            } else {
                parent.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(theme::ThemeSpacing::XS),
                    margin: UiRect::top(Val::Px(theme::ThemeSpacing::XS)),
                    ..default()
                }).with_children(|list| {
                    for (name, count) in &snap.inventory_items {
                        list.spawn((
                            Node {
                                flex_direction: FlexDirection::Row,
                                justify_content: JustifyContent::SpaceBetween,
                                align_items: AlignItems::Center,
                                padding: UiRect::axes(Val::Px(theme::ThemeSpacing::MD), Val::Px(theme::ThemeSpacing::SM)),
                                ..default()
                            },
                            BackgroundColor(theme::ThemeColors::BG_CARD),
                        )).with_children(|row| {
                            row.spawn((
                                Text::new(name.clone()),
                                TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY), ..default() },
                                TextColor(theme::ThemeColors::TEXT_PRIMARY),
                            ));
                            row.spawn((
                                Text::new(format!("{}", count)),
                                TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY), ..default() },
                                TextColor(theme::ThemeColors::ACCENT_CYAN),
                            ));
                        });
                    }
                });
            }

            // ---- SURVEY ----
            section_header(parent, "Survey");
            parent.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(theme::ThemeSpacing::MD),
                ..default()
            }).with_children(|row| {
                let tiles = [
                    ("WRECKS", snap.wrecks_found),
                    ("CAVES", snap.caves_found),
                    ("SETTLEMENTS", snap.settlements_found),
                ];
                for (label, count) in tiles {
                    row.spawn((
                        Node {
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            row_gap: Val::Px(theme::ThemeSpacing::XS),
                            padding: UiRect::axes(Val::Px(theme::ThemeSpacing::SM), Val::Px(theme::ThemeSpacing::MD)),
                            ..default()
                        },
                        BackgroundColor(theme::ThemeColors::BG_CARD),
                    )).with_children(|tile| {
                        tile.spawn((
                            Text::new(format!("{}", count)),
                            TextFont { font_size: FontSize::Px(theme::ThemeFonts::H3), ..default() },
                            TextColor(theme::ThemeColors::TEXT_PRIMARY),
                        ));
                        tile.spawn((
                            Text::new(label),
                            TextFont { font_size: FontSize::Px(theme::ThemeFonts::TINY), ..default() },
                            TextColor(theme::ThemeColors::TEXT_MUTED),
                        ));
                    });
                }
            });

            // ---- LOGS ----
            if !snap.logs_found.is_empty() {
                section_header(parent, "Recovered Logs");
                for log in &snap.logs_found {
                    parent.spawn((
                        Text::new(log.clone()),
                        TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY_SMALL), ..default() },
                        TextColor(theme::ThemeColors::TEXT_SECONDARY),
                    ));
                }
            }

        });
    });
}

/// The local map's fixed reference point — the current system's
/// local_center, or the player's own position if there's no real system
/// loaded (a blind warp landed in empty space, see execute_warp_jump).
/// Shared by build_map_snapshot (render) and map_click_system (the inverse
/// screen-to-world conversion) so both agree on the same frame.
fn current_map_center(world_data: &MapWorldData, player_pos: Vec2) -> Vec2 {
    world_data.streaming.loaded_system
        .and_then(|id| world_data.galaxy_map.systems.iter().find(|s| s.id == id))
        .map(|s| s.local_center)
        .unwrap_or(player_pos)
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
    let current_system = world_data.streaming.loaded_system;
    let map_center = current_map_center(world_data, player_pos);

    // sim.ships now spans every Hot/Warm system, not just "the one system
    // that exists" like before the galaxy map — an off-screen ship ticking
    // in a Warm neighbor is real but belongs to a totally different local
    // space, so plotting its raw position on THIS system's local map would
    // be meaningless (or just pile up at the map edge). Only this system's
    // own off-screen ships belong on the local tactical view.
    let mut hostiles: Vec<Vec2> = world_data.ai_ship_query.iter().map(|t| t.translation.truncate()).collect();
    hostiles.extend(
        world_data.sim.ships.iter()
            .filter(|s| !s.spawned && s.behavior != crate::ai_ship::components::SimBehavior::Dead && Some(s.system_id) == current_system)
            .map(|s| s.position)
    );

    let bounties: Vec<Vec2> = crate::contracts::bounty_nav::active_bounty_positions_with_id(
        &world_data.contract_state, &world_data.sim, &world_data.bounty_ship_query,
    ).into_iter()
        .filter(|(_, id)| world_data.sim.ships.iter().find(|s| s.bounty_id == Some(*id)).is_none_or(|s| Some(s.system_id) == current_system))
        .map(|(pos, _)| pos).collect();

    MapSnapshot {
        panel_size,
        player_pos,
        map_center,
        pending_target,
        current_fuel: fuel_state.current_fuel,
        stations: world_data.stations.sites.iter()
            .map(|s| (s.pos, s.name.clone(), crate::world::home_base::station_accent(s.kind)))
            .collect(),
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
        system_name: current_system
            .and_then(|id| world_data.galaxy_map.systems.iter().find(|s| s.id == id))
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "Uncharted space".to_string()),
        nearest_station: world_data.stations.closest(player_pos)
            .map(|s| (s.name.clone(), player_pos.distance(s.pos))),
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
    mut view_mode: ResMut<MapViewMode>,
) {
    if !keyboard.just_pressed(KeyCode::KeyM) {
        return;
    }

    if let Ok(entity) = existing.single() {
        commands.entity(entity).despawn();
        virtual_time.unpause();
        return;
    }

    // Always opens on the local view — Tab switches to the galaxy view
    // while open (see toggle_galaxy_view).
    *view_mode = MapViewMode::Local;

    // Full-screen map pauses the simulation — ships, timers, damage, fuel
    // burn etc. all read Time<Virtual>, so this freezes everything except
    // UI input (which doesn't depend on Time) with no extra state juggling.
    virtual_time.pause();

    let player_pos = player_query.single().map(|t| t.translation.truncate()).unwrap_or(Vec2::ZERO);
    let snapshot = build_map_snapshot(&windows, player_pos, pending.0, &fuel_state, &discovered, &inventory, &statistics, &world_data);
    spawn_map_overlay(&mut commands, &snapshot);
}

/// Tab, while the M-key overlay is open, flips between the local tactical
/// view and the galaxy-scale starmap — rebuilds whichever view is now
/// active from scratch, same pattern as toggle_map_overlay.
fn toggle_galaxy_view(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    existing: Query<Entity, With<MapOverlay>>,
    mut view_mode: ResMut<MapViewMode>,
    galaxy_map: Res<crate::celestial::resources::GalaxyMap>,
    streaming: Res<crate::celestial::resources::SystemStreamingManager>,
    pending_galaxy: Res<crate::celestial::resources::PendingGalaxyWarpTarget>,
    discovered: Res<DiscoveredLocations>,
    inventory: Res<Inventory>,
    statistics: Res<Statistics>,
    player_query: Query<&Transform, With<Ship>>,
    world_data: MapWorldData,
    pending: Res<PendingWarpTarget>,
    fuel_state: Res<FuelState>,
    windows: Query<&Window>,
) {
    let Ok(entity) = existing.single() else { return };
    if !keyboard.just_pressed(KeyCode::Tab) {
        return;
    }

    commands.entity(entity).despawn();
    *view_mode = match *view_mode {
        MapViewMode::Local => MapViewMode::Galaxy,
        MapViewMode::Galaxy => MapViewMode::Local,
    };

    match *view_mode {
        MapViewMode::Local => {
            let player_pos = player_query.single().map(|t| t.translation.truncate()).unwrap_or(Vec2::ZERO);
            let snapshot = build_map_snapshot(&windows, player_pos, pending.0, &fuel_state, &discovered, &inventory, &statistics, &world_data);
            spawn_map_overlay(&mut commands, &snapshot);
        }
        MapViewMode::Galaxy => {
            let (win_w, win_h) = windows.single().map(|w| (w.width(), w.height())).unwrap_or((1280.0, 800.0));
            let panel_size = (win_w.min(win_h) * 0.85).max(200.0);
            spawn_galaxy_map_overlay(&mut commands, &galaxy_map, &streaming, pending_galaxy.0, &fuel_state, panel_size);
        }
    }
}

/// World-to-pixel projection for the galaxy view, using StarSystemDef's
/// abstract galaxy_pos (map-only, not a real Transform coordinate — see its
/// doc comment) instead of world_to_map_px's real-Transform local scale.
fn galaxy_to_map_px(galaxy_pos: Vec2, panel_size: f32) -> (f32, f32) {
    let half = panel_size / 2.0;
    let range = crate::celestial::galaxy::GALAXY_RADIUS;
    let x = half + (galaxy_pos.x / range) * half;
    let y = half - (galaxy_pos.y / range) * half;
    (x.clamp(0.0, panel_size), y.clamp(0.0, panel_size))
}

/// Galaxy-scale starmap. Unknown systems render nothing at all (true fog of
/// war); Located systems show as a dim unlabeled pip (revealed passively by
/// proximity — celestial::galaxy::passive_proximity_discovery — or by a
/// blind warp landing near one); Visited systems show colored by
/// danger_tier. Clickable ANYWHERE, not just on discovered pips — one
/// continuous space, not a picker limited to what you've already found;
/// see galaxy_map_click_system.
fn spawn_galaxy_map_overlay(
    commands: &mut Commands,
    galaxy_map: &crate::celestial::resources::GalaxyMap,
    streaming: &crate::celestial::resources::SystemStreamingManager,
    pending: Option<crate::celestial::resources::GalaxyWarpTarget>,
    fuel_state: &FuelState,
    panel_size: f32,
) {
    use crate::celestial::resources::SystemDiscovery;

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
            }, BackgroundColor(theme::ThemeColors::BG_VOID), ZIndex(50)),
        MapOverlay,
    )).with_children(|parent| {
        parent.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            flex_shrink: 0.0,
            ..default()
        }).with_children(|col| {
            col.spawn((
                Node {
                    width: Val::Px(panel_size),
                    height: Val::Px(panel_size),
                    position_type: PositionType::Relative,
                    overflow: Overflow::clip(),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(theme::ThemeColors::HUD_BG),
                Interaction::None,
                GalaxyMapPanel,
            )).with_children(|map| {
                for sys in &galaxy_map.systems {
                    let (size, color) = match sys.discovery {
                        SystemDiscovery::Unknown => continue,
                        SystemDiscovery::Located => (6.0, theme::ThemeColors::TEXT_MUTED),
                        SystemDiscovery::Visited => {
                            // Colored by WHOSE territory it is, not a
                            // generic danger bucket — see
                            // ai_ship::components::faction_map_color.
                            let color = match sys.faction {
                                None => Color::srgb(0.3, 0.9, 1.0), // Haven
                                Some(faction) => crate::ai_ship::components::faction_map_color(faction),
                            };
                            (10.0, color)
                        }
                    };
                    let (x, y) = galaxy_to_map_px(sys.galaxy_pos, panel_size);
                    map.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(x - size / 2.0),
                            top: Val::Px(y - size / 2.0),
                            width: Val::Px(size),
                            height: Val::Px(size),
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                }

                // "You are here" — current position, whether it's a real
                // system or an open patch of blind-warped space.
                let (px, py) = galaxy_to_map_px(streaming.current_galaxy_pos, panel_size);
                map.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(px - 4.0),
                        top: Val::Px(py - 4.0),
                        width: Val::Px(8.0),
                        height: Val::Px(8.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.3, 0.9, 1.0)),
                ));

                // Pending target crosshair, wherever it resolves to —
                // a known system's position, or the raw blind-click point.
                if let Some(target) = pending {
                    if let Some(pos) = crate::celestial::warp::target_galaxy_pos(galaxy_map, target) {
                        let (tx, ty) = galaxy_to_map_px(pos, panel_size);
                        map.spawn((
                            Node { position_type: PositionType::Absolute, left: Val::Px(tx - 6.0), top: Val::Px(ty - 1.0), width: Val::Px(12.0), height: Val::Px(2.0), ..default() },
                            BackgroundColor(Color::srgb(1.0, 0.85, 0.1)),
                        ));
                        map.spawn((
                            Node { position_type: PositionType::Absolute, left: Val::Px(tx - 1.0), top: Val::Px(ty - 6.0), width: Val::Px(2.0), height: Val::Px(12.0), ..default() },
                            BackgroundColor(Color::srgb(1.0, 0.85, 0.1)),
                        ));
                    }
                }
            });
            col.spawn((Text::new("GALAXY MAP — Tab: local view | M: close"), TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() }, TextColor(theme::ThemeColors::TEXT_MUTED)));

            // Faction color reference — Visited systems are colored by
            // whose territory they are (see faction_map_color); Located
            // ones show as a dim pip since the faction isn't known yet.
            col.spawn(Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(12.0),
                row_gap: Val::Px(4.0),
                width: Val::Px(panel_size),
                ..default()
            }).with_children(|legend| {
                use crate::ai_ship::components::{faction_map_color, AiShipType};
                let entries: &[(Color, &str)] = &[
                    (Color::srgb(0.3, 0.9, 1.0), "Haven"),
                    (faction_map_color(AiShipType::RustSwarm), "Rust Swarm"),
                    (faction_map_color(AiShipType::Drowned), "Drowned"),
                    (faction_map_color(AiShipType::Leviathan), "Leviathan"),
                    (faction_map_color(AiShipType::AbyssalCult), "Abyssal Cult"),
                    (faction_map_color(AiShipType::GlassEye), "Glass Eye"),
                    (faction_map_color(AiShipType::Blackwater), "Blackwater"),
                    (faction_map_color(AiShipType::PressureKing), "Pressure King"),
                    (faction_map_color(AiShipType::IronTide), "Iron Tide"),
                    (faction_map_color(AiShipType::Dreadnought), "Dreadnought"),
                    (faction_map_color(AiShipType::VoidTitan), "Void Titan"),
                    (theme::ThemeColors::TEXT_MUTED, "Located (unknown)"),
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
                        row.spawn((Text::new(*label), TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() }, TextColor(theme::ThemeColors::TEXT_SECONDARY)));
                    });
                }
            });
        });

        // Sidebar: simple discovery-progress readout for now — per-system
        // detail (name/faction/danger/click-to-warp) lands in Phase 5 once
        // discovery mechanics exist to gate it.
        parent.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                width: Val::Px(260.0),
                height: Val::Percent(90.0),
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(6.0),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme::ThemeColors::BG_PANEL),
        )).with_children(|parent| {
            parent.spawn((Text::new("GALAXY"), TextFont { font_size: FontSize::Px(theme::ThemeFonts::H2), ..default() }, TextColor(theme::ThemeColors::TEXT_TITLE)));

            let visited = galaxy_map.systems.iter().filter(|s| s.discovery == SystemDiscovery::Visited).count();
            let located = galaxy_map.systems.iter().filter(|s| s.discovery == SystemDiscovery::Located).count();
            let total = galaxy_map.systems.len();
            let unknown = total - visited - located;

            for (label, count, color) in [
                ("Visited", visited, theme::ThemeColors::ACCENT_GREEN),
                ("Located", located, theme::ThemeColors::TEXT_SECONDARY),
                ("Unknown", unknown, theme::ThemeColors::TEXT_MUTED),
            ] {
                parent.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(6.0), ..default() })
                    .with_children(|row| {
                        row.spawn((Text::new(label), TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY), ..default() }, TextColor(color)));
                        row.spawn((Text::new(format!("{}", count)), TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY), ..default() }, TextColor(theme::ThemeColors::TEXT_PRIMARY)));
                    });
            }

            // Cost/charge preview for the pending target, if any — same
            // numbers celestial::warp::warp_input_system will actually use.
            if let Some(target) = pending {
                if let Some(pos) = crate::celestial::warp::target_galaxy_pos(galaxy_map, target) {
                    let dist = streaming.current_galaxy_pos.distance(pos);
                    let t = dist / crate::celestial::galaxy::GALAXY_RADIUS;
                    let charge = crate::celestial::warp::interstellar_charge_time(t);
                    let fuel = crate::celestial::warp::interstellar_fuel_cost(t);
                    let name = match target {
                        crate::celestial::resources::GalaxyWarpTarget::System(id) => galaxy_map.systems.iter().find(|s| s.id == id).map(|s| s.name.clone()).unwrap_or_default(),
                        crate::celestial::resources::GalaxyWarpTarget::BlindPoint(_) => "Uncharted space".to_string(),
                    };
                    parent.spawn((
                        Text::new("WARP TARGET"),
                        TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() },
                        TextColor(theme::ThemeColors::TEXT_MUTED),
                        Node { margin: UiRect::top(Val::Px(theme::ThemeSpacing::SM)), ..default() },
                    ));
                    parent.spawn((Text::new(name), TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY), ..default() }, TextColor(theme::ThemeColors::TEXT_PRIMARY)));
                    let fuel_color = if fuel_state.current_fuel < fuel { theme::ThemeColors::ACCENT_ORANGE } else { theme::ThemeColors::TEXT_SECONDARY };
                    parent.spawn((Text::new(format!("Charge: {:.0}s   Fuel: {:.0}", charge, fuel)), TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() }, TextColor(fuel_color)));
                    parent.spawn((Text::new("Press V to jump"), TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() }, TextColor(theme::ThemeColors::TEXT_MUTED)));
                }
            }

            parent.spawn((Text::new("Press M to close"), TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() }, TextColor(theme::ThemeColors::TEXT_MUTED)));
        });
    });
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

    let player_pos = player_query.single().map(|t| t.translation.truncate()).unwrap_or(Vec2::ZERO);
    let map_center = current_map_center(&world_data, player_pos);
    let target = map_center + Vec2::new(norm.x * 2.0 * MAP_WORLD_RANGE, -norm.y * 2.0 * MAP_WORLD_RANGE);
    pending.0 = Some(target);

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

/// Handles clicks on the galaxy map panel — clickable ANYWHERE, not just on
/// discovered pips (one continuous space, see spawn_galaxy_map_overlay's
/// doc comment). If the click lands within SNAP_TOLERANCE of a system
/// that's already Located/Visited (i.e. a pip you can actually see), it
/// targets that system precisely; otherwise it's a blind point — whether
/// anything is actually there is only revealed on arrival
/// (celestial::warp::execute_warp_jump).
fn galaxy_map_click_system(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    map_panel: Query<(&ComputedNode, &bevy::ui::UiGlobalTransform), With<GalaxyMapPanel>>,
    existing: Query<Entity, With<MapOverlay>>,
    windows: Query<&Window>,
    mut pending: ResMut<crate::celestial::resources::PendingGalaxyWarpTarget>,
    galaxy_map: Res<crate::celestial::resources::GalaxyMap>,
    streaming: Res<crate::celestial::resources::SystemStreamingManager>,
    fuel_state: Res<FuelState>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    use crate::celestial::resources::{GalaxyWarpTarget, SystemDiscovery};

    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok((node, transform)) = map_panel.single() else { return };
    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position().map(|p| p * window.scale_factor()) else { return };
    let Some(norm) = node.normalize_point(*transform, cursor_pos) else { return };
    if norm.x.abs() > 0.5 || norm.y.abs() > 0.5 {
        return; // click landed outside the map panel (e.g. on the sidebar)
    }

    let range = crate::celestial::galaxy::GALAXY_RADIUS;
    let clicked_pos = Vec2::new(norm.x * 2.0 * range, -norm.y * 2.0 * range);

    // Snap onto a VISIBLE pip if the click is close to one — precision aid
    // for known systems. Undiscovered systems don't get this treatment
    // (there's no pip to aim at), so clicking near one still just sets a
    // blind point; the arrival-time snap in execute_warp_jump is what
    // reveals it as a surprise.
    let snapped = galaxy_map.systems.iter()
        .filter(|s| s.discovery != SystemDiscovery::Unknown && s.galaxy_pos.distance(clicked_pos) <= crate::celestial::galaxy::SNAP_TOLERANCE)
        .min_by(|a, b| a.galaxy_pos.distance(clicked_pos).partial_cmp(&b.galaxy_pos.distance(clicked_pos)).unwrap());

    let (target, desc) = match snapped {
        Some(sys) => (GalaxyWarpTarget::System(sys.id), sys.name.clone()),
        None => (GalaxyWarpTarget::BlindPoint(clicked_pos), "uncharted space".to_string()),
    };
    pending.0 = Some(target);

    let dist = streaming.current_galaxy_pos.distance(clicked_pos);
    notifications.write(ShowNotification {
        message: format!("Warp target set — {} ({:.0} units away).", desc, dist),
        notification_type: NotificationType::Info,
        duration: 2.5,
    });

    if let Ok(entity) = existing.single() {
        commands.entity(entity).despawn();
    }
    let (win_w, win_h) = windows.single().map(|w| (w.width(), w.height())).unwrap_or((1280.0, 800.0));
    let panel_size = (win_w.min(win_h) * 0.85).max(200.0);
    spawn_galaxy_map_overlay(&mut commands, &galaxy_map, &streaming, pending.0, &fuel_state, panel_size);
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

        // Actions container — clickable buttons (keyboard shortcuts still work:
        // Enter = New Expedition, L+1/2/3/0 = Load).
        parent.spawn((Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(ThemeSpacing::MD),
                ..default()
            })).with_children(|actions| {
            use menu_buttons::{spawn_menu_button, MenuAction};

            spawn_menu_button(actions, "NEW EXPEDITION", Some("Enter"), ThemeColors::ACCENT_BLUE, MenuAction::NewGame);

            // One Load button per existing save slot.
            let slots = crate::meta::get_save_slots();
            for (slot, info) in &slots {
                if let Some(info) = info {
                    let name = if *slot == 99 { "Auto".to_string() } else { format!("Slot {}", slot + 1) };
                    let key = if *slot == 99 { "L+0" } else { match slot { 0 => "L+1", 1 => "L+2", 2 => "L+3", _ => "L+?" } };
                    let time_min = (info.play_time / 60.0) as i32;
                    let time_sec = (info.play_time % 60.0) as i32;
                    let label = format!("LOAD — {} ({} · {}:{:02})", name, format_range_km(info.depth), time_min, time_sec);
                    spawn_menu_button(actions, &label, Some(key), ThemeColors::ACCENT_GREEN, MenuAction::LoadSlot(*slot));
                }
            }

            spawn_menu_button(actions, "SETTINGS", None, ThemeColors::ACCENT_PURPLE, MenuAction::OpenSettings);
            spawn_menu_button(actions, "QUIT", None, ThemeColors::ACCENT_RED, MenuAction::QuitToDesktop);
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
                (format!("Max Distance     {}", format_range_km(statistics.max_depth_reached)), ThemeColors::ACCENT_BLUE),
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

        // Return button (Enter also works — see game_over_input).
        parent.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            }).with_children(|actions| {
            menu_buttons::spawn_menu_button(
                actions,
                "RETURN TO MAIN MENU",
                Some("Enter"),
                ThemeColors::ACCENT_BLUE,
                menu_buttons::MenuAction::ReturnToMainMenu,
            );
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
                "Haven: {}  Hull: {}%  Power: {:.0}/{:.0}",
                format_range_km(depth_state.current_depth), hull_pct,
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

        use menu_buttons::{spawn_chip_button, spawn_menu_button, MenuAction};

        // Spacer between the status readout and the buttons.
        parent.spawn(Node { height: Val::Px(theme::ThemeSpacing::MD), ..default() });

        // Button column (all keyboard shortcuts below still work too).
        parent.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(theme::ThemeSpacing::MD),
                ..default()
            }).with_children(|actions| {
            use theme::ThemeColors;

            spawn_menu_button(actions, "RESUME", Some("Esc"), ThemeColors::ACCENT_GREEN, MenuAction::Resume);

            let slots = crate::meta::get_save_slots();

            // Save row: one chip per slot (1/2/3).
            actions.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(theme::ThemeSpacing::MD),
                    ..default()
                }).with_children(|row| {
                row.spawn((Text::new("SAVE"), TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::TEXT_SECONDARY)));
                for slot in 0u32..3 {
                    spawn_chip_button(row, &format!("{}", slot + 1), ThemeColors::ACCENT_YELLOW, MenuAction::SaveSlot(slot));
                }
            });

            // Load row: one chip per slot that actually has a save.
            let has_saves = slots.iter().any(|(_, info)| info.is_some());
            if has_saves {
                actions.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(theme::ThemeSpacing::MD),
                        ..default()
                    }).with_children(|row| {
                    row.spawn((Text::new("LOAD"), TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY), ..default() }, TextColor(ThemeColors::TEXT_SECONDARY)));
                    for (slot, info) in &slots {
                        if info.is_some() {
                            let label = if *slot == 99 { "Auto".to_string() } else { format!("{}", slot + 1) };
                            spawn_chip_button(row, &label, ThemeColors::ACCENT_BLUE, MenuAction::LoadSlot(*slot));
                        }
                    }
                });
            }

            spawn_menu_button(actions, "SETTINGS", None, ThemeColors::ACCENT_PURPLE, MenuAction::OpenSettings);
            spawn_menu_button(actions, "QUIT TO MAIN MENU", None, ThemeColors::ACCENT_RED, MenuAction::QuitToMainMenu);
        });

        // Hint
        parent.spawn((Text::new("Esc: Resume  •  P: Modules  •  F1-F3 / L+1-3: quick save & load"), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::srgb(0.5, 0.5, 0.5)), Node { margin: UiRect::top(Val::Px(theme::ThemeSpacing::MD)), ..default() }));
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

/// Universal quick-save / quick-load: F5 saves, F9 loads (slot 0). Unlike
/// `save_load_input` (multi-slot, pause-menu only) this runs during normal play
/// so you never have to open a menu — the common-case save/reload loop.
fn quick_save_load_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut save_events: MessageWriter<SaveGameRequest>,
    mut load_events: MessageWriter<LoadGameRequest>,
) {
    if keyboard.just_pressed(KeyCode::F5) {
        save_events.write(SaveGameRequest { slot: 0 });
    }
    if keyboard.just_pressed(KeyCode::F9) {
        load_events.write(LoadGameRequest { slot: 0 });
    }
}

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
    // Without<OwnedByAiShip>: player-only — AI ships carry Module entities
    // too, unscoped this panel would list AI ship modules alongside the
    // player's own.
    module_query: Query<(Entity, &Module), Without<crate::ai_ship::components::OwnedByAiShip>>,
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
            }, BackgroundColor(theme::ThemeColors::BG_PANEL), ZIndex(110)),
        ModulePanelOverlay,
        ModuleListSelection(0),
    )).with_children(|parent| {
        parent.spawn((Text::new("MODULE MANAGEMENT"), TextFont { font_size: FontSize::Px(theme::ThemeFonts::H2), ..default() }, TextColor(theme::ThemeColors::TEXT_TITLE)));

        let mut row_index: usize = 0;
        for cat in ModuleCategory::ALL {
            let Some(modules) = by_cat.get(cat) else { continue };

            // Category header
            parent.spawn((Text::new(format!("--- {} ---", cat.name())), TextFont { font_size: FontSize::Px(theme::ThemeFonts::H3), ..default() }, TextColor(theme::ThemeColors::ACCENT_YELLOW)));

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
                    theme::ThemeColors::STATUS_OK
                } else {
                    theme::ThemeColors::TEXT_MUTED
                };

                parent.spawn((
                    Node {
                        padding: UiRect::new(Val::Px(theme::ThemeSpacing::SM), Val::Px(theme::ThemeSpacing::SM), Val::Px(theme::ThemeSpacing::XS), Val::Px(theme::ThemeSpacing::XS)),
                        margin: UiRect::bottom(Val::Px(theme::ThemeSpacing::XS)),
                        ..default()
                    },
                    BackgroundColor(if row_index == 0 { theme::ThemeColors::BG_ELEVATED } else { theme::ThemeColors::BG_CARD }),
                )).with_children(|card| {
                    card.spawn((
                        (Text::new(&text), TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY), ..default() }, TextColor(color)),
                        ModuleListItem(entity),
                    ));
                });
                row_index += 1;
            }
        }

        if row_index == 0 {
            parent.spawn((Text::new("No modules installed"), TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY), ..default() }, TextColor(theme::ThemeColors::TEXT_MUTED)));
        }

        parent.spawn((Text::new("Up/Down: Select  Enter: Toggle  P: Close"), TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() }, TextColor(theme::ThemeColors::TEXT_MUTED)));
    });
}

/// Handles Up/Down/Enter input on the module panel
fn module_panel_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut panel_query: Query<&mut ModuleListSelection, With<ModulePanelOverlay>>,
    mut item_query: Query<(&ModuleListItem, &mut Text, &mut TextColor, &ChildOf)>,
    mut card_query: Query<&mut BackgroundColor>,
    mut module_query: Query<&mut Module>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let Ok(mut selection) = panel_query.single_mut() else { return };

    let items: Vec<Entity> = item_query.iter().map(|(item, ..)| item.0).collect();
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

    // Rebuild text for all rows, and highlight the selected row's card background
    let new_idx = selection.0;
    for (i, (item, mut text, mut text_color, card)) in item_query.iter_mut().enumerate() {
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
            theme::ThemeColors::STATUS_OK
        } else {
            theme::ThemeColors::TEXT_MUTED
        };
        if let Ok(mut bg) = card_query.get_mut(card.0) {
            bg.0 = if i == new_idx { theme::ThemeColors::BG_ELEVATED } else { theme::ThemeColors::BG_CARD };
        }
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

/// Every tradeable item, in stable menu order.
const TRADE_GOODS: [ItemType; 7] = [
    ItemType::ScrapMetal,
    ItemType::Crystal,
    ItemType::BioSample,
    ItemType::FuelCell,
    ItemType::RareAlloy,
    ItemType::AncientArtifact,
    ItemType::AmmoCrate,
];

/// Sell-cargo choices at a station: index 0 = everything, then one entry
/// per item stack actually held (stable order). Lets the player dump
/// scrap at a Trade Hub while holding artifacts back for the Research
/// Outpost, instead of the old all-or-nothing dump.
fn sell_choices(inventory: &Inventory) -> Vec<Option<ItemType>> {
    let mut choices = vec![None];
    for item in TRADE_GOODS {
        if inventory.items.get(&item).copied().unwrap_or(0) > 0 {
            choices.push(Some(item));
        }
    }
    choices
}

/// Description + unit cost for the current buy-goods choice.
fn buy_row(inventory: &Inventory, station_idx: usize, choice_idx: usize, market: &MarketEvents) -> (String, u32, ItemType) {
    let item = TRADE_GOODS[choice_idx % TRADE_GOODS.len()];
    let price = crate::resources::live_item_buy_price(market, station_idx, item);
    let held = inventory.items.get(&item).copied().unwrap_or(0);
    (
        format!("{} @ {}c each (hold: {})  (Left/Right: browse, Enter: buy 1)", item.name(), price, held),
        price,
        item,
    )
}

/// Description + value for the current sell-cargo choice at this station.
fn sell_row(inventory: &Inventory, station_idx: usize, choice: Option<ItemType>, market: &MarketEvents) -> (String, u32) {
    match choice {
        None => {
            let mut total = 0u32;
            for (item, count) in &inventory.items {
                total += crate::resources::live_item_price(market, station_idx, *item) * count;
            }
            (format!("ALL cargo — {}c  (Left/Right: pick a single stack)", total), total)
        }
        Some(item) => {
            let count = inventory.items.get(&item).copied().unwrap_or(0);
            let price = crate::resources::live_item_price(market, station_idx, item);
            let value = price * count;
            let tag = if market.multiplier(station_idx, item) > 1.0 { "  ★ SHORTAGE" } else { "" };
            (format!("{}x {} @ {}c each = {}c{}  (Left/Right: cycle)", count, item.name(), price, value, tag), value)
        }
    }
}

fn get_docking_services(
    hull_state: &HullState,
    oxygen_state: &OxygenState,
    fuel_state: &FuelState,
    weapon_query: &Query<(&Weapon, Option<&crate::building::customization::tuning::SelectedAmmo>), Without<Creature>>,
    crew_count: usize,
    total_berths: u32,
    inventory: &Inventory,
    station_idx: usize,
    market: &MarketEvents,
) -> Vec<DockingService> {
    // Station identity: repairs/fuel/ammo are cheaper at the right outpost
    // type (Mining/Refuel/Military — see world::station_types). These same
    // multipliers must be applied when charging in docking_menu_input.
    let discounts = crate::world::station_types::service_discounts(
        crate::world::station_types::station_type(station_idx),
    );

    let hull_damage = 1.0 - hull_state.hull_integrity;
    let hull_repair_full_cost = (hull_damage * 500.0 * discounts.hull_repair) as u32;
    let scrap_have = inventory.items.get(&ItemType::ScrapMetal).copied().unwrap_or(0);
    let scrap_usable = (hull_repair_full_cost / 50).min(scrap_have);
    let hull_repair_cost = hull_repair_full_cost.saturating_sub(scrap_usable * 50);

    let o2_missing = oxygen_state.max_oxygen - oxygen_state.current_oxygen;
    let o2_cost = (o2_missing * 2.0) as u32;

    // Count weapons that need ammo, and price each gun's refill by what it
    // has loaded — a railgun full of antimatter costs multiples of the same
    // railgun full of AP (combat::ammo_types::rearm_price).
    let mut ammo_needed = 0u32;
    let mut ammo_cost_raw = 0.0f32;
    for (weapon, selected) in weapon_query.iter() {
        if weapon.ammo < weapon.max_ammo {
            let short = weapon.max_ammo - weapon.ammo;
            ammo_needed += short;
            ammo_cost_raw += crate::combat::ammo_types::rearm_price(selected.map(|a| a.0), short);
        }
    }
    let ammo_cost = ammo_cost_raw as u32;

    let hire_full_cost = 200 + (crew_count as u32) * 50;
    let bio_have = inventory.items.get(&ItemType::BioSample).copied().unwrap_or(0);
    let bio_usable = (hire_full_cost / 60).min(bio_have);
    let hire_cost = hire_full_cost.saturating_sub(bio_usable * 60);

    // Sell value: count total sellable items at this station's prices
    let mut sell_value = 0u32;
    for (item_type, count) in &inventory.items {
        sell_value += crate::resources::live_item_price(market, station_idx, *item_type) * count;
    }

    let fuel_missing = fuel_state.max_fuel - fuel_state.current_fuel;
    let fuel_cost = (fuel_missing * 0.5 * discounts.fuel) as u32;

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
            description: sell_row(inventory, station_idx, None, market).0,
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
            name: "Buy Goods",
            description: buy_row(inventory, station_idx, 0, market).0,
            cost: 0, // Shown per-choice in the description
            available: true,
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
    weapon_query: Query<(&Weapon, Option<&crate::building::customization::tuning::SelectedAmmo>), Without<Creature>>,
    crew_query: Query<&CrewMember>,
    inventory: Res<Inventory>,
    currency: Res<Currency>,
    staffing_state: Res<StaffingState>,
    ship_query: Query<&Transform, With<Ship>>,
    market: Res<MarketEvents>,
    stations: Res<crate::world::home_base::SystemStations>,
) {
    let crew_count = crew_query.iter().count();
    let station_idx = ship_query.single().ok()
        .and_then(|t| stations.nearest_index(t.translation.truncate()))
        .unwrap_or(0);
    let services = get_docking_services(&hull_state, &oxygen_state, &fuel_state, &weapon_query, crew_count, staffing_state.total_berths, &inventory, station_idx, &market);

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
        DockingMenuSelection(0, 0),
    )).with_children(|parent| {
        let s_type = crate::world::station_types::station_type(station_idx);
        let title = format!(
            "{} — {}",
            crate::world::home_base::station_display_name(station_idx).to_uppercase(),
            crate::world::station_types::station_type_name(s_type).to_uppercase()
        );
        parent.spawn((Text::new(title), TextFont { font_size: FontSize::Px(theme::ThemeFonts::H1), ..default() }, TextColor(theme::ThemeColors::ACCENT_CYAN)));

        // Station identity subtitle — discounts were already silently baked
        // into displayed prices with nothing explaining why; this states it
        // plainly. discounts < 1.0 is a genuine price cut (service_discounts
        // multiplies cost), so only surface the ones actually below 1.0.
        let discounts = crate::world::station_types::service_discounts(
            crate::world::station_types::station_type(station_idx),
        );
        let mut perks = Vec::new();
        if discounts.fuel < 1.0 { perks.push(format!("Fuel -{:.0}%", (1.0 - discounts.fuel) * 100.0)); }
        if discounts.hull_repair < 1.0 { perks.push(format!("Repairs -{:.0}%", (1.0 - discounts.hull_repair) * 100.0)); }
        if discounts.ammo < 1.0 { perks.push(format!("Ammo -{:.0}%", (1.0 - discounts.ammo) * 100.0)); }
        if !perks.is_empty() {
            parent.spawn((
                Text::new(perks.join("  •  ")),
                TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() },
                TextColor(theme::ThemeColors::TEXT_MUTED),
            ));
        }

        parent.spawn((Text::new(format!("Credits: {}", currency.credits)), TextFont { font_size: FontSize::Px(theme::ThemeFonts::H2), ..default() }, TextColor(theme::ThemeColors::ACCENT_YELLOW)));

        // Cargo hold — was invisible inside this menu entirely (only
        // visible via the Map overlay, which doesn't even open while
        // docked) even though half this menu is about what to do with it.
        // Static snapshot at open time — the menu doesn't otherwise
        // rebuild live, matching how the rest of this screen already works.
        if !inventory.items.is_empty() {
            parent.spawn((
                Text::new("CARGO HOLD"),
                TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() },
                TextColor(theme::ThemeColors::TEXT_MUTED),
            ));
            parent.spawn(Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(theme::ThemeSpacing::SM),
                row_gap: Val::Px(theme::ThemeSpacing::XS),
                ..default()
            }).with_children(|cargo| {
                for (item_type, count) in inventory.items.iter() {
                    cargo.spawn((
                        Node {
                            padding: UiRect::new(Val::Px(theme::ThemeSpacing::SM), Val::Px(theme::ThemeSpacing::SM), Val::Px(theme::ThemeSpacing::XS), Val::Px(theme::ThemeSpacing::XS)),
                            ..default()
                        },
                        BackgroundColor(theme::ThemeColors::BG_CARD),
                    )).with_children(|card| {
                        card.spawn((
                            Text::new(format!("{} x{}", item_type.name(), count)),
                            TextFont { font_size: FontSize::Px(theme::ThemeFonts::BODY_SMALL), ..default() },
                            TextColor(theme::ThemeColors::TEXT_SECONDARY),
                        ));
                    });
                }
            });
        }
        parent.spawn((Text::new(""), TextFont { font_size: FontSize::Px(8.0), ..default() }, TextColor(Color::WHITE)));

        // Row order/numbering must stay exactly as-is — docking_menu_input
        // (below) matches on these indices by number throughout its ~550
        // lines, so this groups the EXISTING order visually rather than
        // reordering rows into cleaner categories (Repair Modules sits
        // between Sell/Buy in the source order, hence "Trade & Upkeep"
        // rather than a pure "Trade" label).
        for (group_start, group_label) in [(0usize, "Services"), (5, "Trade & Upkeep"), (8, "")] {
            if !group_label.is_empty() {
                parent.spawn((
                    Text::new(group_label.to_uppercase()),
                    TextFont { font_size: FontSize::Px(theme::ThemeFonts::CAPTION), ..default() },
                    TextColor(theme::ThemeColors::TEXT_MUTED),
                    Node { margin: UiRect::top(Val::Px(theme::ThemeSpacing::SM)), ..default() },
                ));
            }
            let group_end = match group_start { 0 => 5, 5 => 8, _ => services.len() };
            for i in group_start..group_end {
                let service = &services[i];
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
        }

        parent.spawn((Text::new(""), TextFont { font_size: FontSize::Px(8.0), ..default() }, TextColor(Color::WHITE)));

        parent.spawn((Text::new("Up/Down: Select | Left/Right: cargo choice | Enter: Purchase | ESC: Undock"), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::srgb(0.25, 0.25, 0.25))));
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
    mut weapon_query: Query<(&mut Weapon, Option<&crate::building::customization::tuning::SelectedAmmo>), Without<Creature>>,
    crew_query: Query<&CrewMember>,
    mut notifications: MessageWriter<ShowNotification>,
    mut next_state: ResMut<NextState<GameState>>,
    mut hull_query: Query<&mut HullSegment>,
    staffing_state: Res<StaffingState>,
    mut module_query: Query<&mut Module>,
    ship_query: Query<&Transform, With<Ship>>,
    market: Res<MarketEvents>,
    stations: Res<crate::world::home_base::SystemStations>,
) {
    let (mut hull_state, mut oxygen_state, mut fuel_state, mut currency, mut inventory) = econ_state;
    let Ok(mut selection) = menu_query.single_mut() else { return };

    let station_idx = ship_query.single().ok()
        .and_then(|t| stations.nearest_index(t.translation.truncate()))
        .unwrap_or(0);
    // Must match the multipliers used for the displayed costs in
    // get_docking_services / the refresh block below.
    let discounts = crate::world::station_types::service_discounts(
        crate::world::station_types::station_type(station_idx),
    );

    let service_count = 9usize;
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

    // Left/Right cycles the cargo choice while Sell Cargo (5) or Buy
    // Goods (7) is selected — what to sell (ALL or one held stack) or
    // which good to buy, both priced for this station.
    if selection.0 == 5 || selection.0 == 7 {
        let len = if selection.0 == 5 {
            sell_choices(&inventory).len()
        } else {
            TRADE_GOODS.len()
        };
        if selection.1 >= len {
            selection.1 = 0;
        }
        if keyboard.just_pressed(KeyCode::ArrowRight) {
            selection.1 = (selection.1 + 1) % len;
            changed = true;
        }
        if keyboard.just_pressed(KeyCode::ArrowLeft) {
            selection.1 = (selection.1 + len - 1) % len;
            changed = true;
        }
    }

    if keyboard.just_pressed(KeyCode::Enter) {
        let crew_count = crew_query.iter().count();
        let weapon_read_query_hack: Vec<_> = weapon_query.iter()
            .map(|(w, sel)| (w.ammo, w.max_ammo, sel.map(|a| a.0)))
            .collect();

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
                let full_cost = (hull_damage * 500.0 * discounts.hull_repair) as u32;
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
                        let cost = (remaining_missing * 0.5 * discounts.fuel) as u32;
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
                let mut full_price = 0.0f32;
                for &(ammo, max_ammo, loaded) in &weapon_read_query_hack {
                    if ammo < max_ammo {
                        let short = max_ammo - ammo;
                        ammo_needed += short;
                        full_price += crate::combat::ammo_types::rearm_price(loaded, short);
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
                    // Crates supply ROUNDS, not credits, so they knock the
                    // same fraction off the bill as off the count. A crate is
                    // therefore worth more against a magazine of antimatter
                    // than against one of AP, which is the right way round.
                    let unpaid = remaining_ammo as f32 / ammo_needed as f32;
                    let cost = (full_price * unpaid * discounts.ammo) as u32;
                    if remaining_ammo > 0 && currency.credits < cost {
                        notifications.write(ShowNotification {
                            message: format!("Not enough credits for full rearm (need {}c)", cost),
                            notification_type: NotificationType::Warning,
                            duration: 2.0,
                        });
                    } else {
                        currency.credits -= cost;
                        for (mut weapon, _) in weapon_query.iter_mut() {
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
                // Sell Cargo — whatever the Left/Right choice says:
                // everything, or one specific stack.
                let choices = sell_choices(&inventory);
                let choice = choices.get(selection.1).copied().flatten();
                match choice {
                    None => {
                        let mut total_value = 0u32;
                        for (item_type, count) in &inventory.items {
                            total_value += crate::resources::live_item_price(&market, station_idx, *item_type) * count;
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
                    Some(item) => {
                        let count = inventory.items.get(&item).copied().unwrap_or(0);
                        if count == 0 {
                            notifications.write(ShowNotification {
                                message: format!("No {} in the hold.", item.name()),
                                notification_type: NotificationType::Info,
                                duration: 2.0,
                            });
                        } else {
                            let price = crate::resources::live_item_price(&market, station_idx, item);
                            let value = price * count;
                            inventory.remove_item(item, count);
                            currency.credits += value;
                            selection.1 = 0;
                            notifications.write(ShowNotification {
                                message: format!("Sold {}x {} for {}c ({}c each)", count, item.name(), value, price),
                                notification_type: NotificationType::Success,
                                duration: 3.0,
                            });
                            changed = true;
                        }
                    }
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
                // Buy Goods — one unit of the Left/Right choice
                let (_, price, item) = buy_row(&inventory, station_idx, selection.1, &market);
                if currency.credits < price {
                    notifications.write(ShowNotification {
                        message: format!("Not enough credits (need {}c, have {}c)", price, currency.credits),
                        notification_type: NotificationType::Warning,
                        duration: 2.0,
                    });
                } else if !inventory.add_item(item, 1) {
                    notifications.write(ShowNotification {
                        message: "Cargo hold full!".into(),
                        notification_type: NotificationType::Warning,
                        duration: 2.0,
                    });
                } else {
                    currency.credits -= price;
                    notifications.write(ShowNotification {
                        message: format!("Bought 1x {} (-{}c)", item.name(), price),
                        notification_type: NotificationType::Success,
                        duration: 2.0,
                    });
                    changed = true;
                }
            }
            8 => {
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
    let weapon_data: Vec<_> = weapon_query.iter()
        .map(|(w, sel)| (w.ammo, w.max_ammo, sel.map(|a| a.0)))
        .collect();

    let hull_damage = 1.0 - hull_state.hull_integrity;
    let hull_repair_full_cost = (hull_damage * 500.0 * discounts.hull_repair) as u32;
    let scrap_have = inventory.items.get(&ItemType::ScrapMetal).copied().unwrap_or(0);
    let scrap_usable = (hull_repair_full_cost / 50).min(scrap_have);
    let hull_repair_cost = hull_repair_full_cost.saturating_sub(scrap_usable * 50);
    let o2_missing = oxygen_state.max_oxygen - oxygen_state.current_oxygen;
    let o2_cost = (o2_missing * 2.0) as u32;
    let mut ammo_needed = 0u32;
    let mut ammo_cost_raw = 0.0f32;
    for &(ammo, max_ammo, loaded) in &weapon_data {
        if ammo < max_ammo {
            let short = max_ammo - ammo;
            ammo_needed += short;
            ammo_cost_raw += crate::combat::ammo_types::rearm_price(loaded, short);
        }
    }
    let ammo_cost = (ammo_cost_raw * discounts.ammo) as u32;
    let hire_full_cost = 200 + (crew_count as u32) * 50;
    let bio_have = inventory.items.get(&ItemType::BioSample).copied().unwrap_or(0);
    let bio_usable = (hire_full_cost / 60).min(bio_have);
    let hire_cost = hire_full_cost.saturating_sub(bio_usable * 60);
    let (sell_desc, sell_total) = {
        let choices = sell_choices(&inventory);
        let choice = choices.get(selection.1).copied().flatten();
        sell_row(&inventory, station_idx, choice, &market)
    };

    let fuel_missing = fuel_state.max_fuel - fuel_state.current_fuel;
    let fuel_cost = (fuel_missing * 0.5 * discounts.fuel) as u32;

    let new_idx = selection.0;
    let service_info: Vec<(&str, String, u32, bool)> = vec![
        ("Repair Hull", format!("Restore hull to 100% (Damage: {:.0}%) - ScrapMetal used first", hull_damage * 100.0), hull_repair_cost, hull_damage > 0.01),
        ("Refill Oxygen", format!("Refill O2 tanks ({:.0}/{:.0})", oxygen_state.current_oxygen, oxygen_state.max_oxygen), o2_cost, o2_missing > 1.0),
        ("Refuel", format!("Fill fuel tanks ({:.0}/{:.0}) - FuelCells used first", fuel_state.current_fuel, fuel_state.max_fuel), fuel_cost, fuel_missing > 1.0),
        ("Rearm Weapons", format!("Resupply {} rounds - AmmoCrates used first", ammo_needed), ammo_cost, ammo_needed > 0),
        ("Hire Crew", format!("Recruit crew ({}/{} berths) - BioSample used first", crew_count, staffing_state.total_berths), hire_cost, (crew_count as u32) < staffing_state.total_berths),
        ("Sell Cargo", sell_desc, 0, sell_total > 0),
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
        ("Buy Goods", buy_row(&inventory, station_idx, selection.1, &market).0, 0, true),
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
