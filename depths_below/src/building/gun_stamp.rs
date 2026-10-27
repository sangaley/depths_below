//! Stamping a saved gun design (combat::gun_design) into the ship.
//!
//! Saved designs follow the built-in templates in build mode's T cycle.
//! With one picked, the ghost shows every part where it would go, turned to
//! the build rotation (R), and Enter lays them all down -- on the hull deck,
//! or on the turret under the cursor when building on the turret deck
//! (PgUp). The firing piece gets the design's shell.

use bevy::prelude::*;

use super::blueprint::ModuleExtras;
use super::turret_build::{allowed_on_turret, deck_under, may_overhang, on_deck, player_decks, turret_cells, DeckUnder};
use super::{GridOccupancy, ModuleRegistry};
use crate::combat::gun_design::{GunDesign, GunDesigns};
use crate::combat::turret_deck::TurretBody;
use crate::components::{DestroyedModule, Module, ModuleType, Rotation, Ship};
use crate::events::{NotificationType, PlaceModuleRequest, ShowNotification};
use crate::resources::{BuildDeck, BuildingState, Currency};
use crate::states::BuildState;

/// One part of a stamp: where it goes in its deck's grid, and whether it can.
#[derive(Clone, Debug, PartialEq)]
pub struct StampPart {
    pub module_type: ModuleType,
    /// In the hull's grid, or the turret's for a turret stamp.
    pub cell: IVec2,
    /// The hull cell it stands over (the same as `cell` on the hull deck).
    pub hull_cell: IVec2,
    pub rotation: Rotation,
    pub ok: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StampPlan {
    pub parts: Vec<StampPart>,
    /// The turret it goes on, if any.
    pub turret: Option<IVec2>,
    /// Why it can't go down, if it can't.
    pub problem: Option<String>,
}

/// Where a design would go with its firing piece at `origin` (a hull cell),
/// facing `facing`. Pure: the world is passed in as what's taken.
pub fn stamp_plan(
    design: &GunDesign,
    origin: IVec2,
    facing: Rotation,
    deck: Option<DeckUnder>,
    hull_taken: &dyn Fn(IVec2) -> bool,
    turret_taken: &[IVec2],
) -> StampPlan {
    let placed = design.placed(origin, facing);
    let mut problem = None;
    let parts: Vec<StampPart> = placed
        .iter()
        .map(|&(module_type, hull_cell, rotation)| match deck {
            Some(d) => {
                let local = hull_cell - d.centre;
                let fits = allowed_on_turret(module_type);
                if !fits && problem.is_none() {
                    problem = Some(format!("{} can't go on a turret", module_type.name()));
                }
                let ok = fits && (may_overhang(module_type) || on_deck(local, d.size)) && !turret_taken.contains(&local);
                StampPart { module_type, cell: local, hull_cell, rotation, ok }
            }
            None => StampPart { module_type, cell: hull_cell, hull_cell, rotation, ok: !hull_taken(hull_cell) },
        })
        .collect();
    if problem.is_none() && parts.iter().any(|p| !p.ok) {
        problem = Some(match deck {
            Some(_) => "Doesn't fit on this turret".into(),
            None => "Overlaps something already there".into(),
        });
    }
    if problem.is_none() && deck.is_none() {
        // Like any block, it has to touch the ship.
        let mine: Vec<IVec2> = parts.iter().map(|p| p.cell).collect();
        let touches = mine.iter().any(|c| {
            [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y]
                .iter()
                .any(|d| !mine.contains(&(*c + *d)) && hull_taken(*c + *d))
        });
        if !touches {
            problem = Some("Must touch the ship".into());
        }
    }
    StampPlan { parts, turret: deck.map(|d| d.ring_cell), problem }
}

/// The design picked in the T cycle, if a saved gun design is what's picked.
pub fn picked_design<'a>(selected: Option<usize>, designs: &'a GunDesigns) -> Option<&'a GunDesign> {
    let builtins = super::multiblock::build_helpers::weapon_templates().len();
    selected.and_then(|i| i.checked_sub(builtins)).and_then(|i| designs.0.get(i))
}

