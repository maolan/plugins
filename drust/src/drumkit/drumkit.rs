use crate::drumkit::{Channel, Instrument};

#[derive(Debug, Clone, Default)]
pub struct DrumKit {
    pub name: String,
    pub description: String,
    pub version: String,
    pub sample_rate: f32,
    pub channels: Vec<Channel>,
    pub instruments: Vec<Instrument>,
}

impl DrumKit {
    pub fn is_valid(&self) -> bool {
        !self.instruments.is_empty() && !self.channels.is_empty()
    }
}
