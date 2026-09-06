use bevy::prelude::*;
use rand::Rng;

// ============================================================================
// EFFECT TEXTURES
//
// Handles for the soft effect art, loaded once at startup and passed to the
// spawn helpers. Modelled on `audio::GameAudio`, which does the same thing for
// the ~43 sound clips: one resource of handles, filled by a Startup system
// with literal paths. There is no asset manifest or folder scan in this
// project, so this is where "which PNGs are effects" is written down.
//
// The textures are pure white with the shape carried in alpha, because a Bevy
// `Sprite.color` MULTIPLIES its texture. Every spawn site already picks a
// meaningful colour — the missile trail rolls a grey per puff, `Blast` lerps
// hot to cool across an explosion's life, projectiles tint per ammo type — so
// baking colour into the art would apply it twice and quietly break the
// readability those colours exist to provide.
// ============================================================================

/// Soft smoke sprites for trails and explosions.
#[derive(Resource)]
pub struct EffectTextures {
    /// Three variants harvested from different frames of one Mantaflow bake.
    /// They are normalised to the same footprint, so they differ in structure
    /// and density but not in size and can be swapped freely.
    smoke: [Handle<Image>; 3],
}

impl EffectTextures {
    /// A random smoke puff.
    ///
    /// Randomising here rather than at the call site keeps every emitter
    /// varied without each one having to know how many variants exist. A
    /// trail that drew the same puff every time reads as a repeating stamp.
    pub fn puff(&self) -> Handle<Image> {
        let i = rand::thread_rng().gen_range(0..self.smoke.len());
        self.smoke[i].clone()
    }
}

pub fn load_effect_textures(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(EffectTextures {
        smoke: [
            assets.load("sprites/effects/smoke_puff_a.png"),
            assets.load("sprites/effects/smoke_puff_b.png"),
            assets.load("sprites/effects/smoke_puff_c.png"),
        ],
    });
}
