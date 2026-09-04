# Parked features

Complete but **intentionally disabled** features, kept out of the build so they
don't clutter (or compile into) the active tree. Nothing here is wired into
`main.rs`, so these files are **not compiled** — they may need small fixups
against the current codebase when revived.

## abyss_horror.rs
An "abyss horror" ambience layer built around real creatures watching/fleeing
the player. Disabled because creature spawning is currently off, which left it
producing false scares (phantom blips) with nothing behind them.

**To re-enable:** move `abyss_horror.rs` back to `src/`, re-add `mod abyss_horror;`
and `use abyss_horror::AbyssHorrorPlugin;` in `main.rs`, and add `AbyssHorrorPlugin`
to the plugin list (it sits by the Radar/Camera plugins). Needs creature spawning on.
