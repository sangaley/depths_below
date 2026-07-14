use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use crate::building::customization::tuning::*;
use crate::building::GridOccupancy;
use crate::combat::ammo_types::KineticAmmoType;
use crate::components::{Module, ModuleType, Weapon};
use crate::resources::PowerState;
use super::framework::*;
use super::tooltip::Tooltip;

// ============================================================================
// WEAPON TUNING WINDOW
// Right-click a weapon while docked → sliders for velocity / fire rate /
// damage, ammo type selector for kinetics, live power + DPS readout.
// Editing is docked-only; windows are despawned on undock.
// ============================================================================

const ACCENT: Color = Color::srgb(0.95, 0.65, 0.25);
const SLIDER_BG: Color = Color::srgba(0.05, 0.06, 0.10, 1.0);
const SLIDER_FILL: Color = Color::srgba(0.35, 0.45, 0.65, 0.9);
const BASELINE_TICK: Color = Color::srgba(0.9, 0.9, 0.9, 0.35);
const AMMO_SELECTED_BG: Color = Color::srgba(0.30, 0.24, 0.12, 1.0);
const AMMO_BG: Color = Color::srgba(0.10, 0.12, 0.18, 1.0);

const ALL_AMMO: [KineticAmmoType; 9] = [
    KineticAmmoType::AP,
    KineticAmmoType::APHE,
    KineticAmmoType::HEFrag,
    KineticAmmoType::Incendiary,
    KineticAmmoType::EMPShell,
    KineticAmmoType::Flak,
    KineticAmmoType::HEAT,
    KineticAmmoType::HESH,
    KineticAmmoType::APFSDS,
];

#[derive(Component)]
pub struct TuningWindow {
    pub module_entity: Entity,
    pub module_type: ModuleType,
}

/// The clickable/draggable track of one slider
#[derive(Component)]
pub struct TuningSliderTrack {
    pub field: TuningField,
}

#[derive(Component)]
pub struct TuningSliderFill {
    pub field: TuningField,
}

#[derive(Component)]
pub struct TuningValueText {
    pub field: TuningField,
}

#[derive(Component)]
pub struct AmmoTypeButton {
    pub ammo: KineticAmmoType,
}

#[derive(Component)]
pub struct AmmoDescText;

#[derive(Component)]
pub struct PowerReadoutText;

#[derive(Component)]
pub struct ShipPowerBarFill;

#[derive(Component)]
pub struct ShipPowerText;

#[derive(Component)]
pub struct StatsReadoutText;

#[derive(Component)]
pub struct ResetTuningButton;

/// Which slider is currently being dragged (survives the cursor leaving the
/// track mid-drag, unlike hover-based Interaction alone).
#[derive(Resource, Default)]
pub struct ActiveSliderDrag(pub Option<TuningField>);

fn mult_to_fraction(mult: f32) -> f32 {
    (mult - TUNING_MIN) / (TUNING_MAX - TUNING_MIN)
}

// ============================================================================
// OPEN — right-click a weapon module while docked
// ============================================================================

pub fn right_click_open_tuning(
    mouse: Res<ButtonInput<MouseButton>>,
    occupancy: Res<GridOccupancy>,
    module_query: Query<(Entity, &Module), With<WeaponTuning>>,
    existing: Query<(Entity, &TuningWindow)>,
    parent_query: Query<&ChildOf>,
    floating: Query<Entity, With<FloatingWindow>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    ship_query: Query<&GlobalTransform, (With<crate::components::Ship>, Without<Camera>)>,
    mut commands: Commands,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }

    let Some(cursor_screen) = windows.single().ok().and_then(|w| w.cursor_position()) else { return };
    let Some(cursor_world) = camera_query.single().ok()
        .and_then(|(cam, gt)| cam.viewport_to_world_2d(gt, cursor_screen).ok())
    else { return };
    let Ok(ship_gt) = ship_query.single() else { return };

    // Grid cells are ship-local — transform the world-space cursor into the
    // ship's frame first (same math as the build ghost, see building/mod.rs).
    let local = ship_gt.rotation().inverse()
        * (Vec3::new(cursor_world.x, cursor_world.y, 0.0) - ship_gt.translation());
    let grid_pos = IVec2::new(
        (local.x / 66.0).round() as i32,
        ((local.y + 33.0) / 66.0).round() as i32,
    );

    let Some(&hit_entity) = occupancy.cells.get(&grid_pos) else { return };
    let Ok((entity, module)) = module_query.get(hit_entity) else { return };

    // One tuning window at a time — clicking another weapon retargets by
    // replacing the window instead of stacking them.
    for (content_entity, tw) in existing.iter() {
        if tw.module_entity == entity {
            return; // already open for this weapon
        }
        commands.entity(find_window_root(content_entity, &parent_query, &floating)).despawn();
    }

    spawn_tuning_window(&mut commands, entity, module.module_type, cursor_screen + Vec2::new(24.0, -12.0));
}

