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
    FilterSubtype = 9,
    FilterCutoff = 10,
    FilterResonance = 11,
    FilterEgAmount = 12,
    FilterKeyTrack = 13,
    FilterDrive = 14,
    FilterEnabled = 15,
    FilterAttack = 16,
    FilterDecay = 17,
    FilterSustain = 18,
    FilterRelease = 19,
    Eg2Attack = 20,
    Eg2Decay = 21,
    Eg2Sustain = 22,
    Eg2Release = 23,
    Eg3Attack = 24,
    Eg3Decay = 25,
    Eg3Sustain = 26,
    Eg3Release = 27,
    Eg4Attack = 28,
    Eg4Decay = 29,
    Eg4Sustain = 30,
    Eg4Release = 31,
    Eg5Attack = 32,
    Eg5Decay = 33,
    Eg5Sustain = 34,
    Eg5Release = 35,
    Lfo1Rate = 36,
    Lfo1Amount = 37,
    Lfo1Shape = 38,
    Lfo1Enabled = 39,
    Lfo1Deform = 40,
    Lfo1Phase = 41,
    Lfo1Trigger = 42,
    Lfo1Unipolar = 43,
    Lfo1SyncMode = 44,
    Lfo2Rate = 45,
    Lfo2Amount = 46,
    Lfo2Shape = 47,
    Lfo2Enabled = 48,
    Lfo2Deform = 49,
    Lfo2Phase = 50,
    Lfo2Trigger = 51,
    Lfo2Unipolar = 52,
    Lfo2SyncMode = 53,
    Lfo3Rate = 54,
    Lfo3Amount = 55,
    Lfo3Shape = 56,
    Lfo3Enabled = 57,
    Lfo3Deform = 58,
    Lfo3Phase = 59,
    Lfo3Trigger = 60,
    Lfo3Unipolar = 61,
    Lfo3SyncMode = 62,
    Lfo4Rate = 63,
    Lfo4Amount = 64,
    Lfo4Shape = 65,
    Lfo4Enabled = 66,
    Lfo4Deform = 67,
    Lfo4Phase = 68,
    Lfo4Trigger = 69,
    Lfo4Unipolar = 70,
    Lfo4SyncMode = 71,
    Lfo5Rate = 72,
    Lfo5Amount = 73,
    Lfo5Shape = 74,
    Lfo5Enabled = 75,
    Lfo5Deform = 76,
    Lfo5Phase = 77,
    Lfo5Trigger = 78,
    Lfo5Unipolar = 79,
    Lfo5SyncMode = 80,
    Lfo6Rate = 81,
    Lfo6Amount = 82,
    Lfo6Shape = 83,
    Lfo6Enabled = 84,
    Lfo6Deform = 85,
    Lfo6Phase = 86,
    Lfo6Trigger = 87,
    Lfo6Unipolar = 88,
    Lfo6SyncMode = 89,
    Filter2Type = 90,
    Filter2Subtype = 91,
    Filter2Cutoff = 92,
    Filter2Resonance = 93,
    Filter2EgAmount = 94,
    Filter2KeyTrack = 95,
    Filter2Drive = 96,
    Filter2Enabled = 97,
    ModRoute1Source = 98,
    ModRoute1Target = 99,
    ModRoute1Depth = 100,
    ModRoute2Source = 101,
    ModRoute2Target = 102,
    ModRoute2Depth = 103,
    ModRoute3Source = 104,
    ModRoute3Target = 105,
    ModRoute3Depth = 106,
    ModRoute4Source = 107,
    ModRoute4Target = 108,
    ModRoute4Depth = 109,
    ModRoute5Source = 110,
    ModRoute5Target = 111,
    ModRoute5Depth = 112,
    ModRoute6Source = 113,
    ModRoute6Target = 114,
    ModRoute6Depth = 115,
}

impl ParamId {
    pub const COUNT: usize = 116;

    pub fn all() -> impl Iterator<Item = Self> {
        (0..Self::COUNT as u16).filter_map(|i| Self::from_raw(i as u32))
    }

