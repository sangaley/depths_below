//! Contact jobs: work on the living galaxy.
//!
//! Talk to a contact in a starport's Comm directory and they offer you
//! work; every job points at something the simulation is already doing, so
//! finishing one moves the world:
//!
//! | Job | Who offers it | What you do |
//! | --- | --- | --- |
//! | Bounty | Military, outposts | Destroy a named enemy fleet nearby |
//! | Hold the line | Military | Destroy hostiles in their contested system before time runs out |
//! | Escort | Trade | See one of their convoys to its next stop, staying close |
//! | Delivery | Trade, outposts | Carry sealed cargo to another of their stations |
//! | Raid | Underworld | Destroy a rival's convoy haulers |
//! | Survey | Underworld, outposts, the Abbess | Spend a minute in a system and report |
//!
//! A job pays on the spot when it's done: credits, the contact's regard,
//! standing with their faction, and for fighting jobs a push on the border.
//! Failing costs a little regard. Three jobs at a time.

use bevy::prelude::*;
use rand::{seq::SliceRandom, Rng};
use serde::{Deserialize, Serialize};

use super::contacts::{ContactDef, ContactRole, Contacts};
use super::FactionStanding;
use crate::ai_ship::components::{factions_hostile, faction_display_name, AiShipType, SimBehavior, WorldSimulation};
use crate::ai_ship::fleet::{role_of, FleetKind, Mission};
use crate::ai_ship::roster::ClassRole;
use crate::celestial::resources::{GalaxyMap, SystemStreamingManager};
use crate::celestial::territory::BorderWar;
use crate::resources::{Currency, ItemType};

/// Jobs you can hold at once.
pub const MAX_ACTIVE: usize = 3;
/// How long holding the line lasts.
const HOLD_SECONDS: f32 = 15.0 * 60.0;
/// How long a survey takes.
const SURVEY_SECONDS: f32 = 60.0;
/// How close you have to stay to a convoy you're escorting.
const ESCORT_RANGE: f32 = 15_000.0;
/// How near its stop a convoy counts as arrived.
const ARRIVED: f32 = 4_000.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum JobKind {
    Bounty { fleet: u32, system: u32 },
    Raid { fleet: u32, system: u32 },
    Escort { fleet: u32, system: u32, stop: Vec2, kept_close: bool },
    HoldTheLine { system: u32, kills: u32, needed: u32, left: f32 },
    Delivery { to_system: u32, item: ItemType, count: u32 },
    Survey { system: u32, seconds: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Offered,
    Active,
    Done,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: u32,
    /// The contact who gave it.
    pub giver: String,
    pub faction: AiShipType,
    pub kind: JobKind,
    pub title: String,
    pub brief: String,
    pub reward: u32,
    pub status: JobStatus,
}

impl Job {
    /// One line on how it stands, for the tracker and the Comm directory.
    pub fn progress(&self) -> String {
        match &self.kind {
            JobKind::HoldTheLine { kills, needed, left, .. } => {
                format!("{kills}/{needed} destroyed, {}:{:02} left", (*left as u32) / 60, (*left as u32) % 60)
            }
            JobKind::Survey { seconds, .. } => format!("{:.0}/{:.0}s scanned", seconds, SURVEY_SECONDS),
            JobKind::Escort { kept_close, .. } => {
                if *kept_close { "with the convoy".into() } else { "join the convoy".into() }
            }
            JobKind::Delivery { item, count, .. } => format!("{} aboard", cargo_count(*item, *count)),
            JobKind::Bounty { .. } | JobKind::Raid { .. } => "hunting".into(),
        }
    }
}

/// Every job offered to you, in hand, or just settled.
#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Jobs {
    pub list: Vec<Job>,
    #[serde(default)]
    pub next_id: u32,
}

impl Jobs {
    pub fn active(&self) -> impl Iterator<Item = &Job> {
        self.list.iter().filter(|j| j.status == JobStatus::Active)
    }

    /// The job a contact has open with you: offered or in hand.
    pub fn of(&self, giver: &str) -> Option<&Job> {
        self.list.iter().find(|j| j.giver == giver && matches!(j.status, JobStatus::Offered | JobStatus::Active))
    }