/// TuningWindow sits on the content node of a floating window — walk up to
/// the FloatingWindow root so despawning removes the whole frame.
fn find_window_root(
    content: Entity,
    parent_query: &Query<&ChildOf>,
    floating: &Query<Entity, With<FloatingWindow>>,
) -> Entity {
    let mut current = content;
    while let Ok(child_of) = parent_query.get(current) {
        current = child_of.parent();
        if floating.get(current).is_ok() {
            break;
        }
    }
    current
}

// ============================================================================
// SPAWN
// ============================================================================

fn spawn_tuning_window(
    commands: &mut Commands,
    module_entity: Entity,
    module_type: ModuleType,
    position: Vec2,
) {
    let title = format!("{:?} — WEAPON TUNING", module_type).to_uppercase();
    let content = spawn_floating_window(
        commands,
        &format!("tuning_{:?}", module_entity),
        &title,
        Vec2::new(340.0, 0.0), // height grows with content
        position,
    );

    commands.entity(content).insert(TuningWindow { module_entity, module_type });

    // AMMO SELECTOR — kinetics only
    if is_kinetic_weapon(module_type) {
        spawn_window_section(commands, content, "AMMUNITION");

        let grid = commands.spawn(
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(4.0),
                row_gap: Val::Px(4.0),
                margin: UiRect::vertical(Val::Px(4.0)),
                ..default()
            },
        ).id();

        for ammo in ALL_AMMO {
            let selected = ammo == KineticAmmoType::AP; // spawner default
            let btn = commands.spawn((
                Node {
                    padding: UiRect::new(Val::Px(7.0), Val::Px(7.0), Val::Px(3.0), Val::Px(3.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(if selected { AMMO_SELECTED_BG } else { AMMO_BG }),
                BorderColor::all(if selected { ACCENT } else { WindowStyle::BORDER_COLOR }),
                Interaction::None,
                Button,
                AmmoTypeButton { ammo },
                Tooltip { text: ammo.description().into(), detail: None },
            )).id();
            let label = commands.spawn((
                Text::new(ammo.name()),
                TextFont { font_size: FontSize::Px(10.0), ..default() },
                TextColor(if selected { ACCENT } else { WindowStyle::TEXT_COLOR }),
            )).id();
            commands.entity(btn).add_child(label);
            commands.entity(grid).add_child(btn);
        }
        commands.entity(content).add_child(grid);

        let desc = commands.spawn((
            Text::new(KineticAmmoType::AP.description()),
            TextFont { font_size: FontSize::Px(10.0), ..default() },
            TextColor(WindowStyle::TEXT_DIM),
            AmmoDescText,
        )).id();
        commands.entity(content).add_child(desc);
    }

    // SLIDERS
    spawn_window_section(commands, content, "TUNING");

    let mut fields = vec![
        (TuningField::FireRate, "FIRE RATE"),
        (TuningField::Damage, "DAMAGE"),
    ];
    // Lasers are instant beams — no projectile to speed up.
    if module_type != ModuleType::Laser {
        fields.insert(0, (TuningField::Velocity, velocity_label(module_type)));
    }

    for (field, label) in fields {
        spawn_slider_row(commands, content, field, label);
    }

    // POWER
    spawn_window_section(commands, content, "POWER");

    let power_text = commands.spawn((
        Text::new("DRAW: —"),
        TextFont { font_size: FontSize::Px(12.0), ..default() },
        TextColor(ACCENT),
        PowerReadoutText,
        Node { margin: UiRect::top(Val::Px(4.0)), ..default() },
    )).id();
    commands.entity(content).add_child(power_text);

    // Ship budget bar
    let bar_bg = commands.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(8.0),
            margin: UiRect::vertical(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(SLIDER_BG),
    )).id();
    let bar_fill = commands.spawn((
        Node {
            width: Val::Percent(0.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgb(0.3, 0.7, 0.4)),
        ShipPowerBarFill,
    )).id();
    commands.entity(bar_bg).add_child(bar_fill);
    commands.entity(content).add_child(bar_bg);

    let ship_power_text = commands.spawn((
        Text::new("SHIP: —"),
        TextFont { font_size: FontSize::Px(10.0), ..default() },
        TextColor(WindowStyle::TEXT_DIM),
        ShipPowerText,
    )).id();
    commands.entity(content).add_child(ship_power_text);

    // LIVE STATS
    let stats = commands.spawn((
        Text::new(""),
        TextFont { font_size: FontSize::Px(11.0), ..default() },
        TextColor(WindowStyle::TEXT_COLOR),
        StatsReadoutText,
        Node { margin: UiRect::top(Val::Px(6.0)), ..default() },
    )).id();
    commands.entity(content).add_child(stats);

    // RESET
    let bottom = commands.spawn(
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::FlexEnd,
            margin: UiRect::top(Val::Px(8.0)),
            ..default()
        },
    ).id();
    let reset = commands.spawn((
        Node {
            padding: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(4.0), Val::Px(4.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(AMMO_BG),
        BorderColor::all(WindowStyle::BORDER_COLOR),
        Interaction::None,
        Button,
        ResetTuningButton,
        Tooltip { text: "Reset all sliders to 1.0×".into(), detail: None },
    )).id();
    let reset_label = commands.spawn((
        Text::new("RESET"),
        TextFont { font_size: FontSize::Px(11.0), ..default() },
        TextColor(WindowStyle::TEXT_DIM),
    )).id();
    commands.entity(reset).add_child(reset_label);
    commands.entity(bottom).add_child(reset);
    commands.entity(content).add_child(bottom);
}

fn spawn_slider_row(
    commands: &mut Commands,
    parent: Entity,
    field: TuningField,
    label: &str,
) {
    let row = commands.spawn(
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            margin: UiRect::vertical(Val::Px(4.0)),
            ..default()
        },
    ).id();

    // Label + value line
    let header = commands.spawn(
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        },
    ).id();
    let label_text = commands.spawn((
        Text::new(label),
        TextFont { font_size: FontSize::Px(11.0), ..default() },
        TextColor(WindowStyle::TEXT_COLOR),
    )).id();
    let value_text = commands.spawn((
        Text::new("1.00×"),
        TextFont { font_size: FontSize::Px(11.0), ..default() },
        TextColor(ACCENT),
        TuningValueText { field },
    )).id();
    commands.entity(header).add_children(&[label_text, value_text]);

    // Track (interactive) with fill + baseline tick. FocusPolicy::Block so
    // dragging over the track doesn't also drag the window underneath.
    let track = commands.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(14.0),
            margin: UiRect::top(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(SLIDER_BG),
        Interaction::None,
        Button,
        FocusPolicy::Block,
        TuningSliderTrack { field },
    )).id();

    let fill = commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(mult_to_fraction(1.0) * 100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(SLIDER_FILL),
        FocusPolicy::Pass,
        TuningSliderFill { field },
    )).id();

    // Baseline tick at 1.0×
    let tick = commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(mult_to_fraction(1.0) * 100.0),
            top: Val::Px(0.0),
            width: Val::Px(2.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(BASELINE_TICK),
        FocusPolicy::Pass,
    )).id();

    commands.entity(track).add_children(&[fill, tick]);
    commands.entity(row).add_children(&[header, track]);
    commands.entity(parent).add_child(row);
}

