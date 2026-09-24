use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED, CLAP_PARAM_REQUIRES_PROCESS,
};

crate::define_params! {
    pub enum ParamId {
        TimeMode = 0,
        TimeMs = 1,
        TimeNote = 2,
        Feedback = 3,
        DryWet = 4,
        Channels = 5,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::TimeMode,
            name: "Time Mode",
            module: "Delay",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::TimeMs,
            name: "Time (ms)",
            module: "Delay",
            min: 1.0,
            max: 5000.0,
            default: 375.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::TimeNote,
            name: "Time (note)",
            module: "Delay",
            min: 0.0,
            max: 1.0,
            default: 0.75,
            step: 1.0 / 15.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Feedback,
            name: "Feedback",
            module: "Delay",
            min: 0.0,
            max: 1.0,
            default: 0.3,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::DryWet,
            name: "Dry/Wet",
            module: "Delay",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Channels,
            name: "Channels",
            module: "Global",
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

pub const NOTE_DIVISIONS: [(&str, f64); 16] = [
    ("1/1", 4.0),
    ("1/2", 2.0),
    ("1/3", 4.0 / 3.0),
    ("1/4", 1.0),
    ("1/6", 2.0 / 3.0),
    ("1/8", 0.5),
    ("1/12", 1.0 / 3.0),
    ("1/16", 0.25),
    ("1/24", 1.0 / 6.0),
    ("1/32", 0.125),
    ("1/48", 1.0 / 12.0),
    ("1/64", 0.0625),
    ("1/1d", 6.0),
    ("1/2d", 3.0),
    ("1/4d", 1.5),
    ("1/8d", 0.75),
];
