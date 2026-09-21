# Demo Checklist

What has to be true before Depths Below goes out as a playable demo.
Compiled 2026-09-20 from three audits of the tree at `cascade-story`.

`ROADMAP.md` is the plan; this is the punch list. Where the two disagree, this
file is newer.

Legend: `[ ]` open · `[x]` done · `[~]` partly done · `[-]` cut for after the demo.

**Status 2026-09-21:** 22 items closed on `cascade-story`, verified by an
eight-minute autoplay run of the real loop: no panics, credits growing, fuel
draining on throttle, plating unlocking, cascade climbing to 0.17 across two
warps. Remaining open items are listed below.

**Since:** distance now costs money (docking no longer fills the tank; prices
scale to 3x at the galaxy's edge), the expedition trail gives the player a
stated objective with a demo wall at tier 1, and hulks can be towed to a
station for the rare loot a boarding party cannot carry. Creatures stay off by
decision; the Kill contracts that depend on them are still being issued and
remain open below.

---

## 1. Blockers — a player gets stuck, confused, or loses their save

- [x] **Tutorial step 13 of 14 cannot be completed.** It says "use the Crew
      button". The docked toolbar has no Crew button, and the crew menu is
      gated to `Exploring`, so pressing C while docked does nothing either. The
      player sits on 13/14 until they find `;` to dismiss.
      `tutorial.rs:422`, `ui/mod.rs:221`, `ui/mod.rs:370`
- [x] **The hull palette places the wrong block.** Seven hull items, six
      labels. "VOD" lays a Hallway, "BLK" lays Void, "ANG" lays a Bulkhead
      Door. Angled Hull Plate has no slot at all. The comment above the list
      says the order must match; it has not since `Hallway` was inserted.
      `ui/build_ui.rs:960-967` vs `resources.rs:908-925`
- [x] **Autosave writes enemy ships into the player's save.** Three unfiltered
      queries; AI hull, modules and crew carry the same components. Hull cells
      are recorded from *local* transforms, so an enemy plate at its own (2,3)
      is stored as yours. Fires every two minutes while exploring. The load
      path has the mirror defect: its despawn queries are unscoped too, so
      loading guts every live AI ship.
      `meta.rs:120-122`, `:279-281`, `:428-430`
- [x] **Nothing states a goal.** Menu says "Build your ship. Explore the void.
      Survive." The tutorial ends on "push deeper". No objective is ever named.
      `ui/mod.rs:3556`, `tutorial.rs:130`
- [x] **"All crew died" never fires while any enemy lives.** The crew query is
      unscoped, so one living AI crewman anywhere keeps it false. The player's
      whole crew can die and the run continues as a ghost ship.
      `ship/systems.rs:44`
- [x] **The player can launch with zero crew.** Starter-crew spawn is suppressed
      by any living AI crew. Combined with the above, a total-crew-loss run can
      neither end nor recover by docking. `crew/mod.rs:277-283`
- [x] **Docking services bill and heal across ship boundaries.** Repair Hull
      full-heals every AI ship in the world on the player's credits; Repair
      Modules charges for enemy battle damage; hire cost and the "berths full"
      check count enemy crew. `ui/mod.rs:4437-4442`, `:4530`, `:4824`

## 2. Introduced by the story work — mine to fix

- [x] **A second expedition can never reach the ending.** `discovered_logs` is
      a system `Local`, which outlives the run. `reset_for_new_game` clears
      `Statistics.logs_found` but cannot touch a `Local`, so every log read in
      run one — the finale included — is permanently unreadable until the
      process restarts. `world/mod.rs:327-338`
- [x] **`check_poi_discovery` is the unfixed twin of the log bug.** Log
      discovery was moved to `PostUpdate` for unpropagated transforms; its
      neighbour 150 lines up was left in `Update`. Every streamed point of
      interest reads as sitting at the origin on its first frame: toast spam
      and phantom contract completions. `world/mod.rs:46`, `:170-212`
- [x] **The ending plays in total silence.** `stop_flight_loops` fires on
      `OnExit(Exploring)` and `Truth` is entered from `Exploring`. `audio.rs:41`
- [x] **The built-in starter ship has no Memory Cores.** Three were added to
      `starter.json` but not to `builtin_starter_design()`. If the JSON is ever
      regenerated the core death condition and the autonomy mechanic silently
      switch off — the same fallback trap as the faction designs.
      `ship/spawner.rs:123`

## 3. The loop

- [ ] **Four of seven contract objectives are dead or free.** `CaptureLive` is
      impossible (`CargoHold.current_weight` is never written by anything).
      `ReachDepth` auto-completes on leaving Haven. `SurveyZone` uses a second,
      different zone function and is either impossible or completes on a timer
      while you sit still. `ExplorePoi` only fires near Haven. Three share one
      root cause: they read `DepthState.current_depth`, which is distance from
      the shared origin and is ≥180,000 in every system but Haven.
- [x] **Difficulty is a cliff, not a ramp.** Creature biome, density and spawn
      rate read the same broken distance and saturate everywhere outside Haven.
      One warp takes you from harmless drifters to 1500-HP leviathans at
      maximum spawn rate. Fix: scale off the system's own `danger_tier`, which
      is already an authored weak-near/strong-far curve.
      `world/mod.rs:128-155`, `creatures/mod.rs:101-107`
- [x] **Crossing the galaxy is free.** Max jump is 500 fuel of a 1500 tank,
      four seconds, and refuel is free at every dock. A minute-one player can
      click the far edge and arrive parked beside a station in the hardest
      territory in the game. `celestial/warp.rs:15-28`
- [x] **Fuel burns while merely powered, not while thrusting.** Five engines at
      1.0 each is 4/sec sitting still: about six minutes a tank. The tutorial
      says "watch FUEL tick down as you burn", which is not what happens.
      `ship/movement.rs:255-278`
- [~] **Progression gates nothing.** Hull materials now unlock by how far out
      the player has actually been, so the gate opens and there is a reason to
      go. Still open: `Unlocks.modules` has no reader, and 52 of 157 modules
      are unreachable because `BuildCategory` has no `Structural` variant —
      **including ShieldEmitter**, so shield strength is fixed for the whole
      run.
- [x] **Deposits are checked but never charged.** Accepting a contract is a
      free option with a downside only on death. `contracts/ui.rs:176-184`
- [x] **Blind warp can strand you.** Landing further than `SNAP_TOLERANCE` from
      any system spawns nothing — no station, so no refuel. Six minutes later
      you cannot jump out. `celestial/warp.rs:233-243`

## 4. Story reachability

- [x] **Give the player a thread to follow.** The finale log sits in a
      30,000–100,000 unit ring around one of six far systems and appears on no
      radar, map or minimap. Reuse the nav arrow and map marker that
      `contracts/bounty_nav.rs` already draws for DestroyShip contracts.
- [ ] **The dread audio bed is inaudible for the whole demo.** Both layers gate
      on cascade ≥0.30 and a demo player sits near zero. Right for a full
      playthrough; means the demo ships with one drone. Revisit once pacing is
      tuned. `audio.rs:393-398`

## 5. Sound — all from files already licensed and in the repo

- [x] **~50 seconds of opening silence.** No menu audio, no station ambience.
      `interior_hum.ogg` and `machine_loop_1-3.ogg` are sitting unused.
- [x] **Taking a hit makes no sound.** `ShipDamaged` has no handler; projectile
      impacts are explicitly unwired. Only a fully destroyed hull tile is
      audible. `combat/new_projectiles.rs:822`
- [x] **Pausing hard-cuts the entire soundscape** and restarts it from sample
      zero on resume, because `stop_flight_loops` fires on `OnExit(Exploring)`.
- [x] **Nothing marks launch**, the most cinematic beat in the first minute.
- [x] **`DockingCompleted` is dead code** — a handler with no writer. Wire it
      or delete it. `audio.rs:559`
- [ ] Ten licensed files loaded by nothing (~2.4 MB). Three `engine_*_loop`
      were deliberately rejected; the rest are free content.

## 6. Cut for after the demo

- [-] Music. The bus drives one ambient drone. Biggest perceived-quality gain
      available and the only item that depends on assets not yet acquired.
- [-] Key rebinding. The controls tab is a read-only cheatsheet.
- [-] Colourblind options, despite a green/red build ghost and a
      green/yellow/red damage overlay.
- [-] Mines and manual fire: four systems exist and are registered nowhere, yet
      Mine ammo is issued and the UI says "Mine deployed!".
- [-] `crew_weapon_system` — weapons auto-firing at creatures has never worked.
- [-] Radiation (`ship/radiation.rs` entirely unregistered), `ResearchState`
      (write-only), `AICombatCore` (registered no-op), `Unlocks.blueprints_found`
      and `Statistics.ships_lost` (both dead fields).
- [-] Main menu art and motion. It is text on a flat colour; the parallax
      starfield already exists but is gated out of `MainMenu`.

## 7. Not mine to close

- [ ] **Steam app registration.** The only item that can block a date
      regardless of the code. Owner: you.
- [ ] Replace the 128 kbps Freesound previews for `engine_rumble_loop` and
      `hostile_atmosphere_loop` — `CREDITS.md` says to do this once a sound
      becomes prominent, and both now are.
- [ ] Decide whether celestial wrecks should appear on radar and map. Affects
      ordinary salvage as well as the ending.

---

## Corrections to ROADMAP.md

- `crew-walk` is **already merged** into master; Phase B lists it as pending.
- The parked "`starter.json` armour drift, 67 plates shipped vs 75 generated"
  note is stale: the file is now 120 hull cells, 71 modules, 32 plates.
- Open items 1 and 3 ("no demo goal", "New Expedition doesn't reset") are
  closed in code, though item 1's second half — *nothing tells a new player
  what they are for* — is still true.

## Branches

Two unmerged lanes, overlapping in four files (`components.rs`, `crew/mod.rs`,
`debug.rs`, `ui/mod.rs`): `suit-damage-control` (4 commits) and `cascade-story`
(15). Land the smaller one first.
