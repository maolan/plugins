use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::random::params::{PARAMS, ParamStore, sanitize_param_value};

const CURRENT_STATE_VERSION: &str = "0.1.0";
const STATE_HEADER_PREFIX: &str = "maolan-random-state-v";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginState {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
}

fn default_version() -> String {
    CURRENT_STATE_VERSION.to_string()
}

impl Default for PluginState {
    fn default() -> Self {
        Self {
            version: CURRENT_STATE_VERSION.to_string(),
            params: BTreeMap::new(),
        }
    }
}

impl PluginState {
    pub fn from_runtime(params: &ParamStore) -> Self {
        let mut params_map = BTreeMap::new();
        for def in PARAMS.iter() {
            params_map.insert(def.name.to_string(), params.get(def.id));
        }
        Self {
            version: CURRENT_STATE_VERSION.to_string(),
            params: params_map,
        }
    }

    pub fn apply(self, params: &ParamStore) {
        for def in PARAMS.iter() {
            if let Some(&value) = self.params.get(def.name) {
                params.set(def.id, sanitize_param_value(def.id, value));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::params::ParamId;
    #[test]
    fn restores_lengths_and_note_range() {
        let params = ParamStore::default();
        params.set(ParamId::NoteLength, 2.0);
        params.set(ParamId::PauseLength, 7.0);
        params.set(ParamId::LowestNote, 55.0);
        params.set(ParamId::HighestNote, 67.0);
        let bytes = PluginState::from_runtime(&params).to_bytes().unwrap();
        let restored = ParamStore::default();
        PluginState::from_bytes(&bytes).unwrap().apply(&restored);
        assert_eq!(restored.get(ParamId::NoteLength), 2.0);
        assert_eq!(restored.get(ParamId::PauseLength), 7.0);
        assert_eq!(restored.get(ParamId::LowestNote), 55.0);
        assert_eq!(restored.get(ParamId::HighestNote), 67.0);
    }
}
