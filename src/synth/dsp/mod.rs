mod engine;
mod oscillator;
mod twist;
mod voice;

pub use crate::common::envelope::{
    AdsrEnvelope, AttackShape, DecayReleaseShape, EnvelopeMode, EnvelopeRetriggerMode,
};
pub use crate::common::filter::{Filter, FilterSubtype, FilterType};
pub use crate::common::flavor::{FlavorFilter, FlavorType};
pub use crate::common::lfo::{
    Lfo, LfoShape, LfoSyncDivision, LfoSyncMode, LfoTriggerMode, MSEG_MAX_NODES, MSEG_MAX_SEGMENTS,
    MsegCurve, MsegLoopMode,
};
pub use crate::common::mts_esp::MtsEspClient;
pub use crate::common::noise::{NoiseColorMode, NoiseGenerator, NoiseType};
pub use crate::common::settings::{EnvelopeSettings, FilterSettings, LfoSettings};
pub use crate::common::tuning::Tuning;
pub use crate::common::voice::{PlayMode, PortamentoCurve, StealMode, VoicePriority};

pub use engine::{SynthEngine, soft_clip_master};
pub use oscillator::{
    AliasWaveform, ClassicOsc, ClassicWaveform, ExciterType, Fm2FeedbackMode, Fm3FeedbackMode,
    OscType, Oscillator, SineShaperMode, WindowType,
};
pub use twist::{TwistModel, TwistOsc};
pub use voice::{
    CombinatorMode, FilterRouting, ModDepthCurve, ModRouting, ModSource, ModTarget, ModValues,
    NoiseSettings, OscFmMode, OscPhaseMode, OscRoute, OscSettings, Voice, VoiceParams,
};
