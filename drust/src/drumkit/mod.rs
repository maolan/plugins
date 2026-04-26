pub mod channel;
pub mod instrument;
pub mod loader;
pub mod sample;

pub use channel::Channel;
pub use instrument::{ChannelMap, Instrument};
pub use sample::{AudioFileRef, Sample};

use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct DrumKit {
    pub name: String,
    pub description: String,
    pub samplerate: u32,
    pub channels: Vec<Channel>,
    pub instruments: Vec<Instrument>,
}

impl DrumKit {
    pub fn new() -> Self {
        Self {
            samplerate: 44100,
            ..Default::default()
        }
    }

    pub fn channel_index_by_name(&self, name: &str) -> Option<usize> {
        self.channels.iter().position(|c| c.name == name)
    }
}

#[derive(Debug, Default, Clone)]
pub struct Midimap {
    pub mappings: HashMap<u8, String>,
}

impl Midimap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn instrument_for_note(&self, note: u8) -> Option<&str> {
        self.mappings.get(&note).map(|s| s.as_str())
    }
}
