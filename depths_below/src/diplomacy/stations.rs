//! Faction stations: each faction parks its station by the capital of its
//! systems (the biggest planet), and the Terran Hegemony rings its with gun
//! platforms. Dock with F, and the station's prices are set by your standing
//! (see `StandingTier::station_prices`); a faction that counts you Hostile
//! won't let you in, and its station shoots.
//!
//! A station is an AI ship with no engines, anchored to its planet: it rides
//! the planet's orbit just outside it, on the side away from the star. It
//! exists as entities only while you are near it, so a system's station costs
//! nothing while you are on the far side of the system.

use bevy::prelude::*;

use super::FactionStanding;
use crate::ai_ship::components::{faction_display_name, AiShipState, AiShipType};
use crate::building::registry::ModuleRegistry;
use crate::celestial::components::{CelestialBody, Planet, StarSystemMember};
use crate::celestial::resources::{GalaxyMap, SystemStreamingManager};
use crate::components::{Ship, Velocity};
use crate::events::{AiShipDestroyed, NotificationType, ShowNotification};
use crate::resources::{DockTerms, Inventory, InteractPress, MarketEvents};
use crate::states::GameState;

/// One faction's station, and what guards it.
#[derive(Clone, Copy, Debug)]
pub struct StationPlan {
    /// Design under designs/factions/<slug>/.
    pub key: &'static str,
    pub name: &'static str,
    /// Gun platform design ringing the station, if the faction has one.
    pub platform: Option<&'static str>,
    pub platforms: usize,
}

const fn plan(key: &'static str, name: &'static str, platform: Option<&'static str>, platforms: usize) -> StationPlan {
    StationPlan { key, name, platform, platforms }
}

// Approved on the fleet concepts page. The Recursive Kingdom's Hive is back
// for review; until it is approved the Kingdom has no station.
const PLANS: &[(AiShipType, StationPlan)] = &[
    (AiShipType::TerranHegemony, plan("bastion", "Bastion", Some("gun_platform"), 4)),
    (AiShipType::EternalHegemony, plan("citadel", "Citadel", None, 0)),
    (AiShipType::GildedThrone, plan("exchange", "The Exchange", None, 0)),
    (AiShipType::BrokenChoir, plan("broken_spire", "Broken Spire", None, 0)),
    (AiShipType::SynthesisCollective, plan("growth_node", "Growth Node", None, 0)),
    (AiShipType::CorpseStars, plan("reliquary", "Reliquary", None, 0)),
    (AiShipType::StellarPreserve, plan("sanctuary", "Sanctuary", None, 0)),
    (AiShipType::TheSilence, plan("monastery", "Monastery", None, 0)),
    (AiShipType::Shepherd, plan("the_fold", "The Fold", None, 0)),
    (AiShipType::MerchantPrinces, plan("trade_post", "Guild Trade Post", None, 0)),
    // Drawn 2026-10-21 for the trade war; awaiting review.
    (AiShipType::FreeHaulersCompact, plan("depot", "Depot", None, 0)),
];

/// Control a point of trade standing buys the station's owner.
const TRADE_PUSH: f32 = 0.5;

/// How a faction's own prices lean, on top of what your standing gets you:
/// the Princes charge more for their luxuries and pay more for your cargo;
/// the Compact is cheap at both ends. (buy, sell) multipliers.
pub fn price_lean(faction: AiShipType) -> (f32, f32) {
    match faction {
        AiShipType::MerchantPrinces => (1.1, 1.1),
        AiShipType::FreeHaulersCompact => (0.9, 0.9),
        _ => (1.0, 1.0),
    }
}

/// What a free port (an ownerless station, Haven included) charges: worse
/// than any faction that will let you dock, so it's the fallback when you're
/// at war with everyone rather than the default. (Unfriendly stations charge
/// up to x1.65 with a faction's own lean and pay at least x0.63; these are
/// worse at both ends.)
pub const FREE_PORT: (f32, f32) = (1.7, 0.55);
/// The opening grace: ordinary prices for the first ninety minutes of play,
/// then easing to free-port prices over the next half hour.
const GRACE_SECONDS: f32 = 90.0 * 60.0;
const GRACE_EASE_SECONDS: f32 = 30.0 * 60.0;

