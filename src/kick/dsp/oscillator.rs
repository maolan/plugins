use std::sync::Arc;

pub use crate::common::modulated_oscillator::{FreqEnvMode, ModulatedOscillator as Oscillator};
pub use crate::common::oscillator::{ClassicWaveform, OscType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Waveform {
    Sine = 0,
    Square = 1,
    Triangle = 2,
    Saw = 3,
    Sample = 4,
}

impl Waveform {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Waveform::Square,
            2 => Waveform::Triangle,
            3 => Waveform::Saw,
            4 => Waveform::Sample,
            _ => Waveform::Sine,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SampleBuffer {
    pub data: Arc<Vec<f32>>,
    pub sample_rate: f32,
}

impl SampleBuffer {
    pub fn new(data: Vec<f32>, sample_rate: f32) -> Self {
        Self {
            data: Arc::new(data),
            sample_rate,
        }
    }
}

pub fn waveform(osc: &Oscillator) -> Waveform {
    match osc.osc_type() {
        OscType::Sample => Waveform::Sample,
        OscType::Sine => Waveform::Sine,
        OscType::Classic => match osc.classic_waveform() {
            Some(ClassicWaveform::Square) => Waveform::Square,
            Some(ClassicWaveform::Triangle) => Waveform::Triangle,
            _ => Waveform::Saw,
        },
        _ => Waveform::Sine,
    }
}

pub fn set_waveform(osc: &mut Oscillator, waveform: Waveform) {
    match waveform {
        Waveform::Sine => osc.set_osc_type(OscType::Sine),
        Waveform::Sample => osc.set_osc_type(OscType::Sample),
        Waveform::Square => osc.set_classic_waveform(ClassicWaveform::Square),
        Waveform::Triangle => osc.set_classic_waveform(ClassicWaveform::Triangle),
        Waveform::Saw => osc.set_classic_waveform(ClassicWaveform::Saw),
    }
}

pub fn set_sample_buffer(osc: &mut Oscillator, buffer: Option<SampleBuffer>) {
    if let Some(buffer) = buffer {
        osc.set_sample_buffer(buffer.data.to_vec(), buffer.sample_rate);
    } else {
        osc.set_osc_type(OscType::Sine);
    }
}
