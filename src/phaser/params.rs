use std::sync::atomic::Ordering;

use portable_atomic::AtomicF64;

use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED, CLAP_PARAM_REQUIRES_PROCESS,
};

use crate::common::ClapParamId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ParamId {
    LfoRate = 0,
    LfoDepth = 1,
    Manual = 2,
    Feedback = 3,
    FeedbackDelayOn = 4,
    DelayTime = 5,
    Stages = 6,
}

impl ParamId {
    pub const COUNT: usize = 7;

    pub const fn all() -> [ParamId; Self::COUNT] {
        [
            ParamId::LfoRate,
            ParamId::LfoDepth,
            ParamId::Manual,
            ParamId::Feedback,
            ParamId::FeedbackDelayOn,
            ParamId::DelayTime,
            ParamId::Stages,
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
    const COUNT: usize = 7;

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
const STEPPED_INT: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;
const TOGGLE: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;

pub const PARAMS: [ParamDef; ParamId::COUNT] = [
    ParamDef {
        id: ParamId::LfoRate,
        name: "LFO Rate (Hz)",
        module: "Phaser",
        min: 0.01,
        max: 2.0,
        default: 0.1,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::LfoDepth,
        name: "LFO Depth",
        module: "Phaser",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Manual,
        name: "Manual (Center)",
        module: "Phaser",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Feedback,
        name: "Feedback",
        module: "Phaser",
        min: -0.98,
        max: 0.98,
        default: 0.0,
        step: 0.01,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::FeedbackDelayOn,
        name: "Feedback Delay On",
        module: "Phaser",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 1.0,
        flags: TOGGLE,
    },
    ParamDef {
        id: ParamId::DelayTime,
        name: "Delay Time (ms)",
        module: "Phaser",
        min: 0.0,
        max: 20.0,
        default: 1.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::Stages,
        name: "Stages (All-pass)",
        module: "Phaser",
        min: 1.0,
        max: 12.0,
        default: 12.0,
        step: 1.0,
        flags: STEPPED_INT,
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
