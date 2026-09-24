use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED, CLAP_PARAM_REQUIRES_PROCESS,
};

crate::define_params! {
    pub enum ParamId {
        Intensity = 0,
        Sharpness = 1,
        Depth = 2,
        Filter = 3,
        Monitor = 4,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::Intensity,
            name: "Intensity",
            module: "DeEsser",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Sharpness,
            name: "Sharpness",
            module: "DeEsser",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Depth,
            name: "Depth",
            module: "DeEsser",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Filter,
            name: "Filter",
            module: "DeEsser",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Monitor,
            name: "Monitor",
            module: "DeEsser",
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
