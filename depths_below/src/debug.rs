use bevy::prelude::*;
use rand::Rng;

use crate::components::*;
use crate::events::*;
use crate::resources::*;
use crate::states::GameState;
use crate::ai_ship::components::{AiShip, AiShipState, AiShipType};

// ============================================================================
// DEBUG MENU — dev tooling, ` (backtick) to toggle. Not player-facing polish:
// spawn ships/wrecks on demand, grant credits, repair/refuel/rearm, clear an
// area, teleport, dump cargo, visualize hitboxes. Actions only respond while
// the panel is open so the hotkeys can't misfire during normal play.
//
// Opens alongside a second panel, the TUNING PANEL — live +/- multipliers
// for speed/damage/fire-rate plus god mode, infinite fuel, and a teleport
// button, for pushing the game's numbers past their normal range to see
// where things break. DebugTuning is read by ship/movement.rs (speed, fuel)
// and combat/new_projectiles.rs (damage, fire rate — the player's kinetic
// weapons specifically: Cannon/Railgun/Coilgun/Gatling; lasers and missiles
// aren't wired to it yet) and ship/damage.rs (god mode).
// ============================================================================

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugMenu>()
            .init_resource::<DebugTuning>()
            .add_systems(Update, toggle_debug_menu)
            .add_systems(
                Update,
                (
                    debug_actions,
                    debug_kill_flagged,
                    draw_hitboxes,
                )
                    .run_if(in_state(GameState::Exploring)),
            )
            .add_systems(
                Update,
                apply_debug_teleport.run_if(in_state(GameState::Exploring)),
            )
            .add_systems(
                Update,
                (tuning_button_system, update_tuning_value_text)
                    .run_if(in_state(GameState::Exploring)),
            );
    }
}

#[derive(Resource, Default)]
pub struct DebugMenu {
    pub open: bool,
    pub show_hitboxes: bool,
}

/// Live gameplay multipliers/toggles for exploring the game's limits — see
/// module doc comment for exactly which systems read each field.
#[derive(Resource)]
pub struct DebugTuning {
    pub speed_mult: f32,
    pub damage_mult: f32,
    pub fire_rate_mult: f32,
    pub god_mode: bool,
    pub infinite_fuel: bool,
}

impl Default for DebugTuning {
    fn default() -> Self {
        Self {
            speed_mult: 1.0,
            damage_mult: 1.0,
            fire_rate_mult: 1.0,
            god_mode: false,
            infinite_fuel: false,
        }
    }
}

#[derive(Component)]
struct DebugMenuPanel;

#[derive(Component)]
struct TuningPanel;

/// Which field a tuning row's +/- buttons (or the row itself, for toggles
/// and actions) affects.
#[derive(Component, Clone, Copy, PartialEq)]
enum TuningButton {
    SpeedDec,
    SpeedInc,
    DamageDec,
    DamageInc,
    FireRateDec,
    FireRateInc,
    ToggleGodMode,
    ToggleInfiniteFuel,
    Teleport,
    ResetAll,
}

/// Tags a value-display Text so update_tuning_value_text knows what to
/// write into it each frame.
#[derive(Component, Clone, Copy)]
enum TuningValueText {
    Speed,
    Damage,
    FireRate,
    GodMode,
    InfiniteFuel,
}

/// Ships flagged for instant destruction ONE FRAME after spawning — the
/// block hierarchy must exist (commands flushed) before
/// ai_ship_death_system walks the children to build the wreck.
#[derive(Component)]
struct DebugKillNextFrame;

const SPAWNABLE: [AiShipType; 8] = [
    AiShipType::IronTide,
    AiShipType::Blackwater,
    AiShipType::PressureKing,
    AiShipType::GlassEye,
    AiShipType::Drowned,
    AiShipType::AbyssalCult,
    AiShipType::RustSwarm,
    AiShipType::Leviathan,
];

const BOSSES: [AiShipType; 2] = [AiShipType::Dreadnought, AiShipType::VoidTitan];

