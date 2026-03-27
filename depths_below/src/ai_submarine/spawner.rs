use bevy::prelude::*;
use rand::Rng;

use crate::components::*;
use crate::building::registry::ModuleRegistry;
use crate::submarine::spawn_module;

use super::components::*;
use super::layouts::{self, HullCellDef};

/// Color tint per AI sub type
fn sub_tint(sub_type: AiSubType) -> Color {
    match sub_type {
        AiSubType::Leviathan => Color::rgb(0.2, 0.6, 0.5),    // teal - creature riders
        AiSubType::AbyssalCult => Color::rgb(0.4, 0.15, 0.5),  // purple - bio-organic cult
        AiSubType::Drowned => Color::rgb(0.35, 0.4, 0.35),     // ghostly gray-green
        AiSubType::PressureKing => Color::rgb(0.15, 0.1, 0.25),// dark violet - deep lords
        AiSubType::GlassEye => Color::rgb(0.85, 0.88, 0.9),    // white/translucent
        AiSubType::IronTide => Color::rgb(0.45, 0.45, 0.5),    // steel gray - battleship
        AiSubType::Blackwater => Color::rgb(0.2, 0.2, 0.25),   // dark tactical
        AiSubType::RustSwarm => Color::rgb(0.7, 0.4, 0.15),    // rusty orange
    }
}

/// Spawns a complete AI submarine at the given position.
/// Returns the root entity.
pub fn spawn_ai_submarine(
    sub_type: AiSubType,
    position: Vec2,
    commands: &mut Commands,
    registry: &ModuleRegistry,
    asset_server: &AssetServer,
) -> Entity {
    let layout = layouts::get_layout(sub_type);

    let mut rng = rand::thread_rng();

    // Generate patrol waypoints around spawn position
    let waypoints: Vec<Vec2> = (0..4)
        .map(|i| {
            let angle = std::f32::consts::TAU * (i as f32 / 4.0) + rng.gen_range(-0.3..0.3);
            let dist = rng.gen_range(300.0..600.0);
            position + Vec2::new(angle.cos() * dist, angle.sin() * dist)
        })
        .collect();

    let initial_behavior = match sub_type {
        AiSubType::Leviathan => AiSubBehavior::Patrolling,
        AiSubType::AbyssalCult => AiSubBehavior::Patrolling,
        AiSubType::Drowned => AiSubBehavior::Patrolling,
        AiSubType::PressureKing => AiSubBehavior::Patrolling,
        AiSubType::GlassEye => AiSubBehavior::FollowingTradeRoute,
        AiSubType::IronTide => AiSubBehavior::Patrolling,
        AiSubType::Blackwater => AiSubBehavior::Patrolling,
        AiSubType::RustSwarm => AiSubBehavior::Patrolling,
    };

    let fuel = match sub_type {
        AiSubType::Leviathan => 400.0,
        AiSubType::AbyssalCult => 600.0,
        AiSubType::Drowned => 200.0,
        AiSubType::PressureKing => 800.0,
        AiSubType::GlassEye => 500.0,
        AiSubType::IronTide => 1000.0,
        AiSubType::Blackwater => 700.0,
        AiSubType::RustSwarm => 150.0,
    };

    // Compute initial rotation: face toward first waypoint (or default to 0 = facing right)
    let initial_rotation = if let Some(&first_wp) = waypoints.first() {
        let dir = first_wp - position;
        if dir.length_squared() > 0.01 {
            dir.y.atan2(dir.x)
        } else {
            0.0
        }
    } else {
        0.0
    };

    // Spawn root entity (body sprite invisible — hull cells provide the visual)
    let root = commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: Color::NONE,
                custom_size: Some(layout.body_size),
                ..default()
            },
            transform: Transform {
                translation: Vec3::new(position.x, position.y, 0.05),
                rotation: Quat::from_rotation_z(initial_rotation),
                ..default()
            },
            texture: asset_server.load(crate::sprite_map::effect_sprite_path("submarine_body")),
            ..default()
        },
        AiSubmarine,
        sub_type,
        AiSubState {
            fuel,
            max_fuel: fuel,
            ..default()
        },
        initial_behavior,
        AiSubNav {
            waypoints: waypoints.clone(),
            destination: waypoints.first().copied(),
            ..default()
        },
        AiSubDecisionTimer::default(),
        Velocity(Vec2::ZERO),
        Depth(position.y.abs() / 10.0),
    )).id();

    // Spawn hull segments as children
    spawn_ai_hull(commands, asset_server, root, &layout.hull_cells);

    // Spawn modules as children, reusing existing spawn_module
    for mp in &layout.modules {
        let module_entity = spawn_module(
            commands,
            asset_server,
            root,
            mp.module_type,
            mp.grid_pos,
            mp.rotation,
            registry,
        );
        commands.entity(module_entity).insert(OwnedByAiSub { root });
    }

    root
}

/// Spawns hull segment children for the AI submarine
fn spawn_ai_hull(
    commands: &mut Commands,
    asset_server: &AssetServer,
    parent: Entity,
    hull_cells: &[HullCellDef],
) {
    for cell in hull_cells {
        let health = cell.material.health_multiplier() * 100.0;
        let depth_rating = cell.material.depth_rating();

        let x = cell.grid_pos.x as f32 * 66.0;
        let y = cell.grid_pos.y as f32 * 66.0 - 33.0;

        let hull_color = match cell.material {
            HullMaterial::Steel => Color::rgb(0.4, 0.4, 0.45),
            HullMaterial::Titanium => Color::rgb(0.55, 0.55, 0.6),
            HullMaterial::Composite => Color::rgb(0.3, 0.45, 0.35),
            HullMaterial::AbyssalAlloy => Color::rgb(0.2, 0.15, 0.3),
        };

        let hull_entity = commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: hull_color,
                    custom_size: Some(Vec2::splat(60.0)),
                    ..default()
                },
                transform: Transform::from_xyz(x, y, 0.1),
                texture: asset_server.load(
                    crate::sprite_map::hull_sprite_path(cell.material),
                ),
                ..default()
            },
            HullSegment {
                health,
                max_health: health,
                depth_rating,
                is_flooded: false,
                flood_level: 0.0,
                hull_layer: cell.layer,
                material: cell.material,
                grid_position: cell.grid_pos,
            },
            OwnedByAiSub { root: parent },
        )).id();

        commands.entity(hull_entity).set_parent(parent);
    }
}
