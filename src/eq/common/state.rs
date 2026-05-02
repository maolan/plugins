use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::eq::common::params::{ParamDef, ParamIdExt, ParamStore, sanitize_param_value};

const CURRENT_STATE_VERSION: &str = "0.1.0";
const STATE_HEADER_PREFIX: &str = "maolan-equalizer-state-v";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginState {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub params: HashMap<String, f64>,
}

fn default_version() -> String {
    CURRENT_STATE_VERSION.to_string()
}

impl Default for PluginState {
    fn default() -> Self {
        Self {
            version: CURRENT_STATE_VERSION.to_string(),
            params: HashMap::new(),
        }
    }
}

impl PluginState {
    pub fn from_runtime<T: ParamIdExt>(params: &ParamStore<T>, defs: &[ParamDef<T>]) -> Self {
        let mut params_map = HashMap::new();
        for def in defs.iter() {
            params_map.insert(def.name.to_string(), params.get(def.id));
        }
        Self {
            version: CURRENT_STATE_VERSION.to_string(),
            params: params_map,
        }
    }

    pub fn apply<T: ParamIdExt>(self, params: &ParamStore<T>, defs: &[ParamDef<T>]) {
        for def in defs.iter() {
            if let Some(&value) = self.params.get(def.name) {
                params.set(def.id, sanitize_param_value(def.id, value, defs));
            } else {
                params.set(def.id, def.default);
            }
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut text = format!("{STATE_HEADER_PREFIX}{}\n", self.version);
        text.push_str(&serde_json::to_string(self)?);
        Ok(text.into_bytes())
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let text =
            std::str::from_utf8(bytes).map_err(|e| format!("state is not valid UTF-8: {e}"))?;
        let json_text = if let Some(line_end) = text.find('\n') {
            let header = &text[..line_end];
            if header.starts_with(STATE_HEADER_PREFIX) {
                &text[line_end + 1..]
            } else {
                text
            }
        } else {
            text
        };
        serde_json::from_str(json_text).map_err(|e| format!("failed to parse plugin state: {e}"))
    }
}
