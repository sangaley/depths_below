use bevy::prelude::*;
use crate::components::{Ship, Module, ModuleType};
use crate::ai_ship::components::{AiShip, AiShipType};

// ============================================================================
// SHIP SHIELDS — Starsector-style visible shield bubble.
// Every ship carries a shield that absorbs incoming fire until depleted;
// it recharges after a few seconds without hits. Only once the shield is
// down do shots reach the hull and damage individual blocks.
// ============================================================================

#[derive(Component)]
pub struct ShipShield {
    pub current: f32,
    pub max: f32,
    pub recharge_rate: f32,
    /// Seconds without taking a hit before recharge begins
    pub recharge_delay: f32,
    pub since_hit: f32,
    /// Collision radius of the bubble
    pub radius: f32,
    /// Bubble center in ship-local space (centroid of the blocks — the ship
    /// root is often at one end of the layout, not the middle)
    pub center_offset: Vec2,
    /// Visual hit-flash intensity, decays each frame
    pub flash: f32,
    /// Player toggle (R). AI shields are always enabled.
    pub enabled: bool,
    /// Recent-hit power surge — extra power drain that decays over time
    pub surge: f32,
}

impl ShipShield {
    pub fn absorb(&mut self, damage: f32) {
        self.current = (self.current - damage).max(0.0);
        self.since_hit = 0.0;
        self.flash = 1.0;
    }
    pub fn is_up(&self) -> bool {
        self.enabled && self.current > 0.0
    }
    /// World-space center of the bubble for hit tests
    pub fn world_center(&self, transform: &Transform) -> Vec2 {
        transform.translation.truncate()
            + (transform.rotation * self.center_offset.extend(0.0)).truncate()
    }
}

/// Marker for the bubble visual (child sprite of the shielded ship)
#[derive(Component)]
pub struct ShieldBubble;

const BUBBLE_SPRITE: &str = "sprites/effects/shield_bubble.png";

/// Bubble geometry that actually wraps the ship: centered on the blocks'
/// centroid (the root is often at one end of the layout), radius = farthest
/// block from that centroid plus margin. Root-centered fixed radii produced
/// huge off-center bubbles with the ship poking out one side.
fn ship_extent(ship: Entity, modules: &Query<(&Transform, &ChildOf), With<Module>>) -> (Vec2, f32) {
    let positions: Vec<Vec2> = modules.iter()
        .filter(|(_, p)| p.parent() == ship)
        .map(|(t, _)| t.translation.truncate())
        .collect();
    if positions.is_empty() {
        return (Vec2::ZERO, 200.0);
    }
    let centroid = positions.iter().sum::<Vec2>() / positions.len() as f32;
    let max_dist = positions.iter()
        .map(|p| p.distance(centroid))
        .fold(0.0_f32, f32::max);
    (centroid, (max_dist + 70.0).max(150.0))
}

fn spawn_bubble(commands: &mut Commands, asset_server: &AssetServer, owner: Entity, center: Vec2, radius: f32) {
    let bubble = commands.spawn((
        Sprite {
            image: asset_server.load(BUBBLE_SPRITE),
            color: Color::srgba(0.5, 0.8, 1.0, 0.35),
            custom_size: Some(Vec2::splat(radius * 2.0)),
            ..default()
        },
        Transform::from_xyz(center.x, center.y, 0.9),
        ShieldBubble,
    )).id();
    commands.entity(owner).add_child(bubble);
}

/// Attach a shield to the player ship (scaled by Shield Emitter modules).
pub fn attach_player_shield(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    ship_query: Query<Entity, (With<Ship>, Without<ShipShield>)>,
    module_query: Query<(&Module, &ChildOf)>,
    transform_query: Query<(&Transform, &ChildOf), With<Module>>,
) {
    let Ok(ship) = ship_query.single() else { return };

    // Wait until the ship actually has modules so the extent is real
    let emitters = module_query.iter()
        .filter(|(m, p)| p.parent() == ship && m.module_type == ModuleType::ShieldEmitter && m.health > 0.0)
        .count() as f32;
    let module_count = module_query.iter().filter(|(_, p)| p.parent() == ship).count();
    if module_count == 0 { return; }

    let max = 40.0 + emitters * 40.0;
    let (center, radius) = ship_extent(ship, &transform_query);

    commands.entity(ship).insert(ShipShield {
        current: max,
        max,
        recharge_rate: 12.0,
        recharge_delay: 4.0,
        since_hit: 999.0,
        radius,
        center_offset: center,
        flash: 0.0,
        enabled: true,
        surge: 0.0,
    });
    spawn_bubble(&mut commands, &asset_server, ship, center, radius);
}

