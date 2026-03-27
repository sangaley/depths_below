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

Bevy 0.11 ECS submarine survival game. 2D, sprite-based, grid-based building system (66.0 unit cells).

### Plugin Structure (registered in `main.rs`)

| Plugin | Location | Responsibility |
|---|---|---|
| **EventsPlugin** | `events.rs` | Registers all game events (30+ event types) |
| **SubmarinePlugin** | `submarine/` | Movement, physics, power, oxygen, pressure, hull, combat, flooding, sonar |
| **WorldPlugin** | `world/` | Chunk management, biomes, POI discovery, depth zones, procedural generation |
| **CreaturePlugin** | `creatures/` | Hostile creature AI/spawning, ambient life (fish, jellyfish, whales) |
| **CrewPlugin** | `crew/` | Crew spawning, needs (O2/morale), AI, suffocation, death |
| **BuildingPlugin** | `building/` | Grid placement/removal, occupancy, room detection, module registry |
| **UiPlugin** | `ui/` | HUD, menus (main/pause/game-over), build ghost, notifications, overlays |
| **MetaPlugin** | `meta.rs` | Persistence (unlocks JSON), inventory, currency, statistics |

### Core Data Flow

1. **Components** (`components.rs`): All ECS components live here. Central types: `Module`, `ModuleType` (42 variants), `ModuleCategory` (9 categories), `Rotation`, `HullSegment`, `HullMaterial`, `Creature`, `CrewMember`.

2. **Resources** (`resources.rs`): Global state. Key resources: `SubmarineState`, `BuildingState`, `GameConfig`, `WorldState`, `ChunkManager`, `Inventory`, `Unlocks`, `Statistics`.

3. **Events** (`events.rs`): All events registered in `EventsPlugin`. Grouped by domain: submarine damage/breach, building place/remove, crew damage/death, creature spotted/attack, world/UI/save-load.

4. **States** (`states.rs`): `GameState` (MainMenu, Loading, SurfaceBase, Exploring, Docked, Paused, GameOver) and `BuildState` (Inactive, Placing, Moving, Connecting, Deleting).

### Module Registry System

`building/registry.rs` defines `ModuleRegistry` — a data-driven HashMap<ModuleType, ModuleDef> with stats, size, color, and `CompanionData` for each of the 42 modules. `submarine/spawner.rs::spawn_module()` reads the registry to spawn entities with the correct `Module` component plus companion components (Reactor, Engine, Weapon+WeaponCooldown+WeaponMount+TargetingSystem+AmmoStorage, Sonar, etc.).

**To add a new module**: Add variant to `ModuleType` enum, add it to the relevant `ModuleCategory::module_types()` list, add `ModuleDef` entry in `build_registry()`, and if needed add a new `CompanionData` variant + handling in `spawn_module()`.

### Grid & Building System

- Grid cell size: 66.0 world units
- `GridOccupancy` (HashMap<IVec2, Entity>) tracks occupied cells, rebuilt each frame
- Multi-cell modules supported via `ModuleDef.size` (e.g., LargeReactor is 2x1)
- Placement validation: no overlap + adjacency required + positional rules (propulsion at rear, crew not near power)
- Building only active in `GameState::SurfaceBase`
- Build flow: input -> `PlaceModuleRequest`/`PlaceHullRequest` event -> process system -> `ModulePlaced` event

### Key Conventions

- Events use typed enums (`ModuleType`, `CreatureType`) not strings
- Existing systems query companion components (`Weapon`, `Sonar`, `Engine`, etc.) not `ModuleType` — this keeps them backward-compatible when new module types are added
- Systems are `.chain()`-ed within plugins and gated by `.run_if(in_state(...))` on `GameState`/`BuildState`
- Hull segments and modules are children of the `Submarine` entity
- Notifications use `ShowNotification` events with `NotificationType` (Info/Warning/Danger/Success)

### Game Controls

- WASD: submarine movement, Q/E: ballast, Space: fire weapons, Z: sonar ping
- B: toggle build mode, Tab: cycle build categories, [/]: cycle items, R: rotate, M: cycle hull material, X: delete mode
- C: crew menu, M: map/inventory overlay, P: module panel (while paused), ESC: pause, Enter: start/dive
