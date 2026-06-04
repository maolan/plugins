//! Group data model — monophonic container for zones with shared settings.

use crate::common::envelope::AdsrEnvelope;
use crate::common::lfo::Lfo;
use crate::common::voice::{PlayMode, StealMode, VoicePriority};
use crate::sampler::dsp::processor::ProcessorChain;
use crate::sampler::dsp::zone::Zone;

/// How a group is activated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TriggerType {
    /// Group is always active.
    #[default]
    None,
    /// Group activates when `trigger_note` is pressed, stays active.
    /// Deactivates when another latch group in the same part is triggered.
    KeyswitchLatch,
    /// Group activates while `trigger_note` is held, deactivates on note-off.
    KeyswitchMomentary,
    /// Group activates when a macro value matches.
    Macro,
    /// Group activates when a MIDI CC value matches.
    MidiCc,
}

/// Conjunction for combining multiple trigger conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TriggerConjunction {
    #[default]
    And,
    Or,
    AndNot,
    OrNot,
}

/// A single trigger condition.
#[derive(Debug, Clone, Copy, Default)]
pub struct TriggerCondition {
    /// Note number for keyswitch conditions.
    pub note: u8,
    /// MIDI CC number for CC conditions.
    pub cc: u8,
    /// CC value threshold (0-127).
    pub cc_value: u8,
    /// Macro ID for macro conditions.
    pub macro_id: u8,
}

/// A group holds zones and shared playback settings.
#[derive(Debug, Clone)]
pub struct Group {
    pub name: String,
    pub zones: Vec<Zone>,
    /// Polyphony limit for this group (0 = unlimited).
    pub poly_limit: usize,
    /// Play mode override (or inherit from part).
    pub play_mode: Option<PlayMode>,
    /// Voice priority.
    pub voice_priority: VoicePriority,
    /// Voice stealing strategy.
    pub steal_mode: StealMode,
    /// Exclusive group ID: non-zero chokes other voices in same group on note-on.
    pub exclusive_group: u8,
    /// Portamento time in seconds.
    pub portamento: f32,
    /// Portamento curve.
    pub portamento_curve: u8,
    /// Group output gain (dB).
    pub gain_db: f32,
    /// Group pan.
    pub pan: f32,
    /// How this group is activated.
    pub trigger_type: TriggerType,
    /// Trigger note for keyswitch types.
    pub trigger_note: u8,
    /// Whether this group is currently active (for trigger evaluation).
    pub trigger_active: bool,
    /// Up to 4 trigger conditions.
    pub trigger_conditions: [TriggerCondition; 4],
    /// Conjunctions between conditions (3 entries for 4 conditions).
    pub trigger_conjunctions: [TriggerConjunction; 3],
    /// Group-level insert processor chain (monophonic, runs on summed output).
    pub processor_chain: ProcessorChain,
    /// Group-level envelopes (2 EGs for group-level modulation).
    pub eg1: AdsrEnvelope,
    pub eg2: AdsrEnvelope,
    /// Group-level LFOs (4 LFOs for group-level modulation).
    pub lfo1: Lfo,
    pub lfo2: Lfo,
    pub lfo3: Lfo,
    pub lfo4: Lfo,
}

impl Default for Group {
    fn default() -> Self {
        Self {
            name: String::new(),
            zones: Vec::new(),
            poly_limit: 0,
            play_mode: None,
            voice_priority: VoicePriority::Last,
            steal_mode: StealMode::Oldest,
            exclusive_group: 0,
            portamento: 0.0,
            portamento_curve: 0,
            gain_db: 0.0,
            pan: 0.0,
            trigger_type: TriggerType::None,
            trigger_note: 0,
            trigger_active: false,
            trigger_conditions: [TriggerCondition::default(); 4],
            trigger_conjunctions: [TriggerConjunction::default(); 3],
            processor_chain: ProcessorChain::default(),
            eg1: AdsrEnvelope::new(48000.0),
            eg2: AdsrEnvelope::new(48000.0),
            lfo1: Lfo::new(48000.0),
            lfo2: Lfo::new(48000.0),
            lfo3: Lfo::new(48000.0),
            lfo4: Lfo::new(48000.0),
        }
    }
}

impl Group {
    /// Find the first zone that matches the given note and velocity.
    pub fn find_zone(&self, note: u8, velocity: u8) -> Option<&Zone> {
        self.zones.iter().find(|z| z.contains(note, velocity))
    }
}

/// Evaluate whether a group's trigger conditions are met.
/// For keyswitch types, uses `group.trigger_active`.
/// For Macro/MidiCc types, evaluates conditions dynamically against part macros and CC values.
pub fn group_is_active(
    group: &Group,
    part: &crate::sampler::dsp::part::Part,
    cc_values: &[u8; 128],
) -> bool {
    match group.trigger_type {
        TriggerType::None => true,
        TriggerType::KeyswitchLatch | TriggerType::KeyswitchMomentary => group.trigger_active,
        TriggerType::Macro => evaluate_conditions(group, part, cc_values),
        TriggerType::MidiCc => evaluate_conditions(group, part, cc_values),
    }
}