/// Free-port (buy, sell) after `play_time` seconds of play.
pub fn free_port_terms(play_time: f32) -> (f32, f32) {
    let t = ((play_time - GRACE_SECONDS) / GRACE_EASE_SECONDS).clamp(0.0, 1.0);
    (1.0 + (FREE_PORT.0 - 1.0) * t, 1.0 + (FREE_PORT.1 - 1.0) * t)
}

pub fn station_plan(faction: AiShipType) -> Option<StationPlan> {
    PLANS.iter().find(|(f, _)| *f == faction).map(|(_, p)| *p)
}

/// The station proper (not a platform). Killing one costs the most standing.
#[derive(Component, Clone, Copy, Debug)]
pub struct StationCore;

/// A gun platform guarding a station.
#[derive(Component, Clone, Copy, Debug)]
pub struct StationPlatform;

/// Holds a station or platform in place relative to its planet.
#[derive(Component, Clone, Copy, Debug)]
pub struct StationAnchor {
    pub planet: Entity,
    pub star: Vec2,
    /// Distance out from the planet's centre, along the star-to-planet line.
    pub out: f32,
    /// Offset from that point, for platforms around the station.
    pub around: Vec2,
    /// How far the hull reaches from its centre, for docking range.
    pub extent: f32,
}

/// The live station of the loaded system, and the systems whose station you
/// have destroyed (it doesn't grow back this run).
#[derive(Resource, Default)]
pub struct FactionStations {
    pub live: Option<LiveStation>,
    pub destroyed: Vec<u32>,
    /// Where the loaded system's station is (system, centre, hull reach),
    /// whether or not it has materialised: its guard fleet holds there and
    /// its convoys run to it. None with no station, or a destroyed one.
    pub post: Option<(u32, Vec2, f32)>,
}

pub struct LiveStation {
    pub system_id: u32,
    pub core: Entity,
    pub parts: Vec<Entity>,
}

/// Materialise within this distance of the anchor point, dissolve past the second.
const SPAWN_RANGE: f32 = 40_000.0;
const DESPAWN_RANGE: f32 = 55_000.0;
/// Clear water between the planet's surface and the station's hull.
const PLANET_GAP: f32 = 6_000.0;
/// How far past the station's own hull you can dock from.
pub const DOCK_MARGIN: f32 = 3_500.0;

/// How far a design reaches from its centre, in world units.
fn design_extent(faction: AiShipType, key: &str) -> f32 {
    let path = format!("designs/factions/{}/{}.json", crate::ai_ship::design_slug(faction), key);
    crate::building::blueprint::load_design_file(&path)
        .map(|bp| {
            bp.hull_cells
                .iter()
                .map(|c| c.grid_pos.as_vec2().length())
                .fold(0.0_f32, f32::max)
                * 66.0
                + 66.0
        })
        .unwrap_or(3_000.0)
}

fn anchor_point(planet_pos: Vec2, anchor: &StationAnchor) -> Vec2 {
    let out = (planet_pos - anchor.star).normalize_or(Vec2::X);
    planet_pos + out * anchor.out + anchor.around
}

