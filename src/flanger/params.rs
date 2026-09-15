use std::sync::atomic::Ordering;

use portable_atomic::AtomicF64;

use maolan_clap::ffi::{CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED};

use crate::common::ClapParamId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ParamId {
    Stereo = 0,
    Rate = 1,
    Tempo = 2,
    TempoSync = 3,
    TimeMode = 4,
    Fraction = 5,
    Crossfade = 6,
    CrossfadeType = 7,
    LfoType = 8,
    LfoPeriod = 9,
    Lfo2Type = 10,
    Lfo2Period = 11,
    InitPhase = 12,
    PhaseDiff = 13,
    ResetPhase = 14,
    MidSide = 15,
    MinDepth = 16,
    Depth = 17,
    SignalPhase = 18,
    FeedbackOn = 19,
    FeedbackGain = 20,
    FeedbackDrive = 21,
    FeedbackDelay = 22,
    FeedbackPhase = 23,
    InputGain = 24,
    DryWet = 25,
    OutputGain = 26,
}

impl ParamId {
    pub const COUNT: usize = 27;

    pub const fn all() -> [ParamId; Self::COUNT] {
        [
            ParamId::Stereo,
            ParamId::Rate,
            ParamId::Tempo,
            ParamId::TempoSync,
            ParamId::TimeMode,
            ParamId::Fraction,
            ParamId::Crossfade,
            ParamId::CrossfadeType,
            ParamId::LfoType,
            ParamId::LfoPeriod,
            ParamId::Lfo2Type,
            ParamId::Lfo2Period,
            ParamId::InitPhase,
            ParamId::PhaseDiff,
            ParamId::ResetPhase,
            ParamId::MidSide,
            ParamId::MinDepth,
            ParamId::Depth,
            ParamId::SignalPhase,
            ParamId::FeedbackOn,
            ParamId::FeedbackGain,
            ParamId::FeedbackDrive,
            ParamId::FeedbackDelay,
            ParamId::FeedbackPhase,
            ParamId::InputGain,
            ParamId::DryWet,
            ParamId::OutputGain,
        ]
    }

    pub const fn as_index(self) -> usize {
        self as usize
    }

    pub fn from_raw(id: u32) -> Option<Self> {
        if id < Self::COUNT as u32 {
            Some(unsafe { std::mem::transmute::<u16, ParamId>(id as u16) })
        } else {
            None
        }
    }
}

impl ClapParamId for ParamId {
    const COUNT: usize = 27;

    fn as_index(self) -> usize {
        self as usize
    }

