use std::sync::atomic::{AtomicU64, Ordering};

use clap_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_BYPASS, CLAP_PARAM_IS_HIDDEN, CLAP_PARAM_IS_STEPPED,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum ParamId {
    MasterGain = 0,
    EnableResampling = 1,
    VelocityMin = 2,
    VelocityMax = 3,
    ResampleQuality = 4,
    HumanizeAmount = 5,
    RoundRobinMix = 6,
    BleedAmount = 7,
    LimiterThreshold = 8,
    EnableNormalized = 9,
    RandomSeed = 10,
    VoiceLimitMax = 11,
    VoiceLimitRampdown = 12,
    Bypass = 13,
    Balance1 = 14,
    Balance2 = 15,
    Balance3 = 16,
    Balance4 = 17,
    Balance5 = 18,
    Balance6 = 19,
    Balance7 = 20,
    Balance8 = 21,
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
            2 => Some(ParamId::VelocityMin),
            3 => Some(ParamId::VelocityMax),
            4 => Some(ParamId::ResampleQuality),
            5 => Some(ParamId::HumanizeAmount),
            6 => Some(ParamId::RoundRobinMix),
            7 => Some(ParamId::BleedAmount),
            8 => Some(ParamId::LimiterThreshold),
            9 => Some(ParamId::EnableNormalized),
            10 => Some(ParamId::RandomSeed),
            11 => Some(ParamId::VoiceLimitMax),
            12 => Some(ParamId::VoiceLimitRampdown),
            13 => Some(ParamId::Bypass),
            14 => Some(ParamId::Balance1),
            15 => Some(ParamId::Balance2),
            16 => Some(ParamId::Balance3),
            17 => Some(ParamId::Balance4),
            18 => Some(ParamId::Balance5),
            19 => Some(ParamId::Balance6),
            20 => Some(ParamId::Balance7),
            21 => Some(ParamId::Balance8),
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
        id: ParamId::BleedAmount,
        name: "Bleed Amount",
        module: "Output",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: 0.0,
        max: 100.0,
        default: 100.0,
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
        id: ParamId::VoiceLimitMax,
        name: "Voice Limit Max",
        module: "Voices",
        flags: CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_STEPPED,
        min: 1.0,
        max: 128.0,
        default: 128.0,
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
    ParamDef {
        id: ParamId::Balance1,
        name: "Balance 1-2",
        module: "Balance",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: ParamId::Balance2,
        name: "Balance 3-4",
        module: "Balance",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: ParamId::Balance3,
        name: "Balance 5-6",
        module: "Balance",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: ParamId::Balance4,
        name: "Balance 7-8",
        module: "Balance",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: ParamId::Balance5,
        name: "Balance 9-10",
        module: "Balance",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: ParamId::Balance6,
        name: "Balance 11-12",
        module: "Balance",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: ParamId::Balance7,
        name: "Balance 13-14",
        module: "Balance",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
    ParamDef {
        id: ParamId::Balance8,
        name: "Balance 15-16",
        module: "Balance",
        flags: CLAP_PARAM_IS_AUTOMATABLE,
        min: -1.0,
        max: 1.0,
        default: 0.0,
    },
];

pub fn param_def(id: ParamId) -> Option<&'static ParamDef> {
    PARAMS.iter().find(|d| d.id == id)
}

pub fn sanitize_param_value(id: ParamId, value: f64) -> f64 {
    let Some(def) = param_def(id) else {
        return value;
    };
    let v = value.clamp(def.min, def.max);
    if def.flags & CLAP_PARAM_IS_STEPPED != 0 {
        v.round()
    } else {
        v
    }
}

const MAX_PARAM_ID: usize = ParamId::Balance8 as usize;

#[derive(Debug)]
pub struct ParamStore {
    values: Vec<AtomicU64>,
}

impl Default for ParamStore {
    fn default() -> Self {
        let values: Vec<AtomicU64> = (0..=MAX_PARAM_ID).map(|_| AtomicU64::new(0)).collect();
        let store = Self { values };
        for def in PARAMS.iter() {
            store.values[def.id.as_index()].store(def.default.to_bits(), Ordering::Release);
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
