// Event fields are part of the data contract — readers may not consume all fields yet.
#![allow(dead_code)]

use bevy::prelude::*;
use crate::components::{ModuleType, Rotation, HullLayer, HullMaterial, SubComponentType, ExplosiveType};
use crate::ai_submarine::components::AiSubType;
use crate::resources::ItemType;

// ============================================================================
// SUBMARINE EVENTS
// ============================================================================

/// Fired when submarine takes damage
#[derive(Event)]
pub struct SubmarineDamaged {
    pub source: DamageSource,
    pub amount: f32,
    pub position: Option<Vec2>,
    pub direction: Option<Vec2>,  // Normalized attack direction
}

#[derive(Clone, Debug)]
pub enum DamageSource {
    Pressure,
    Creature(Entity),
    Collision,
    Explosion,
    Fire,
}

/// Fired when a hull segment is breached
#[derive(Event)]
pub struct HullBreached {
    pub segment: Entity,
    pub severity: f32,  // 0.0 to 1.0
}

/// Fired when a module is damaged
#[derive(Event)]
pub struct ModuleDamaged {
    pub module: Entity,
    pub amount: f32,
}

/// Fired when a module is destroyed
#[derive(Event)]
pub struct ModuleDestroyed {
    pub module: Entity,
}

/// Fired when a room floods
#[derive(Event)]
pub struct RoomFlooded {
    pub room_id: usize,
    pub severity: f32,
}

/// Fired when power state changes significantly
#[derive(Event)]
pub struct PowerStateChanged {
    pub new_balance: f32,
    pub is_critical: bool,
}

/// Fired when oxygen state changes significantly
#[derive(Event)]
pub struct OxygenStateChanged {
    pub new_level: f32,
    pub is_critical: bool,
}

/// Fired when depth changes zones
#[derive(Event)]
pub struct DepthZoneChanged {
    pub new_depth: f32,
    pub new_zone: crate::components::ZoneType,
}

// ============================================================================
// BUILDING EVENTS
// ============================================================================

/// Request to place a module (uses ModuleType enum instead of String)
#[derive(Event)]
pub struct PlaceModuleRequest {
    pub module_type: ModuleType,
    pub grid_position: IVec2,
    pub rotation: Rotation,
    pub custom_name: Option<String>,
    pub subcomponents: Option<Vec<SubComponentType>>,
    /// If true, skip cost deduction (used by blueprint loading)
    pub free: bool,
}

/// Request to place a hull segment
#[derive(Event)]
pub struct PlaceHullRequest {
    pub layer: HullLayer,
    pub material: HullMaterial,
    pub grid_position: IVec2,
    /// If true, skip cost deduction (used by blueprint loading)
    pub free: bool,
}

/// Module was successfully placed
#[derive(Event)]
pub struct ModulePlaced {
    pub module: Entity,
    pub module_type: ModuleType,
    pub grid_position: IVec2,
}

/// Request to remove a module
#[derive(Event)]
pub struct RemoveModuleRequest {
    pub module: Entity,
}

/// Module was removed
#[derive(Event)]
pub struct ModuleRemoved {
    pub module_type: ModuleType,
    pub grid_position: IVec2,
}

// ============================================================================
// CREW EVENTS
// ============================================================================

/// Crew member took damage
#[derive(Event)]
pub struct CrewDamaged {
    pub crew: Entity,
    pub amount: f32,
    pub source: CrewDamageSource,
}

#[derive(Clone, Debug)]
pub enum CrewDamageSource {
    Suffocation,
    Flooding,
    Fire,
    Creature,
    Explosion,
}

/// Crew member died
#[derive(Event)]
pub struct CrewDied {
    pub crew: Entity,
    pub name: String,
    pub cause: CrewDamageSource,
}

/// Staffing priority changed on a module
#[derive(Event)]
pub struct StaffingPriorityChanged {
    pub module: Entity,
    pub new_priority: u8,
}

// ============================================================================
// CREATURE EVENTS
// ============================================================================

/// Creature spotted the submarine
#[derive(Event)]
pub struct CreatureSpotted {
    pub creature: Entity,
    pub creature_type: crate::components::CreatureType,
    pub distance: f32,
}

