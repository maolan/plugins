//! State serialization for Maolan Synth.

use serde::{Deserialize, Serialize};

use crate::synth::params::{ParamId, ParamStore};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginState {
    pub version: u32,
    pub params: Vec<f64>,
}

impl PluginState {
    pub const CURRENT_VERSION: u32 = 1;

    pub fn from_runtime(store: &ParamStore) -> Self {
        let params = ParamId::all().map(|id| store.get(id)).to_vec();
        Self {
            version: Self::CURRENT_VERSION,
            params,
        }
    }

    pub fn apply(&self, store: &ParamStore) {
        for (idx, &value) in self.params.iter().enumerate() {
            if let Some(id) = ParamId::from_raw(idx as u32) {
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
