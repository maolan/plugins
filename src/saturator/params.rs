use maolan_clap::ffi::{CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_REQUIRES_PROCESS};

crate::define_params! {
    pub enum ParamId {
        Triode = 0,
        ClassAB = 1,
        ClassB = 2,
        DryWet = 3,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::Triode,
            name: "Triode",
            module: "Saturation",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::ClassAB,
            name: "Class AB",
            module: "Saturation",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::ClassB,
            name: "Class B",
            module: "Saturation",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::DryWet,
            name: "Dry/Wet",
            module: "Saturation",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
    ];
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
