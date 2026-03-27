use crate::resources::BiomeType;

/// Returns creature spawn weights for a biome
pub fn biome_creature_weights(biome: BiomeType) -> Vec<(&'static str, f32)> {
    match biome {
        BiomeType::OpenOcean => vec![
            ("scavenger", 0.4),
            ("stalker", 0.2),
        ],
        BiomeType::KelpForest => vec![
            ("scavenger", 0.3),
            ("ambusher", 0.3),
            ("lure_fish", 0.2),
        ],
        BiomeType::CoralReef => vec![
            ("scavenger", 0.3),
            ("electric_eel", 0.2),
            ("lure_fish", 0.3),
        ],
        BiomeType::ThermalVents => vec![
            ("electric_eel", 0.4),
            ("parasite", 0.2),
            ("blind_hunter", 0.2),
        ],
        BiomeType::IceCaverns => vec![
            ("stalker", 0.3),
            ("ambusher", 0.3),
            ("blind_hunter", 0.2),
        ],
        BiomeType::AbyssalPlain => vec![
            ("blind_hunter", 0.3),
            ("watcher", 0.3),
            ("swarm_queen", 0.1),
            ("lure_fish", 0.2),
        ],
        BiomeType::DeepTrench => vec![
            ("blind_hunter", 0.3),
            ("leviathan", 0.05),
            ("watcher", 0.3),
            ("swarm_queen", 0.15),
        ],
        BiomeType::SunkenCity => vec![
            ("watcher", 0.4),
            ("ambusher", 0.2),
            ("parasite", 0.2),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_biomes_have_creature_weights() {
        let biomes = [
            BiomeType::OpenOcean,
            BiomeType::KelpForest,
            BiomeType::CoralReef,
            BiomeType::ThermalVents,
            BiomeType::IceCaverns,
            BiomeType::AbyssalPlain,
            BiomeType::DeepTrench,
            BiomeType::SunkenCity,
        ];

        for biome in biomes {
            let weights = biome_creature_weights(biome);
            assert!(!weights.is_empty(), "{:?} should have creature spawn weights", biome);

            // All weights should be positive
            for (name, weight) in &weights {
                assert!(*weight > 0.0, "Creature '{}' in {:?} has non-positive weight", name, biome);
            }
        }
    }

    #[test]
    fn deep_biomes_have_dangerous_creatures() {
        let deep_weights = biome_creature_weights(BiomeType::DeepTrench);
        let creature_names: Vec<&str> = deep_weights.iter().map(|(n, _)| *n).collect();
        assert!(creature_names.contains(&"leviathan"), "DeepTrench should contain leviathan");
    }
}