    /// Take a contact's offered job, if you have room. A delivery's cargo
    /// goes aboard through `load`, which says whether the hold took it.
    pub fn accept(&mut self, giver: &str, load: impl FnOnce(ItemType, u32) -> bool) -> Result<(), &'static str> {
        if self.active().count() >= MAX_ACTIVE {
            return Err("You already have three jobs in hand.");
        }
        let Some(job) = self.list.iter_mut().find(|j| j.giver == giver && j.status == JobStatus::Offered) else {
            return Err("They have nothing for you.");
        };
        if let JobKind::Delivery { item, count, .. } = job.kind {
            if !load(item, count) {
                return Err("There's no room in your hold for the cargo.");
            }
        }
        job.status = JobStatus::Active;
        Ok(())
    }

    /// Offers you walked away from lapse; work in hand stays.
    pub fn drop_offers(&mut self) {
        self.list.retain(|j| j.status != JobStatus::Offered);
    }

    pub fn push(&mut self, mut job: Job) {
        self.next_id += 1;
        job.id = self.next_id;
        self.list.push(job);
    }
}

/// What a faction ships when it ships something sealed.
fn cargo_of(faction: AiShipType) -> ItemType {
    use AiShipType::*;
    match faction {
        TerranHegemony => ItemType::AmmoCrate,
        GildedThrone | CorpseStars => ItemType::RareAlloy,
        MerchantPrinces | BrokenChoir => ItemType::AncientArtifact,
        FreeHaulersCompact => ItemType::FuelCell,
        TheSilence => ItemType::Crystal,
        SynthesisCollective | StellarPreserve => ItemType::BioSample,
        _ => ItemType::ScrapMetal,
    }
}

/// "A Terran Hegemony patrol", "The Silence's patrol".
fn a_fleet_of(faction: AiShipType, what: &str) -> String {
    let name = faction_display_name(faction);
    if name.starts_with("The ") { format!("{name}'s {what}") } else { format!("A {name} {what}") }
}

/// "3 Ammo Crates", "2 crates of Rare Alloy".
fn cargo_count(item: ItemType, count: u32) -> String {
    match item {
        ItemType::ScrapMetal | ItemType::RareAlloy => format!("{count} crates of {}", item.name()),
        _ => format!("{count} {}s", item.name()),
    }
}

/// The kinds of work a contact gives, by their role.
fn kinds_for(role: ContactRole) -> &'static [&'static str] {
    match role {
        ContactRole::Military => &["bounty", "hold"],
        ContactRole::Trade => &["escort", "delivery"],
        ContactRole::Underworld => &["raid", "survey"],
        ContactRole::Outpost => &["bounty", "delivery", "survey"],
        ContactRole::Hub => &["survey"],
    }
}

/// The world as a job is written against.
pub struct World<'a> {
    pub sim: &'a WorldSimulation,
    pub galaxy: &'a GalaxyMap,
    pub war: &'a BorderWar,
    pub here: u32,
}

