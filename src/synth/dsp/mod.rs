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

mod character;
mod envelope;
mod engine;
mod filter;
mod lfo;
mod noise;
mod mts_esp;
mod oscillator;
mod tuning;
mod twist;
mod voice;
mod waveshaper;

pub use character::{CharacterFilter, CharacterType};
pub use envelope::{AdsrEnvelope, AttackShape, DecayReleaseShape, EnvelopeMode, EnvelopeRetriggerMode};
pub use engine::{StealMode, SynthEngine};
pub use filter::{Filter, FilterSubtype, FilterType};
pub use lfo::{Lfo, LfoShape, LfoSyncDivision, LfoSyncMode, LfoTriggerMode, MsegCurve, MsegLoopMode, MSEG_MAX_NODES, MSEG_MAX_SEGMENTS};
pub use noise::{NoiseColorMode, NoiseGenerator, NoiseType};
pub use tuning::Tuning;
pub use mts_esp::MtsEspClient;
pub use oscillator::{AliasWaveform, ClassicWaveform, ExciterType, Fm2FeedbackMode, Fm3FeedbackMode, ModernSubWaveform, OscType, Oscillator, SineShaperMode, WindowType};
pub use twist::{TwistModel, TwistOsc};
pub use voice::{
    CombinatorMode, EnvelopeSettings, FilterRouting, FilterSettings, LfoSettings, ModDepthCurve, ModRouting, ModSource, ModTarget,
    ModValues, NoiseSettings, OscFmMode, OscPhaseMode, OscRoute, OscSettings, PlayMode, PortamentoCurve,
    VoicePriority, WaveshaperSettings, Voice, VoiceParams,
};
pub use waveshaper::{Waveshape, Waveshaper};
