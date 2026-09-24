use crate::reverb::params::ParamId;

pub type PluginState = crate::common::json_state::JsonParamState<ParamId>;

pub const STATE_HEADER_PREFIX: &str = "maolan-reverb-state-v";