/// Spawn the loaded system's station when you come near its capital, and
/// clear it away when you leave.
#[allow(clippy::too_many_arguments)]
fn sync_faction_stations(
    mut commands: Commands,
    streaming: Res<SystemStreamingManager>,
    galaxy: Res<GalaxyMap>,
    mut stations: ResMut<FactionStations>,
    planets: Query<(Entity, &Transform, &CelestialBody, &StarSystemMember), With<Planet>>,
    player: Query<&Transform, With<Ship>>,
    alive: Query<&AiShipState>,
    registry: Res<ModuleRegistry>,
    asset_server: Res<AssetServer>,
) {
    let Ok(player) = player.single() else { return };
    let player_pos = player.translation.truncate();
    let system = streaming.loaded_system.and_then(|id| galaxy.systems.iter().find(|s| s.id == id));

    // Forget a station whose system is gone or which has been destroyed.
    let stale = stations.live.as_ref().and_then(|live| {
        let wrong_system = system.map_or(true, |s| s.id != live.system_id);
        // Destroyed is a station shot down; gone (despawned by a load, say)
        // is only a record to drop.
        let core_dead = alive.get(live.core).is_ok_and(|s| s.is_destroyed);
        let core_gone = alive.get(live.core).is_err();
        (wrong_system || core_dead || core_gone).then(|| (wrong_system, core_dead))
    });
    if let Some((wrong_system, core_dead)) = stale {
        let live = stations.live.take().unwrap();
        if core_dead && !wrong_system && !stations.destroyed.contains(&live.system_id) {
            stations.destroyed.push(live.system_id);
        }
        // A dead station stays as its wreck; one left behind in another
        // system goes with it.
        if wrong_system {
            for e in std::iter::once(live.core).chain(live.parts) {
                if let Ok(mut ec) = commands.get_entity(e) {
                    ec.despawn();
                }
            }
        }
    }

    stations.post = None;
    let Some(system) = system else { return };
    let Some(faction) = system.faction else { return };
    let Some(plan) = station_plan(faction) else { return };
    if stations.destroyed.contains(&system.id) {
        return;
    }
    // The capital: the system's biggest planet.
    let Some((planet, planet_tf, body, _)) = planets
        .iter()
        .filter(|(_, _, _, m)| m.system_id == system.id)
        .max_by(|a, b| a.2.radius.total_cmp(&b.2.radius))
    else {
        return;
    };
    let planet_pos = planet_tf.translation.truncate();
    let extent = design_extent(faction, plan.key);
    let core_anchor = StationAnchor {
        planet,
        star: system.local_center,
        out: body.radius + PLANET_GAP + extent,
        around: Vec2::ZERO,
        extent,
    };
    let at = anchor_point(planet_pos, &core_anchor);
    let dist = player_pos.distance(at);
    stations.post = Some((system.id, at, extent));

    if let Some(live) = &stations.live {
        if dist > DESPAWN_RANGE {
            for &e in std::iter::once(&live.core).chain(live.parts.iter()) {
                if let Ok(mut ec) = commands.get_entity(e) {
                    ec.despawn();
                }
            }
            stations.live = None;
        }
        return;
    }
    if dist > SPAWN_RANGE {
        return;
    }

    let core = crate::ai_ship::spawner::spawn_ai_ship_class(
        faction, Some(plan.key), at, &mut commands, &registry, &asset_server,
    );
    commands.entity(core).insert((
        super::FactionStation,
        StationCore,
        core_anchor,
        StarSystemMember { system_id: system.id },
    ));
    let mut parts = Vec::new();
    if let Some(platform) = plan.platform {
        let reach = extent + design_extent(faction, platform) + 2_500.0;
        for i in 0..plan.platforms {
            let angle = std::f32::consts::TAU * (i as f32 + 0.5) / plan.platforms as f32;
            let around = Vec2::from_angle(angle) * reach;
            let anchor = StationAnchor { around, extent: design_extent(faction, platform), ..core_anchor };
            let e = crate::ai_ship::spawner::spawn_ai_ship_class(
                faction, Some(platform), at + around, &mut commands, &registry, &asset_server,
            );
            commands.entity(e).insert((
                super::FactionStation,
                StationPlatform,
                anchor,
                StarSystemMember { system_id: system.id },
            ));
            parts.push(e);
        }
    }
    info!("Faction station: {} {} at the capital of system {}", faction_display_name(faction), plan.name, system.id);
    stations.live = Some(LiveStation { system_id: system.id, core, parts });
}

/// Ride the planet. Runs after everything that moves ships, so nothing a
/// station's brain or a collision decides can carry it off.
fn hold_anchored(
    planets: Query<&Transform, (With<Planet>, Without<StationAnchor>)>,
    mut anchored: Query<(&StationAnchor, &mut Transform, Option<&mut Velocity>), Without<Planet>>,
) {
    for (anchor, mut tf, velocity) in anchored.iter_mut() {
        let at = match planets.get(anchor.planet) {
            Ok(planet) => anchor_point(planet.translation.truncate(), anchor),
            // No planet (a test station): hold the point it was put at.
            Err(_) => anchor.star + anchor.around,
        };
        tf.translation.x = at.x;
        tf.translation.y = at.y;
        tf.rotation = Quat::IDENTITY;
        if let Some(mut v) = velocity {
            v.0 = Vec2::ZERO;
        }
    }
}

