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
    ModRoute1Source = 41,
    ModRoute1Target = 42,
    ModRoute1Depth = 43,
    ModRoute2Source = 44,
    ModRoute2Target = 45,
    ModRoute2Depth = 46,
    ModRoute3Source = 47,
    ModRoute3Target = 48,
    ModRoute3Depth = 49,
    ModRoute4Source = 50,
    ModRoute4Target = 51,
    ModRoute4Depth = 52,
    ModRoute5Source = 53,
    ModRoute5Target = 54,
    ModRoute5Depth = 55,
    ModRoute6Source = 56,
    ModRoute6Target = 57,
    ModRoute6Depth = 58,
}

impl ParamId {
    pub const COUNT: usize = 59;

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
            41 => Some(ParamId::ModRoute1Source),
            42 => Some(ParamId::ModRoute1Target),
            43 => Some(ParamId::ModRoute1Depth),
            44 => Some(ParamId::ModRoute2Source),
            45 => Some(ParamId::ModRoute2Target),
            46 => Some(ParamId::ModRoute2Depth),
            47 => Some(ParamId::ModRoute3Source),
            48 => Some(ParamId::ModRoute3Target),
            49 => Some(ParamId::ModRoute3Depth),
            50 => Some(ParamId::ModRoute4Source),
            51 => Some(ParamId::ModRoute4Target),
            52 => Some(ParamId::ModRoute4Depth),
            53 => Some(ParamId::ModRoute5Source),
            54 => Some(ParamId::ModRoute5Target),
            55 => Some(ParamId::ModRoute5Depth),
            56 => Some(ParamId::ModRoute6Source),
            57 => Some(ParamId::ModRoute6Target),
            58 => Some(ParamId::ModRoute6Depth),
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
    def_stepped!(ParamId::ModRoute1Source, "Source", "Mod 1", 0.0, 8.0, 0.0),
    def_stepped!(ParamId::ModRoute1Target, "Target", "Mod 1", 0.0, 6.0, 0.0),
    def_automatable!(
        ParamId::ModRoute1Depth,
        "Depth",
        "Mod 1",
        -1.0,
        1.0,
        0.0,
        0.01
    ),
    def_stepped!(ParamId::ModRoute2Source, "Source", "Mod 2", 0.0, 8.0, 0.0),
    def_stepped!(ParamId::ModRoute2Target, "Target", "Mod 2", 0.0, 6.0, 0.0),
    def_automatable!(
        ParamId::ModRoute2Depth,
        "Depth",
        "Mod 2",
        -1.0,
        1.0,
        0.0,
        0.01
    ),
    def_stepped!(ParamId::ModRoute3Source, "Source", "Mod 3", 0.0, 8.0, 0.0),
    def_stepped!(ParamId::ModRoute3Target, "Target", "Mod 3", 0.0, 6.0, 0.0),
    def_automatable!(
        ParamId::ModRoute3Depth,
        "Depth",
        "Mod 3",
        -1.0,
        1.0,
        0.0,
        0.01
    ),
    def_stepped!(ParamId::ModRoute4Source, "Source", "Mod 4", 0.0, 8.0, 0.0),
    def_stepped!(ParamId::ModRoute4Target, "Target", "Mod 4", 0.0, 6.0, 0.0),
    def_automatable!(
        ParamId::ModRoute4Depth,
        "Depth",
        "Mod 4",
        -1.0,
        1.0,
        0.0,
        0.01
    ),
    def_stepped!(ParamId::ModRoute5Source, "Source", "Mod 5", 0.0, 8.0, 0.0),
    def_stepped!(ParamId::ModRoute5Target, "Target", "Mod 5", 0.0, 6.0, 0.0),
    def_automatable!(
        ParamId::ModRoute5Depth,
        "Depth",
        "Mod 5",
        -1.0,
        1.0,
        0.0,
        0.01
    ),
    def_stepped!(ParamId::ModRoute6Source, "Source", "Mod 6", 0.0, 8.0, 0.0),
    def_stepped!(ParamId::ModRoute6Target, "Target", "Mod 6", 0.0, 6.0, 0.0),
    def_automatable!(
        ParamId::ModRoute6Depth,
        "Depth",
        "Mod 6",
        -1.0,
        1.0,
        0.0,
        0.01
    ),
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
