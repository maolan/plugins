use maolan_clap::ffi::{CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_REQUIRES_PROCESS};

crate::define_params! {
    pub enum ParamId {
        Attack = 0,
        Sustain = 1,
        AttackTime = 2,
        ReleaseTime = 3,
        DryWet = 4,
        OutputGain = 5,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::Attack,
            name: "Attack",
            module: "Transient",
            min: -24.0,
            max: 24.0,
            default: 0.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Sustain,
            name: "Sustain",
            module: "Transient",
            min: -24.0,
            max: 24.0,
            default: 0.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::AttackTime,
            name: "Attack Time",
            module: "Detector",
            min: 0.1,
            max: 20.0,
            default: 1.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::ReleaseTime,
            name: "Release Time",
            module: "Detector",
            min: 10.0,
            max: 1000.0,
            default: 100.0,
            step: 1.0,
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
