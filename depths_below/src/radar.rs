use bevy::prelude::*;
use crate::components::*;
use crate::resources::*;
use crate::events::*;
use crate::states::{GameState, RadarSet};
use crate::ai_ship::components::{AiShip, AiShipType, AiShipRadarContact, BountyTarget, WorldSimulation};
use crate::contracts::{ContractObjective, ContractState};
use crate::world::home_base::{OUTPOST_POSITIONS, STATION_POS};

/// Marker for the radar radar display UI
#[derive(Component)]
pub struct RadarDisplay;

/// Marker for a radar blip (dot on radar)
#[derive(Component)]
pub struct RadarBlip {
    pub lifetime: Timer,
}

/// Radar sweep line angle
#[derive(Resource)]
pub struct RadarSweepState {
    pub angle: f32,
    pub sweep_speed: f32,
    pub display_visible: bool,
}

impl Default for RadarSweepState {
    fn default() -> Self {
        Self {
            angle: 0.0,
            sweep_speed: std::f32::consts::TAU / 4.0, // Full rotation every 4 seconds
            display_visible: false,
        }
    }
}

pub struct RadarPlugin;

impl Plugin for RadarPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<RadarSweepState>()
            .configure_sets(Update, RadarSet::Input.run_if(in_state(GameState::Exploring)))
            .configure_sets(Update, RadarSet::Update.after(RadarSet::Input).run_if(in_state(GameState::Exploring)))
            .configure_sets(Update, RadarSet::Visibility.after(RadarSet::Update).run_if(in_state(GameState::Exploring)))
            .add_systems(
                Update,
                (
                    radar_ping_system.in_set(RadarSet::Input),
                    toggle_radar_display.in_set(RadarSet::Input),
                    update_radar_pings.in_set(RadarSet::Update),
                    update_radar_revealed.in_set(RadarSet::Update),
                    update_radar.in_set(RadarSet::Update),
                    update_depth_visibility.in_set(RadarSet::Visibility),
                ),
            );
    }
}

/// Toggle radar radar display with Tab key
fn toggle_radar_display(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut sweep_state: ResMut<RadarSweepState>,
    mut commands: Commands,
    existing_display: Query<Entity, With<RadarDisplay>>,
    radar_query: Query<(&Radar, &Module)>,
) {
    // Tab toggles radar display (Z is for ping)
    if keyboard.just_pressed(KeyCode::Tab) {
        let has_radar = radar_query.iter().any(|(_, m)| m.is_active);
        if !has_radar {
            return;
        }

        if sweep_state.display_visible {
            // Hide display
            sweep_state.display_visible = false;
            for entity in existing_display.iter() {
                commands.entity(entity).despawn();
            }
        } else {
            // Show display - spawn the radar circle
            sweep_state.display_visible = true;
            spawn_radar_display(&mut commands);
        }
    }
}

/// Spawn the circular radar display UI in the corner
fn spawn_radar_display(commands: &mut Commands) {
    let radar_size = 180.0;

    // Main radar container
    commands.spawn((
        (Node {
                position_type: PositionType::Absolute,
                right: Val::Px(10.0),
                bottom: Val::Px(10.0),
                width: Val::Px(radar_size),
                height: Val::Px(radar_size),
                ..default()
            }, BackgroundColor(Color::srgba(0.0, 0.1, 0.0, 0.7)), ZIndex(50)),
        RadarDisplay,
    )).with_children(|parent| {
        // Title
        parent.spawn((Text::new("RADAR"), TextFont { font_size: FontSize::Px(12.0), ..default() }, TextColor(Color::srgb(0.2, 0.8, 0.2)), Node {
                position_type: PositionType::Absolute,
                left: Val::Px(2.0),
                top: Val::Px(2.0),
                ..default()
            }));

        // Center dot (ship position)
        parent.spawn((Node {
                position_type: PositionType::Absolute,
                left: Val::Px(radar_size / 2.0 - 3.0),
                top: Val::Px(radar_size / 2.0 - 3.0),
                width: Val::Px(6.0),
                height: Val::Px(6.0),
                ..default()
            }, BackgroundColor(Color::srgb(0.3, 1.0, 0.3))));

        // Range rings (inner and outer)
        for ring_frac in [0.33, 0.66, 1.0] {
            let ring_size = radar_size * ring_frac;
            let offset = (radar_size - ring_size) / 2.0;
            parent.spawn((Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(offset),
                    top: Val::Px(offset),
                    width: Val::Px(ring_size),
                    height: Val::Px(ring_size),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                }, BorderColor::all(Color::srgba(0.1, 0.4, 0.1, 0.5)), BackgroundColor(Color::NONE)));
        }
    });
}

