use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_ENUM, CLAP_PARAM_IS_HIDDEN, CLAP_PARAM_IS_STEPPED,
    CLAP_PARAM_REQUIRES_PROCESS,
};

crate::define_params! {
    pub enum ParamId {
        Replace = 0,
        Brightness = 1,
        Detune = 2,
        Bigness = 3,
        DryWet = 4,
        Channels = 5,
        PreDelay = 6,
        Decay = 7,
        Size = 8,
        Attack = 9,
        BassMult = 10,
        BassFreq = 11,
        HighShelf = 12,
        HighFreq = 13,
        Early = 14,
        Late = 15,
        ModRate = 16,
        ModDepth = 17,
        HighCut = 18,
        LowCut = 19,
        Mode = 20,
        Color = 21,
        MixLock = 22,

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
            flags: AUTOMATABLE | CLAP_PARAM_IS_HIDDEN,
        }
        {
            id: ParamId::Brightness,
            name: "Brightness",
            module: "Reverb",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE | CLAP_PARAM_IS_HIDDEN,
        }
        {
            id: ParamId::Detune,
            name: "Detune",
            module: "Reverb",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            flags: AUTOMATABLE | CLAP_PARAM_IS_HIDDEN,
        }
        {
            id: ParamId::Bigness,
            name: "Bigness",
            module: "Reverb",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            step: 0.01,
            flags: AUTOMATABLE | CLAP_PARAM_IS_HIDDEN,
        }
        {
            id: ParamId::DryWet,
            name: "Dry/Wet",
            module: "Reverb",
            min: 0.0,
            max: 1.0,
            default: 0.5,
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
        {
            id: ParamId::PreDelay,
            name: "Predelay",
            module: "Reverb",
            min: 0.0,
            max: 500.0,
            default: 0.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Decay,
            name: "Decay",
            module: "Reverb",
            min: 0.2,
            max: 70.0,
            default: 4.0,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Size,
            name: "Size",
            module: "Shape",
            min: 0.0,
            max: 100.0,
            default: 100.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Attack,
            name: "Attack",
            module: "Shape",
            min: 0.0,
            max: 100.0,
            default: 50.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::BassMult,
            name: "BassMult",
            module: "Damping",
            min: 0.25,
            max: 4.0,
            default: 1.5,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::BassFreq,
            name: "BassFreq",
            module: "Damping",
            min: 100.0,
            max: 10000.0,
            default: 700.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::HighShelf,
            name: "HighShelf",
            module: "Damping",
            min: -24.0,
            max: 0.0,
            default: -24.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::HighFreq,
            name: "HighFreq",
            module: "Damping",
            min: 100.0,
            max: 20000.0,
            default: 6000.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Early,
            name: "Early",
            module: "Diffusion",
            min: 0.0,
            max: 100.0,
            default: 100.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Late,
            name: "Late",
            module: "Diffusion",
            min: 0.0,
            max: 100.0,
            default: 100.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::ModRate,
            name: "Rate",
            module: "Modulation",
            min: 0.1,
            max: 10.0,
            default: 2.53,
            step: 0.01,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::ModDepth,
            name: "Depth",
            module: "Modulation",
            min: 0.0,
            max: 100.0,
            default: 0.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::HighCut,
            name: "HighCut",
            module: "EQ",
            min: 100.0,
            max: 20000.0,
            default: 20000.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::LowCut,
            name: "LowCut",
            module: "EQ",
            min: 10.0,
            max: 1500.0,
            default: 10.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Mode,
            name: "Mode",
            module: "Reverb",
            min: 0.0,
            max: 21.0,
            default: 0.0,
            step: 1.0,
            flags: STEPPED_BOOL | CLAP_PARAM_IS_ENUM,
        }
        {
            id: ParamId::Color,
            name: "Color",
            module: "Reverb",
            min: 0.0,
            max: 0.0,
            default: 0.0,
            step: 1.0,
            flags: STEPPED_BOOL | CLAP_PARAM_IS_ENUM | CLAP_PARAM_IS_HIDDEN,
        }
        {
            id: ParamId::MixLock,
            name: "Mix Lock",
            module: "Reverb",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 1.0,
            flags: STEPPED_BOOL | CLAP_PARAM_IS_HIDDEN,
        }
    ];
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const STEPPED_BOOL: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;

pub const MODES: [&str; 22] = [
    "Concert Hall",
    "Plate",
    "Room",
    "Chamber",
    "Random Space",
    "Chorus Space",
    "Ambience",
    "Bright Hall",
    "Sanctuary",
    "Dirty Hall",
    "Dirty Plate",
    "Smooth Plate",
    "Smooth Room",
    "Smooth Random",
    "Nonlin",
    "Chaotic Chamber",
    "Chaotic Hall",
    "Chaotic Neutral",
    "Cathedral",
    "Palace",
    "Chamber1979",
    "Hall1984",
];
pub const COLORS: [&str; 1] = ["NOW"];

pub fn value_text(id: ParamId, value: f64) -> String {
    match id {
        ParamId::Mode => MODES[(value.round() as usize).min(MODES.len() - 1)].into(),
        ParamId::Color => COLORS[(value.round() as usize).min(COLORS.len() - 1)].into(),
        ParamId::Channels => if value >= 2.0 { "Stereo" } else { "Mono" }.into(),
        ParamId::MixLock => if value >= 0.5 { "Locked" } else { "Unlocked" }.into(),
        ParamId::DryWet => format!("{:.1} %", value * 100.0),
        ParamId::Size | ParamId::Attack | ParamId::Early | ParamId::Late | ParamId::ModDepth => {
            format!("{value:.1} %")
        }
        ParamId::PreDelay => format!("{value:.2} ms"),
        ParamId::Decay => format!("{value:.2} s"),
        ParamId::BassMult => format!("{value:.2} x"),
        ParamId::HighShelf => format!("{value:.1} dB"),
        ParamId::BassFreq | ParamId::HighFreq | ParamId::HighCut | ParamId::LowCut => {
            format!("{value:.0} Hz")
        }
        ParamId::ModRate => format!("{value:.2} Hz"),
        _ => format!("{value:.2}"),
    }
}
