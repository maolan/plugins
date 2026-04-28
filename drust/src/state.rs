use crate::params::{ParamId, ParamStore};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PluginState {
    pub version: u32,
    pub kit_path: String,
    pub midimap_path: String,
    #[serde(default)]
    pub variation: String,
    pub state_id: String,
    #[serde(default)]
    pub active_channels: u32,
    pub params: Vec<(u16, f64)>,
}

impl Default for PluginState {
    fn default() -> Self {
        Self {
            version: 1,
            kit_path: String::new(),
            midimap_path: String::new(),
            variation: String::new(),
            state_id: String::new(),
            active_channels: 0,
            params: Vec::new(),
        }
    }
}

impl PluginState {
    pub fn from_runtime(
        params: &ParamStore,
        kit_path: String,
        midimap_path: String,
        variation: String,
        state_id: String,
        active_channels: u32,
    ) -> Self {
        let mut param_values = Vec::new();
        for def in crate::params::PARAMS.iter() {
            param_values.push((def.id.as_u16(), params.get(def.id)));
        }
        Self {
            version: 1,
            kit_path,
            midimap_path,
            variation,
            state_id,
            active_channels,
            params: param_values,
        }
    }

    pub fn apply(&self, params: &ParamStore) -> (String, String, String, u32) {
        for &(raw, value) in &self.params {
            if let Some(id) = ParamId::from_raw(raw as u32) {
                params.set(id, crate::params::sanitize_param_value(id, value));
            }
        }
        (
            self.kit_path.clone(),
            self.midimap_path.clone(),
            self.variation.clone(),
            self.active_channels,
        )
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(self).map_err(|e| e.to_string())
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(bytes).map_err(|e| e.to_string())
    }
}
