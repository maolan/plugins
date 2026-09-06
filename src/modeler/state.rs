use std::collections::BTreeMap;

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

use crate::modeler::params::{PARAMS, ParamStore, sanitize_param_value};

const CURRENT_STATE_VERSION: &str = "0.1.0";
const STATE_HEADER_PREFIX: &str = "maolan-modeler-state-v";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginState {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default, deserialize_with = "deserialize_params")]
    pub params: BTreeMap<String, f64>,
    #[serde(default, rename = "NAMPath")]
    pub model_path: String,
    #[serde(default, rename = "IRPath")]
    pub ir_path: String,
    #[serde(default, rename = "NAMDisplayName")]
    pub model_display_name: String,
    #[serde(default, rename = "NAMPicturePath")]
    pub model_picture_path: String,
    #[serde(default, rename = "IRDisplayName")]
    pub ir_display_name: String,
    #[serde(default, rename = "IRPicturePath")]
    pub ir_picture_path: String,
}

fn default_version() -> String {
    CURRENT_STATE_VERSION.to_string()
}

impl Default for PluginState {
    fn default() -> Self {
        Self {
            version: CURRENT_STATE_VERSION.to_string(),
            params: BTreeMap::new(),
            model_path: String::new(),
            ir_path: String::new(),
            model_display_name: String::new(),
            model_picture_path: String::new(),
            ir_display_name: String::new(),
            ir_picture_path: String::new(),
        }
    }
}

impl PluginState {
    pub fn from_runtime(
        params: &ParamStore,
        model_path: String,
        ir_path: String,
        model_display_name: String,
        model_picture_path: String,
        ir_display_name: String,
        ir_picture_path: String,
    ) -> Self {
        let mut params_map = BTreeMap::new();
        for def in PARAMS.iter() {
            params_map.insert(def.name.to_string(), params.get(def.id));
        }
        Self {
            version: CURRENT_STATE_VERSION.to_string(),
            params: params_map,
            model_path,
            ir_path,
            model_display_name,
            model_picture_path,
            ir_display_name,
            ir_picture_path,
        }
    }

    pub fn apply(self, params: &ParamStore) -> (String, String, String, String, String, String) {
        for def in PARAMS.iter() {
            if let Some(&value) = self.params.get(def.name) {
                params.set(def.id, sanitize_param_value(def.id, value));
            } else {
                params.set(def.id, def.default);
            }
        }
        (
            self.model_path,
            self.ir_path,
            self.model_display_name,
            self.model_picture_path,
            self.ir_display_name,
            self.ir_picture_path,
        )
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

fn deserialize_params<'de, D>(deserializer: D) -> Result<BTreeMap<String, f64>, D::Error>
where
    D: Deserializer<'de>,
{
    struct ParamsVisitor;

    impl<'de> Visitor<'de> for ParamsVisitor {
        type Value = BTreeMap<String, f64>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str(
                "an array of f64 values (legacy index-based) or a map of parameter names to f64 values",
            )
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut map = BTreeMap::new();
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
            let mut map = BTreeMap::new();
            while let Some((key, value)) = map_access.next_entry::<String, f64>()? {
                map.insert(key, value);
            }
            Ok(map)
        }
    }

    deserializer.deserialize_any(ParamsVisitor)
}
