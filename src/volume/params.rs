use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_ENUM, CLAP_PARAM_IS_STEPPED,
    CLAP_PARAM_REQUIRES_PROCESS,
};

crate::define_params! {
    pub enum ParamId {
        Volume = 0,
        Channels = 1,
        Pan = 2,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::Volume,
            name: "Volume",
            module: "Output",
            min: -60.0,
            max: 12.0,
            default: 0.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Channels,
            name: "Channels",
            module: "Configuration",
            min: 1.0,
            max: 2.0,
            default: 2.0,
            step: 1.0,
            flags: ENUM_FLAGS,
        }
        {
            id: ParamId::Pan,
            name: "Pan",
            module: "Output",
            min: -1.0,
            max: 1.0,
            default: 0.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
    ];
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const ENUM_FLAGS: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED | CLAP_PARAM_IS_ENUM;
