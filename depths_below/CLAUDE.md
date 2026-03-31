# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run Commands

```bash
cargo build              # Build (dev profile, dynamic linking enabled)
cargo run                # Run the game
cargo build --release    # Release build (LTO enabled, single codegen unit)
cargo check              # Fast type-check without full build
cargo clippy             # Lint (if clippy installed)
```

No test suite exists yet. No CI/CD pipeline.

## Architecture

Bevy 0.11 ECS **space survival game**. 2D, sprite-based, grid-based building system (66.0 unit cells). Originally a submarine game, fully converted to space theme.

### Plugin Structure (registered in `main.rs`)

| Plugin | Location | Responsibility |
|---|---|---|
| **EventsPlugin** | `events.rs` | Registers all game events (30+ event types) |
| **SubmarinePlugin** | `submarine/` | Ship movement, physics, power, oxygen, radiation, hull, combat, decompression, radar |
| **WorldPlugin** | `world/` | Chunk management, biomes, POI discovery, zone transitions, procedural generation |
| **CreaturePlugin** | `creatures/` | Hostile creature AI/spawning, ambient life (space motes, pulsing spores, cosmic whales) |
| **CrewPlugin** | `crew/` | Crew spawning, needs (O2/morale), AI, suffocation, death |
| **BuildingPlugin** | `building/` | Grid placement/removal, occupancy, room detection, module registry |
| **UiPlugin** | `ui/` | HUD, menus (main/pause/game-over), build ghost, notifications, overlays |
| **MetaPlugin** | `meta.rs` | Persistence (unlocks JSON), inventory, currency, statistics |

### Core Data Flow

1. **Components** (`components.rs`): All ECS components live here. Central types: `Module`, `ModuleType` (42 variants), `ModuleCategory` (9 categories), `Rotation`, `HullSegment`, `HullMaterial`, `Creature`, `CrewMember`.

2. **Resources** (`resources.rs`): Global state. Key resources: `SubmarineState`, `BuildingState`, `GameConfig`, `WorldState`, `ChunkManager`, `Inventory`, `Unlocks`, `Statistics`.

3. **Events** (`events.rs`): All events registered in `EventsPlugin`. Grouped by domain: ship damage/breach, building place/remove, crew damage/death, creature spotted/attack, world/UI/save-load.

4. **States** (`states.rs`): `GameState` (MainMenu, Loading, StationDocked, Exploring, Docked, Paused, GameOver) and `BuildState` (Inactive, Placing, Moving, Connecting, Deleting).

### Space Theme Key Systems

- **Radiation damage** (`submarine/pressure.rs`): Replaces old pressure system. Radiation intensity scales with distance from safe zones. Hull segments have `radiation_shielding` ratings per material tier.
- **Decompression** (`submarine/flooding.rs`): Hull breaches cause air to escape (rooms have `air_level` 1.0→0.0). Drains oxygen. Crew seal breaches to restore air. Fire is extinguished by vacuum (low air).
- **Thrusters** (`submarine/movement.rs`): Q/E controls vertical thrusters instead of ballast. Space physics with minimal drag.
- **Zones**: NearOrbit → AsteroidBelt → DeepSpace → Nebula → BlackHole
- **Biomes**: OpenVoid, AsteroidField, CrystalFormation, VoidRift, ThermalVents, IceShells, DeadZone, AncientRuins

### Module Registry System

`building/registry.rs` defines `ModuleRegistry` — a data-driven HashMap<ModuleType, ModuleDef> with stats, size, color, and `CompanionData` for each of the 42 modules. `submarine/spawner.rs::spawn_module()` reads the registry to spawn entities with the correct `Module` component plus companion components (Reactor, Engine, Weapon+WeaponCooldown+WeaponMount+TargetingSystem+AmmoStorage, Sonar, etc.).

**To add a new module**: Add variant to `ModuleType` enum, add it to the relevant `ModuleCategory::module_types()` list, add `ModuleDef` entry in `build_registry()`, and if needed add a new `CompanionData` variant + handling in `spawn_module()`.

### Grid & Building System

- Grid cell size: 66.0 world units
- `GridOccupancy` (HashMap<IVec2, Entity>) tracks occupied cells, rebuilt each frame
- Multi-cell modules supported via `ModuleDef.size` (e.g., LargeReactor is 2x1)
- Placement validation: no overlap + adjacency required + positional rules (propulsion at rear, crew not near power)
- Building only active in `GameState::StationDocked`
- Build flow: input -> `PlaceModuleRequest`/`PlaceHullRequest` event -> process system -> `ModulePlaced` event

### Key Conventions

- Events use typed enums (`ModuleType`, `CreatureType`) not strings
- Existing systems query companion components (`Weapon`, `Sonar`, `Engine`, etc.) not `ModuleType` — this keeps them backward-compatible when new module types are added
- Systems are `.chain()`-ed within plugins and gated by `.run_if(in_state(...))` on `GameState`/`BuildState`
- Hull segments and modules are children of the ship entity
- Notifications use `ShowNotification` events with `NotificationType` (Info/Warning/Danger/Success)

### Game Controls

- WASD: ship movement, Q/E: vertical thrusters, Space: fire weapons, Z: radar ping
- B: toggle build mode, Tab: cycle build categories, [/]: cycle items, R: rotate, M: cycle hull material, X: delete mode
- C: crew menu, M: map/inventory overlay, P: module panel (while paused), ESC: pause, Enter: start/launch
