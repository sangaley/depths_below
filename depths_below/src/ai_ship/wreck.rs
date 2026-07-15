use bevy::prelude::*;
use rand::Rng;

use crate::components::*;
use crate::events::*;
use crate::resources::ItemType;
use super::components::*;

/// LOOT IDENTITY — what a wreck yields depends on who you killed and how.
/// Each faction has signature cargo (a Glass Eye carries intel, a Rust Swarm
/// carries junk); scrap metal is the filler that survives any kill. The
/// forensic record then biases composition: a shattered hulk's delicate
/// cargo is slag (non-scrap weights crushed), a pristine kill preserves and
/// even concentrates it. Thresholds match the quantity tiers in
/// ai_ship_death_system (>= 0.7 pristine, < 0.3 shattered).
pub fn roll_wreck_loot(ship_type: AiShipType, intact_frac: f32, rng: &mut impl Rng) -> ItemType {
    use ItemType::*;
    let table: &[(ItemType, f32)] = match ship_type {
        AiShipType::RustSwarm => &[(ScrapMetal, 8.0), (FuelCell, 1.0), (AmmoCrate, 1.0)],
        AiShipType::IronTide => &[(ScrapMetal, 4.0), (RareAlloy, 3.0), (AmmoCrate, 3.0)],
        AiShipType::Blackwater => &[(AmmoCrate, 4.0), (FuelCell, 3.0), (ScrapMetal, 2.0), (RareAlloy, 1.0)],
        AiShipType::PressureKing => &[(RareAlloy, 4.0), (Crystal, 3.0), (ScrapMetal, 2.0), (FuelCell, 1.0)],
        AiShipType::GlassEye => &[(Crystal, 4.0), (AncientArtifact, 3.0), (FuelCell, 2.0), (ScrapMetal, 1.0)],
        AiShipType::Drowned => &[(AncientArtifact, 4.0), (ScrapMetal, 3.0), (Crystal, 2.0), (BioSample, 1.0)],
        AiShipType::AbyssalCult => &[(BioSample, 4.0), (AncientArtifact, 2.0), (Crystal, 2.0), (ScrapMetal, 2.0)],
        AiShipType::Leviathan => &[(BioSample, 5.0), (Crystal, 2.0), (ScrapMetal, 2.0), (RareAlloy, 1.0)],
        AiShipType::Dreadnought => &[(AmmoCrate, 4.0), (RareAlloy, 3.0), (ScrapMetal, 2.0), (FuelCell, 1.0)],
        AiShipType::VoidTitan => &[(AncientArtifact, 3.0), (RareAlloy, 3.0), (Crystal, 2.0), (AmmoCrate, 1.0), (FuelCell, 1.0)],
    };

    let good_mult = if intact_frac >= 0.7 {
        1.5
    } else if intact_frac < 0.3 {
        0.35
    } else {
        1.0
    };
    let weight = |item: ItemType, w: f32| if item == ScrapMetal { w } else { w * good_mult };

    let total: f32 = table.iter().map(|&(i, w)| weight(i, w)).sum();
    let mut roll = rng.gen_range(0.0..total);
    for &(item, w) in table {
        roll -= weight(item, w);
        if roll <= 0.0 {
            return item;
        }
    }
    table[0].0
}

/// Secondary explosions marching across a fresh wreck for a second or two
/// after the kill, ending in one final boom. Pure spectacle — pops don't
/// deal damage (the ship is already dead), they sell the death.
#[derive(Component)]
pub struct DeathRattle {
    pub timer: Timer,
    pub remaining: u32,
}