/// Write a job for a contact from what the galaxy is doing right now, or
/// None if there's nothing of their kind to be had.
pub fn offer_for(def: &ContactDef, relationship: f32, world: &World, rng: &mut impl Rng) -> Option<Job> {
    let name_of = |id: u32| world.galaxy.systems.iter().find(|s| s.id == id).map_or("somewhere".to_string(), |s| s.name.clone());
    let mut near = vec![world.here];
    near.extend(crate::celestial::galaxy::nearest_neighbors(world.galaxy, world.here, 3));
    let alive = |fleet: u32| world.sim.ships.iter().filter(|s| s.fleet == fleet && s.behavior != SimBehavior::Dead).collect::<Vec<_>>();
    let blocks = |fleet: u32| -> u32 {
        alive(fleet).iter().map(|s| crate::ai_ship::spawner::design_blocks(s.faction, s.class) as u32).sum()
    };
    // Better pay for people who trust you.
    let pay = |base: f32| ((base * (1.0 + relationship / 100.0)) / 10.0).round() as u32 * 10;
    let faction = def.faction;
    let job = |kind: JobKind, title: String, brief: String, reward: u32| Job {
        id: 0,
        giver: def.id.clone(),
        faction,
        kind,
        title,
        brief,
        reward,
        status: JobStatus::Offered,
    };

    let mut kinds: Vec<&str> = kinds_for(def.role).to_vec();
    kinds.shuffle(rng);
    for kind in kinds {
        match kind {
            "bounty" => {
                let targets: Vec<_> = world
                    .sim
                    .fleets
                    .iter()
                    .filter(|f| near.contains(&f.system_id) && f.mission == Mission::None)
                    .filter(|f| matches!(f.kind, FleetKind::Patrol | FleetKind::Raiders | FleetKind::BattleGroup))
                    .filter(|f| factions_hostile(faction, f.faction) && !alive(f.id).is_empty())
                    // The things that haunt the deep are the jackpot board's,
                    // not a contact's errand.
                    .filter(|f| !crate::celestial::territory::ROAMING.contains(&f.faction))
                    .collect();
                if let Some(f) = targets.choose(rng) {
                    let n = alive(f.id).len();
                    let what = match f.kind { FleetKind::BattleGroup => "battle group", FleetKind::Raiders => "raider band", _ => "patrol" };
                    return Some(job(
                        JobKind::Bounty { fleet: f.id, system: f.system_id },
                        format!("Bounty: {} {what}", faction_display_name(f.faction)),
                        format!("{} of {n} in {}. Destroy every hull.", a_fleet_of(f.faction, what), name_of(f.system_id)),
                        pay(300.0 + blocks(f.id) as f32 * 0.6),
                    ));
                }
            }
            "raid" => {
                let targets: Vec<_> = world
                    .sim
                    .fleets
                    .iter()
                    .filter(|f| near.contains(&f.system_id) && f.kind == FleetKind::Convoy)
                    .filter(|f| factions_hostile(faction, f.faction))
                    .filter(|f| alive(f.id).iter().any(|s| role_of(s.faction, s.class) == ClassRole::Civilian))
                    .collect();
                if let Some(f) = targets.choose(rng) {
                    let haulers = alive(f.id).iter().filter(|s| role_of(s.faction, s.class) == ClassRole::Civilian).count();
                    return Some(job(
                        JobKind::Raid { fleet: f.id, system: f.system_id },
                        format!("Raid: {} convoy", faction_display_name(f.faction)),
                        format!("{} of {haulers} haulers in {}. Make sure none of them arrive.", a_fleet_of(f.faction, "convoy"), name_of(f.system_id)),
                        pay(400.0 + haulers as f32 * 150.0),
                    ));
                }
            }
            "escort" => {
                let convoys: Vec<_> = world
                    .sim
                    .fleets
                    .iter()
                    .filter(|f| f.system_id == world.here && f.kind == FleetKind::Convoy && f.faction == faction && f.route.len() >= 2)
                    .filter(|f| alive(f.id).iter().any(|s| role_of(s.faction, s.class) == ClassRole::Civilian))
                    .collect();
                if let Some(f) = convoys.choose(rng) {
                    return Some(job(
                        JobKind::Escort { fleet: f.id, system: f.system_id, stop: f.destination, kept_close: false },
                        "Escort: our convoy".to_string(),
                        format!("One of our convoys is running through {}. Stay within 15 km of it until it makes its next stop.", name_of(f.system_id)),
                        pay(350.0 + alive(f.id).len() as f32 * 60.0),
                    ));
                }
            }
            "hold" => {
                let contested: Vec<(u32, f32)> = near
                    .iter()
                    .filter(|id| world.galaxy.systems.iter().any(|s| s.id == **id && s.faction == Some(faction)))
                    .filter_map(|id| world.war.contest(*id).map(|c| (*id, c.claimed())))
                    .filter(|(_, claimed)| *claimed >= 5.0)
                    .collect();
                if let Some(&(system, _)) = contested.choose(rng) {
                    let needed = rng.gen_range(3..=6);
                    return Some(job(
                        JobKind::HoldTheLine { system, kills: 0, needed, left: HOLD_SECONDS },
                        format!("Hold the line at {}", name_of(system)),
                        format!("{} is slipping. Destroy {needed} of the ships fighting us for it within fifteen minutes.", name_of(system)),
                        pay(500.0 + needed as f32 * 90.0),
                    ));
                }
            }
            "delivery" => {
                let others: Vec<u32> = world
                    .galaxy
                    .systems
                    .iter()
                    .filter(|s| s.faction == Some(faction) && s.id != world.here)
                    .map(|s| s.id)
                    .collect();
                if let Some(&to) = others.choose(rng) {
                    let item = cargo_of(faction);
                    let count = rng.gen_range(2..=4);
                    return Some(job(
                        JobKind::Delivery { to_system: to, item, count },
                        format!("Delivery to {}", name_of(to)),
                        format!("Carry {}, sealed, to our station at {}. Don't sell them.", cargo_count(item, count), name_of(to)),
                        pay(250.0 + count as f32 * 60.0),
                    ));
                }
            }
            "survey" => {
                let elsewhere: Vec<u32> = near.iter().copied().filter(|id| *id != world.here).collect();
                if let Some(&system) = elsewhere.choose(rng) {
                    return Some(job(
                        JobKind::Survey { system, seconds: 0.0 },
                        format!("Survey {}", name_of(system)),
                        format!("Take a minute in {} and tell us what's moving there.", name_of(system)),
                        pay(300.0),
                    ));
                }
            }
            _ => {}
        }
    }
    None
}