/// Attach shields to AI ships as they spawn, sized by faction.
pub fn attach_ai_shields(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    ai_query: Query<(Entity, &AiShipType, &Children), (With<AiShip>, Without<ShipShield>)>,
    transform_query: Query<(&Transform, &ChildOf), With<Module>>,
) {
    // DEPTHS_MOVETEST_ENEMY: zero-HP shield on the test dummy. The shield
    // component is still what defines the ship's hittable radius/center in
    // check_projectile_hits (it's not just an HP pool) — dropping the
    // component entirely last time meant ai_ship_query's `&mut ShipShield`
    // requirement silently excluded the dummy from hit detection altogether,
    // so shots passed straight through with no collision at all. Zero HP
    // means is_up() is false (no absorption) while the geometry stays real.
    let no_shield_hp = std::env::var("DEPTHS_MOVETEST_ENEMY").ok().as_deref() == Some("1");

    for (entity, ship_type, children) in ai_query.iter() {
        // Wait until the AI ship's modules exist so the extent is real
        if !children.iter().any(|c| transform_query.get(c).is_ok()) { continue; }

        // Deliberately weak: the shield is a brief opener, the real fight is
        // carving up the hull block by block.
        let max = if no_shield_hp { 0.0 } else {
            match ship_type {
                AiShipType::IronTide => 40.0,      // tanky battleship
                AiShipType::PressureKing => 30.0,
                AiShipType::Blackwater => 24.0,
                AiShipType::Leviathan => 22.0,
                AiShipType::AbyssalCult => 20.0,
                AiShipType::GlassEye => 12.0,
                AiShipType::Drowned => 6.0,        // half-dead ghost ships
                AiShipType::RustSwarm => 4.0,      // junk ships, paper shields
            }
        };
        let (center, radius) = ship_extent(entity, &transform_query);
        commands.entity(entity).insert(ShipShield {
            current: max,
            max,
            recharge_rate: 8.0,
            recharge_delay: 5.0,
            since_hit: 999.0,
            radius,
            center_offset: center,
            flash: 0.0,
            enabled: true,
            surge: 0.0,
        });
        spawn_bubble(&mut commands, &asset_server, entity, center, radius);
    }
}

/// Passive power the player's shield draws while enabled. Hits add a surge
/// on top (see ShipShield::absorb) — sustained fire can push the ship's
/// power balance negative, which collapses the shield AND silences weapons.
pub const SHIELD_UPKEEP_POWER: f32 = 10.0;

/// Toggle the player's shield with R. Dropping it saves power; raising it
/// again starts from whatever charge remains.
pub fn toggle_player_shield(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut shield_query: Query<&mut ShipShield, With<Ship>>,
    mut notifications: MessageWriter<crate::events::ShowNotification>,
) {
    if !keyboard.just_pressed(KeyCode::KeyR) { return; }
    let Ok(mut shield) = shield_query.single_mut() else { return };
    shield.enabled = !shield.enabled;
    notifications.write(crate::events::ShowNotification {
        message: if shield.enabled { "Shield raised".into() } else { "Shield lowered".into() },
        notification_type: crate::events::NotificationType::Info,
        duration: 1.5,
    });
}

/// Recharge shields after a quiet period; decay hit-flash; drive bubble alpha.
/// The shield is a plain health pool — its only tie to the power grid is the
/// flat upkeep drawn while raised (see update_power_system).
pub fn update_shields(
    time: Res<Time>,
    mut shield_query: Query<(&mut ShipShield, &Children)>,
    mut bubble_query: Query<&mut Sprite, With<ShieldBubble>>,
) {
    let dt = time.delta_secs();

    for (mut shield, children) in shield_query.iter_mut() {
        shield.since_hit += dt;
        shield.flash = (shield.flash - dt * 3.0).max(0.0);

        if shield.enabled
            && shield.since_hit > shield.recharge_delay
            && shield.current < shield.max
        {
            shield.current = (shield.current + shield.recharge_rate * dt).min(shield.max);
        }

        // Bubble opacity: proportional to charge, spikes on hit, gone when down
        for child in children.iter() {
            if let Ok(mut sprite) = bubble_query.get_mut(child) {
                let alpha = if shield.is_up() {
                    0.10 + 0.20 * (shield.current / shield.max) + shield.flash * 0.5
                } else {
                    0.0
                };
                sprite.color = Color::srgba(0.5, 0.8, 1.0, alpha.min(0.85));
            }
        }
    }
}
