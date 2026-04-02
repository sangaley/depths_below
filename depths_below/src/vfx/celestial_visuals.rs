use bevy::prelude::*;
use crate::celestial::components::*;

// ============================================================================
// CELESTIAL BODY VISUAL LAYERS
// Each celestial body gets multiple sprite layers for realistic appearance.
// Stars: core + corona + ambient glow
// Planets: body + shadow + optional atmosphere
// Black holes: center + accretion disk + outer distortion ring
// ============================================================================

/// Marks that an entity already has visual layers attached
#[derive(Component)]
pub struct HasVisualLayers;

/// Star glow layer — pulsing outer corona
#[derive(Component)]
pub struct StarGlow {
    pub base_alpha: f32,
    pub pulse_speed: f32,
    pub pulse_amplitude: f32,
}

/// Star corona layer — larger, dimmer outer ring
#[derive(Component)]
pub struct StarCorona;

/// Star flare visual — brightens during buildup
#[derive(Component)]
pub struct StarFlareGlow;

/// Planet atmosphere layer
#[derive(Component)]
pub struct PlanetAtmosphere {
    pub rotation_speed: f32,
}

/// Planet shadow (dark side away from star)
#[derive(Component)]
pub struct PlanetShadow;

/// Black hole accretion disk
#[derive(Component)]
pub struct AccretionDisk {
    pub rotation_speed: f32,
}

/// Black hole event horizon visual
#[derive(Component)]
pub struct EventHorizonVisual;

// ============================================================================
// ATTACH VISUAL LAYERS — runs once per entity when first seen
// ============================================================================

/// Attach glow layers to stars that don't have them yet
pub fn attach_star_visuals(
    mut commands: Commands,
    star_query: Query<(Entity, &CelestialBody, &Star), Without<HasVisualLayers>>,
) {
    for (entity, body, star) in star_query.iter() {
        let radius = body.radius;

        // Inner glow — bright, tight around the star
        let inner_glow = commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: star_glow_color(star.size_class, 0.35),
                    custom_size: Some(Vec2::splat(radius * 2.8)),
                    ..default()
                },
                transform: Transform::from_xyz(0.0, 0.0, -0.05),
                ..default()
            },
            StarGlow {
                base_alpha: 0.35,
                pulse_speed: 0.8 + star.luminosity * 0.3,
                pulse_amplitude: 0.08,
            },
        )).id();

        // Outer corona — wide, dim, atmospheric
        let corona = commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: star_corona_color(star.size_class, 0.12),
                    custom_size: Some(Vec2::splat(radius * 4.5)),
                    ..default()
                },
                transform: Transform::from_xyz(0.0, 0.0, -0.1),
                ..default()
            },
            StarCorona,
        )).id();

        // Flare glow — invisible until flare builds up
        let flare_glow = commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgba(1.0, 0.9, 0.7, 0.0),
                    custom_size: Some(Vec2::splat(radius * 6.0)),
                    ..default()
                },
                transform: Transform::from_xyz(0.0, 0.0, -0.15),
                ..default()
            },
            StarFlareGlow,
        )).id();

        commands.entity(entity)
            .insert(HasVisualLayers)
            .add_child(inner_glow)
            .add_child(corona)
            .add_child(flare_glow);
    }
}

/// Attach visual layers to planets
pub fn attach_planet_visuals(
    mut commands: Commands,
    planet_query: Query<(Entity, &CelestialBody, &Planet), Without<HasVisualLayers>>,
) {
    for (entity, body, planet) in planet_query.iter() {
        let radius = body.radius;

        // Atmosphere glow for gas/rocky planets with atmosphere
        if planet.has_atmosphere {
            let atmo_color = match planet.planet_type {
                PlanetType::Gas => Color::rgba(0.4, 0.5, 0.7, 0.15),
                PlanetType::Rocky => Color::rgba(0.5, 0.6, 0.8, 0.10),
                _ => Color::rgba(0.4, 0.4, 0.5, 0.08),
            };

            let atmosphere = commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color: atmo_color,
                        custom_size: Some(Vec2::splat(radius * 2.3)),
                        ..default()
                    },
                    transform: Transform::from_xyz(0.0, 0.0, -0.05),
                    ..default()
                },
                PlanetAtmosphere {
                    rotation_speed: 0.02,
                },
            )).id();

            commands.entity(entity).add_child(atmosphere);
        }

        // Shadow overlay — simulates dark side (offset from center)
        let shadow = commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgba(0.0, 0.0, 0.02, 0.5),
                    custom_size: Some(Vec2::splat(radius * 2.0)),
                    ..default()
                },
                transform: Transform::from_xyz(radius * 0.15, -radius * 0.1, 0.01),
                ..default()
            },
            PlanetShadow,
        )).id();

        commands.entity(entity)
            .insert(HasVisualLayers)
            .add_child(shadow);
    }
}

