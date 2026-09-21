//! Towing a hulk home.
//!
//! Stripping a wreck where it died already works: crew suit up, cross over,
//! and carry it back a piece at a time. That stays. Towing is the other
//! option, and the point is that it is a *choice* rather than an upgrade.
//!
//! Strip in place and you get what a boarding party can carry — scrap and
//! common goods, quickly, and you leave whenever you like. Drag the whole
//! hulk to a station and the yard opens it properly: everything still aboard,
//! plus the rare classes a crewman cannot pry loose and carry home through
//! vacuum.
//!
//! The cost is the trip. A hulk under tow adds its mass to yours, so you
//! accelerate and turn like something much heavier, and it makes noise, which
//! is already what draws hunters. Scavengers are meanwhile eating whatever you
//! left behind. One at a time.

use bevy::prelude::*;
use rand::Rng;

use crate::ai_ship::components::AiShipWreck;
use crate::components::{Module, ModuleType, Ship, ShipPhysics, Velocity, Wreck};
use crate::events::{NotificationType, ShowNotification};
use crate::resources::{Inventory, ItemType, NoiseState};
use crate::states::GameState;

/// How close the ship must be to latch on.
const LATCH_RANGE: f32 = 900.0;

/// Where the hulk rides, behind the ship along its facing.
const TOW_OFFSET: f32 = 620.0;

/// How quickly the hulk settles into its tow position. Low, so it swings
/// rather than snapping — it should feel dragged, not welded.
const TOW_FOLLOW: f32 = 2.2;

/// Mass a towed hulk adds, per unit of loot still aboard. Everything in
/// `ship_movement` divides force by `physics.mass`, so this costs acceleration
/// and turning without touching the movement code at all.
const MASS_PER_LOOT: f32 = 220.0;

/// Noise a tow adds. Noise already draws hunters, which is the point: you are
/// slow and loud at the same time.
const TOW_NOISE: f32 = 45.0;

/// The hulk currently under tow, and what the ship weighed before it.
#[derive(Resource, Default)]
pub struct Towing {
    pub hulk: Option<Entity>,
    base_mass: Option<f32>,
}

/// Latch onto or release the nearest hulk. Requires a Tractor Beam aboard —
/// the module has existed in the registry since long before this, described as
/// "Pulls objects toward ship. Salvage.", and did nothing at all.
#[allow(clippy::type_complexity)]
fn toggle_tow(
    keys: Res<ButtonInput<KeyCode>>,
    mut tow: ResMut<Towing>,
    ship: Query<(&GlobalTransform, &ShipPhysics), With<Ship>>,
    modules: Query<&Module, Without<crate::components::DestroyedModule>>,
    wrecks: Query<(Entity, &GlobalTransform, &AiShipWreck)>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    // Y, not T: T is hold-to-free-look. Y's only other binding is build-mode
    // symmetry, which cannot be active while flying.
    if !keys.just_pressed(KeyCode::KeyY) {
        return;
    }
    let Ok((ship_gt, _)) = ship.single() else { return };

    if tow.hulk.is_some() {
        tow.hulk = None;
        notifications.write(ShowNotification {
            message: "Tow released.".into(),
            notification_type: NotificationType::Info,
            duration: 3.0,
        });
        return;
    }

    let has_beam = modules
        .iter()
        .any(|m| m.module_type == ModuleType::TractorBeam && m.is_active);
    if !has_beam {
        notifications.write(ShowNotification {
            message: "No tractor beam aboard, or it has no power.".into(),
            notification_type: NotificationType::Warning,
            duration: 4.0,
        });
        return;
    }

    let ship_pos = ship_gt.translation().truncate();
    let nearest = wrecks
        .iter()
        .map(|(e, gt, w)| (e, gt.translation().truncate().distance(ship_pos), w))
        .filter(|(_, d, _)| *d < LATCH_RANGE)
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    match nearest {
        Some((entity, _, wreck)) => {
            tow.hulk = Some(entity);
            notifications.write(ShowNotification {
                message: format!(
                    "Tow latched. {} hulk under tow - heavy and loud. Bring it to a station.",
                    crate::ai_ship::components::faction_display_name(wreck.ship_type)
                ),
                notification_type: NotificationType::Success,
                duration: 6.0,
            });
        }
        None => {
            notifications.write(ShowNotification {
                message: "Nothing in range to tow.".into(),
                notification_type: NotificationType::Warning,
                duration: 3.0,
            });
        }
    }
}

