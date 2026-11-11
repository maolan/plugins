use crate::transient::params::ParamId;

pub type PluginState = crate::common::json_state::JsonParamState<ParamId>;

pub const STATE_HEADER_PREFIX: &str = "maolan-transient-state-v";
