//! Part data model — MIDI channel container for groups.

use crate::common::macro_param::{MacroParam, default_macros};
use crate::common::tuning::Tuning;
use crate::sampler::dsp::bus::{AuxSend, Bus};
use crate::sampler::dsp::group::Group;
use crate::sampler::dsp::zone::Zone;

/// A part receives MIDI on a specific channel and routes to its groups.
#[derive(Debug, Clone)]
pub struct Part {
    pub name: String,
    pub groups: Vec<Group>,
    /// MIDI channel (0-15) or 255 for OMNI.
    pub midi_channel: u8,
    /// Global transpose in semitones.
    pub transpose: i8,
    /// Fine tuning in cents.
    pub tuning: f32,
    /// Output level (dB).
    pub gain_db: f32,
    /// Output pan.
    pub pan: f32,
    /// Master polyphony limit (0 = unlimited).
    pub poly_limit: usize,
    /// MPE enabled.
    pub mpe_enabled: bool,
    /// 16 user-defined macros for this part.
    pub macros: [MacroParam; 16],
    /// Part bus with up to 4 insert effects.
    pub bus: Bus,
    /// Up to 4 aux sends from this part.
    pub aux_sends: [AuxSend; 4],
    /// Optional microtuning (defaults to 12-TET).
    pub microtuning: Option<Tuning>,
}

impl Default for Part {
    fn default() -> Self {
        Self {
            name: String::new(),
            groups: Vec::new(),
            midi_channel: 255, // OMNI
            transpose: 0,
            tuning: 0.0,
            gain_db: 0.0,
            pan: 0.0,
            poly_limit: 0,
            mpe_enabled: false,
            macros: default_macros(),
            bus: Bus::default(),
            aux_sends: [AuxSend::default(); 4],
            microtuning: None,
        }
    }
}

impl Part {
    /// Find a group and zone that matches the given note/velocity.
    /// Skips groups that are inactive due to trigger conditions.
    pub fn find_zone(
        &self,
        note: u8,
        velocity: u8,
        cc_values: &[u8; 128],
    ) -> Option<(usize, &Group, &Zone)> {
        for (gi, group) in self.groups.iter().enumerate() {
            // Skip inactive triggered groups.
            if !crate::sampler::dsp::group::group_is_active(group, self, cc_values) {
                continue;
            }
            if let Some(zone) = group.find_zone(note, velocity) {
                return Some((gi, group, zone));
            }
        }
        None
    }

    /// Handle a keyswitch note-on. Activates/deactivates latch groups.
    pub fn handle_keyswitch_on(&mut self, note: u8) {
        // Activate any latch group with matching trigger note.
        let mut activated = false;
        for group in &mut self.groups {
            if group.trigger_type == crate::sampler::dsp::group::TriggerType::KeyswitchLatch
                && group.trigger_note == note
            {
                group.trigger_active = true;
                activated = true;
            }
        }
        // If a latch group was activated, deactivate all other latch groups.
        if activated {
            for group in &mut self.groups {
                if group.trigger_type == crate::sampler::dsp::group::TriggerType::KeyswitchLatch
                    && group.trigger_note != note
                {
                    group.trigger_active = false;
                }
            }
        }
        // Activate momentary groups with matching trigger note.
        for group in &mut self.groups {
            if group.trigger_type == crate::sampler::dsp::group::TriggerType::KeyswitchMomentary
                && group.trigger_note == note
            {
                group.trigger_active = true;
            }
        }
    }

    /// Handle a keyswitch note-off. Deactivates momentary groups.
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
