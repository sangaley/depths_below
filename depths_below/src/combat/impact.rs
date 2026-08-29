use bevy::math::{IVec2, Vec2};

use crate::building::Block;
use crate::combat::ammo_types::KineticAmmoType;

// ============================================================================
// IMPACT RESOLUTION — the one place a round meets a block.
// Both damage paths (player rounds into AI hulls, AI/creature rounds into the
// player) funnel their per-block step through here, so armour geometry has a
// single home.
// ============================================================================

/// Floor on cos(impact) when inflating thickness, so a near-grazing hit that
/// somehow escapes the ricochet test can't divide its way to infinite armour.
const MIN_COS: f32 = 0.15;

/// Converts a weapon's caliber scale (`caliber_scale`: gatling 0.45 …
/// railgun 1.25) into the same units as `Block::thickness` (15/30/50/80), for
/// the overmatch rule. Tuned so a railgun overmatches bare Steel and nothing
/// overmatches Titanium or better — big guns beat thin plate regardless of
/// how it's angled, which is what stops sloping being a universal answer.
const CALIBER_TO_THICKNESS: f32 = 30.0;

/// What became of the round at this block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImpactOutcome {
    /// Stopped in the plate.
    Absorbed,
    /// Carried on — into whatever the plate covers, or the next cell.
    Penetrated,
    /// Skipped off the surface. The plate is barely scratched and the round
    /// is still flying; the caller decides where it goes.
    Ricochet,
}

/// Impact geometry for one step of the walk: how square-on the round struck,
/// and whether it bit or skipped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obliquity {
    /// cos of the angle between the round and the plate's outward normal,
    /// after normalisation. 1.0 = dead-on, 0.0 = grazing.
    pub cos_impact: f32,
    /// Extra line-of-sight thickness from the plate being TILTED INSIDE its
    /// cell. The walk's `span` already covers the angle through the cell
    /// itself, so this is 1.0 for unsloped armour and only departs from it
    /// when `Block::slope` is non-zero. Double-counting the two is the easy
    /// mistake here.
    pub slope_mult: f32,
    pub ricochet: bool,
}

impl Obliquity {
    /// Square-on, no deflection. For damage with no geometry behind it —
    /// splash, burn ticks, radiation, a round already inside the block.
    pub const HEAD_ON: Self = Self { cos_impact: 1.0, slope_mult: 1.0, ricochet: false };
}

/// cos(θ_ric) per round — the round bounces when cos(impact) falls BELOW this,
/// so a smaller number means a steeper angle is needed and the round is harder
/// to deflect. A long dart barely bounces at all; a squash head never does,
/// because it isn't trying to get through the plate in the first place.
fn ricochet_cos(ammo: Option<KineticAmmoType>) -> f32 {
    use KineticAmmoType::*;
    match ammo {
        Some(APFSDS) => 0.208,                       // 78° — long rod
        Some(AP) | None => 0.342,                    // 70° — also beams/rams
        Some(APHE) | Some(HEAT) => 0.423,            // 65°
        Some(HESH) => -1.0,                          // never: it squashes
        Some(HEFrag) | Some(Flak) | Some(Incendiary) | Some(EMPShell) => 0.574, // 55°
    }
}

/// (cos Δ, sin Δ) for the round's normalisation — how far it tips TOWARD the
/// normal as it bites. Applied through the angle-subtraction identity so the
/// hot path never calls acos.
fn normalisation(ammo: Option<KineticAmmoType>) -> (f32, f32) {
    let degrees: f32 = match ammo {
        Some(KineticAmmoType::APFSDS) => 2.0,   // rigid, barely tips
        Some(KineticAmmoType::APHE) => 6.0,     // heavy, blunt
        Some(KineticAmmoType::HEAT) => 0.0,     // jet doesn't care
        _ => 4.0,
    };
    let r = degrees.to_radians();
    (r.cos(), r.sin())
}

