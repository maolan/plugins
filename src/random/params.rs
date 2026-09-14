use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_ENUM, CLAP_PARAM_IS_STEPPED,
    CLAP_PARAM_REQUIRES_PROCESS,
};
use portable_atomic::AtomicF64;
use std::sync::atomic::Ordering;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ParamId {
    NoteLength,
    PauseLength,
    LowestNote,
    HighestNote,
}
impl ParamId {
    pub const COUNT: usize = 4;
    pub fn as_index(self) -> usize {
        self as usize
    }
    pub fn from_raw(id: u32) -> Option<Self> {
        match id {
            0 => Some(Self::NoteLength),
            1 => Some(Self::PauseLength),
            2 => Some(Self::LowestNote),
            3 => Some(Self::HighestNote),
            _ => None,
        }
    }
}
impl crate::common::ClapParamId for ParamId {
    const COUNT: usize = 4;
    fn as_index(self) -> usize {
        self as usize
    }
    fn from_raw(id: u32) -> Option<Self> {
        Self::from_raw(id)
    }
}
pub const LENGTHS: [&str; 8] = [
    "1/16 note",
    "1/8 note",
    "1/4 note",
    "1/2 note",
    "Whole note",
    "1 bar",
    "2 bars",
    "4 bars",
];
pub struct ParamDef {
    pub id: ParamId,
    pub name: &'static str,
    pub module: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub flags: u32,
}
const FLAGS: u32 = CLAP_PARAM_IS_AUTOMATABLE
    | CLAP_PARAM_IS_STEPPED
    | CLAP_PARAM_IS_ENUM
    | CLAP_PARAM_REQUIRES_PROCESS;
pub const PARAMS: [ParamDef; 4] = [
    ParamDef {
        id: ParamId::NoteLength,
        name: "Note length",
        module: "Random",
        min: 0.0,
        max: 7.0,
        default: 5.0,
        flags: FLAGS,
    },
    ParamDef {
        id: ParamId::PauseLength,
        name: "Pause length",
        module: "Random",
        min: 0.0,
        max: 7.0,
        default: 5.0,
        flags: FLAGS,
    },
    ParamDef {
        id: ParamId::LowestNote,
        name: "Lowest note",
        module: "Random",
        min: 0.0,
        max: 127.0,
        default: 48.0,
        flags: FLAGS,
    },
    ParamDef {
        id: ParamId::HighestNote,
        name: "Highest note",
        module: "Random",
        min: 0.0,
        max: 127.0,
        default: 72.0,
        flags: FLAGS,
    },
];
pub fn sanitize_param_value(id: ParamId, value: f64) -> f64 {
    let def = &PARAMS[id.as_index()];
    if value.is_finite() {
        value.round().clamp(def.min, def.max)
    } else {
        def.default
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

/// MIDI note names use middle C = C4 (MIDI 60).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note(pub u8);
impl std::fmt::Display for Note {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const NAMES: [&str; 12] = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        write!(
            f,
            "{}{}",
            NAMES[usize::from(self.0 % 12)],
            i16::from(self.0 / 12) - 1
        )
    }
}
