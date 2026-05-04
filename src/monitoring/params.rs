use std::sync::atomic::{AtomicU64, Ordering};

use clap_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED, CLAP_PARAM_REQUIRES_PROCESS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ParamId {
    Mode = 0,
}

impl ParamId {
    pub const COUNT: usize = 1;

    pub const fn all() -> [ParamId; Self::COUNT] {
        [ParamId::Mode]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    Out24 = 0,
    Out16 = 1,
    Peaks = 2,
    Slew = 3,
    Subs = 4,
    Mono = 5,
    Side = 6,
    Vinyl = 7,
    Aurat = 8,
    MonoRat = 9,
    MonoLat = 10,
    Phone = 11,
    CansA = 12,
    CansB = 13,
    CansC = 14,
    CansD = 15,
    VTrick = 16,
}

impl Mode {
    pub const VARIANTS: [Mode; 17] = [
        Mode::Out24,
        Mode::Out16,
        Mode::Peaks,
        Mode::Slew,
        Mode::Subs,
        Mode::Mono,
        Mode::Side,
        Mode::Vinyl,
        Mode::Aurat,
        Mode::MonoRat,
        Mode::MonoLat,
        Mode::Phone,
        Mode::CansA,
        Mode::CansB,
        Mode::CansC,
        Mode::CansD,
        Mode::VTrick,
    ];

    pub fn from_raw(v: u32) -> Self {
        Self::VARIANTS[v.min(16) as usize]
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Out24 => "Out24",
            Mode::Out16 => "Out16",
            Mode::Peaks => "Peaks",
            Mode::Slew => "Slew",
            Mode::Subs => "Subs",
            Mode::Mono => "Mono",
            Mode::Side => "Side",
            Mode::Vinyl => "Vinyl",
            Mode::Aurat => "Aurat",
            Mode::MonoRat => "MonoRat",
            Mode::MonoLat => "MonoLat",
            Mode::Phone => "Phone",
            Mode::CansA => "Cans A",
            Mode::CansB => "Cans B",
            Mode::CansC => "Cans C",
            Mode::CansD => "Cans D",
            Mode::VTrick => "V Trick",
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

pub const PARAMS: [ParamDef; ParamId::COUNT] = [ParamDef {
    id: ParamId::Mode,
    name: "Mode",
    module: "Monitoring",
    min: 0.0,
    max: 16.0,
    default: 0.0,
    step: 1.0,
    flags: STEPPED,
}];

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