/// Update the radar radar display - sweep line and blips
pub fn update_radar(
    time: Res<Time>,
    mut sweep_state: ResMut<RadarSweepState>,
    radar_display: Query<Entity, With<RadarDisplay>>,
    radar_query: Query<(&Radar, &Module)>,
    ship_query: Query<&Transform, With<Ship>>,
    creature_query: Query<(&Transform, &Creature, Option<&RadarRevealed>), Without<Ship>>,
    poi_query: Query<(&Transform, &PointOfInterest), Without<Ship>>,
    ai_ship_query: Query<(&Transform, &AiShipType, Option<&AiShipRadarContact>), With<AiShip>>,
    contract_state: Res<ContractState>,
    sim: Res<WorldSimulation>,
    bounty_ship_query: Query<(&Transform, &BountyTarget), With<AiShip>>,
    mut commands: Commands,
) {
    if !sweep_state.display_visible {
        return;
    }

    let Ok(radar_entity) = radar_display.single() else { return };
    let Ok(ship_transform) = ship_query.single() else { return };
    let ship_pos = ship_transform.translation.truncate();

    // Get radar range
    let radar_range = radar_query.iter()
        .filter(|(_, m)| m.is_active)
        .map(|(s, _)| s.range)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or(500.0);

    let radar_size = 180.0;
    let radar_half = radar_size / 2.0;

    // Advance sweep angle
    sweep_state.angle += sweep_state.sweep_speed * time.delta_secs();
    if sweep_state.angle > std::f32::consts::TAU {
        sweep_state.angle -= std::f32::consts::TAU;
    }

    // Note: blip lifetime ticking and despawning is handled by update_radar_revealed

    // Spawn blips for entities near the sweep line
    let sweep_tolerance = 0.15; // Radians - how wide the sweep detection is

    // Check creatures
    for (c_transform, creature, radar_revealed) in creature_query.iter() {
        let c_pos = c_transform.translation.truncate();
        let offset = c_pos - ship_pos;
        let dist = offset.length();

        if dist > radar_range {
            continue;
        }

        // Check if entity is near the sweep line
        let entity_angle = offset.y.atan2(offset.x);
        let angle_diff = (entity_angle - sweep_state.angle).abs();
        let angle_diff = angle_diff.min(std::f32::consts::TAU - angle_diff);

        if angle_diff < sweep_tolerance || radar_revealed.is_some() {
            // Map world position to radar position
            let radar_x = (offset.x / radar_range) * radar_half + radar_half;
            let radar_y = radar_half - (offset.y / radar_range) * radar_half; // Flip Y

            // Blip color based on creature threat
            let blip_color = match creature.creature_type {
                CreatureType::Leviathan => Color::srgb(1.0, 0.2, 0.2),
                CreatureType::Stalker => Color::srgb(1.0, 0.6, 0.2),
                CreatureType::ParasiteSwarm => Color::srgb(1.0, 1.0, 0.3),
                CreatureType::VoidDrifter => Color::srgb(0.5, 0.8, 0.5),
            };

            // Blip size based on creature threat
            let blip_size = match creature.creature_type {
                CreatureType::Leviathan => 8.0,
                CreatureType::Stalker => 5.0,
                _ => 3.0,
            };

            commands.spawn((
                (Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(radar_x - blip_size / 2.0),
                        top: Val::Px(radar_y - blip_size / 2.0),
                        width: Val::Px(blip_size),
                        height: Val::Px(blip_size),
                        ..default()
                    }, BackgroundColor(blip_color), ZIndex(51)),
                RadarBlip {
                    lifetime: Timer::from_seconds(4.0, TimerMode::Once),
                },
            )).insert(ChildOf(radar_entity));
        }
    }

    // Check POIs - show as cyan blips
    for (poi_transform, _poi) in poi_query.iter() {
        let p_pos = poi_transform.translation.truncate();
        let offset = p_pos - ship_pos;
        let dist = offset.length();

        if dist > radar_range {
            continue;
        }

        let entity_angle = offset.y.atan2(offset.x);
        let angle_diff = (entity_angle - sweep_state.angle).abs();
        let angle_diff = angle_diff.min(std::f32::consts::TAU - angle_diff);

        if angle_diff < sweep_tolerance {
            let radar_x = (offset.x / radar_range) * radar_half + radar_half;
            let radar_y = radar_half - (offset.y / radar_range) * radar_half;

            commands.spawn((
                (Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(radar_x - 3.0),
                        top: Val::Px(radar_y - 3.0),
                        width: Val::Px(6.0),
                        height: Val::Px(6.0),
                        ..default()
                    }, BackgroundColor(Color::srgb(0.2, 0.8, 1.0)), ZIndex(51)),
                RadarBlip {
                    lifetime: Timer::from_seconds(4.0, TimerMode::Once),
                },
            )).insert(ChildOf(radar_entity));
        }
    }

    // Check stations (Haven Station + resupply outposts) — green beacons,
    // revealed by the sweep line like POIs (not every frame — that would
    // spam a fresh blip entity per station every frame instead of the
    // periodic sweep-triggered trickle every other blip type uses).
    for station_pos in std::iter::once(STATION_POS).chain(OUTPOST_POSITIONS.iter().copied()) {
        let offset = station_pos - ship_pos;
        let dist = offset.length();

        if dist > radar_range {
            continue;
        }

        let entity_angle = offset.y.atan2(offset.x);
        let angle_diff = (entity_angle - sweep_state.angle).abs();
        let angle_diff = angle_diff.min(std::f32::consts::TAU - angle_diff);
        if angle_diff >= sweep_tolerance {
            continue;
        }

        let radar_x = (offset.x / radar_range) * radar_half + radar_half;
        let radar_y = radar_half - (offset.y / radar_range) * radar_half;

        commands.spawn((
            (Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(radar_x - 4.0),
                    top: Val::Px(radar_y - 4.0),
                    width: Val::Px(8.0),
                    height: Val::Px(8.0),
                    ..default()
                }, BackgroundColor(Color::srgb(0.25, 1.0, 0.35)), ZIndex(51)),
            RadarBlip {
                lifetime: Timer::from_seconds(4.0, TimerMode::Once),
            },
        )).insert(ChildOf(radar_entity));
    }

    // Active bounty targets — bright red marker over whatever's at that
    // position, resolved from the real spawned entity if in range, else the
    // off-screen simulated position.
    let bounty_positions: Vec<Vec2> = contract_state.active_contracts.iter()
        .filter_map(|c| match &c.objective {
            ContractObjective::DestroyShip { target_id, destroyed: false, .. } => {
                bounty_ship_query.iter()
                    .find(|(_, b)| b.0 == *target_id)
                    .map(|(t, _)| t.translation.truncate())
                    .or_else(|| sim.bounty_position(*target_id))
            }
            _ => None,
        })
        .collect();
    for bounty_pos in bounty_positions {
        let offset = bounty_pos - ship_pos;
        let dist = offset.length();

        if dist > radar_range {
            continue;
        }

        let entity_angle = offset.y.atan2(offset.x);
        let angle_diff = (entity_angle - sweep_state.angle).abs();
        let angle_diff = angle_diff.min(std::f32::consts::TAU - angle_diff);
        if angle_diff >= sweep_tolerance {
            continue;
        }

        let radar_x = (offset.x / radar_range) * radar_half + radar_half;
        let radar_y = radar_half - (offset.y / radar_range) * radar_half;

        commands.spawn((
            (Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(radar_x - 5.0),
                    top: Val::Px(radar_y - 5.0),
                    width: Val::Px(10.0),
                    height: Val::Px(10.0),
                    ..default()
                }, BackgroundColor(Color::srgb(1.0, 0.15, 0.15)), ZIndex(52)),
            RadarBlip {
                lifetime: Timer::from_seconds(4.0, TimerMode::Once),
            },
        )).insert(ChildOf(radar_entity));
    }

    // Check AI ships — show as colored blips (blue=Cargo, red=Military, yellow=Salvager)
    for (ai_transform, ai_ship_type, radar_contact) in ai_ship_query.iter() {
        let ai_pos = ai_transform.translation.truncate();
        let offset = ai_pos - ship_pos;
        let dist = offset.length();

        if dist > radar_range {
            continue;
        }

        let entity_angle = offset.y.atan2(offset.x);
        let angle_diff = (entity_angle - sweep_state.angle).abs();
        let angle_diff = angle_diff.min(std::f32::consts::TAU - angle_diff);

        if angle_diff < sweep_tolerance || radar_contact.is_some() {
            let radar_x = (offset.x / radar_range) * radar_half + radar_half;
            let radar_y = radar_half - (offset.y / radar_range) * radar_half;

            let blip_color = match ai_ship_type {
                AiShipType::VoidTitan => Color::srgb(1.0, 0.85, 0.1),   // gold — unmistakable
                AiShipType::Dreadnought => Color::srgb(0.8, 0.05, 0.05), // deep crimson
                AiShipType::Leviathan => Color::srgb(0.2, 0.7, 0.6),
                AiShipType::AbyssalCult => Color::srgb(0.6, 0.2, 0.8),
                AiShipType::Drowned => Color::srgb(0.4, 0.5, 0.4),
                AiShipType::PressureKing => Color::srgb(0.3, 0.2, 0.5),
                AiShipType::GlassEye => Color::srgb(0.8, 0.85, 0.9),
                AiShipType::IronTide => Color::srgb(1.0, 0.2, 0.2),
                AiShipType::Blackwater => Color::srgb(0.3, 0.3, 0.4),
                AiShipType::RustSwarm => Color::srgb(0.9, 0.5, 0.2),
            };
            let blip_size = match ai_ship_type {
                AiShipType::VoidTitan | AiShipType::Dreadnought => 13.0, // bosses stand out
                _ => 8.0,
            };

            commands.spawn((
                (Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(radar_x - blip_size / 2.0),
                        top: Val::Px(radar_y - blip_size / 2.0),
                        width: Val::Px(blip_size),
                        height: Val::Px(blip_size),
                        ..default()
                    }, BackgroundColor(blip_color), ZIndex(51)),
                RadarBlip {
                    lifetime: Timer::from_seconds(4.0, TimerMode::Once),
                },
            )).insert(ChildOf(radar_entity));
        }
    }
}

