use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED, CLAP_PARAM_REQUIRES_PROCESS,
};

crate::define_params! {
    pub enum ParamId {
        Depth = 0,
        Rate = 1,
        DryWet = 2,
        Voices = 3,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::Depth,
            name: "Mod Depth",
            module: "Chorus",
            min: 0.0,
            max: 10.0,
            default: 5.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Rate,
            name: "Mod Rate",
            module: "Chorus",
            min: 0.1,
            max: 5.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::DryWet,
            name: "Dry/Wet",
            module: "Chorus",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Voices,
            name: "Voices",
            module: "Chorus",
            min: 2.0,
            max: 16.0,
            default: 8.0,
            step: 1.0,
            flags: STEPPED_INT,
        }
    ];
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const STEPPED_INT: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;
