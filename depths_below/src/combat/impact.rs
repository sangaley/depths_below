use crate::building::Block;

// ============================================================================
// IMPACT RESOLUTION — the one place a round meets a block.
// Both damage paths (player rounds into AI hulls, AI/creature rounds into the
// player) funnel their per-block step through here, so armour geometry has a
// single home. The angle terms are stubbed to 1.0 for now: today's balance is
// preserved exactly, and slope/obliquity land here when they land at all.
// ============================================================================

/// Obliquity term — how much a glancing hit inflates line-of-sight
/// thickness beyond what `span` already captures. Stubbed: no balance change.
const OBLIQUITY_TERM: f32 = 1.0;
/// Sloped-armour term (Block::slope). Stubbed: no balance change.
const SLOPE_TERM: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Impact {
    /// Damage the struck block takes.
    pub to_block: f32,
    /// Damage that carries on past it (into what it covers, or the next
    /// cell along the walk).
    pub through: f32,
    /// Armour actually in the round's way, in penetration units:
    /// thickness × line-of-sight span × angle terms.
    pub effective_thickness: f32,
}

/// Resolve one round against one block.
///
/// `span` is the walk's line-of-sight thickness for the cell (cell widths).
/// `pass_through` is the caller's rule for how much of the round carries
/// past the block: `Some(fraction)` for the ammo-table rule the AI-hull path
/// uses (armour-exposure split), `None` to absorb by effective thickness —
/// the player-hull rule, where a plate soaks up to its material rating and
/// the remainder continues inward.
pub fn resolve_impact(incoming: f32, block: &Block, span: f32, pass_through: Option<f32>) -> Impact {
    let effective_thickness = block.thickness * span.max(0.0) * OBLIQUITY_TERM * SLOPE_TERM;
    let through = match pass_through {
        Some(fraction) => incoming * fraction.clamp(0.0, 1.0),
        None => (incoming - effective_thickness).max(0.0),
    };
    Impact {
        to_block: (incoming - through).max(0.0),
        through,
        effective_thickness,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::HullMaterial;
    use bevy::math::IVec2;

    #[test]
    fn plate_absorbs_up_to_its_rating_and_passes_the_rest() {
        let steel = Block::hull(IVec2::ZERO, HullMaterial::Steel); // 15
        let hit = resolve_impact(40.0, &steel, 1.0, None);
        assert_eq!(hit.to_block, 15.0);
        assert_eq!(hit.through, 25.0);
        assert_eq!(hit.effective_thickness, 15.0);
    }

    #[test]
    fn weak_round_is_fully_absorbed() {
        let alloy = Block::hull(IVec2::ZERO, HullMaterial::AbyssalAlloy); // 80
        let hit = resolve_impact(30.0, &alloy, 1.0, None);
        assert_eq!(hit.to_block, 30.0);
        assert_eq!(hit.through, 0.0);
    }

    #[test]
    fn diagonal_span_thickens_the_plate() {
        let steel = Block::hull(IVec2::ZERO, HullMaterial::Steel);
        let hit = resolve_impact(40.0, &steel, 2f32.sqrt(), None);
        assert!((hit.effective_thickness - 21.213).abs() < 1e-2);
        assert!((hit.to_block - 21.213).abs() < 1e-2);
    }

    #[test]
    fn ammo_rule_splits_by_fraction_regardless_of_thickness() {
        let alloy = Block::hull(IVec2::ZERO, HullMaterial::AbyssalAlloy);
        let hit = resolve_impact(40.0, &alloy, 1.0, Some(0.3));
        assert!((hit.to_block - 28.0).abs() < 1e-4);
        assert!((hit.through - 12.0).abs() < 1e-4);
        let exposed = resolve_impact(40.0, &Block::module(IVec2::ZERO), 1.0, Some(0.0));
        assert_eq!(exposed.to_block, 40.0);
    }

    #[test]
    fn unarmoured_module_absorbs_nothing_by_thickness() {
        let hit = resolve_impact(40.0, &Block::module(IVec2::ZERO), 1.0, None);
        assert_eq!(hit.to_block, 0.0);
        assert_eq!(hit.through, 40.0);
    }
}
