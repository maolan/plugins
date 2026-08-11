use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::kick::dsp::{
    INSTRUMENTS_PER_KIT, LAYERS_PER_INSTRUMENT, OSCILLATORS_PER_LAYER,
    envelope::{EnvPoint, Envelope},
};
use crate::kick::params::{ParamId, ParamStore, param_type_def, sanitize_param_value, state_key};

const CURRENT_STATE_VERSION: &str = "0.2.0";
const STATE_HEADER_PREFIX: &str = "maolan-kick-state-v";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KitState {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub kit: KitConfig,
    #[serde(default, deserialize_with = "deserialize_params")]
    pub params: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct KitConfig {
    #[serde(default, skip_serializing_if = "is_default")]
    pub humanizer_velocity: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub humanizer_timing_ms: f32,
    #[serde(
        default = "default_instruments",
        skip_serializing_if = "is_default_instruments"
    )]
    pub instruments: Vec<InstrumentConfig>,
}

fn default_instruments() -> Vec<InstrumentConfig> {
    (0..INSTRUMENTS_PER_KIT)
        .map(|_| InstrumentConfig::default())
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstrumentConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default = "default_layers", skip_serializing_if = "is_default_layers")]
    pub layers: Vec<LayerConfig>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub master_filter_type: u8,
    #[serde(default = "default_cutoff", skip_serializing_if = "is_default_cutoff")]
    pub master_filter_cutoff_hz: f32,
    #[serde(default = "default_q", skip_serializing_if = "is_default_q")]
    pub master_filter_q: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub master_distortion_type: u8,
    #[serde(default, skip_serializing_if = "is_default")]
    pub master_distortion_drive: f32,
    #[serde(default = "default_limit", skip_serializing_if = "is_default_limit")]
    pub master_distortion_input_limit: f32,
    #[serde(default = "default_limit", skip_serializing_if = "is_default_limit")]
    pub master_distortion_output_limit: f32,
    #[serde(
        default = "default_flat_env",
        skip_serializing_if = "is_default_flat_env"
    )]
    pub master_distortion_volume_env: SerdeEnvelope,
    #[serde(
        default = "default_limiter_threshold",
        skip_serializing_if = "is_default"
    )]
    pub master_limiter_threshold_db: f32,
    #[serde(
        default = "default_limiter_release",
        skip_serializing_if = "is_default_limiter_release"
    )]
    pub master_limiter_release_ms: f32,
    #[serde(default = "default_length", skip_serializing_if = "is_default_length")]
    pub length_ms: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub output_gain_db: f32,
    #[serde(
        default = "default_note_off",
        skip_serializing_if = "is_default_note_off"
    )]
    pub note_off_decay_ms: f32,
    #[serde(default = "default_true", skip_serializing_if = "is_default_true")]
    pub note_off_enabled: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub pitch_to_note: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub key_min: u8,
    #[serde(
        default = "default_key_max",
        skip_serializing_if = "is_default_key_max"
    )]
    pub key_max: u8,
    #[serde(default, skip_serializing_if = "is_default")]
    pub midi_channel: u8,
    #[serde(default, skip_serializing_if = "is_default")]
    pub muted: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub soloed: bool,
    #[serde(default, skip_serializing_if = "is_default_amp_env")]
    pub global_amp_env: SerdeEnvelope,
}

impl Default for InstrumentConfig {
    fn default() -> Self {
        Self {
            layers: default_layers(),
            master_filter_type: 0,
            master_filter_cutoff_hz: 20000.0,
            master_filter_q: 0.7,
            master_distortion_type: 1,
            master_distortion_drive: 0.0,
            master_distortion_input_limit: 1.0,
            master_distortion_output_limit: 1.0,
            master_distortion_volume_env: default_flat_env(),
            master_limiter_threshold_db: 0.0,
            master_limiter_release_ms: 50.0,
            length_ms: 300.0,
            output_gain_db: 0.0,
            note_off_decay_ms: 30.0,
            note_off_enabled: true,
            pitch_to_note: false,
            key_min: 0,
            key_max: 127,
            midi_channel: 0,
            muted: false,
            soloed: false,
            global_amp_env: default_amp_env(),
            name: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayerConfig {
    #[serde(
        default = "default_oscillators",
        skip_serializing_if = "is_default_oscillators"
    )]
    pub oscillators: Vec<OscillatorConfig>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub noise: NoiseConfig,
    #[serde(default = "default_true", skip_serializing_if = "is_default_true")]
    pub enabled: bool,
    #[serde(default = "default_one", skip_serializing_if = "is_default_one")]
    pub amplitude: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub filter_type: u8,
    #[serde(default = "default_cutoff", skip_serializing_if = "is_default_cutoff")]
    pub filter_cutoff_hz: f32,
    #[serde(default = "default_q", skip_serializing_if = "is_default_q")]
    pub filter_q: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub distortion_type: u8,
    #[serde(default, skip_serializing_if = "is_default")]
    pub distortion_drive: f32,
    #[serde(
        default = "default_flat_env",
        skip_serializing_if = "is_default_flat_env"
    )]
    pub distortion_volume_env: SerdeEnvelope,
    #[serde(default, skip_serializing_if = "is_default")]
    pub fm_routing: Vec<u8>,
}

