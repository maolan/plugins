use std::sync::atomic::Ordering;

use portable_atomic::AtomicF64;

use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_ENUM, CLAP_PARAM_IS_STEPPED,
    CLAP_PARAM_REQUIRES_PROCESS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ParamId {
    Boost = 0,
    Ceiling = 1,
    Lookahead = 2,
    Attack = 3,
    Release = 4,
    LinkTransients = 5,
    LinkRelease = 6,
    Channels = 7,
    OutputGain = 8,
}

impl ParamId {
    pub const COUNT: usize = 9;

    pub const fn all() -> [ParamId; Self::COUNT] {
        [
            ParamId::Boost,
            ParamId::Ceiling,
            ParamId::Lookahead,
            ParamId::Attack,
            ParamId::Release,
            ParamId::LinkTransients,
            ParamId::LinkRelease,
            ParamId::Channels,
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
        id: ParamId::Boost,
        name: "Gain",
        module: "Limiter",
        min: -90.0,
        max: 20.0,
        default: 0.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Ceiling,
        name: "Ceiling",
        module: "Limiter",
        min: -90.0,
        max: 0.0,
        default: 0.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Lookahead,
        name: "Lookahead",
        module: "Envelope",
        min: 0.0,
        max: 20.0,
        default: 1.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Attack,
        name: "Attack",
        module: "Envelope",
        min: 0.0,
        max: 10.0,
        default: 1.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Release,
        name: "Release",
        module: "Envelope",
        min: 1.0,
        max: 999.0,
        default: 250.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::LinkTransients,
        name: "Channel Linking Transients",
        module: "Channel Linking",
        min: 0.0,
        max: 100.0,
        default: 100.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::LinkRelease,
        name: "Channel Linking Release",
        module: "Channel Linking",
        min: 0.0,
        max: 100.0,
        default: 100.0,
        step: 1.0,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Channels,
        name: "Channels",
        module: "Global",
        min: 1.0,
        max: 2.0,
        default: 2.0,
        step: 1.0,
        flags: ENUM_FLAGS,
    },
    ParamDef {
        id: ParamId::OutputGain,
        name: "Output Volume",
        module: "Limiter",
        min: -90.0,
        max: 20.0,
        default: 0.0,
        step: 0.1,
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

    pub fn get_enum(&self, id: ParamId) -> u32 {
        self.get(id).round().clamp(0.0, 1024.0) as u32
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
