//! Distant previews: a far-off ship drawn as one picture of its design.
//!
//! A ship becomes real blocks only inside RENDER_DISTANCE (10 km), and the
//! biggest hulls are kilometres long, so a capital used to pop into being
//! half on screen. Now every ship of the system you're in, out to
//! PREVIEW_RANGE, is drawn as a flat silhouette of its own design -- one
//! sprite, one texture per design -- riding its simulated position and
//! heading. When the ship materialises the picture goes and the blocks
//! appear where it was. A big fleet can be seen coming from 35 km for the
//! price of a sprite a ship.
//!
//! The pictures are dim: the dark is the game's (camera::light_cone keeps
//! everything you haven't lit near-black), and a far fleet should read as
//! shapes against the stars, not as lit hulls.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::collections::HashMap;

use super::components::*;
use crate::components::Ship;

/// How far out ships are drawn.
pub const PREVIEW_RANGE: f32 = 35_000.0;
/// How bright a preview is right at the edge of materialising, and at the
/// edge of the preview range.
const NEAR_ALPHA: f32 = 0.55;
const FAR_ALPHA: f32 = 0.18;
/// World units per grid cell (the spawner's).
const CELL: f32 = 66.0;

/// A ship's picture, standing in for it until it materialises.
#[derive(Component, Clone, Copy, Debug)]
pub struct Preview {
    pub ship_id: u32,
}

