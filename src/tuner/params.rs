use maolan_clap::ffi::{CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_REQUIRES_PROCESS};

crate::define_params! {
    pub enum ParamId {
        ReferenceHz = 0,
        ClarityThreshold = 1,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::ReferenceHz,
            name: "Reference Hz",
            module: "Global",
            min: 420.0,
            max: 460.0,
            default: 440.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::ClarityThreshold,
            name: "Clarity Threshold",
            module: "Global",
            min: 0.0,
            max: 1.0,
            default: 0.7,
            step: 0.01,
            flags: AUTOMATABLE,
        }
    ];
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