// ============================================================================
// INTERACTION
// ============================================================================

/// Press on a track starts a drag; the value keeps following the cursor until
/// the button is released, even if the cursor slides off the track.
pub fn tuning_slider_drag(
    mouse: Res<ButtonInput<MouseButton>>,
    mut drag: ResMut<ActiveSliderDrag>,
    tracks: Query<(&TuningSliderTrack, &Interaction, &ComputedNode, &GlobalTransform)>,
    windows: Query<&Window>,
    tuning_windows: Query<&TuningWindow>,
    mut tuning_query: Query<&mut WeaponTuning>,
) {
    if mouse.just_released(MouseButton::Left) {
        drag.0 = None;
    }

    let Some(cursor) = windows.single().ok().and_then(|w| w.cursor_position()) else { return };

    // Start a drag
    if drag.0.is_none() && mouse.just_pressed(MouseButton::Left) {
        for (track, interaction, _, _) in tracks.iter() {
            if *interaction == Interaction::Pressed {
                drag.0 = Some(track.field);
                break;
            }
        }
    }

    let Some(active_field) = drag.0 else { return };
    if !mouse.pressed(MouseButton::Left) {
        drag.0 = None;
        return;
    }

    // Value from cursor position over the active track
    for (track, _, node, transform) in tracks.iter() {
        if track.field != active_field { continue; }
        let size = node.size();
        if size.x <= 0.0 { continue; }
        let left_edge = transform.translation().x - size.x / 2.0;
        let normalized = ((cursor.x - left_edge) / size.x).clamp(0.0, 1.0);
        let value = TUNING_MIN + normalized * (TUNING_MAX - TUNING_MIN);

        let Ok(tw) = tuning_windows.single() else { return };
        if let Ok(mut tuning) = tuning_query.get_mut(tw.module_entity) {
            tuning.set(active_field, value);
        }
        return;
    }
}

