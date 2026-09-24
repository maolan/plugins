use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED, CLAP_PARAM_REQUIRES_PROCESS,
};

crate::define_params! {
    pub enum ParamId {
        LfoRate = 0,
        LfoDepth = 1,
        Manual = 2,
        Feedback = 3,
        FeedbackDelayOn = 4,
        DelayTime = 5,
        Stages = 6,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::LfoRate,
            name: "LFO Rate (Hz)",
            module: "Phaser",
            min: 0.01,
            max: 2.0,
            default: 0.1,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::LfoDepth,
            name: "LFO Depth",
            module: "Phaser",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Manual,
            name: "Manual (Center)",
            module: "Phaser",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Feedback,
            name: "Feedback",
            module: "Phaser",
            min: -0.98,
            max: 0.98,
            default: 0.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::FeedbackDelayOn,
            name: "Feedback Delay On",
            module: "Phaser",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 1.0,
            flags: TOGGLE,
        }
        {
            id: ParamId::DelayTime,
            name: "Delay Time (ms)",
            module: "Phaser",
            min: 0.0,
            max: 20.0,
            default: 1.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Stages,
            name: "Stages (All-pass)",
            module: "Phaser",
            min: 1.0,
            max: 12.0,
            default: 12.0,
            step: 1.0,
            flags: STEPPED_INT,
        }
    ];
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const STEPPED_INT: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;
const TOGGLE: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;
