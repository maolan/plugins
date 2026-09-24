use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_ENUM, CLAP_PARAM_IS_STEPPED,
    CLAP_PARAM_REQUIRES_PROCESS,
};

crate::define_params! {
    pub enum ParamId {
        Boost = 0,
        Ceiling = 1,
        Lookahead = 2,
        Attack = 3,
        Release = 4,
        LinkTransients = 5,
        LinkRelease = 6,
        Channels = 7,
        OutputGain = 8,
        Mode = 9,
        Envelope = 10,
        Window = 11,
        Oversampling = 12,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::Boost,
            name: "Gain",
            module: "Limiter",
            min: -90.0,
            max: 20.0,
            default: 0.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Ceiling,
            name: "Ceiling",
            module: "Limiter",
            min: -90.0,
            max: 0.0,
            default: 0.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Lookahead,
            name: "Lookahead",
            module: "Envelope",
            min: 0.0,
            max: 20.0,
            default: 1.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Attack,
            name: "Attack",
            module: "Envelope",
            min: 0.0,
            max: 10.0,
            default: 1.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Release,
            name: "Release",
            module: "Envelope",
            min: 1.0,
            max: 999.0,
            default: 250.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::LinkTransients,
            name: "Channel Linking Transients",
            module: "Channel Linking",
            min: 0.0,
            max: 100.0,
            default: 100.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::LinkRelease,
            name: "Channel Linking Release",
            module: "Channel Linking",
            min: 0.0,
            max: 100.0,
            default: 100.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Channels,
            name: "Channels",
            module: "Global",
            min: 1.0,
            max: 2.0,
            default: 2.0,
            step: 1.0,
            flags: ENUM_FLAGS,
        }
        {
            id: ParamId::OutputGain,
            name: "Output Volume",
            module: "Limiter",
            min: -90.0,
            max: 20.0,
            default: 0.0,
            step: 0.1,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Mode,
            name: "Mode",
            module: "Shape",
            min: 1.0,
            max: 1.0,
            default: 1.0,
            step: 1.0,
            flags: ENUM_FLAGS,
        }
        {
            id: ParamId::Envelope,
            name: "Envelope",
            module: "Shape",
            min: 1.0,
            max: 1.0,
            default: 1.0,
            step: 1.0,
            flags: ENUM_FLAGS,
        }
        {
            id: ParamId::Window,
            name: "Window",
            module: "Shape",
            min: 0.0,
            max: 100.0,
            default: 25.0,
            step: 1.0,
            flags: AUTOMATABLE,
        }
        {
            id: ParamId::Oversampling,
            name: "True Peak",
            module: "Envelope",
            min: 0.0,
            max: 3.0,
            default: 0.0,
            step: 1.0,
            flags: ENUM_FLAGS,
        }
    ];
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const ENUM_FLAGS: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED | CLAP_PARAM_IS_ENUM;

pub type ParamStore = crate::common::param_store::ParamStore<ParamId>;
