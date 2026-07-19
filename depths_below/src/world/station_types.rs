use crate::resources::ItemType;

// ============================================================================
// STATION TYPES
// Haven is always the Shipyard (the only station with build access). Every
// outpost is deterministically assigned one of the other types — stable
// across a run (seeded by station index), giving each outpost an actual
// identity instead of being an interchangeable resupply blob with an
// arbitrary price hash. Type drives both what an outpost pays well for
// (sell prices) and what it's cheap to buy from it (service costs).
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StationType {
    /// Haven only. Balanced prices, the only place with build access.
    Shipyard,
    /// Pays poorly for raw ore/crystal/alloy (they mine their own), pays
    /// over for imported fuel/ammo. Cheap hull repair (scrap on hand).
    MiningColony,
    /// Generous sell prices across the board — the place to dump mixed
    /// cargo — but no service discounts.
    TradeHub,
    /// Cheap ammo resupply, pays well for salvaged artifacts (intel value).
    MilitaryOutpost,
    /// Pays well for crystal/artifacts/bio samples (study value), poorly
    /// for scrap.
    ResearchOutpost,
    /// Cheap fuel resupply, otherwise unremarkable prices.
    RefuelDepot,
}

/// station_idx: 0 = Haven, 1..=12 = outposts (see world::home_base).
pub fn station_type(station_idx: usize) -> StationType {
    if station_idx == 0 {
        return StationType::Shipyard;
    }
    // Deterministic per-station hash — stable across the run, not re-rolled
    // every time you dock.
    let hash = (station_idx as u32).wrapping_mul(2654435761);
    match hash % 5 {
        0 => StationType::MiningColony,
        1 => StationType::TradeHub,
        2 => StationType::MilitaryOutpost,
        3 => StationType::ResearchOutpost,
        _ => StationType::RefuelDepot,
    }
}

pub fn station_type_name(t: StationType) -> &'static str {
    match t {
        StationType::Shipyard => "Shipyard",
        StationType::MiningColony => "Mining Colony",
        StationType::TradeHub => "Trade Hub",
        StationType::MilitaryOutpost => "Military Outpost",
        StationType::ResearchOutpost => "Research Outpost",
        StationType::RefuelDepot => "Refuel Depot",
    }
}

/// Base sell-price multiplier for `item` at a station of type `t`, before
/// the small per-station random jitter (see resources::station_item_price).
pub fn type_price_multiplier(t: StationType, item: ItemType) -> f32 {
    use ItemType::*;
    match (t, item) {
        (StationType::MiningColony, ScrapMetal) => 0.55,
        (StationType::MiningColony, Crystal) => 0.65,
        (StationType::MiningColony, RareAlloy) => 0.70,
        (StationType::MiningColony, FuelCell) => 1.30,
        (StationType::MiningColony, AmmoCrate) => 1.20,

        (StationType::TradeHub, _) => 1.20,

        (StationType::MilitaryOutpost, AmmoCrate) => 0.70,
        (StationType::MilitaryOutpost, ScrapMetal) => 1.10,
        (StationType::MilitaryOutpost, AncientArtifact) => 1.30,

        (StationType::ResearchOutpost, Crystal) => 1.50,
        (StationType::ResearchOutpost, AncientArtifact) => 1.60,
        (StationType::ResearchOutpost, BioSample) => 1.40,
        (StationType::ResearchOutpost, ScrapMetal) => 0.75,

        (StationType::RefuelDepot, FuelCell) => 0.60,

        _ => 1.0,
    }
}

/// Service cost multiplier for a station of type `t` — applied to the
/// credit portion of a docking/resupply service (after any resource offset).
#[derive(Clone, Copy, Default)]
pub struct ServiceDiscounts {
    pub fuel: f32,
    pub ammo: f32,
    pub hull_repair: f32,
}

pub fn service_discounts(t: StationType) -> ServiceDiscounts {
    match t {
        StationType::RefuelDepot => ServiceDiscounts { fuel: 0.5, ammo: 1.0, hull_repair: 1.0 },
        StationType::MilitaryOutpost => ServiceDiscounts { fuel: 1.0, ammo: 0.5, hull_repair: 1.0 },
        StationType::MiningColony => ServiceDiscounts { fuel: 1.0, ammo: 1.0, hull_repair: 0.7 },
        _ => ServiceDiscounts { fuel: 1.0, ammo: 1.0, hull_repair: 1.0 },
    }
}
