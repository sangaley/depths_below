use bevy::prelude::*;
use crate::components::{Ship, Module, ModuleType, HullSegment, Projectile};
use crate::ai_ship::components::{AiShip, AiShipType};
use std::collections::HashSet;
use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

// ============================================================================
// SHIP SHIELDS — Starsector-style visible shield bubble.
// Every ship carries a shield that absorbs incoming fire until depleted;
// it recharges after a few seconds without hits. Only once the shield is
// down do shots reach the hull and damage individual blocks.
// ============================================================================

#[derive(Component)]
pub struct ShipShield {
    pub current: f32,
    /// Effective capacity. For the player this is `base_max` scaled by routed
    /// shield power each frame (see update_shields); for AI ships it stays at
    /// base_max.
    pub max: f32,
    /// Capacity at nominal power — the emitter-derived value set at spawn.
    /// Power routing scales `max` around this; it never changes after spawn.
    pub base_max: f32,
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
    /// SAMPLE omni arc: world-angle the low-coverage arc currently points at.
    /// Auto-swings toward the biggest incoming threat (update_player_shield_arc).
    pub arc_facing: f32,
    /// Half-width of the arc, radians. Player: low coverage. AI: PI (unused).
    pub arc_half: f32,
    /// Max swing speed of the arc, rad/s — fast but not instant.
    pub arc_traverse: f32,
}

impl ShipShield {
    pub fn absorb(&mut self, damage: f32) {
        self.current = (self.current - damage).max(0.0);
        self.since_hit = 0.0;
        self.flash = (self.flash + damage * SHIELD_FLASH_PER_DAMAGE).min(SHIELD_FLASH_MAX);
    }
    pub fn is_up(&self) -> bool {
        self.enabled && self.current > 0.0
    }
    /// Whether the omni arc currently covers a hit arriving from `hit_dir`
    /// (world-space direction from the bubble centre to the impact). Outside
    /// the arc the shot slips past to the hull — coverage is low, so the arc
    /// has to swing to face threats (see update_player_shield_arc).
    pub fn covers_arc(&self, hit_dir: Vec2) -> bool {
        let h = hit_dir.normalize_or_zero();
        if h == Vec2::ZERO {
            return true;
        }
        Vec2::from_angle(self.arc_facing).dot(h) >= self.arc_half.cos()
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

/// The grid cells the ship occupies (module footprints + hull segments). Cell
/// (gx,gy) maps to ship-local (gx*66, gy*66-33), the same layout spawner.rs and
/// decompression.rs use. Drives the shield-skin perimeter trace.
fn ship_cells(
    ship: Entity,
    modules: &Query<(&Module, &ChildOf)>,
    hulls: &Query<(&HullSegment, &ChildOf)>,
) -> HashSet<IVec2> {
    let mut cells = HashSet::new();
    for (m, p) in modules.iter() {
        if p.parent() != ship { continue; }
        for dx in 0..m.size.x.max(1) {
            for dy in 0..m.size.y.max(1) {
                cells.insert(m.grid_position + IVec2::new(dx, dy));
            }
        }
    }
    for (h, p) in hulls.iter() {
        if p.parent() == ship { cells.insert(h.grid_position); }
    }
    cells
}

/// Separable box blur (clamped edges). Three passes approximate a Gaussian —
/// enough to erase the block-grid bumps so the shield contour reads as a single
/// smooth curve rather than a stepped outline.
fn box_blur(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let ri = r as i32;
    let norm = 1.0 / (2 * r + 1) as f32;
    let mut tmp = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for k in -ri..=ri {
                let xx = (x as i32 + k).clamp(0, w as i32 - 1) as usize;
                s += src[y * w + xx];
            }
            tmp[y * w + x] = s * norm;
        }
    }
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for k in -ri..=ri {
                let yy = (y as i32 + k).clamp(0, h as i32 - 1) as usize;
                s += tmp[yy * w + x];
            }
            out[y * w + x] = s * norm;
        }
    }
    out
}

