use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED, CLAP_PARAM_REQUIRES_PROCESS,
};

crate::define_params! {
    pub enum ParamId {
        Replace = 0,
        Brightness = 1,
        Detune = 2,
        Bigness = 3,
        DryWet = 4,
        Channels = 5,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::Replace,
            name: "Replace",
            module: "Reverb",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Brightness,
            name: "Brightness",
            module: "Reverb",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Detune,
            name: "Detune",
            module: "Reverb",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Bigness,
            name: "Bigness",
            module: "Reverb",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::DryWet,
            name: "Dry/Wet",
            module: "Reverb",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Channels,
            name: "Channels",
            module: "Reverb",
            min: 1.0,
            max: 2.0,
            default: 1.0,
            step: 1.0,
            flags: STEPPED_BOOL,
        }
    ];
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const STEPPED_BOOL: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;
