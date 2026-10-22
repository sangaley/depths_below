//! The station window's Bar and Comm directory tabs (ui::spawn_docking_menu
//! builds the window and its Market).
//!
//! - **Bar:** one to three people with something to say, and what they say is
//!   true: drawn from what the simulation is doing right now -- a battle
//!   group nearby, a convoy running through, a border slipping, a hunter
//!   wing someone put on you.
//! - **Comm directory:** the contacts at this starport (contacts.rs): pick one,
//!   talk, and they get to know you.

use bevy::prelude::*;
use rand::{seq::SliceRandom, Rng, SeedableRng};

use super::contacts::{self, Contacts};
use super::FactionStanding;
use crate::ai_ship::components::{faction_display_name, AiShipType, SimBehavior, WorldSimulation};
use crate::ai_ship::fleet::{FleetKind, Mission};
use crate::celestial::resources::{GalaxyMap, SystemDiscovery, SystemStreamingManager};
use crate::celestial::territory::{border_factions, BorderWar};
use crate::components::*;
use crate::resources::MarketEvents;
use crate::ui::theme::{ThemeColors, ThemeFonts};

/// Q / E switch tabs; in the Comm directory Up / Down pick a contact and
/// Enter talks to them.
pub fn dock_tab_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut windows: Query<&mut DockTabState, With<DockingOverlay>>,
    market: Res<MarketEvents>,
    galaxy: Res<GalaxyMap>,
    streaming: Res<SystemStreamingManager>,
    mut contacts: ResMut<Contacts>,
) {
    let Ok(mut state) = windows.single_mut() else { return };
    if !state.tabs {
        return;
    }
    let i = DockTab::ALL.iter().position(|t| *t == state.tab).unwrap_or(0);
    if keyboard.just_pressed(KeyCode::KeyE) {
        state.tab = DockTab::ALL[(i + 1) % DockTab::ALL.len()];
    }
    if keyboard.just_pressed(KeyCode::KeyQ) {
        state.tab = DockTab::ALL[(i + DockTab::ALL.len() - 1) % DockTab::ALL.len()];
    }
    if state.tab != DockTab::Comm {
        return;
    }
    let (Some(faction), Some(system)) = (market.dock_terms.as_ref().and_then(|t| t.faction), streaming.loaded_system) else {
        return;
    };
    let here = contacts::present_at(&galaxy, &contacts, system, faction);
    if here.is_empty() {
        return;
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) {
        state.contact = (state.contact + 1) % here.len();
    }
    if keyboard.just_pressed(KeyCode::ArrowUp) {
        state.contact = (state.contact + here.len() - 1) % here.len();
    }
    state.contact = state.contact.min(here.len() - 1);
    if keyboard.just_pressed(KeyCode::Enter) {
        contacts.talk(here[state.contact]);
        // Change-detection on the state is what redraws the panel.
        state.set_changed();
    }
}

/// Redraw the tabs when the window opens or its state changes: which panel
/// shows, which tab button is lit, the key hints, and the Bar and Comm
/// directory's contents.
#[allow(clippy::too_many_arguments)]
pub fn refresh_dock_tabs(
    mut commands: Commands,
    windows: Query<&DockTabState, (With<DockingOverlay>, Changed<DockTabState>)>,
    mut panels: Query<(Entity, &DockTabPanel, &mut Node)>,
    mut buttons: Query<(&DockTabButton, &mut BackgroundColor, &mut BorderColor, &Children)>,
    mut button_text: Query<&mut TextColor>,
    mut footer: Query<&mut Text, With<DockFooterHint>>,
    world: (Res<WorldSimulation>, Res<GalaxyMap>, Res<BorderWar>, Res<FactionStanding>, Res<SystemStreamingManager>, Res<Time>),
    market: Res<MarketEvents>,
    contacts: Res<Contacts>,
) {
    let Ok(state) = windows.single() else { return };
    let (sim, galaxy, war, standing, streaming, time) = world;
    for (entity, panel, mut node) in panels.iter_mut() {
        let show = panel.0 == state.tab;
        node.display = if show { Display::Flex } else { Display::None };
        if show && panel.0 != DockTab::Market {
            commands.entity(entity).despawn_related::<Children>();
            let owner = market.dock_terms.as_ref().and_then(|t| t.faction);
            match panel.0 {
                DockTab::Bar => fill_bar(&mut commands, entity, &sim, &galaxy, &war, &standing, &streaming, owner, time.elapsed_secs()),
                DockTab::Comm => fill_comm(&mut commands, entity, &galaxy, &contacts, &streaming, owner, state.contact),
                DockTab::Market => {}
            }
        }
    }
    for (button, mut bg, mut border, children) in buttons.iter_mut() {
        let on = button.0 == state.tab;
        bg.0 = if on { ThemeColors::BG_ELEVATED } else { ThemeColors::BG_CARD };
        *border = BorderColor::all(if on { ThemeColors::ACCENT_CYAN } else { Color::NONE });
        for child in children.iter() {
            if let Ok(mut color) = button_text.get_mut(child) {
                color.0 = if on { ThemeColors::TEXT_PRIMARY } else { ThemeColors::TEXT_MUTED };
            }
        }
    }
    if let Ok(mut text) = footer.single_mut() {
        let want = match state.tab {
            DockTab::Market => "Up/Down  select    Left/Right  cargo choice    Enter  buy    Esc  undock",
            DockTab::Bar => "Q / E  switch tab    Esc  undock",
            DockTab::Comm => "Up/Down  pick a contact    Enter  talk    Q / E  switch tab    Esc  undock",
        };
        if text.0 != want {
            text.0 = want.to_string();
        }
    }
}