impl Default for LayerConfig {
    fn default() -> Self {
        Self {
            oscillators: default_oscillators(),
            noise: NoiseConfig::default(),
            enabled: true,
            amplitude: 1.0,
            filter_type: 0,
            filter_cutoff_hz: 20000.0,
            filter_q: 0.7,
            distortion_type: 1,
            distortion_drive: 0.0,
            distortion_volume_env: default_flat_env(),
            fm_routing: vec![0, 0, 0],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OscillatorConfig {
    #[serde(default, skip_serializing_if = "is_default")]
    pub waveform: u8,
    #[serde(default = "default_freq", skip_serializing_if = "is_default_freq")]
    pub base_freq_hz: f32,
    #[serde(
        default = "default_osc_amp",
        skip_serializing_if = "is_default_osc_amp"
    )]
    pub amplitude: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub initial_phase: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub fm_amount: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub pitch_to_note: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub filter_type: u8,
    #[serde(default = "default_cutoff", skip_serializing_if = "is_default_cutoff")]
    pub filter_cutoff_hz: f32,
    #[serde(default = "default_q", skip_serializing_if = "is_default_q")]
    pub filter_q: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub distortion_type: u8,
    #[serde(default, skip_serializing_if = "is_default")]
    pub distortion_drive: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_data: Option<String>,
    #[serde(default, skip_serializing_if = "is_default_sample_rate")]
    pub sample_rate: f32,
    #[serde(default, skip_serializing_if = "is_default_pitch_env")]
    pub pitch_env: SerdeEnvelope,
    #[serde(
        default = "default_amp_env",
        skip_serializing_if = "is_default_amp_env"
    )]
    pub amp_env: SerdeEnvelope,
    #[serde(
        default = "default_flat_env",
        skip_serializing_if = "is_default_flat_env"
    )]
    pub filter_cutoff_env: SerdeEnvelope,
    #[serde(
        default = "default_flat_env",
        skip_serializing_if = "is_default_flat_env"
    )]
    pub filter_q_env: SerdeEnvelope,
    #[serde(
        default = "default_flat_env",
        skip_serializing_if = "is_default_flat_env"
    )]
    pub distortion_drive_env: SerdeEnvelope,
    #[serde(
        default = "default_flat_env",
        skip_serializing_if = "is_default_flat_env"
    )]
    pub distortion_volume_env: SerdeEnvelope,
    #[serde(
        default = "default_flat_env",
        skip_serializing_if = "is_default_flat_env"
    )]
    pub pitch_shift_env: SerdeEnvelope,
    #[serde(
        default = "default_flat_env",
        skip_serializing_if = "is_default_flat_env"
    )]
    pub freq_env: SerdeEnvelope,
    #[serde(default, skip_serializing_if = "is_default")]
    pub freq_env_mode: u8,
}

