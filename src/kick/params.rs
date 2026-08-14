use std::sync::atomic::Ordering;

use clap_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_ENUM, CLAP_PARAM_IS_STEPPED,
    CLAP_PARAM_REQUIRES_PROCESS,
};
use portable_atomic::AtomicF64;

pub const PARAM_TYPES_PER_INSTRUMENT: usize = 124;
pub const INSTRUMENT_COUNT: usize = 16;
pub const TOTAL_PARAM_COUNT: usize = PARAM_TYPES_PER_INSTRUMENT * INSTRUMENT_COUNT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ParamType {
    HumanizerVelocity = 0,
    HumanizerTiming = 1,
    ActiveInstrument = 2,
    MasterLength = 3,
    MasterOutputGain = 4,
    MasterNoteOffDecay = 5,
    MasterNoteOffEnabled = 6,
    MasterPitchToNote = 7,
    MasterKeyMin = 8,
    MasterKeyMax = 9,
    MasterMidiChannel = 10,
    MasterMuted = 11,
    MasterSoloed = 12,
    MasterFilterType = 13,
    MasterFilterCutoff = 14,
    MasterFilterQ = 15,
    MasterDistortionType = 16,
    MasterDistortionDrive = 17,
    MasterDistortionInputLimit = 18,
    MasterDistortionOutputLimit = 19,
    MasterLimiterThreshold = 20,
    MasterLimiterRelease = 21,
    Layer0Enabled = 22,
    Layer0Amp = 23,
    Layer0FilterType = 24,
    Layer0FilterCutoff = 25,
    Layer0FilterQ = 26,
    Layer0DistortionType = 27,
    Layer0DistortionDrive = 28,
    Layer0FmRouting1 = 29,
    Layer1Enabled = 30,
    Layer1Amp = 31,
    Layer2Enabled = 32,
    Layer2Amp = 33,
    Osc0Waveform = 34,
    Osc0Freq = 35,
    Osc0Amp = 36,
    Osc0Phase = 37,
    Osc0FmAmount = 38,
    Osc0FilterType = 39,
    Osc0FilterCutoff = 40,
    Osc0FilterQ = 41,
    Osc0DistortionType = 42,
    Osc0DistortionDrive = 43,
    Osc1Waveform = 44,
    Osc1Freq = 45,
    Osc1Amp = 46,
    Osc1Phase = 47,
    Osc1FmAmount = 48,
    Osc1FilterType = 49,
    Osc1FilterCutoff = 50,
    Osc1FilterQ = 51,
    Osc1DistortionType = 52,
    Osc1DistortionDrive = 53,
    NoiseType = 54,
    NoiseAmp = 55,
    NoiseDensity = 56,
    NoiseFilterType = 57,
    NoiseFilterCutoff = 58,
    NoiseFilterQ = 59,
    Layer1FilterType = 60,
    Layer1FilterCutoff = 61,
    Layer1FilterQ = 62,
    Layer1DistortionType = 63,
    Layer1DistortionDrive = 64,
    Layer1FmRouting1 = 65,
    Layer1Osc0Waveform = 66,
    Layer1Osc0Freq = 67,
    Layer1Osc0Amp = 68,
    Layer1Osc0Phase = 69,
    Layer1Osc0FmAmount = 70,
    Layer1Osc0FilterType = 71,
    Layer1Osc0FilterCutoff = 72,
    Layer1Osc0FilterQ = 73,
    Layer1Osc0DistortionType = 74,
    Layer1Osc0DistortionDrive = 75,
    Layer1Osc1Waveform = 76,
    Layer1Osc1Freq = 77,
    Layer1Osc1Amp = 78,
    Layer1Osc1Phase = 79,
    Layer1Osc1FmAmount = 80,
    Layer1Osc1FilterType = 81,
    Layer1Osc1FilterCutoff = 82,
    Layer1Osc1FilterQ = 83,
    Layer1Osc1DistortionType = 84,
    Layer1Osc1DistortionDrive = 85,
    Layer1NoiseType = 86,
    Layer1NoiseAmp = 87,
    Layer1NoiseDensity = 88,
    Layer1NoiseFilterType = 89,
    Layer1NoiseFilterCutoff = 90,
    Layer1NoiseFilterQ = 91,
    Layer2FilterType = 92,
    Layer2FilterCutoff = 93,
    Layer2FilterQ = 94,
    Layer2DistortionType = 95,
    Layer2DistortionDrive = 96,
    Layer2FmRouting1 = 97,
    Layer2Osc0Waveform = 98,
    Layer2Osc0Freq = 99,
    Layer2Osc0Amp = 100,
    Layer2Osc0Phase = 101,
    Layer2Osc0FmAmount = 102,
    Layer2Osc0FilterType = 103,
    Layer2Osc0FilterCutoff = 104,
    Layer2Osc0FilterQ = 105,
    Layer2Osc0DistortionType = 106,
    Layer2Osc0DistortionDrive = 107,
    Layer2Osc1Waveform = 108,
    Layer2Osc1Freq = 109,
    Layer2Osc1Amp = 110,
    Layer2Osc1Phase = 111,
    Layer2Osc1FmAmount = 112,
    Layer2Osc1FilterType = 113,
    Layer2Osc1FilterCutoff = 114,
    Layer2Osc1FilterQ = 115,
    Layer2Osc1DistortionType = 116,
    Layer2Osc1DistortionDrive = 117,
    Layer2NoiseType = 118,
    Layer2NoiseAmp = 119,
    Layer2NoiseDensity = 120,
    Layer2NoiseFilterType = 121,
    Layer2NoiseFilterCutoff = 122,
    Layer2NoiseFilterQ = 123,
}

