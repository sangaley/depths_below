use bevy::prelude::*;
use crate::components::*;
use crate::resources::*;
use crate::events::*;
use crate::states::{GameState, SonarSet};
use crate::ai_submarine::components::{AiSubmarine, AiSubType, AiSubSonarContact};

/// Marker for the sonar radar display UI
#[derive(Component)]
pub struct SonarRadarDisplay;

/// Marker for a sonar blip (dot on radar)
#[derive(Component)]
pub struct SonarBlip {
    pub lifetime: Timer,
}

/// Sonar sweep line angle
#[derive(Resource)]
pub struct SonarSweepState {
    pub angle: f32,
    pub sweep_speed: f32,
    pub display_visible: bool,
}

impl Default for SonarSweepState {
    fn default() -> Self {
        Self {
            angle: 0.0,
            sweep_speed: std::f32::consts::TAU / 4.0, // Full rotation every 4 seconds
            display_visible: false,
        }
    }
}

pub struct SonarPlugin;

impl Plugin for SonarPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<SonarSweepState>()
            .configure_set(Update, SonarSet::Input.run_if(in_state(GameState::Exploring)))
            .configure_set(Update, SonarSet::Update.after(SonarSet::Input).run_if(in_state(GameState::Exploring)))
            .configure_set(Update, SonarSet::Visibility.after(SonarSet::Update).run_if(in_state(GameState::Exploring)))
            .add_systems(
                Update,
                (
                    sonar_ping_system.in_set(SonarSet::Input),
                    toggle_sonar_display.in_set(SonarSet::Input),
                    update_sonar_pings.in_set(SonarSet::Update),
                    update_sonar_revealed.in_set(SonarSet::Update),
                    update_sonar_radar.in_set(SonarSet::Update),
                    update_depth_visibility.in_set(SonarSet::Visibility),
                ),
            );
    }
}

/// Toggle sonar radar display with Tab key
fn toggle_sonar_display(
    keyboard: Res<Input<KeyCode>>,
    mut sweep_state: ResMut<SonarSweepState>,
    mut commands: Commands,
    existing_display: Query<Entity, With<SonarRadarDisplay>>,
    sonar_query: Query<(&Sonar, &Module)>,
) {
    // Tab toggles radar display (Z is for ping)
    if keyboard.just_pressed(KeyCode::Tab) {
        let has_sonar = sonar_query.iter().any(|(_, m)| m.is_active);
        if !has_sonar {
            return;
        }

        if sweep_state.display_visible {
            // Hide display
            sweep_state.display_visible = false;
            for entity in existing_display.iter() {
                commands.entity(entity).despawn_recursive();
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
        NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                right: Val::Px(10.0),
                bottom: Val::Px(10.0),
                width: Val::Px(radar_size),
                height: Val::Px(radar_size),
                ..default()
            },
            background_color: Color::rgba(0.0, 0.1, 0.0, 0.7).into(),
            z_index: ZIndex::Global(50),
            ..default()
        },
        SonarRadarDisplay,
    )).with_children(|parent| {
        // Title
        parent.spawn(TextBundle {
            text: Text::from_section("SONAR", TextStyle {
                font_size: 12.0,
                color: Color::rgb(0.2, 0.8, 0.2),
                ..default()
            }),
            style: Style {
                position_type: PositionType::Absolute,
                left: Val::Px(2.0),
                top: Val::Px(2.0),
                ..default()
            },
            ..default()
        });

        // Center dot (submarine position)
        parent.spawn(NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                left: Val::Px(radar_size / 2.0 - 3.0),
                top: Val::Px(radar_size / 2.0 - 3.0),
                width: Val::Px(6.0),
                height: Val::Px(6.0),
                ..default()
            },
            background_color: Color::rgb(0.3, 1.0, 0.3).into(),
            ..default()
        });

        // Range rings (inner and outer)
        for ring_frac in [0.33, 0.66, 1.0] {
            let ring_size = radar_size * ring_frac;
            let offset = (radar_size - ring_size) / 2.0;
            parent.spawn(NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    left: Val::Px(offset),
                    top: Val::Px(offset),
                    width: Val::Px(ring_size),
                    height: Val::Px(ring_size),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                border_color: Color::rgba(0.1, 0.4, 0.1, 0.5).into(),
                background_color: Color::NONE.into(),
                ..default()
            });
        }
    });
}

