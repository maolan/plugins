//! DSP core for Maolan Synth.
//!
//! Ports key Surge XT synthesis concepts to Rust:
//! - Multi-oscillator voices with unison and FM routing
//! - Multiple filter types (SVF, Comb, Ladder, K35, Allpass)
//! - ADSR envelopes
//! - LFO modulation with step sequencer and tempo sync
//! - Polyphonic voice management with mono/legato/latch modes
//! - Full modulation matrix with programmable routings
//! - Noise generator, waveshaper, character filter

mod engine;
mod oscillator;
mod twist;
mod voice;

pub use crate::common::character::{CharacterFilter, CharacterType};
pub use crate::common::envelope::{
    AdsrEnvelope, AttackShape, DecayReleaseShape, EnvelopeMode, EnvelopeRetriggerMode,
};
pub use crate::common::filter::{Filter, FilterSubtype, FilterType};
pub use crate::common::lfo::{
    Lfo, LfoShape, LfoSyncDivision, LfoSyncMode, LfoTriggerMode, MSEG_MAX_NODES, MSEG_MAX_SEGMENTS,
    MsegCurve, MsegLoopMode,
};
pub use crate::common::mts_esp::MtsEspClient;
pub use crate::common::noise::{NoiseColorMode, NoiseGenerator, NoiseType};
pub use crate::common::settings::{
    EnvelopeSettings, FilterSettings, LfoSettings, WaveshaperSettings,
};
pub use crate::common::tuning::Tuning;
pub use crate::common::voice::{PlayMode, PortamentoCurve, StealMode, VoicePriority};
pub use crate::common::waveshaper::{Waveshape, Waveshaper};

pub use engine::SynthEngine;
pub use oscillator::{
    AliasWaveform, ClassicWaveform, ExciterType, Fm2FeedbackMode, Fm3FeedbackMode,
    ModernSubWaveform, OscType, Oscillator, SineShaperMode, WindowType,
};
pub use twist::{TwistModel, TwistOsc};
pub use voice::{
    CombinatorMode, FilterRouting, ModDepthCurve, ModRouting, ModSource, ModTarget, ModValues,
    NoiseSettings, OscFmMode, OscPhaseMode, OscRoute, OscSettings, Voice, VoiceParams,
};