/// A line of text in the window's style.
fn line(text: impl Into<String>, size: f32, color: Color) -> (Text, TextFont, TextColor) {
    (Text::new(text.into()), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(color))
}

fn card() -> (Node, BackgroundColor) {
    (
        Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)), ..default() },
        BackgroundColor(ThemeColors::BG_CARD),
    )
}

/// Who's at the bar, by the station's owner.
fn patrons(owner: Option<AiShipType>) -> &'static [&'static str] {
    use AiShipType::*;
    match owner {
        Some(TerranHegemony) => &["An off-duty rating", "A dock foreman", "A pilot in a Hegemony jacket"],
        Some(GildedThrone) => &["A junior auditor", "A bodyguard on her break", "A man counting his chips twice"],
        Some(MerchantPrinces) => &["A pedlar between contracts", "A Guild clerk", "Someone selling the table they sit at"],
        Some(FreeHaulersCompact) => &["A tug pilot", "A miner with a broken hand", "A union steward"],
        Some(TheSilence) => &["A pilgrim, writing instead of speaking", "A novice who whispers", "A visitor who stopped talking"],
        Some(BrokenChoir) => &["Three voices from one mouth", "A chorister, off key", "Somebody humming your name"],
        Some(SynthesisCollective) => &["A body the Collective is using today", "A graft-tender", "Two of them, talking as one"],
        Some(CorpseStars) => &["A procession hand", "An old cultist, older than he looks", "A mourner"],
        Some(StellarPreserve) => &["A keeper off shift", "A ranger with herd-dust on her boots", "A specimen handler"],
        Some(RecursiveKingdom) => &["Rex, aged thirty", "Rex, aged twelve", "Rex, aged sixty-one"],
        _ => &["A hauler", "A pilot", "A dock hand"],
    }
}