impl Default for OscillatorConfig {
    fn default() -> Self {
        Self {
            waveform: 0,
            base_freq_hz: 150.0,
            amplitude: 0.8,
            initial_phase: 0.0,
            fm_amount: 0.0,
            pitch_to_note: false,
            filter_type: 0,
            filter_cutoff_hz: 20000.0,
            filter_q: 0.7,
            distortion_type: 1,
            distortion_drive: 0.0,
            sample_data: None,
            sample_rate: 48000.0,
            pitch_env: default_pitch_env(),
            amp_env: default_amp_env(),
            filter_cutoff_env: default_flat_env(),
            filter_q_env: default_flat_env(),
            distortion_drive_env: default_flat_env(),
            distortion_volume_env: default_flat_env(),
            pitch_shift_env: default_flat_env(),
            freq_env: default_flat_env(),
            freq_env_mode: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NoiseConfig {
    #[serde(default, skip_serializing_if = "is_default")]
    pub noise_type: u8,
    #[serde(
        default = "default_noise_amp",
        skip_serializing_if = "is_default_noise_amp"
    )]
    pub amplitude: f32,
    #[serde(
        default = "default_density",
        skip_serializing_if = "is_default_density"
    )]
    pub density: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub filter_type: u8,
    #[serde(default = "default_cutoff", skip_serializing_if = "is_default_cutoff")]
    pub filter_cutoff_hz: f32,
    #[serde(default = "default_q", skip_serializing_if = "is_default_q")]
    pub filter_q: f32,
    #[serde(
        default = "default_amp_env",
        skip_serializing_if = "is_default_amp_env"
    )]
    pub amp_env: SerdeEnvelope,
    #[serde(
        default = "default_flat_env",
        skip_serializing_if = "is_default_flat_env"
    )]
    pub density_env: SerdeEnvelope,
}