#[derive(bevy::ecs::system::SystemParam)]
pub struct StampWorld<'w, 's> {
    build_state: Res<'w, BuildingState>,
    current: Res<'w, State<BuildState>>,
    templates: Res<'w, super::templates::TemplateState>,
    designs: Res<'w, GunDesigns>,
    occupancy: Res<'w, GridOccupancy>,
    ship: Query<'w, 's, Entity, With<Ship>>,
    bodies: Query<'w, 's, (Entity, &'static TurretBody, &'static ChildOf, Option<&'static Children>)>,
    rings: Query<'w, 's, &'static Module, Without<DestroyedModule>>,
}

impl StampWorld<'_, '_> {
    /// The picked design's plan at the ghost, if one is picked and building.
    fn plan(&self) -> Option<(&GunDesign, StampPlan)> {
        if *self.current.get() != BuildState::Placing || !self.build_state.cursor_on_grid {
            return None;
        }
        let design = picked_design(self.templates.selected_index, &self.designs)?;
        let origin = self.build_state.ghost_position;
        let (deck, taken) = if self.build_state.deck == BuildDeck::Turret {
            let player = self.ship.single().ok()?;
            let decks = player_decks(player, &self.bodies, &self.rings);
            let deck = deck_under(origin, &decks);
            let taken = deck.map(|d| turret_cells(d.body, &self.bodies, &self.rings)).unwrap_or_default();
            if deck.is_none() {
                let plan = StampPlan { parts: vec![], turret: None, problem: Some("Not over a turret".into()) };
                return Some((design, plan));
            }
            (deck, taken)
        } else {
            (None, Vec::new())
        };
        let plan = stamp_plan(design, origin, self.build_state.rotation, deck, &|c| self.occupancy.cells.contains_key(&c), &taken);
        Some((design, plan))
    }
}

/// Enter lays the picked design down.
pub fn stamp_gun_design(
    keyboard: Res<ButtonInput<KeyCode>>,
    world: StampWorld,
    registry: Res<ModuleRegistry>,
    currency: Res<Currency>,
    mut place: MessageWriter<PlaceModuleRequest>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }
    let Some((design, plan)) = world.plan() else { return };
    let cost = design.cost(&registry);
    let problem = plan.problem.clone().or_else(|| (currency.credits < cost).then(|| format!("Not enough credits (need {cost}c)")));
    if let Some(problem) = problem {
        notifications.write(ShowNotification { message: format!("Can't place {}: {}", design.name, problem), notification_type: NotificationType::Warning, duration: 2.0 });
        return;
    }
    for part in &plan.parts {
        let extras = ModuleExtras {
            turret: plan.turret,
            shell: design.shell.clone().filter(|_| part.module_type == ModuleType::FiringPiece),
            ..default()
        };
        place.write(PlaceModuleRequest {
            module_type: part.module_type,
            grid_position: part.cell,
            rotation: part.rotation,
            custom_name: None,
            subcomponents: None,
            extras: (!extras.is_empty()).then_some(extras),
            free: false,
        });
    }
    info!("Stamped gun design '{}' ({} parts, {}c)", design.name, plan.parts.len(), cost);
    notifications.write(ShowNotification { message: format!("Placed {} ({}c)", design.name, cost), notification_type: NotificationType::Success, duration: 2.0 });
}

/// The ghost of the picked design: each part's own art where it would go,
/// red where it can't.
#[derive(Component)]
pub struct GunStampGhost;

pub fn gun_stamp_ghost(
    mut commands: Commands,
    world: StampWorld,
    old: Query<Entity, With<GunStampGhost>>,
    asset_server: Res<AssetServer>,
    mut shown: Local<Option<(String, Vec<(IVec2, Rotation, bool)>, bool)>>,
) {
    let plan = world.plan();
    let key = plan.as_ref().map(|(d, p)| {
        (d.name.clone(), p.parts.iter().map(|q| (q.hull_cell, q.rotation, q.ok)).collect::<Vec<_>>(), p.problem.is_none())
    });
    if key == *shown {
        return;
    }
    *shown = key;
    for e in old.iter() {
        commands.entity(e).despawn();
    }
    let (Some((_, plan)), Ok(ship)) = (plan, world.ship.single()) else { return };
    let z = if world.build_state.deck == BuildDeck::Turret { 2.6 } else { 0.95 };
    for part in &plan.parts {
        let tint = if part.ok && plan.problem.is_none() { Color::srgba(0.6, 1.0, 0.6, 0.6) } else { Color::srgba(1.0, 0.3, 0.3, 0.6) };
        let at = super::grid_to_local(part.hull_cell).extend(z);
        let angle = part.rotation.to_radians() + crate::sprite_map::sprite_base_rotation(part.module_type);
        commands.spawn((
            Sprite {
                image: crate::sprite_map::module_sprite_path(part.module_type).map(|p| asset_server.load(p)).unwrap_or_default(),
                color: tint,
                custom_size: Some(Vec2::splat(60.0)),
                ..default()
            },
            Transform::from_translation(at).with_rotation(Quat::from_rotation_z(angle)),
            GunStampGhost,
            ChildOf(ship),
        ));
    }
}

