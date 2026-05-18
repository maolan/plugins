use std::sync::atomic::{AtomicU64, Ordering};

use clap_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_ENUM, CLAP_PARAM_IS_STEPPED,
    CLAP_PARAM_REQUIRES_PROCESS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ParamId {
    OutputGain = 0,
    Boost = 1,
    Low = 2,
    Mid = 3,
    High = 4,
    SoloLow = 5,
    SoloMid = 6,
    SoloHigh = 7,
    X1 = 8,
    X2 = 9,
    Strength = 10,
    MonitorMode = 11,
}

impl ParamId {
    pub const COUNT: usize = 12;

    pub const fn all() -> [ParamId; Self::COUNT] {
        [
            ParamId::OutputGain,
            ParamId::Boost,
            ParamId::Low,
            ParamId::Mid,
            ParamId::High,
            ParamId::SoloLow,
            ParamId::SoloMid,
            ParamId::SoloHigh,
            ParamId::X1,
            ParamId::X2,
            ParamId::Strength,
            ParamId::MonitorMode,
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
const STEPPED: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;
const ENUM_FLAGS: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED | CLAP_PARAM_IS_ENUM;

pub const PARAMS: [ParamDef; ParamId::COUNT] = [
    ParamDef {
        id: ParamId::OutputGain,
        name: "Output Gain",
        module: "Bandwidth",
        min: -24.0,
        max: 4.0,
        default: -5.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Boost,
        name: "Boost",
        module: "Bandwidth",
        min: 0.0,
        max: 4.0,
        default: 1.0,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Low,
        name: "Low",
        module: "Bandwidth",
        min: 0.0,
        max: 200.0,
        default: 100.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Mid,
        name: "Mid",
        module: "Bandwidth",
        min: 0.0,
        max: 200.0,
        default: 100.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::High,
        name: "High",
        module: "Bandwidth",
        min: 0.0,
        max: 200.0,
        default: 100.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::SoloLow,
        name: "Solo Low",
        module: "Bandwidth",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 1.0,
        flags: STEPPED,
    },
    ParamDef {
        id: ParamId::SoloMid,
        name: "Solo Mid",
        module: "Bandwidth",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 1.0,
        flags: STEPPED,
    },
    ParamDef {
        id: ParamId::SoloHigh,
        name: "Solo High",
        module: "Bandwidth",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 1.0,
        flags: STEPPED,
    },
    ParamDef {
        id: ParamId::X1,
        name: "X1",
        module: "Bandwidth",
        min: 40.0,
        max: 1000.0,
        default: 400.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::X2,
        name: "X2",
        module: "Bandwidth",
        min: 1000.0,
        max: 18000.0,
        default: 4000.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Strength,
        name: "Strength",
        module: "Bandwidth",
        min: 1.0,
        max: 20.0,
        default: 10.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::MonitorMode,
        name: "Monitor Mode",
        module: "Bandwidth",
        min: 0.0,
        max: 2.0,
        default: 0.0,
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
