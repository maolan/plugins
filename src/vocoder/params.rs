use maolan_clap::ffi::{CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_REQUIRES_PROCESS};

crate::define_params! {
    pub enum ParamId {
        SpectralShift = 0,
        DryWet = 1,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::SpectralShift,
            name: "Spectral Shift",
            module: "Vocoder",
            min: 0.5,
            max: 4.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::DryWet,
            name: "Dry/Wet",
            module: "Vocoder",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
    ];
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