/// Ticks death rattles: random block pops with debris, then the final boom.
pub fn update_death_rattle(
    time: Res<Time>,
    mut commands: Commands,
    mut rattle_query: Query<(Entity, &mut DeathRattle, Option<&Velocity>)>,
    children_query: Query<&Children>,
    block_pos_query: Query<(&GlobalTransform, &Sprite), Or<(With<Module>, With<HullSegment>)>>,
    mut boom_events: MessageWriter<AiModuleExploded>,
) {
    let mut rng = rand::thread_rng();
    for (ship_entity, mut rattle, velocity) in rattle_query.iter_mut() {
        rattle.timer.tick(time.delta());
        if !rattle.timer.is_finished() { continue; }

        // Pick a random surviving block to pop at
        let pop_pos = children_query.get(ship_entity).ok().and_then(|children| {
            let blocks: Vec<(Vec2, Color)> = children.iter()
                .filter_map(|c| block_pos_query.get(c).ok())
                .map(|(gt, sprite)| (gt.translation().truncate(), sprite.color))
                .collect();
            if blocks.is_empty() { None } else {
                Some(blocks[rng.gen_range(0..blocks.len())])
            }
        });
        let Some((pos, color)) = pop_pos else {
            commands.entity(ship_entity).try_remove::<DeathRattle>();
            continue;
        };

        let inherited = velocity.map(|v| v.0 * 0.6).unwrap_or(Vec2::ZERO);

        if rattle.remaining > 1 {
            // Secondary pop: flash + chunk spray + attenuated crunch
            crate::combat::spawn_hit_effect(&mut commands, pos, Color::srgb(1.0, 0.6, 0.15), rng.gen_range(30.0..60.0));
            crate::vfx::debris::spawn_chunks(&mut commands, &mut rng, pos, color, inherited);
            boom_events.write(AiModuleExploded { position: pos, blast_damage: 20.0 });
            rattle.remaining -= 1;
            rattle.timer = Timer::from_seconds(rng.gen_range(0.15..0.4), TimerMode::Once);
        } else {
            // Final boom — big flash, big spray, deep audio layer
            crate::combat::spawn_hit_effect(&mut commands, pos, Color::srgb(1.0, 0.5, 0.1), 140.0);
            for _ in 0..3 {
                crate::vfx::debris::spawn_chunks(&mut commands, &mut rng, pos, color, inherited);
            }
            boom_events.write(AiModuleExploded { position: pos, blast_damage: 100.0 });
            commands.entity(ship_entity).try_remove::<DeathRattle>();
        }
    }
}

/// When an AI ship is destroyed, turn it into a wreck IN PLACE — keep the
/// hull/module hierarchy intact (attach Wreck/PointOfInterest/AiShipWreck to
/// the existing root) instead of despawning it and spawning an unrelated
/// generic gray sprite. That old approach erased the actual battle damage
/// the player had just fought for and replaced it with an abstract blob;
/// this way the derelict really is the ship you shot up, sitting there for
/// F-key salvage (world::salvage_wreck_system already works off any entity
/// with Wreck + PointOfInterest, no changes needed there). Remaining
/// modules are deactivated and darkened — the reactor's dead, nothing on
/// this hulk still has power — but nothing is despawned.
pub fn ai_ship_death_system(
    mut commands: Commands,
    mut destroyed_events: MessageReader<AiShipDestroyed>,
    children_query: Query<&Children>,
    mut module_query: Query<(&mut Module, &mut Sprite, Has<DestroyedModule>), Without<HullSegment>>,
    mut hull_query: Query<(&mut Sprite, &HullSegment, Has<HullDestroyed>), Without<Module>>,
    registry: Res<crate::building::ModuleRegistry>,
    mut currency: ResMut<crate::resources::Currency>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for event in destroyed_events.read() {
        let base_loot = match event.ship_type {
            AiShipType::VoidTitan => 30,    // legendary hoard
            AiShipType::Dreadnought => 20,  // colossal wreck
            AiShipType::Leviathan => 6,
            AiShipType::AbyssalCult => 4,
            AiShipType::Drowned => 8,     // rare old loot
            AiShipType::PressureKing => 5,
            AiShipType::GlassEye => 7,    // intel data
            AiShipType::IronTide => 10,    // massive wreck
            AiShipType::Blackwater => 6,
            AiShipType::RustSwarm => 2,    // junk
        };

        // Power off and darken every block — dead reactor, no power,
        // nothing left running. Blocks that survived the fight (not
        // individually destroyed) pay out half their build cost in
        // credits — salvage value for the parts that are still intact.
        const WRECK_TINT: Color = Color::srgb(0.18, 0.18, 0.2);
        let mut salvage_value = 0.0_f32;
        let mut block_count = 0u32;
        let mut intact_count = 0u32;
        if let Ok(children) = children_query.get(event.entity) {
            for child in children.iter() {
                if let Ok((mut module, mut sprite, is_destroyed)) = module_query.get_mut(child) {
                    block_count += 1;
                    if !is_destroyed {
                        intact_count += 1;
                        salvage_value += registry.get(module.module_type).cost as f32 * 0.5;
                    }
                    module.is_active = false;
                    sprite.color = WRECK_TINT;
                } else if let Ok((mut sprite, hull, is_destroyed)) = hull_query.get_mut(child) {
                    block_count += 1;
                    if !is_destroyed {
                        intact_count += 1;
                        salvage_value += hull.material.cost() as f32 * 0.5;
                    }
                    sprite.color = WRECK_TINT;
                }
            }
        }

        // FORENSIC WRECKS — the kill method shapes the loot. A surgical kill
        // (EMP-disable, snipe the reactor, hull mostly whole) leaves a
        // pristine wreck worth extra; grinding the ship to dust leaves
        // scraps. intact fraction is the honest measure of "how gently
        // did you kill this".
        let intact_frac = if block_count > 0 {
            intact_count as f32 / block_count as f32
        } else {
            0.0
        };
        let (loot, condition) = if intact_frac >= 0.7 {
            (((base_loot as f32) * 1.5).ceil() as u32, "Pristine wreck — bonus salvage!")
        } else if intact_frac < 0.3 {
            (((base_loot as f32) * 0.5).ceil() as u32, "Shattered hulk — little left to take.")
        } else {
            (base_loot, "Wreck can be salvaged.")
        };

        commands.entity(event.entity).try_insert((
            AiShipWreck {
                ship_type: event.ship_type,
                loot_remaining: loot,
                intact_frac,
            },
            PointOfInterest {
                poi_type: PoiType::Wreck,
                discovered: true,
            },
            Wreck {
                loot_remaining: loot,
                is_explored: false,
            },
        ));

        // Bigger ships rattle longer before the final boom; shattered kills
        // rattle extra hard — there's more unstable wreckage going up.
        let extra_pops = if intact_frac < 0.3 { 3 } else { 0 };
        commands.entity(event.entity).try_insert(DeathRattle {
            timer: Timer::from_seconds(0.2, TimerMode::Once),
            remaining: (block_count / 5).clamp(3, 10) + extra_pops,
        });

        let type_name = match event.ship_type {
            AiShipType::VoidTitan => "Void Titan",
            AiShipType::Dreadnought => "Dreadnought",
            AiShipType::Leviathan => "Leviathan Rider",
            AiShipType::AbyssalCult => "Abyssal Cult",
            AiShipType::Drowned => "Drowned",
            AiShipType::PressureKing => "Pressure King",
            AiShipType::GlassEye => "Glass Eye",
            AiShipType::IronTide => "Iron Tide",
            AiShipType::Blackwater => "Blackwater",
            AiShipType::RustSwarm => "Rust Swarm",
        };

        let mut message = format!("{} vessel destroyed! {} (F: salvage detail)", type_name, condition);
        if salvage_value > 0.0 {
            let payout = salvage_value.round() as u32;
            currency.credits += payout;
            message = format!("{} destroyed! +{}c intact salvage. {} (F: salvage detail)", type_name, payout, condition);
        }

        notifications.write(ShowNotification {
            message,
            notification_type: NotificationType::Success,
            duration: 4.0,
        });
    }
}