/// What finishing (or failing) moves.
const DONE_REGARD: f32 = 8.0;
const DONE_STANDING: f32 = 3.0;
const FAILED_REGARD: f32 = -5.0;
/// Border control a won fight buys the giver.
const DONE_PUSH: f32 = 6.0;

/// Keep every job in hand up to date, and settle the ones that are done or
/// lost.
#[allow(clippy::too_many_arguments)]
pub fn track_jobs(
    time: Res<Time>,
    mut jobs: ResMut<Jobs>,
    world: (Res<WorldSimulation>, ResMut<GalaxyMap>, ResMut<BorderWar>, Res<SystemStreamingManager>),
    mut contacts: ResMut<Contacts>,
    mut standing: ResMut<FactionStanding>,
    mut currency: ResMut<Currency>,
    player: Query<(Entity, &Transform), With<crate::components::Ship>>,
    mut deaths: MessageReader<crate::events::AiShipDestroyed>,
    victims: Query<&crate::ai_ship::components::AiShipState>,
    mut notes: MessageWriter<crate::events::ShowNotification>,
) {
    let (sim, mut galaxy, mut war, streaming) = world;
    let dt = time.delta_secs();
    let here = streaming.loaded_system;
    let you = player.single().ok();
    // Kills you made this frame, by faction, in the system you're in.
    let kills: Vec<AiShipType> = deaths
        .read()
        .filter(|d| victims.get(d.entity).is_ok_and(|s| you.is_some_and(|(p, _)| s.last_attacker == Some(p))))
        .map(|d| d.ship_type)
        .collect();
    let alive = |fleet: u32| sim.ships.iter().filter(|s| s.fleet == fleet && s.behavior != SimBehavior::Dead).collect::<Vec<_>>();

    for job in jobs.list.iter_mut().filter(|j| j.status == JobStatus::Active) {
        let outcome: Option<bool> = match &mut job.kind {
            // The target has to go while you're there: a fleet someone else
            // finished off-screen isn't your kill.
            JobKind::Bounty { fleet, system } => {
                if sim.fleet(*fleet).is_none() {
                    Some(false)
                } else if alive(*fleet).is_empty() {
                    Some(here == Some(*system))
                } else {
                    None
                }
            }
            JobKind::Raid { fleet, system } => {
                let haulers = alive(*fleet).iter().filter(|s| role_of(s.faction, s.class) == ClassRole::Civilian).count();
                if sim.fleet(*fleet).is_none() {
                    Some(false)
                } else if haulers == 0 {
                    Some(here == Some(*system))
                } else {
                    None
                }
            }
            JobKind::Escort { fleet, stop, kept_close, .. } => {
                let m = alive(*fleet);
                let haulers = m.iter().filter(|s| role_of(s.faction, s.class) == ClassRole::Civilian).count();
                if let Some((_, tf)) = you {
                    if m.iter().any(|s| s.position.distance(tf.translation.truncate()) < ESCORT_RANGE) {
                        *kept_close = true;
                    }
                }
                if haulers == 0 || sim.fleet(*fleet).is_none() {
                    Some(false)
                } else if *kept_close && m.iter().any(|s| s.position.distance(*stop) < ARRIVED) {
                    Some(true)
                } else {
                    None
                }
            }
            JobKind::HoldTheLine { system, kills: k, needed, left } => {
                if here == Some(*system) {
                    *k += kills.iter().filter(|v| factions_hostile(job.faction, **v)).count() as u32;
                }
                *left -= dt;
                if *k >= *needed { Some(true) } else if *left <= 0.0 { Some(false) } else { None }
            }
            JobKind::Survey { system, seconds } => {
                if here == Some(*system) {
                    *seconds += dt;
                }
                (*seconds >= SURVEY_SECONDS).then_some(true)
            }
            // Settled at the dock (deliver_cargo).
            JobKind::Delivery { .. } => None,
        };
        let Some(won) = outcome else { continue };
        settle(job, won, &mut contacts, &mut standing, &mut currency, &mut galaxy, &mut war, &mut notes);
    }
    // Settled jobs leave the list; a contact offers new work next time.
    jobs.list.retain(|j| matches!(j.status, JobStatus::Offered | JobStatus::Active));
}

