use std::collections::BTreeMap;

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

use crate::modeler::params::{PARAMS, ParamStore, sanitize_param_value};

const CURRENT_STATE_VERSION: &str = "0.3.0";
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
    /// Tone3000 download reference (tone id or asset URL) for the NAM model,
    /// recorded only when the file was downloaded from Tone3000. Used to
    /// re-download the file when the stored path no longer exists.
    #[serde(default, rename = "NAMTone3000Ref")]
    pub model_tone3000_reference: Option<String>,
    /// Same as `model_tone3000_reference` for the impulse response.
    #[serde(default, rename = "IRTone3000Ref")]
    pub ir_tone3000_reference: Option<String>,
    /// Display slug (`user/profile/variant`) for a Tone3000-downloaded NAM
    /// model; empty for locally loaded files.
    #[serde(default, rename = "NAMTone3000Name")]
    pub model_tone3000_name: Option<String>,
    /// Same as `model_tone3000_name` for the impulse response.
    #[serde(default, rename = "IRTone3000Name")]
    pub ir_tone3000_name: Option<String>,
}

fn default_version() -> String {
    CURRENT_STATE_VERSION.to_string()
}

/// File paths, preview labels, and Tone3000 download references that
/// `PluginState` persists for the loaded NAM model and impulse response.
#[derive(Debug, Clone, Default)]
pub struct AssetPaths {
    pub model_path: String,
    pub ir_path: String,
    pub model_display_name: String,
    pub model_picture_path: String,
    pub ir_display_name: String,
    pub ir_picture_path: String,
    pub model_tone3000_reference: Option<String>,
    pub ir_tone3000_reference: Option<String>,
    pub model_tone3000_name: Option<String>,
    pub ir_tone3000_name: Option<String>,
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
            model_tone3000_reference: None,
            ir_tone3000_reference: None,
            model_tone3000_name: None,
            ir_tone3000_name: None,
        }
    }
}

impl PluginState {
    pub fn from_runtime(params: &ParamStore, paths: AssetPaths) -> Self {
        let mut params_map = BTreeMap::new();
        for def in PARAMS.iter() {
            params_map.insert(def.name.to_string(), params.get(def.id));
        }
        Self {
            version: CURRENT_STATE_VERSION.to_string(),
            params: params_map,
            model_path: paths.model_path,
            ir_path: paths.ir_path,
            model_display_name: paths.model_display_name,
            model_picture_path: paths.model_picture_path,
            ir_display_name: paths.ir_display_name,
            ir_picture_path: paths.ir_picture_path,
            model_tone3000_reference: paths.model_tone3000_reference,
            ir_tone3000_reference: paths.ir_tone3000_reference,
            model_tone3000_name: paths.model_tone3000_name,
            ir_tone3000_name: paths.ir_tone3000_name,
        }
    }

    pub fn apply(self, params: &ParamStore) -> AssetPaths {
        for def in PARAMS.iter() {
            if let Some(&value) = self.params.get(def.name) {
                params.set(def.id, sanitize_param_value(def.id, value));
            } else {
                params.set(def.id, def.default);
            }
        }
        AssetPaths {
            model_path: self.model_path,
            ir_path: self.ir_path,
            model_display_name: self.model_display_name,
            model_picture_path: self.model_picture_path,
            ir_display_name: self.ir_display_name,
            ir_picture_path: self.ir_picture_path,
            model_tone3000_reference: self.model_tone3000_reference,
            ir_tone3000_reference: self.ir_tone3000_reference,
            model_tone3000_name: self.model_tone3000_name,
            ir_tone3000_name: self.ir_tone3000_name,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> PluginState {
        PluginState {
            model_path: "/home/user/.cache/maolan/tone3000/m.nam".to_string(),
            ir_path: "/home/user/.cache/maolan/tone3000/ir.wav".to_string(),
            model_display_name: "Lead".to_string(),
            ir_display_name: "Cab".to_string(),
            model_tone3000_reference: Some("12345".to_string()),
            ir_tone3000_reference: Some("67890".to_string()),
            model_tone3000_name: Some("jane/ENGL E650/SM57".to_string()),
            ir_tone3000_name: Some("jane/OwnHammer 4x12/MD421".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn state_round_trip_preserves_tone3000_references() {
        let state = sample_state();
        let bytes = state.to_bytes().expect("serialize state");
        let parsed = PluginState::from_bytes(&bytes).expect("parse state");

        assert_eq!(parsed.model_tone3000_reference, Some("12345".to_string()));
        assert_eq!(parsed.ir_tone3000_reference, Some("67890".to_string()));
        assert_eq!(
            parsed.model_tone3000_name,
            Some("jane/ENGL E650/SM57".to_string())
        );
        assert_eq!(
            parsed.ir_tone3000_name,
            Some("jane/OwnHammer 4x12/MD421".to_string())
        );
        assert_eq!(parsed.model_path, state.model_path);
        assert_eq!(parsed.model_display_name, state.model_display_name);
    }

    #[test]
    fn legacy_state_without_tone3000_fields_still_loads() {
        let legacy = r#"{
            "version": "0.1.0",
            "params": {"Input": -6.0},
            "NAMPath": "/tmp/gone.nam",
            "IRPath": "/tmp/gone.wav",
            "NAMDisplayName": "Old",
            "NAMPicturePath": "",
            "IRDisplayName": "Old IR",
            "IRPicturePath": ""
        }"#;
        let mut text = format!("{STATE_HEADER_PREFIX}0.1.0\n");
        text.push_str(legacy);
        let parsed = PluginState::from_bytes(text.as_bytes()).expect("parse legacy state");

        assert_eq!(parsed.model_path, "/tmp/gone.nam");
        assert_eq!(parsed.ir_path, "/tmp/gone.wav");
        assert_eq!(parsed.model_tone3000_reference, None);
        assert_eq!(parsed.ir_tone3000_reference, None);
        assert_eq!(parsed.model_tone3000_name, None);
        assert_eq!(parsed.ir_tone3000_name, None);
    }

    #[test]
    fn state_0_2_0_without_tone3000_names_still_loads() {
        // 0.2.0 states have the re-download references but not the display names.
        let mut text = format!("{STATE_HEADER_PREFIX}0.2.0\n");
        text.push_str(
            r#"{
            "version": "0.2.0",
            "params": {},
            "NAMPath": "/tmp/gone.nam",
            "IRPath": "",
            "NAMDisplayName": "Old",
            "NAMPicturePath": "",
            "IRDisplayName": "",
            "IRPicturePath": "",
            "NAMTone3000Ref": "12345",
            "IRTone3000Ref": null
        }"#,
        );
        let parsed = PluginState::from_bytes(text.as_bytes()).expect("parse 0.2.0 state");

        assert_eq!(parsed.model_tone3000_reference, Some("12345".to_string()));
        assert_eq!(parsed.model_tone3000_name, None);
        assert_eq!(parsed.ir_tone3000_name, None);
    }

    #[test]
    fn state_without_references_serializes_them_as_null() {
        let state = PluginState::default();
        let bytes = state.to_bytes().expect("serialize state");
        let text = String::from_utf8(bytes).expect("state is utf-8");
        assert!(text.contains("\"NAMTone3000Ref\":null"));
        assert!(text.contains("\"IRTone3000Ref\":null"));
        assert!(text.contains("\"NAMTone3000Name\":null"));
        assert!(text.contains("\"IRTone3000Name\":null"));
    }
}