/// Handles Z key radar ping - spawns expanding ring, reveals entities, generates noise
fn radar_ping_system(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    keyboard: Res<ButtonInput<KeyCode>>,
    radar_query: Query<(&Radar, &Module)>,
    ship_query: Query<&Transform, With<Ship>>,
    mut noise_state: ResMut<NoiseState>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    if !keyboard.just_pressed(KeyCode::KeyZ) {
        return;
    }

    let Ok(ship_transform) = ship_query.single() else { return };

    // Find active radar module
    let radar_data = radar_query.iter()
        .find(|(_, module)| module.is_active)
        .map(|(radar, _)| (radar.range, radar.noise_on_ping));

    let Some((range, noise)) = radar_data else {
        notifications.write(ShowNotification {
            message: "No active radar module!".into(),
            notification_type: NotificationType::Warning,
            duration: 2.0,
        });
        return;
    };

    // Spawn expanding ping ring visual
    commands.spawn((
        (Sprite {
                image: asset_server.load(crate::sprite_map::effect_sprite_path("radar_ring")),
                color: Color::srgba(0.2, 0.8, 0.2, 0.5),
                custom_size: Some(Vec2::splat(10.0)),
                ..default()
            }, Transform::from_translation(ship_transform.translation)),
        RadarPing {
            radius: 0.0,
            max_radius: range,
            speed: 400.0,
        },
    ));

    // Noise spike - attracts creatures
    noise_state.noise_level += noise;

    notifications.write(ShowNotification {
        message: "PING!".into(),
        notification_type: NotificationType::Info,
        duration: 1.0,
    });
}

