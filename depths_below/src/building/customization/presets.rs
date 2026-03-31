use std::collections::HashMap;
use super::parameters::ModuleCustomization;

// ============================================================================
// PRESET SYSTEM
// One-click configurations for players who don't want to touch Tier 3.
// Each preset sets all parameters to a curated configuration.
// ============================================================================

#[derive(Clone, Debug)]
pub struct Preset {
    pub name: String,
    pub description: String,
    pub slot_selections: HashMap<String, usize>,
    pub parameter_values: HashMap<String, f32>,
}

impl Preset {
    pub fn apply(&self, customization: &mut ModuleCustomization) {
        customization.slot_selections = self.slot_selections.clone();
        customization.parameter_values = self.parameter_values.clone();
    }
}

/// Built-in presets for cannon
pub fn cannon_presets() -> Vec<Preset> {
    vec![
        Preset {
            name: "Balanced".into(),
            description: "Default configuration. Good at everything, great at nothing.".into(),
            slot_selections: HashMap::from([
                ("Barrel".into(), 0),      // Standard
                ("Ammunition".into(), 0),  // AP
                ("Feed Mechanism".into(), 1), // Belt Feed
                ("Cooling".into(), 0),     // Passive
                ("Mount".into(), 1),       // Turret
                ("Targeting".into(), 1),   // Basic Lead
            ]),
            parameter_values: HashMap::new(), // All defaults
        },
        Preset {
            name: "Sniper".into(),
            description: "Maximum range and accuracy. Slow fire rate. Picks off targets at distance.".into(),
            slot_selections: HashMap::from([
                ("Barrel".into(), 1),      // Long Range
                ("Ammunition".into(), 0),  // AP
                ("Feed Mechanism".into(), 0), // Manual (for precision)
                ("Cooling".into(), 0),     // Passive (low fire rate = low heat)
                ("Mount".into(), 1),       // Turret
                ("Targeting".into(), 2),   // Advanced Tracking
            ]),
            parameter_values: HashMap::from([
                ("Barrel.barrel_length".into(), 4000.0),
                ("Barrel.bore_diameter".into(), 50.0),
                ("Barrel.rifling_depth".into(), 4.0),
                ("Ammunition.propellant_charge".into(), 80.0),
                ("Ammunition.penetrator_length".into(), 400.0),
                ("Targeting.prediction_range".into(), 2500.0),
            ]),
        },
        Preset {
            name: "Brawler".into(),
            description: "Maximum fire rate at close range. Gets in close and hammers.".into(),
            slot_selections: HashMap::from([
                ("Barrel".into(), 2),      // Stubby
                ("Ammunition".into(), 1),  // HE (area damage up close)
                ("Feed Mechanism".into(), 2), // Autoloader
                ("Cooling".into(), 1),     // Active Coolant
                ("Mount".into(), 1),       // Turret
                ("Targeting".into(), 0),   // Manual (close range doesn't need lead)
            ]),
            parameter_values: HashMap::from([
                ("Barrel.bore_diameter".into(), 200.0),
                ("Feed Mechanism.loader_speed".into(), 600.0),
                ("Cooling.coolant_flow_rate".into(), 15.0),
            ]),
        },
        Preset {
            name: "Fire Starter".into(),
            description: "Incendiary rounds. Sets everything on fire. Chaos.".into(),
            slot_selections: HashMap::from([
                ("Barrel".into(), 0),      // Standard
                ("Ammunition".into(), 2),  // Incendiary
                ("Feed Mechanism".into(), 1), // Belt Feed
                ("Cooling".into(), 0),     // Passive
                ("Mount".into(), 1),       // Turret
                ("Targeting".into(), 1),   // Basic Lead
            ]),
            parameter_values: HashMap::from([
                ("Ammunition.incendiary_compound".into(), 80.0),
                ("Ammunition.burn_duration".into(), 10.0),
            ]),
        },
    ]
}