/// What the bar is talking about, drawn from what is actually happening:
/// one to three rumours, the same for a few minutes at a time.
pub fn rumours(
    sim: &WorldSimulation,
    galaxy: &GalaxyMap,
    war: &BorderWar,
    standing: &FactionStanding,
    system_id: u32,
    seed: u64,
) -> Vec<String> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let name_of = |id: u32| galaxy.systems.iter().find(|s| s.id == id).map_or("somewhere".to_string(), |s| s.name.clone());
    let mut near: Vec<u32> = vec![system_id];
    near.extend(crate::celestial::galaxy::nearest_neighbors(galaxy, system_id, 3));
    let mut said: Vec<String> = Vec::new();
    let members = |fleet: u32| sim.ships.iter().filter(|s| s.fleet == fleet && s.behavior != SimBehavior::Dead).collect::<Vec<_>>();

    // A battle group nearby.
    if let Some(f) = sim.fleets.iter().filter(|f| f.kind == FleetKind::BattleGroup && near.contains(&f.system_id) && f.mission == Mission::None).collect::<Vec<_>>().choose(&mut rng) {
        let m = members(f.id);
        if let Some(lead) = m.first() {
            let class = lead.class.and_then(|k| crate::ai_ship::roster::find(lead.faction, k)).map_or("capital ship", |c| c.name);
            let at = if f.system_id == system_id { "here, in this system".to_string() } else { format!("in {}", name_of(f.system_id)) };
            said.push(format!("A {} battle group is {at}: {} hulls behind a {class}.", faction_display_name(f.faction), m.len()));
        }
    }
    // A convoy running through.
    if let Some(f) = sim.fleets.iter().filter(|f| f.kind == FleetKind::Convoy && f.system_id == system_id).collect::<Vec<_>>().choose(&mut rng) {
        let m = members(f.id);
        let civilian = |s: &&&crate::ai_ship::components::SimulatedShip| {
            crate::ai_ship::fleet::role_of(s.faction, s.class) == crate::ai_ship::roster::ClassRole::Civilian
        };
        let haulers = m.iter().filter(civilian).count();
        let guards = m.len() - haulers;
        let guard = match m.iter().find(|s| s.faction != f.faction) {
            Some(g) => format!(", with {guards} {} guns riding along", faction_display_name(g.faction)),
            None if guards > 0 => format!(", {guards} guarding"),
            None => ", and nobody guarding it".to_string(),
        };
        said.push(format!("A {} convoy is running through here: {haulers} haulers{guard}.", faction_display_name(f.faction)));
    }
    // A border slipping.
    let slipping: Vec<(u32, AiShipType, f32)> = near
        .iter()
        .filter_map(|&id| war.contest(id))
        .flat_map(|c| c.claims.iter().filter(|(_, s)| *s >= 25.0).map(move |&(f, s)| (c.system_id, f, s)))
        .collect();
    if let Some(&(id, f, share)) = slipping.choose(&mut rng) {
        said.push(format!("{} is slipping. The {} hold {share:.0}% of it now.", name_of(id), faction_display_name(f)));
    }
    // Someone after you.
    if let Some(f) = sim.fleets.iter().filter(|f| matches!(f.mission, Mission::Hunt { .. }) && f.system_id == system_id).collect::<Vec<_>>().choose(&mut rng) {
        said.push(format!(
            "Someone's sent a {} wing after a ship like yours. {} hulls. I'd leave by a different door.",
            faction_display_name(f.faction),
            members(f.id).len()
        ));
    }
    // The news.
    if let Some((id, _, to)) = war.news.iter().rev().find(|(id, ..)| galaxy.systems.iter().any(|s| s.id == *id && s.discovery != SystemDiscovery::Unknown)) {
        said.push(format!("Did you hear? {} fell to the {}.", name_of(*id), faction_display_name(*to)));
    }
    // Who wants you dead.
    let haters: Vec<AiShipType> = border_factions(galaxy, system_id).into_iter().filter(|f| standing.attacks_player(*f)).collect();
    if let Some(f) = haters.choose(&mut rng) {
        if let Some(their) = galaxy.systems.iter().find(|s| s.faction == Some(*f) && near.contains(&s.id)) {
            said.push(format!("The {} count you hostile, you know. Their patrols cross {}.", faction_display_name(*f), their.name));
        }
    }
    // Somewhere nobody goes -- or, here.
    if let Some(h) = galaxy.systems.iter().find(|s| s.haunt.is_some() && near.contains(&s.id)) {
        let boss = faction_display_name(h.haunt.unwrap());
        said.push(if h.id == system_id {
            format!("Nobody leaves the dock here if they can help it. The {boss} is out there.")
        } else {
            format!("Nobody goes to {}. The {boss} is there.", h.name)
        });
    }
    said.shuffle(&mut rng);
    let keep = rng.gen_range(1..=3);
    said.truncate(keep);
    if said.is_empty() {
        said.push("A quiet night. Nobody here has anything worth selling you.".into());
    }
    said
}

#[allow(clippy::too_many_arguments)]
fn fill_bar(
    commands: &mut Commands,
    panel: Entity,
    sim: &WorldSimulation,
    galaxy: &GalaxyMap,
    war: &BorderWar,
    standing: &FactionStanding,
    streaming: &SystemStreamingManager,
    owner: Option<AiShipType>,
    now: f32,
) {
    let system = streaming.loaded_system.unwrap_or(0);
    // The same talk for five minutes at a stretch, not a new crowd every tab.
    let seed = system as u64 * 1_000_003 + (now / 300.0) as u64;
    let said = rumours(sim, galaxy, war, standing, system, seed);
    let who = patrons(owner);
    commands.entity(panel).with_children(|p| {
        p.spawn(line("WHO'S TALKING", ThemeFonts::CAPTION, ThemeColors::TEXT_MUTED));
        for (i, rumour) in said.into_iter().enumerate() {
            p.spawn(card()).with_children(|c| {
                c.spawn(line(who[i % who.len()], ThemeFonts::CAPTION, ThemeColors::TEXT_MUTED));
                c.spawn(line(format!("\"{rumour}\""), 16.0, ThemeColors::TEXT_PRIMARY));
            });
        }
    });
}

