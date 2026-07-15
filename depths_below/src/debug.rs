use bevy::prelude::*;
use rand::Rng;

use crate::components::*;
use crate::events::*;
use crate::resources::*;
use crate::states::GameState;
use crate::ai_ship::components::{AiShipState, AiShipType};

// ============================================================================
// DEBUG MENU — dev tooling, F10 to toggle. Not player-facing polish:
// spawn ships/wrecks on demand, grant credits, repair/refuel/rearm,
// visualize hitboxes. Actions only respond while the panel is open so
// the hotkeys can't misfire during normal play.
// ============================================================================

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugMenu>()
            .add_systems(Update, toggle_debug_menu)
            .add_systems(
                Update,
                (debug_actions, debug_kill_flagged, draw_hitboxes)
                    .run_if(in_state(GameState::Exploring)),
            );
    }
}

#[derive(Resource, Default)]
pub struct DebugMenu {
    pub open: bool,
    pub show_hitboxes: bool,
}

#[derive(Component)]
struct DebugMenuPanel;

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

fn toggle_debug_menu(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<DebugMenu>,
    mut commands: Commands,
    panel_query: Query<Entity, With<DebugMenuPanel>>,
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
    } else {
        for entity in panel_query.iter() {
            commands.entity(entity).despawn();
        }
    }
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