/// Pay out, or not, and say so.
#[allow(clippy::too_many_arguments)]
fn settle(
    job: &mut Job,
    won: bool,
    contacts: &mut Contacts,
    standing: &mut FactionStanding,
    currency: &mut Currency,
    galaxy: &mut GalaxyMap,
    war: &mut BorderWar,
    notes: &mut MessageWriter<crate::events::ShowNotification>,
) {
    let who = super::contacts::all().iter().find(|c| c.id == job.giver).map_or("Your contact".to_string(), |c| c.name.clone());
    info!("JOB {} for {who}: {} ({} cr)", if won { "done" } else { "failed" }, job.title, job.reward);
    if won {
        job.status = JobStatus::Done;
        currency.credits += job.reward;
        let s = contacts.state_mut(&job.giver);
        s.relationship = (s.relationship + DONE_REGARD).min(100.0);
        standing.change(job.faction, DONE_STANDING, format!("You did a job for {who}"));
        let fought = match job.kind {
            JobKind::Bounty { system, .. } | JobKind::Raid { system, .. } | JobKind::HoldTheLine { system, .. } => Some(system),
            _ => None,
        };
        if let Some(system) = fought {
            war.push(galaxy, system, job.faction, DONE_PUSH);
        }
        notes.write(crate::events::ShowNotification {
            message: format!("{} done for {who}: +{} credits", job.title, job.reward),
            notification_type: crate::events::NotificationType::Success,
            duration: 5.0,
        });
    } else {
        job.status = JobStatus::Failed;
        let s = contacts.state_mut(&job.giver);
        s.relationship = (s.relationship + FAILED_REGARD).max(0.0);
        notes.write(crate::events::ShowNotification {
            message: format!("{} failed. {who} won't forget it.", job.title),
            notification_type: crate::events::NotificationType::Warning,
            duration: 5.0,
        });
    }
}

/// Docked at one of the giver's stations, in the system the cargo was
/// meant for: hand it over.
#[allow(clippy::too_many_arguments)]
pub fn deliver_cargo(
    mut jobs: ResMut<Jobs>,
    market: Res<crate::resources::MarketEvents>,
    streaming: Res<SystemStreamingManager>,
    mut inventory: ResMut<crate::resources::Inventory>,
    mut contacts: ResMut<Contacts>,
    mut standing: ResMut<FactionStanding>,
    mut currency: ResMut<Currency>,
    world: (ResMut<GalaxyMap>, ResMut<BorderWar>),
    mut notes: MessageWriter<crate::events::ShowNotification>,
) {
    let (mut galaxy, mut war) = world;
    let Some(faction) = market.dock_terms.as_ref().and_then(|t| t.faction) else { return };
    let Some(here) = streaming.loaded_system else { return };
    for job in jobs.list.iter_mut().filter(|j| j.status == JobStatus::Active && j.faction == faction) {
        let JobKind::Delivery { to_system, item, count } = job.kind else { continue };
        if to_system != here {
            continue;
        }
        if inventory.items.get(&item).copied().unwrap_or(0) < count {
            notes.write(crate::events::ShowNotification {
                message: format!("{}: the cargo isn't aboard any more.", job.title),
                notification_type: crate::events::NotificationType::Warning,
                duration: 4.0,
            });
            settle(job, false, &mut contacts, &mut standing, &mut currency, &mut galaxy, &mut war, &mut notes);
            continue;
        }
        inventory.remove_item(item, count);
        settle(job, true, &mut contacts, &mut standing, &mut currency, &mut galaxy, &mut war, &mut notes);
    }
    jobs.list.retain(|j| matches!(j.status, JobStatus::Offered | JobStatus::Active));
}