    pub fn as_index(self) -> usize {
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
    def_stepped!(ParamId::FilterSubtype, "Subtype", "Filter", 0.0, 21.0, 0.0),
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
    def_automatable!(
        ParamId::FilterKeyTrack,
        "Key Track",
        "Filter",
        0.0,
        1.0,
        0.0,
        0.01
    ),
    def_automatable!(ParamId::FilterDrive, "Drive", "Filter", 0.0, 1.0, 0.0, 0.01),
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
    def_stepped!(ParamId::Lfo1Enabled, "Enabled", "LFO1", 0.0, 1.0, 1.0),
    def_automatable!(ParamId::Lfo1Deform, "Deform", "LFO1", -1.0, 1.0, 0.0, 0.01),
    def_automatable!(ParamId::Lfo1Phase, "Phase", "LFO1", 0.0, 1.0, 0.0, 0.01),
    def_stepped!(ParamId::Lfo1Trigger, "Trigger", "LFO1", 0.0, 2.0, 1.0),
    def_stepped!(ParamId::Lfo1Unipolar, "Unipolar", "LFO1", 0.0, 1.0, 0.0),
    def_stepped!(ParamId::Lfo1SyncMode, "Sync Mode", "LFO1", 0.0, 1.0, 1.0),
    def_automatable!(ParamId::Lfo2Rate, "Rate", "LFO2", 0.01, 20.0, 1.0, 0.01),
    def_automatable!(ParamId::Lfo2Amount, "Amount", "LFO2", 0.0, 1.0, 0.0, 0.01),
    def_stepped!(ParamId::Lfo2Shape, "Shape", "LFO2", 0.0, 9.0, 0.0),
    def_stepped!(ParamId::Lfo2Enabled, "Enabled", "LFO2", 0.0, 1.0, 1.0),
    def_automatable!(ParamId::Lfo2Deform, "Deform", "LFO2", -1.0, 1.0, 0.0, 0.01),
    def_automatable!(ParamId::Lfo2Phase, "Phase", "LFO2", 0.0, 1.0, 0.0, 0.01),
    def_stepped!(ParamId::Lfo2Trigger, "Trigger", "LFO2", 0.0, 2.0, 1.0),
    def_stepped!(ParamId::Lfo2Unipolar, "Unipolar", "LFO2", 0.0, 1.0, 0.0),
    def_stepped!(ParamId::Lfo2SyncMode, "Sync Mode", "LFO2", 0.0, 1.0, 1.0),
    def_automatable!(ParamId::Lfo3Rate, "Rate", "LFO3", 0.01, 20.0, 1.0, 0.01),
    def_automatable!(ParamId::Lfo3Amount, "Amount", "LFO3", 0.0, 1.0, 0.0, 0.01),
    def_stepped!(ParamId::Lfo3Shape, "Shape", "LFO3", 0.0, 9.0, 0.0),
    def_stepped!(ParamId::Lfo3Enabled, "Enabled", "LFO3", 0.0, 1.0, 1.0),
    def_automatable!(ParamId::Lfo3Deform, "Deform", "LFO3", -1.0, 1.0, 0.0, 0.01),
    def_automatable!(ParamId::Lfo3Phase, "Phase", "LFO3", 0.0, 1.0, 0.0, 0.01),
    def_stepped!(ParamId::Lfo3Trigger, "Trigger", "LFO3", 0.0, 2.0, 1.0),
    def_stepped!(ParamId::Lfo3Unipolar, "Unipolar", "LFO3", 0.0, 1.0, 0.0),
    def_stepped!(ParamId::Lfo3SyncMode, "Sync Mode", "LFO3", 0.0, 1.0, 1.0),
    def_automatable!(ParamId::Lfo4Rate, "Rate", "LFO4", 0.01, 20.0, 1.0, 0.01),
    def_automatable!(ParamId::Lfo4Amount, "Amount", "LFO4", 0.0, 1.0, 0.0, 0.01),
    def_stepped!(ParamId::Lfo4Shape, "Shape", "LFO4", 0.0, 9.0, 0.0),
    def_stepped!(ParamId::Lfo4Enabled, "Enabled", "LFO4", 0.0, 1.0, 1.0),
    def_automatable!(ParamId::Lfo4Deform, "Deform", "LFO4", -1.0, 1.0, 0.0, 0.01),
    def_automatable!(ParamId::Lfo4Phase, "Phase", "LFO4", 0.0, 1.0, 0.0, 0.01),
    def_stepped!(ParamId::Lfo4Trigger, "Trigger", "LFO4", 0.0, 2.0, 1.0),
    def_stepped!(ParamId::Lfo4Unipolar, "Unipolar", "LFO4", 0.0, 1.0, 0.0),
    def_stepped!(ParamId::Lfo4SyncMode, "Sync Mode", "LFO4", 0.0, 1.0, 1.0),
    def_automatable!(ParamId::Lfo5Rate, "Rate", "LFO5", 0.01, 20.0, 1.0, 0.01),
    def_automatable!(ParamId::Lfo5Amount, "Amount", "LFO5", 0.0, 1.0, 0.0, 0.01),
    def_stepped!(ParamId::Lfo5Shape, "Shape", "LFO5", 0.0, 9.0, 0.0),
    def_stepped!(ParamId::Lfo5Enabled, "Enabled", "LFO5", 0.0, 1.0, 1.0),
    def_automatable!(ParamId::Lfo5Deform, "Deform", "LFO5", -1.0, 1.0, 0.0, 0.01),
    def_automatable!(ParamId::Lfo5Phase, "Phase", "LFO5", 0.0, 1.0, 0.0, 0.01),
    def_stepped!(ParamId::Lfo5Trigger, "Trigger", "LFO5", 0.0, 2.0, 1.0),
    def_stepped!(ParamId::Lfo5Unipolar, "Unipolar", "LFO5", 0.0, 1.0, 0.0),
    def_stepped!(ParamId::Lfo5SyncMode, "Sync Mode", "LFO5", 0.0, 1.0, 1.0),
    def_automatable!(ParamId::Lfo6Rate, "Rate", "LFO6", 0.01, 20.0, 1.0, 0.01),
    def_automatable!(ParamId::Lfo6Amount, "Amount", "LFO6", 0.0, 1.0, 0.0, 0.01),
    def_stepped!(ParamId::Lfo6Shape, "Shape", "LFO6", 0.0, 9.0, 0.0),
    def_stepped!(ParamId::Lfo6Enabled, "Enabled", "LFO6", 0.0, 1.0, 1.0),
    def_automatable!(ParamId::Lfo6Deform, "Deform", "LFO6", -1.0, 1.0, 0.0, 0.01),
    def_automatable!(ParamId::Lfo6Phase, "Phase", "LFO6", 0.0, 1.0, 0.0, 0.01),
    def_stepped!(ParamId::Lfo6Trigger, "Trigger", "LFO6", 0.0, 2.0, 1.0),
    def_stepped!(ParamId::Lfo6Unipolar, "Unipolar", "LFO6", 0.0, 1.0, 0.0),
    def_stepped!(ParamId::Lfo6SyncMode, "Sync Mode", "LFO6", 0.0, 1.0, 1.0),
    def_stepped!(ParamId::Filter2Type, "Type", "Filter 2", 0.0, 48.0, 1.0),
    def_stepped!(
        ParamId::Filter2Subtype,
        "Subtype",
        "Filter 2",
        0.0,
        21.0,
        0.0
    ),
    def_automatable!(
        ParamId::Filter2Cutoff,
        "Cutoff",
        "Filter 2",
        20.0,
        20000.0,
        20000.0,
        1.0
    ),
    def_automatable!(
        ParamId::Filter2Resonance,
        "Resonance",
        "Filter 2",
        0.01,
        10.0,
        0.7,
        0.01
    ),
    def_automatable!(
        ParamId::Filter2EgAmount,
        "EG Amount",
        "Filter 2",
        -1.0,
        1.0,
        0.0,
        0.01
    ),
    def_automatable!(
        ParamId::Filter2KeyTrack,
        "Key Track",
        "Filter 2",
        0.0,
        1.0,
        0.0,
        0.01
    ),
    def_automatable!(
        ParamId::Filter2Drive,
        "Drive",
        "Filter 2",
        0.0,
        1.0,
        0.0,
        0.01
    ),
    def_stepped!(
        ParamId::Filter2Enabled,
        "Enabled",
        "Filter 2",
        0.0,
        1.0,
        0.0
    ),
    def_stepped!(ParamId::ModRoute1Source, "Source", "Mod 1", 0.0, 24.0, 0.0),
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
    def_stepped!(ParamId::ModRoute2Source, "Source", "Mod 2", 0.0, 24.0, 0.0),
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
    def_stepped!(ParamId::ModRoute3Source, "Source", "Mod 3", 0.0, 24.0, 0.0),
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
    def_stepped!(ParamId::ModRoute4Source, "Source", "Mod 4", 0.0, 24.0, 0.0),
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
    def_stepped!(ParamId::ModRoute5Source, "Source", "Mod 5", 0.0, 24.0, 0.0),
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
    def_stepped!(ParamId::ModRoute6Source, "Source", "Mod 6", 0.0, 24.0, 0.0),
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