#[cfg(test)]
mod gun_stamp_tests {
    use super::*;
    use crate::combat::gun_design::GunDesignPart;

    fn design() -> GunDesign {
        let part = |t, x, y| GunDesignPart { module_type: t, offset: IVec2::new(x, y), rotation: Rotation::East };
        GunDesign {
            name: "t".into(),
            description: String::new(),
            parts: vec![
                part(ModuleType::FiringPiece, 0, 0),
                part(ModuleType::GunBarrel, 1, 0),
                part(ModuleType::Loader, -1, 0),
            ],
            shell: None,
        }
    }

    #[test]
    fn on_the_hull_it_needs_empty_cells_touching_the_ship() {
        let taken = |c: IVec2| c == IVec2::new(0, 1);
        let plan = stamp_plan(&design(), IVec2::ZERO, Rotation::East, None, &taken, &[]);
        assert_eq!(plan.problem, None);
        let blocked = |c: IVec2| c == IVec2::new(1, 0);
        assert!(stamp_plan(&design(), IVec2::ZERO, Rotation::East, None, &blocked, &[]).problem.is_some());
        let alone = |_c: IVec2| false;
        assert_eq!(stamp_plan(&design(), IVec2::ZERO, Rotation::East, None, &alone, &[]).problem.as_deref(), Some("Must touch the ship"));
    }

    #[test]
    fn turned_south_the_barrel_points_south() {
        let plan = stamp_plan(&design(), IVec2::new(5, 5), Rotation::South, None, &|c| c == IVec2::new(4, 5), &[]);
        let barrel = plan.parts.iter().find(|p| p.module_type == ModuleType::GunBarrel).unwrap();
        assert_eq!((barrel.cell, barrel.rotation), (IVec2::new(5, 4), Rotation::South));
    }

    #[test]
    fn on_a_turret_it_goes_in_the_turrets_grid_and_must_fit_its_deck() {
        let deck = DeckUnder { body: Entity::PLACEHOLDER, ring_cell: IVec2::new(9, 9), centre: IVec2::new(10, 10), size: 3 };
        let plan = stamp_plan(&design(), IVec2::new(10, 10), Rotation::East, Some(deck), &|_| true, &[]);
        assert_eq!(plan.problem, None, "{:?}", plan);
        assert_eq!(plan.turret, Some(IVec2::new(9, 9)));
        assert!(plan.parts.iter().any(|p| p.module_type == ModuleType::GunBarrel && p.cell == IVec2::new(1, 0)));
        let taken = [IVec2::new(-1, 0)];
        assert!(stamp_plan(&design(), IVec2::new(10, 10), Rotation::East, Some(deck), &|_| true, &taken).problem.is_some());
        let reaching = stamp_plan(&design(), IVec2::new(12, 10), Rotation::East, Some(deck), &|_| true, &[]);
        assert_eq!(reaching.problem, None, "a barrel may reach past the deck");
        let far = stamp_plan(&design(), IVec2::new(13, 10), Rotation::East, Some(deck), &|_| true, &[]);
        assert_eq!(far.problem.as_deref(), Some("Doesn't fit on this turret"), "the firing piece can't");
    }

    #[test]
    fn a_magazine_in_the_design_keeps_it_off_a_turret() {
        let mut d = design();
        d.parts.push(GunDesignPart { module_type: ModuleType::ShellFeeder, offset: IVec2::new(-2, 0), rotation: Rotation::East });
        let deck = DeckUnder { body: Entity::PLACEHOLDER, ring_cell: IVec2::ZERO, centre: IVec2::ZERO, size: 5 };
        let plan = stamp_plan(&d, IVec2::ZERO, Rotation::East, Some(deck), &|_| true, &[]);
        assert_eq!(plan.problem.as_deref(), Some("Shell Feeder can't go on a turret"));
    }
}