fn toggle_debug_menu(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<DebugMenu>,
    mut commands: Commands,
    panel_query: Query<Entity, With<DebugMenuPanel>>,
    tuning_panel_query: Query<Entity, With<TuningPanel>>,
) {
    // Backquote as primary — F10 is a macOS media key unless Fn is held.
    if !keyboard.just_pressed(KeyCode::Backquote) && !keyboard.just_pressed(KeyCode::F10) {
        return;
    }
    menu.open = !menu.open;

    if menu.open {
        commands
            .spawn((
                DebugMenuPanel,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(12.0),
                    top: Val::Px(220.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(10.0)),
                    row_gap: Val::Px(2.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.02, 0.05, 0.08, 0.85)),
                GlobalZIndex(50),
            ))
            .with_children(|panel| {
                for line in [
                    "DEBUG  (` close)",
                    "7  +1000 credits",
                    "8  spawn hostile ship",
                    "9  spawn fresh wreck",
                    "2  clear nearby hostiles",
                    "3  teleport to system star",
                    "4  fill cargo (one of everything)",
                    "5  spawn a boss ship",
                    "0  reveal + target nearest system",
                    "1  reveal entire galaxy map",
                    "H  toggle hitboxes",
                    "J  repair + refuel + rearm",
                ] {
                    panel.spawn((
                        Text::new(line),
                        TextFont { font_size: FontSize::Px(12.0), ..default() },
                        TextColor(Color::srgb(0.75, 0.9, 1.0)),
                    ));
                }
            });

        spawn_tuning_panel(&mut commands);
    } else {
        for entity in panel_query.iter() {
            commands.entity(entity).despawn();
        }
        for entity in tuning_panel_query.iter() {
            commands.entity(entity).despawn();
        }
    }
}

fn spawn_tuning_panel(commands: &mut Commands) {
    commands
        .spawn((
            TuningPanel,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(260.0),
                top: Val::Px(220.0),
                width: Val::Px(230.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.05, 0.08, 0.85)),
            GlobalZIndex(50),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("TUNING — see the limits"),
                TextFont { font_size: FontSize::Px(12.0), ..default() },
                TextColor(Color::srgb(0.75, 0.9, 1.0)),
            ));

            spawn_stepper_row(panel, "Speed", TuningValueText::Speed, TuningButton::SpeedDec, TuningButton::SpeedInc);
            spawn_stepper_row(panel, "Damage", TuningValueText::Damage, TuningButton::DamageDec, TuningButton::DamageInc);
            spawn_stepper_row(panel, "Fire Rate", TuningValueText::FireRate, TuningButton::FireRateDec, TuningButton::FireRateInc);

            spawn_toggle_row(panel, "God Mode", TuningValueText::GodMode, TuningButton::ToggleGodMode);
            spawn_toggle_row(panel, "Infinite Fuel", TuningValueText::InfiniteFuel, TuningButton::ToggleInfiniteFuel);

            spawn_action_row(panel, "Teleport to map target", TuningButton::Teleport);
            spawn_action_row(panel, "Reset all", TuningButton::ResetAll);
        });
}

fn tuning_button_style() -> (Node, BackgroundColor) {
    (
        Node {
            width: Val::Px(22.0),
            height: Val::Px(18.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.15, 0.2, 0.28, 1.0)),
    )
}

fn spawn_stepper_row(
    panel: &mut ChildSpawnerCommands,
    label: &str,
    value_tag: TuningValueText,
    dec: TuningButton,
    inc: TuningButton,
) {
    panel.spawn(Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: Val::Px(4.0),
        ..default()
    }).with_children(|row| {
        row.spawn((
            Text::new(label),
            TextFont { font_size: FontSize::Px(11.0), ..default() },
            TextColor(Color::srgb(0.7, 0.75, 0.8)),
            Node { width: Val::Px(70.0), ..default() },
        ));
        let (node, bg) = tuning_button_style();
        row.spawn((node.clone(), bg, Interaction::None, dec))
            .with_children(|b| { b.spawn((Text::new("-"), TextFont { font_size: FontSize::Px(13.0), ..default() }, TextColor(Color::WHITE))); });
        row.spawn((
            Text::new("1.00x"),
            TextFont { font_size: FontSize::Px(11.0), ..default() },
            TextColor(Color::srgb(1.0, 0.9, 0.5)),
            Node { width: Val::Px(48.0), justify_content: JustifyContent::Center, ..default() },
            value_tag,
        ));
        row.spawn((node, bg, Interaction::None, inc))
            .with_children(|b| { b.spawn((Text::new("+"), TextFont { font_size: FontSize::Px(13.0), ..default() }, TextColor(Color::WHITE))); });
    });
}