/// Work out how obliquely a round met a block.
///
/// `entry_face` is the walk's outward normal for the face the round came in
/// through, in ship-local cell space; `dir_local` is the round's direction in
/// that same space. Both are already ship-local, so the ship's own heading is
/// baked in — which is the whole Layer-0 payoff: turning the ship off the
/// threat axis angles every flat plate on that side, with no new data.
pub fn obliquity(
    entry_face: IVec2,
    dir_local: Vec2,
    block: &Block,
    ammo: Option<KineticAmmoType>,
    caliber: f32,
) -> Obliquity {
    // No face means the round began this step already inside the cell (it
    // penetrated last frame). There's no surface left to skip off.
    if entry_face == IVec2::ZERO || dir_local.length_squared() < 1e-12 {
        return Obliquity::HEAD_ON;
    }
    let face = Vec2::new(entry_face.x as f32, entry_face.y as f32).normalize();
    let v = dir_local.normalize();

    // The plate's own normal: the cell face, rotated by whatever slope this
    // block declares. slope == 0 leaves it exactly on the face.
    let n = Vec2::from_angle(block.slope).rotate(face);

    let facing = -v.dot(n);
    let cos_face = (-v.dot(face)).abs().max(MIN_COS);

    // facing <= 0 means the round reached the BACK of this plate. A slope
    // protects the side it faces and nothing else — flanking defeats it, and
    // from behind a sloped plate is just a plate.
    //
    // Otherwise: `span` already carries the angle through the CELL (measured
    // against the face), so only the plate's extra tilt WITHIN the cell
    // belongs in slope_mult. The face term divides back out, which is what
    // makes slope == 0 come to exactly 1.0 and leave today's balance alone.
    // Note this uses the RAW angle, not the normalised one — normalisation is
    // about whether the round bites, not about how much steel is in its way.
    let (cos_t, slope_mult) = if facing <= 0.0 {
        (1.0, 1.0)
    } else {
        (facing, cos_face / facing.max(MIN_COS))
    };

    // Normalisation straightens the round TOWARD the normal, but never past
    // it: theta <= delta (i.e. cos_t >= cos_norm) means it's already square
    // enough, and cos(theta - delta) would overshoot to more-than-dead-on.
    let (cos_norm, sin_norm) = normalisation(ammo);
    let cos_eff = if cos_t >= cos_norm {
        1.0
    } else {
        let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
        (cos_t * cos_norm + sin_t * sin_norm).clamp(0.0, 1.0)
    };

    // Overmatch: a round far fatter than the plate punches through instead of
    // skipping, however the plate is angled.
    let overmatch = caliber * CALIBER_TO_THICKNESS >= 2.0 * block.thickness;
    let ricochet = block.thickness > 0.0 && !overmatch && cos_eff < ricochet_cos(ammo);

    Obliquity { cos_impact: cos_eff, slope_mult, ricochet }
}

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
    /// What became of the round here.
    pub outcome: ImpactOutcome,
    /// How square-on it struck. 1.0 = dead-on. Drives the hit readout.
    pub cos_impact: f32,
}

/// Fraction of a bounced round's energy the plate still eats. A skipping hit
/// scrapes rather than bites, and the more square-on it was the more it leaves
/// behind — so this scales with cos, not against it.
const RICOCHET_SCRAPE: f32 = 0.15;

