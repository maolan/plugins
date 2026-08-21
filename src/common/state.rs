use serde::{Deserialize, Serialize};

use crate::common::ClapParamId;
use crate::common::param_store::ParamStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplerZoneState {
    pub name: String,
    pub files: Vec<String>,
    pub start_note: usize,
    pub end_note: usize,
    pub vel_low: u8,
    pub vel_high: u8,
    #[serde(default)]
    pub group: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplerGroupState {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginState {
    pub version: u32,
    pub params: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampler_zones: Option<Vec<SamplerZoneState>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampler_groups: Option<Vec<SamplerGroupState>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampler_instrument_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampler_sf2_preset: Option<usize>,
}

impl PluginState {
    pub const CURRENT_VERSION: u32 = 1;

    pub fn from_runtime<P: ClapParamId>(store: &ParamStore<P>) -> Self {
        let params = (0..P::COUNT)
            .filter_map(|i| P::from_raw(i as u32))
            .map(|id| store.get(id))
            .collect();
        Self {
            version: Self::CURRENT_VERSION,
            params,
            sampler_zones: None,
            sampler_groups: None,
            sampler_instrument_path: None,
            sampler_sf2_preset: None,
        }
    }

    pub fn apply<P: ClapParamId>(&self, store: &ParamStore<P>) {
        for (idx, &value) in self.params.iter().enumerate() {
            if let Some(id) = P::from_raw(idx as u32) {
                store.set(id, value);
            }
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}