fn spawn_toggle_row(panel: &mut ChildSpawnerCommands, label: &str, value_tag: TuningValueText, toggle: TuningButton) {
    panel.spawn((
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            padding: UiRect::all(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.12, 0.16, 0.22, 1.0)),
        Interaction::None,
        toggle,
    )).with_children(|row| {
        row.spawn((
            Text::new(label),
            TextFont { font_size: FontSize::Px(11.0), ..default() },
            TextColor(Color::srgb(0.7, 0.75, 0.8)),
        ));
        row.spawn((
            Text::new("OFF"),
            TextFont { font_size: FontSize::Px(11.0), ..default() },
            TextColor(Color::srgb(1.0, 0.4, 0.4)),
            value_tag,
        ));
    });
}

fn spawn_action_row(panel: &mut ChildSpawnerCommands, label: &str, action: TuningButton) {
    panel.spawn((
        Node {
            padding: UiRect::all(Val::Px(4.0)),
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.18, 0.14, 0.1, 1.0)),
        Interaction::None,
        action,
    )).with_children(|row| {
        row.spawn((
            Text::new(label),
            TextFont { font_size: FontSize::Px(11.0), ..default() },
            TextColor(Color::srgb(0.9, 0.8, 0.6)),
        ));
    });
}

/// Step sizes chosen so a handful of clicks gets you somewhere absurd —
/// this panel exists to find the game's breaking points, not for fine
/// balance tuning (that's the customization/tuning window's job).
const SPEED_STEP: f32 = 0.25;
const DAMAGE_STEP: f32 = 0.5;
const FIRE_RATE_STEP: f32 = 0.25;

fn tuning_button_system(
    buttons: Query<(&TuningButton, &Interaction), Changed<Interaction>>,
    mut tuning: ResMut<DebugTuning>,
    mut ship_query: Query<(&mut Transform, &mut Velocity), With<Ship>>,
    pending: Res<crate::ui::PendingWarpTarget>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for (button, interaction) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button {
            TuningButton::SpeedDec => tuning.speed_mult = (tuning.speed_mult - SPEED_STEP).max(0.0),
            TuningButton::SpeedInc => tuning.speed_mult = (tuning.speed_mult + SPEED_STEP).min(20.0),
            TuningButton::DamageDec => tuning.damage_mult = (tuning.damage_mult - DAMAGE_STEP).max(0.0),
            TuningButton::DamageInc => tuning.damage_mult = (tuning.damage_mult + DAMAGE_STEP).min(50.0),
            TuningButton::FireRateDec => tuning.fire_rate_mult = (tuning.fire_rate_mult - FIRE_RATE_STEP).max(0.05),
            TuningButton::FireRateInc => tuning.fire_rate_mult = (tuning.fire_rate_mult + FIRE_RATE_STEP).min(20.0),
            TuningButton::ToggleGodMode => tuning.god_mode = !tuning.god_mode,
            TuningButton::ToggleInfiniteFuel => tuning.infinite_fuel = !tuning.infinite_fuel,
            TuningButton::ResetAll => *tuning = DebugTuning::default(),
            TuningButton::Teleport => {
                let Some(target) = pending.0 else {
                    notifications.write(ShowNotification {
                        message: "[debug] no map target set — open the map (M) and click one first".into(),
                        notification_type: NotificationType::Warning,
                        duration: 2.5,
                    });
                    continue;
                };
                if let Ok((mut transform, mut velocity)) = ship_query.single_mut() {
                    transform.translation.x = target.x;
                    transform.translation.y = target.y;
                    velocity.0 = Vec2::ZERO;
                    notifications.write(ShowNotification {
                        message: "[debug] teleported to map target".into(),
                        notification_type: NotificationType::Info,
                        duration: 2.0,
                    });
                }
            }
        }
    }
}

