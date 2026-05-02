use std::collections::HashMap;

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

use crate::monitoring::params::{PARAMS, ParamStore, sanitize_param_value};

const CURRENT_STATE_VERSION: &str = "0.1.0";
const STATE_HEADER_PREFIX: &str = "maolan-monitoring-state-v";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginState {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default, deserialize_with = "deserialize_params")]
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
    pub fn from_runtime(params: &ParamStore) -> Self {
        let mut params_map = HashMap::new();
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

fn deserialize_params<'de, D>(deserializer: D) -> Result<HashMap<String, f64>, D::Error>
where
    D: Deserializer<'de>,
{
    struct ParamsVisitor;

    impl<'de> Visitor<'de> for ParamsVisitor {
        type Value = HashMap<String, f64>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str(
                "an array of f64 values (legacy index-based) or a map of parameter names to f64 values",
            )
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut map = HashMap::new();
            let mut index = 0usize;
            while let Some(value) = seq.next_element::<f64>()? {
                if let Some(def) = PARAMS.get(index) {
                    map.insert(def.name.to_string(), value);
                }
                index += 1;
            }
            Ok(map)
        }

        fn visit_map<A>(self, mut map_access: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut map = HashMap::new();
            while let Some((key, value)) = map_access.next_entry::<String, f64>()? {
                map.insert(key, value);
            }
            Ok(map)
        }
    }

    deserializer.deserialize_any(ParamsVisitor)
}
