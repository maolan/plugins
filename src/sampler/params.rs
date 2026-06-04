//! Parameter definitions for Maolan Sampler.

use clap_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED, CLAP_PARAM_REQUIRES_PROCESS,
};

use crate::common::ClapParamId;

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const STEPPED: u32 =
    CLAP_PARAM_IS_STEPPED | CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ParamId {
    MasterGain = 0,
    MasterPan = 1,
    AmpAttack = 2,
    AmpDecay = 3,
    AmpSustain = 4,
    AmpRelease = 5,
    PitchBendUp = 6,
    PitchBendDown = 7,
    FilterType = 8,
    FilterCutoff = 9,
    FilterResonance = 10,
    FilterEgAmount = 11,
    FilterEnabled = 12,
    FilterAttack = 13,
    FilterDecay = 14,
    FilterSustain = 15,
    FilterRelease = 16,
    Eg2Attack = 17,
    Eg2Decay = 18,
    Eg2Sustain = 19,
    Eg2Release = 20,
    Eg3Attack = 21,
    Eg3Decay = 22,
    Eg3Sustain = 23,
    Eg3Release = 24,
    Eg4Attack = 25,
    Eg4Decay = 26,
    Eg4Sustain = 27,
    Eg4Release = 28,
    Eg5Attack = 29,
    Eg5Decay = 30,
    Eg5Sustain = 31,
    Eg5Release = 32,
    Lfo1Rate = 33,
    Lfo1Amount = 34,
    Lfo1Shape = 35,
    Lfo1Enabled = 36,
    Lfo2Rate = 37,
    Lfo2Amount = 38,
    Lfo2Shape = 39,
    Lfo2Enabled = 40,
}

impl ParamId {
    pub const COUNT: usize = 41;

    pub fn all() -> impl Iterator<Item = Self> {
        (0..Self::COUNT as u16).filter_map(|i| Self::from_raw(i as u32))
    }

    pub fn as_index(self) -> usize {
        self as usize
    }

