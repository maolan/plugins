//! Patch data model — top-level container with multiple parts.

use crate::sampler::dsp::bus::Bus;
use crate::sampler::dsp::part::Part;

/// A patch is the top-level preset containing up to 16 parts.
#[derive(Debug, Clone)]
pub struct Patch {
    pub name: String,
    pub parts: Vec<Part>,
    /// Main output bus.
    pub main_bus: Bus,
    /// 4 auxiliary busses.
    pub aux_busses: [Bus; 4],
}

impl Default for Patch {
    fn default() -> Self {
        // Default patch has one empty part.
        Self {
            name: String::from("Init"),
            parts: vec![Part::default()],
            main_bus: Bus::default(),
            aux_busses: std::array::from_fn(|_| Bus::default()),
        }
    }
}

impl Patch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Find a part that responds to the given MIDI channel.
    pub fn find_part(&self, channel: u8) -> Option<(usize, &Part)> {
        for (i, part) in self.parts.iter().enumerate() {
            if part.midi_channel == 255 || part.midi_channel == channel {
                return Some((i, part));
            }
        }
        None
    }

    /// Find a mutable part that responds to the given MIDI channel.
    pub fn find_part_mut(&mut self, channel: u8) -> Option<(usize, &mut Part)> {
        for (i, part) in self.parts.iter_mut().enumerate() {
            if part.midi_channel == 255 || part.midi_channel == channel {
                return Some((i, part));
            }
        }
        None
    }
}