/// Build the shield skin as a SMOOTH glowing outline around the ship: rasterise
/// the filled cell silhouette, blur it heavily (killing the blocky bumps), then
/// draw a soft glow band at a coverage threshold. The blur is what makes it a
/// clean smooth shape hugging the hull — no grid steps, no 90° angles. The gap
/// off the hull just falls out of the blur amount. Returns (texture, local
/// centre, local size).
fn build_shield_skin(cells: &HashSet<IVec2>) -> Option<(Image, Vec2, Vec2)> {
    const CELL: f32 = 66.0;
    const HALF: f32 = 33.0;
    const TPU: f32 = 0.25;          // texels per world unit (1 texel = 4 units)
    const SMOOTH: f32 = 76.0;       // blur radius in world units → smoothness + gap
    const THRESH: f32 = 0.22;       // coverage iso-value the glow sits on (lower = further out)
    const BANDW: f32 = 0.085;       // glow width in coverage space (smaller = thinner)
    if cells.is_empty() { return None; }
    let centers: Vec<Vec2> = cells.iter()
        .map(|c| Vec2::new(c.x as f32 * CELL, c.y as f32 * CELL - HALF))
        .collect();
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for c in &centers { lo = lo.min(*c); hi = hi.max(*c); }
    let r_tex = ((SMOOTH * TPU).round() as usize).max(1);
    let pad = 3.0 * SMOOTH + 8.0;   // room for the blurred halo
    let tmin = Vec2::new(lo.x - HALF - pad, lo.y - HALF - pad);
    let tmax = Vec2::new(hi.x + HALF + pad, hi.y + HALF + pad);
    let world = tmax - tmin;
    let w = ((world.x * TPU).ceil() as usize).max(1);
    let h = ((world.y * TPU).ceil() as usize).max(1);

    // Rasterise filled silhouette (1 inside a cell square, 0 outside).
    let mut cov = vec![0.0f32; w * h];
    for c in &centers {
        // world square -> texel rect (row 0 = top = +Y)
        let x0 = (((c.x - HALF) - tmin.x) * TPU).floor().max(0.0) as usize;
        let x1 = ((((c.x + HALF) - tmin.x) * TPU).ceil() as usize).min(w);
        let ry0 = (((tmax.y - (c.y + HALF)) * TPU).floor().max(0.0)) as usize;
        let ry1 = (((tmax.y - (c.y - HALF)) * TPU).ceil() as usize).min(h);
        for ty in ry0..ry1 {
            for tx in x0..x1 { cov[ty * w + tx] = 1.0; }
        }
    }
    cov = box_blur(&cov, w, h, r_tex);
    cov = box_blur(&cov, w, h, r_tex);
    cov = box_blur(&cov, w, h, r_tex);

    // Glow band at the coverage iso-contour (a smooth ring outside the hull).
    let mut data = vec![0u8; w * h * 4];
    for i in 0..w * h {
        let d = (cov[i] - THRESH) / BANDW;
        let k = (-d * d).exp();     // gaussian band centred on THRESH
        if k > 0.02 {
            data[i * 4] = 255; data[i * 4 + 1] = 255; data[i * 4 + 2] = 255;
            data[i * 4 + 3] = (k * 255.0) as u8;   // white; sprite.color tints it
        }
    }
    let img = Image::new(
        Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default(),
    );
    Some((img, (tmin + tmax) * 0.5, world))
}

/// Spawn the shield skin as a single child sprite carrying the generated
/// distance-field texture. Marked ShieldBubble so `update_shields` drives its
/// alpha/flash exactly like the old bubble.
fn spawn_shield_skin(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    owner: Entity,
    cells: &HashSet<IVec2>,
) {
    let Some((img, center, size)) = build_shield_skin(cells) else { return };
    let handle = images.add(img);
    let skin = commands.spawn((
        Sprite {
            image: handle,
            color: Color::srgba(0.5, 0.8, 1.0, 0.0), // alpha driven by update_shields
            custom_size: Some(size),
            ..default()
        },
        Transform::from_xyz(center.x, center.y, 0.9),
        ShieldBubble,
    )).id();
    commands.entity(owner).add_child(skin);
}