/// Drag the hulk along behind, and make the ship feel it.
#[allow(clippy::type_complexity)]
fn drag_hulk(
    time: Res<Time>,
    mut tow: ResMut<Towing>,
    mut ship: Query<(&GlobalTransform, &mut ShipPhysics), With<Ship>>,
    mut hulks: Query<(&mut Transform, &AiShipWreck), Without<Ship>>,
    mut noise: ResMut<NoiseState>,
) {
    let Ok((ship_gt, mut physics)) = ship.single_mut() else { return };

    let Some(hulk) = tow.hulk else {
        // Give the ship its own weight back.
        if let Some(base) = tow.base_mass.take() {
            physics.mass = base;
        }
        return;
    };

    let Ok((mut hulk_tf, wreck)) = hulks.get_mut(hulk) else {
        // It was destroyed or despawned underneath us.
        tow.hulk = None;
        if let Some(base) = tow.base_mass.take() {
            physics.mass = base;
        }
        return;
    };

    let base = *tow.base_mass.get_or_insert(physics.mass);
    physics.mass = base + wreck.loot_remaining as f32 * MASS_PER_LOOT;
    noise.noise_level += TOW_NOISE * time.delta_secs();

    let ship_pos = ship_gt.translation().truncate();
    let facing = ship_gt.compute_transform().rotation;
    let behind = ship_pos - (facing * Vec3::Y).truncate().normalize_or_zero() * TOW_OFFSET;

    let here = hulk_tf.translation.truncate();
    let next = here + (behind - here) * (time.delta_secs() * TOW_FOLLOW).min(1.0);
    hulk_tf.translation.x = next.x;
    hulk_tf.translation.y = next.y;
}

/// What a hulk is worth when the yard opens it, as opposed to what a boarding
/// party can carry off it.
///
/// Everything still aboard comes out, and on top of that the classes a crewman
/// cannot pry loose and haul home through vacuum. Those rare items are the
/// reason to take the slow road; later they are what better weapons and an
/// upgrade path should be built out of.
pub fn hulk_yard_value(wreck: &AiShipWreck, rng: &mut impl Rng) -> Vec<ItemType> {
    let mut out = Vec::new();
    for _ in 0..wreck.loot_remaining {
        out.push(match rng.gen_range(0..100) {
            0..=54 => ItemType::ScrapMetal,
            55..=74 => ItemType::Crystal,
            75..=89 => ItemType::FuelCell,
            _ => ItemType::BioSample,
        });
    }

    // The rare tier scales with how intact the hull was when it died, so the
    // way you killed it still decides the harvest. A clean kill that struck
    // its colours yields more than a reactor breach that gutted half of it.
    let rare = ((wreck.loot_remaining as f32 * 0.35) * wreck.intact_frac).round() as u32;
    for _ in 0..rare.max(1) {
        out.push(if rng.gen::<f32>() < 0.25 {
            ItemType::AncientArtifact
        } else {
            ItemType::RareAlloy
        });
    }
    out
}

