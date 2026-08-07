# Depths Below — Roadmap

Target: **playable demo (or full launch, if scope allows) by September 3, 2026, on Steam.**
Written 2026-07-20, deadline moved up from Nov 12 same day. ~6.5 weeks out,
~20-30 hrs/week available (~130-190 hours total budget — less than half the
original 320-480 hour plan). This is a living doc — update it as phases close.

**This cut the runway by more than half, not just moved a date.** Phases below are
recompressed to fit, and the cut list is real — if a week slips, cut from the bottom
up, don't crunch the ship date.

---

## Where we are

Started 2026-03-26. 81 commits, core gameplay loop fully working end to end since the
`qol-batch` merge to master on 2026-07-19:

- **Build** — freeform grid ship building, 155 buildable modules (power, propulsion,
  life support, weapons, detection, storage, crew, utility, structural, control).
  Drag-paint hull, undo/redo, copy/paste, controller support.
- **Launch → Explore** — procedural space: 5 distance zones, 8 biomes, real gravity
  from stars/planets/black holes.
- **Survive** — power routing, hull breaches/decompression, fire spread, crew
  morale/panic, radiation.
- **Fight** — kinetic/energy/missile weapons with per-ammo behaviors, heat as a
  tuning counterweight, shields, 10 AI factions with real crew + power simulation
  (not just flavor text), destructible ships (debris, death-rattle, forensic wrecks).
- **Salvage** — crew EVA dismantling, Breaker Drill, scavenger ships contesting wrecks.
  **Economy** — station buy/sell, market shortages, hiring board.
  **Repair & construction** — field repair, ghost rebuild-in-flight.
- **Discover** — 23+ log entries building a cosmic-horror thread.
- **UI** — consistent theme/card styling across HUD, crew/module panels, map,
  build mode, docking menu.

In flight right now:
- Uncommitted: AI ship power/spawner tweaks, energy weapon adjustments, a cargo HUD
  widget, two new faction ship designs (Dreadnought, Void Titan).
- Stashed: galaxy-map phase 1 — seeded/deterministic star-system generation, groundwork
  for a multi-system galaxy map (warp to a system, get the same layout back from its seed).
- Art brief sent to an outside artist (128×128/cell spec, priority list) — waiting on
  first delivery.

Explicitly still open (from README + MODULES.md):
- Progression depth — hull upgrade path, build-cost curve
- Tutorial / onboarding — no teaching layer yet
- Stealth mechanics, alert ramping
- Fuller audio pass (gritty/cinematic direction chosen, sourcing ongoing)
- A handful of deferred module mechanics (sloped-deflection ricochet, staggered-armor
  seam bonus, conveyors/pipes, weapon-chain shapes) — cosmetic/low-impact, safe to cut
  from a demo scope

---

## Development history

**2026-03-26 — Initial commit.** Started as a submarine survival game.

**2026-03-30 to 04-02 — Space conversion + first combat/building pass.** Converted
the whole game from submarine to space theme, built the first combat system and
building overhaul, then an architecture cleanup (recoil system, entity limits,
combat consolidation). Went quiet for ~3 months after this.

**2026-07-11 to 07-12 — Picked back up.** Bounty contract system, AI ship overhaul,
made the GitHub repo public (`sangaley/depths_below`, All Rights Reserved license —
kept commercial-friendly on purpose, not open source).

**2026-07-14 — Audio + weapon tuning, space-conversion merged to master.** First
sound system (event-driven audio plugin, curated CC0/CC-BY set). Weapon tuning:
stat-only customization with power as the spend currency. Ammo behaviors made real
(penetration, AoE, EMP, burn, HESH spall), heat added as the counterweight to maxed
tuning.

**2026-07-15 — Destruction + salvage systems, ART_BRIEF.md.** Destruction: module
cook-offs/ammo fires → block debris + death rattle → impulse/mass-physics shockwaves
→ forensic wrecks (kill method shapes the loot) → decompression vent-thrust and
drunken death spirals. Salvage: loot identity (per-faction tables), crew EVA
dismantling (Cosmoteer-style — claim a block, fly out, dismantle, haul cargo home),
Breaker Drill (contact salvage), scavenger ships contesting wrecks. Sent the art
brief to an outside artist this same day.

**2026-07-16 — Economy, build QoL, repair/construction, ship layout rework.**
Economy: per-item selling, buy side, market shortages, hiring board. Build QoL:
drag-paint hull placement, real undo (Ctrl+Z), copy/paste, full controller support.
Repair & construction: idle crew field-repair, ghost rebuild (ship remembers queued
changes and rebuilds them in flight). Ship layout rework: Blueprint v2 became the
one canonical ship design format; AI faction ships moved to spawning from design
files instead of hardcoded layouts.

