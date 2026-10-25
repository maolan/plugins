use crate::common::envelope::{AdsrEnvelope, AdsrParams};
use crate::common::filter::FilterParams;
use crate::common::lfo::Lfo;
use crate::common::voice::{PlayMode, StealMode, VoicePriority};
use crate::sampler::dsp::mod_matrix::ModMatrix;
use crate::sampler::dsp::processor::ProcessorChain;
use crate::sampler::dsp::voice::LfoParams;
use crate::sampler::dsp::zone::Zone;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TriggerType {
    #[default]
    None,

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

    pub extra_sfz_opcodes: Vec<(String, String)>,

    pub output: u16,

    pub trigger_type: TriggerType,

    pub trigger_conditions: [TriggerCondition; 4],

    pub trigger_conjunctions: [TriggerConjunction; 3],

    pub sw_last: Option<u8>,

    pub sw_down: Option<u8>,

    pub sw_up: Option<u8>,

    pub sw_previous: Option<u8>,

    pub sw_lolast: Option<u8>,

    pub sw_hilast: Option<u8>,

    pub sw_default: Option<u8>,

    pub sw_label: Option<String>,

    pub processor_chain: ProcessorChain,

    pub eg1: AdsrEnvelope,
    pub eg2: AdsrEnvelope,

    pub lfo1: Lfo,
    pub lfo2: Lfo,
    pub lfo3: Lfo,
    pub lfo4: Lfo,

    pub eg1_params: Option<AdsrParams>,
    pub eg2_params: Option<AdsrParams>,

    pub lfo1_params: Option<LfoParams>,
    pub lfo2_params: Option<LfoParams>,
    pub lfo3_params: Option<LfoParams>,
    pub lfo4_params: Option<LfoParams>,

    pub filter_params: Option<FilterParams>,

    pub mod_matrix: ModMatrix,

    pub master_gain_db: f32,

    pub master_pan: f32,

    pub master_tuning: f32,
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
            extra_sfz_opcodes: Vec::new(),
            output: 0,
            trigger_type: TriggerType::None,
            trigger_conditions: [TriggerCondition::default(); 4],
            trigger_conjunctions: [TriggerConjunction::default(); 3],
            sw_last: None,
            sw_down: None,
            sw_up: None,
            sw_previous: None,
            sw_lolast: None,
            sw_hilast: None,
            sw_default: None,
            sw_label: None,
            processor_chain: ProcessorChain::default(),
            eg1: AdsrEnvelope::new(48000.0),
            eg2: AdsrEnvelope::new(48000.0),
            lfo1: Lfo::new(48000.0),
            lfo2: Lfo::new(48000.0),
            lfo3: Lfo::new(48000.0),
            lfo4: Lfo::new(48000.0),
            eg1_params: None,
            eg2_params: None,
            lfo1_params: None,
            lfo2_params: None,
            lfo3_params: None,
            lfo4_params: None,
            filter_params: None,
            mod_matrix: ModMatrix::default(),
            master_gain_db: 0.0,
            master_pan: 0.0,
            master_tuning: 0.0,
        }
    }
}

impl Group {
    pub fn find_zone(
        &self,
        note: u8,
        velocity: u8,
        channel: u8,
        cc_values: &[u8; 128],
        pitch_bend_raw: i16,
    ) -> Option<&Zone> {
        self.zones.iter().find(|zone| {
            zone.contains_with_context(note, velocity, channel, cc_values, pitch_bend_raw)
        })
    }
}

pub fn group_is_active(
    group: &Group,
    part: &crate::sampler::dsp::part::Part,
    cc_values: &[u8; 128],
    held_notes: &[bool; 128],
) -> bool {
    let mut active = true;

    if group.sw_last.is_some()
        || group.sw_down.is_some()
        || group.sw_up.is_some()
        || group.sw_previous.is_some()
        || group.sw_lolast.is_some()
        || group.sw_hilast.is_some()
    {
        active = keyswitch_active(group, part, held_notes);
    }

    if !active {
        return false;
    }

    match group.trigger_type {
        TriggerType::None => active,
        TriggerType::MidiCc => evaluate_conditions(group, part, cc_values),
    }
}

