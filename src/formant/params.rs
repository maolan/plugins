use maolan_clap::ffi::{CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_REQUIRES_PROCESS};

crate::define_params! {
    pub enum ParamId {
        Vowel = 0,
        Sharpness = 1,
        OutputGain = 2,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::Vowel,
            name: "Vowel (A-E-I-O-U)",
            module: "Formant",
            min: 0.0,
            max: 4.0,
            default: 0.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Sharpness,
            name: "Sharpness (Q)",
            module: "Formant",
            min: 2.0,
            max: 40.0,
            default: 2.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::OutputGain,
            name: "Output Gain (dB)",
            module: "Formant",
            min: -60.0,
            max: 20.0,
            default: 0.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
    ];
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