/// Creature is attacking
#[derive(Event)]
pub struct CreatureAttacking {
    pub creature: Entity,
    pub target: Entity,
}

/// Creature was killed
#[derive(Event)]
pub struct CreatureKilled {
    pub creature: Entity,
    pub creature_type: crate::components::CreatureType,
    pub loot: Vec<ItemType>,
}

// ============================================================================
// ECOSYSTEM EVENTS
// ============================================================================

/// A creature ate another creature
#[derive(Event)]
pub struct CreatureAteCreature {
    pub predator: Entity,
    pub predator_type: crate::components::CreatureType,
    pub prey_type: crate::components::CreatureType,
    pub position: Vec2,
}

/// A creature ate from a corpse
#[derive(Event)]
pub struct CreatureAteCorpse {
    pub creature: Entity,
    pub creature_type: crate::components::CreatureType,
    pub corpse_type: crate::components::CreatureType,
}

/// Type of ecosystem cascade
#[derive(Clone, Debug)]
pub enum CascadeType {
    PredatorVoid,
    ScavengerSwarm,
    PreyBoom,
    TerritoryShift,
}

/// Large-scale ecosystem event triggered by player actions
#[derive(Event)]
pub struct EcosystemCascade {
    pub cascade_type: CascadeType,
    pub position: Vec2,
}

// ============================================================================
// WORLD EVENTS
// ============================================================================

/// Entered a new chunk
#[derive(Event)]
pub struct ChunkEntered {
    pub chunk_pos: IVec2,
}

/// Discovered a point of interest
#[derive(Event)]
pub struct PoiDiscovered {
    pub poi_type: crate::components::PoiType,
    pub position: Vec2,
}

/// Started docking with something
#[derive(Event)]
pub struct DockingStarted {
    pub target: Entity,
}

/// Finished docking
#[derive(Event)]
pub struct DockingCompleted {
    pub target: Entity,
}

// ============================================================================
// UI EVENTS
// ============================================================================

