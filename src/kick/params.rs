use std::sync::atomic::{AtomicU64, Ordering};

use clap_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_ENUM, CLAP_PARAM_IS_STEPPED,
    CLAP_PARAM_REQUIRES_PROCESS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ParamId {
    OscWaveform = 0,
    OscFreq = 1,
    OscAmp = 2,
    OscPitchEnvStart = 3,
    OscPitchEnvEnd = 4,
    OscPitchEnvTime = 5,
    OscAmpEnvAttack = 6,
    OscAmpEnvDecay = 7,
    OscAmpEnvSustain = 8,
    OscAmpEnvRelease = 9,
    NoiseAmp = 10,
    NoiseDensity = 11,
    NoiseFilterType = 12,
    NoiseFilterCutoff = 13,
    NoiseFilterQ = 14,
    NoiseAmpEnvAttack = 15,
    NoiseAmpEnvDecay = 16,
    NoiseAmpEnvSustain = 17,
    NoiseAmpEnvRelease = 18,
    Distortion = 19,
    OutputGain = 20,
    KickLength = 21,
    NoiseType = 22,
    MasterFilterType = 23,
    MasterFilterCutoff = 24,
    MasterFilterQ = 25,
}

impl ParamId {
    pub const COUNT: usize = 26;

    pub const fn all() -> [ParamId; Self::COUNT] {
        [
            ParamId::OscWaveform,
            ParamId::OscFreq,
            ParamId::OscAmp,
            ParamId::OscPitchEnvStart,
            ParamId::OscPitchEnvEnd,
            ParamId::OscPitchEnvTime,
            ParamId::OscAmpEnvAttack,
            ParamId::OscAmpEnvDecay,
            ParamId::OscAmpEnvSustain,
            ParamId::OscAmpEnvRelease,
            ParamId::NoiseAmp,
            ParamId::NoiseDensity,
            ParamId::NoiseFilterType,
            ParamId::NoiseFilterCutoff,
            ParamId::NoiseFilterQ,
            ParamId::NoiseAmpEnvAttack,
            ParamId::NoiseAmpEnvDecay,
            ParamId::NoiseAmpEnvSustain,
            ParamId::NoiseAmpEnvRelease,
            ParamId::Distortion,
            ParamId::OutputGain,
            ParamId::KickLength,
            ParamId::NoiseType,
            ParamId::MasterFilterType,
            ParamId::MasterFilterCutoff,
            ParamId::MasterFilterQ,
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

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const ENUM: u32 = CLAP_PARAM_IS_ENUM
    | CLAP_PARAM_IS_STEPPED
    | CLAP_PARAM_IS_AUTOMATABLE
    | CLAP_PARAM_REQUIRES_PROCESS;

pub const PARAMS: [ParamDef; ParamId::COUNT] = [
    ParamDef {
        id: ParamId::OscWaveform,
        name: "Osc Waveform",
        module: "Oscillator",
        min: 0.0,
        max: 3.0,
        default: 0.0,
        step: 1.0,
        flags: ENUM,
    },
    ParamDef {
        id: ParamId::OscFreq,
        name: "Osc Frequency",
        module: "Oscillator",
        min: 20.0,
        max: 500.0,
        default: 150.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::OscAmp,
        name: "Osc Amplitude",
        module: "Oscillator",
        min: 0.0,
        max: 1.0,
        default: 0.8,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::OscPitchEnvStart,
        name: "Osc Pitch Start",
        module: "Oscillator",
        min: 20.0,
        max: 2000.0,
        default: 800.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::OscPitchEnvEnd,
        name: "Osc Pitch End",
        module: "Oscillator",
        min: 20.0,
        max: 500.0,
        default: 40.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::OscPitchEnvTime,
        name: "Osc Pitch Time",
        module: "Oscillator",
        min: 1.0,
        max: 500.0,
        default: 80.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::OscAmpEnvAttack,
        name: "Osc Amp Attack",
        module: "Oscillator",
        min: 0.0,
        max: 100.0,
        default: 1.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::OscAmpEnvDecay,
        name: "Osc Amp Decay",
        module: "Oscillator",
        min: 1.0,
        max: 1000.0,
        default: 200.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::OscAmpEnvSustain,
        name: "Osc Amp Sustain",
        module: "Oscillator",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::OscAmpEnvRelease,
        name: "Osc Amp Release",
        module: "Oscillator",
        min: 1.0,
        max: 1000.0,
        default: 50.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::NoiseAmp,
        name: "Noise Amplitude",
        module: "Noise",
        min: 0.0,
        max: 1.0,
        default: 0.3,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::NoiseDensity,
        name: "Noise Density",
        module: "Noise",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::NoiseFilterType,
        name: "Noise Filter Type",
        module: "Noise",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        step: 1.0,
        flags: ENUM,
    },
    ParamDef {
        id: ParamId::NoiseFilterCutoff,
        name: "Noise Filter Cutoff",
        module: "Noise",
        min: 20.0,
        max: 20000.0,
        default: 8000.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::NoiseFilterQ,
        name: "Noise Filter Q",
        module: "Noise",
        min: 0.1,
        max: 10.0,
        default: 0.7,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::NoiseAmpEnvAttack,
        name: "Noise Amp Attack",
        module: "Noise",
        min: 0.0,
        max: 100.0,
        default: 0.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::NoiseAmpEnvDecay,
        name: "Noise Amp Decay",
        module: "Noise",
        min: 1.0,
        max: 500.0,
        default: 30.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::NoiseAmpEnvSustain,
        name: "Noise Amp Sustain",
        module: "Noise",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::NoiseAmpEnvRelease,
        name: "Noise Amp Release",
        module: "Noise",
        min: 1.0,
        max: 500.0,
        default: 20.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Distortion,
        name: "Distortion",
        module: "Master",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::OutputGain,
        name: "Output Gain",
        module: "Master",
        min: -24.0,
        max: 24.0,
        default: 0.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::KickLength,
        name: "Kick Length",
        module: "Master",
        min: 10.0,
        max: 2000.0,
        default: 300.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::NoiseType,
        name: "Noise Type",
        module: "Noise",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 1.0,
        flags: ENUM,
    },
    ParamDef {
        id: ParamId::MasterFilterType,
        name: "Master Filter Type",
        module: "Master",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        step: 1.0,
        flags: ENUM,
    },
    ParamDef {
        id: ParamId::MasterFilterCutoff,
        name: "Master Filter Cutoff",
        module: "Master",
        min: 20.0,
        max: 20000.0,
        default: 20000.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::MasterFilterQ,
        name: "Master Filter Q",
        module: "Master",
        min: 0.1,
        max: 10.0,
        default: 0.7,
        step: 0.01,
        flags: AUTOMATABLE,
    },
];

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
    values: [AtomicU64; ParamId::COUNT],
}

impl Default for ParamStore {
    fn default() -> Self {
        Self {
            values: PARAMS.map(|param| AtomicU64::new(param.default.to_bits())),
        }
    }
}

impl ParamStore {
    pub fn get(&self, id: ParamId) -> f64 {
        f64::from_bits(self.values[id.as_index()].load(Ordering::Acquire))
    }

    pub fn set(&self, id: ParamId, value: f64) {
        self.values[id.as_index()].store(value.to_bits(), Ordering::Release);
    }
}

impl crate::common::ClapParamId for ParamId {
    const COUNT: usize = Self::COUNT;

    fn as_index(self) -> usize {
        self.as_index()
    }

    fn from_raw(id: u32) -> Option<Self> {
        Self::from_raw(id)
    }
}
