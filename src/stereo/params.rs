use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_ENUM, CLAP_PARAM_IS_STEPPED,
    CLAP_PARAM_REQUIRES_PROCESS,
};

crate::define_params! {
    pub enum ParamId {
        OutputGain = 0,
        Boost = 1,
        LowGain = 2,
        MidGain = 3,
        HighGain = 4,
        SoloLow = 5,
        SoloMid = 6,
        SoloHigh = 7,
        X1 = 8,
        X2 = 9,
        Strength = 10,
        MonitorMode = 11,
        LowDelay = 12,
        MidDelay = 13,
        HighDelay = 14,
        Density = 15,
        Focus = 16,
        Amount = 17,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::OutputGain,
            name: "Volume",
            module: "Output",
            min: -24.0,
            max: 4.0,
            default: 0.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Boost,
            name: "Boost",
            module: "Gain",
            min: 0.0,
            max: 2.0,
            default: 1.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::LowGain,
            name: "Low Gain",
            module: "Gain",
            min: 0.0,
            max: 100.0,
            default: 50.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::MidGain,
            name: "Mid Gain",
            module: "Gain",
            min: 0.0,
            max: 100.0,
            default: 50.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::HighGain,
            name: "High Gain",
            module: "Gain",
            min: 0.0,
            max: 100.0,
            default: 50.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::SoloLow,
            name: "Solo Low",
            module: "Output",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 1.0,
            flags: STEPPED,
        }
        {
            id: ParamId::SoloMid,
            name: "Solo Mid",
            module: "Output",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 1.0,
            flags: STEPPED,
        }
        {
            id: ParamId::SoloHigh,
            name: "Solo High",
            module: "Output",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 1.0,
            flags: STEPPED,
        }
        {
            id: ParamId::X1,
            name: "X1",
            module: "Gain",
            min: 40.0,
            max: 1000.0,
            default: 400.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::X2,
            name: "X2",
            module: "Gain",
            min: 1000.0,
            max: 18000.0,
            default: 4000.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Strength,
            name: "Strength",
            module: "Delay",
            min: 1.0,
            max: 20.0,
            default: 10.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::MonitorMode,
            name: "Monitor Mode",
            module: "Output",
            min: 0.0,
            max: 2.0,
            default: 0.0,
            step: 1.0,
            flags: ENUM_FLAGS,
        }
        {
            id: ParamId::LowDelay,
            name: "Low Delay",
            module: "Delay",
            min: 0.0,
            max: 100.0,
            default: 50.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::MidDelay,
            name: "Mid Delay",
            module: "Delay",
            min: 0.0,
            max: 100.0,
            default: 50.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::HighDelay,
            name: "High Delay",
            module: "Delay",
            min: 0.0,
            max: 100.0,
            default: 50.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Density,
            name: "Density",
            module: "Character",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Focus,
            name: "Focus",
            module: "Character",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Amount,
            name: "Amount",
            module: "Character",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
    ];
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const STEPPED: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;
const ENUM_FLAGS: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED | CLAP_PARAM_IS_ENUM;

pub type ParamStore = crate::common::param_store::ParamStore<ParamId>;