/// Update the sonar radar display - sweep line and blips
pub fn update_sonar_radar(
    time: Res<Time>,
    mut sweep_state: ResMut<SonarSweepState>,
    radar_display: Query<Entity, With<SonarRadarDisplay>>,
    sonar_query: Query<(&Sonar, &Module)>,
    sub_query: Query<&Transform, With<Submarine>>,
    creature_query: Query<(&Transform, &Creature, Option<&SonarRevealed>), Without<Submarine>>,
    poi_query: Query<(&Transform, &PointOfInterest), Without<Submarine>>,
    ai_sub_query: Query<(&Transform, &AiSubType, Option<&AiSubSonarContact>), With<AiSubmarine>>,
    mut commands: Commands,
) {
    if !sweep_state.display_visible {
        return;
    }

    let Ok(radar_entity) = radar_display.get_single() else { return };
    let Ok(sub_transform) = sub_query.get_single() else { return };
    let sub_pos = sub_transform.translation.truncate();

    // Get sonar range
    let sonar_range = sonar_query.iter()
        .filter(|(_, m)| m.is_active)
        .map(|(s, _)| s.range)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or(500.0);

    let radar_size = 180.0;
    let radar_half = radar_size / 2.0;

    // Advance sweep angle
    sweep_state.angle += sweep_state.sweep_speed * time.delta_seconds();
    if sweep_state.angle > std::f32::consts::TAU {
        sweep_state.angle -= std::f32::consts::TAU;
    }

    // Note: blip lifetime ticking and despawning is handled by update_sonar_revealed

    // Spawn blips for entities near the sweep line
    let sweep_tolerance = 0.15; // Radians - how wide the sweep detection is

    // Check creatures
    for (c_transform, creature, sonar_revealed) in creature_query.iter() {
        let c_pos = c_transform.translation.truncate();
        let offset = c_pos - sub_pos;
        let dist = offset.length();

        if dist > sonar_range {
            continue;
        }

        // Check if entity is near the sweep line
        let entity_angle = offset.y.atan2(offset.x);
        let angle_diff = (entity_angle - sweep_state.angle).abs();
        let angle_diff = angle_diff.min(std::f32::consts::TAU - angle_diff);

        if angle_diff < sweep_tolerance || sonar_revealed.is_some() {
            // Map world position to radar position
            let radar_x = (offset.x / sonar_range) * radar_half + radar_half;
            let radar_y = radar_half - (offset.y / sonar_range) * radar_half; // Flip Y

            // Blip color based on creature threat
            let blip_color = match creature.creature_type {
                CreatureType::Leviathan | CreatureType::SwarmQueen => Color::rgb(1.0, 0.2, 0.2),
                CreatureType::Stalker | CreatureType::BlindHunter => Color::rgb(1.0, 0.6, 0.2),
                _ => Color::rgb(1.0, 1.0, 0.3),
            };

            // Blip size based on creature threat
            let blip_size = match creature.creature_type {
                CreatureType::Leviathan => 8.0,
                CreatureType::SwarmQueen => 6.0,
                _ => 4.0,
            };

            commands.spawn((
                NodeBundle {
                    style: Style {
                        position_type: PositionType::Absolute,
                        left: Val::Px(radar_x - blip_size / 2.0),
                        top: Val::Px(radar_y - blip_size / 2.0),
                        width: Val::Px(blip_size),
                        height: Val::Px(blip_size),
                        ..default()
                    },
                    background_color: blip_color.into(),
                    z_index: ZIndex::Global(51),
                    ..default()
                },
                SonarBlip {
                    lifetime: Timer::from_seconds(4.0, TimerMode::Once),
                },
            )).set_parent(radar_entity);
        }
    }

    // Check POIs - show as cyan blips
    for (poi_transform, _poi) in poi_query.iter() {
        let p_pos = poi_transform.translation.truncate();
        let offset = p_pos - sub_pos;
        let dist = offset.length();

        if dist > sonar_range {
            continue;
        }

        let entity_angle = offset.y.atan2(offset.x);
        let angle_diff = (entity_angle - sweep_state.angle).abs();
        let angle_diff = angle_diff.min(std::f32::consts::TAU - angle_diff);

        if angle_diff < sweep_tolerance {
            let radar_x = (offset.x / sonar_range) * radar_half + radar_half;
            let radar_y = radar_half - (offset.y / sonar_range) * radar_half;

            commands.spawn((
                NodeBundle {
                    style: Style {
                        position_type: PositionType::Absolute,
                        left: Val::Px(radar_x - 3.0),
                        top: Val::Px(radar_y - 3.0),
                        width: Val::Px(6.0),
                        height: Val::Px(6.0),
                        ..default()
                    },
                    background_color: Color::rgb(0.2, 0.8, 1.0).into(),
                    z_index: ZIndex::Global(51),
                    ..default()
                },
                SonarBlip {
                    lifetime: Timer::from_seconds(4.0, TimerMode::Once),
                },
            )).set_parent(radar_entity);
        }
    }

    // Check AI submarines — show as colored blips (blue=Cargo, red=Military, yellow=Salvager)
    for (ai_transform, ai_sub_type, sonar_contact) in ai_sub_query.iter() {
        let ai_pos = ai_transform.translation.truncate();
        let offset = ai_pos - sub_pos;
        let dist = offset.length();

        if dist > sonar_range {
            continue;
        }

        let entity_angle = offset.y.atan2(offset.x);
        let angle_diff = (entity_angle - sweep_state.angle).abs();
        let angle_diff = angle_diff.min(std::f32::consts::TAU - angle_diff);

        if angle_diff < sweep_tolerance || sonar_contact.is_some() {
            let radar_x = (offset.x / sonar_range) * radar_half + radar_half;
            let radar_y = radar_half - (offset.y / sonar_range) * radar_half;

            let blip_color = match ai_sub_type {
                AiSubType::Leviathan => Color::rgb(0.2, 0.7, 0.6),
                AiSubType::AbyssalCult => Color::rgb(0.6, 0.2, 0.8),
                AiSubType::Drowned => Color::rgb(0.4, 0.5, 0.4),
                AiSubType::PressureKing => Color::rgb(0.3, 0.2, 0.5),
                AiSubType::GlassEye => Color::rgb(0.8, 0.85, 0.9),
                AiSubType::IronTide => Color::rgb(1.0, 0.2, 0.2),
                AiSubType::Blackwater => Color::rgb(0.3, 0.3, 0.4),
                AiSubType::RustSwarm => Color::rgb(0.9, 0.5, 0.2),
            };

            commands.spawn((
                NodeBundle {
                    style: Style {
                        position_type: PositionType::Absolute,
                        left: Val::Px(radar_x - 4.0),
                        top: Val::Px(radar_y - 4.0),
                        width: Val::Px(8.0),
                        height: Val::Px(8.0),
                        ..default()
                    },
                    background_color: blip_color.into(),
                    z_index: ZIndex::Global(51),
                    ..default()
                },
                SonarBlip {
                    lifetime: Timer::from_seconds(4.0, TimerMode::Once),
                },
            )).set_parent(radar_entity);
        }
    }
}

