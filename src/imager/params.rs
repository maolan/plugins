use std::sync::atomic::{AtomicU64, Ordering};

use clap_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_ENUM, CLAP_PARAM_IS_STEPPED,
    CLAP_PARAM_REQUIRES_PROCESS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ParamId {
    Mode = 0,
    Width = 1,
    Focus = 2,
    Amount = 3,
    Resonance = 4,
    Mix = 5,
}

impl ParamId {
    pub const COUNT: usize = 6;

    pub const fn all() -> [ParamId; Self::COUNT] {
        [
            ParamId::Mode,
            ParamId::Width,
            ParamId::Focus,
            ParamId::Amount,
            ParamId::Resonance,
            ParamId::Mix,
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
const ENUM_FLAGS: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED | CLAP_PARAM_IS_ENUM;

pub const PARAMS: [ParamDef; ParamId::COUNT] = [
    ParamDef {
        id: ParamId::Mode,
        name: "Mode",
        module: "Global",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        step: 1.0,
        flags: ENUM_FLAGS,
    },
    ParamDef {
        id: ParamId::Width,
        name: "Width",
        module: "Imager",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Focus,
        name: "Focus",
        module: "Imager",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Amount,
        name: "Amount",
        module: "Imager",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Resonance,
        name: "Resonance",
        module: "Imager",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Mix,
        name: "Mix",
        module: "Imager",
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

    pub fn get_enum(&self, id: ParamId) -> u32 {
        self.get(id).round().clamp(0.0, 1024.0) as u32
    }
}
