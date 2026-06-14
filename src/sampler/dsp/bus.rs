use crate::sampler::dsp::processor::ProcessorChain;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuxTapPoint {
    PreFx,

    PostFxPreVca,

    #[default]
    PostVca,
}

#[derive(Debug, Clone, Copy)]
pub struct AuxSend {
    pub bus_index: u8,

    pub amount: f32,

    pub tap_point: AuxTapPoint,
}

impl Default for AuxSend {
    fn default() -> Self {
        Self {
            bus_index: 0,
            amount: 0.0,
            tap_point: AuxTapPoint::PostVca,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Bus {
    pub effects: ProcessorChain,

    pub gain_db: f32,

    pub pan: f32,
}

impl Default for Bus {
    fn default() -> Self {
        Self {
            effects: ProcessorChain::default(),
            gain_db: 0.0,
            pan: 0.0,
        }
    }
}

impl Bus {
    pub fn new() -> Self {
        Self::default()
    }
}