/// F near a faction station: dock on your standing's terms, or be refused.
fn faction_station_docking(
    mut press: ResMut<InteractPress>,
    player: Query<&Transform, With<Ship>>,
    cores: Query<(&Transform, &StationAnchor, &AiShipType, &AiShipState), With<StationCore>>,
    standing: Res<FactionStanding>,
    mut market: ResMut<MarketEvents>,
    mut notifications: MessageWriter<ShowNotification>,
    mut next_state: ResMut<NextState<GameState>>,
    mut prompted: Local<bool>,
    where_: (Res<GalaxyMap>, Res<SystemStreamingManager>),
) {
    let (galaxy, streaming) = where_;
    let Ok(player) = player.single() else { return };
    let pos = player.translation.truncate();
    let near = cores.iter().find(|(tf, anchor, _, state)| {
        !state.is_destroyed && tf.translation.truncate().distance(pos) < anchor.extent + DOCK_MARGIN
    });
    let Some((_, _, &faction, _)) = near else {
        *prompted = false;
        return;
    };
    // Over a capital world the starport is the faction's High Command.
    let capital = streaming
        .loaded_system
        .and_then(|id| galaxy.systems.iter().find(|s| s.id == id))
        .filter(|s| s.capital && s.faction == Some(faction));
    let name = match capital {
        Some(sys) => format!(
            "{} High Command, {}",
            faction_display_name(faction),
            crate::celestial::capital_worlds::capital_world_name(&sys.name)
        ),
        None => format!("{} {}", faction_display_name(faction), station_plan(faction).map_or("station", |p| p.name)),
    };
    let tier = standing.tier(faction);
    let (lean_buy, lean_sell) = price_lean(faction);
    let prices = tier.station_prices().map(|(b, s)| (b * lean_buy, s * lean_sell));
    if !*prompted {
        *prompted = true;
        let message = match prices {
            Some((buy, sell)) => format!("{name}: press F to dock ({}, buy x{buy:.2}, sell x{sell:.2})", tier.label()),
            None => format!("{name} counts you Hostile and won't let you dock"),
        };
        notifications.write(ShowNotification { message, notification_type: NotificationType::Info, duration: 4.0 });
    }
    if !press.pending() {
        return;
    }
    press.claim();
    let Some((buy, sell)) = prices else {
        notifications.write(ShowNotification {
            message: format!("{name} refuses you. Improve your standing first (X: diplomacy)."),
            notification_type: NotificationType::Danger,
            duration: 4.0,
        });
        return;
    };
    market.dock_terms = Some(DockTerms { faction: Some(faction), name: name.clone(), standing: tier.label(), buy, sell });
    notifications.write(ShowNotification {
        message: format!("Docking at {name}..."),
        notification_type: NotificationType::Info,
        duration: 2.0,
    });
    next_state.set(GameState::Docked);
}

fn clear_dock_terms(mut market: ResMut<MarketEvents>) {
    market.dock_terms = None;
}

/// A system changed hands: the old owner's station there goes (the new
/// owner's appears next time you come near), and one you'd destroyed is
/// rebuilt by the new owner.
fn station_changes_hands(
    mut commands: Commands,
    mut flips: MessageReader<crate::celestial::territory::SystemChangedHands>,
    mut stations: ResMut<FactionStations>,
) {
    for flip in flips.read() {
        stations.destroyed.retain(|&s| s != flip.system_id);
        if stations.live.as_ref().is_some_and(|l| l.system_id == flip.system_id) {
            let live = stations.live.take().unwrap();
            for e in std::iter::once(live.core).chain(live.parts) {
                if let Ok(mut ec) = commands.get_entity(e) {
                    ec.try_despawn();
                }
            }
        }
    }
}

/// Docked at an ownerless station: free-port prices (after the opening grace).
fn free_port_docking(
    mut market: ResMut<MarketEvents>,
    stats: Res<crate::resources::Statistics>,
    player: Query<&Transform, With<Ship>>,
    stations: Res<crate::world::home_base::SystemStations>,
) {
    if market.dock_terms.is_some() {
        return;
    }
    let index = player
        .single()
        .ok()
        .and_then(|t| stations.nearest_index(t.translation.truncate()))
        .unwrap_or(0);
    let (buy, sell) = free_port_terms(stats.play_time_seconds);
    market.dock_terms = Some(DockTerms {
        faction: None,
        name: crate::world::home_base::station_display_name(index),
        standing: "Free Port",
        buy,
        sell,
    });
}

