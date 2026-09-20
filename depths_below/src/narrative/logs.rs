//! The log corpus, and which of it a place is allowed to hold.
//!
//! These were previously gated on `depth_level`, a number the chunk layer
//! derived from world Y. That layer only generates in a band roughly -5,600
//! to +500 in Y, and it caps `depth_level` at 11 — so nine of the twenty-three
//! entries, the whole finale tier, could never spawn anywhere, and
//! `check_victory` asked for one of them. Every other star system centres up
//! to 1,500,000 below that band, so outside Haven no log appeared at all.
//!
//! Tier replaces depth. It comes from how dangerous a system's faction is
//! (`faction_power`), which is already the galaxy's authored difficulty ramp:
//! weak factions near Haven, the worst ones at the edge. Going further out is
//! what advances the story, which is the same axis progression already uses.

/// One entry. Tier 0 is a quiet system near Haven; tier 3 is the far edge.
pub struct LogEntryDef {
    pub tier: u8,
    pub title: &'static str,
    pub text: &'static str,
}

/// Highest tier that exists. Tier 3 holds the finale.
pub const MAX_TIER: u8 = 3;

/// Danger tier of a system (`StarSystemDef.danger_tier`, i.e. `faction_power`,
/// 0.1..8.0) to a log tier. Haven and any unclaimed system are tier 0.
///
/// Cuts put The Silence and the Recursive Kingdom at 0; the Broken Choir,
/// Stellar Preserve and Synthesis Collective at 1; the Gilded Throne, Corpse
/// Stars and Terran Hegemony at 2; and the two uniques at 3.
pub fn tier_for_danger(danger: f32) -> u8 {
    if danger < 1.0 { 0 } else if danger < 2.0 { 1 } else if danger < 4.0 { 2 } else { 3 }
}

pub const LOG_ENTRIES: &[LogEntryDef] = &[
    // ---- tier 0 ----
    LogEntryDef { tier: 0, title: "Expedition Log #1", text: "Day 3: We've pushed past the asteroid fields. Radar shows massive structures ahead. Not natural formations." },
    LogEntryDef { tier: 0, title: "Recovered Note", text: "To whoever finds this: the company lied about what's out here. Turn back. The station has forgotten this sector for good reason." },
    LogEntryDef { tier: 0, title: "Ship's Log: CSS Meridian", text: "Engine failure at sector 180. Hull compromised. Three crew missing since last night. Nobody heard them leave." },
    LogEntryDef { tier: 0, title: "Expedition Log #2", text: "Day 7: Found wreckage of a previous expedition. Their hull was breached from the INSIDE. What could do that?" },
    LogEntryDef { tier: 0, title: "Research Note: Acoustics", text: "We've been recording infrasound from deeper in the void. When played back at normal speed, it sounds like breathing." },
    LogEntryDef { tier: 0, title: "Distress Signal (Decoded)", text: "MAYDAY MAYDAY. Something is following us. It matches our speed exactly. It's been three days. It never gets closer, never falls behind." },
    LogEntryDef { tier: 0, title: "Research Note: Luminescence", text: "The creatures here don't just glow - they communicate with light. Patterns too complex to be random. Are they... words?" },
    // ---- tier 1 ----
    LogEntryDef { tier: 1, title: "Expedition Log #3", text: "Day 12: The ruins are older than anything at the station. Carved metal at sector 800. Impossible engineering. The carvings depict... us. Ships. How?" },
    LogEntryDef { tier: 1, title: "Personal Journal: Dr. Vasquez", text: "The symbols match nothing in any database. But I dream about them now. In the dreams, I can read them perfectly. I just can't remember what they say when I wake." },
    LogEntryDef { tier: 1, title: "Engineering Report", text: "Hull sensors report external contact - something is running along the hull. Like fingers. There's nothing on radar." },
    LogEntryDef { tier: 1, title: "Audio Transcript #47", text: "RESEARCHER: The artifact we recovered - it's warm to the touch. CAPTAIN: That's impossible in the void. RESEARCHER: I know. And it's getting warmer." },
    LogEntryDef { tier: 1, title: "Warning Beacon", text: "AUTOMATED MESSAGE: Do not proceed past sector 1000. Repeat: DO NOT proceed. The watchers are not what they seem." },
    // ---- tier 2 ----
    LogEntryDef { tier: 2, title: "Expedition Log #4", text: "Day 18: We can hear it now. A low hum from deeper in. The instruments say nothing is there, but we can all hear it. Chen says it's trying to communicate." },
    LogEntryDef { tier: 2, title: "Recovered Black Box", text: "Last words of the crew of the DSV Orpheus: 'It opened its eyes. Oh god, the whole void opened its eyes.'" },
    LogEntryDef { tier: 2, title: "Research Note: Evolution", text: "These creatures didn't evolve to live here. They evolved somewhere else and were... placed here. Like prisoners. Or guards." },
    LogEntryDef { tier: 2, title: "Fragment: Ancient Text", text: "Translation (partial): '...and in the deep void we built our prisons, for what slumbers must never dream of the worlds above...'" },
    LogEntryDef { tier: 2, title: "Personal Log: Unknown Author", text: "Day ??? The compass doesn't work anymore. Neither does time. My watch says it's been 3 hours. My body says weeks. I can feel the hum in my teeth." },
    LogEntryDef { tier: 2, title: "Radio Intercept", text: "Station control, this is Deep Outpost Seven. We are NOT alone out here. I don't mean the creatures. Something is watching through them. Request immediate extraction." },
    // ---- tier 3 ----
    LogEntryDef { tier: 3, title: "Final Transmission", text: "They built this place to contain something. The ruins aren't ruins - they're a cage. And it's waking up." },
    LogEntryDef { tier: 3, title: "Carved Metal (Translated)", text: "WE WHO GUARD THE DEEP VOID WARN YOU: WHAT SLEEPS BEYOND DREAMS OF YOUR WORLDS. DO NOT WAKE IT. DO NOT LISTEN TO ITS SONGS." },
    LogEntryDef { tier: 3, title: "???", text: "The hum has stopped. That's worse. That's so much worse." },
    LogEntryDef { tier: 3, title: "Final Entry", text: "We were wrong about everything. The void isn't hostile. It's terrified. Space itself is trying to keep us away from what lies beyond." },
    LogEntryDef { tier: 3, title: "[UNTITLED]", text: "You found it. The deepest point. The silence is absolute. The void itself seems alive. You understand now - you were always meant to come here. It was always going to be you." },];

