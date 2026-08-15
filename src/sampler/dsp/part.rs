use crate::common::tuning::Tuning;
use crate::sampler::dsp::bus::{AuxSend, Bus};
use crate::sampler::dsp::group::Group;
use crate::sampler::dsp::zone::Zone;

#[derive(Debug, Clone)]
pub struct Part {
    pub name: String,
    pub groups: Vec<Group>,

    pub midi_channel: u8,

    pub transpose: i8,

    pub tuning: f32,

    pub gain_db: f32,

    pub pan: f32,

    pub poly_limit: usize,

    pub mpe_enabled: bool,

    pub bus: Bus,

    pub aux_sends: [AuxSend; 4],

    pub microtuning: Option<Tuning>,
}

impl Default for Part {
    fn default() -> Self {
        Self {
            name: String::new(),
            groups: Vec::new(),
            midi_channel: 255,
            transpose: 0,
            tuning: 0.0,
            gain_db: 0.0,
            pan: 0.0,
            poly_limit: 0,
            mpe_enabled: false,
            bus: Bus::default(),
            aux_sends: [AuxSend::default(); 4],
            microtuning: None,
        }
    }
}

impl Part {
    pub fn find_zone(
        &self,
        note: u8,
        velocity: u8,
        cc_values: &[u8; 128],
    ) -> Option<(usize, &Group, &Zone)> {
        for (gi, group) in self.groups.iter().enumerate() {
            if !crate::sampler::dsp::group::group_is_active(group, self, cc_values) {
                continue;
            }
            if let Some(zone) = group.find_zone(note, velocity) {
                return Some((gi, group, zone));
            }
        }
        None
    }

    pub fn handle_keyswitch_on(&mut self, note: u8) {
        let mut activated = false;
        for group in &mut self.groups {
            if group.trigger_type == crate::sampler::dsp::group::TriggerType::KeyswitchLatch
                && group.trigger_note == note
            {
                group.trigger_active = true;
                activated = true;
            }
        }

        if activated {
            for group in &mut self.groups {
                if group.trigger_type == crate::sampler::dsp::group::TriggerType::KeyswitchLatch
                    && group.trigger_note != note
                {
                    group.trigger_active = false;
                }
            }
        }

        for group in &mut self.groups {
            if group.trigger_type == crate::sampler::dsp::group::TriggerType::KeyswitchMomentary
                && group.trigger_note == note
            {
                group.trigger_active = true;
            }
        }
    }

    pub fn handle_keyswitch_off(&mut self, note: u8) {
        for group in &mut self.groups {
            if group.trigger_type == crate::sampler::dsp::group::TriggerType::KeyswitchMomentary
                && group.trigger_note == note
            {
                group.trigger_active = false;
            }
        }
    }
}