fn fill_comm(
    commands: &mut Commands,
    panel: Entity,
    galaxy: &GalaxyMap,
    contacts: &Contacts,
    streaming: &SystemStreamingManager,
    owner: Option<AiShipType>,
    selected: usize,
) {
    let here = match (owner, streaming.loaded_system) {
        (Some(f), Some(system)) => contacts::present_at(galaxy, contacts, system, f),
        _ => Vec::new(),
    };
    commands.entity(panel).with_children(|p| {
        if here.is_empty() {
            p.spawn(line("No one here takes calls.", ThemeFonts::BODY, ThemeColors::TEXT_MUTED));
            return;
        }
        p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(12.0), ..default() }).with_children(|row| {
            // Who's here.
            row.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(5.0), width: Val::Px(270.0), flex_shrink: 0.0, ..default() })
                .with_children(|list| {
                    list.spawn(line("CONTACTS", ThemeFonts::CAPTION, ThemeColors::TEXT_MUTED));
                    for (i, def) in here.iter().enumerate() {
                        let on = i == selected.min(here.len() - 1);
                        let rel = contacts.relationship(&def.id);
                        let met = contacts.state(&def.id).is_some_and(|s| s.met);
                        list.spawn((
                            Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)), border: UiRect::left(Val::Px(3.0)), ..default() },
                            BackgroundColor(if on { ThemeColors::BG_ELEVATED } else { ThemeColors::BG_CARD }),
                            BorderColor::all(if on { ThemeColors::ACCENT_CYAN } else { Color::NONE }),
                        )).with_children(|c| {
                            let star = if def.story { "  *" } else { "" };
                            c.spawn(line(format!("{}{star}", def.name), 15.0, if on { Color::WHITE } else { ThemeColors::TEXT_PRIMARY }));
                            let known = if met { format!("{}  |  relationship {rel:.0}", def.role.label()) } else { format!("{}  |  not met", def.role.label()) };
                            c.spawn(line(known, ThemeFonts::CAPTION, ThemeColors::TEXT_SECONDARY));
                        });
                    }
                });
            // The one you're talking to.
            let def = here[selected.min(here.len() - 1)];
            let state = contacts.state(&def.id);
            let rel = state.map_or(0.0, |s| s.relationship);
            let said = contacts::line_for(def, rel, state.map_or(0, |s| s.talks)).to_string();
            row.spawn((
                Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), padding: UiRect::all(Val::Px(16.0)), flex_grow: 1.0, ..default() },
                BackgroundColor(ThemeColors::BG_CARD),
            )).with_children(|d| {
                d.spawn(line(def.name.clone(), ThemeFonts::H3, ThemeColors::TEXT_TITLE));
                d.spawn(line(format!("{}  |  {}", faction_display_name(def.faction), def.role.label()), ThemeFonts::BODY_SMALL, ThemeColors::TEXT_SECONDARY));
                // Relationship, as a bar.
                d.spawn((Node { width: Val::Percent(100.0), height: Val::Px(6.0), ..default() }, BackgroundColor(ThemeColors::BG_INPUT)))
                    .with_children(|bar| {
                        bar.spawn((Node { width: Val::Percent(rel.clamp(0.0, 100.0)), height: Val::Percent(100.0), ..default() }, BackgroundColor(ThemeColors::ACCENT_GREEN)));
                    });
                d.spawn((line(format!("\"{said}\""), 17.0, ThemeColors::TEXT_PRIMARY), Node { margin: UiRect::top(Val::Px(8.0)), ..default() }));
                if def.story {
                    d.spawn(line("*  Holds a piece of the summit record", ThemeFonts::CAPTION, ThemeColors::ACCENT_YELLOW));
                }
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_always_has_something_to_say_and_it_is_true() {
        let galaxy = crate::celestial::galaxy::generate_galaxy_map(42);
        let mut sim = WorldSimulation::default();
        let capital = galaxy.systems.iter().find(|s| s.capital && s.faction == Some(AiShipType::TerranHegemony)).unwrap();
        // An empty world: a quiet night.
        let quiet = rumours(&sim, &galaxy, &BorderWar::default(), &FactionStanding::default(), capital.id, 1);
        assert_eq!(quiet.len(), 1);
        // A populated one: real fleets in this system get talked about.
        crate::ai_ship::simulation::ensure_system_population(&mut sim, &galaxy, capital.id);
        let mut heard_fleet = false;
        for seed in 0..40 {
            let said = rumours(&sim, &galaxy, &BorderWar::default(), &FactionStanding::default(), capital.id, seed);
            assert!((1..=3).contains(&said.len()));
            heard_fleet |= said.iter().any(|s| s.contains("battle group") || s.contains("convoy"));
        }
        assert!(heard_fleet, "forty nights at the Hegemony's capital and nobody mentioned a fleet");
    }
}