**2026-07-17 — AI ships became real opponents, not scenery.** Projectiles gained
real ownership (AI shots can hit OTHER AI ships, not just the player). AI target
selection by size/firepower/distance with target stickiness (fixed target
flip-flopping). Real attacker tracking so retaliation targets whoever actually hit
you. Per-faction weapon loadout pass across all 10 factions. AI ships gained real
crew and power simulation — unstaffed AI weapons now produce nothing, same as the
player's own crew rules. Four real bugs found and fixed through iterative
playtesting this session alone (retaliation priority, target flip-flop, stale aim,
wrong hit-radius).

**2026-07-18 — Bug fixes + UI/UX pass.** Fixed AI ships not firing and shields
never taking damage in ship-vs-ship fights. Full UI consistency pass — HUD,
crew/module/hiring panels, map, build mode, docking menu all rebuilt onto the
existing theme system instead of each rolling its own colors.

**2026-07-19 — `qol-batch` merged to master.** The big one: 61 commits landed in
one merge — destruction, salvage, economy, build QoL, ship layout rework, AI
crew/power simulation, and the UI pass all shipped together. This is the version
the game is at right now.

**2026-07-20 (today) — Roadmap created.** First roadmap.md written from commit
history + in-flight state; deadline set to Nov 12, then moved up to Sep 3 same day.

---

## Phase 1 — Land in-flight work + start Steam clock (Jul 20 → Jul 27, ~1 wk)

- Commit/land current uncommitted work (AI tuning, cargo HUD, new faction designs)
- Land the stashed galaxy-map work (deterministic/seeded system generation) —
  **trimmed scope: this alone, not a full warp-between-systems + map UI build-out.**
  With 6.5 weeks total, the full galaxy map from the Nov-12 plan doesn't fit
  alongside progression/onboarding, which matters more for a first playthrough.
  Recommend treating "warp to more than one system + a real map screen" as a
  post-ship feature — flag if you want it back in scope, but something else will
  have to give.
- **Register the Steam app today if possible.** App approval + store page review
  has its own lead time independent of your dev time — start this in parallel,
  not after the build is ready. Placeholder art/description is fine for the
  initial submission.
- First art assets arrive → integrate, confirm the 64→128px swap doesn't break
  multi-cell sprite sizing (known fragile area per past bug history)

## Phase 2 — Progression & onboarding (Jul 27 → Aug 10, ~2 wks)

This is the biggest real gap for a stranger's first 10 minutes with the game.
- Hull upgrade path / build-cost curve so early game isn't "everything unlocked"
- Tutorial or at least a guided first launch (controls legend exists but nothing
  teaches the loop itself)
- Balance pass on the economy/salvage loop now that it's feature-complete
- Steam page goes live for wishlists once store assets exist (can run alongside dev work)

## Phase 3 — Content & polish (Aug 10 → Aug 24, ~2 wks)

- Audio pass — core loop SFX first (combat, alerts, ambience); treat a full
  music pass as a stretch, not a requirement, given the compressed timeline
- Continue integrating art as the artist delivers batches
- Playtest-driven bugfix cycles (this project's proven working method — ship,
  get a plain-language report, root-cause, fix, reverify)
- Stealth mechanics + alert ramping: **likely cut** — nice-to-have polish, not
  a gap that breaks a first playthrough; revisit only if Phase 2 finished early

## Phase 4 — Demo hardening (Aug 24 → Sep 3, ~1.5 wks)

- Feature freeze — no new systems, bugs and balance only
- Full playthrough passes looking for progression cliffs, dead ends, crashes
- Finalize Steam store assets (screenshots, capsule art, short trailer) against
  actual in-game art
- Submit final build to Steam with buffer before Sep 3 — don't submit day-of
- Cut list: anything from Phase 3 not done by ~Aug 28 gets cut, not crunched

## Ship — September 3, 2026

Demo (or launch, if everything above finishes early) goes live on Steam.

## Post-ship backlog (not blocking Sep 3)

- Full galaxy map: multi-system warp + map UI beyond the base seeded generation
  (moved here from in-scope, given the compressed timeline)
- Tractor beam + hangar salvage (tier-3 salvage idea, already deferred once)
- Deferred module mechanics listed above (ricochet, conveyors, weapon-chain shapes)
- Full AI-vs-AI targeting parity across all 10 factions (currently 6 of 10 wired
  to general-target combat by design)
- Stealth mechanics + alert ramping, if cut from Phase 3
- Full music pass, if cut from Phase 3

---

## Decisions locked in

- **2026-07-20:** Distribution = Steam. Time budget ~20-30 hrs/week.
- **2026-07-20 (revised same day):** Deadline moved Nov 12 → Sep 3. Galaxy map
  scope walked back from "full multi-system + map UI" to "land the seeded-generation
  stash only" — flagged above, open to override if you want it back in, but
  something else in Phase 2/3 would need to move to post-ship instead.
