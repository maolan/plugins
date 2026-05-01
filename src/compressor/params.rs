use std::sync::atomic::{AtomicU64, Ordering};

use clap_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_ENUM, CLAP_PARAM_IS_STEPPED,
    CLAP_PARAM_REQUIRES_PROCESS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ParamId {
    InputGain = 0,
    OutputGain = 1,
    DryGain = 2,
    WetGain = 3,
    ScMode = 4,
    Bypass = 5,

    Split1 = 6,
    Split2 = 7,
    Split3 = 8,

    B1Threshold = 9,
    B1Ratio = 10,
    B1Attack = 11,
    B1Release = 12,
    B1Knee = 13,
    B1Makeup = 14,

    B2Threshold = 15,
    B2Ratio = 16,
    B2Attack = 17,
    B2Release = 18,
    B2Knee = 19,
    B2Makeup = 20,

    B3Threshold = 21,
    B3Ratio = 22,
    B3Attack = 23,
    B3Release = 24,
    B3Knee = 25,
    B3Makeup = 26,

    B4Threshold = 27,
    B4Ratio = 28,
    B4Attack = 29,
    B4Release = 30,
    B4Knee = 31,
    B4Makeup = 32,
    Mode = 33,
    Lookahead = 34,
    ScBoost = 35,
    Topology = 36,
}

impl ParamId {
    pub const COUNT: usize = 37;