/// Arriving at a station with a hulk in tow opens it.
fn process_hulk_on_dock(
    mut tow: ResMut<Towing>,
    mut commands: Commands,
    hulks: Query<&AiShipWreck>,
    mut inventory: ResMut<Inventory>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let Some(hulk) = tow.hulk.take() else { return };
    let Ok(wreck) = hulks.get(hulk) else { return };

    let mut rng = rand::thread_rng();
    let haul = hulk_yard_value(wreck, &mut rng);
    let rare = haul
        .iter()
        .filter(|i| matches!(i, ItemType::RareAlloy | ItemType::AncientArtifact))
        .count();

    for item in &haul {
        inventory.add_item(*item, 1);
    }

    notifications.write(ShowNotification {
        message: format!(
            "{} hulk broken up: {} units recovered, {} of them rare. Nothing a boarding party could have carried.",
            crate::ai_ship::components::faction_display_name(wreck.ship_type),
            haul.len(),
            rare
        ),
        notification_type: NotificationType::Success,
        duration: 9.0,
    });

    commands.entity(hulk).try_despawn();
}

fn release_on_exit(mut tow: ResMut<Towing>) {
    tow.hulk = None;
}

pub struct TowingPlugin;

impl Plugin for TowingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Towing>()
            .add_systems(
                Update,
                (toggle_tow, drag_hulk).chain().run_if(in_state(GameState::Exploring)),
            )
            .add_systems(OnEnter(GameState::StationDocked), process_hulk_on_dock)
            .add_systems(OnEnter(GameState::MainMenu), release_on_exit)
            .add_systems(OnEnter(GameState::GameOver), release_on_exit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_ship::components::AiShipType;

    fn hulk(loot: u32, intact: f32) -> AiShipWreck {
        AiShipWreck { ship_type: AiShipType::TerranHegemony, loot_remaining: loot, intact_frac: intact }
    }

    /// Towing must yield the rare classes, because that is the entire reason
    /// to take the slow, loud, heavy road instead of stripping and leaving.
    #[test]
    fn the_yard_gives_what_a_boarding_party_cannot() {
        let mut rng = rand::thread_rng();
        let haul = hulk_yard_value(&hulk(10, 1.0), &mut rng);
        let rare = haul
            .iter()
            .filter(|i| matches!(i, ItemType::RareAlloy | ItemType::AncientArtifact))
            .count();
        assert!(rare > 0, "a towed hulk produced nothing rare, so towing is pointless");
        assert!(haul.len() > 10, "the yard returned less than was aboard");
    }

    /// How you killed it still decides the harvest. A gutted hulk must be
    /// worth less than one that struck its colours, or careful shooting stops
    /// mattering the moment towing exists.
    #[test]
    fn a_clean_kill_is_worth_more() {
        let mut rng = rand::thread_rng();
        let clean: usize = (0..40)
            .map(|_| rare_count(&hulk_yard_value(&hulk(10, 1.0), &mut rng)))
            .sum();
        let gutted: usize = (0..40)
            .map(|_| rare_count(&hulk_yard_value(&hulk(10, 0.2), &mut rng)))
            .sum();
        assert!(clean > gutted, "clean {clean} vs gutted {gutted}");
    }

    /// Even a wrecked hulk is worth towing at all, or the choice collapses to
    /// "only tow perfect kills" and the mechanic is dead most of the time.
    #[test]
    fn even_a_gutted_hulk_pays_something() {
        let mut rng = rand::thread_rng();
        let haul = hulk_yard_value(&hulk(2, 0.05), &mut rng);
        assert!(rare_count(&haul) > 0);
    }

    /// Mass has to actually bite. ship_movement divides force by mass, so this
    /// is what makes a tow feel heavy without touching the movement code.
    #[test]
    fn a_tow_is_heavy() {
        assert!(MASS_PER_LOOT > 100.0, "a hulk barely changes how the ship handles");
        assert!(TOW_NOISE > 0.0, "a tow should be loud as well as slow");
    }

    fn rare_count(v: &[ItemType]) -> usize {
        v.iter().filter(|i| matches!(i, ItemType::RareAlloy | ItemType::AncientArtifact)).count()
    }
}