impl Default for NoiseConfig {
    fn default() -> Self {
        Self {
            noise_type: 0,
            amplitude: 0.3,
            density: 0.5,
            filter_type: 0,
            filter_cutoff_hz: 8000.0,
            filter_q: 0.7,
            amp_env: default_amp_env(),
            density_env: default_flat_env(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SerdeEnvelope {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<SerdeEnvPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SerdeEnvPoint {
    #[serde(default, skip_serializing_if = "is_default")]
    pub t: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub v: f32,
    #[serde(default = "default_cp", skip_serializing_if = "is_default_cp")]
    pub cp_t: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub cp_v: f32,
}

fn default_version() -> String {
    CURRENT_STATE_VERSION.to_string()
}
fn default_true() -> bool {
    true
}
fn default_one() -> f32 {
    1.0
}
fn default_cutoff() -> f32 {
    20000.0
}
fn default_q() -> f32 {
    0.7
}
fn default_limit() -> f32 {
    1.0
}
fn default_limiter_threshold() -> f32 {
    0.0
}
fn default_limiter_release() -> f32 {
    50.0
}
fn default_length() -> f32 {
    300.0
}
fn default_note_off() -> f32 {
    30.0
}
fn default_key_max() -> u8 {
    127
}
fn default_freq() -> f32 {
    150.0
}
fn default_osc_amp() -> f32 {
    0.8
}
fn default_noise_amp() -> f32 {
    0.3
}
fn default_density() -> f32 {
    0.5
}
fn default_cp() -> f32 {
    0.33
}

fn is_default<T>(value: &T) -> bool
where
    T: Default + PartialEq,
{
    value == &T::default()
}
fn is_default_instruments(value: &[InstrumentConfig]) -> bool {
    value == default_instruments().as_slice()
}
fn is_default_layers(value: &[LayerConfig]) -> bool {
    value == default_layers().as_slice()
}
fn is_default_oscillators(value: &[OscillatorConfig]) -> bool {
    value == default_oscillators().as_slice()
}
fn is_default_true(value: &bool) -> bool {
    *value == default_true()
}
fn is_default_one(value: &f32) -> bool {
    *value == default_one()
}
fn is_default_cutoff(value: &f32) -> bool {
    *value == default_cutoff()
}
fn is_default_q(value: &f32) -> bool {
    *value == default_q()
}
fn is_default_limit(value: &f32) -> bool {
    *value == default_limit()
}
fn is_default_limiter_release(value: &f32) -> bool {
    *value == default_limiter_release()
}
fn is_default_length(value: &f32) -> bool {
    *value == default_length()
}
fn is_default_note_off(value: &f32) -> bool {
    *value == default_note_off()
}
fn is_default_key_max(value: &u8) -> bool {
    *value == default_key_max()
}
fn is_default_freq(value: &f32) -> bool {
    *value == default_freq()
}
fn is_default_osc_amp(value: &f32) -> bool {
    *value == default_osc_amp()
}
fn is_default_noise_amp(value: &f32) -> bool {
    *value == default_noise_amp()
}
fn is_default_density(value: &f32) -> bool {
    *value == default_density()
}
fn is_default_cp(value: &f32) -> bool {
    *value == default_cp()
}
fn is_default_sample_rate(value: &f32) -> bool {
    *value == 48000.0
}
fn is_default_pitch_env(value: &SerdeEnvelope) -> bool {
    *value == default_pitch_env()
}
fn is_default_amp_env(value: &SerdeEnvelope) -> bool {
    *value == default_amp_env()
}
fn is_default_flat_env(value: &SerdeEnvelope) -> bool {
    *value == default_flat_env()
}

fn default_layers() -> Vec<LayerConfig> {
    (0..LAYERS_PER_INSTRUMENT)
        .map(|_| LayerConfig::default())
        .collect()
}
fn default_oscillators() -> Vec<OscillatorConfig> {
    (0..OSCILLATORS_PER_LAYER)
        .map(|_| OscillatorConfig::default())
        .collect()
}

fn default_pitch_env() -> SerdeEnvelope {
    SerdeEnvelope {
        points: vec![
            SerdeEnvPoint {
                t: 0.0,
                v: 1.0,
                cp_t: 0.33,
                cp_v: 0.0,
            },
            SerdeEnvPoint {
                t: 1.0,
                v: 1.0,
                cp_t: 0.33,
                cp_v: 0.0,
            },
        ],
    }
}

fn default_amp_env() -> SerdeEnvelope {
    SerdeEnvelope {
        points: vec![
            SerdeEnvPoint {
                t: 0.0,
                v: 0.0,
                cp_t: 0.33,
                cp_v: 0.0,
            },
            SerdeEnvPoint {
                t: 0.001,
                v: 1.0,
                cp_t: 0.33,
                cp_v: 0.0,
            },
            SerdeEnvPoint {
                t: 0.2,
                v: 0.0,
                cp_t: 0.33,
                cp_v: 0.0,
            },
            SerdeEnvPoint {
                t: 1.0,
                v: 0.0,
                cp_t: 0.33,
                cp_v: 0.0,
            },
        ],
    }
}

fn default_flat_env() -> SerdeEnvelope {
    SerdeEnvelope {
        points: vec![
            SerdeEnvPoint {
                t: 0.0,
                v: 1.0,
                cp_t: 0.33,
                cp_v: 0.0,
            },
            SerdeEnvPoint {
                t: 1.0,
                v: 1.0,
                cp_t: 0.33,
                cp_v: 0.0,
            },
        ],
    }
}

impl KitState {
    pub fn from_runtime(params: &ParamStore, kit: &KitConfig) -> Self {
        let mut params_map = BTreeMap::new();
        for id in ParamId::all() {
            let value = params.get(id);
            let default = param_type_def(id.param_type()).default;
            if (value - default).abs() > f64::EPSILON {
                params_map.insert(state_key(id), value);
            }
        }
        Self {
            version: CURRENT_STATE_VERSION.to_string(),
            kit: kit.clone(),
            params: params_map,
        }
    }

    pub fn apply_params(self, params: &ParamStore) {
        for id in ParamId::all() {
            let key = state_key(id);
            if let Some(&value) = self.params.get(&key) {
                params.set(id, sanitize_param_value(id, value));
            } else {
                let ty = id.param_type();
                let def = param_type_def(ty);
                let legacy_key = def.base_name.to_string();
                if let Some(&value) = self.params.get(&legacy_key) {
                    params.set(id, sanitize_param_value(id, value));
                } else {
                    params.set(id, def.default);
                }
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

impl From<&SerdeEnvelope> for Envelope {
    fn from(se: &SerdeEnvelope) -> Self {
        let points: Vec<EnvPoint> = se
            .points
            .iter()
            .map(|p| EnvPoint::with_control(p.t, p.v, p.cp_t, p.cp_v))
            .collect();
        Envelope::new(points)
    }
}

impl From<&Envelope> for SerdeEnvelope {
    fn from(env: &Envelope) -> Self {
        let points: Vec<SerdeEnvPoint> = env
            .points()
            .iter()
            .map(|p| SerdeEnvPoint {
                t: p.t,
                v: p.v,
                cp_t: p.cp_t,
                cp_v: p.cp_v,
            })
            .collect();
        SerdeEnvelope { points }
    }
}

fn deserialize_params<'de, D>(deserializer: D) -> Result<BTreeMap<String, f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct ParamsVisitor;

    impl<'de> serde::de::Visitor<'de> for ParamsVisitor {
        type Value = BTreeMap<String, f64>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str(
                "an array of f64 values (legacy index-based) or a map of parameter names to f64 values",
            )
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut map = BTreeMap::new();
            let mut index = 0usize;
            while let Some(value) = seq.next_element::<f64>()? {
                if let Some(id) = ParamId::from_index(index) {
                    map.insert(state_key(id), value);
                }
                index += 1;
            }
            Ok(map)
        }

        fn visit_map<A>(self, mut map_access: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
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