/// Expands radar ping rings and reveals entities they pass through
fn update_radar_pings(
    mut commands: Commands,
    time: Res<Time>,
    mut ping_query: Query<(Entity, &mut RadarPing, &mut Sprite, &Transform), Without<Creature>>,
    creature_query: Query<(Entity, &Transform, &Creature), Without<RadarPing>>,
    ai_ship_ping_query: Query<(Entity, &Transform), (With<AiShip>, Without<RadarPing>, Without<Creature>)>,
) {
    for (ping_entity, mut ping, mut sprite, transform) in ping_query.iter_mut() {
        ping.radius += ping.speed * time.delta_secs();

        // Update visual size
        let diameter = ping.radius * 2.0;
        sprite.custom_size = Some(Vec2::splat(diameter));

        // Fade out as it expands
        let alpha = 1.0 - (ping.radius / ping.max_radius);
        sprite.color = Color::srgba(0.2, 0.8, 0.2, alpha.max(0.0) * 0.4);

        // Reveal creatures the ring passes through
        let ping_pos = transform.translation.truncate();
        for (c_entity, c_transform, _creature) in creature_query.iter() {
            let dist = c_transform.translation.truncate().distance(ping_pos);
            // Ring thickness of ~30 units
            if (dist - ping.radius).abs() < 30.0 {
                commands.entity(c_entity).insert(RadarRevealed {
                    timer: Timer::from_seconds(3.0, TimerMode::Once),
                });
            }
        }

        // Reveal AI ships the ring passes through
        for (ai_entity, ai_transform) in ai_ship_ping_query.iter() {
            let dist = ai_transform.translation.truncate().distance(ping_pos);
            if (dist - ping.radius).abs() < 30.0 {
                commands.entity(ai_entity).insert(AiShipRadarContact {
                    noise_signature: 50.0,
                    revealed_timer: Timer::from_seconds(3.0, TimerMode::Once),
                });
            }
        }

        // Despawn when max radius reached
        if ping.radius >= ping.max_radius {
            commands.entity(ping_entity).despawn();
        }
    }
}