    pub const fn all() -> [ParamId; Self::COUNT] {
        [
            ParamId::InputGain,
            ParamId::OutputGain,
            ParamId::DryGain,
            ParamId::WetGain,
            ParamId::ScMode,
            ParamId::Bypass,
            ParamId::Split1,
            ParamId::Split2,
            ParamId::Split3,
            ParamId::B1Threshold,
            ParamId::B1Ratio,
            ParamId::B1Attack,
            ParamId::B1Release,
            ParamId::B1Knee,
            ParamId::B1Makeup,
            ParamId::B2Threshold,
            ParamId::B2Ratio,
            ParamId::B2Attack,
            ParamId::B2Release,
            ParamId::B2Knee,
            ParamId::B2Makeup,
            ParamId::B3Threshold,
            ParamId::B3Ratio,
            ParamId::B3Attack,
            ParamId::B3Release,
            ParamId::B3Knee,
            ParamId::B3Makeup,
            ParamId::B4Threshold,
            ParamId::B4Ratio,
            ParamId::B4Attack,
            ParamId::B4Release,
            ParamId::B4Knee,
            ParamId::B4Makeup,
            ParamId::Mode,
            ParamId::Lookahead,
            ParamId::ScBoost,
            ParamId::Topology,
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
const BOOL_FLAGS: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;
const ENUM_FLAGS: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED | CLAP_PARAM_IS_ENUM;

pub const PARAMS: [ParamDef; ParamId::COUNT] = [
    ParamDef {
        id: ParamId::InputGain,
        name: "Input Gain",
        module: "Gain",
        min: -24.0,
        max: 24.0,
        default: 0.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::OutputGain,
        name: "Output Gain",
        module: "Gain",
        min: -24.0,
        max: 24.0,
        default: 0.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::DryGain,
        name: "Dry Gain",
        module: "Mix",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::WetGain,
        name: "Wet Gain",
        module: "Mix",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::ScMode,
        name: "Sidechain Mode",
        module: "Sidechain",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        step: 1.0,
        flags: ENUM_FLAGS,
    },
    ParamDef {
        id: ParamId::Bypass,
        name: "Bypass",
        module: "Global",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 1.0,
        flags: BOOL_FLAGS,
    },
    ParamDef {
        id: ParamId::Split1,
        name: "Split 1",
        module: "Crossover",
        min: 20.0,
        max: 500.0,
        default: 120.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Split2,
        name: "Split 2",
        module: "Crossover",
        min: 200.0,
        max: 4000.0,
        default: 1000.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Split3,
        name: "Split 3",
        module: "Crossover",
        min: 1500.0,
        max: 18000.0,
        default: 6000.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B1Threshold,
        name: "Band 1 Threshold",
        module: "Band 1",
        min: -60.0,
        max: 0.0,
        default: -12.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B1Ratio,
        name: "Band 1 Ratio",
        module: "Band 1",
        min: 1.0,
        max: 100.0,
        default: 4.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B1Attack,
        name: "Band 1 Attack",
        module: "Band 1",
        min: 0.0,
        max: 2000.0,
        default: 20.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B1Release,
        name: "Band 1 Release",
        module: "Band 1",
        min: 0.0,
        max: 5000.0,
        default: 100.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B1Knee,
        name: "Band 1 Knee",
        module: "Band 1",
        min: 0.0,
        max: 24.0,
        default: 6.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B1Makeup,
        name: "Band 1 Makeup",
        module: "Band 1",
        min: -24.0,
        max: 24.0,
        default: 0.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B2Threshold,
        name: "Band 2 Threshold",
        module: "Band 2",
        min: -60.0,
        max: 0.0,
        default: -12.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B2Ratio,
        name: "Band 2 Ratio",
        module: "Band 2",
        min: 1.0,
        max: 100.0,
        default: 4.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B2Attack,
        name: "Band 2 Attack",
        module: "Band 2",
        min: 0.0,
        max: 2000.0,
        default: 20.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B2Release,
        name: "Band 2 Release",
        module: "Band 2",
        min: 0.0,
        max: 5000.0,
        default: 100.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B2Knee,
        name: "Band 2 Knee",
        module: "Band 2",
        min: 0.0,
        max: 24.0,
        default: 6.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B2Makeup,
        name: "Band 2 Makeup",
        module: "Band 2",
        min: -24.0,
        max: 24.0,
        default: 0.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B3Threshold,
        name: "Band 3 Threshold",
        module: "Band 3",
        min: -60.0,
        max: 0.0,
        default: -12.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B3Ratio,
        name: "Band 3 Ratio",
        module: "Band 3",
        min: 1.0,
        max: 100.0,
        default: 4.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B3Attack,
        name: "Band 3 Attack",
        module: "Band 3",
        min: 0.0,
        max: 2000.0,
        default: 20.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B3Release,
        name: "Band 3 Release",
        module: "Band 3",
        min: 0.0,
        max: 5000.0,
        default: 100.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B3Knee,
        name: "Band 3 Knee",
        module: "Band 3",
        min: 0.0,
        max: 24.0,
        default: 6.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B3Makeup,
        name: "Band 3 Makeup",
        module: "Band 3",
        min: -24.0,
        max: 24.0,
        default: 0.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B4Threshold,
        name: "Band 4 Threshold",
        module: "Band 4",
        min: -60.0,
        max: 0.0,
        default: -12.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B4Ratio,
        name: "Band 4 Ratio",
        module: "Band 4",
        min: 1.0,
        max: 100.0,
        default: 4.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B4Attack,
        name: "Band 4 Attack",
        module: "Band 4",
        min: 0.0,
        max: 2000.0,
        default: 20.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B4Release,
        name: "Band 4 Release",
        module: "Band 4",
        min: 0.0,
        max: 5000.0,
        default: 100.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B4Knee,
        name: "Band 4 Knee",
        module: "Band 4",
        min: 0.0,
        max: 24.0,
        default: 6.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::B4Makeup,
        name: "Band 4 Makeup",
        module: "Band 4",
        min: -24.0,
        max: 24.0,
        default: 0.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Mode,
        name: "Mode",
        module: "Compressor",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        step: 1.0,
        flags: ENUM_FLAGS,
    },
    ParamDef {
        id: ParamId::Lookahead,
        name: "Lookahead",
        module: "Compressor",
        min: 0.0,
        max: 20.0,
        default: 0.0,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::ScBoost,
        name: "SC Boost",
        module: "Sidechain",
        min: 0.0,
        max: 4.0,
        default: 0.0,
        step: 1.0,
        flags: ENUM_FLAGS,
    },
    ParamDef {
        id: ParamId::Topology,
        name: "Topology",
        module: "Compressor",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        step: 1.0,
        flags: ENUM_FLAGS,
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

    pub fn get_bool(&self, id: ParamId) -> bool {
        self.get(id) >= 0.5
    }

    pub fn get_enum(&self, id: ParamId) -> u32 {
        self.get(id).round().clamp(0.0, 1024.0) as u32
    }

}
