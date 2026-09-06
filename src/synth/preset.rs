use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};

use crate::synth::params::{ParamId, ParamStore, param_def, sanitize_param_value};

const PRESET_FORMAT: &str = "maolan-synth-preset";
const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthPreset {
    pub format: String,
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub parameters: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom_wavetables: BTreeMap<String, String>,
}

impl SynthPreset {
    pub fn from_runtime(
        store: &ParamStore,
        name: Option<String>,
        wavetable_paths: &[(u8, String)],
    ) -> Self {
        let parameters = ParamId::all()
            .filter(|id| param_def(*id).is_some())
            .map(|id| (param_key(id), store.get(id)))
            .collect();
        let custom_wavetables = wavetable_paths
            .iter()
            .filter_map(|(osc_index, path)| {
                wavetable_key(*osc_index).map(|key| (key, path.clone()))
            })
            .collect();
        Self {
            format: PRESET_FORMAT.to_string(),
            version: CURRENT_VERSION,
            name,
            parameters,
            custom_wavetables,
        }
    }

    pub fn apply(&self, store: &ParamStore) {
        for (key, value) in &self.parameters {
            let Some(id) = param_id_for_key(key) else {
                continue;
            };
            store.set(id, sanitize_param_value(id, *value));
        }
    }

    pub fn wavetable_paths(&self) -> Vec<(u8, String)> {
        self.custom_wavetables
            .iter()
            .filter_map(|(key, path)| {
                wavetable_index_for_key(key).map(|index| (index, path.clone()))
            })
            .collect()
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| format!("failed to serialize preset: {error}"))?;
        std::fs::write(path, bytes)
            .map_err(|error| format!("failed to write {}: {error}", path.display()))
    }

    pub fn load_from_path(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let preset: SynthPreset = serde_json::from_slice(&bytes)
            .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
        if preset.format != PRESET_FORMAT {
            return Err(format!(
                "unsupported preset format '{}': expected '{PRESET_FORMAT}'",
                preset.format
            ));
        }
        if preset.version > CURRENT_VERSION {
            return Err(format!(
                "unsupported preset version {}: current version is {CURRENT_VERSION}",
                preset.version
            ));
        }
        Ok(preset)
    }
}

fn param_key(id: ParamId) -> String {
    format!("{id:?}")
}

fn param_id_for_key(key: &str) -> Option<ParamId> {
    ParamId::all().find(|id| param_key(*id) == key)
}

fn wavetable_key(osc_index: u8) -> Option<String> {
    match osc_index {
        0 => Some("osc1".to_string()),
        1 => Some("osc2".to_string()),
        2 => Some("osc3".to_string()),
        _ => None,
    }
}

fn wavetable_index_for_key(key: &str) -> Option<u8> {
    match key {
        "osc1" => Some(0),
        "osc2" => Some(1),
        "osc3" => Some(2),
        _ => None,
    }
}