/// Fades radar-revealed markers and ticks blip lifetimes
fn update_radar_revealed(
    mut commands: Commands,
    time: Res<Time>,
    mut revealed_query: Query<(Entity, &mut RadarRevealed)>,
    mut blip_query: Query<(Entity, &mut RadarBlip)>,
) {
    for (entity, mut revealed) in revealed_query.iter_mut() {
        revealed.timer.tick(time.delta());
        if revealed.timer.is_finished() {
            commands.entity(entity).remove::<RadarRevealed>();
        }
    }

    // Tick blip lifetimes
    for (entity, mut blip) in blip_query.iter_mut() {
        blip.lifetime.tick(time.delta());
        if blip.lifetime.is_finished() {
            commands.entity(entity).despawn();
        }
    }
}

/// Depth-based visibility: dims/hides entities beyond visibility range
fn update_depth_visibility(
    ship_state: Res<DepthState>,
    ship_query: Query<&Transform, With<Ship>>,
    light_query: Query<(&ShipLight, &Module)>,
    mut creature_query: Query<(&Transform, &mut Sprite, Option<&RadarRevealed>), (With<Creature>, Without<Ship>, Without<PointOfInterest>, Without<WorldDecoration>)>,
    mut poi_query: Query<(&Transform, &mut Sprite, Option<&RadarRevealed>), (With<PointOfInterest>, Without<Creature>, Without<Ship>, Without<WorldDecoration>)>,
    mut deco_query: Query<(&Transform, &mut Sprite), (With<WorldDecoration>, Without<Creature>, Without<PointOfInterest>, Without<Ship>)>,
) {
    let Ok(ship_transform) = ship_query.single() else { return };
    let ship_pos = ship_transform.translation.truncate();
    let depth = ship_state.current_depth;

    // Calculate visibility range based on depth zone
    let base_visibility = match depth {
        d if d < 200.0 => 800.0,
        d if d < 500.0 => 400.0,
        d if d < 1000.0 => 200.0,
        _ => 100.0,
    };

    // Sub lights extend visibility
    let light_bonus: f32 = light_query.iter()
        .filter(|(_, m)| m.is_active)
        .map(|(light, _)| light.range * light.intensity)
        .sum();

    let visibility_range = base_visibility + light_bonus * 0.5;

    // Apply to creatures
    for (transform, mut sprite, radar_revealed) in creature_query.iter_mut() {
        let dist = transform.translation.truncate().distance(ship_pos);

        if radar_revealed.is_some() {
            sprite.color.set_alpha(1.0);
        } else if dist > visibility_range {
            sprite.color.set_alpha(0.0);
        } else {
            let fade = 1.0 - ((dist / visibility_range).powi(2));
            sprite.color.set_alpha(fade.clamp(0.1, 1.0));
        }
    }

    // Apply to POIs
    for (transform, mut sprite, radar_revealed) in poi_query.iter_mut() {
        let dist = transform.translation.truncate().distance(ship_pos);

        if radar_revealed.is_some() {
            sprite.color.set_alpha(1.0);
        } else if dist > visibility_range {
            sprite.color.set_alpha(0.0);
        } else {
            let fade = 1.0 - ((dist / visibility_range).powi(2));
            sprite.color.set_alpha(fade.clamp(0.1, 1.0));
        }
    }

    // Apply to decorations
    for (transform, mut sprite) in deco_query.iter_mut() {
        let dist = transform.translation.truncate().distance(ship_pos);

        if dist > visibility_range {
            sprite.color.set_alpha(0.0);
        } else {
            let fade = 1.0 - ((dist / visibility_range).powi(2));
            sprite.color.set_alpha(fade.clamp(0.1, 1.0));
        }
    }

}