fn evaluate_conditions(
    group: &Group,
    part: &crate::sampler::dsp::part::Part,
    cc_values: &[u8; 128],
) -> bool {
    let mut results = [false; 4];
    for (i, result) in results.iter_mut().enumerate() {
        let cond = &group.trigger_conditions[i];
        *result = match group.trigger_type {
            TriggerType::Macro => {
                let macro_idx = cond.macro_id as usize % 16;
                let macro_val = part.macros[macro_idx].normalized_value();
                // Condition is met if macro value > 0.5 (above midpoint).
                macro_val > 0.5
            }
            TriggerType::MidiCc => {
                let cc_val = cc_values[cond.cc as usize % 128];
                // Condition is met if CC value >= threshold.
                cc_val >= cond.cc_value
            }
            _ => false,
        };
    }

    // Apply conjunctions: cond0 conj0 cond1 conj1 cond2 conj2 cond3
    let mut active = results[0];
    for i in 0..3 {
        active = match group.trigger_conjunctions[i] {
            TriggerConjunction::And => active && results[i + 1],
            TriggerConjunction::Or => active || results[i + 1],
            TriggerConjunction::AndNot => active && !results[i + 1],
            TriggerConjunction::OrNot => active || !results[i + 1],
        };
    }
    active
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::dsp::part::Part;

    #[test]
    fn test_group_is_active_none() {
        let group = Group::default();
        let part = Part::default();
        let cc = [0u8; 128];
        assert!(group_is_active(&group, &part, &cc));
    }

    #[test]
    fn test_group_is_active_keyswitch() {
        let mut group = Group {
            trigger_type: TriggerType::KeyswitchLatch,
            trigger_active: false,
            ..Default::default()
        };
        let part = Part::default();
        let cc = [0u8; 128];
        assert!(!group_is_active(&group, &part, &cc));
        group.trigger_active = true;
        assert!(group_is_active(&group, &part, &cc));
    }

    #[test]
    fn test_group_macro_trigger() {
        let mut group = Group {
            trigger_type: TriggerType::Macro,
            ..Default::default()
        };
        group.trigger_conditions[0].macro_id = 0;
        let mut part = Part::default();
        let cc = [0u8; 128];
        // Macro 0 at 0.0 (normalized 0.0) -> inactive.
        assert!(!group_is_active(&group, &part, &cc));
        // Macro 0 at 1.0 (normalized 1.0) -> active.
        part.macros[0].value = 1.0;
        assert!(group_is_active(&group, &part, &cc));
    }

    #[test]
    fn test_group_midi_cc_trigger() {
        let mut group = Group {
            trigger_type: TriggerType::MidiCc,
            ..Default::default()
        };
        group.trigger_conditions[0].cc = 10;
        group.trigger_conditions[0].cc_value = 64;
        let part = Part::default();
        let mut cc = [0u8; 128];
        // CC10 at 63 (< 64) -> inactive.
        cc[10] = 63;
        assert!(!group_is_active(&group, &part, &cc));
        // CC10 at 64 (>= 64) -> active.
        cc[10] = 64;
        assert!(group_is_active(&group, &part, &cc));
    }

    #[test]
    fn test_trigger_conjunction_and() {
        let mut group = Group {
            trigger_type: TriggerType::Macro,
            ..Default::default()
        };
        group.trigger_conditions[0].macro_id = 0;
        group.trigger_conditions[1].macro_id = 1;
        group.trigger_conjunctions[0] = TriggerConjunction::And;
        let mut part = Part::default();
        let cc = [0u8; 128];
        part.macros[0].value = 1.0;
        part.macros[1].value = 0.0;
        // First true, second false -> AND = false.
        assert!(!group_is_active(&group, &part, &cc));
        part.macros[1].value = 1.0;
        assert!(group_is_active(&group, &part, &cc));
    }

    #[test]
    fn test_trigger_conjunction_or() {
        let mut group = Group {
            trigger_type: TriggerType::Macro,
            ..Default::default()
        };
        group.trigger_conditions[0].macro_id = 0;
        group.trigger_conditions[1].macro_id = 1;
        group.trigger_conjunctions[0] = TriggerConjunction::Or;
        let mut part = Part::default();
        let cc = [0u8; 128];
        part.macros[0].value = 1.0;
        part.macros[1].value = 0.0;
        // First true, second false -> OR = true.
        assert!(group_is_active(&group, &part, &cc));
        part.macros[0].value = 0.0;
        assert!(!group_is_active(&group, &part, &cc));
    }
}
