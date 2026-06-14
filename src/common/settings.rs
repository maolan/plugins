use crate::common::{
    envelope::{AttackShape, DecayReleaseShape, EnvelopeMode, EnvelopeRetriggerMode},
    filter::{FilterSubtype, FilterType},
    lfo::{LfoShape, LfoSyncDivision, LfoSyncMode, LfoTriggerMode},
    waveshaper::Waveshape,
};

#[derive(Debug, Clone)]
pub struct FilterSettings {
    pub filter_type: FilterType,
    pub subtype: FilterSubtype,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub eg_amount: f32,
    pub key_tracking: f32,
    pub drive: f32,
    pub feedback_drive: f32,
    pub enabled: bool,
}

impl Default for FilterSettings {
    fn default() -> Self {
        Self {
            filter_type: FilterType::Lowpass,
            subtype: FilterSubtype::Clean,
            cutoff_hz: 20000.0,
            resonance: 0.7,
            eg_amount: 0.0,
            key_tracking: 0.0,
            drive: 0.0,
            feedback_drive: 0.0,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EnvelopeSettings {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub mode: EnvelopeMode,
    pub attack_shape: AttackShape,
    pub decay_shape: DecayReleaseShape,
    pub release_shape: DecayReleaseShape,
    pub retrigger_mode: EnvelopeRetriggerMode,
    pub tempo_sync: bool,
    pub uber_release: f32,
    pub gated_release: bool,
    pub correct_analog_mode: bool,
}

impl Default for EnvelopeSettings {
    fn default() -> Self {
        Self {
            attack: 0.01,
            decay: 0.2,
            sustain: 0.7,
            release: 0.3,
            mode: EnvelopeMode::Digital,
            attack_shape: AttackShape::Convex,
            decay_shape: DecayReleaseShape::Linear,
            release_shape: DecayReleaseShape::Linear,
            retrigger_mode: EnvelopeRetriggerMode::Reset,
            tempo_sync: false,
            uber_release: 0.0,
            gated_release: false,
            correct_analog_mode: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LfoSettings {
    pub rate_hz: f32,
    pub shape: LfoShape,
    pub amount: f32,
    pub deform: f32,
    pub deform_type: u8,
    pub enabled: bool,
    pub sync_mode: LfoSyncMode,
    pub sync_division: LfoSyncDivision,
    pub trigger_mode: LfoTriggerMode,
    pub env_delay: f32,
    pub env_attack: f32,
    pub env_hold: f32,
    pub env_decay: f32,
    pub env_sustain: f32,
    pub env_release: f32,
    pub start_phase: f32,
    pub unipolar: bool,
    pub env_tempo_sync: bool,
}

impl Default for LfoSettings {
    fn default() -> Self {
        Self {
            rate_hz: 1.0,
            shape: LfoShape::Sine,
            amount: 0.0,
            deform: 0.0,
            deform_type: 0,
            enabled: true,
            sync_mode: LfoSyncMode::Free,
            sync_division: LfoSyncDivision::One4,
            trigger_mode: LfoTriggerMode::KeyTrigger,
            env_delay: 0.0,
            env_attack: 0.01,
            env_hold: 0.0,
            env_decay: 0.2,
            env_sustain: 1.0,
            env_release: 0.3,
            start_phase: 0.0,
            unipolar: false,
            env_tempo_sync: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WaveshaperSettings {
    pub shape: Waveshape,
    pub drive: f32,
    pub mix: f32,
    pub enabled: bool,
}

impl Default for WaveshaperSettings {
    fn default() -> Self {
        Self {
            shape: Waveshape::Off,
            drive: 0.0,
            mix: 1.0,
            enabled: false,
        }
    }
}
