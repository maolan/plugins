use crate::common::envelope::AdsrEnvelope;
use crate::common::lfo::Lfo;
use crate::common::voice::{PlayMode, StealMode, VoicePriority};
use crate::sampler::dsp::processor::ProcessorChain;
use crate::sampler::dsp::zone::Zone;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TriggerType {
    #[default]
    None,

    KeyswitchLatch,

    KeyswitchMomentary,

    Macro,

    MidiCc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TriggerConjunction {
    #[default]
    And,
    Or,
    AndNot,
    OrNot,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TriggerCondition {
    pub note: u8,

    pub cc: u8,

    pub cc_value: u8,

    pub macro_id: u8,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub name: String,
    pub zones: Vec<Zone>,

    pub poly_limit: usize,

    pub play_mode: Option<PlayMode>,

    pub voice_priority: VoicePriority,

    pub steal_mode: StealMode,

    pub exclusive_group: u8,

    pub portamento: f32,

    pub portamento_curve: u8,

    pub gain_db: f32,

    pub pan: f32,

    pub trigger_type: TriggerType,

    pub trigger_note: u8,

    pub trigger_active: bool,

    pub trigger_conditions: [TriggerCondition; 4],

    pub trigger_conjunctions: [TriggerConjunction; 3],

    pub processor_chain: ProcessorChain,

    pub eg1: AdsrEnvelope,
    pub eg2: AdsrEnvelope,

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
    pub fn find_zone(&self, note: u8, velocity: u8) -> Option<&Zone> {
        self.zones.iter().find(|z| z.contains(note, velocity))
    }
}

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

                macro_val > 0.5
            }
            TriggerType::MidiCc => {
                let cc_val = cc_values[cond.cc as usize % 128];

                cc_val >= cond.cc_value
            }
            _ => false,
        };
    }

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

        assert!(!group_is_active(&group, &part, &cc));

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

        cc[10] = 63;
        assert!(!group_is_active(&group, &part, &cc));

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

        assert!(group_is_active(&group, &part, &cc));
        part.macros[0].value = 0.0;
        assert!(!group_is_active(&group, &part, &cc));
    }
}
