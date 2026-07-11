use bevy::prelude::*;
use rand::Rng;

use crate::building::registry::{CompanionData, ModuleRegistry};
use crate::components::*;
use crate::ship::spawn_module;

use super::components::*;
use super::layouts::{self, HullCellDef};

/// How much tougher and harder-hitting a ship gets the farther its spawn
/// position sits from Haven Station (world origin). Distance is normalized
/// against ~350,000 units — roughly the farthest faction territory (Pressure
/// Kings) — and clamped so the far-out wanderers past that don't run away
/// with the multiplier. Health scales more aggressively than weapon damage
/// so distant ships read as tankier fights, not instant-death ones.
fn distance_difficulty(position: Vec2) -> (f32, f32) {
    let t = (position.length() / 350_000.0).clamp(0.0, 1.0);
    let health_mult = 1.0 + t * 1.5; // up to 2.5x hull HP far out
    let damage_mult = 1.0 + t * 0.8; // up to 1.8x weapon damage far out
    (health_mult, damage_mult)
}

/// Color tint per AI ship type
fn ship_tint(ship_type: AiShipType) -> Color {
    match ship_type {
        AiShipType::Leviathan => Color::srgb(0.2, 0.6, 0.5),    // teal - creature riders
        AiShipType::AbyssalCult => Color::srgb(0.4, 0.15, 0.5),  // purple - bio-organic cult
        AiShipType::Drowned => Color::srgb(0.35, 0.4, 0.35),     // ghostly gray-green
        AiShipType::PressureKing => Color::srgb(0.15, 0.1, 0.25),// dark violet - deep lords
        AiShipType::GlassEye => Color::srgb(0.85, 0.88, 0.9),    // white/translucent
        AiShipType::IronTide => Color::srgb(0.45, 0.45, 0.5),    // steel gray - battleship
        AiShipType::Blackwater => Color::srgb(0.2, 0.2, 0.25),   // dark tactical
        AiShipType::RustSwarm => Color::srgb(0.7, 0.4, 0.15),    // rusty orange
    }
}

/// Spawns a complete AI ship at the given position.
/// Returns the root entity.
pub fn spawn_ai_ship(
    ship_type: AiShipType,
    position: Vec2,
    commands: &mut Commands,
    registry: &ModuleRegistry,
    asset_server: &AssetServer,
) -> Entity {
    let layout = layouts::get_layout(ship_type);

    let mut rng = rand::thread_rng();

    // Generate patrol waypoints around spawn position. Wider than the old
    // 300-600 range so a rendered ship visibly roams across the screen
    // instead of pacing a tight little loop right where it spawned.
    let waypoints: Vec<Vec2> = (0..4)
        .map(|i| {
            let angle = std::f32::consts::TAU * (i as f32 / 4.0) + rng.gen_range(-0.3..0.3);
            let dist = rng.gen_range(500.0..1000.0);
            position + Vec2::new(angle.cos() * dist, angle.sin() * dist)
        })
        .collect();

    let initial_behavior = match ship_type {
        AiShipType::Leviathan => AiShipBehavior::Patrolling,
        AiShipType::AbyssalCult => AiShipBehavior::Patrolling,
        AiShipType::Drowned => AiShipBehavior::Patrolling,
        AiShipType::PressureKing => AiShipBehavior::Patrolling,
        AiShipType::GlassEye => AiShipBehavior::FollowingTradeRoute,
        AiShipType::IronTide => AiShipBehavior::Patrolling,
        AiShipType::Blackwater => AiShipBehavior::Patrolling,
        AiShipType::RustSwarm => AiShipBehavior::Patrolling,
    };

    let fuel = match ship_type {
        AiShipType::Leviathan => 400.0,
        AiShipType::AbyssalCult => 600.0,
        AiShipType::Drowned => 200.0,
        AiShipType::PressureKing => 800.0,
        AiShipType::GlassEye => 500.0,
        AiShipType::IronTide => 1000.0,
        AiShipType::Blackwater => 700.0,
        AiShipType::RustSwarm => 150.0,
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
        (Sprite {
                image: asset_server.load(crate::sprite_map::effect_sprite_path("ship_body")),
                color: Color::NONE,
                custom_size: Some(layout.body_size),
                ..default()
            }, Transform {
                translation: Vec3::new(position.x, position.y, 0.05),
                rotation: Quat::from_rotation_z(initial_rotation),
                ..default()
            }),
        AiShip,
        ship_type,
        AiShipState {
            fuel,
            max_fuel: fuel,
            ..default()
        },
        initial_behavior,
        AiShipNav {
            waypoints: waypoints.clone(),
            destination: waypoints.first().copied(),
            ..default()
        },
        AiShipDecisionTimer::default(),
        Velocity(Vec2::ZERO),
        Depth(position.y.abs() / 10.0),
    )).id();

    let (health_mult, damage_mult) = distance_difficulty(position);

    // Spawn hull segments as children
    spawn_ai_hull(commands, asset_server, root, &layout.hull_cells, health_mult);

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
        commands.entity(module_entity).insert(OwnedByAiShip { root });

        // Distant ships hit harder too — scale weapon damage by the same
        // distance factor (see distance_difficulty doc comment).
        if damage_mult > 1.0 {
            if let CompanionData::Weapon { damage, range, fire_rate, ammo, .. } = &registry.get(mp.module_type).companion {
                commands.entity(module_entity).insert(Weapon {
                    damage: damage * damage_mult,
                    range: *range,
                    fire_rate: *fire_rate,
                    ammo: *ammo,
                    max_ammo: *ammo,
                });
            }
        }
    }

    root
}

/// Spawns hull segment children for the AI ship
fn spawn_ai_hull(
    commands: &mut Commands,
    asset_server: &AssetServer,
    parent: Entity,
    hull_cells: &[HullCellDef],
    health_mult: f32,
) {
    for cell in hull_cells {
        let health = cell.material.health_multiplier() * 100.0 * health_mult;
        let radiation_shielding = cell.material.radiation_shielding();

        let x = cell.grid_pos.x as f32 * 66.0;
        let y = cell.grid_pos.y as f32 * 66.0 - 33.0;

        let hull_color = match cell.material {
            HullMaterial::Steel => Color::srgb(0.55, 0.55, 0.6),
            HullMaterial::Titanium => Color::srgb(0.7, 0.7, 0.75),
            HullMaterial::Composite => Color::srgb(0.45, 0.62, 0.5),
            HullMaterial::AbyssalAlloy => Color::srgb(0.35, 0.28, 0.45),
        };

        let hull_entity = commands.spawn((
            (Sprite {
                    image: asset_server.load(
                        crate::sprite_map::hull_sprite_path(cell.material),
                    ),
                    color: hull_color,
                    custom_size: Some(Vec2::splat(60.0)),
                    ..default()
                }, Transform::from_xyz(x, y, 0.1)),
            BaseSpriteColor(hull_color),
            BaseHullStats {
                max_health: health,
                radiation_shielding,
            },
            HullSegment {
                health,
                max_health: health,
                radiation_shielding,
                is_depressurized: false,
                depressurization_level: 0.0,
                hull_layer: cell.layer,
                material: cell.material,
                grid_position: cell.grid_pos,
            },
            OwnedByAiShip { root: parent },
        )).id();

        commands.entity(hull_entity).insert(ChildOf(parent));
    }
}
