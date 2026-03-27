pub mod generation;
pub mod tracking;
pub mod ui;

use bevy::prelude::*;
use serde::{Serialize, Deserialize};

use crate::components::{CreatureType, PoiType, ZoneType};
use crate::resources::ItemType;
use crate::states::GameState;

// ============================================================================
// CORE DATA TYPES
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub enum Faction {
    ResearchInstitute,
    Navy,
    SalvageGuild,
}

impl Faction {
    pub fn name(&self) -> &'static str {
        match self {
            Faction::ResearchInstitute => "Research Institute",
            Faction::Navy => "Navy",
            Faction::SalvageGuild => "Salvage Guild",
        }
    }

    pub fn all() -> &'static [Faction] {
        &[Faction::ResearchInstitute, Faction::Navy, Faction::SalvageGuild]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum ContractType {
    Kill,
    ExplorePoi,
    ReachDepth,
    RetrieveSalvage,
    CaptureLive,
    SurveyZone,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum ContractObjective {
    Kill { creature_type: CreatureType, target_count: u32, current_count: u32 },
    ExplorePoi { poi_type: PoiType, discovered: bool },
    ReachDepth { target_depth: f32, reached: bool },
    RetrieveSalvage { item_type: ItemType, target_count: u32, current_count: u32 },
    CaptureLive { creature_type: CreatureType, captured: bool },
    SurveyZone { zone: ZoneType, required_seconds: f32, elapsed_seconds: f32 },
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum ContractStatus {
    Available,
    Active,
    Completed,
    TurnedIn,
    Failed,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Contract {
    pub id: u32,
    pub faction: Faction,
    pub contract_type: ContractType,
    pub title: String,
    pub description: String,
    pub objective: ContractObjective,
    pub reward: u32,
    pub deposit: u32,
    pub star_rating: u8,
    pub depth_zone: ZoneType,
    pub status: ContractStatus,
}

impl Contract {
    /// Returns a short progress string for HUD display.
    pub fn progress_text(&self) -> String {
        match &self.objective {
            ContractObjective::Kill { current_count, target_count, .. } => {
                format!("{}/{}", current_count, target_count)
            }
            ContractObjective::ExplorePoi { discovered, .. } => {
                if *discovered { "Done".into() } else { "X".into() }
            }
            ContractObjective::ReachDepth { reached, .. } => {
                if *reached { "Done".into() } else { "X".into() }
            }
            ContractObjective::RetrieveSalvage { current_count, target_count, .. } => {
                format!("{}/{}", current_count, target_count)
            }
            ContractObjective::CaptureLive { captured, .. } => {
                if *captured { "Done".into() } else { "X".into() }
            }
            ContractObjective::SurveyZone { elapsed_seconds, required_seconds, .. } => {
                format!("{:.0}/{:.0}s", elapsed_seconds, required_seconds)
            }
        }
    }

    /// Check if the objective is fulfilled.
    pub fn is_objective_complete(&self) -> bool {
        match &self.objective {
            ContractObjective::Kill { current_count, target_count, .. } => current_count >= target_count,
            ContractObjective::ExplorePoi { discovered, .. } => *discovered,
            ContractObjective::ReachDepth { reached, .. } => *reached,
            ContractObjective::RetrieveSalvage { current_count, target_count, .. } => current_count >= target_count,
            ContractObjective::CaptureLive { captured, .. } => *captured,
            ContractObjective::SurveyZone { elapsed_seconds, required_seconds, .. } => elapsed_seconds >= required_seconds,
        }
    }

    /// Star display string like "[**]" or "[***]".
    pub fn star_display(&self) -> String {
        format!("[{}]", "*".repeat(self.star_rating as usize))
    }
}

// ============================================================================
// RESOURCES
// ============================================================================

#[derive(Resource, Serialize, Deserialize, Clone, Default)]
pub struct ContractState {
    pub available_contracts: Vec<Contract>,
    pub active_contracts: Vec<Contract>,
    pub next_id: u32,
    pub contracts_completed_total: u32,
    pub contracts_failed_total: u32,
}

impl ContractState {
    pub const MAX_ACTIVE: usize = 3;
}

#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct FactionReputation {
    pub research: f32,
    pub navy: f32,
    pub salvage: f32,
}

impl Default for FactionReputation {
    fn default() -> Self {
        Self {
            research: 10.0,
            navy: 10.0,
            salvage: 10.0,
        }
    }
}

impl FactionReputation {
    pub fn get(&self, faction: &Faction) -> f32 {
        match faction {
            Faction::ResearchInstitute => self.research,
            Faction::Navy => self.navy,
            Faction::SalvageGuild => self.salvage,
        }
    }

    pub fn add(&mut self, faction: &Faction, amount: f32) {
        let val = match faction {
            Faction::ResearchInstitute => &mut self.research,
            Faction::Navy => &mut self.navy,
            Faction::SalvageGuild => &mut self.salvage,
        };
        *val = (*val + amount).clamp(0.0, 100.0);
    }

    /// Max star rating unlocked for a given faction.
    pub fn max_star(&self, faction: &Faction) -> u8 {
        let rep = self.get(faction);
        if rep >= 80.0 { 5 }
        else if rep >= 50.0 { 4 }
        else if rep >= 25.0 { 3 }
        else if rep >= 10.0 { 2 }
        else { 1 }
    }

    /// Display stars like "***" for the max star rating.
    pub fn star_string(&self, faction: &Faction) -> String {
        "*".repeat(self.max_star(faction) as usize)
    }
}

#[derive(Resource, Default)]
pub struct MissionBoardOpen(pub bool);

// ============================================================================
// PLUGIN
// ============================================================================

pub struct ContractsPlugin;

impl Plugin for ContractsPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<ContractState>()
            .init_resource::<FactionReputation>()
            .init_resource::<MissionBoardOpen>()
            .init_resource::<ui::MissionBoardSelection>()
            // Generate board + turn in completed contracts on surface
            .add_systems(OnEnter(GameState::SurfaceBase), (
                generation::generate_initial_board,
                tracking::turn_in_contracts,
            ).chain())
            // Tracking during exploration
            .add_systems(Update, (
                tracking::track_kill_contracts,
                tracking::track_poi_contracts,
                tracking::track_depth_contracts,
                tracking::track_salvage_contracts,
                tracking::track_survey_contracts,
                tracking::track_capture_contracts,
                tracking::check_contract_completion,
            ).chain().run_if(in_state(GameState::Exploring)))
            // Mission board UI at surface
            .add_systems(Update, (
                ui::toggle_mission_board,
                ui::mission_board_input,
                ui::update_mission_board_display,
            ).chain().run_if(in_state(GameState::SurfaceBase)))
            // Contract HUD during exploration
            .add_systems(OnEnter(GameState::Exploring), ui::spawn_contract_hud)
            .add_systems(OnExit(GameState::Exploring), ui::despawn_contract_hud)
            .add_systems(Update, ui::update_contract_hud
                .run_if(in_state(GameState::Exploring)))
            // Failure on game over
            .add_systems(OnEnter(GameState::GameOver), tracking::handle_contract_failure);
    }
}