/// Trading at a faction's station earns a little standing with it: a fifth
/// of a point per item bought or sold, a few points at most per deal.
fn standing_from_trade(
    market: Res<MarketEvents>,
    inventory: Res<Inventory>,
    mut standing: ResMut<FactionStanding>,
    mut last: Local<Option<u32>>,
    // Trade also strengthens the station's owner's hold on its system.
    border: (
        ResMut<crate::celestial::territory::BorderWar>,
        ResMut<crate::celestial::resources::GalaxyMap>,
        Res<SystemStreamingManager>,
    ),
) {
    let (mut war, mut galaxy, streaming) = border;
    let held: u32 = inventory.items.values().sum();
    let Some((terms, faction)) = market.dock_terms.as_ref().and_then(|t| t.faction.map(|f| (t, f))) else {
        *last = None;
        return;
    };
    let Some(before) = *last else {
        *last = Some(held);
        return;
    };
    if held != before {
        let moved = held.abs_diff(before) as f32;
        let gain = (moved * 0.2).clamp(0.5, 3.0);
        let verb = if held < before { "sold" } else { "bought" };
        standing.change(faction, gain, format!("You {verb} goods at {}", terms.name));
        // Strengthening an owner never hands its system to anyone else.
        if let Some(system) = streaming.loaded_system {
            war.push(&mut galaxy, system, faction, gain * TRADE_PUSH);
        }
        // The trade war: business with one guild is business taken from the other.
        if faction.is_trading() {
            for &rival in super::FACTIONS.iter() {
                if rival != faction && rival.is_trading() && crate::ai_ship::components::factions_hostile(rival, faction) {
                    standing.change(rival, -gain * 0.5, format!("You traded with their rival at {}", terms.name));
                }
            }
        }
        *last = Some(held);
    }
}

/// A station's death leaves its system without one for the rest of the run.
fn remember_destroyed(
    mut deaths: MessageReader<AiShipDestroyed>,
    cores: Query<&StarSystemMember, With<StationCore>>,
    mut stations: ResMut<FactionStations>,
) {
    for death in deaths.read() {
        if let Ok(member) = cores.get(death.entity) {
            if !stations.destroyed.contains(&member.system_id) {
                stations.destroyed.push(member.system_id);
            }
        }
    }
}

/// DEPTHS_STATION_TEST=<faction> (the AiShipType name, e.g. TerranHegemony):
/// move you 60 km out into open space and park that faction's station,
/// platforms and all, just east of you, so it can be looked at and docked
/// with without flying to its capital. It holds still rather than riding a
/// planet.
fn spawn_test_station(
    mut commands: Commands,
    mut done: Local<bool>,
    mut player: Query<&mut Transform, With<Ship>>,
    registry: Res<ModuleRegistry>,
    asset_server: Res<AssetServer>,
) {
    if *done {
        return;
    }
    let Ok(wanted) = std::env::var("DEPTHS_STATION_TEST") else {
        *done = true;
        return;
    };
    let Ok(mut player) = player.single_mut() else { return };
    *done = true;
    // Out into open space first: the start berth has Haven and a salvageable
    // wreck in reach, and either would take the F press.
    player.translation.y += 60_000.0;
    let Some(&faction) = super::FACTIONS.iter().find(|f| format!("{f:?}") == wanted) else {
        warn!("DEPTHS_STATION_TEST: no faction called {wanted:?}");
        return;
    };
    let Some(plan) = station_plan(faction) else {
        warn!("DEPTHS_STATION_TEST: {wanted} has no station yet");
        return;
    };
    let extent = design_extent(faction, plan.key);
    let at = player.translation.truncate() + Vec2::X * (extent + 2_500.0);
    let fixed = StationAnchor { planet: Entity::PLACEHOLDER, star: at, out: 0.0, around: Vec2::ZERO, extent };
    let core = crate::ai_ship::spawner::spawn_ai_ship_class(faction, Some(plan.key), at, &mut commands, &registry, &asset_server);
    commands.entity(core).insert((super::FactionStation, StationCore, fixed));
    if let Some(platform) = plan.platform {
        let reach = extent + design_extent(faction, platform) + 2_500.0;
        for i in 0..plan.platforms {
            let around = Vec2::from_angle(std::f32::consts::TAU * (i as f32 + 0.5) / plan.platforms as f32) * reach;
            let e = crate::ai_ship::spawner::spawn_ai_ship_class(faction, Some(platform), at + around, &mut commands, &registry, &asset_server);
            commands.entity(e).insert((super::FactionStation, StationPlatform, StationAnchor { around, ..fixed }));
        }
    }
    info!("DEPTHS_STATION_TEST: {} {} parked at {:?}", faction_display_name(faction), plan.name, at);
}

pub struct FactionStationPlugin;

