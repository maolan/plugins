use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED, CLAP_PARAM_REQUIRES_PROCESS,
};

crate::define_params! {
    pub enum ParamId {
        BitDepth = 0,
        Downsample = 1,
        Drive = 2,
        DryWet = 3,
        OutputGain = 4,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::BitDepth,
            name: "Bit Depth",
            module: "Crusher",
            min: 1.0,
            max: 24.0,
            default: 8.0,
            step: 1.0,
            flags: STEPPED,
        }
        {
            id: ParamId::Downsample,
            name: "Downsample",
            module: "Crusher",
            min: 1.0,
            max: 128.0,
            default: 4.0,
            step: 1.0,
            flags: STEPPED,
        }
        {
            id: ParamId::Drive,
            name: "Drive",
            module: "Crusher",
            min: 0.0,
            max: 24.0,
            default: 0.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::DryWet,
            name: "Dry/Wet",
            module: "Mix",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::OutputGain,
            name: "Output Gain",
            module: "Output",
            min: -24.0,
            max: 24.0,
            default: 0.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
    ];
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const STEPPED: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;
