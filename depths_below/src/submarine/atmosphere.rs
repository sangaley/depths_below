use bevy::prelude::*;
use rand::Rng;

use crate::events::{ShowNotification, NotificationType};
use crate::resources::DepthState;

/// Tracks timing for atmospheric events
#[derive(Resource)]
pub struct AtmosphereState {
    pub timer: Timer,
    pub base_interval: f32,
    pub min_interval: f32,
}

impl Default for AtmosphereState {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(30.0, TimerMode::Once),
            base_interval: 30.0,
            min_interval: 8.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum AtmosphericEventType {
    HullCreaking,
    RadarGhost,
    InstrumentGlitch,
    HullBump,
    CosmicFlash,
    LightsFlicker,
}

impl AtmosphericEventType {
    /// Minimum depth required for this event to trigger
    fn min_depth(&self) -> f32 {
        match self {
            Self::HullCreaking => 0.0,
            Self::RadarGhost => 200.0,
            Self::InstrumentGlitch => 500.0,
            Self::HullBump => 300.0,
            Self::CosmicFlash => 500.0,
            Self::LightsFlicker => 1000.0,
        }
    }

    /// Weight for random selection (deeper events are rarer at their unlock depth)
    fn weight(&self) -> f32 {
        match self {
            Self::HullCreaking => 3.0,
            Self::RadarGhost => 2.0,
            Self::InstrumentGlitch => 1.5,
            Self::HullBump => 2.0,
            Self::CosmicFlash => 1.5,
            Self::LightsFlicker => 1.0,
        }
    }

    fn notification_type(&self) -> NotificationType {
        match self {
            Self::HullCreaking => NotificationType::Warning,
            Self::RadarGhost => NotificationType::Warning,
            Self::InstrumentGlitch => NotificationType::Warning,
            Self::HullBump => NotificationType::Danger,
            Self::CosmicFlash => NotificationType::Info,
            Self::LightsFlicker => NotificationType::Warning,
        }
    }

    fn random_message(&self, rng: &mut impl Rng) -> &'static str {
        match self {
            Self::HullCreaking => match rng.gen_range(0..4) {
                0 => "The hull groans under radiation stress...",
                1 => "Metal creaks ominously around you.",
                2 => "A deep, resonant groan echoes through the hull.",
                _ => "The bulkheads shudder with a low creak.",
            },
            Self::RadarGhost => match rng.gen_range(0..4) {
                0 => "Radar picks up a faint contact... then nothing.",
                1 => "A phantom blip appears on radar and vanishes.",
                2 => "Radar echo returns something massive... probably an asteroid.",
                _ => "Brief radar contact — too fast to identify.",
            },
            Self::InstrumentGlitch => match rng.gen_range(0..3) {
                0 => "Navigation instruments flicker momentarily.",
                1 => "Distance gauge spikes, then returns to normal.",
                _ => "Compass spins wildly for a second, then stabilizes.",
            },
            Self::HullBump => match rng.gen_range(0..4) {
                0 => "Something bumps against the hull!",
                1 => "A heavy thud reverberates through the ship.",
                2 => "Impact detected — external contact on the starboard side.",
                _ => "The ship shudders from an unseen collision.",
            },
            Self::CosmicFlash => match rng.gen_range(0..3) {
                0 => "A cascade of cosmic energy drifts past the viewport.",
                1 => "Strange luminous particles pulse in the darkness outside.",
                _ => "The void shimmers with an eerie blue-green light.",
            },
            Self::LightsFlicker => match rng.gen_range(0..3) {
                0 => "Interior lights flicker and dim briefly.",
                1 => "The lights cut out for a heartbeat, then return.",
                _ => "Electrical systems stutter — lights blink twice.",
            },
        }
    }
}

const ALL_EVENTS: [AtmosphericEventType; 6] = [
    AtmosphericEventType::HullCreaking,
    AtmosphericEventType::RadarGhost,
    AtmosphericEventType::InstrumentGlitch,
    AtmosphericEventType::HullBump,
    AtmosphericEventType::CosmicFlash,
    AtmosphericEventType::LightsFlicker,
];

pub fn atmospheric_event_system(
    time: Res<Time>,
    depth: Res<DepthState>,
    mut state: ResMut<AtmosphereState>,
    mut notifications: EventWriter<ShowNotification>,
) {
    state.timer.tick(time.delta());

    if !state.timer.just_finished() {
        return;
    }

    let mut rng = rand::thread_rng();
    let current_depth = depth.current_depth;

    // Collect eligible events and their weights
    let eligible: Vec<(AtmosphericEventType, f32)> = ALL_EVENTS
        .iter()
        .filter(|e| current_depth >= e.min_depth())
        .map(|e| (*e, e.weight()))
        .collect();

    if let Some(event) = weighted_pick(&eligible, &mut rng) {
        let message = event.random_message(&mut rng);
        notifications.send(ShowNotification {
            message: message.to_string(),
            notification_type: event.notification_type(),
            duration: 4.0,
        });
    }

    // Scale interval by depth: lerp from base_interval to min_interval
    let depth_factor = (current_depth / 2000.0).clamp(0.0, 1.0);
    let interval = state.base_interval + (state.min_interval - state.base_interval) * depth_factor;
    state.timer = Timer::from_seconds(interval, TimerMode::Once);
}

fn weighted_pick(
    items: &[(AtmosphericEventType, f32)],
    rng: &mut impl Rng,
) -> Option<AtmosphericEventType> {
    if items.is_empty() {
        return None;
    }
    let total: f32 = items.iter().map(|(_, w)| w).sum();
    let mut roll = rng.gen_range(0.0..total);
    for (event, weight) in items {
        roll -= weight;
        if roll <= 0.0 {
            return Some(*event);
        }
    }
    Some(items.last().unwrap().0)
}
