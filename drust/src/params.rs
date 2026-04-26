use std::sync::atomic::{AtomicU64, Ordering};

use clap_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_BYPASS, CLAP_PARAM_IS_HIDDEN, CLAP_PARAM_IS_STEPPED,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum ParamId {
    MasterGain = 0,
    EnableResampling = 1,
    EnableVelocityFilter = 2,
    VelocityMin = 3,
    VelocityMax = 4,
    ResampleQuality = 5,
    EnableHumanizer = 6,
    HumanizeAmount = 7,
    RoundRobinMix = 8,
    EnableBleedControl = 9,
    BleedAmount = 10,
    EnableLimiter = 11,
    LimiterThreshold = 12,
    EnableNormalized = 13,
    RandomSeed = 14,
    EnableVoiceLimit = 15,
    VoiceLimitMax = 16,
    VoiceLimitRampdown = 17,
    Bypass = 18,
}

impl ParamId {
    pub fn as_index(self) -> usize {
        self as usize
    }

    pub fn as_u16(self) -> u16 {
        self as u16
    }

    pub fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(ParamId::MasterGain),
            1 => Some(ParamId::EnableResampling),
            2 => Some(ParamId::EnableVelocityFilter),
            3 => Some(ParamId::VelocityMin),
            4 => Some(ParamId::VelocityMax),
            5 => Some(ParamId::ResampleQuality),
            6 => Some(ParamId::EnableHumanizer),
            7 => Some(ParamId::HumanizeAmount),
            8 => Some(ParamId::RoundRobinMix),
            9 => Some(ParamId::EnableBleedControl),
            10 => Some(ParamId::BleedAmount),
            11 => Some(ParamId::EnableLimiter),
            12 => Some(ParamId::LimiterThreshold),
            13 => Some(ParamId::EnableNormalized),
            14 => Some(ParamId::RandomSeed),
            15 => Some(ParamId::EnableVoiceLimit),
            16 => Some(ParamId::VoiceLimitMax),
            17 => Some(ParamId::VoiceLimitRampdown),
            18 => Some(ParamId::Bypass),
            _ => None,
        }
    }
}

pub struct ParamDef {
    pub id: ParamId,
    pub name: &'static str,
    pub module: &'static str,
    pub flags: u32,
    pub min: f64,
    pub max: f64,
    pub default: f64,
}

pub const PARAMS: &[ParamDef] = &[
    ParamDef {
        id: ParamId::MasterGain,
        name: "Master Gain",
        module: "Output",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: -60.0,
        max: 12.0,
        default: 0.0,
    },
    ParamDef {
        id: ParamId::EnableResampling,
        name: "Enable Resampling",
        module: "Quality",
        flags: CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_STEPPED,
        min: 0.0,
        max: 1.0,
        default: 1.0,
    },
    ParamDef {
        id: ParamId::EnableVelocityFilter,
        name: "Velocity Filter",
        module: "Input",
        flags: CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_STEPPED,
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: ParamId::VelocityMin,
        name: "Min Velocity",
        module: "Input",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: 0.0,
        max: 127.0,
        default: 0.0,
    },
    ParamDef {
        id: ParamId::VelocityMax,
        name: "Max Velocity",
        module: "Input",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: 0.0,
        max: 127.0,
        default: 127.0,
    },
    ParamDef {
        id: ParamId::ResampleQuality,
        name: "Resample Quality",
        module: "Quality",
        flags: CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_STEPPED,
        min: 0.0,
        max: 3.0,
        default: 1.0,
    },
    ParamDef {
        id: ParamId::EnableHumanizer,
        name: "Humanizer",
        module: "Input",
        flags: CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_STEPPED,
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: ParamId::HumanizeAmount,
        name: "Humanize Amount",
        module: "Input",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: 0.0,
        max: 100.0,
        default: 8.0,
    },
    ParamDef {
        id: ParamId::RoundRobinMix,
        name: "Round Robin Mix",
        module: "Input",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: 0.0,
        max: 1.0,
        default: 0.7,
    },
    ParamDef {
        id: ParamId::EnableBleedControl,
        name: "Bleed Control",
        module: "Output",
        flags: CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_STEPPED,
        min: 0.0,
        max: 1.0,
        default: 1.0,
    },
    ParamDef {
        id: ParamId::BleedAmount,
        name: "Bleed Amount",
        module: "Output",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: 0.0,
        max: 100.0,
        default: 100.0,
    },
    ParamDef {
        id: ParamId::EnableLimiter,
        name: "Limiter",
        module: "Output",
        flags: CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_STEPPED,
        min: 0.0,
        max: 1.0,
        default: 1.0,
    },
    ParamDef {
        id: ParamId::LimiterThreshold,
        name: "Limiter Threshold",
        module: "Output",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: -48.0,
        max: 0.0,
        default: -3.0,
    },
    ParamDef {
        id: ParamId::EnableNormalized,
        name: "Normalize Samples",
        module: "Quality",
        flags: CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_STEPPED,
        min: 0.0,
        max: 1.0,
        default: 1.0,
    },
    ParamDef {
        id: ParamId::RandomSeed,
        name: "Random Seed",
        module: "Input",
        flags: CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_STEPPED,
        min: 0.0,
        max: 1000.0,
        default: 0.0,
    },
    ParamDef {
        id: ParamId::EnableVoiceLimit,
        name: "Enable Voice Limit",
        module: "Voices",
        flags: CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_STEPPED,
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: ParamId::VoiceLimitMax,
        name: "Voice Limit Max",
        module: "Voices",
        flags: CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_STEPPED,
        min: 1.0,
        max: 128.0,
        default: 15.0,
    },
    ParamDef {
        id: ParamId::VoiceLimitRampdown,
        name: "Voice Limit Rampdown",
        module: "Voices",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: 0.01,
        max: 2.0,
        default: 0.5,
    },
    ParamDef {
        id: ParamId::Bypass,
        name: "Bypass",
        module: "",
        flags: CLAP_PARAM_IS_BYPASS | CLAP_PARAM_IS_HIDDEN | CLAP_PARAM_IS_STEPPED,
        min: 0.0,
        max: 1.0,
        default: 0.0,
    },
];

pub fn sanitize_param_value(id: ParamId, value: f64) -> f64 {
    let def = &PARAMS[id.as_index()];
    let v = value.clamp(def.min, def.max);
    if def.flags & CLAP_PARAM_IS_STEPPED != 0 {
        v.round()
    } else {
        v
    }
}

#[derive(Debug)]
pub struct ParamStore {
    values: [AtomicU64; PARAMS.len()],
}

impl Default for ParamStore {
    fn default() -> Self {
        let store = Self {
            values: std::array::from_fn(|_| AtomicU64::new(0)),
        };
        for (i, def) in PARAMS.iter().enumerate() {
            store.values[i].store(def.default.to_bits(), Ordering::Release);
        }
        store
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