/// Finishing a job on the old contract board counts with the real faction
/// behind it: the Navy's work is the Hegemony's, the Salvage Guild's the
/// Compact's, the Research Institute's the Preserve's.
pub fn standing_from_contracts(
    mut done: MessageReader<crate::events::ContractTurnedIn>,
    mut standing: ResMut<FactionStanding>,
) {
    for c in done.read() {
        standing.change(c.faction.real(), 2.0, format!("You finished a contract for them ({})", c.faction.name()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_at_hegemony_capital() -> (WorldSimulation, GalaxyMap, u32) {
        let galaxy = crate::celestial::galaxy::generate_galaxy_map(42);
        let here = galaxy.systems.iter().find(|s| s.capital && s.faction == Some(AiShipType::TerranHegemony)).unwrap().id;
        let mut sim = WorldSimulation::default();
        crate::ai_ship::simulation::ensure_system_population(&mut sim, &galaxy, here);
        for n in crate::celestial::galaxy::nearest_neighbors(&galaxy, here, 3) {
            crate::ai_ship::simulation::ensure_system_population(&mut sim, &galaxy, n);
        }
        (sim, galaxy, here)
    }

    #[test]
    fn every_role_gets_work_and_it_points_at_something_real() {
        let (sim, galaxy, here) = world_at_hegemony_capital();
        let war = BorderWar::default();
        let world = World { sim: &sim, galaxy: &galaxy, war: &war, here };
        let mut rng = rand::thread_rng();
        let hegemony: Vec<_> = super::super::contacts::all().iter().filter(|c| c.faction == AiShipType::TerranHegemony).collect();
        for def in hegemony {
            let job = (0..10).find_map(|_| offer_for(def, 0.0, &world, &mut rng));
            let job = job.unwrap_or_else(|| panic!("{} had nothing to offer", def.name));
            assert!(job.reward > 0);
            match job.kind {
                JobKind::Bounty { fleet, .. } | JobKind::Raid { fleet, .. } | JobKind::Escort { fleet, .. } => {
                    let f = sim.fleet(fleet).expect("the job's fleet exists");
                    if !matches!(job.kind, JobKind::Escort { .. }) {
                        assert!(factions_hostile(AiShipType::TerranHegemony, f.faction), "a job against a friend");
                    }
                }
                JobKind::Delivery { to_system, .. } => assert_ne!(to_system, here),
                _ => {}
            }
        }
    }

    #[test]
    fn three_jobs_at_a_time() {
        let mut jobs = Jobs::default();
        for i in 0..4 {
            jobs.push(Job {
                id: 0, giver: format!("c{i}"), faction: AiShipType::TerranHegemony,
                kind: JobKind::Survey { system: 1, seconds: 0.0 },
                title: "t".into(), brief: "b".into(), reward: 10, status: JobStatus::Offered,
            });
        }
        for i in 0..3 {
            assert!(jobs.accept(&format!("c{i}"), |_, _| true).is_ok());
        }
        assert!(jobs.accept("c3", |_, _| true).is_err());
        jobs.drop_offers();
        assert_eq!(jobs.list.len(), 3, "the unaccepted offer lapses");
    }

    fn app_with(jobs: Vec<Job>, here: u32) -> App {
        let galaxy = crate::celestial::galaxy::generate_galaxy_map(42);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<crate::events::AiShipDestroyed>()
            .add_message::<crate::events::ShowNotification>()
            .init_resource::<WorldSimulation>()
            .insert_resource(galaxy)
            .init_resource::<BorderWar>()
            .insert_resource(SystemStreamingManager { loaded_system: Some(here), ..default() })
            .init_resource::<Contacts>()
            .init_resource::<FactionStanding>()
            .init_resource::<Currency>()
            .init_resource::<crate::resources::Inventory>()
            .insert_resource(Jobs { list: jobs, next_id: 1 });
        app
    }

    fn active(kind: JobKind) -> Job {
        Job {
            id: 1, giver: "TerranHegemony_captain_idris_vale".into(), faction: AiShipType::TerranHegemony, kind,
            title: "t".into(), brief: "b".into(), reward: 100, status: JobStatus::Active,
        }
    }

    #[test]
    fn a_survey_counts_only_time_spent_there() {
        let mut app = app_with(vec![active(JobKind::Survey { system: 5, seconds: SURVEY_SECONDS - 0.5 })], 4);
        app.add_systems(Update, track_jobs);
        app.update();
        app.update();
        assert_eq!(app.world().resource::<Jobs>().active().count(), 1, "surveying from the wrong system");
        app.world_mut().resource_mut::<SystemStreamingManager>().loaded_system = Some(5);
        let JobKind::Survey { seconds, .. } = &mut app.world_mut().resource_mut::<Jobs>().list[0].kind else { unreachable!() };
        *seconds = SURVEY_SECONDS;
        app.update();
        assert_eq!(app.world().resource::<Currency>().credits, Currency::default().credits + 100);
    }

    #[test]
    fn holding_the_line_runs_out_of_time() {
        let mut app = app_with(vec![active(JobKind::HoldTheLine { system: 5, kills: 1, needed: 4, left: 0.0 })], 5);
        app.add_systems(Update, track_jobs);
        app.update();
        let regard = app.world().resource::<Contacts>().relationship("TerranHegemony_captain_idris_vale");
        assert!(app.world().resource::<Jobs>().list.is_empty());
        assert_eq!(regard, 0.0, "regard can't go below nothing");
        assert_eq!(app.world().resource::<Currency>().credits, Currency::default().credits);
    }

    #[test]
    fn cargo_is_handed_over_at_the_right_station() {
        let job = active(JobKind::Delivery { to_system: 7, item: ItemType::AmmoCrate, count: 3 });
        let mut app = app_with(vec![job], 7);
        app.world_mut().resource_mut::<crate::resources::Inventory>().add_item(ItemType::AmmoCrate, 3);
        let mut market = crate::resources::MarketEvents::default();
        market.dock_terms = Some(crate::resources::DockTerms {
            faction: Some(AiShipType::TerranHegemony), name: "x".into(), standing: "Neutral", buy: 1.0, sell: 1.0,
        });
        app.insert_resource(market).add_systems(Update, deliver_cargo);
        app.update();
        assert!(app.world().resource::<Jobs>().list.is_empty());
        assert!(!app.world().resource::<crate::resources::Inventory>().items.contains_key(&ItemType::AmmoCrate));
        assert_eq!(app.world().resource::<Currency>().credits, Currency::default().credits + 100);
    }

    #[test]
    fn a_bounty_pays_when_the_fleet_is_gone() {
        let (mut sim, galaxy, here) = world_at_hegemony_capital();
        let war = BorderWar::default();
        let world = World { sim: &sim, galaxy: &galaxy, war: &war, here };
        let mut rng = rand::thread_rng();
        let captain = super::super::contacts::all().iter().find(|c| c.name == "Captain Idris Vale").unwrap();
        let job = (0..40).find_map(|_| offer_for(captain, 0.0, &world, &mut rng).filter(|j| matches!(j.kind, JobKind::Bounty { .. })));
        let mut job = job.expect("the Hegemony capital has enemies in reach");
        let JobKind::Bounty { fleet, system } = job.kind else { unreachable!() };
        job.status = JobStatus::Active;
        for s in sim.ships.iter_mut().filter(|s| s.fleet == fleet) {
            s.behavior = SimBehavior::Dead;
        }

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<crate::events::AiShipDestroyed>()
            .add_message::<crate::events::ShowNotification>()
            .insert_resource(sim)
            .insert_resource(galaxy)
            .init_resource::<BorderWar>()
            .insert_resource(SystemStreamingManager { loaded_system: Some(system), ..default() })
            .init_resource::<Contacts>()
            .init_resource::<FactionStanding>()
            .init_resource::<Currency>()
            .insert_resource(Jobs { list: vec![job.clone()], next_id: 1 })
            .add_systems(Update, track_jobs);
        let before = app.world().resource::<Currency>().credits;
        app.update();
        assert_eq!(app.world().resource::<Currency>().credits, before + job.reward);
        assert!(app.world().resource::<Jobs>().list.is_empty(), "a settled job leaves the list");
        assert!(app.world().resource::<Contacts>().relationship(&captain.id) > 0.0);
    }
}