fn update_tuning_value_text(
    tuning: Res<DebugTuning>,
    mut texts: Query<(&mut Text, &mut TextColor, &TuningValueText)>,
) {
    if !tuning.is_changed() {
        return;
    }
    for (mut text, mut color, tag) in texts.iter_mut() {
        match tag {
            TuningValueText::Speed => text.0 = format!("{:.2}x", tuning.speed_mult),
            TuningValueText::Damage => text.0 = format!("{:.2}x", tuning.damage_mult),
            TuningValueText::FireRate => text.0 = format!("{:.2}x", tuning.fire_rate_mult),
            TuningValueText::GodMode => {
                text.0 = if tuning.god_mode { "ON".into() } else { "OFF".into() };
                color.0 = if tuning.god_mode { Color::srgb(0.4, 1.0, 0.4) } else { Color::srgb(1.0, 0.4, 0.4) };
            }
            TuningValueText::InfiniteFuel => {
                text.0 = if tuning.infinite_fuel { "ON".into() } else { "OFF".into() };
                color.0 = if tuning.infinite_fuel { Color::srgb(0.4, 1.0, 0.4) } else { Color::srgb(1.0, 0.4, 0.4) };
            }
        }
    }
}

/// Bundled so debug_actions stays under Bevy's ~16-raw-parameter ceiling on
/// function systems — same pattern MapWorldData (ui/mod.rs) already uses
/// for the same reason.
#[derive(bevy::ecs::system::SystemParam)]
struct DebugGalaxyParams<'w> {
    galaxy_map: ResMut<'w, crate::celestial::resources::GalaxyMap>,
    pending_galaxy_target: ResMut<'w, crate::celestial::resources::PendingGalaxyWarpTarget>,
    streaming: Res<'w, crate::celestial::resources::SystemStreamingManager>,
}

