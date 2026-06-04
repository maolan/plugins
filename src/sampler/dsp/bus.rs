//! Bus effects architecture — part bus, main bus, and aux sends.

use crate::sampler::dsp::processor::ProcessorChain;

/// Where in the signal chain to tap the aux send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuxTapPoint {
    /// Pre-FX, pre-VCA (dry voice sum).
    PreFx,
    /// Post-FX, pre-VCA (after insert processors, before gain).
    PostFxPreVca,
    /// Post-VCA (final group output).
    #[default]
    PostVca,
}

/// A single aux send from a part to an aux bus.
#[derive(Debug, Clone, Copy)]
pub struct AuxSend {
    /// Which aux bus (0-3) to send to.
    pub bus_index: u8,
    /// Send level (0-1).
    pub amount: f32,
    /// Tap point in the signal chain.
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

/// A bus with up to 4 effect slots.
#[derive(Debug, Clone)]
pub struct Bus {
    /// Effect processor chain.
    pub effects: ProcessorChain,
    /// Bus output gain (dB).
    pub gain_db: f32,
    /// Bus output pan.
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