/// Notification to display
#[derive(Event)]
pub struct ShowNotification {
    pub message: String,
    pub notification_type: NotificationType,
    pub duration: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum NotificationType {
    Info,
    Warning,
    Danger,
    Success,
}

/// Request to open a menu
#[derive(Event)]
pub struct OpenMenu {
    pub menu_type: MenuType,
}

#[derive(Clone, Copy, Debug)]
pub enum MenuType {
    Pause,
    Inventory,
    Map,
    CrewManagement,
    BuildMenu,
    Settings,
}

/// Request to close current menu
#[derive(Event)]
pub struct CloseMenu;

// ============================================================================
// CHAIN REACTION / FIRE / CASCADE EVENTS
// ============================================================================

/// Fired when an explosive module detonates (after PendingDetonation timer)
#[derive(Event)]
pub struct ModuleExploded {
    pub grid_position: IVec2,
    pub blast_damage: f32,
    pub explosive_type: ExplosiveType,
}

/// Fired to ignite a module
#[derive(Event)]
pub struct FireStarted {
    pub module: Entity,
    pub grid_position: IVec2,
    pub intensity: f32,
}

/// Fired when a fire goes out
#[derive(Event)]
pub struct FireExtinguished {
    pub module: Entity,
    pub cause: FireExtinguishCause,
}

#[derive(Clone, Copy, Debug)]
pub enum FireExtinguishCause {
    Flooding,
    BurnedOut,
    CrewSuppressed,
}

/// Fired when a hull segment reaches 0 HP
#[derive(Event)]
pub struct HullSegmentDestroyed {
    pub segment: Entity,
    pub grid_position: IVec2,
}

// ============================================================================
// CRISIS MANAGEMENT EVENTS
// ============================================================================

/// Request to seal/unseal a bulkhead door
#[derive(Event)]
pub struct ToggleBulkhead {
    pub segment: Entity,
    pub seal: bool,
}

/// Fired when a crew member is dispatched to handle an emergency
#[derive(Event)]
pub struct CrewDispatched {
    pub crew: Entity,
    pub room_id: usize,
    pub reason: DispatchReason,
}

#[derive(Clone, Copy, Debug)]
pub enum DispatchReason {
    Flooding,
    Fire,
}

// ============================================================================
// AI SUBMARINE EVENTS
// ============================================================================

/// Fired when an AI submarine takes damage
#[derive(Event)]
pub struct AiSubDamaged {
    pub target: Entity,
    pub source: DamageSource,
    pub amount: f32,
    pub position: Option<Vec2>,
    pub direction: Option<Vec2>,
}

/// Fired when an AI submarine is destroyed
#[derive(Event)]
pub struct AiSubDestroyed {
    pub entity: Entity,
    pub sub_type: AiSubType,
    pub position: Vec2,
}

// ============================================================================
// CONTRACT EVENTS
// ============================================================================

/// A contract was accepted from the mission board
#[derive(Event)]
pub struct ContractAccepted {
    pub contract_id: u32,
}

/// A contract objective was completed (during exploration)
#[derive(Event)]
pub struct ContractCompleted {
    pub contract_id: u32,
}

/// A contract was failed (e.g. game over with deposit)
#[derive(Event)]
pub struct ContractFailed {
    pub contract_id: u32,
}

/// A completed contract was turned in at surface for rewards
#[derive(Event)]
pub struct ContractTurnedIn {
    pub contract_id: u32,
    pub reward: u32,
    pub faction: crate::contracts::Faction,
}

// ============================================================================
// SAVE/LOAD EVENTS
// ============================================================================

#[derive(Event)]
pub struct SaveGameRequest {
    pub slot: u32,
}

#[derive(Event)]
pub struct LoadGameRequest {
    pub slot: u32,
}

#[derive(Event)]
pub struct GameSaved {
    pub slot: u32,
    pub success: bool,
}

#[derive(Event)]
pub struct GameLoaded {
    pub slot: u32,
    pub success: bool,
}

// ============================================================================
// PLUGIN TO REGISTER ALL EVENTS
// ============================================================================

pub struct EventsPlugin;

impl Plugin for EventsPlugin {
    fn build(&self, app: &mut App) {
        app
            // Submarine events
            .add_event::<SubmarineDamaged>()
            .add_event::<HullBreached>()
            .add_event::<ModuleDamaged>()
            .add_event::<ModuleDestroyed>()
            .add_event::<RoomFlooded>()
            .add_event::<PowerStateChanged>()
            .add_event::<OxygenStateChanged>()
            .add_event::<DepthZoneChanged>()
            // Building events
            .add_event::<PlaceModuleRequest>()
            .add_event::<PlaceHullRequest>()
            .add_event::<ModulePlaced>()
            .add_event::<RemoveModuleRequest>()
            .add_event::<ModuleRemoved>()
            // Crew events
            .add_event::<CrewDamaged>()
            .add_event::<CrewDied>()
            .add_event::<StaffingPriorityChanged>()
            // Creature events
            .add_event::<CreatureSpotted>()
            .add_event::<CreatureAttacking>()
            .add_event::<CreatureKilled>()
            // Ecosystem events
            .add_event::<CreatureAteCreature>()
            .add_event::<CreatureAteCorpse>()
            .add_event::<EcosystemCascade>()
            // World events
            .add_event::<ChunkEntered>()
            .add_event::<PoiDiscovered>()
            .add_event::<DockingStarted>()
            .add_event::<DockingCompleted>()
            // UI events
            .add_event::<ShowNotification>()
            .add_event::<OpenMenu>()
            .add_event::<CloseMenu>()
            // Chain reaction / fire / cascade events
            .add_event::<ModuleExploded>()
            .add_event::<FireStarted>()
            .add_event::<FireExtinguished>()
            .add_event::<HullSegmentDestroyed>()
            // Crisis management events
            .add_event::<ToggleBulkhead>()
            .add_event::<CrewDispatched>()
            // AI submarine events
            .add_event::<AiSubDamaged>()
            .add_event::<AiSubDestroyed>()
            // Contract events
            .add_event::<ContractAccepted>()
            .add_event::<ContractCompleted>()
            .add_event::<ContractFailed>()
            .add_event::<ContractTurnedIn>()
            // Save/load events
            .add_event::<SaveGameRequest>()
            .add_event::<LoadGameRequest>()
            .add_event::<GameSaved>()
            .add_event::<GameLoaded>();
    }
}