/// Attach a shield to the player ship (scaled by Shield Emitter modules).
pub fn attach_player_shield(
    mut commands: Commands,
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

    let max = SHIELD_BASE_HP + emitters * SHIELD_HP_PER_EMITTER;
    let (center, radius) = ship_extent(ship, &transform_query);

    commands.entity(ship).insert(ShipShield {
        current: max,
        max,
        base_max: max,
        recharge_rate: 12.0,
        recharge_delay: 4.0,
        since_hit: 999.0,
        radius,
        center_offset: center,
        flash: 0.0,
        enabled: true,
        surge: 0.0,
        arc_facing: 0.0,
        arc_half: SHIELD_ARC_HALF,
        arc_traverse: SHIELD_ARC_TRAVERSE,
    });
    // The silhouette-hugging skin itself is built (and kept up to date as blocks
    // change) by refresh_player_shield_skin. Collision stays radius-based via
    // `radius`/`center_offset` above — the skin is purely a visual.
}

/// Rebuild the player's shield skin whenever the ship's occupied-cell set
/// changes — a block placed (on the next launch), or blocks destroyed in a
/// fight. Cheap: it hashes the cell set every frame and only regenerates the
/// distance-field texture when that hash actually changes, so it costs nothing
/// while the ship is unchanged.
pub fn refresh_player_shield_skin(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut last_hash: Local<u64>,
    ship_query: Query<Entity, (With<Ship>, With<ShipShield>)>,
    module_query: Query<(&Module, &ChildOf)>,
    hull_query: Query<(&HullSegment, &ChildOf)>,
    skin_query: Query<(Entity, &ChildOf), With<ShieldBubble>>,
) {
    let Ok(ship) = ship_query.single() else { return };
    let cells = ship_cells(ship, &module_query, &hull_query);
    if cells.is_empty() { return; }
    // order-independent hash of the cell set
    let hash = cells.iter().fold(0u64, |acc, c| {
        acc.wrapping_add(
            (c.x as i64 as u64).wrapping_mul(73856093)
                ^ (c.y as i64 as u64).wrapping_mul(19349663),
        )
    });
    if hash == *last_hash { return; }
    *last_hash = hash;
    // drop the previous skin sprite(s) for the player ship, then rebuild
    for (e, p) in skin_query.iter() {
        if p.parent() == ship { commands.entity(e).despawn(); }
    }
    spawn_shield_skin(&mut commands, &mut images, ship, &cells);
}

/// Attach shields to AI ships as they spawn, sized by faction.
pub fn attach_ai_shields(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    ai_query: Query<(Entity, &AiShipType, &Children), (With<AiShip>, Without<ShipShield>)>,
    transform_query: Query<(&Transform, &ChildOf), With<Module>>,
    module_query: Query<(&Module, &ChildOf)>,
    hull_query: Query<(&HullSegment, &ChildOf)>,
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
                AiShipType::VoidTitan => 140.0,    // the hardest kill in the game
                AiShipType::Dreadnought => 100.0,  // mega-battleship
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
            base_max: max,
            recharge_rate: 8.0,
            recharge_delay: 5.0,
            since_hit: 999.0,
            radius,
            center_offset: center,
            flash: 0.0,
            enabled: true,
            surge: 0.0,
            arc_facing: 0.0,
            arc_half: std::f32::consts::PI, // AI: full coverage, arc unused
            arc_traverse: 0.0,
        });
        // Same silhouette skin as the player (built once — AI ships don't get
        // rebuilt in a hangar; a destroyed one just leaves its skin slightly wide).
        let cells = ship_cells(entity, &module_query, &hull_query);
        spawn_shield_skin(&mut commands, &mut images, entity, &cells);
    }
}

/// Passive power the player's shield draws while enabled. Hits add a surge
/// on top (see ShipShield::absorb) — sustained fire can push the ship's
/// power balance negative, which collapses the shield AND silences weapons.
pub const SHIELD_UPKEEP_POWER: f32 = 10.0;

/// Below this routed-power multiplier the player's shield is considered
/// browned out: it stops recharging and slowly bleeds, so cutting shield
/// power visibly drops the bubble (see update_shields / PowerChannels).
const SHIELD_BROWNOUT_MULT: f32 = 0.35;
/// How fast a browned-out shield bleeds charge (per second).
const SHIELD_BROWNOUT_BLEED: f32 = 6.0;