    pub fn from_raw(id: u32) -> Option<Self> {
        match id {
            0 => Some(ParamId::MasterGain),
            1 => Some(ParamId::MasterPan),
            2 => Some(ParamId::AmpAttack),
            3 => Some(ParamId::AmpDecay),
            4 => Some(ParamId::AmpSustain),
            5 => Some(ParamId::AmpRelease),
            6 => Some(ParamId::PitchBendUp),
            7 => Some(ParamId::PitchBendDown),
            8 => Some(ParamId::FilterType),
            9 => Some(ParamId::FilterCutoff),
            10 => Some(ParamId::FilterResonance),
            11 => Some(ParamId::FilterEgAmount),
            12 => Some(ParamId::FilterEnabled),
            13 => Some(ParamId::FilterAttack),
            14 => Some(ParamId::FilterDecay),
            15 => Some(ParamId::FilterSustain),
            16 => Some(ParamId::FilterRelease),
            17 => Some(ParamId::Eg2Attack),
            18 => Some(ParamId::Eg2Decay),
            19 => Some(ParamId::Eg2Sustain),
            20 => Some(ParamId::Eg2Release),
            21 => Some(ParamId::Eg3Attack),
            22 => Some(ParamId::Eg3Decay),
            23 => Some(ParamId::Eg3Sustain),
            24 => Some(ParamId::Eg3Release),
            25 => Some(ParamId::Eg4Attack),
            26 => Some(ParamId::Eg4Decay),
            27 => Some(ParamId::Eg4Sustain),
            28 => Some(ParamId::Eg4Release),
            29 => Some(ParamId::Eg5Attack),
            30 => Some(ParamId::Eg5Decay),
            31 => Some(ParamId::Eg5Sustain),
            32 => Some(ParamId::Eg5Release),
            33 => Some(ParamId::Lfo1Rate),
            34 => Some(ParamId::Lfo1Amount),
            35 => Some(ParamId::Lfo1Shape),
            36 => Some(ParamId::Lfo1Enabled),
            37 => Some(ParamId::Lfo2Rate),
            38 => Some(ParamId::Lfo2Amount),
            39 => Some(ParamId::Lfo2Shape),
            40 => Some(ParamId::Lfo2Enabled),
            _ => None,
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

macro_rules! def_automatable {
    ($id:expr, $name:expr, $module:expr, $min:expr, $max:expr, $default:expr, $step:expr) => {
        ParamDef {
            id: $id,
            name: $name,
            module: $module,
            min: $min,
            max: $max,
            default: $default,
            step: $step,
            flags: AUTOMATABLE,
        }
    };
}

macro_rules! def_stepped {
    ($id:expr, $name:expr, $module:expr, $min:expr, $max:expr, $default:expr) => {
        ParamDef {
            id: $id,
            name: $name,
            module: $module,
            min: $min,
            max: $max,
            default: $default,
            step: 1.0,
            flags: STEPPED,
        }
    };
}

pub const PARAMS: [ParamDef; ParamId::COUNT] = [
    def_automatable!(ParamId::MasterGain, "Gain", "Master", 0.0, 2.0, 1.0, 0.01),
    def_automatable!(ParamId::MasterPan, "Pan", "Master", -1.0, 1.0, 0.0, 0.01),
    def_automatable!(
        ParamId::AmpAttack,
        "Attack",
        "Amp EG",
        0.0,
        10.0,
        0.01,
        0.01
    ),
    def_automatable!(ParamId::AmpDecay, "Decay", "Amp EG", 0.0, 10.0, 0.2, 0.01),
    def_automatable!(
        ParamId::AmpSustain,
        "Sustain",
        "Amp EG",
        0.0,
        1.0,
        1.0,
        0.01
    ),
    def_automatable!(
        ParamId::AmpRelease,
        "Release",
        "Amp EG",
        0.0,
        10.0,
        0.3,
        0.01
    ),
    def_stepped!(ParamId::PitchBendUp, "Bend Up", "Pitch", 0.0, 24.0, 2.0),
    def_stepped!(ParamId::PitchBendDown, "Bend Down", "Pitch", 0.0, 24.0, 2.0),
    def_stepped!(ParamId::FilterType, "Type", "Filter", 0.0, 48.0, 1.0),
    def_automatable!(
        ParamId::FilterCutoff,
        "Cutoff",
        "Filter",
        20.0,
        20000.0,
        20000.0,
        1.0
    ),
    def_automatable!(
        ParamId::FilterResonance,
        "Resonance",
        "Filter",
        0.01,
        10.0,
        0.7,
        0.01
    ),
    def_automatable!(
        ParamId::FilterEgAmount,
        "EG Amount",
        "Filter",
        -1.0,
        1.0,
        0.0,
        0.01
    ),
    def_stepped!(ParamId::FilterEnabled, "Enabled", "Filter", 0.0, 1.0, 0.0),
    def_automatable!(
        ParamId::FilterAttack,
        "Attack",
        "Filter EG",
        0.0,
        10.0,
        0.01,
        0.01
    ),
    def_automatable!(
        ParamId::FilterDecay,
        "Decay",
        "Filter EG",
        0.0,
        10.0,
        0.2,
        0.01
    ),
    def_automatable!(
        ParamId::FilterSustain,
        "Sustain",
        "Filter EG",
        0.0,
        1.0,
        0.0,
        0.01
    ),
    def_automatable!(
        ParamId::FilterRelease,
        "Release",
        "Filter EG",
        0.0,
        10.0,
        0.3,
        0.01
    ),
    def_automatable!(ParamId::Eg2Attack, "Attack", "EG2", 0.0, 10.0, 0.01, 0.01),
    def_automatable!(ParamId::Eg2Decay, "Decay", "EG2", 0.0, 10.0, 0.2, 0.01),
    def_automatable!(ParamId::Eg2Sustain, "Sustain", "EG2", 0.0, 1.0, 1.0, 0.01),
    def_automatable!(ParamId::Eg2Release, "Release", "EG2", 0.0, 10.0, 0.3, 0.01),
    def_automatable!(ParamId::Eg3Attack, "Attack", "EG3", 0.0, 10.0, 0.01, 0.01),
    def_automatable!(ParamId::Eg3Decay, "Decay", "EG3", 0.0, 10.0, 0.2, 0.01),
    def_automatable!(ParamId::Eg3Sustain, "Sustain", "EG3", 0.0, 1.0, 1.0, 0.01),
    def_automatable!(ParamId::Eg3Release, "Release", "EG3", 0.0, 10.0, 0.3, 0.01),
    def_automatable!(ParamId::Eg4Attack, "Attack", "EG4", 0.0, 10.0, 0.01, 0.01),
    def_automatable!(ParamId::Eg4Decay, "Decay", "EG4", 0.0, 10.0, 0.2, 0.01),
    def_automatable!(ParamId::Eg4Sustain, "Sustain", "EG4", 0.0, 1.0, 1.0, 0.01),
    def_automatable!(ParamId::Eg4Release, "Release", "EG4", 0.0, 10.0, 0.3, 0.01),
    def_automatable!(ParamId::Eg5Attack, "Attack", "EG5", 0.0, 10.0, 0.01, 0.01),
    def_automatable!(ParamId::Eg5Decay, "Decay", "EG5", 0.0, 10.0, 0.2, 0.01),
    def_automatable!(ParamId::Eg5Sustain, "Sustain", "EG5", 0.0, 1.0, 1.0, 0.01),
    def_automatable!(ParamId::Eg5Release, "Release", "EG5", 0.0, 10.0, 0.3, 0.01),
    def_automatable!(ParamId::Lfo1Rate, "Rate", "LFO1", 0.01, 20.0, 1.0, 0.01),
    def_automatable!(ParamId::Lfo1Amount, "Amount", "LFO1", 0.0, 1.0, 0.0, 0.01),
    def_stepped!(ParamId::Lfo1Shape, "Shape", "LFO1", 0.0, 9.0, 0.0),
    def_stepped!(ParamId::Lfo1Enabled, "Enabled", "LFO1", 0.0, 1.0, 0.0),
    def_automatable!(ParamId::Lfo2Rate, "Rate", "LFO2", 0.01, 20.0, 1.0, 0.01),
    def_automatable!(ParamId::Lfo2Amount, "Amount", "LFO2", 0.0, 1.0, 0.0, 0.01),
    def_stepped!(ParamId::Lfo2Shape, "Shape", "LFO2", 0.0, 9.0, 0.0),
    def_stepped!(ParamId::Lfo2Enabled, "Enabled", "LFO2", 0.0, 1.0, 0.0),
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

pub type ParamStore = crate::common::param_store::ParamStore<ParamId>;

impl Default for ParamStore {
    fn default() -> Self {
        let store = Self::new();
        for param in PARAMS.iter() {
            store.set(param.id, param.default);
        }
        store
    }
}

impl ClapParamId for ParamId {
    const COUNT: usize = Self::COUNT;

    fn as_index(self) -> usize {
        self.as_index()
    }

    fn from_raw(id: u32) -> Option<Self> {
        Self::from_raw(id)
    }
}