/// Pick one entry allowed at `tier`, chosen by `key` so a given place always
/// yields the same log. Falls back to lower tiers, so an early system is never
/// silent just because its own band ran dry.
pub fn pick_log(tier: u8, key: u64) -> Option<&'static LogEntryDef> {
    let mut t = tier.min(MAX_TIER) as i16;
    while t >= 0 {
        let band: Vec<&LogEntryDef> = LOG_ENTRIES.iter().filter(|e| e.tier == t as u8).collect();
        if !band.is_empty() {
            // Cheap deterministic mix — this only has to spread, not be random.
            let h = key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33;
            return Some(band[(h as usize) % band.len()]);
        }
        t -= 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every tier must be reachable. The bug this replaces was a whole band of
    /// entries that existed in the table and could never appear in the game.
    #[test]
    fn every_tier_has_entries() {
        for t in 0..=MAX_TIER {
            assert!(
                LOG_ENTRIES.iter().any(|e| e.tier == t),
                "no log entries at tier {t} — that band would be unreachable"
            );
        }
    }

    /// The danger cuts must actually land every faction somewhere, and the
    /// strongest must reach the top tier where the finale lives.
    #[test]
    fn danger_cuts_span_all_tiers() {
        use crate::ai_ship::components::{faction_power, AiShipType::*};
        let powers = [TheSilence, RecursiveKingdom, BrokenChoir, StellarPreserve,
                      SynthesisCollective, GildedThrone, CorpseStars, TerranHegemony,
                      EternalHegemony, Shepherd];
        let tiers: Vec<u8> = powers.iter().map(|f| tier_for_danger(faction_power(*f))).collect();
        for t in 0..=MAX_TIER {
            assert!(tiers.contains(&t), "no faction maps to log tier {t}");
        }
        assert_eq!(tier_for_danger(0.0), 0, "Haven (no faction) must be tier 0");
    }

    /// Same place, same log — otherwise a log changes under the player when a
    /// system reloads.
    #[test]
    fn pick_is_stable_for_a_key() {
        for key in [0u64, 1, 42, 9_999, u64::MAX] {
            let a = pick_log(2, key).map(|e| e.title);
            let b = pick_log(2, key).map(|e| e.title);
            assert_eq!(a, b);
        }
    }
}