    fn from_raw(id: u32) -> Option<Self> {
        ParamId::from_raw(id)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ParamDef {
    pub id: ParamId,
    pub name: &'static str,
    pub module: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub step: f64,
    pub flags: u32,
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE;
const STEPPED: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;
const SWITCH: u32 = STEPPED;

pub const PARAMS: [ParamDef; ParamId::COUNT] = [
    ParamDef {
        id: ParamId::Stereo,
        name: "Stereo",
        module: "Flanger",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        step: 1.0,
        flags: SWITCH,
    },
    ParamDef {
        id: ParamId::Rate,
        name: "Rate",
        module: "Flanger",
        min: 0.01,
        max: 20.0,
        default: 0.25,
        step: 0.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Tempo,
        name: "Tempo",
        module: "Flanger",
        min: 20.0,
        max: 360.0,
        default: 120.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::TempoSync,
        name: "Tempo Sync",
        module: "Flanger",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 1.0,
        flags: SWITCH,
    },
    ParamDef {
        id: ParamId::TimeMode,
        name: "Time Mode",
        module: "Flanger",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 1.0,
        flags: STEPPED,
    },
    ParamDef {
        id: ParamId::Fraction,
        name: "Time Fraction",
        module: "Flanger",
        min: 0.0,
        max: 9.0,
        default: 6.0,
        step: 1.0,
        flags: STEPPED,
    },
    ParamDef {
        id: ParamId::Crossfade,
        name: "Crossfade",
        module: "Flanger",
        min: 0.0,
        max: 50.0,
        default: 0.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::CrossfadeType,
        name: "Crossfade Type",
        module: "Flanger",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        step: 1.0,
        flags: STEPPED,
    },
    ParamDef {
        id: ParamId::LfoType,
        name: "LFO Type",
        module: "Flanger",
        min: 0.0,
        max: 13.0,
        default: 0.0,
        step: 1.0,
        flags: STEPPED,
    },
    ParamDef {
        id: ParamId::LfoPeriod,
        name: "LFO Period",
        module: "Flanger",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        step: 1.0,
        flags: STEPPED,
    },
    ParamDef {
        id: ParamId::Lfo2Type,
        name: "LFO2 Type",
        module: "Flanger",
        min: 0.0,
        max: 14.0,
        default: 0.0,
        step: 1.0,
        flags: STEPPED,
    },
    ParamDef {
        id: ParamId::Lfo2Period,
        name: "LFO2 Period",
        module: "Flanger",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        step: 1.0,
        flags: STEPPED,
    },
    ParamDef {
        id: ParamId::InitPhase,
        name: "Initial Phase",
        module: "Flanger",
        min: 0.0,
        max: 360.0,
        default: 0.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::PhaseDiff,
        name: "Phase Diff L/R",
        module: "Flanger",
        min: 0.0,
        max: 360.0,
        default: 0.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::ResetPhase,
        name: "Reset Phase",
        module: "Flanger",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 1.0,
        flags: SWITCH,
    },
    ParamDef {
        id: ParamId::MidSide,
        name: "Mid/Side",
        module: "Flanger",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 1.0,
        flags: SWITCH,
    },
    ParamDef {
        id: ParamId::MinDepth,
        name: "Min Depth",
        module: "Flanger",
        min: 0.01,
        max: 10.0,
        default: 0.25,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Depth,
        name: "Depth",
        module: "Flanger",
        min: 0.1,
        max: 20.0,
        default: 2.0,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::SignalPhase,
        name: "Signal Phase",
        module: "Flanger",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 1.0,
        flags: SWITCH,
    },
    ParamDef {
        id: ParamId::FeedbackOn,
        name: "Feedback On",
        module: "Flanger",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 1.0,
        flags: SWITCH,
    },
    ParamDef {
        id: ParamId::FeedbackGain,
        name: "Feedback Gain",
        module: "Flanger",
        min: 0.0,
        max: 0.89125,
        default: 0.5,
        step: 0.001,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::FeedbackDrive,
        name: "Feedback Drive",
        module: "Flanger",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::FeedbackDelay,
        name: "Feedback Delay",
        module: "Flanger",
        min: 0.0,
        max: 5.0,
        default: 0.0,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::FeedbackPhase,
        name: "Feedback Phase",
        module: "Flanger",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 1.0,
        flags: SWITCH,
    },
    ParamDef {
        id: ParamId::InputGain,
        name: "Input Gain",
        module: "Flanger",
        min: -24.0,
        max: 24.0,
        default: 0.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::DryWet,
        name: "Dry/Wet",
        module: "Flanger",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::OutputGain,
        name: "Output Gain",
        module: "Flanger",
        min: -24.0,
        max: 24.0,
        default: 0.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
];

/// Note fractions for the `Fraction` parameter (index 0..=9).
pub const FRACTIONS: [f64; 10] = [
    1.0 / 64.0,
    1.0 / 32.0,
    1.0 / 16.0,
    1.0 / 8.0,
    1.0 / 4.0,
    1.0 / 2.0,
    1.0,
    2.0,
    4.0,
    8.0,
];

pub const FRACTION_NAMES: [&str; 10] = [
    "1/64", "1/32", "1/16", "1/8", "1/4", "1/2", "1", "2", "4", "8",
];

/// LFO shape names for `LfoType` (index 0..=13, 13 = Off).
pub const LFO_TYPE_NAMES: [&str; 14] = [
    "Triangular",
    "Sine",
    "Stepped Sine",
    "Cubic",
    "Stepped Cubic",
    "Parabolic",
    "Reverse Parabolic",
    "Logarithmic",
    "Reverse Logarithmic",
    "Square Root",
    "Reverse Square Root",
    "Circular",
    "Reverse Circular",
    "Off",
];

/// LFO2 shape names for `Lfo2Type` (index 0 = Same, 1..=13 shapes, 14 = Off).
pub const LFO2_TYPE_NAMES: [&str; 15] = [
    "Same",
    "Triangular",
    "Sine",
    "Stepped Sine",
    "Cubic",
    "Stepped Cubic",
    "Parabolic",
    "Reverse Parabolic",
    "Logarithmic",
    "Reverse Logarithmic",
    "Square Root",
    "Reverse Square Root",
    "Circular",
    "Reverse Circular",
    "Off",
];

pub const PERIOD_NAMES: [&str; 3] = ["Full", "First", "Last"];

pub fn fraction_from_param(value: f64) -> f64 {
    let idx = (value.round() as usize).min(FRACTIONS.len() - 1);
    FRACTIONS[idx]
}

pub fn sanitize_param_value(id: ParamId, value: f64) -> f64 {
    let def = PARAMS[id.as_index()];
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
    values: [AtomicF64; ParamId::COUNT],
}

impl Default for ParamStore {
    fn default() -> Self {
        Self {
            values: PARAMS.map(|param| AtomicF64::new(param.default)),
        }
    }
}

impl ParamStore {
    pub fn get(&self, id: ParamId) -> f64 {
        self.values[id.as_index()].load(Ordering::Acquire)
    }

    pub fn set(&self, id: ParamId, value: f64) {
        self.values[id.as_index()].store(value, Ordering::Release);
    }
}