/// How much routed shield power flexes the bubble's max capacity, relative to
/// the channel's ×0..×2 power range. At full shield power (×2) capacity rises
/// by this fraction; at zero power it drops by it. Deliberately gentler than
/// the raw power multiplier — routing power mostly buys faster recharge, with
/// a modest capacity bump on top. 0.35 → up to +35% / −35% HP.
const SHIELD_HP_POWER_BONUS: f32 = 0.35;

/// SAMPLE omni arc: a low-coverage but tanky shield patch that auto-swings to
/// block the most damaging incoming shot. Half-width → ~110° of coverage; it
/// can rotate fast (but not instantly), so simultaneous fire from other angles
/// slips past to the hull until the arc catches up.
const SHIELD_ARC_HALF: f32 = 0.96; // ~55° half → ~110° total coverage
const SHIELD_ARC_TRAVERSE: f32 = 8.0; // rad/s — fast, not instant
const SHIELD_ARC_THREAT_RANGE: f32 = 700.0; // hostile fire within this is tracked

/// Bubble look: next-to-invisible at rest, glowing on impact and brighter the
/// more damage the hit dealt (flash accumulates in absorb, decays each frame).
const SHIELD_BASE_ALPHA: f32 = 0.04;
const SHIELD_FLASH_ALPHA: f32 = 0.55;
const SHIELD_FLASH_PER_DAMAGE: f32 = 0.03; // ~30 dmg → a strong flash
const SHIELD_FLASH_MAX: f32 = 1.6;

/// Player bubble capacity with zero emitters, and the HP each live Shield
/// Emitter module adds. Used both to size the shield at attach and to keep it
/// tracking the emitter count as blocks are built or destroyed. Nudged up ~25%
/// from the original 40/40 for a bit more overall bubble, on top of the routed
/// power flex.
const SHIELD_BASE_HP: f32 = 50.0;
const SHIELD_HP_PER_EMITTER: f32 = 50.0;

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

/// Keeps the player's shield capacity in step with its live Shield Emitter
/// count, so building an emitter at the dock grows the bubble and losing one in
/// a fight shrinks it — instead of the capacity being frozen at first-launch.
/// Only `base_max` is touched here; update_shields then applies the routed
/// power HP flex on top and re-clamps `current`. AI shields keep their fixed
/// faction capacity (this only runs for the Ship-marked player shield).
pub fn refresh_player_shield_capacity(
    module_query: Query<(&Module, &ChildOf)>,
    mut shield_query: Query<(Entity, &mut ShipShield), With<Ship>>,
) {
    let Ok((ship, mut shield)) = shield_query.single_mut() else { return };
    let emitters = module_query
        .iter()
        .filter(|(m, p)| {
            p.parent() == ship && m.module_type == ModuleType::ShieldEmitter && m.health > 0.0
        })
        .count() as f32;
    let new_base = SHIELD_BASE_HP + emitters * SHIELD_HP_PER_EMITTER;
    if (shield.base_max - new_base).abs() > f32::EPSILON {
        shield.base_max = new_base;
        // Capacity may have dropped (emitter destroyed) — don't leave current
        // above it. The routed HP flex re-clamps against the scaled max too.
        shield.current = shield.current.min(shield.base_max);
    }
}

