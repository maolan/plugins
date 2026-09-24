use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED, CLAP_PARAM_REQUIRES_PROCESS,
};

crate::define_params! {
    pub enum ParamId {
        Mode = 0,
        MinCutoff = 1,
        MaxCutoff = 2,
        Resonance = 3,
        Position = 4,
        LfoRate = 5,
        LfoDepth = 6,
        LfoShape = 7,
        EnvAttack = 8,
        EnvRelease = 9,
        EnvDepth = 10,
        DryWet = 11,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::Mode,
            name: "Mode",
            module: "Global",
            min: 0.0,
            max: 2.0,
            default: 0.0,
            step: 1.0,
            flags: STEPPED,
        }
        {
            id: ParamId::MinCutoff,
            name: "Min Cutoff",
            module: "Filter",
            min: 50.0,
            max: 2000.0,
            default: 300.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::MaxCutoff,
            name: "Max Cutoff",
            module: "Filter",
            min: 500.0,
            max: 10000.0,
            default: 3000.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Resonance,
            name: "Resonance",
            module: "Filter",
            min: 0.1,
            max: 10.0,
            default: 4.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Position,
            name: "Position",
            module: "Global",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::LfoRate,
            name: "LFO Rate",
            module: "LFO",
            min: 0.1,
            max: 20.0,
            default: 2.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::LfoDepth,
            name: "LFO Depth",
            module: "LFO",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::LfoShape,
            name: "LFO Shape",
            module: "LFO",
            min: 0.0,
            max: 3.0,
            default: 0.0,
            step: 1.0,
            flags: STEPPED,
        }
        {
            id: ParamId::EnvAttack,
            name: "Env Attack",
            module: "Envelope",
            min: 1.0,
            max: 500.0,
            default: 20.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::EnvRelease,
            name: "Env Release",
            module: "Envelope",
            min: 10.0,
            max: 2000.0,
            default: 200.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::EnvDepth,
            name: "Env Depth",
            module: "Envelope",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::DryWet,
            name: "Dry/Wet",
            module: "Global",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
    ];
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const STEPPED: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;

pub const MODE_LABELS: [&str; 3] = ["Manual", "LFO", "Envelope"];
pub const SHAPE_LABELS: [&str; 4] = ["Sine", "Triangle", "Saw", "Square"];
