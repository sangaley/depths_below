use bevy::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

// ============================================================================
// PROCEDURAL CIRCLE TEXTURES
// Celestial bodies (stars, planets, asteroids) and their glow/atmosphere
// layers were plain `Sprite { custom_size, .. }` with no image — Bevy draws
// an untextured Sprite as a flat rectangle, so every "circular" body was
// actually rendering as a solid-color square. Two small textures generated
// once at startup (a hard-edged disc and a soft radial falloff) fix that for
// every existing spawn site with just an `image: Some(handle)` swap — no new
// art assets, no shaders.
// ============================================================================

const TEX_SIZE: u32 = 128;

#[derive(Resource, Clone)]
pub struct CelestialTextures {
    /// Opaque disc, white RGB (tint via Sprite.color) — solid bodies: star
    /// core, planet, asteroid.
    pub solid: Handle<Image>,
    /// Soft radial gradient, white RGB, alpha 1.0 at center fading to 0 at
    /// the edge — glow/corona/atmosphere/shadow layers.
    pub glow: Handle<Image>,
}

fn circle_image(soft: bool) -> Image {
    let size = TEX_SIZE;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let center = size as f32 / 2.0;
    let radius = center;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            let t = (dx * dx + dy * dy).sqrt() / radius;

            let alpha = if soft {
                (1.0 - t.min(1.0)).powf(1.6)
            } else {
                // Opaque disc with a ~1.5px anti-aliased edge so it doesn't
                // look jagged at small sizes.
                let edge = (1.5 / radius).max(0.001);
                if t <= 1.0 - edge { 1.0 } else { ((1.0 - t) / edge).clamp(0.0, 1.0) }
            };

            let idx = ((y * size + x) * 4) as usize;
            data[idx] = 255;
            data[idx + 1] = 255;
            data[idx + 2] = 255;
            data[idx + 3] = (alpha * 255.0).round() as u8;
        }
    }

    Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

pub fn generate_celestial_textures(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
) {
    let solid = images.add(circle_image(false));
    let glow = images.add(circle_image(true));
    commands.insert_resource(CelestialTextures { solid, glow });
}