/// Attach visual layers to black holes
pub fn attach_black_hole_visuals(
    mut commands: Commands,
    bh_query: Query<(Entity, &CelestialBody, &BlackHole), Without<HasVisualLayers>>,
) {
    for (entity, _body, bh) in bh_query.iter() {
        // Event horizon — pitch black center
        let horizon = commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgba(0.0, 0.0, 0.0, 1.0),
                    custom_size: Some(Vec2::splat(bh.event_horizon_radius * 2.0)),
                    ..default()
                },
                transform: Transform::from_xyz(0.0, 0.0, 0.02),
                ..default()
            },
            EventHorizonVisual,
        )).id();

        // Accretion disk — spinning orange/red ring
        let disk_inner = commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgba(0.9, 0.4, 0.1, 0.6),
                    custom_size: Some(Vec2::new(bh.accretion_disk_radius * 2.5, bh.accretion_disk_radius * 0.4)),
                    ..default()
                },
                transform: Transform::from_xyz(0.0, 0.0, -0.02),
                ..default()
            },
            AccretionDisk { rotation_speed: 0.5 },
        )).id();

        // Outer accretion glow
        let disk_outer = commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgba(0.6, 0.15, 0.05, 0.25),
                    custom_size: Some(Vec2::new(bh.accretion_disk_radius * 4.0, bh.accretion_disk_radius * 0.8)),
                    ..default()
                },
                transform: Transform::from_xyz(0.0, 0.0, -0.05),
                ..default()
            },
            AccretionDisk { rotation_speed: 0.3 },
        )).id();

        // Gravitational distortion ring — faint purple/blue outer halo
        let distortion = commands.spawn(
            SpriteBundle {
                sprite: Sprite {
                    color: Color::rgba(0.2, 0.1, 0.3, 0.08),
                    custom_size: Some(Vec2::splat(bh.accretion_disk_radius * 6.0)),
                    ..default()
                },
                transform: Transform::from_xyz(0.0, 0.0, -0.1),
                ..default()
            },
        ).id();

        commands.entity(entity)
            .insert(HasVisualLayers)
            .add_child(horizon)
            .add_child(disk_inner)
            .add_child(disk_outer)
            .add_child(distortion);
    }
}

// ============================================================================
// ANIMATION SYSTEMS
// ============================================================================

/// Pulsing star glow
pub fn animate_star_glow(
    time: Res<Time>,
    mut glow_query: Query<(&StarGlow, &mut Sprite)>,
) {
    let t = time.elapsed_seconds();
    for (glow, mut sprite) in glow_query.iter_mut() {
        let pulse = glow.base_alpha + (t * glow.pulse_speed).sin() * glow.pulse_amplitude;
        sprite.color.set_a(pulse.clamp(0.05, 0.6));
    }
}

/// Star brightens as flare buildup increases
pub fn animate_star_flare_buildup(
    star_query: Query<(&Star, &Children)>,
    mut flare_glow_query: Query<&mut Sprite, With<StarFlareGlow>>,
) {
    for (star, children) in star_query.iter() {
        let flare_alpha = (star.flare_buildup / star.flare_threshold).clamp(0.0, 1.0) * 0.4;
        for child in children.iter() {
            if let Ok(mut sprite) = flare_glow_query.get_mut(*child) {
                sprite.color.set_a(flare_alpha);
            }
        }
    }
}

/// Spinning accretion disk
pub fn animate_black_hole_disk(
    time: Res<Time>,
    mut disk_query: Query<(&AccretionDisk, &mut Transform)>,
) {
    let dt = time.delta_seconds();
    for (disk, mut transform) in disk_query.iter_mut() {
        transform.rotation *= Quat::from_rotation_z(disk.rotation_speed * dt);
    }
}

/// Subtle atmosphere shimmer
pub fn animate_planet_atmosphere(
    time: Res<Time>,
    mut atmo_query: Query<(&PlanetAtmosphere, &mut Sprite)>,
) {
    let t = time.elapsed_seconds();
    for (_atmo, mut sprite) in atmo_query.iter_mut() {
        let shimmer = 0.10 + (t * 0.5).sin() * 0.03;
        sprite.color.set_a(shimmer);
    }
}

// ============================================================================
// COLOR HELPERS
// ============================================================================

fn star_glow_color(class: StarSizeClass, alpha: f32) -> Color {
    match class {
        StarSizeClass::Dwarf => Color::rgba(1.0, 0.6, 0.3, alpha),
        StarSizeClass::Main => Color::rgba(1.0, 0.95, 0.85, alpha),
        StarSizeClass::Giant => Color::rgba(1.0, 0.7, 0.3, alpha),
        StarSizeClass::Supergiant => Color::rgba(0.7, 0.8, 1.0, alpha),
    }
}

fn star_corona_color(class: StarSizeClass, alpha: f32) -> Color {
    match class {
        StarSizeClass::Dwarf => Color::rgba(1.0, 0.4, 0.15, alpha),
        StarSizeClass::Main => Color::rgba(1.0, 0.85, 0.6, alpha),
        StarSizeClass::Giant => Color::rgba(1.0, 0.5, 0.15, alpha),
        StarSizeClass::Supergiant => Color::rgba(0.5, 0.6, 1.0, alpha),
    }
}