/// HOT WRECKS — fires on a fresh wreck keep eating it. While any block on a
/// wreck is still burning, its remaining loot ticks away (one unit every few
/// seconds). Looting immediately means working next to live fires and
/// cook-offs (burning explosive blocks still detonate); waiting for the
/// burn-out is safe but costs cargo. Fires are finite (BlockBurning has a
/// duration), so a wreck never burns to literally nothing unless it was
/// already ablaze everywhere.
pub fn wreck_fire_consumes_loot(
    time: Res<Time>,
    mut commands: Commands,
    mut wreck_query: Query<(Entity, &mut Wreck, &mut AiShipWreck, &Children)>,
    burning_query: Query<(), With<crate::combat::new_projectiles::BlockBurning>>,
    block_pos_query: Query<&GlobalTransform, Or<(With<Module>, With<HullSegment>)>>,
    mut tick: Local<f32>,
) {
    *tick += time.delta_secs();
    if *tick < 4.0 { return; }
    *tick = 0.0;

    let mut rng = rand::thread_rng();
    for (_entity, mut wreck, mut ai_wreck, children) in wreck_query.iter_mut() {
        if wreck.loot_remaining == 0 { continue; }
        let burning_blocks: Vec<Entity> = children.iter()
            .filter(|c| burning_query.get(*c).is_ok())
            .collect();
        if burning_blocks.is_empty() { continue; }

        wreck.loot_remaining = wreck.loot_remaining.saturating_sub(1);
        ai_wreck.loot_remaining = ai_wreck.loot_remaining.saturating_sub(1);

        // Smoke puff over a random burning block so the loss reads visually
        let smoke_at = burning_blocks[rng.gen_range(0..burning_blocks.len())];
        if let Ok(gt) = block_pos_query.get(smoke_at) {
            crate::combat::spawn_hit_effect(
                &mut commands,
                gt.translation().truncate(),
                Color::srgba(0.4, 0.38, 0.35, 0.6),
                40.0,
            );
        }
    }
}