fn keyswitch_active(
    group: &Group,
    part: &crate::sampler::dsp::part::Part,
    held_notes: &[bool; 128],
) -> bool {
    if let Some(note) = group.sw_down
        && !held_notes[note as usize]
    {
        return false;
    }

    if let Some(note) = group.sw_up
        && held_notes[note as usize]
    {
        return false;
    }

    if let Some(note) = group.sw_last
        && part.last_keyswitch_note != Some(note)
    {
        return false;
    }

    if group.sw_lolast.is_some() || group.sw_hilast.is_some() {
        let lo = group.sw_lolast.unwrap_or(0);
        let hi = group.sw_hilast.unwrap_or(127);
        if let Some(last) = part.last_keyswitch_note {
            if last < lo || last > hi {
                return false;
            }
        } else {
            return false;
        }
    }

    if let Some(note) = group.sw_previous
        && part.previous_note != Some(note)
    {
        return false;
    }

    true
}

fn evaluate_conditions(
    group: &Group,
    _part: &crate::sampler::dsp::part::Part,
    cc_values: &[u8; 128],
) -> bool {
    let mut results = [false; 4];
    for (i, result) in results.iter_mut().enumerate() {
        let cond = &group.trigger_conditions[i];
        *result = match group.trigger_type {
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
        let held = [false; 128];
        assert!(group_is_active(&group, &part, &cc, &held));
    }

    #[test]
    fn test_group_is_active_keyswitch_last() {
        let group = Group {
            sw_last: Some(24),
            ..Default::default()
        };
        let mut part = Part::default();
        let cc = [0u8; 128];
        let held = [false; 128];
        assert!(!group_is_active(&group, &part, &cc, &held));
        part.last_keyswitch_note = Some(24);
        assert!(group_is_active(&group, &part, &cc, &held));
    }

    #[test]
    fn test_group_is_active_keyswitch_range() {
        let group = Group {
            sw_lolast: Some(24),
            sw_hilast: Some(26),
            ..Default::default()
        };
        let mut part = Part::default();
        let cc = [0u8; 128];
        let held = [false; 128];
        part.last_keyswitch_note = Some(25);
        assert!(group_is_active(&group, &part, &cc, &held));
        part.last_keyswitch_note = Some(27);
        assert!(!group_is_active(&group, &part, &cc, &held));
    }

    #[test]
    fn test_group_is_active_keyswitch_previous() {
        let group = Group {
            sw_previous: Some(60),
            ..Default::default()
        };
        let mut part = Part::default();
        let cc = [0u8; 128];
        let held = [false; 128];
        assert!(!group_is_active(&group, &part, &cc, &held));
        part.previous_note = Some(60);
        assert!(group_is_active(&group, &part, &cc, &held));
    }

    #[test]
    fn test_group_is_active_keyswitch_down_and_up() {
        let down_group = Group {
            sw_down: Some(24),
            ..Default::default()
        };
        let up_group = Group {
            sw_up: Some(24),
            ..Default::default()
        };
        let part = Part::default();
        let cc = [0u8; 128];
        let mut held = [false; 128];
        assert!(!group_is_active(&down_group, &part, &cc, &held));
        assert!(group_is_active(&up_group, &part, &cc, &held));
        held[24] = true;
        assert!(group_is_active(&down_group, &part, &cc, &held));
        assert!(!group_is_active(&up_group, &part, &cc, &held));
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
        let held = [false; 128];

        cc[10] = 63;
        assert!(!group_is_active(&group, &part, &cc, &held));

        cc[10] = 64;
        assert!(group_is_active(&group, &part, &cc, &held));
    }

    #[test]
    fn test_trigger_conjunction_and() {
        let mut group = Group {
            trigger_type: TriggerType::MidiCc,
            ..Default::default()
        };
        group.trigger_conditions[0].cc = 10;
        group.trigger_conditions[0].cc_value = 64;
        group.trigger_conditions[1].cc = 11;
        group.trigger_conditions[1].cc_value = 64;
        group.trigger_conjunctions[0] = TriggerConjunction::And;
        let part = Part::default();
        let mut cc = [0u8; 128];
        let held = [false; 128];
        cc[10] = 64;

        assert!(!group_is_active(&group, &part, &cc, &held));
        cc[11] = 64;
        assert!(group_is_active(&group, &part, &cc, &held));
    }

    #[test]
    fn test_trigger_conjunction_or() {
        let mut group = Group {
            trigger_type: TriggerType::MidiCc,
            ..Default::default()
        };
        group.trigger_conditions[0].cc = 10;
        group.trigger_conditions[0].cc_value = 64;
        group.trigger_conditions[1].cc = 11;
        group.trigger_conditions[1].cc_value = 64;
        group.trigger_conjunctions[0] = TriggerConjunction::Or;
        let part = Part::default();
        let mut cc = [0u8; 128];
        let held = [false; 128];
        cc[10] = 64;

        assert!(group_is_active(&group, &part, &cc, &held));
        cc[10] = 0;
        assert!(!group_is_active(&group, &part, &cc, &held));
    }
}