/// Handles Z key sonar ping - spawns expanding ring, reveals entities, generates noise
fn sonar_ping_system(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    keyboard: Res<Input<KeyCode>>,
    sonar_query: Query<(&Sonar, &Module)>,
    sub_query: Query<&Transform, With<Submarine>>,
    mut noise_state: ResMut<NoiseState>,
    mut notifications: EventWriter<ShowNotification>,
) {
    if !keyboard.just_pressed(KeyCode::Z) {
        return;
    }

    let Ok(sub_transform) = sub_query.get_single() else { return };

    // Find active sonar module
    let sonar_data = sonar_query.iter()
        .find(|(_, module)| module.is_active)
        .map(|(sonar, _)| (sonar.range, sonar.noise_on_ping));

    let Some((range, noise)) = sonar_data else {
        notifications.send(ShowNotification {
            message: "No active sonar module!".into(),
            notification_type: NotificationType::Warning,
            duration: 2.0,
        });
        return;
    };

    // Spawn expanding ping ring visual
    commands.spawn((
        SpriteBundle {
            texture: asset_server.load(crate::sprite_map::effect_sprite_path("sonar_ring")),
            sprite: Sprite {
                color: Color::rgba(0.2, 0.8, 0.2, 0.5),
                custom_size: Some(Vec2::splat(10.0)),
                ..default()
            },
            transform: Transform::from_translation(sub_transform.translation),
            ..default()
        },
        SonarPing {
            radius: 0.0,
            max_radius: range,
            speed: 400.0,
        },
    ));

    // Noise spike - attracts creatures
    noise_state.noise_level += noise;

    notifications.send(ShowNotification {
        message: "PING!".into(),
        notification_type: NotificationType::Info,
        duration: 1.0,
    });
}