impl ParamType {
    pub const COUNT: usize = 124;

    pub fn from_raw(v: u8) -> Option<Self> {
        if v < Self::COUNT as u8 {
            Some(unsafe { std::mem::transmute::<u8, ParamType>(v) })
        } else {
            None
        }
    }

    pub fn as_index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParamId(pub u32);

impl ParamId {
    pub const COUNT: usize = TOTAL_PARAM_COUNT;

    #[inline]
    pub const fn new(instrument: u8, param_type: ParamType) -> Self {
        Self((instrument as u32) * (ParamType::COUNT as u32) + (param_type as u32))
    }

    #[inline]
    pub const fn instrument(self) -> u8 {
        (self.0 / (ParamType::COUNT as u32)) as u8
    }

    #[inline]
    pub const fn param_type(self) -> ParamType {
        let idx = (self.0 % (ParamType::COUNT as u32)) as u8;
        unsafe { std::mem::transmute(idx) }
    }

    #[inline]
    pub const fn as_index(self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub fn from_raw(id: u32) -> Option<Self> {
        if id < Self::COUNT as u32 {
            Some(Self(id))
        } else {
            None
        }
    }

    #[inline]
    pub const fn from_index(index: usize) -> Option<Self> {
        if index < Self::COUNT {
            Some(Self(index as u32))
        } else {
            None
        }
    }

    pub fn all() -> impl Iterator<Item = Self> {
        (0..Self::COUNT as u32).map(Self)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ParamTypeDef {
    pub base_name: &'static str,
    pub base_module: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub step: f64,
    pub flags: u32,
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const ENUM: u32 = CLAP_PARAM_IS_ENUM
    | CLAP_PARAM_IS_STEPPED
    | CLAP_PARAM_IS_AUTOMATABLE
    | CLAP_PARAM_REQUIRES_PROCESS;
const TOGGLE: u32 = CLAP_PARAM_IS_STEPPED | CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;

macro_rules! param_def {
    ($name:literal, $module:literal, $min:expr, $max:expr, $default:expr, $step:expr, $flags:expr) => {
        ParamTypeDef {
            base_name: $name,
            base_module: $module,
            min: $min,
            max: $max,
            default: $default,
            step: $step,
            flags: $flags,
        }
    };
}

pub const PARAM_TYPE_DEFS: [ParamTypeDef; ParamType::COUNT] = [
    param_def!(
        "Humanizer Velocity",
        "Kit",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!("Humanizer Timing", "Kit", 0.0, 50.0, 0.0, 0.1, AUTOMATABLE),
    param_def!("Active Instrument", "Kit", 0.0, 15.0, 0.0, 1.0, ENUM),
    param_def!("Length", "Master", 10.0, 4000.0, 300.0, 1.0, AUTOMATABLE),
    param_def!("Output Gain", "Master", -24.0, 24.0, 0.0, 0.1, AUTOMATABLE),
    param_def!(
        "Note-Off Decay",
        "Master",
        0.0,
        500.0,
        30.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!("Note-Off Enabled", "Master", 0.0, 1.0, 1.0, 1.0, TOGGLE),
    param_def!("Pitch to Note", "Master", 0.0, 1.0, 0.0, 1.0, TOGGLE),
    param_def!("Key Min", "Master", 0.0, 127.0, 0.0, 1.0, ENUM),
    param_def!("Key Max", "Master", 0.0, 127.0, 127.0, 1.0, ENUM),
    param_def!("MIDI Channel", "Master", 0.0, 16.0, 0.0, 1.0, ENUM),
    param_def!("Muted", "Master", 0.0, 1.0, 0.0, 1.0, TOGGLE),
    param_def!("Soloed", "Master", 0.0, 1.0, 0.0, 1.0, TOGGLE),
    param_def!("Filter Type", "Master", 0.0, 3.0, 1.0, 1.0, ENUM),
    param_def!(
        "Filter Cutoff",
        "Master",
        20.0,
        20000.0,
        20000.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!("Filter Q", "Master", 0.01, 10.0, 0.7, 0.01, AUTOMATABLE),
    param_def!("Distortion Type", "Master", 0.0, 8.0, 1.0, 1.0, ENUM),
    param_def!(
        "Distortion Drive",
        "Master",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Dist Input Limit",
        "Master",
        0.01,
        10.0,
        1.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Dist Output Limit",
        "Master",
        0.01,
        10.0,
        1.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Limiter Threshold",
        "Master",
        -24.0,
        6.0,
        0.0,
        0.1,
        AUTOMATABLE
    ),
    param_def!(
        "Limiter Release",
        "Master",
        1.0,
        500.0,
        50.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!("Layer 0 Enabled", "Layer0", 0.0, 1.0, 1.0, 1.0, TOGGLE),
    param_def!("Layer 0 Amp", "Layer0", 0.0, 1.0, 1.0, 0.01, AUTOMATABLE),
    param_def!("Layer 0 Filter Type", "Layer0", 0.0, 3.0, 1.0, 1.0, ENUM),
    param_def!(
        "Layer 0 Filter Cutoff",
        "Layer0",
        20.0,
        20000.0,
        20000.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 0 Filter Q",
        "Layer0",
        0.01,
        10.0,
        0.7,
        0.01,
        AUTOMATABLE
    ),
    param_def!("Layer 0 Dist Type", "Layer0", 0.0, 8.0, 1.0, 1.0, ENUM),
    param_def!(
        "Layer 0 Dist Drive",
        "Layer0",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!("Layer 0 FM Route 1", "Layer0", 0.0, 1.0, 0.0, 1.0, ENUM),
    param_def!("Layer 1 Enabled", "Layer1", 0.0, 1.0, 0.0, 1.0, TOGGLE),
    param_def!("Layer 1 Amp", "Layer1", 0.0, 1.0, 1.0, 0.01, AUTOMATABLE),
    param_def!("Layer 2 Enabled", "Layer2", 0.0, 1.0, 0.0, 1.0, TOGGLE),
    param_def!("Layer 2 Amp", "Layer2", 0.0, 1.0, 1.0, 0.01, AUTOMATABLE),
    param_def!("Osc 0 Waveform", "Osc0", 0.0, 4.0, 0.0, 1.0, ENUM),
    param_def!(
        "Osc 0 Freq",
        "Osc0",
        1000.0,
        20000.0,
        1000.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!("Osc 0 Amp", "Osc0", 0.0, 1.0, 1.0, 0.01, AUTOMATABLE),
    param_def!("Osc 0 Phase", "Osc0", 0.0, 180.0, 0.0, 1.0, AUTOMATABLE),
    param_def!("Osc 0 FM Amount", "Osc0", 0.0, 1.0, 0.0, 0.01, AUTOMATABLE),
    param_def!("Osc 0 Filter Type", "Osc0", 0.0, 3.0, 1.0, 1.0, ENUM),
    param_def!(
        "Osc 0 Filter Cutoff",
        "Osc0",
        20.0,
        20000.0,
        20000.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!("Osc 0 Filter Q", "Osc0", 0.01, 10.0, 0.7, 0.01, AUTOMATABLE),
    param_def!("Osc 0 Dist Type", "Osc0", 0.0, 8.0, 1.0, 1.0, ENUM),
    param_def!("Osc 0 Dist Drive", "Osc0", 0.0, 1.0, 0.0, 0.01, AUTOMATABLE),
    param_def!("Osc 1 Waveform", "Osc1", 0.0, 4.0, 0.0, 1.0, ENUM),
    param_def!(
        "Osc 1 Freq",
        "Osc1",
        1000.0,
        20000.0,
        1000.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!("Osc 1 Amp", "Osc1", 0.0, 1.0, 0.0, 0.01, AUTOMATABLE),
    param_def!("Osc 1 Phase", "Osc1", 0.0, 180.0, 0.0, 1.0, AUTOMATABLE),
    param_def!("Osc 1 FM Amount", "Osc1", 0.0, 1.0, 0.0, 0.01, AUTOMATABLE),
    param_def!("Osc 1 Filter Type", "Osc1", 0.0, 3.0, 1.0, 1.0, ENUM),
    param_def!(
        "Osc 1 Filter Cutoff",
        "Osc1",
        20.0,
        20000.0,
        20000.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!("Osc 1 Filter Q", "Osc1", 0.01, 10.0, 0.7, 0.01, AUTOMATABLE),
    param_def!("Osc 1 Dist Type", "Osc1", 0.0, 8.0, 1.0, 1.0, ENUM),
    param_def!("Osc 1 Dist Drive", "Osc1", 0.0, 1.0, 0.0, 0.01, AUTOMATABLE),
    param_def!("Noise Type", "Noise", 0.0, 2.0, 0.0, 1.0, ENUM),
    param_def!("Noise Amp", "Noise", 0.0, 1.0, 0.0, 0.01, AUTOMATABLE),
    param_def!("Noise Density", "Noise", 0.0, 1.0, 0.5, 0.01, AUTOMATABLE),
    param_def!("Noise Filter Type", "Noise", 0.0, 3.0, 1.0, 1.0, ENUM),
    param_def!(
        "Noise Filter Cutoff",
        "Noise",
        20.0,
        20000.0,
        8000.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!(
        "Noise Filter Q",
        "Noise",
        0.01,
        10.0,
        0.7,
        0.01,
        AUTOMATABLE
    ),
    param_def!("Layer 1 Filter Type", "Layer1", 0.0, 3.0, 1.0, 1.0, ENUM),
    param_def!(
        "Layer 1 Filter Cutoff",
        "Layer1",
        20.0,
        20000.0,
        20000.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Filter Q",
        "Layer1",
        0.01,
        10.0,
        0.7,
        0.01,
        AUTOMATABLE
    ),
    param_def!("Layer 1 Dist Type", "Layer1", 0.0, 8.0, 1.0, 1.0, ENUM),
    param_def!(
        "Layer 1 Dist Drive",
        "Layer1",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!("Layer 1 FM Route 1", "Layer1", 0.0, 1.0, 0.0, 1.0, ENUM),
    param_def!(
        "Layer 1 Osc 0 Waveform",
        "Layer1 Osc0",
        0.0,
        4.0,
        0.0,
        1.0,
        ENUM
    ),
    param_def!(
        "Layer 1 Osc 0 Freq",
        "Layer1 Osc0",
        1000.0,
        20000.0,
        1000.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Osc 0 Amp",
        "Layer1 Osc0",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Osc 0 Phase",
        "Layer1 Osc0",
        0.0,
        180.0,
        0.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Osc 0 FM Amount",
        "Layer1 Osc0",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Osc 0 Filter Type",
        "Layer1 Osc0",
        0.0,
        3.0,
        1.0,
        1.0,
        ENUM
    ),
    param_def!(
        "Layer 1 Osc 0 Filter Cutoff",
        "Layer1 Osc0",
        20.0,
        20000.0,
        20000.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Osc 0 Filter Q",
        "Layer1 Osc0",
        0.01,
        10.0,
        0.7,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Osc 0 Dist Type",
        "Layer1 Osc0",
        0.0,
        8.0,
        1.0,
        1.0,
        ENUM
    ),
    param_def!(
        "Layer 1 Osc 0 Dist Drive",
        "Layer1 Osc0",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Osc 1 Waveform",
        "Layer1 Osc1",
        0.0,
        4.0,
        0.0,
        1.0,
        ENUM
    ),
    param_def!(
        "Layer 1 Osc 1 Freq",
        "Layer1 Osc1",
        1000.0,
        20000.0,
        1000.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Osc 1 Amp",
        "Layer1 Osc1",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Osc 1 Phase",
        "Layer1 Osc1",
        0.0,
        180.0,
        0.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Osc 1 FM Amount",
        "Layer1 Osc1",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Osc 1 Filter Type",
        "Layer1 Osc1",
        0.0,
        3.0,
        1.0,
        1.0,
        ENUM
    ),
    param_def!(
        "Layer 1 Osc 1 Filter Cutoff",
        "Layer1 Osc1",
        20.0,
        20000.0,
        20000.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Osc 1 Filter Q",
        "Layer1 Osc1",
        0.01,
        10.0,
        0.7,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Osc 1 Dist Type",
        "Layer1 Osc1",
        0.0,
        8.0,
        1.0,
        1.0,
        ENUM
    ),
    param_def!(
        "Layer 1 Osc 1 Dist Drive",
        "Layer1 Osc1",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Noise Type",
        "Layer1 Noise",
        0.0,
        2.0,
        0.0,
        1.0,
        ENUM
    ),
    param_def!(
        "Layer 1 Noise Amp",
        "Layer1 Noise",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Noise Density",
        "Layer1 Noise",
        0.0,
        1.0,
        0.5,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Noise Filter Type",
        "Layer1 Noise",
        0.0,
        3.0,
        1.0,
        1.0,
        ENUM
    ),
    param_def!(
        "Layer 1 Noise Filter Cutoff",
        "Layer1 Noise",
        20.0,
        20000.0,
        8000.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 1 Noise Filter Q",
        "Layer1 Noise",
        0.01,
        10.0,
        0.7,
        0.01,
        AUTOMATABLE
    ),
    param_def!("Layer 2 Filter Type", "Layer2", 0.0, 3.0, 1.0, 1.0, ENUM),
    param_def!(
        "Layer 2 Filter Cutoff",
        "Layer2",
        20.0,
        20000.0,
        20000.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Filter Q",
        "Layer2",
        0.01,
        10.0,
        0.7,
        0.01,
        AUTOMATABLE
    ),
    param_def!("Layer 2 Dist Type", "Layer2", 0.0, 8.0, 1.0, 1.0, ENUM),
    param_def!(
        "Layer 2 Dist Drive",
        "Layer2",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!("Layer 2 FM Route 1", "Layer2", 0.0, 1.0, 0.0, 1.0, ENUM),
    param_def!(
        "Layer 2 Osc 0 Waveform",
        "Layer2 Osc0",
        0.0,
        4.0,
        0.0,
        1.0,
        ENUM
    ),
    param_def!(
        "Layer 2 Osc 0 Freq",
        "Layer2 Osc0",
        1000.0,
        20000.0,
        1000.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Osc 0 Amp",
        "Layer2 Osc0",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Osc 0 Phase",
        "Layer2 Osc0",
        0.0,
        180.0,
        0.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Osc 0 FM Amount",
        "Layer2 Osc0",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Osc 0 Filter Type",
        "Layer2 Osc0",
        0.0,
        3.0,
        1.0,
        1.0,
        ENUM
    ),
    param_def!(
        "Layer 2 Osc 0 Filter Cutoff",
        "Layer2 Osc0",
        20.0,
        20000.0,
        20000.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Osc 0 Filter Q",
        "Layer2 Osc0",
        0.01,
        10.0,
        0.7,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Osc 0 Dist Type",
        "Layer2 Osc0",
        0.0,
        8.0,
        1.0,
        1.0,
        ENUM
    ),
    param_def!(
        "Layer 2 Osc 0 Dist Drive",
        "Layer2 Osc0",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Osc 1 Waveform",
        "Layer2 Osc1",
        0.0,
        4.0,
        0.0,
        1.0,
        ENUM
    ),
    param_def!(
        "Layer 2 Osc 1 Freq",
        "Layer2 Osc1",
        1000.0,
        20000.0,
        1000.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Osc 1 Amp",
        "Layer2 Osc1",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Osc 1 Phase",
        "Layer2 Osc1",
        0.0,
        180.0,
        0.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Osc 1 FM Amount",
        "Layer2 Osc1",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Osc 1 Filter Type",
        "Layer2 Osc1",
        0.0,
        3.0,
        1.0,
        1.0,
        ENUM
    ),
    param_def!(
        "Layer 2 Osc 1 Filter Cutoff",
        "Layer2 Osc1",
        20.0,
        20000.0,
        20000.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Osc 1 Filter Q",
        "Layer2 Osc1",
        0.01,
        10.0,
        0.7,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Osc 1 Dist Type",
        "Layer2 Osc1",
        0.0,
        8.0,
        1.0,
        1.0,
        ENUM
    ),
    param_def!(
        "Layer 2 Osc 1 Dist Drive",
        "Layer2 Osc1",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Noise Type",
        "Layer2 Noise",
        0.0,
        2.0,
        0.0,
        1.0,
        ENUM
    ),
    param_def!(
        "Layer 2 Noise Amp",
        "Layer2 Noise",
        0.0,
        1.0,
        0.0,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Noise Density",
        "Layer2 Noise",
        0.0,
        1.0,
        0.5,
        0.01,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Noise Filter Type",
        "Layer2 Noise",
        0.0,
        3.0,
        1.0,
        1.0,
        ENUM
    ),
    param_def!(
        "Layer 2 Noise Filter Cutoff",
        "Layer2 Noise",
        20.0,
        20000.0,
        8000.0,
        1.0,
        AUTOMATABLE
    ),
    param_def!(
        "Layer 2 Noise Filter Q",
        "Layer2 Noise",
        0.01,
        10.0,
        0.7,
        0.01,
        AUTOMATABLE
    ),
];

#[inline]
pub fn param_type_def(param_type: ParamType) -> &'static ParamTypeDef {
    &PARAM_TYPE_DEFS[param_type.as_index()]
}

pub fn param_name(id: ParamId) -> String {
    let inst = id.instrument();
    let ty = id.param_type();
    let def = param_type_def(ty);
    format!("Inst {inst} {}", def.base_name)
}

pub fn state_key(id: ParamId) -> String {
    let inst = id.instrument();
    let ty = id.param_type();
    let def = param_type_def(ty);
    if inst == 0 {
        def.base_name.to_string()
    } else {
        format!("Inst {inst} {}", def.base_name)
    }
}

pub fn sanitize_param_value(id: ParamId, value: f64) -> f64 {
    let def = param_type_def(id.param_type());
    let clamped = value.clamp(def.min, def.max);
    if def.step > 0.0 {
        let ticks = ((clamped - def.min) / def.step).round();
        (def.min + ticks * def.step).clamp(def.min, def.max)
    } else {
        clamped
    }
}

#[derive(Debug)]
pub struct ParamStore {
    values: Vec<AtomicF64>,
}

impl Default for ParamStore {
    fn default() -> Self {
        let mut values = Vec::with_capacity(TOTAL_PARAM_COUNT);
        for i in 0..TOTAL_PARAM_COUNT {
            let ty_idx = i % PARAM_TYPES_PER_INSTRUMENT;
            let def = &PARAM_TYPE_DEFS[ty_idx];
            values.push(AtomicF64::new(def.default));
        }
        Self { values }
    }
}

impl ParamStore {
    pub fn get(&self, id: ParamId) -> f64 {
        self.values[id.as_index()].load(Ordering::Acquire)
    }

    pub fn set(&self, id: ParamId, value: f64) {
        self.values[id.as_index()].store(value, Ordering::Release);
    }

    pub fn get_bool(&self, id: ParamId) -> bool {
        self.get(id) > 0.5
    }

    pub fn set_bool(&self, id: ParamId, value: bool) {
        self.set(id, if value { 1.0 } else { 0.0 });
    }
}

impl crate::common::ClapParamId for ParamId {
    const COUNT: usize = TOTAL_PARAM_COUNT;

    fn as_index(self) -> usize {
        self.as_index()
    }

    fn from_raw(id: u32) -> Option<Self> {
        Self::from_raw(id)
    }
}