/// SAMPLE: swing the player's low-coverage omni arc toward the most dangerous
/// incoming shot (capped by a traverse rate) and draw it. Scans hostile
/// projectiles heading at the ship within range, points at the highest-damage
/// one (nearest on a tie), and rotates there at `arc_traverse` rad/s — fast but
/// not instant, so fire from other angles gets through until the arc catches
/// up. Rendered as a faint blue arc that flares brighter on impact.
pub fn update_player_shield_arc(
    time: Res<Time>,
    mut gizmos: Gizmos,
    mut ship_query: Query<(&Transform, &mut ShipShield), With<Ship>>,
    projectiles: Query<(&Transform, &Projectile)>,
) {
    let dt = time.delta_secs();
    let Ok((ship_transform, mut shield)) = ship_query.single_mut() else { return };
    let center = shield.world_center(ship_transform);

    // Most dangerous hostile shot that's actually incoming and in range.
    let mut best: Option<(f32, f32, Vec2)> = None; // (damage, dist, pos)
    for (t, proj) in projectiles.iter() {
        if proj.owner.is_player() {
            continue;
        }
        let p = t.translation.truncate();
        let to_ship = center - p;
        let dist = to_ship.length();
        if dist > SHIELD_ARC_THREAT_RANGE {
            continue;
        }
        // Only fire heading toward us counts (skip shots already past).
        if proj.direction.dot(to_ship.normalize_or_zero()) <= 0.0 {
            continue;
        }
        let better = match best {
            None => true,
            Some((bd, bdist, _)) => proj.damage > bd || (proj.damage == bd && dist < bdist),
        };
        if better {
            best = Some((proj.damage, dist, p));
        }
    }

    // Swing toward the threat, shortest path, rate-limited.
    if let Some((_, _, pos)) = best {
        let dir = pos - center;
        if dir.length_squared() > 1.0 {
            let target = dir.y.atan2(dir.x);
            let mut diff = target - shield.arc_facing;
            while diff > std::f32::consts::PI {
                diff -= std::f32::consts::TAU;
            }
            while diff < -std::f32::consts::PI {
                diff += std::f32::consts::TAU;
            }
            let max_step = shield.arc_traverse * dt;
            shield.arc_facing += diff.clamp(-max_step, max_step);
        }
    }

    // Draw the arc: faint at rest, brighter on hit.
    if shield.is_up() {
        let color = Color::srgba(0.5, 0.8, 1.0, (0.14 + shield.flash * 0.5).min(0.85));
        let r = shield.radius;
        let segs = 24;
        let start = shield.arc_facing - shield.arc_half;
        let sweep = shield.arc_half * 2.0;
        let mut prev = center + Vec2::from_angle(start) * r;
        for i in 1..=segs {
            let a = start + sweep * (i as f32 / segs as f32);
            let p = center + Vec2::from_angle(a) * r;
            gizmos.line_2d(prev, p, color);
            prev = p;
        }
    }
}

/// Recharge shields after a quiet period; decay hit-flash; drive bubble alpha.
/// The shield is a plain health pool — its only tie to the power grid is the
/// flat upkeep drawn while raised (see update_power_system).
pub fn update_shields(
    time: Res<Time>,
    channels: Res<crate::resources::PowerChannels>,
    player_query: Query<Entity, With<Ship>>,
    mut shield_query: Query<(Entity, &mut ShipShield, &Children)>,
    mut bubble_query: Query<&mut Sprite, With<ShieldBubble>>,
) {
    let dt = time.delta_secs();
    let player = player_query.single().ok();

    for (entity, mut shield, children) in shield_query.iter_mut() {
        shield.since_hit += dt;
        shield.flash = (shield.flash - dt * 3.0).max(0.0);

        // Only the player's shield answers to power routing; AI shields run at
        // their fixed rate.
        let power_mult = if Some(entity) == player { channels.shields_mult } else { 1.0 };

        // Routed power gently flexes the player's max capacity around base_max
        // (a ×2 power channel → only ~+35% HP), then clamp current to it.
        if Some(entity) == player {
            let hp_factor = 1.0 + (power_mult - 1.0) * SHIELD_HP_POWER_BONUS;
            shield.max = shield.base_max * hp_factor;
            shield.current = shield.current.min(shield.max);
        }

        if power_mult < SHIELD_BROWNOUT_MULT {
            // Starved: the bubble can't hold — no recharge, and it bleeds down.
            shield.current = (shield.current - SHIELD_BROWNOUT_BLEED * dt).max(0.0);
        } else if shield.enabled
            && shield.since_hit > shield.recharge_delay
            && shield.current < shield.max
        {
            shield.current =
                (shield.current + shield.recharge_rate * power_mult * dt).min(shield.max);
        }

        // Bubble is next-to-invisible at rest and glows on impact, brighter the
        // more damage the hit dealt (flash accumulates in absorb, decays above).
        for child in children.iter() {
            if let Ok(mut sprite) = bubble_query.get_mut(child) {
                let base = if shield.is_up() { SHIELD_BASE_ALPHA } else { 0.0 };
                let alpha = (base + shield.flash * SHIELD_FLASH_ALPHA).min(0.9);
                sprite.color = Color::srgba(0.5, 0.8, 1.0, alpha);
            }
        }
    }
}
