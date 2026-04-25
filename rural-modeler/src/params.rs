use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use clap_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_ENUM, CLAP_PARAM_IS_STEPPED,
    CLAP_PARAM_REQUIRES_PROCESS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ParamId {
    InputLevel = 0,
    NoiseGateThreshold = 1,
    ToneBass = 2,
    ToneMid = 3,
    ToneTreble = 4,
    OutputLevel = 5,
    NoiseGateActive = 6,
    EqActive = 7,
    IrToggle = 8,
    CalibrateInput = 9,
    InputCalibrationLevel = 10,
    OutputMode = 11,
}

impl ParamId {
    pub const COUNT: usize = 12;

    pub const fn all() -> [ParamId; Self::COUNT] {
        [
            ParamId::InputLevel,
            ParamId::NoiseGateThreshold,
            ParamId::ToneBass,
            ParamId::ToneMid,
            ParamId::ToneTreble,
            ParamId::OutputLevel,
            ParamId::NoiseGateActive,
            ParamId::EqActive,
            ParamId::IrToggle,
            ParamId::CalibrateInput,
            ParamId::InputCalibrationLevel,
            ParamId::OutputMode,
        ]
    }

    pub const fn as_index(self) -> usize {
        self as usize
    }

    pub fn from_raw(id: u32) -> Option<Self> {
        match id {
            0 => Some(Self::InputLevel),
            1 => Some(Self::NoiseGateThreshold),
            2 => Some(Self::ToneBass),
            3 => Some(Self::ToneMid),
            4 => Some(Self::ToneTreble),
            5 => Some(Self::OutputLevel),
            6 => Some(Self::NoiseGateActive),
            7 => Some(Self::EqActive),
            8 => Some(Self::IrToggle),
            9 => Some(Self::CalibrateInput),
            10 => Some(Self::InputCalibrationLevel),
            11 => Some(Self::OutputMode),
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

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const BOOL_FLAGS: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;
const ENUM_FLAGS: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED | CLAP_PARAM_IS_ENUM;

pub const PARAMS: [ParamDef; ParamId::COUNT] = [
    ParamDef {
        id: ParamId::InputLevel,
        name: "Input",
        module: "Gain",
        min: -20.0,
        max: 20.0,
        default: 0.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::NoiseGateThreshold,
        name: "Threshold",
        module: "Gate",
        min: -100.0,
        max: 0.0,
        default: -80.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::ToneBass,
        name: "Bass",
        module: "EQ",
        min: 0.0,
        max: 10.0,
        default: 5.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::ToneMid,
        name: "Middle",
        module: "EQ",
        min: 0.0,
        max: 10.0,
        default: 5.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::ToneTreble,
        name: "Treble",
        module: "EQ",
        min: 0.0,
        max: 10.0,
        default: 5.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::OutputLevel,
        name: "Output",
        module: "Gain",
        min: -40.0,
        max: 40.0,
        default: 0.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::NoiseGateActive,
        name: "NoiseGateActive",
        module: "Gate",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        step: 1.0,
        flags: BOOL_FLAGS,
    },
    ParamDef {
        id: ParamId::EqActive,
        name: "ToneStack",
        module: "EQ",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        step: 1.0,
        flags: BOOL_FLAGS,
    },
    ParamDef {
        id: ParamId::IrToggle,
        name: "IRToggle",
        module: "IR",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        step: 1.0,
        flags: BOOL_FLAGS,
    },
    ParamDef {
        id: ParamId::CalibrateInput,
        name: "CalibrateInput",
        module: "Calibration",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 1.0,
        flags: BOOL_FLAGS,
    },
    ParamDef {
        id: ParamId::InputCalibrationLevel,
        name: "InputCalibrationLevel",
        module: "Calibration",
        min: -60.0,
        max: 60.0,
        default: 12.0,
        step: 0.1,
        flags: AUTOMATABLE,
    },
    ParamDef {
        id: ParamId::OutputMode,
        name: "OutputMode",
        module: "Calibration",
        min: 0.0,
        max: 2.0,
        default: 1.0,
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

#[cfg(test)]
mod tests {
    use super::{ParamId, sanitize_param_value};

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1.0e-9,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn continuous_params_snap_to_tenths_like_nam_knobs() {
        assert_close(sanitize_param_value(ParamId::InputLevel, 0.04), 0.0);
        assert_close(sanitize_param_value(ParamId::InputLevel, 0.05), 0.1);
        assert_close(sanitize_param_value(ParamId::ToneBass, 4.96), 5.0);
        assert_close(sanitize_param_value(ParamId::ToneBass, 4.94), 4.9);
    }

    #[test]
    fn stepped_params_round_and_clamp() {
        assert_close(sanitize_param_value(ParamId::NoiseGateActive, -0.1), 0.0);
        assert_close(sanitize_param_value(ParamId::NoiseGateActive, 0.7), 1.0);
        assert_close(sanitize_param_value(ParamId::OutputMode, 1.6), 2.0);
        assert_close(sanitize_param_value(ParamId::OutputMode, 99.0), 2.0);
    }
}

#[derive(Debug)]
pub struct ParamStore {
    values: [AtomicU64; ParamId::COUNT],
    dirty: AtomicBool,
}

impl Default for ParamStore {
    fn default() -> Self {
        Self {
            values: PARAMS.map(|param| AtomicU64::new(param.default.to_bits())),
            dirty: AtomicBool::new(false),
        }
    }
}

impl ParamStore {
    pub fn get(&self, id: ParamId) -> f64 {
        f64::from_bits(self.values[id.as_index()].load(Ordering::Acquire))
    }

    pub fn set(&self, id: ParamId, value: f64) {
        self.values[id.as_index()].store(value.to_bits(), Ordering::Release);
        self.dirty.store(true, Ordering::Release);
    }

    pub fn get_bool(&self, id: ParamId) -> bool {
        self.get(id) >= 0.5
    }

    pub fn get_enum(&self, id: ParamId) -> u32 {
        self.get(id).round().clamp(0.0, 1024.0) as u32
    }
}