/// Expands sonar ping rings and reveals entities they pass through
fn update_sonar_pings(
    mut commands: Commands,
    time: Res<Time>,
    mut ping_query: Query<(Entity, &mut SonarPing, &mut Sprite, &Transform), Without<Creature>>,
    creature_query: Query<(Entity, &Transform, &Creature), Without<SonarPing>>,
    ai_sub_ping_query: Query<(Entity, &Transform), (With<AiSubmarine>, Without<SonarPing>, Without<Creature>)>,
) {
    for (ping_entity, mut ping, mut sprite, transform) in ping_query.iter_mut() {
        ping.radius += ping.speed * time.delta_seconds();

        // Update visual size
        let diameter = ping.radius * 2.0;
        sprite.custom_size = Some(Vec2::splat(diameter));

        // Fade out as it expands
        let alpha = 1.0 - (ping.radius / ping.max_radius);
        sprite.color = Color::rgba(0.2, 0.8, 0.2, alpha.max(0.0) * 0.4);

        // Reveal creatures the ring passes through
        let ping_pos = transform.translation.truncate();
        for (c_entity, c_transform, _creature) in creature_query.iter() {
            let dist = c_transform.translation.truncate().distance(ping_pos);
            // Ring thickness of ~30 units
            if (dist - ping.radius).abs() < 30.0 {
                commands.entity(c_entity).insert(SonarRevealed {
                    timer: Timer::from_seconds(3.0, TimerMode::Once),
                });
            }
        }

        // Reveal AI submarines the ring passes through
        for (ai_entity, ai_transform) in ai_sub_ping_query.iter() {
            let dist = ai_transform.translation.truncate().distance(ping_pos);
            if (dist - ping.radius).abs() < 30.0 {
                commands.entity(ai_entity).insert(AiSubSonarContact {
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

/// Fades sonar-revealed markers and ticks blip lifetimes
fn update_sonar_revealed(
    mut commands: Commands,
    time: Res<Time>,
    mut revealed_query: Query<(Entity, &mut SonarRevealed)>,
    mut blip_query: Query<(Entity, &mut SonarBlip)>,
) {
    for (entity, mut revealed) in revealed_query.iter_mut() {
        revealed.timer.tick(time.delta());
        if revealed.timer.finished() {
            commands.entity(entity).remove::<SonarRevealed>();
        }
    }

    // Tick blip lifetimes
    for (entity, mut blip) in blip_query.iter_mut() {
        blip.lifetime.tick(time.delta());
        if blip.lifetime.finished() {
            commands.entity(entity).despawn_recursive();
        }
    }
}

/// Depth-based visibility: dims/hides entities beyond visibility range
fn update_depth_visibility(
    sub_state: Res<DepthState>,
    sub_query: Query<&Transform, With<Submarine>>,
    light_query: Query<(&SubmarineLight, &Module)>,
    mut creature_query: Query<(&Transform, &mut Sprite, Option<&SonarRevealed>), (With<Creature>, Without<Submarine>, Without<PointOfInterest>, Without<WorldDecoration>, Without<AmbientCreature>)>,
    mut poi_query: Query<(&Transform, &mut Sprite, Option<&SonarRevealed>), (With<PointOfInterest>, Without<Creature>, Without<Submarine>, Without<WorldDecoration>, Without<AmbientCreature>)>,
    mut deco_query: Query<(&Transform, &mut Sprite), (With<WorldDecoration>, Without<Creature>, Without<PointOfInterest>, Without<Submarine>, Without<AmbientCreature>)>,
    mut ambient_query: Query<(&Transform, &mut Sprite, &AmbientCreature), (Without<Creature>, Without<PointOfInterest>, Without<Submarine>, Without<WorldDecoration>)>,
) {
    let Ok(sub_transform) = sub_query.get_single() else { return };
    let sub_pos = sub_transform.translation.truncate();
    let depth = sub_state.current_depth;

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
    for (transform, mut sprite, sonar_revealed) in creature_query.iter_mut() {
        let dist = transform.translation.truncate().distance(sub_pos);

        if sonar_revealed.is_some() {
            sprite.color.set_a(1.0);
        } else if dist > visibility_range {
            sprite.color.set_a(0.0);
        } else {
            let fade = 1.0 - ((dist / visibility_range).powi(2));
            sprite.color.set_a(fade.clamp(0.1, 1.0));
        }
    }

    // Apply to POIs
    for (transform, mut sprite, sonar_revealed) in poi_query.iter_mut() {
        let dist = transform.translation.truncate().distance(sub_pos);

        if sonar_revealed.is_some() {
            sprite.color.set_a(1.0);
        } else if dist > visibility_range {
            sprite.color.set_a(0.0);
        } else {
            let fade = 1.0 - ((dist / visibility_range).powi(2));
            sprite.color.set_a(fade.clamp(0.1, 1.0));
        }
    }

    // Apply to decorations
    for (transform, mut sprite) in deco_query.iter_mut() {
        let dist = transform.translation.truncate().distance(sub_pos);

        if dist > visibility_range {
            sprite.color.set_a(0.0);
        } else {
            let fade = 1.0 - ((dist / visibility_range).powi(2));
            sprite.color.set_a(fade.clamp(0.1, 1.0));
        }
    }

    // Apply to ambient life
    for (transform, mut sprite, ambient) in ambient_query.iter_mut() {
        let dist = transform.translation.truncate().distance(sub_pos);

        let max_alpha = match ambient.kind {
            AmbientKind::Jellyfish => 0.35,
            AmbientKind::GiantSquid => 0.25,
            AmbientKind::Whale => 0.3,
            AmbientKind::DeepFish => 0.6,
            _ => 0.8,
        };

        if dist > visibility_range {
            sprite.color.set_a(0.0);
        } else {
            let fade = 1.0 - ((dist / visibility_range).powi(2));
            sprite.color.set_a((fade * max_alpha).clamp(0.0, max_alpha));
        }
    }
}