impl Plugin for FactionStationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FactionStations>()
            .add_systems(
                Update,
                (
                    sync_faction_stations,
                    spawn_test_station,
                    remember_destroyed,
                    // Ahead of the ordinary stations: when you're beside a
                    // faction station, F is for it. (They sit by planets, far
                    // from the system stations, so the two rarely meet.)
                    faction_station_docking
                        .after(crate::crew::eva_salvage::order_salvage_detail)
                        .before(crate::world::home_base::station_docking),
                )
                    .run_if(in_state(GameState::Exploring)),
            )
            .add_systems(PostUpdate, hold_anchored.before(bevy::transform::TransformSystems::Propagate))
            .add_systems(Update, standing_from_trade.run_if(in_state(GameState::Docked)))
            // Cleared on the way back out to space, not on leaving the dock:
            // pausing while docked leaves and re-enters Docked, and the
            // station's terms must survive that.
            .add_systems(OnEnter(GameState::Exploring), clear_dock_terms)
            .add_systems(Update, station_changes_hands)
            .add_systems(OnEnter(GameState::Docked), free_port_docking)
            .add_systems(OnEnter(GameState::StationDocked), free_port_docking);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diplomacy::StandingTier;

    /// Every station and platform on a plan has its design on disk.
    #[test]
    fn every_station_has_a_design() {
        for (faction, plan) in PLANS {
            let slug = crate::ai_ship::design_slug(*faction);
            for key in std::iter::once(plan.key).chain(plan.platform) {
                let path = format!("designs/factions/{slug}/{key}.json");
                assert!(
                    crate::building::blueprint::load_design_file(&path).is_some(),
                    "{path} is missing or doesn't parse"
                );
            }
        }
    }

    #[test]
    fn hostile_standing_shuts_the_door() {
        assert!(StandingTier::Hostile.station_prices().is_none());
        let (buy, sell) = StandingTier::Allied.station_prices().unwrap();
        let (nbuy, nsell) = StandingTier::Neutral.station_prices().unwrap();
        assert!(buy < nbuy && sell > nsell, "allies get a better deal than strangers");
    }

    /// A free port is ordinary at the start, then worse than any faction
    /// station that will let you dock.
    #[test]
    fn free_ports_are_the_fallback_after_the_grace() {
        assert_eq!(free_port_terms(0.0), (1.0, 1.0));
        assert_eq!(free_port_terms(GRACE_SECONDS), (1.0, 1.0));
        let (buy, sell) = free_port_terms(GRACE_SECONDS + GRACE_EASE_SECONDS * 2.0);
        assert_eq!((buy, sell), FREE_PORT);
        for tier in [StandingTier::Unfriendly, StandingTier::Neutral, StandingTier::Friendly, StandingTier::Allied] {
            let (b, s) = tier.station_prices().unwrap();
            for faction in crate::diplomacy::FACTIONS {
                let (lb, ls) = price_lean(faction);
                assert!(buy > b * lb && sell < s * ls, "{tier:?} {faction:?} is worse than a free port");
            }
        }
        // Halfway through the ease, halfway there.
        let (mid, _) = free_port_terms(GRACE_SECONDS + GRACE_EASE_SECONDS * 0.5);
        assert!((mid - 1.35).abs() < 1e-4);
    }

    /// Docking at an ownerless station sets free-port terms, after the grace;
    /// a faction station's terms are left alone.
    #[test]
    fn docking_at_a_free_port_sets_its_terms() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.init_resource::<MarketEvents>();
        app.insert_resource(crate::resources::Statistics { play_time_seconds: 5.0 * 3600.0, ..Default::default() });
        app.init_resource::<crate::world::home_base::SystemStations>();
        app.world_mut().spawn((Ship, Transform::default()));
        app.world_mut().run_system_once(free_port_docking).unwrap();
        let terms = app.world().resource::<MarketEvents>().dock_terms.clone().expect("free-port terms set");
        assert_eq!(terms.faction, None);
        assert_eq!((terms.buy, terms.sell), FREE_PORT);
        assert_eq!(terms.standing, "Free Port");

        // Already docked at a faction station: untouched.
        let mut market = app.world_mut().resource_mut::<MarketEvents>();
        market.dock_terms = Some(DockTerms {
            faction: Some(AiShipType::TerranHegemony), name: "x".into(), standing: "Neutral", buy: 1.2, sell: 0.85,
        });
        app.world_mut().run_system_once(free_port_docking).unwrap();
        let terms = app.world().resource::<MarketEvents>().dock_terms.clone().unwrap();
        assert_eq!(terms.faction, Some(AiShipType::TerranHegemony));
    }
}