/// Resolve one round against one block.
///
/// `span` is the walk's line-of-sight thickness for the cell (cell widths).
/// `pass_through` is the caller's rule for how much of the round carries
/// past the block: `Some(fraction)` for the ammo-table rule the AI-hull path
/// uses (armour-exposure split), `None` to absorb by effective thickness —
/// the player-hull rule, where a plate soaks up to its material rating and
/// the remainder continues inward.
pub fn resolve_impact(
    incoming: f32,
    block: &Block,
    span: f32,
    obl: Obliquity,
    pass_through: Option<f32>,
) -> Impact {
    let effective_thickness = block.thickness * span.max(0.0) * obl.slope_mult;

    // A skipping round never gets to spend its energy on the plate at all.
    if obl.ricochet {
        return Impact {
            to_block: incoming * RICOCHET_SCRAPE * obl.cos_impact,
            through: 0.0,
            effective_thickness,
            outcome: ImpactOutcome::Ricochet,
            cos_impact: obl.cos_impact,
        };
    }

    let through = match pass_through {
        Some(fraction) => incoming * fraction.clamp(0.0, 1.0),
        None => (incoming - effective_thickness).max(0.0),
    };
    Impact {
        to_block: (incoming - through).max(0.0),
        through,
        effective_thickness,
        outcome: if through > 0.0 { ImpactOutcome::Penetrated } else { ImpactOutcome::Absorbed },
        cos_impact: obl.cos_impact,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::HullMaterial;
    use bevy::math::IVec2;

    // Entry faces come from the walk as the OUTWARD normal of the face the
    // round came in through: entering through the -x face gives (-1, 0), so a
    // round travelling +x meets it dead-on.
    const WEST_FACE: IVec2 = IVec2::new(-1, 0);

    fn steel() -> Block { Block::hull(IVec2::ZERO, HullMaterial::Steel) }

    /// Dead-on is dead-on: no deflection, and no thickness inflation, because
    /// the walk's span already owns the through-cell geometry.
    #[test]
    fn head_on_hit_neither_bounces_nor_thickens() {
        let o = obliquity(WEST_FACE, Vec2::X, &steel(), Some(KineticAmmoType::AP), 1.0);
        assert!(!o.ricochet);
        assert!((o.cos_impact - 1.0).abs() < 1e-3);
        assert!((o.slope_mult - 1.0).abs() < 1e-3, "unsloped armour must not change balance");
    }

    /// An UNSLOPED plate struck obliquely still leaves slope_mult at 1.0 —
    /// double-counting it against the walk's span is the easy mistake here.
    #[test]
    fn oblique_hit_on_flat_plate_leaves_thickness_to_the_span() {
        for degrees in [15.0_f32, 30.0, 45.0, 60.0] {
            let dir = Vec2::from_angle(degrees.to_radians());
            let o = obliquity(WEST_FACE, dir, &steel(), Some(KineticAmmoType::AP), 1.0);
            assert!((o.slope_mult - 1.0).abs() < 1e-3, "slope_mult moved at {degrees}deg");
        }
    }

    /// Layer 0, the payoff that needs no new blocks: because the direction is
    /// in SHIP-LOCAL space, turning the hull off the threat axis is what makes
    /// a flat plate glance. 30 degrees bites; 75 skips.
    #[test]
    fn angling_the_hull_makes_flat_armour_glance() {
        let shallow = Vec2::from_angle(30f32.to_radians());
        assert!(!obliquity(WEST_FACE, shallow, &steel(), Some(KineticAmmoType::AP), 0.45).ricochet);

        let steep = Vec2::from_angle(75f32.to_radians());
        assert!(obliquity(WEST_FACE, steep, &steel(), Some(KineticAmmoType::AP), 0.45).ricochet);
    }

    /// Slope protects the face it points at and nothing else. The same round
    /// arriving at the plate's BACK gets no benefit — flanking beats angling.
    #[test]
    fn a_slope_does_nothing_from_behind() {
        let sloped = Block { slope: 0.9, ..steel() };
        let from_behind = obliquity(IVec2::new(1, 0), Vec2::X, &sloped, Some(KineticAmmoType::AP), 1.0);
        assert!(!from_behind.ricochet);
        assert_eq!(from_behind.slope_mult, 1.0);
        assert_eq!(from_behind.cos_impact, 1.0);
    }

    /// A declared slope tilts the plate inside its cell, which puts more steel
    /// in the way than the span alone accounts for.
    #[test]
    fn declared_slope_thickens_the_plate() {
        let sloped = Block { slope: 55f32.to_radians(), ..steel() };
        let o = obliquity(WEST_FACE, Vec2::X, &sloped, Some(KineticAmmoType::AP), 1.0);
        assert!(o.slope_mult > 1.5, "55deg slope should inflate LOS thickness, got {}", o.slope_mult);
        let hit = resolve_impact(60.0, &sloped, 1.0, o, None);
        assert!(hit.effective_thickness > steel().thickness);
    }

    /// Ammo decides what a slope is worth. A dart barely skips, a squash head
    /// never does (it isn't trying to get through), and flak bounces off
    /// anything angled.
    #[test]
    fn rounds_differ_in_what_they_bounce_off() {
        // 78deg: past AP's 70deg threshold even after 4deg of normalisation,
        // but still inside APFSDS's 78deg one.
        let steep = Vec2::from_angle(78f32.to_radians());
        let plate = Block::hull(IVec2::ZERO, HullMaterial::Composite);
        let bounced = |ammo| obliquity(WEST_FACE, steep, &plate, Some(ammo), 0.45).ricochet;

        assert!(!bounced(KineticAmmoType::APFSDS), "a long rod should bite at 78deg");
        assert!(!bounced(KineticAmmoType::HESH), "HESH squashes, it never ricochets");
        assert!(bounced(KineticAmmoType::HEFrag), "frag should skip off angled plate");
        assert!(bounced(KineticAmmoType::AP), "AP should skip past its 70deg threshold");
    }

    /// Overmatch: a round far fatter than the plate punches through however
    /// it's angled. A railgun beats bare steel; a gatling does not.
    #[test]
    fn a_big_enough_gun_overmatches_thin_plate() {
        let steep = Vec2::from_angle(80f32.to_radians());
        let railgun = obliquity(WEST_FACE, steep, &steel(), Some(KineticAmmoType::AP), 1.25);
        let gatling = obliquity(WEST_FACE, steep, &steel(), Some(KineticAmmoType::AP), 0.45);
        assert!(!railgun.ricochet, "railgun should overmatch 15mm-equivalent steel");
        assert!(gatling.ricochet, "gatling should skip off the same plate");

        // ...but nothing overmatches the good stuff.
        let alloy = Block::hull(IVec2::ZERO, HullMaterial::AbyssalAlloy);
        assert!(obliquity(WEST_FACE, steep, &alloy, Some(KineticAmmoType::AP), 1.25).ricochet);
    }

    /// A bounced round spends almost nothing on the plate and passes nothing
    /// inward — the block behind cover stays untouched.
    #[test]
    fn a_ricochet_scratches_the_plate_and_passes_nothing_through() {
        let steep = Vec2::from_angle(80f32.to_radians());
        let o = obliquity(WEST_FACE, steep, &steel(), Some(KineticAmmoType::AP), 0.45);
        assert!(o.ricochet);
        let hit = resolve_impact(100.0, &steel(), 1.0, o, Some(0.9));
        assert_eq!(hit.outcome, ImpactOutcome::Ricochet);
        assert_eq!(hit.through, 0.0, "a bounce must not drive damage into cover");
        assert!(hit.to_block < 20.0, "a skip should scratch, not bite");
    }

    /// Modules carry no armour, so there's nothing to skip off — an exposed
    /// engine eats the round whatever the angle.
    #[test]
    fn unarmoured_blocks_never_ricochet() {
        let steep = Vec2::from_angle(85f32.to_radians());
        let o = obliquity(WEST_FACE, steep, &Block::module(IVec2::ZERO), Some(KineticAmmoType::AP), 0.45);
        assert!(!o.ricochet);
    }

    /// A round that began the step already inside the cell penetrated last
    /// frame; there's no surface left to skip off.
    #[test]
    fn no_entry_face_means_no_deflection() {
        let o = obliquity(IVec2::ZERO, Vec2::X, &steel(), Some(KineticAmmoType::AP), 0.45);
        assert_eq!(o, Obliquity::HEAD_ON);
    }

    #[test]
    fn plate_absorbs_up_to_its_rating_and_passes_the_rest() {
        let steel = Block::hull(IVec2::ZERO, HullMaterial::Steel); // 15
        let hit = resolve_impact(40.0, &steel, 1.0, Obliquity::HEAD_ON, None);
        assert_eq!(hit.to_block, 15.0);
        assert_eq!(hit.through, 25.0);
        assert_eq!(hit.effective_thickness, 15.0);
    }

    #[test]
    fn weak_round_is_fully_absorbed() {
        let alloy = Block::hull(IVec2::ZERO, HullMaterial::AbyssalAlloy); // 80
        let hit = resolve_impact(30.0, &alloy, 1.0, Obliquity::HEAD_ON, None);
        assert_eq!(hit.to_block, 30.0);
        assert_eq!(hit.through, 0.0);
    }

    #[test]
    fn diagonal_span_thickens_the_plate() {
        let steel = Block::hull(IVec2::ZERO, HullMaterial::Steel);
        let hit = resolve_impact(40.0, &steel, 2f32.sqrt(), Obliquity::HEAD_ON, None);
        assert!((hit.effective_thickness - 21.213).abs() < 1e-2);
        assert!((hit.to_block - 21.213).abs() < 1e-2);
    }

    #[test]
    fn ammo_rule_splits_by_fraction_regardless_of_thickness() {
        let alloy = Block::hull(IVec2::ZERO, HullMaterial::AbyssalAlloy);
        let hit = resolve_impact(40.0, &alloy, 1.0, Obliquity::HEAD_ON, Some(0.3));
        assert!((hit.to_block - 28.0).abs() < 1e-4);
        assert!((hit.through - 12.0).abs() < 1e-4);
        let exposed = resolve_impact(40.0, &Block::module(IVec2::ZERO), 1.0, Obliquity::HEAD_ON, Some(0.0));
        assert_eq!(exposed.to_block, 40.0);
    }

    #[test]
    fn unarmoured_module_absorbs_nothing_by_thickness() {
        let hit = resolve_impact(40.0, &Block::module(IVec2::ZERO), 1.0, Obliquity::HEAD_ON, None);
        assert_eq!(hit.to_block, 0.0);
        assert_eq!(hit.through, 40.0);
    }
}