pub fn ammo_button_click(
    buttons: Query<(&AmmoTypeButton, &Interaction), Changed<Interaction>>,
    tuning_windows: Query<&TuningWindow>,
    mut ammo_query: Query<&mut SelectedAmmo>,
    mut all_buttons: Query<(&AmmoTypeButton, &mut BackgroundColor, &mut BorderColor, &Children)>,
    mut text_colors: Query<&mut TextColor>,
    mut desc_query: Query<&mut Text, With<AmmoDescText>>,
) {
    let mut clicked: Option<KineticAmmoType> = None;
    for (btn, interaction) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            clicked = Some(btn.ammo);
        }
    }
    let Some(ammo) = clicked else { return };
    let Ok(tw) = tuning_windows.single() else { return };
    let Ok(mut selected) = ammo_query.get_mut(tw.module_entity) else { return };
    selected.0 = ammo;

    // Update visuals: highlight selection, dim the rest
    for (btn, mut bg, mut border, children) in all_buttons.iter_mut() {
        let is_sel = btn.ammo == ammo;
        *bg = BackgroundColor(if is_sel { AMMO_SELECTED_BG } else { AMMO_BG });
        *border = BorderColor::all(if is_sel { ACCENT } else { WindowStyle::BORDER_COLOR });
        for child in children.iter() {
            if let Ok(mut tc) = text_colors.get_mut(child) {
                tc.0 = if is_sel { ACCENT } else { WindowStyle::TEXT_COLOR };
            }
        }
    }

    if let Ok(mut desc) = desc_query.single_mut() {
        desc.0 = ammo.description().to_string();
    }
}

pub fn reset_tuning_click(
    buttons: Query<&Interaction, (With<ResetTuningButton>, Changed<Interaction>)>,
    tuning_windows: Query<&TuningWindow>,
    mut tuning_query: Query<&mut WeaponTuning>,
) {
    for interaction in buttons.iter() {
        if *interaction != Interaction::Pressed { continue; }
        let Ok(tw) = tuning_windows.single() else { return };
        if let Ok(mut tuning) = tuning_query.get_mut(tw.module_entity) {
            *tuning = WeaponTuning::default();
        }
    }
}

