use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED, CLAP_PARAM_REQUIRES_PROCESS,
};

crate::define_params! {
    pub enum ParamId {
        Threshold = 0,
        Ratio = 1,
        Range = 2,
        Attack = 3,
        Hold = 4,
        Release = 5,
        Hysteresis = 6,
        SidechainHpf = 7,
        SidechainLpf = 8,
        ExternalSidechain = 9,
        Monitor = 10,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::Threshold,
            name: "Threshold",
            module: "Gate",
            min: -80.0,
            max: 0.0,
            default: -50.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Ratio,
            name: "Ratio",
            module: "Gate",
            min: 1.0,
            max: 100.0,
            default: 4.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Range,
            name: "Range",
            module: "Gate",
            min: 0.0,
            max: 100.0,
            default: 80.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Attack,
            name: "Attack",
            module: "Gate",
            min: 0.01,
            max: 100.0,
            default: 1.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Hold,
            name: "Hold",
            module: "Gate",
            min: 0.0,
            max: 500.0,
            default: 50.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Release,
            name: "Release",
            module: "Gate",
            min: 1.0,
            max: 2000.0,
            default: 150.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Hysteresis,
            name: "Hysteresis",
            module: "Gate",
            min: 0.0,
            max: 24.0,
            default: 3.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::SidechainHpf,
            name: "Sidechain HPF",
            module: "Detector",
            min: 20.0,
            max: 2000.0,
            default: 20.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::SidechainLpf,
            name: "Sidechain LPF",
            module: "Detector",
            min: 1000.0,
            max: 20000.0,
            default: 20000.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::ExternalSidechain,
            name: "External Sidechain",
            module: "Detector",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 1.0,
            flags: BOOL_FLAGS,
        }
        {
            id: ParamId::Monitor,
            name: "Monitor",
            module: "Detector",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 1.0,
            flags: BOOL_FLAGS,
        }
    ];
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const BOOL_FLAGS: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;
