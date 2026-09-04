use std::sync::atomic::Ordering;

use clap_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED, CLAP_PARAM_REQUIRES_PROCESS,
};
use portable_atomic::AtomicF64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ParamId {
    Mode = 0,
    MinCutoff = 1,
    MaxCutoff = 2,
    Resonance = 3,
    Position = 4,
    LfoRate = 5,
    LfoDepth = 6,
    LfoShape = 7,
    EnvAttack = 8,
    EnvRelease = 9,
    EnvDepth = 10,
    DryWet = 11,
}

impl ParamId {
    pub const COUNT: usize = 12;

    pub const fn all() -> [ParamId; Self::COUNT] {
        [
            ParamId::Mode,
            ParamId::MinCutoff,
            ParamId::MaxCutoff,
            ParamId::Resonance,
            ParamId::Position,
            ParamId::LfoRate,
            ParamId::LfoDepth,
            ParamId::LfoShape,
            ParamId::EnvAttack,
            ParamId::EnvRelease,
            ParamId::EnvDepth,
            ParamId::DryWet,
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

impl crate::common::ClapParamId for ParamId {
    const COUNT: usize = 12;

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

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const STEPPED: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;

pub const MODE_LABELS: [&str; 3] = ["Manual", "LFO", "Envelope"];
pub const SHAPE_LABELS: [&str; 4] = ["Sine", "Triangle", "Saw", "Square"];

pub const PARAMS: [ParamDef; ParamId::COUNT] = [
    ParamDef {
        id: ParamId::Mode,
        name: "Mode",
        module: "Global",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        step: 1.0,
        flags: STEPPED,
    },
    ParamDef {
        id: ParamId::MinCutoff,
        name: "Min Cutoff",
        module: "Filter",
        min: 50.0,
        max: 2000.0,
        default: 300.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::MaxCutoff,
        name: "Max Cutoff",
        module: "Filter",
        min: 500.0,
        max: 10000.0,
        default: 3000.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Resonance,
        name: "Resonance",
        module: "Filter",
        min: 0.1,
        max: 10.0,
        default: 4.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Position,
        name: "Position",
        module: "Global",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::LfoRate,
        name: "LFO Rate",
        module: "LFO",
        min: 0.1,
        max: 20.0,
        default: 2.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::LfoDepth,
        name: "LFO Depth",
        module: "LFO",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::LfoShape,
        name: "LFO Shape",
        module: "LFO",
        min: 0.0,
        max: 3.0,
        default: 0.0,
        step: 1.0,
        flags: STEPPED,
    },
    ParamDef {
        id: ParamId::EnvAttack,
        name: "Env Attack",
        module: "Envelope",
        min: 1.0,
        max: 500.0,
        default: 20.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::EnvRelease,
        name: "Env Release",
        module: "Envelope",
        min: 10.0,
        max: 2000.0,
        default: 200.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::EnvDepth,
        name: "Env Depth",
        module: "Envelope",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::DryWet,
        name: "Dry/Wet",
        module: "Global",
        min: 0.0,
        max: 1.0,
        default: 1.0,
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
