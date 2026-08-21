use serde::{Deserialize, Serialize};

use crate::common::ClapParamId;
use crate::common::param_store::ParamStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplerModRouteState {
    pub source: u8,
    #[serde(default)]
    pub source_cc: u8,
    pub target: u8,
    pub depth: f32,
    #[serde(default)]
    pub active: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_curve: Vec<f32>,
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_key: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_fade_low: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_fade_high: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vel_fade_low: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vel_fade_high: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_fade_in: Option<(u8, u8)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_fade_out: Option<(u8, u8)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vel_fade_in: Option<(u8, u8)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vel_fade_out: Option<(u8, u8)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch_offset: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_tracking: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity_curve: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_tracking_curve: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain_db: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pan: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amp_keytrack_db: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub play_mode: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_mode: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_direction: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_start: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_end: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_crossfade: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_offset: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_random: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_offset: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_random: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch_bend_up: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch_bend_down: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant_mode: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_low: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_high: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch_bend_low: Option<i16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch_bend_high: Option<i16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cc_conditions: Vec<(u8, u8, u8)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub random_low: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub random_high: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq_position: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off_by: Option<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mod_routes: Vec<SamplerModRouteState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_sfz_opcodes: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplerGroupState {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poly_limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusive_group: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain_db: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pan: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_sfz_opcodes: Vec<(String, String)>,
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
