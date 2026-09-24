use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_ENUM, CLAP_PARAM_IS_STEPPED,
    CLAP_PARAM_REQUIRES_PROCESS,
};

crate::define_params! {
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
        ToneBassMode = 12,
        ToneTrebleMode = 13,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::InputLevel,
            name: "Input",
            module: "Gain",
            min: -20.0,
            max: 20.0,
            default: 0.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::NoiseGateThreshold,
            name: "Threshold",
            module: "Gate",
            min: -100.0,
            max: 0.0,
            default: -80.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::ToneBass,
            name: "Bass",
            module: "EQ",
            min: 0.0,
            max: 10.0,
            default: 5.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::ToneMid,
            name: "Middle",
            module: "EQ",
            min: 0.0,
            max: 10.0,
            default: 5.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::ToneTreble,
            name: "Treble",
            module: "EQ",
            min: 0.0,
            max: 10.0,
            default: 5.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::OutputLevel,
            name: "Output",
            module: "Gain",
            min: -40.0,
            max: 40.0,
            default: 0.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::NoiseGateActive,
            name: "NoiseGateActive",
            module: "Gate",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            step: 1.0,
            flags: BOOL_FLAGS,
        }
        {
            id: ParamId::EqActive,
            name: "ToneStack",
            module: "EQ",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            step: 1.0,
            flags: BOOL_FLAGS,
        }
        {
            id: ParamId::IrToggle,
            name: "IRToggle",
            module: "IR",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            step: 1.0,
            flags: BOOL_FLAGS,
        }
        {
            id: ParamId::CalibrateInput,
            name: "CalibrateInput",
            module: "Calibration",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            step: 1.0,
            flags: BOOL_FLAGS,
        }
        {
            id: ParamId::InputCalibrationLevel,
            name: "InputCalibrationLevel",
            module: "Calibration",
            min: -60.0,
            max: 60.0,
            default: 12.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::OutputMode,
            name: "OutputMode",
            module: "Calibration",
            min: 0.0,
            max: 2.0,
            default: 1.0,
            step: 1.0,
            flags: ENUM_FLAGS,
        }
        {
            id: ParamId::ToneBassMode,
            name: "BassMode",
            module: "EQ",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 1.0,
            flags: ENUM_FLAGS,
        }
        {
            id: ParamId::ToneTrebleMode,
            name: "TrebleMode",
            module: "EQ",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 1.0,
            flags: ENUM_FLAGS,
        }
    ];
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const BOOL_FLAGS: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;
const ENUM_FLAGS: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED | CLAP_PARAM_IS_ENUM;

pub type ParamStore = crate::common::param_store::ParamStore<ParamId>;

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