/// One texture per design, made once.
#[derive(Resource, Default)]
pub struct PreviewImages {
    images: HashMap<(AiShipType, Option<&'static str>), (Handle<Image>, Vec2, Vec2)>,
}

/// The silhouette of a design: one pixel per cell, outer hull bright, inner
/// decks darker, modules lighter still. Returns the image, its size in world
/// units, and the offset from the ship's root to the picture's centre.
pub fn silhouette(design: &crate::building::blueprint::Blueprint) -> Option<(Image, Vec2, Vec2)> {
    use crate::components::HullLayer;
    let cells = &design.hull_cells;
    let (lo, hi) = cells.iter().fold((IVec2::MAX, IVec2::MIN), |(lo, hi), c| (lo.min(c.grid_pos), hi.max(c.grid_pos)));
    if cells.is_empty() {
        return None;
    }
    let size = hi - lo + IVec2::ONE;
    let (w, h) = (size.x as usize, size.y as usize);
    let mut data = vec![0u8; w * h * 4];
    let mut put = |p: IVec2, shade: u8| {
        let x = (p.x - lo.x) as usize;
        // Image rows run top-down; grid y runs up.
        let y = (hi.y - p.y) as usize;
        let i = (y * w + x) * 4;
        data[i] = shade;
        data[i + 1] = shade;
        data[i + 2] = shade;
        data[i + 3] = 255;
    };
    for c in cells {
        let shade = match c.layer {
            HullLayer::Outer => 210,
            _ => 120,
        };
        put(c.grid_pos, shade);
    }
    for m in &design.modules {
        if m.grid_pos.cmpge(lo).all() && m.grid_pos.cmple(hi).all() {
            put(m.grid_pos, 255);
        }
    }
    let mut image = Image::new(
        Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = bevy::image::ImageSampler::nearest();
    let world_size = size.as_vec2() * CELL;
    // Cells sit at grid_pos * 66 (y less half a cell) from the root.
    let centre = (lo + hi).as_vec2() * 0.5 * CELL - Vec2::new(0.0, CELL * 0.5);
    Some((image, world_size, centre))
}

fn picture(
    cache: &mut PreviewImages,
    images: &mut Assets<Image>,
    faction: AiShipType,
    class: Option<&'static str>,
) -> Option<(Handle<Image>, Vec2, Vec2)> {
    if let Some(p) = cache.images.get(&(faction, class)) {
        return Some(p.clone());
    }
    let design = super::spawner::built_design(faction, class);
    let (image, size, centre) = silhouette(&design)?;
    let entry = (images.add(image), size, centre);
    cache.images.insert((faction, class), entry.clone());
    Some(entry)
}

/// How a preview fades with distance: brighter as it nears materialising.
pub fn preview_alpha(distance: f32) -> f32 {
    let t = ((distance - super::simulation::RENDER_DISTANCE) / (PREVIEW_RANGE - super::simulation::RENDER_DISTANCE)).clamp(0.0, 1.0);
    NEAR_ALPHA + (FAR_ALPHA - NEAR_ALPHA) * t
}

/// Keep a picture on every off-screen ship of the loaded system within
/// range, and none on anything else.
#[allow(clippy::too_many_arguments)]
pub fn sync_previews(
    mut commands: Commands,
    time: Res<Time>,
    sim: Res<WorldSimulation>,
    streaming: Res<crate::celestial::resources::SystemStreamingManager>,
    player: Query<&Transform, (With<Ship>, Without<Preview>)>,
    mut previews: Query<(Entity, &Preview, &mut Transform, &mut Sprite)>,
    mut cache: ResMut<PreviewImages>,
    mut images: ResMut<Assets<Image>>,
) {
    let Ok(player) = player.single() else {
        for (e, ..) in previews.iter() {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let player_pos = player.translation.truncate();
    let dt = time.delta_secs();
    let wanted: HashMap<u32, &SimulatedShip> = sim
        .ships
        .iter()
        .filter(|s| {
            s.id != 0
                && !s.spawned
                && s.behavior != SimBehavior::Dead
                && streaming.loaded_system == Some(s.system_id)
                && s.position.distance(player_pos) < PREVIEW_RANGE
        })
        .map(|s| (s.id, s))
        .collect();
    let heading = |s: &SimulatedShip| -> f32 {
        sim.fleet(s.fleet).map(|f| f.heading).unwrap_or_else(|| {
            if s.velocity.length() > 1.0 { s.velocity.to_angle() } else { 0.0 }
        })
    };

    let mut have: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for (e, preview, mut tf, mut sprite) in previews.iter_mut() {
        let Some(ship) = wanted.get(&preview.ship_id) else {
            commands.entity(e).try_despawn();
            continue;
        };
        have.insert(preview.ship_id);
        // The simulation moves in two-second steps: drift with the ship's
        // velocity between them, and ease onto its record when it lands.
        let angle = heading(ship);
        let Some((_, _, centre)) = picture(&mut cache, &mut images, ship.faction, ship.class) else { continue };
        let target = ship.position + Vec2::from_angle(angle).rotate(centre);
        let mut at = tf.translation.truncate() + ship.velocity * dt;
        at = at.lerp(target, (dt * 1.5).min(1.0));
        tf.translation.x = at.x;
        tf.translation.y = at.y;
        tf.rotation = tf.rotation.slerp(Quat::from_rotation_z(angle), (dt * 2.0).min(1.0));
        let alpha = preview_alpha(ship.position.distance(player_pos));
        sprite.color = sprite.color.with_alpha(alpha);
    }
    for (id, ship) in wanted {
        if have.contains(&id) {
            continue;
        }
        let Some((image, size, centre)) = picture(&mut cache, &mut images, ship.faction, ship.class) else { continue };
        let angle = heading(ship);
        let at = ship.position + Vec2::from_angle(angle).rotate(centre);
        // The map palette, not the hull tint: several hull tints are near
        // black (tuned for lit ships close up) and vanish against space.
        let tint = faction_map_color(ship.faction);
        commands.spawn((
            Sprite {
                image,
                custom_size: Some(size),
                color: tint.with_alpha(preview_alpha(ship.position.distance(player_pos))),
                ..default()
            },
            Transform {
                translation: at.extend(0.04),
                rotation: Quat::from_rotation_z(angle),
                ..default()
            },
            Preview { ship_id: id },
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_silhouette_covers_its_design() {
        let design = super::super::spawner::built_design(AiShipType::TerranHegemony, crate::ai_ship::roster::intern("sovereign"));
        let (image, size, _) = silhouette(&design).unwrap();
        let w = image.width() as usize;
        let h = image.height() as usize;
        assert_eq!(size, Vec2::new(w as f32, h as f32) * CELL);
        let data = image.data.as_ref().unwrap();
        let opaque = (0..w * h).filter(|i| data[i * 4 + 3] == 255).count();
        // One opaque pixel per cell the ship covers: its hull, and the
        // modules that sit outboard of it (plates, external fittings).
        let mut covered: std::collections::HashSet<IVec2> = design.hull_cells.iter().map(|c| c.grid_pos).collect();
        covered.extend(design.modules.iter().map(|m| m.grid_pos));
        assert_eq!(opaque, covered.len(), "one opaque pixel per covered cell");
    }

    #[test]
    fn previews_brighten_as_they_near() {
        assert!(preview_alpha(super::super::simulation::RENDER_DISTANCE) > preview_alpha(PREVIEW_RANGE - 1.0));
        assert!((preview_alpha(PREVIEW_RANGE * 2.0) - FAR_ALPHA).abs() < 1e-6);
    }
}