#[derive(bevy::ecs::system::SystemParam)]
struct DebugHostileParams<'w, 's> {
    hostile_query: Query<'w, 's, (Entity, &'static GlobalTransform), With<AiShip>>,
    destroyed_events: MessageWriter<'w, AiShipDestroyed>,
    ai_state_query: Query<'w, 's, &'static mut AiShipState>,
    ai_type_query: Query<'w, 's, &'static AiShipType>,
}

fn debug_actions(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<DebugMenu>,
    mut commands: Commands,
    ship_query: Query<(Entity, &GlobalTransform), With<Ship>>,
    children_query: Query<&Children>,
    mut currency: ResMut<Currency>,
    mut fuel: ResMut<FuelState>,
    mut hull_query: Query<&mut HullSegment>,
    mut module_query: Query<&mut Module>,
    mut weapon_query: Query<&mut Weapon>,
    registry: Res<crate::building::ModuleRegistry>,
    asset_server: Res<AssetServer>,
    mut notifications: MessageWriter<ShowNotification>,
    mut galaxy: DebugGalaxyParams,
    mut inventory: ResMut<Inventory>,
    mut hostiles: DebugHostileParams,
) {
    if !menu.open {
        return;
    }
    let Ok((ship_entity, ship_gt)) = ship_query.single() else { return };
    let ship_pos = ship_gt.translation().truncate();
    let mut rng = rand::thread_rng();

    let mut notify = |notifications: &mut MessageWriter<ShowNotification>, msg: String| {
        notifications.write(ShowNotification {
            message: msg,
            notification_type: NotificationType::Info,
            duration: 2.0,
        });
    };

    if keyboard.just_pressed(KeyCode::Digit7) {
        currency.credits += 1000;
        notify(&mut notifications, "[debug] +1000 credits".into());
    }

    if keyboard.just_pressed(KeyCode::Digit8) {
        let ship_type = SPAWNABLE[rng.gen_range(0..SPAWNABLE.len())];
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let pos = ship_pos + Vec2::new(angle.cos(), angle.sin()) * 700.0;
        crate::ai_ship::spawner::spawn_ai_ship(ship_type, pos, &mut commands, &registry, &asset_server);
        notify(&mut notifications, format!("[debug] spawned {:?}", ship_type));
    }

    if keyboard.just_pressed(KeyCode::Digit9) {
        let ship_type = SPAWNABLE[rng.gen_range(0..SPAWNABLE.len())];
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let pos = ship_pos + Vec2::new(angle.cos(), angle.sin()) * 450.0;
        let entity = crate::ai_ship::spawner::spawn_ai_ship(ship_type, pos, &mut commands, &registry, &asset_server);
        commands.entity(entity).insert(DebugKillNextFrame);
        notify(&mut notifications, format!("[debug] spawning {:?} wreck", ship_type));
    }

    // Clears every real (spawned) hostile within 6000 units — for testing
    // an empty arena, or just cleaning up after a debug-spawn spree.
    if keyboard.just_pressed(KeyCode::Digit2) {
        let mut cleared = 0;
        for (entity, gt) in hostiles.hostile_query.iter() {
            if gt.translation().truncate().distance(ship_pos) > 6000.0 { continue; }
            let ship_type = hostiles.ai_type_query.get(entity).copied().unwrap_or(AiShipType::Drowned);
            if let Ok(mut state) = hostiles.ai_state_query.get_mut(entity) {
                if !state.is_destroyed {
                    state.is_destroyed = true;
                    state.hull_integrity = 0.0;
                    hostiles.destroyed_events.write(AiShipDestroyed {
                        entity,
                        ship_type,
                        position: gt.translation().truncate(),
                        bounty_id: None,
                    });
                    cleared += 1;
                }
            }
        }
        notify(&mut notifications, format!("[debug] cleared {} nearby hostiles", cleared));
    }

    // Teleports to the current system's star — always a known, reachable
    // point regardless of how the local map looks.
    if keyboard.just_pressed(KeyCode::Digit3) {
        if let Some(def) = galaxy.streaming.loaded_system.and_then(|id| galaxy.galaxy_map.systems.iter().find(|s| s.id == id)) {
            let target = def.local_center + Vec2::new(80_000.0, 0.0);
            commands.entity(ship_entity).try_insert(DebugTeleportTo(target));
            notify(&mut notifications, format!("[debug] teleporting to {}'s star", def.name));
        }
    }

    // Fills cargo with one stack of every item type, for testing sell/buy
    // and weight-capacity behavior without a mining/looting grind.
    if keyboard.just_pressed(KeyCode::Digit4) {
        for item in [ItemType::ScrapMetal, ItemType::Crystal, ItemType::BioSample, ItemType::FuelCell, ItemType::RareAlloy, ItemType::AncientArtifact, ItemType::AmmoCrate] {
            inventory.add_item(item, 5);
        }
        notify(&mut notifications, "[debug] cargo filled with 5x of everything".into());
    }

    // Spawns a boss (Dreadnought/VoidTitan) for testing endgame-tier fights
    // without needing to actually travel to boss territory.
    if keyboard.just_pressed(KeyCode::Digit5) {
        let ship_type = BOSSES[rng.gen_range(0..BOSSES.len())];
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let pos = ship_pos + Vec2::new(angle.cos(), angle.sin()) * 1200.0;
        crate::ai_ship::spawner::spawn_ai_ship(ship_type, pos, &mut commands, &registry, &asset_server);
        notify(&mut notifications, format!("[debug] spawned boss {:?}", ship_type));
    }

    // Instantly reveals + targets the nearest not-yet-visited system for
    // interstellar warp testing — real discovery (scanning, star charts,
    // and the click-anywhere galaxy map) is wired now too, this just saves
    // clicking around for a quick test.
    if keyboard.just_pressed(KeyCode::Digit0) {
        use crate::celestial::resources::{GalaxyWarpTarget, SystemDiscovery};
        let current_pos = galaxy.streaming.current_galaxy_pos;
        let nearest = galaxy.galaxy_map.systems.iter_mut()
            .filter(|s| s.discovery != SystemDiscovery::Visited && s.galaxy_pos.distance(current_pos) > 1.0)
            .min_by(|a, b| a.galaxy_pos.distance(current_pos).partial_cmp(&b.galaxy_pos.distance(current_pos)).unwrap());
        if let Some(sys) = nearest {
            sys.discovery = SystemDiscovery::Located;
            galaxy.pending_galaxy_target.0 = Some(GalaxyWarpTarget::System(sys.id));
            notify(&mut notifications, format!("[debug] revealed + targeted {} for warp (V to jump)", sys.name));
        }
    }

    // Reveals every system on the galaxy map as fully Visited (name/faction/
    // danger all shown) — for eyeballing the whole layout at once, not a
    // real gameplay mechanic.
    if keyboard.just_pressed(KeyCode::Digit1) {
        use crate::celestial::resources::SystemDiscovery;
        for sys in galaxy.galaxy_map.systems.iter_mut() {
            sys.discovery = SystemDiscovery::Visited;
        }
        notify(&mut notifications, "[debug] revealed entire galaxy map".into());
    }

    if keyboard.just_pressed(KeyCode::KeyH) {
        menu.show_hitboxes = !menu.show_hitboxes;
        notify(
            &mut notifications,
            format!("[debug] hitboxes {}", if menu.show_hitboxes { "on" } else { "off" }),
        );
    }

    if keyboard.just_pressed(KeyCode::KeyJ) {
        fuel.current_fuel = fuel.max_fuel;
        if let Ok(children) = children_query.get(ship_entity) {
            for child in children.iter() {
                if let Ok(mut hull) = hull_query.get_mut(child) {
                    hull.health = hull.max_health;
                    hull.is_depressurized = false;
                    hull.depressurization_level = 0.0;
                }
                if let Ok(mut module) = module_query.get_mut(child) {
                    if module.health > 0.0 {
                        module.health = module.max_health;
                    }
                }
                if let Ok(mut weapon) = weapon_query.get_mut(child) {
                    weapon.ammo = weapon.max_ammo;
                }
            }
        }
        notify(&mut notifications, "[debug] repaired + refueled + rearmed".into());
    }
}

/// One-frame marker: teleport this entity to the given position next frame.
/// Needed because debug_actions only has a GlobalTransform query for the
/// player ship (shared with other read-only lookups); a dedicated tiny
/// system below does the actual mutable Transform write.
#[derive(Component)]
struct DebugTeleportTo(Vec2);

fn apply_debug_teleport(
    mut commands: Commands,
    mut query: Query<(Entity, &mut Transform, &mut Velocity, &DebugTeleportTo)>,
) {
    for (entity, mut transform, mut velocity, teleport) in query.iter_mut() {
        transform.translation.x = teleport.0.x;
        transform.translation.y = teleport.0.y;
        velocity.0 = Vec2::ZERO;
        commands.entity(entity).remove::<DebugTeleportTo>();
    }
}

/// Detonate ships flagged by the wreck-spawn action, one frame later.
fn debug_kill_flagged(
    mut commands: Commands,
    mut flagged: Query<(Entity, &mut AiShipState, &Transform, &AiShipType), With<DebugKillNextFrame>>,
    mut destroyed_events: MessageWriter<AiShipDestroyed>,
) {
    for (entity, mut state, transform, ship_type) in flagged.iter_mut() {
        if !state.is_destroyed {
            state.is_destroyed = true;
            state.hull_integrity = 0.0;
            destroyed_events.write(AiShipDestroyed {
                entity,
                ship_type: *ship_type,
                position: transform.translation.truncate(),
                bounty_id: None,
            });
        }
        commands.entity(entity).try_remove::<DebugKillNextFrame>();
    }
}

/// Gizmo overlay: block bounds (green), creature bounds (orange),
/// missile blast radii (yellow). Sprite bounds ARE the hitboxes — combat
/// collision is distance-vs-block-cell math, no physics engine.
fn draw_hitboxes(
    menu: Res<DebugMenu>,
    mut gizmos: Gizmos,
    block_query: Query<(&GlobalTransform, &Sprite), Or<(With<Module>, With<HullSegment>)>>,
    creature_query: Query<(&GlobalTransform, &Sprite), With<Creature>>,
    missile_query: Query<(&GlobalTransform, &crate::combat::new_projectiles::MissileProjectile)>,
) {
    if !menu.show_hitboxes {
        return;
    }
    for (gt, sprite) in block_query.iter() {
        let size = sprite.custom_size.unwrap_or(Vec2::splat(66.0));
        let angle = gt.rotation().to_euler(EulerRot::XYZ).2;
        gizmos.rect_2d(
            Isometry2d::new(gt.translation().truncate(), Rot2::radians(angle)),
            size,
            Color::srgba(0.2, 1.0, 0.4, 0.5),
        );
    }
    for (gt, sprite) in creature_query.iter() {
        let radius = sprite.custom_size.map(|s| s.x.max(s.y) * 0.5).unwrap_or(20.0);
        gizmos.circle_2d(gt.translation().truncate(), radius, Color::srgba(1.0, 0.45, 0.2, 0.6));
    }
    for (gt, missile) in missile_query.iter() {
        gizmos.circle_2d(gt.translation().truncate(), missile.blast_radius, Color::srgba(1.0, 0.85, 0.2, 0.35));
    }
}
