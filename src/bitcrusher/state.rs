use crate::bitcrusher::params::ParamId;

pub type PluginState = crate::common::json_state::JsonParamState<ParamId>;

pub const STATE_HEADER_PREFIX: &str = "maolan-bitcrusher-state-v";
