//! Ships seen from far out: a name over each one.
//!
//! Whose a ship is shows in its paint (ai_ship::livery), Cosmoteer-style; what
//! it is doesn't, once it's a few pixels across. Past `FAR_FROM` every enemy
//! carries a label -- its faction and class, in the faction's colour -- that
//! fades in as the camera pulls out and stays readable however far it goes.

use bevy::prelude::*;

use crate::ai_ship::components::{AiShip, AiShipClass, AiShipType};
use crate::building::layout::LayoutVersion;
use crate::building::ShipGrid;
use crate::camera::{MainCamera, ZoomInvariantText};

/// Camera zoom where the labels start to show...
pub const FAR_FROM: f32 = 4.5;
/// ...and where they're fully there.
pub const FAR_SOLID: f32 = 7.0;

/// On an enemy ship: its label, and how far its blocks reach from its centre.
#[derive(Component)]
pub struct FarView {
    label: Entity,
    measured: Option<LayoutVersion>,
    reach: f32,
}

/// A name over a ship, kept upright above it whichever way it turns.
#[derive(Component)]
pub struct FarLabel {
    ship: Entity,
}

/// The dark copy behind a label, so it reads over the ship's own paint.
#[derive(Component)]
pub struct FarLabelShadow;

/// How much of the far view shows at this zoom: 0 close in, 1 far out.
pub fn far_alpha(zoom: f32) -> f32 {
    let t = ((zoom - FAR_FROM) / (FAR_SOLID - FAR_FROM)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// What the label says: "Terran Hegemony - Warden", or the station.
fn label_text(faction: AiShipType, class: Option<&AiShipClass>, station: bool) -> String {
    let faction_name = crate::ai_ship::components::faction_display_name(faction);
    if station {
        return format!("{faction_name} - station");
    }
    match class.and_then(|c| crate::ai_ship::roster::find(faction, c.0)) {
        Some(c) => format!("{faction_name} - {}", c.name),
        None => faction_name.to_string(),
    }
}

/// A faction's label colour: its map colour, lifted a little to read as text.
fn label_colour(faction: AiShipType) -> Color {
    Color::Srgba(crate::ai_ship::components::faction_map_color(faction).to_srgba().mix(&Srgba::WHITE, 0.3))
}

/// Give every enemy ship its label.
#[allow(clippy::type_complexity)]
pub fn attach_far_view(
    mut commands: Commands,
    ships: Query<(Entity, &AiShipType, Option<&AiShipClass>, Has<crate::diplomacy::FactionStation>), (With<AiShip>, Without<FarView>)>,
) {
    for (ship, &faction, class, station) in ships.iter() {
        let text = label_text(faction, class, station);
        let label = commands
            .spawn((
                Text2d::new(text.clone()),
                TextFont { font_size: FontSize::Px(15.0), ..default() },
                TextColor(label_colour(faction).with_alpha(0.0)),
                Transform::from_xyz(0.0, 0.0, 5.0),
                Visibility::Hidden,
                ZoomInvariantText,
                FarLabel { ship },
            ))
            .id();
        commands.spawn((
            Text2d::new(text),
            TextFont { font_size: FontSize::Px(15.0), ..default() },
            TextColor(Color::BLACK.with_alpha(0.0)),
            Transform::from_xyz(1.2, -1.2, -0.1),
            FarLabelShadow,
            ChildOf(label),
        ));
        commands.entity(ship).try_insert(FarView { label, measured: None, reach: 0.0 });
    }
}

/// How far a ship's blocks reach, re-measured when its layout changes.
pub fn measure_far_view(mut ships: Query<(&mut FarView, &ShipGrid, &LayoutVersion)>) {
    for (mut view, grid, version) in ships.iter_mut() {
        if view.measured == Some(*version) {
            continue;
        }
        let reach = grid
            .cells
            .keys()
            .map(|c| crate::building::grid_to_local(*c).length())
            .fold(0.0f32, f32::max);
        view.reach = reach + crate::building::GRID_SIZE * 0.5;
        view.measured = Some(*version);
    }
}

/// Fade labels in and out with the zoom, and keep them over their ships.
#[allow(clippy::type_complexity)]
pub fn show_far_view(
    mut commands: Commands,
    camera: Query<&Projection, With<MainCamera>>,
    ships: Query<(&FarView, &GlobalTransform)>,
    mut labels: Query<(Entity, &FarLabel, &mut Transform, &mut TextColor, &mut Visibility)>,
    mut shadows: Query<&mut TextColor, (With<FarLabelShadow>, Without<FarLabel>)>,
) {
    let Ok(Projection::Orthographic(projection)) = camera.single() else { return };
    let alpha = far_alpha(projection.scale);
    for mut shadow in shadows.iter_mut() {
        shadow.0.set_alpha(alpha * 0.85);
    }
    let lift = 30.0 * crate::camera::label_scale(projection.scale);
    for (label, far, mut t, mut colour, mut vis) in labels.iter_mut() {
        let Ok((view, gt)) = ships.get(far.ship) else {
            commands.entity(label).try_despawn();
            continue;
        };
        let want = if alpha > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        colour.0.set_alpha(alpha);
        let at = gt.translation().truncate() + Vec2::new(0.0, view.reach + lift);
        t.translation = at.extend(t.translation.z);
    }
}

/// Labels left behind when the game state changes.
pub fn clear_far_labels(mut commands: Commands, labels: Query<Entity, With<FarLabel>>, ships: Query<Entity, With<FarView>>) {
    for e in labels.iter() {
        commands.entity(e).try_despawn();
    }
    for e in ships.iter() {
        commands.entity(e).try_remove::<FarView>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_shows_only_zoomed_out() {
        assert_eq!(far_alpha(1.8), 0.0);
        assert_eq!(far_alpha(FAR_FROM), 0.0);
        assert_eq!(far_alpha(FAR_SOLID), 1.0);
        assert!(far_alpha(5.7) > 0.0 && far_alpha(5.7) < 1.0);
    }

    #[test]
    fn labels_name_the_faction_and_class() {
        assert_eq!(label_text(AiShipType::TerranHegemony, Some(&AiShipClass("warden")), false), "Terran Hegemony - Warden");
        assert_eq!(label_text(AiShipType::TerranHegemony, None, true), "Terran Hegemony - station");
    }
}