// ============================================================================
// LIVE REFRESH — fills, value texts, power bar, stat readout
// ============================================================================

pub fn tuning_window_refresh(
    tuning_windows: Query<&TuningWindow>,
    power: Res<PowerState>,
    module_query: Query<(&Module, &Weapon, &WeaponTuning, Option<&SelectedAmmo>)>,
    mut fills: Query<(&TuningSliderFill, &mut Node)>,
    mut value_texts: Query<(&TuningValueText, &mut Text)>,
    mut power_text: Query<&mut Text, (With<PowerReadoutText>, Without<TuningValueText>, Without<ShipPowerText>, Without<StatsReadoutText>)>,
    mut ship_bar: Query<(&mut Node, &mut BackgroundColor), (With<ShipPowerBarFill>, Without<TuningSliderFill>)>,
    mut ship_text: Query<&mut Text, (With<ShipPowerText>, Without<TuningValueText>, Without<PowerReadoutText>, Without<StatsReadoutText>)>,
    mut stats_text: Query<&mut Text, (With<StatsReadoutText>, Without<TuningValueText>, Without<PowerReadoutText>, Without<ShipPowerText>)>,
) {
    let Ok(tw) = tuning_windows.single() else { return };
    let Ok((module, weapon, tuning, ammo)) = module_query.get(tw.module_entity) else { return };

    for (fill, mut node) in fills.iter_mut() {
        node.width = Val::Percent(mult_to_fraction(tuning.get(fill.field)) * 100.0);
    }
    for (vt, mut text) in value_texts.iter_mut() {
        text.0 = format!("{:.2}×", tuning.get(vt.field));
    }

    if let Ok(mut text) = power_text.single_mut() {
        text.0 = format!("DRAW: {:.0} MW  ({:.2}× base)", module.power_consumption, tuning.power_factor());
    }

    let gen = power.total_power_generation.max(1.0);
    let ratio = (power.total_power_consumption / gen).clamp(0.0, 1.0);
    if let Ok((mut node, mut bg)) = ship_bar.single_mut() {
        node.width = Val::Percent(ratio * 100.0);
        bg.0 = if ratio < 0.7 {
            Color::srgb(0.3, 0.7, 0.4)
        } else if ratio < 0.95 {
            Color::srgb(0.85, 0.7, 0.25)
        } else {
            Color::srgb(0.85, 0.3, 0.25)
        };
    }
    if let Ok(mut text) = ship_text.single_mut() {
        text.0 = format!("SHIP: {:.0} / {:.0} MW", power.total_power_consumption, power.total_power_generation);
    }

    if let Ok(mut text) = stats_text.single_mut() {
        let ammo_dmg = ammo.map(|a| a.0.damage_mult()).unwrap_or(1.0);
        let dps = weapon.damage * ammo_dmg * weapon.fire_rate;
        let mut line = format!("DPS {:.0}   ROF {:.2}/s", dps, weapon.fire_rate);
        if is_kinetic_weapon(tw.module_type) {
            let ammo_vel = ammo.map(|a| a.0.velocity_mult()).unwrap_or(1.0);
            let vel = base_projectile_speed(tw.module_type) * tuning.velocity * ammo_vel;
            line.push_str(&format!("   VEL {:.0}", vel));
        }
        text.0 = line;
    }
}

/// OnExit(StationDocked): tuning is a dock-side workshop activity — close any
/// open windows when leaving the station.
pub fn despawn_tuning_windows(
    content_query: Query<Entity, With<TuningWindow>>,
    parent_query: Query<&ChildOf>,
    floating: Query<Entity, With<FloatingWindow>>,
    mut drag: ResMut<ActiveSliderDrag>,
    mut commands: Commands,
) {
    drag.0 = None;
    for content in content_query.iter() {
        commands.entity(find_window_root(content, &parent_query, &floating)).despawn();
    }
}
