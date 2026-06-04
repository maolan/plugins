#![allow(dead_code)]

//! Synthesizer voice with full modulation matrix.
//!
//! Inspired by Surge XT's scene/voice architecture.

use rand::random;

use super::{
    AdsrEnvelope, AliasWaveform, CharacterFilter, ClassicWaveform, EnvelopeSettings, ExciterType,
    Filter, FilterSettings, FilterType, Fm2FeedbackMode, Lfo, LfoSettings, LfoShape,
    MSEG_MAX_NODES, MSEG_MAX_SEGMENTS, ModernSubWaveform, MsegCurve, MsegLoopMode, MtsEspClient,
    NoiseColorMode, NoiseGenerator, NoiseType, OscType, Oscillator, PlayMode, PortamentoCurve,
    SineShaperMode, Tuning, VoicePriority, Waveshape, Waveshaper, WaveshaperSettings, WindowType,
};
use parking_lot::Mutex;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Settings structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OscPhaseMode {
    Random = 0,
    Zero = 1,
    Current = 2,
}

impl OscPhaseMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => OscPhaseMode::Random,
            1 => OscPhaseMode::Zero,
            _ => OscPhaseMode::Current,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OscSettings {
    pub osc_type: OscType,
    pub octave: i8,
    pub semitone: i8,
    pub fine: f32,
    pub shape: f32,
    pub skew: f32,
    pub formant: f32,
    pub level: f32,
    pub enabled: bool,
    pub unison_voices: u8,
    pub unison_detune: f32,
    pub unison_spread: f32,
    pub phase_mode: OscPhaseMode,
    pub sync: f32,
    pub waveform: u8,
    pub fm_depth: f32,
    pub sub_level: f32,
    pub sub_octave: u8,
    pub pm_mode: bool,
    pub shaper_mode: u8,
    pub fm2_feedback: f32,
    pub fm2_m12offset: f32,
    pub fm2_m12phase: f32,
    pub fm2_feedback_mode: u8,
    pub fm3_m3_abs_freq: f32,
    pub fm3_feedback: f32,
    pub fm3_feedback_mode: u8,
    pub sine_lowcut: f32,
    pub sine_highcut: f32,
    pub window_lowcut: f32,
    pub window_highcut: f32,
    pub sh_noise_lowcut: f32,
    pub sh_noise_highcut: f32,
    pub width2: f32,
    pub wavetable_skew_v: f32,
    pub wavetable_saturate: f32,
    pub string_tone_lp: f32,
    pub string_tone_hp: f32,
    pub wavetable_sampler_mode: u8,
    pub string_dual_detune: f32,
    pub string_dual_decay: f32,
    pub string_oversample: bool,
    pub sub_one: bool,
    pub alias_partials: [f32; 16],
    pub route: OscRoute,
    pub mute: bool,
    pub solo: bool,
}

impl Default for OscSettings {
    fn default() -> Self {
        Self {
            osc_type: OscType::Classic,
            octave: 0,
            semitone: 0,
            fine: 0.0,
            shape: 0.5,
            skew: 0.0,
            formant: 1.0,
            level: 0.8,
            enabled: true,
            unison_voices: 1,
            unison_detune: 0.0,
            unison_spread: 1.0,
            phase_mode: OscPhaseMode::Random,
            sync: 0.0,
            waveform: 0,
            fm_depth: 1.0,
            sub_level: 0.0,
            sub_octave: 0,
            pm_mode: false,
            shaper_mode: 0,
            fm2_feedback: 0.0,
            fm2_m12offset: 0.0,
            fm2_m12phase: 0.0,
            fm2_feedback_mode: 0,
            fm3_m3_abs_freq: 0.0,
            fm3_feedback: 0.0,
            fm3_feedback_mode: 0,
            sine_lowcut: 20.0,
            sine_highcut: 20000.0,
            window_lowcut: 20.0,
            window_highcut: 20000.0,
            sh_noise_lowcut: 20.0,
            sh_noise_highcut: 20000.0,
            width2: 0.5,
            wavetable_skew_v: 0.0,
            wavetable_saturate: 0.0,
            string_tone_lp: 20000.0,
            string_tone_hp: 20.0,
            wavetable_sampler_mode: 0,
            string_dual_detune: 0.0,
            string_dual_decay: 0.5,
            string_oversample: false,
            sub_one: false,
            alias_partials: [
                1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
            route: OscRoute::Both,
            mute: false,
            solo: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NoiseSettings {
    pub noise_type: NoiseType,
    pub level: f32,
    pub filter_type: FilterType,
    pub filter_cutoff: f32,
    pub filter_resonance: f32,
    pub filter_enabled: bool,
    pub enabled: bool,
    pub color: f32,
    pub stereo: bool,
    pub color_mode: u8,
    pub route: OscRoute,
    pub mute: bool,
    pub solo: bool,
}

impl Default for NoiseSettings {
    fn default() -> Self {
        Self {
            noise_type: NoiseType::White,
            level: 0.0,
            filter_type: FilterType::Lowpass,
            filter_cutoff: 8000.0,
            filter_resonance: 0.7,
            filter_enabled: false,
            enabled: false,
            color: 0.5,
            stereo: false,
            color_mode: 0,
            route: OscRoute::Both,
            mute: false,
            solo: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterRouting {
    Series = 0,   // S1: F1 → WS → F2
    Parallel = 1, // D1: F1 || F2 → sum → WS
    Wide = 2,     // S2 doubled: F2 → WS → F1 (stereo)
    Split = 3,    // Stereo: L→F1, R→F2 → WS
    Serial2 = 4,  // F2 → WS → F1
    Serial3 = 5,  // F1→WS + F2 parallel mix
    Dual2 = 6,    // F1(WS) || F2 → sum
    Ring = 7,     // F1 × F2 → WS
}

impl FilterRouting {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => FilterRouting::Series,
            1 => FilterRouting::Parallel,
            2 => FilterRouting::Wide,
            3 => FilterRouting::Split,
            4 => FilterRouting::Serial2,
            5 => FilterRouting::Serial3,
            6 => FilterRouting::Dual2,
            _ => FilterRouting::Ring,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FilterRouting::Series => "Series",
            FilterRouting::Parallel => "Parallel",
            FilterRouting::Wide => "Wide",
            FilterRouting::Split => "Split",
            FilterRouting::Serial2 => "Serial 2",
            FilterRouting::Serial3 => "Serial 3",
            FilterRouting::Dual2 => "Dual 2",
            FilterRouting::Ring => "Ring",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OscFmMode {
    Off = 0,
    Osc1To2 = 1,
    Osc2To3 = 2,
    Osc1To2To3 = 3,
    Osc1To3 = 4,
    Ring1x2 = 5,
    Ring2x3 = 6,
    Osc2To1 = 7,
    Osc3To1 = 8,
    Osc3To2 = 9,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OscRoute {
    Filter1 = 0,
    Both = 1,
    Filter2 = 2,
}

impl OscRoute {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => OscRoute::Filter1,
            2 => OscRoute::Filter2,
            _ => OscRoute::Both,
        }
    }
}

impl OscFmMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => OscFmMode::Off,
            1 => OscFmMode::Osc1To2,
            2 => OscFmMode::Osc2To3,
            3 => OscFmMode::Osc1To2To3,
            4 => OscFmMode::Osc1To3,
            5 => OscFmMode::Ring1x2,
            6 => OscFmMode::Ring2x3,
            7 => OscFmMode::Osc2To1,
            8 => OscFmMode::Osc3To1,
            _ => OscFmMode::Osc3To2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombinatorMode {
    Ring = 0,
    Cxor43_0 = 1,
    Cxor43_1 = 2,
    Cxor43_2 = 3,
    Cxor43_3 = 4,
    Cxor43_4 = 5,
    Cxor93_0 = 6,
    Cxor93_1 = 7,
    Cxor93_2 = 8,
    Cxor93_3 = 9,
    Cxor93_4 = 10,
}

impl CombinatorMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => CombinatorMode::Cxor43_0,
            2 => CombinatorMode::Cxor43_1,
            3 => CombinatorMode::Cxor43_2,
            4 => CombinatorMode::Cxor43_3,
            5 => CombinatorMode::Cxor43_4,
            6 => CombinatorMode::Cxor93_0,
            7 => CombinatorMode::Cxor93_1,
            8 => CombinatorMode::Cxor93_2,
            9 => CombinatorMode::Cxor93_3,
            10 => CombinatorMode::Cxor93_4,
            _ => CombinatorMode::Ring,
        }
    }
}

#[inline]
fn cxor43_0(a: f32, b: f32) -> f32 {
    let mx = a.max(b);
    let mn = a.min(b);
    mx.min(-mn)
}

#[inline]
fn cxor43_1(a: f32, b: f32) -> f32 {
    let v1 = a.max(b);
    let cx = cxor43_0(a, b);
    v1.min(-cx.min(v1))
}

#[inline]
fn cxor43_2(a: f32, b: f32) -> f32 {
    let v1 = a.max(b);
    let cx = cxor43_0(a, b);
    a.min(-cx.min(v1))
}

#[inline]
fn cxor43_3(a: f32, b: f32) -> f32 {
    let cx = cxor43_0(a, b);
    (-cx.min(-b)).min(a.max(b))
}

#[inline]
fn cxor43_4(a: f32, b: f32) -> f32 {
    let cx = cxor43_0(a, b);
    (-cx.min(b)).min(a.max(cx))
}

#[inline]
fn cxor93_0(a: f32, b: f32) -> f32 {
    let p = a + b;
    let m = a - b;
    p.max(m).min(-p.min(m))
}

#[inline]
fn cxor93_1(a: f32, b: f32) -> f32 {
    a - b.max(a.min(0.0)).min(a.max(0.0))
}

#[inline]
fn cxor93_2(a: f32, b: f32) -> f32 {
    let p = b + a;
    let mf = b - a;
    b.min((0.0f32).max(p.min(mf)))
}

#[inline]
fn cxor93_3(a: f32, b: f32) -> f32 {
    let p = b + a;
    let mf = b - a;
    b.max(p).min((0.0f32).max(p.min(mf)))
}

#[inline]
fn cxor93_4(a: f32, b: f32) -> f32 {
    let p = b + a;
    let mf = b - a;
    (-a).max(b).min(mf).max(p.min(-p))
}

#[inline]
fn apply_combinator(a: f32, b: f32, mode: CombinatorMode) -> f32 {
    match mode {
        CombinatorMode::Ring => a * b,
        CombinatorMode::Cxor43_0 => cxor43_0(a, b),
        CombinatorMode::Cxor43_1 => cxor43_1(a, b),
        CombinatorMode::Cxor43_2 => cxor43_2(a, b),
        CombinatorMode::Cxor43_3 => cxor43_3(a, b),
        CombinatorMode::Cxor43_4 => cxor43_4(a, b),
        CombinatorMode::Cxor93_0 => cxor93_0(a, b),
        CombinatorMode::Cxor93_1 => cxor93_1(a, b),
        CombinatorMode::Cxor93_2 => cxor93_2(a, b),
        CombinatorMode::Cxor93_3 => cxor93_3(a, b),
        CombinatorMode::Cxor93_4 => cxor93_4(a, b),
    }
}

// ---------------------------------------------------------------------------
// Modulation Matrix
// ---------------------------------------------------------------------------

pub const MOD_MATRIX_SIZE: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModSource {
    Velocity = 0,
    Keytrack = 1,
    ModWheel = 2,
    Aftertouch = 3,
    PitchBend = 4,
    Lfo1 = 5,
    Lfo2 = 6,
    Lfo3 = 7,
    Lfo4 = 8,
    Lfo5 = 9,
    Lfo6 = 10,
    AmpEg = 11,
    FilterEg = 12,
    PitchEg = 13,
    RandomBipolar = 14,
    RandomUnipolar = 15,
    AlternateBipolar = 16,
    AlternateUnipolar = 17,
    Macro1 = 18,
    Macro2 = 19,
    Macro3 = 20,
    Macro4 = 21,
    Macro5 = 22,
    Macro6 = 23,
    Macro7 = 24,
    Macro8 = 25,
    Breath = 26,
    Expression = 27,
    Sustain = 28,
    PolyAftertouch = 29,
    NoteGate = 30,
    MpeTimbre = 31,
    ReleaseVelocity = 32,
    Constant = 33,
    NoteExpressionVolume = 34,
    NoteExpressionPan = 35,
    SceneLfo1 = 36,
    SceneLfo2 = 37,
    SceneLfo3 = 38,
    SceneLfo4 = 39,
    SceneLfo5 = 40,
    SceneLfo6 = 41,
    LowestKey = 42,
    HighestKey = 43,
    LatestKey = 44,
}

impl ModSource {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(ModSource::Velocity),
            1 => Some(ModSource::Keytrack),
            2 => Some(ModSource::ModWheel),
            3 => Some(ModSource::Aftertouch),
            4 => Some(ModSource::PitchBend),
            5 => Some(ModSource::Lfo1),
            6 => Some(ModSource::Lfo2),
            7 => Some(ModSource::Lfo3),
            8 => Some(ModSource::Lfo4),
            9 => Some(ModSource::Lfo5),
            10 => Some(ModSource::Lfo6),
            11 => Some(ModSource::AmpEg),
            12 => Some(ModSource::FilterEg),
            13 => Some(ModSource::PitchEg),
            14 => Some(ModSource::RandomBipolar),
            15 => Some(ModSource::RandomUnipolar),
            16 => Some(ModSource::AlternateBipolar),
            17 => Some(ModSource::AlternateUnipolar),
            18 => Some(ModSource::Macro1),
            19 => Some(ModSource::Macro2),
            20 => Some(ModSource::Macro3),
            21 => Some(ModSource::Macro4),
            22 => Some(ModSource::Macro5),
            23 => Some(ModSource::Macro6),
            24 => Some(ModSource::Macro7),
            25 => Some(ModSource::Macro8),
            26 => Some(ModSource::Breath),
            27 => Some(ModSource::Expression),
            28 => Some(ModSource::Sustain),
            29 => Some(ModSource::PolyAftertouch),
            30 => Some(ModSource::NoteGate),
            31 => Some(ModSource::MpeTimbre),
            32 => Some(ModSource::ReleaseVelocity),
            33 => Some(ModSource::Constant),
            34 => Some(ModSource::NoteExpressionVolume),
            35 => Some(ModSource::NoteExpressionPan),
            36 => Some(ModSource::SceneLfo1),
            37 => Some(ModSource::SceneLfo2),
            38 => Some(ModSource::SceneLfo3),
            39 => Some(ModSource::SceneLfo4),
            40 => Some(ModSource::SceneLfo5),
            41 => Some(ModSource::SceneLfo6),
            42 => Some(ModSource::LowestKey),
            43 => Some(ModSource::HighestKey),
            44 => Some(ModSource::LatestKey),
            _ => None,
        }
    }

    pub const COUNT: u8 = 42;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModTarget {
    Osc1Pitch = 0,
    Osc2Pitch = 1,
    Osc3Pitch = 2,
    Osc1Level = 3,
    Osc2Level = 4,
    Osc3Level = 5,
    Osc1Shape = 6,
    Osc2Shape = 7,
    Osc3Shape = 8,
    Osc1Skew = 9,
    Osc2Skew = 10,
    Osc3Skew = 11,
    Osc1Formant = 12,
    Osc2Formant = 13,
    Osc3Formant = 14,
    Filter1Cutoff = 15,
    Filter1Resonance = 16,
    Filter1EgAmount = 17,
    Filter1Drive = 18,
    Filter2Cutoff = 19,
    Filter2Resonance = 20,
    Filter2EgAmount = 21,
    Filter2Drive = 22,
    AmpAttack = 23,
    AmpDecay = 24,
    AmpSustain = 25,
    AmpRelease = 26,
    FilterAttack = 27,
    FilterDecay = 28,
    FilterSustain = 29,
    FilterRelease = 30,
    PitchAttack = 31,
    PitchDecay = 32,
    PitchSustain = 33,
    PitchRelease = 34,
    Lfo1Rate = 35,
    Lfo1Amount = 36,
    Lfo1Deform = 37,
    Lfo2Rate = 38,
    Lfo2Amount = 39,
    Lfo2Deform = 40,
    Lfo3Rate = 41,
    Lfo3Amount = 42,
    Lfo3Deform = 43,
    Lfo4Rate = 44,
    Lfo4Amount = 45,
    Lfo4Deform = 46,
    Lfo5Rate = 47,
    Lfo5Amount = 48,
    Lfo5Deform = 49,
    Lfo6Rate = 50,
    Lfo6Amount = 51,
    Lfo6Deform = 52,
    OutputVolume = 53,
    OutputPan = 54,
    OutputWidth = 55,
    NoiseLevel = 56,
    WaveshaperDrive = 57,
    Portamento = 58,
    CharacterCutoff = 59,
    FilterBalance = 60,
    OscFmDepth = 61,
    Osc1Sync = 62,
    Osc2Sync = 63,
    Osc3Sync = 64,
    Lfo1Phase = 65,
    Lfo2Phase = 66,
    Lfo3Phase = 67,
    Lfo4Phase = 68,
    Lfo5Phase = 69,
    Lfo6Phase = 70,
    ModRoute1Depth = 71,
    ModRoute2Depth = 72,
    ModRoute3Depth = 73,
    ModRoute4Depth = 74,
    ModRoute5Depth = 75,
    ModRoute6Depth = 76,
    ModRoute7Depth = 77,
    ModRoute8Depth = 78,
    ModRoute9Depth = 79,
    ModRoute10Depth = 80,
    ModRoute11Depth = 81,
    ModRoute12Depth = 82,
}

impl ModTarget {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(ModTarget::Osc1Pitch),
            1 => Some(ModTarget::Osc2Pitch),
            2 => Some(ModTarget::Osc3Pitch),
            3 => Some(ModTarget::Osc1Level),
            4 => Some(ModTarget::Osc2Level),
            5 => Some(ModTarget::Osc3Level),
            6 => Some(ModTarget::Osc1Shape),
            7 => Some(ModTarget::Osc2Shape),
            8 => Some(ModTarget::Osc3Shape),
            9 => Some(ModTarget::Osc1Skew),
            10 => Some(ModTarget::Osc2Skew),
            11 => Some(ModTarget::Osc3Skew),
            12 => Some(ModTarget::Osc1Formant),
            13 => Some(ModTarget::Osc2Formant),
            14 => Some(ModTarget::Osc3Formant),
            15 => Some(ModTarget::Filter1Cutoff),
            16 => Some(ModTarget::Filter1Resonance),
            17 => Some(ModTarget::Filter1EgAmount),
            18 => Some(ModTarget::Filter1Drive),
            19 => Some(ModTarget::Filter2Cutoff),
            20 => Some(ModTarget::Filter2Resonance),
            21 => Some(ModTarget::Filter2EgAmount),
            22 => Some(ModTarget::Filter2Drive),
            23 => Some(ModTarget::AmpAttack),
            24 => Some(ModTarget::AmpDecay),
            25 => Some(ModTarget::AmpSustain),
            26 => Some(ModTarget::AmpRelease),
            27 => Some(ModTarget::FilterAttack),
            28 => Some(ModTarget::FilterDecay),
            29 => Some(ModTarget::FilterSustain),
            30 => Some(ModTarget::FilterRelease),
            31 => Some(ModTarget::PitchAttack),
            32 => Some(ModTarget::PitchDecay),
            33 => Some(ModTarget::PitchSustain),
            34 => Some(ModTarget::PitchRelease),
            35 => Some(ModTarget::Lfo1Rate),
            36 => Some(ModTarget::Lfo1Amount),
            37 => Some(ModTarget::Lfo1Deform),
            38 => Some(ModTarget::Lfo2Rate),
            39 => Some(ModTarget::Lfo2Amount),
            40 => Some(ModTarget::Lfo2Deform),
            41 => Some(ModTarget::Lfo3Rate),
            42 => Some(ModTarget::Lfo3Amount),
            43 => Some(ModTarget::Lfo3Deform),
            44 => Some(ModTarget::Lfo4Rate),
            45 => Some(ModTarget::Lfo4Amount),
            46 => Some(ModTarget::Lfo4Deform),
            47 => Some(ModTarget::Lfo5Rate),
            48 => Some(ModTarget::Lfo5Amount),
            49 => Some(ModTarget::Lfo5Deform),
            50 => Some(ModTarget::Lfo6Rate),
            51 => Some(ModTarget::Lfo6Amount),
            52 => Some(ModTarget::Lfo6Deform),
            53 => Some(ModTarget::OutputVolume),
            54 => Some(ModTarget::OutputPan),
            55 => Some(ModTarget::OutputWidth),
            56 => Some(ModTarget::NoiseLevel),
            57 => Some(ModTarget::WaveshaperDrive),
            58 => Some(ModTarget::Portamento),
            59 => Some(ModTarget::CharacterCutoff),
            60 => Some(ModTarget::FilterBalance),
            61 => Some(ModTarget::OscFmDepth),
            62 => Some(ModTarget::Osc1Sync),
            63 => Some(ModTarget::Osc2Sync),
            64 => Some(ModTarget::Osc3Sync),
            65 => Some(ModTarget::Lfo1Phase),
            66 => Some(ModTarget::Lfo2Phase),
            67 => Some(ModTarget::Lfo3Phase),
            68 => Some(ModTarget::Lfo4Phase),
            69 => Some(ModTarget::Lfo5Phase),
            70 => Some(ModTarget::Lfo6Phase),
            71 => Some(ModTarget::ModRoute1Depth),
            72 => Some(ModTarget::ModRoute2Depth),
            73 => Some(ModTarget::ModRoute3Depth),
            74 => Some(ModTarget::ModRoute4Depth),
            75 => Some(ModTarget::ModRoute5Depth),
            76 => Some(ModTarget::ModRoute6Depth),
            77 => Some(ModTarget::ModRoute7Depth),
            78 => Some(ModTarget::ModRoute8Depth),
            79 => Some(ModTarget::ModRoute9Depth),
            80 => Some(ModTarget::ModRoute10Depth),
            81 => Some(ModTarget::ModRoute11Depth),
            82 => Some(ModTarget::ModRoute12Depth),
            _ => None,
        }
    }

    pub const COUNT: u8 = 83;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModDepthCurve {
    Linear = 0,
    Exp = 1,
    Log = 2,
    Sqrt = 3,
    Squared = 4,
}

impl ModDepthCurve {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => ModDepthCurve::Linear,
            1 => ModDepthCurve::Exp,
            2 => ModDepthCurve::Log,
            3 => ModDepthCurve::Sqrt,
            _ => ModDepthCurve::Squared,
        }
    }

    pub fn apply(&self, depth: f32) -> f32 {
        match self {
            ModDepthCurve::Linear => depth,
            ModDepthCurve::Exp => {
                let sign = depth.signum();
                sign * (1.0 - (-depth.abs() * 3.0).exp())
            }
            ModDepthCurve::Log => {
                let sign = depth.signum();
                sign * ((1.0 + depth.abs() * 2.0).ln() / 3.0f32.ln())
            }
            ModDepthCurve::Sqrt => depth.signum() * depth.abs().sqrt(),
            ModDepthCurve::Squared => depth * depth * depth.signum(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ModRouting {
    pub source: ModSource,
    pub target: ModTarget,
    pub depth: f32,
    pub depth_curve: ModDepthCurve,
    pub active: bool,
}

impl Default for ModRouting {
    fn default() -> Self {
        Self {
            source: ModSource::Velocity,
            target: ModTarget::Filter1Cutoff,
            depth: 0.0,
            depth_curve: ModDepthCurve::Linear,
            active: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Pre-computed modulation values per sample
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ModValues {
    osc_pitch: [f32; 3],
    osc_level: [f32; 3],
    osc_shape: [f32; 3],
    osc_skew: [f32; 3],
    osc_formant: [f32; 3],
    f1_cutoff: f32,
    f1_resonance: f32,
    f1_eg_amount: f32,
    f1_drive: f32,
    f2_cutoff: f32,
    f2_resonance: f32,
    f2_eg_amount: f32,
    f2_drive: f32,
    amp_attack: f32,
    amp_decay: f32,
    amp_sustain: f32,
    amp_release: f32,
    filter_attack: f32,
    filter_decay: f32,
    filter_sustain: f32,
    filter_release: f32,
    pitch_attack: f32,
    pitch_decay: f32,
    pitch_sustain: f32,
    pitch_release: f32,
    lfo1_rate: f32,
    lfo1_amount: f32,
    lfo1_deform: f32,
    lfo2_rate: f32,
    lfo2_amount: f32,
    lfo2_deform: f32,
    lfo3_rate: f32,
    lfo3_amount: f32,
    lfo3_deform: f32,
    lfo4_rate: f32,
    lfo4_amount: f32,
    lfo4_deform: f32,
    lfo5_rate: f32,
    lfo5_amount: f32,
    lfo5_deform: f32,
    lfo6_rate: f32,
    lfo6_amount: f32,
    lfo6_deform: f32,
    lfo1_phase: f32,
    lfo2_phase: f32,
    lfo3_phase: f32,
    lfo4_phase: f32,
    lfo5_phase: f32,
    lfo6_phase: f32,
    output_volume: f32,
    output_pan: f32,
    output_width: f32,
    noise_level: f32,
    waveshaper_drive: f32,
    portamento: f32,
    character_cutoff: f32,
    filter_balance: f32,
    osc_fm_depth: f32,
    osc_sync: [f32; 3],
    mod_depth: [f32; 12],
}

// ---------------------------------------------------------------------------
// Voice Parameters
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct VoiceParams {
    pub oscs: [OscSettings; 3],
    pub filter1: FilterSettings,
    pub filter2: FilterSettings,
    pub filter_routing: FilterRouting,
    pub filter_balance: f32,
    pub amp_eg: EnvelopeSettings,
    pub filter_eg: EnvelopeSettings,
    pub pitch_eg: EnvelopeSettings,
    pub lfo1: LfoSettings,
    pub lfo2: LfoSettings,
    pub lfo3: LfoSettings,
    pub lfo4: LfoSettings,
    pub lfo5: LfoSettings,
    pub lfo6: LfoSettings,
    pub scene_lfo1: LfoSettings,
    pub scene_lfo2: LfoSettings,
    pub scene_lfo3: LfoSettings,
    pub scene_lfo4: LfoSettings,
    pub scene_lfo5: LfoSettings,
    pub scene_lfo6: LfoSettings,
    pub noise: NoiseSettings,
    pub waveshaper: WaveshaperSettings,
    pub character: super::CharacterType,
    pub character_cutoff: f32,
    pub character_resonance: f32,
    pub osc_fm_mode: OscFmMode,
    pub osc_fm_depth: f32,
    pub ring12_combinator: CombinatorMode,
    pub ring23_combinator: CombinatorMode,
    pub portamento: f32,
    pub portamento_curve: PortamentoCurve,
    pub volume: f32,
    pub pan: f32,
    pub width: f32,
    pub pitch_bend_range: f32,
    pub pitch_bend_up: f32,
    pub pitch_bend_down: f32,
    pub glissando: bool,
    pub portamento_sync: bool,
    pub portamento_retrigger: bool,
    pub mpe_enabled: bool,
    pub pitch_bend_smooth: f32,
    pub modulations: [ModRouting; MOD_MATRIX_SIZE],
    pub mod_wheel: f32,
    pub aftertouch: f32,
    pub poly_aftertouch: f32,
    pub mpe_timbre: f32,
    pub note_expression_volume: f32,
    pub note_expression_pan: f32,
    pub release_velocity: f32,
    pub macros: [f32; 8],
    pub breath: f32,
    pub expression: f32,
    pub sustain: f32,
    pub tuning_scale: u8,
    pub tuning_root: u8,
    pub tuning_override: Option<Tuning>,
    pub play_mode: PlayMode,
    pub voice_priority: VoicePriority,
    pub drift_amount: f32,
    pub step_seq_values: [f32; 16],
    pub step_seq_loop_start: usize,
    pub step_seq_loop_end: usize,
    pub step_seq_shuffle: f32,
    pub step_seq_trig_amp: u16,
    pub step_seq_trig_filter: u16,
    pub step_seq_trig_pitch: u16,
    pub mseg_retrig_amp: u16,
    pub mseg_retrig_filter: u16,
    pub mseg_retrig_pitch: u16,
    pub mseg_nodes: [f32; MSEG_MAX_NODES],
    pub mseg_curves: [MsegCurve; MSEG_MAX_SEGMENTS],
    pub mseg_loop_start: usize,
    pub mseg_loop_end: usize,
    pub mseg_loop_mode: MsegLoopMode,
    pub string_stereo_spread: f32,
    pub wavetable_keytrack: f32,
    pub pre_filter_gain: f32,
    pub vca_level: f32,
    pub vca_velsense: f32,
    pub f2_cutoff_offset: bool,
    pub f2_res_link: bool,
    pub lowcut_hz: f32,
    pub sh_noise_correlation: f32,
    pub sh_noise_width: f32,
    pub sh_noise_sync: f32,
    pub filter_feedback: f32,
    pub poly_repeated_key_mode: bool,
    pub twist_aux_mix: f32,
    pub twist_lpg_response: f32,
    pub twist_lpg_decay: f32,
    pub mono_pedal_mode: bool,
    pub lowcut_slope: u8,
}

impl Default for VoiceParams {
    fn default() -> Self {
        Self {
            oscs: [
                OscSettings::default(),
                OscSettings::default(),
                OscSettings::default(),
            ],
            filter1: FilterSettings::default(),
            filter2: FilterSettings::default(),
            filter_routing: FilterRouting::Series,
            filter_balance: 0.0,
            amp_eg: EnvelopeSettings::default(),
            filter_eg: EnvelopeSettings::default(),
            pitch_eg: EnvelopeSettings::default(),
            lfo1: LfoSettings::default(),
            lfo2: LfoSettings::default(),
            lfo3: LfoSettings::default(),
            lfo4: LfoSettings::default(),
            lfo5: LfoSettings::default(),
            lfo6: LfoSettings::default(),
            scene_lfo1: LfoSettings::default(),
            scene_lfo2: LfoSettings::default(),
            scene_lfo3: LfoSettings::default(),
            scene_lfo4: LfoSettings::default(),
            scene_lfo5: LfoSettings::default(),
            scene_lfo6: LfoSettings::default(),
            noise: NoiseSettings::default(),
            waveshaper: WaveshaperSettings::default(),
            character: super::CharacterType::Off,
            character_cutoff: 8000.0,
            character_resonance: 0.5,
            osc_fm_mode: OscFmMode::Off,
            osc_fm_depth: 0.5,
            ring12_combinator: CombinatorMode::Ring,
            ring23_combinator: CombinatorMode::Ring,
            portamento: 0.0,
            portamento_curve: PortamentoCurve::Linear,
            volume: 0.8,
            pan: 0.0,
            width: 0.0,
            pitch_bend_range: 2.0,
            pitch_bend_up: 0.0,
            pitch_bend_down: 0.0,
            glissando: false,
            portamento_sync: false,
            portamento_retrigger: false,
            mpe_enabled: false,
            pitch_bend_smooth: 0.0,
            modulations: [ModRouting::default(); MOD_MATRIX_SIZE],
            mod_wheel: 0.0,
            aftertouch: 0.0,
            poly_aftertouch: 0.0,
            mpe_timbre: 0.0,
            note_expression_volume: 1.0,
            note_expression_pan: 0.0,
            release_velocity: 0.0,
            macros: [0.0; 8],
            breath: 0.0,
            expression: 0.0,
            sustain: 0.0,
            play_mode: PlayMode::Poly,
            voice_priority: VoicePriority::Last,
            drift_amount: 0.0,
            step_seq_values: [0.0; 16],
            step_seq_loop_start: 0,
            step_seq_loop_end: 15,
            step_seq_shuffle: 0.0,
            step_seq_trig_amp: 0,
            step_seq_trig_filter: 0,
            step_seq_trig_pitch: 0,
            mseg_retrig_amp: 0,
            mseg_retrig_filter: 0,
            mseg_retrig_pitch: 0,
            mseg_nodes: [0.0; MSEG_MAX_NODES],
            mseg_curves: [MsegCurve::Linear; MSEG_MAX_SEGMENTS],
            mseg_loop_start: 0,
            mseg_loop_end: MSEG_MAX_NODES - 1,
            mseg_loop_mode: MsegLoopMode::Loop,
            string_stereo_spread: 0.0,
            wavetable_keytrack: 0.0,
            pre_filter_gain: 1.0,
            vca_level: 1.0,
            vca_velsense: 1.0,
            f2_cutoff_offset: false,
            f2_res_link: false,
            lowcut_hz: 20.0,
            sh_noise_correlation: 0.0,
            sh_noise_width: 0.5,
            sh_noise_sync: 0.0,
            filter_feedback: 0.0,
            poly_repeated_key_mode: false,
            twist_aux_mix: 0.0,
            twist_lpg_response: 0.0,
            twist_lpg_decay: 0.0,
            mono_pedal_mode: false,
            lowcut_slope: 1,
            tuning_scale: 0,
            tuning_root: 60,
            tuning_override: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Voice
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Voice {
    sample_rate: f32,
    oscillators: [Oscillator; 3],
    noise: NoiseGenerator,
    character: CharacterFilter,
    character2: CharacterFilter,
    waveshaper: Waveshaper,
    filter1_l: Filter,
    filter1_r: Filter,
    filter2_l: Filter,
    filter2_r: Filter,
    lowcut_l: Filter,
    lowcut_r: Filter,
    lowcut_states_l: [f32; 4],
    lowcut_states_r: [f32; 4],
    lowcut_states_l2: [f32; 4],
    lowcut_states_r2: [f32; 4],
    amp_eg: AdsrEnvelope,
    filter_eg: AdsrEnvelope,
    pitch_eg: AdsrEnvelope,
    pub lfo1: Lfo,
    pub lfo2: Lfo,
    pub lfo3: Lfo,
    pub lfo4: Lfo,
    pub lfo5: Lfo,
    pub lfo6: Lfo,

    // State
    pub note: u8,
    pub velocity: f32,
    pub pitch_bend: f32,
    pub gate: bool,
    pub active: bool,
    pub sample_counter: usize,
    pub tempo_bpm: f32,

    // Portamento
    current_freq: f32,
    pub target_freq: f32,
    tuning: Tuning,
    last_scale_degree: i32,

    // Parameters
    pub params: VoiceParams,

    // Cached modulation outputs (updated per sample)
    lfo1_output: f32,
    lfo2_output: f32,
    lfo3_output: f32,
    lfo4_output: f32,
    lfo5_output: f32,
    lfo6_output: f32,
    scene_lfo1_output: f32,
    scene_lfo2_output: f32,
    scene_lfo3_output: f32,
    scene_lfo4_output: f32,
    scene_lfo5_output: f32,
    scene_lfo6_output: f32,
    lowest_key: f32,
    highest_key: f32,
    latest_key: f32,
    amp_eg_output: f32,
    filter_eg_output: f32,
    pitch_eg_output: f32,

    // Random / alternate state
    random_value: f32,
    alternate_sign: f32,
    note_counter: usize,

    // Oscillator drift state
    drift_phase: [f32; 3],
    drift_target: [f32; 3],
    drift_smooth: [f32; 3],
    pitch_bend_smooth_state: f32,
    filter_feedback_prev_l: f32,
    filter_feedback_prev_r: f32,
    mts_esp: Option<Arc<Mutex<MtsEspClient>>>,
}

impl Voice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            oscillators: [
                Oscillator::new(OscType::Classic, sample_rate),
                Oscillator::new(OscType::Sine, sample_rate),
                Oscillator::new(OscType::Fm2, sample_rate),
            ],
            noise: NoiseGenerator::new(sample_rate),
            character: CharacterFilter::new(sample_rate),
            character2: CharacterFilter::new(sample_rate),
            waveshaper: Waveshaper::new(),
            filter1_l: Filter::new(FilterType::Lowpass, sample_rate),
            filter1_r: Filter::new(FilterType::Lowpass, sample_rate),
            filter2_l: Filter::new(FilterType::Lowpass, sample_rate),
            filter2_r: Filter::new(FilterType::Lowpass, sample_rate),
            lowcut_l: Filter::new(FilterType::Highpass12dB, sample_rate),
            lowcut_r: Filter::new(FilterType::Highpass12dB, sample_rate),
            lowcut_states_l: [0.0; 4],
            lowcut_states_r: [0.0; 4],
            lowcut_states_l2: [0.0; 4],
            lowcut_states_r2: [0.0; 4],
            amp_eg: AdsrEnvelope::new(sample_rate),
            filter_eg: AdsrEnvelope::new(sample_rate),
            pitch_eg: AdsrEnvelope::new(sample_rate),
            lfo1: Lfo::new(sample_rate),
            lfo2: Lfo::new(sample_rate),
            lfo3: Lfo::new(sample_rate),
            lfo4: Lfo::new(sample_rate),
            lfo5: Lfo::new(sample_rate),
            lfo6: Lfo::new(sample_rate),
            note: 0,
            velocity: 0.0,
            pitch_bend: 0.0,
            gate: false,
            active: false,
            sample_counter: 0,
            tempo_bpm: 120.0,
            current_freq: 440.0,
            target_freq: 440.0,
            tuning: Tuning::default(),
            last_scale_degree: 0,
            params: VoiceParams::default(),
            lfo1_output: 0.0,
            lfo2_output: 0.0,
            lfo3_output: 0.0,
            lfo4_output: 0.0,
            lfo5_output: 0.0,
            lfo6_output: 0.0,
            scene_lfo1_output: 0.0,
            scene_lfo2_output: 0.0,
            scene_lfo3_output: 0.0,
            scene_lfo4_output: 0.0,
            scene_lfo5_output: 0.0,
            scene_lfo6_output: 0.0,
            lowest_key: 0.0,
            highest_key: 0.0,
            latest_key: 0.0,
            amp_eg_output: 0.0,
            filter_eg_output: 0.0,
            pitch_eg_output: 0.0,
            random_value: random::<f32>() * 2.0 - 1.0,
            alternate_sign: 1.0,
            note_counter: 0,
            drift_phase: [0.0; 3],
            drift_target: [0.0; 3],
            drift_smooth: [0.0; 3],
            pitch_bend_smooth_state: 0.0,
            filter_feedback_prev_l: 0.0,
            filter_feedback_prev_r: 0.0,
            mts_esp: None,
        }
    }

    pub fn set_params(&mut self, params: &VoiceParams) {
        let old_scale = self.params.tuning_scale;
        let old_root = self.params.tuning_root;
        self.params = params.clone();

        // Update tuning if changed
        if let Some(ref tuning) = params.tuning_override {
            self.tuning = tuning.clone();
        } else if params.tuning_scale != old_scale || params.tuning_root != old_root {
            self.tuning = crate::common::tuning::built_in_tuning(params.tuning_scale);
            self.tuning.root_midi_note = params.tuning_root as i32;
        }

        // Update envelopes
        self.amp_eg.set_params(
            params.amp_eg.attack,
            params.amp_eg.decay,
            params.amp_eg.sustain,
            params.amp_eg.release,
        );
        self.amp_eg.set_mode(params.amp_eg.mode);
        self.amp_eg.set_shapes(
            params.amp_eg.attack_shape,
            params.amp_eg.decay_shape,
            params.amp_eg.release_shape,
        );
        self.amp_eg.set_retrigger_mode(params.amp_eg.retrigger_mode);
        self.amp_eg.set_tempo_sync(params.amp_eg.tempo_sync);
        self.amp_eg.set_uber_release(params.amp_eg.uber_release);
        self.amp_eg.set_gated_release(params.amp_eg.gated_release);
        self.amp_eg
            .set_correct_analog_mode(params.amp_eg.correct_analog_mode);

        self.filter_eg.set_params(
            params.filter_eg.attack,
            params.filter_eg.decay,
            params.filter_eg.sustain,
            params.filter_eg.release,
        );
        self.filter_eg.set_mode(params.filter_eg.mode);
        self.filter_eg.set_shapes(
            params.filter_eg.attack_shape,
            params.filter_eg.decay_shape,
            params.filter_eg.release_shape,
        );
        self.filter_eg
            .set_retrigger_mode(params.filter_eg.retrigger_mode);
        self.filter_eg.set_tempo_sync(params.filter_eg.tempo_sync);
        self.filter_eg
            .set_uber_release(params.filter_eg.uber_release);
        self.filter_eg
            .set_gated_release(params.filter_eg.gated_release);
        self.filter_eg
            .set_correct_analog_mode(params.filter_eg.correct_analog_mode);

        self.pitch_eg.set_params(
            params.pitch_eg.attack,
            params.pitch_eg.decay,
            params.pitch_eg.sustain,
            params.pitch_eg.release,
        );
        self.pitch_eg.set_mode(params.pitch_eg.mode);
        self.pitch_eg.set_shapes(
            params.pitch_eg.attack_shape,
            params.pitch_eg.decay_shape,
            params.pitch_eg.release_shape,
        );
        self.pitch_eg
            .set_retrigger_mode(params.pitch_eg.retrigger_mode);
        self.pitch_eg.set_tempo_sync(params.pitch_eg.tempo_sync);
        self.pitch_eg.set_uber_release(params.pitch_eg.uber_release);
        self.pitch_eg
            .set_gated_release(params.pitch_eg.gated_release);
        self.pitch_eg
            .set_correct_analog_mode(params.pitch_eg.correct_analog_mode);

        // Update LFOs
        self.lfo1.set_rate_hz(params.lfo1.rate_hz);
        self.lfo1.set_shape(params.lfo1.shape);
        self.lfo1.set_amount(params.lfo1.amount);
        self.lfo1.set_deform(params.lfo1.deform);
        self.lfo1.set_deform_type(params.lfo1.deform_type);
        self.lfo1.set_sync_mode(params.lfo1.sync_mode);
        self.lfo1.set_sync_division(params.lfo1.sync_division);
        self.lfo1.set_trigger_mode(params.lfo1.trigger_mode);
        self.lfo1.set_env_params(
            params.lfo1.env_delay,
            params.lfo1.env_attack,
            params.lfo1.env_hold,
            params.lfo1.env_decay,
            params.lfo1.env_sustain,
            params.lfo1.env_release,
        );
        self.lfo1.set_start_phase(params.lfo1.start_phase);
        self.lfo1.set_unipolar(params.lfo1.unipolar);
        self.lfo1.set_env_tempo_sync(params.lfo1.env_tempo_sync);
        self.lfo2.set_rate_hz(params.lfo2.rate_hz);
        self.lfo2.set_shape(params.lfo2.shape);
        self.lfo2.set_amount(params.lfo2.amount);
        self.lfo2.set_deform(params.lfo2.deform);
        self.lfo2.set_deform_type(params.lfo2.deform_type);
        self.lfo2.set_sync_mode(params.lfo2.sync_mode);
        self.lfo2.set_sync_division(params.lfo2.sync_division);
        self.lfo2.set_trigger_mode(params.lfo2.trigger_mode);
        self.lfo2.set_env_params(
            params.lfo2.env_delay,
            params.lfo2.env_attack,
            params.lfo2.env_hold,
            params.lfo2.env_decay,
            params.lfo2.env_sustain,
            params.lfo2.env_release,
        );
        self.lfo2.set_start_phase(params.lfo2.start_phase);
        self.lfo2.set_unipolar(params.lfo2.unipolar);
        self.lfo2.set_env_tempo_sync(params.lfo2.env_tempo_sync);
        self.lfo3.set_rate_hz(params.lfo3.rate_hz);
        self.lfo3.set_shape(params.lfo3.shape);
        self.lfo3.set_amount(params.lfo3.amount);
        self.lfo3.set_deform(params.lfo3.deform);
        self.lfo3.set_deform_type(params.lfo3.deform_type);
        self.lfo3.set_sync_mode(params.lfo3.sync_mode);
        self.lfo3.set_sync_division(params.lfo3.sync_division);
        self.lfo3.set_trigger_mode(params.lfo3.trigger_mode);
        self.lfo3.set_env_params(
            params.lfo3.env_delay,
            params.lfo3.env_attack,
            params.lfo3.env_hold,
            params.lfo3.env_decay,
            params.lfo3.env_sustain,
            params.lfo3.env_release,
        );
        self.lfo3.set_start_phase(params.lfo3.start_phase);
        self.lfo3.set_unipolar(params.lfo3.unipolar);
        self.lfo3.set_env_tempo_sync(params.lfo3.env_tempo_sync);
        self.lfo4.set_rate_hz(params.lfo4.rate_hz);
        self.lfo4.set_shape(params.lfo4.shape);
        self.lfo4.set_amount(params.lfo4.amount);
        self.lfo4.set_deform(params.lfo4.deform);
        self.lfo4.set_deform_type(params.lfo4.deform_type);
        self.lfo4.set_sync_mode(params.lfo4.sync_mode);
        self.lfo4.set_sync_division(params.lfo4.sync_division);
        self.lfo4.set_trigger_mode(params.lfo4.trigger_mode);
        self.lfo4.set_env_params(
            params.lfo4.env_delay,
            params.lfo4.env_attack,
            params.lfo4.env_hold,
            params.lfo4.env_decay,
            params.lfo4.env_sustain,
            params.lfo4.env_release,
        );
        self.lfo4.set_start_phase(params.lfo4.start_phase);
        self.lfo4.set_unipolar(params.lfo4.unipolar);
        self.lfo4.set_env_tempo_sync(params.lfo4.env_tempo_sync);
        self.lfo5.set_rate_hz(params.lfo5.rate_hz);
        self.lfo5.set_shape(params.lfo5.shape);
        self.lfo5.set_amount(params.lfo5.amount);
        self.lfo5.set_deform(params.lfo5.deform);
        self.lfo5.set_deform_type(params.lfo5.deform_type);
        self.lfo5.set_sync_mode(params.lfo5.sync_mode);
        self.lfo5.set_sync_division(params.lfo5.sync_division);
        self.lfo5.set_trigger_mode(params.lfo5.trigger_mode);
        self.lfo5.set_env_params(
            params.lfo5.env_delay,
            params.lfo5.env_attack,
            params.lfo5.env_hold,
            params.lfo5.env_decay,
            params.lfo5.env_sustain,
            params.lfo5.env_release,
        );
        self.lfo5.set_start_phase(params.lfo5.start_phase);
        self.lfo5.set_unipolar(params.lfo5.unipolar);
        self.lfo5.set_env_tempo_sync(params.lfo5.env_tempo_sync);
        self.lfo6.set_rate_hz(params.lfo6.rate_hz);
        self.lfo6.set_shape(params.lfo6.shape);
        self.lfo6.set_amount(params.lfo6.amount);
        self.lfo6.set_deform(params.lfo6.deform);
        self.lfo6.set_deform_type(params.lfo6.deform_type);
        self.lfo6.set_sync_mode(params.lfo6.sync_mode);
        self.lfo6.set_sync_division(params.lfo6.sync_division);
        self.lfo6.set_trigger_mode(params.lfo6.trigger_mode);
        self.lfo6.set_env_params(
            params.lfo6.env_delay,
            params.lfo6.env_attack,
            params.lfo6.env_hold,
            params.lfo6.env_decay,
            params.lfo6.env_sustain,
            params.lfo6.env_release,
        );
        self.lfo6.set_start_phase(params.lfo6.start_phase);
        self.lfo6.set_unipolar(params.lfo6.unipolar);
        self.lfo6.set_env_tempo_sync(params.lfo6.env_tempo_sync);

        // Update step sequencer values
        for (i, step) in params.step_seq_values.iter().enumerate() {
            self.lfo1.stepseq.steps[i] = *step;
            self.lfo2.stepseq.steps[i] = *step;
            self.lfo3.stepseq.steps[i] = *step;
            self.lfo4.stepseq.steps[i] = *step;
            self.lfo5.stepseq.steps[i] = *step;
            self.lfo6.stepseq.steps[i] = *step;
        }
        self.lfo1.stepseq.loop_start = params.step_seq_loop_start;
        self.lfo1.stepseq.loop_end = params.step_seq_loop_end;
        self.lfo1.stepseq.shuffle = params.step_seq_shuffle;
        self.lfo2.stepseq.loop_start = params.step_seq_loop_start;
        self.lfo2.stepseq.loop_end = params.step_seq_loop_end;
        self.lfo2.stepseq.shuffle = params.step_seq_shuffle;
        self.lfo3.stepseq.loop_start = params.step_seq_loop_start;
        self.lfo3.stepseq.loop_end = params.step_seq_loop_end;
        self.lfo3.stepseq.shuffle = params.step_seq_shuffle;
        self.lfo4.stepseq.loop_start = params.step_seq_loop_start;
        self.lfo4.stepseq.loop_end = params.step_seq_loop_end;
        self.lfo4.stepseq.shuffle = params.step_seq_shuffle;
        self.lfo5.stepseq.loop_start = params.step_seq_loop_start;
        self.lfo5.stepseq.loop_end = params.step_seq_loop_end;
        self.lfo5.stepseq.shuffle = params.step_seq_shuffle;
        self.lfo6.stepseq.loop_start = params.step_seq_loop_start;
        self.lfo6.stepseq.loop_end = params.step_seq_loop_end;
        self.lfo6.stepseq.shuffle = params.step_seq_shuffle;

        // Update MSEG data on all LFOs
        for i in 0..MSEG_MAX_NODES {
            self.lfo1.mseg.nodes[i] = params.mseg_nodes[i];
            self.lfo2.mseg.nodes[i] = params.mseg_nodes[i];
            self.lfo3.mseg.nodes[i] = params.mseg_nodes[i];
            self.lfo4.mseg.nodes[i] = params.mseg_nodes[i];
            self.lfo5.mseg.nodes[i] = params.mseg_nodes[i];
            self.lfo6.mseg.nodes[i] = params.mseg_nodes[i];
        }
        for i in 0..MSEG_MAX_SEGMENTS {
            self.lfo1.mseg.curves[i] = params.mseg_curves[i];
            self.lfo2.mseg.curves[i] = params.mseg_curves[i];
            self.lfo3.mseg.curves[i] = params.mseg_curves[i];
            self.lfo4.mseg.curves[i] = params.mseg_curves[i];
            self.lfo5.mseg.curves[i] = params.mseg_curves[i];
            self.lfo6.mseg.curves[i] = params.mseg_curves[i];
        }
        self.lfo1.mseg.loop_start = params.mseg_loop_start;
        self.lfo1.mseg.loop_end = params.mseg_loop_end;
        self.lfo1.mseg.loop_mode = params.mseg_loop_mode;
        self.lfo2.mseg.loop_start = params.mseg_loop_start;
        self.lfo2.mseg.loop_end = params.mseg_loop_end;
        self.lfo2.mseg.loop_mode = params.mseg_loop_mode;
        self.lfo3.mseg.loop_start = params.mseg_loop_start;
        self.lfo3.mseg.loop_end = params.mseg_loop_end;
        self.lfo3.mseg.loop_mode = params.mseg_loop_mode;
        self.lfo4.mseg.loop_start = params.mseg_loop_start;
        self.lfo4.mseg.loop_end = params.mseg_loop_end;
        self.lfo4.mseg.loop_mode = params.mseg_loop_mode;
        self.lfo5.mseg.loop_start = params.mseg_loop_start;
        self.lfo5.mseg.loop_end = params.mseg_loop_end;
        self.lfo5.mseg.loop_mode = params.mseg_loop_mode;
        self.lfo6.mseg.loop_start = params.mseg_loop_start;
        self.lfo6.mseg.loop_end = params.mseg_loop_end;
        self.lfo6.mseg.loop_mode = params.mseg_loop_mode;

        // Update filters
        self.filter1_l
            .set_params(params.filter1.cutoff_hz, params.filter1.resonance);
        self.filter1_r
            .set_params(params.filter1.cutoff_hz, params.filter1.resonance);
        self.filter2_l
            .set_params(params.filter2.cutoff_hz, params.filter2.resonance);
        self.filter2_r
            .set_params(params.filter2.cutoff_hz, params.filter2.resonance);
        let lowcut_hz = params.lowcut_hz.clamp(20.0, 20000.0);
        self.lowcut_l.set_params(lowcut_hz, 0.7);
        self.lowcut_l.prepare_block(lowcut_hz, 0.7, 1);
        self.lowcut_r.set_params(lowcut_hz, 0.7);
        self.lowcut_r.prepare_block(lowcut_hz, 0.7, 1);
        self.filter1_l.set_filter_type(params.filter1.filter_type);
        self.filter1_r.set_filter_type(params.filter1.filter_type);
        self.filter2_l.set_filter_type(params.filter2.filter_type);
        self.filter2_r.set_filter_type(params.filter2.filter_type);
        self.filter1_l.set_drive(params.filter1.drive);
        self.filter1_r.set_drive(params.filter1.drive);
        self.filter2_l.set_drive(params.filter2.drive);
        self.filter2_r.set_drive(params.filter2.drive);
        self.filter1_l
            .set_feedback_drive(params.filter1.feedback_drive);
        self.filter1_r
            .set_feedback_drive(params.filter1.feedback_drive);
        self.filter2_l
            .set_feedback_drive(params.filter2.feedback_drive);
        self.filter2_r
            .set_feedback_drive(params.filter2.feedback_drive);
        self.filter1_l.set_subtype(params.filter1.subtype);
        self.filter1_r.set_subtype(params.filter1.subtype);
        self.filter2_l.set_subtype(params.filter2.subtype);
        self.filter2_r.set_subtype(params.filter2.subtype);

        // Update oscillators (reconstruct if type changed)
        for (idx, osc) in self.oscillators.iter_mut().enumerate() {
            let settings = &params.oscs[idx];
            if osc.osc_type() != settings.osc_type {
                *osc = Oscillator::new(settings.osc_type, self.sample_rate);
            }
            osc.set_shape(settings.shape);
            osc.set_skew(settings.skew);
            osc.set_formant(settings.formant);
            osc.set_sync_amount(settings.sync);
            osc.set_unison(settings.unison_voices as usize, settings.unison_detune);
            osc.set_unison_spread(settings.unison_spread);
            if let Oscillator::Classic(o) = osc {
                o.set_waveform(ClassicWaveform::from_u8(settings.waveform));
                o.set_sub_level(settings.sub_level);
                o.set_sub_octave(settings.sub_octave as i8);
            }
            if let Oscillator::String(o) = osc {
                o.set_exciter(ExciterType::from_u8(settings.waveform));
            }
            if let Oscillator::Sine(o) = osc {
                o.set_pm_mode(settings.pm_mode);
                o.set_shaper_mode(SineShaperMode::from_u8(settings.shaper_mode));
            }
            if let Oscillator::Fm2(o) = osc {
                o.set_feedback(settings.fm2_feedback);
                o.set_m12offset(settings.fm2_m12offset);
                o.set_m12phase(settings.fm2_m12phase);
                o.set_feedback_mode(Fm2FeedbackMode::from_u8(settings.fm2_feedback_mode));
            }
            if let Oscillator::Fm3(o) = osc {
                o.set_m3_abs_freq(settings.fm3_m3_abs_freq);
                o.set_feedback(settings.fm3_feedback);
                o.set_feedback_mode(super::Fm3FeedbackMode::from_u8(settings.fm3_feedback_mode));
            }
            if let Oscillator::Sine(o) = osc {
                o.set_lowcut(settings.sine_lowcut);
                o.set_highcut(settings.sine_highcut);
            }
            if let Oscillator::Window(o) = osc {
                o.set_lowcut(settings.window_lowcut);
                o.set_highcut(settings.window_highcut);
            }
            if let Oscillator::Modern(o) = osc {
                o.set_sub_octave(settings.sub_octave as i8);
                o.set_sub_waveform(ModernSubWaveform::from_u8(settings.waveform));
                o.set_sub_one(settings.sub_one);
            }
            if let Oscillator::Alias(o) = osc {
                for (i, &amp) in settings.alias_partials.iter().enumerate() {
                    o.set_partial_amplitude(i, amp);
                }
            }
            if let Oscillator::Window(o) = osc {
                o.set_window_type(WindowType::from_u8(settings.waveform));
            }
            if let Oscillator::String(o) = osc {
                o.set_stereo_spread(params.string_stereo_spread);
                o.set_exciter(ExciterType::from_u8(settings.waveform));
            }
            if let Oscillator::Wavetable(o) = osc {
                o.set_keytrack(params.wavetable_keytrack);
            }
            if let Oscillator::Alias(o) = osc {
                o.set_waveform(AliasWaveform::from_u8(settings.waveform));
            }
            osc.set_sh_noise_correlation(params.sh_noise_correlation);
            osc.set_sh_noise_width(params.sh_noise_width);
            osc.set_sh_noise_sync(params.sh_noise_sync);
            osc.set_sh_noise_lowcut(settings.sh_noise_lowcut);
            osc.set_sh_noise_highcut(settings.sh_noise_highcut);
            osc.set_width2(settings.width2);
            osc.set_skew_v(settings.wavetable_skew_v);
            osc.set_saturate(settings.wavetable_saturate);
            osc.set_tone_lp(settings.string_tone_lp);
            osc.set_tone_hp(settings.string_tone_hp);
            osc.set_sampler_mode(settings.wavetable_sampler_mode);
            osc.set_dual_detune(settings.string_dual_detune);
            osc.set_dual_decay(settings.string_dual_decay);
            osc.set_oversample(settings.string_oversample);
            if let Oscillator::Twist(o) = osc {
                o.set_aux_mix(params.twist_aux_mix);
                o.set_lpg_response(params.twist_lpg_response);
                o.set_lpg_decay(params.twist_lpg_decay);
            }
        }

        // Update noise
        self.noise.noise_type = params.noise.noise_type;
        self.noise.level = params.noise.level;
        self.noise.color = params.noise.color;
        self.noise.color_mode = if params.noise.color_mode == 0 {
            NoiseColorMode::Tilt
        } else {
            NoiseColorMode::Legacy
        };
        self.noise.filter_enabled = params.noise.filter_enabled;
        if params.noise.filter_enabled {
            self.noise.filter.set_filter_type(params.noise.filter_type);
            self.noise
                .filter
                .set_params(params.noise.filter_cutoff, params.noise.filter_resonance);
            self.noise.filter.prepare_block(
                params.noise.filter_cutoff,
                params.noise.filter_resonance,
                1,
            );
        }

        // Update character
        self.character.set_type(params.character);
        self.character.cutoff_hz = params.character_cutoff;
        self.character.resonance = params.character_resonance;
        self.character2.set_type(params.character);
        self.character2.cutoff_hz = params.character_cutoff;
        self.character2.resonance = params.character_resonance;

        // Update waveshaper
        self.waveshaper.set_shape(params.waveshaper.shape);
        self.waveshaper.drive = params.waveshaper.drive;
        self.waveshaper.mix = params.waveshaper.mix;
    }

    pub fn set_mts_esp(&mut self, client: Option<Arc<Mutex<MtsEspClient>>>) {
        self.mts_esp = client;
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.filter1_l = Filter::new(self.params.filter1.filter_type, sample_rate);
        self.filter1_r = Filter::new(self.params.filter1.filter_type, sample_rate);
        self.filter2_l = Filter::new(self.params.filter2.filter_type, sample_rate);
        self.filter2_r = Filter::new(self.params.filter2.filter_type, sample_rate);
        self.lowcut_l = Filter::new(FilterType::Highpass12dB, sample_rate);
        self.lowcut_r = Filter::new(FilterType::Highpass12dB, sample_rate);
        self.amp_eg.set_sample_rate(sample_rate);
        self.filter_eg.set_sample_rate(sample_rate);
        self.pitch_eg.set_sample_rate(sample_rate);
        self.lfo1.set_sample_rate(sample_rate);
        self.lfo2.set_sample_rate(sample_rate);
        self.lfo3.set_sample_rate(sample_rate);
        self.lfo4.set_sample_rate(sample_rate);
        self.lfo5.set_sample_rate(sample_rate);
        self.lfo6.set_sample_rate(sample_rate);
        self.noise = NoiseGenerator::new(sample_rate);
        self.character.set_sample_rate(sample_rate);
    }

    pub fn set_eg_tempo(&mut self, tempo_bpm: f32) {
        self.amp_eg.set_tempo(tempo_bpm);
        self.filter_eg.set_tempo(tempo_bpm);
        self.pitch_eg.set_tempo(tempo_bpm);
    }

    pub fn set_scene_lfo_outputs(&mut self, outputs: [f32; 6]) {
        self.scene_lfo1_output = outputs[0];
        self.scene_lfo2_output = outputs[1];
        self.scene_lfo3_output = outputs[2];
        self.scene_lfo4_output = outputs[3];
        self.scene_lfo5_output = outputs[4];
        self.scene_lfo6_output = outputs[5];
    }

    pub fn set_key_mod_values(&mut self, lowest: f32, highest: f32, latest: f32) {
        self.lowest_key = lowest;
        self.highest_key = highest;
        self.latest_key = latest;
    }

    pub fn trigger(&mut self, note: u8, velocity: f32) {
        self.note = note;
        self.velocity = velocity;
        self.gate = true;
        self.active = true;
        self.sample_counter = 0;
        self.note_counter += 1;

        self.filter_feedback_prev_l = 0.0;
        self.filter_feedback_prev_r = 0.0;
        self.target_freq = note_to_freq(note, &self.tuning, &self.mts_esp);
        let portamento_time = if self.params.portamento_sync && self.tempo_bpm > 0.0 {
            self.params.portamento * 60.0 / self.tempo_bpm
        } else {
            self.params.portamento
        };
        if portamento_time <= 0.0 || !self.amp_eg.is_active() {
            self.current_freq = self.target_freq;
        }

        self.update_oscillator_freqs(&ModValues::default());
        for (idx, osc) in self.oscillators.iter_mut().enumerate() {
            match self.params.oscs[idx].phase_mode {
                OscPhaseMode::Random => osc.reset(),
                OscPhaseMode::Zero => osc.reset_to_zero(),
                OscPhaseMode::Current => { /* keep current phase */ }
            }
        }
        self.noise.reset();

        self.amp_eg.trigger();
        self.filter_eg.trigger();
        self.pitch_eg.trigger();
        self.last_scale_degree = freq_to_scale_degree(self.target_freq, &self.tuning);

        self.lfo1.reset();
        self.lfo2.reset();
        self.lfo3.reset();
        self.lfo4.reset();
        self.lfo5.reset();
        self.lfo6.reset();

        // New random/alternate values per note
        self.random_value = random::<f32>() * 2.0 - 1.0;
        self.alternate_sign = if self.note_counter.is_multiple_of(2) {
            1.0
        } else {
            -1.0
        };
    }

    pub fn release(&mut self) {
        self.gate = false;
        self.amp_eg.release();
        self.filter_eg.release();
        self.pitch_eg.release();
        self.lfo1.release();
        self.lfo2.release();
        self.lfo3.release();
        self.lfo4.release();
        self.lfo5.release();
        self.lfo6.release();
    }

    pub fn kill(&mut self) {
        self.active = false;
        self.gate = false;
    }

    pub fn is_active(&self) -> bool {
        self.active && (self.amp_eg.is_active() || self.gate)
    }

    // -----------------------------------------------------------------------
    // Modulation
    // -----------------------------------------------------------------------

    fn get_mod_source_value(&self, source: ModSource) -> f32 {
        match source {
            ModSource::Velocity => self.velocity,
            ModSource::Keytrack => (self.note as f32 - 60.0) / 60.0,
            ModSource::ModWheel => self.params.mod_wheel,
            ModSource::Aftertouch => self.params.aftertouch,
            ModSource::PitchBend => self.pitch_bend,
            ModSource::Lfo1 => self.lfo1_output,
            ModSource::Lfo2 => self.lfo2_output,
            ModSource::Lfo3 => self.lfo3_output,
            ModSource::Lfo4 => self.lfo4_output,
            ModSource::Lfo5 => self.lfo5_output,
            ModSource::Lfo6 => self.lfo6_output,
            ModSource::AmpEg => self.amp_eg_output,
            ModSource::FilterEg => self.filter_eg_output,
            ModSource::PitchEg => self.pitch_eg_output,
            ModSource::RandomBipolar => self.random_value,
            ModSource::RandomUnipolar => (self.random_value + 1.0) * 0.5,
            ModSource::AlternateBipolar => self.alternate_sign,
            ModSource::AlternateUnipolar => (self.alternate_sign + 1.0) * 0.5,
            ModSource::Macro1 => self.params.macros[0],
            ModSource::Macro2 => self.params.macros[1],
            ModSource::Macro3 => self.params.macros[2],
            ModSource::Macro4 => self.params.macros[3],
            ModSource::Macro5 => self.params.macros[4],
            ModSource::Macro6 => self.params.macros[5],
            ModSource::Macro7 => self.params.macros[6],
            ModSource::Macro8 => self.params.macros[7],
            ModSource::Breath => self.params.breath,
            ModSource::Expression => self.params.expression,
            ModSource::Sustain => self.params.sustain,
            ModSource::PolyAftertouch => self.params.poly_aftertouch,
            ModSource::MpeTimbre => self.params.mpe_timbre,
            ModSource::ReleaseVelocity => self.params.release_velocity,
            ModSource::Constant => 1.0,
            ModSource::NoteGate => {
                if self.gate {
                    1.0
                } else {
                    0.0
                }
            }
            ModSource::NoteExpressionVolume => self.params.note_expression_volume,
            ModSource::NoteExpressionPan => self.params.note_expression_pan,
            ModSource::SceneLfo1 => self.scene_lfo1_output,
            ModSource::SceneLfo2 => self.scene_lfo2_output,
            ModSource::SceneLfo3 => self.scene_lfo3_output,
            ModSource::SceneLfo4 => self.scene_lfo4_output,
            ModSource::SceneLfo5 => self.scene_lfo5_output,
            ModSource::SceneLfo6 => self.scene_lfo6_output,
            ModSource::LowestKey => self.lowest_key,
            ModSource::HighestKey => self.highest_key,
            ModSource::LatestKey => self.latest_key,
        }
    }

    /// Compute all modulation target deltas for the current sample.
    fn compute_mod_values(&self) -> ModValues {
        let mut vals = ModValues::default();

        // First pass: compute depth modulations using base depths.
        // These routes modulate the depth of other routes.
        for routing in self.params.modulations.iter() {
            if !routing.active || routing.depth == 0.0 {
                continue;
            }
            let src_val = self.get_mod_source_value(routing.source);
            let curved_depth = routing.depth_curve.apply(routing.depth);
            let delta = src_val * curved_depth;

            match routing.target {
                ModTarget::ModRoute1Depth => vals.mod_depth[0] += delta,
                ModTarget::ModRoute2Depth => vals.mod_depth[1] += delta,
                ModTarget::ModRoute3Depth => vals.mod_depth[2] += delta,
                ModTarget::ModRoute4Depth => vals.mod_depth[3] += delta,
                ModTarget::ModRoute5Depth => vals.mod_depth[4] += delta,
                ModTarget::ModRoute6Depth => vals.mod_depth[5] += delta,
                ModTarget::ModRoute7Depth => vals.mod_depth[6] += delta,
                ModTarget::ModRoute8Depth => vals.mod_depth[7] += delta,
                ModTarget::ModRoute9Depth => vals.mod_depth[8] += delta,
                ModTarget::ModRoute10Depth => vals.mod_depth[9] += delta,
                ModTarget::ModRoute11Depth => vals.mod_depth[10] += delta,
                ModTarget::ModRoute12Depth => vals.mod_depth[11] += delta,
                _ => {}
            }
        }

        // Second pass: compute all other modulations using adjusted depths.
        for (i, routing) in self.params.modulations.iter().enumerate() {
            if !routing.active {
                continue;
            }
            let effective_depth = (routing.depth + vals.mod_depth[i]).clamp(-1.0, 1.0);
            if effective_depth == 0.0 {
                continue;
            }
            let src_val = self.get_mod_source_value(routing.source);
            let curved_depth = routing.depth_curve.apply(effective_depth);
            let delta = src_val * curved_depth;

            match routing.target {
                ModTarget::Osc1Pitch => vals.osc_pitch[0] += delta,
                ModTarget::Osc2Pitch => vals.osc_pitch[1] += delta,
                ModTarget::Osc3Pitch => vals.osc_pitch[2] += delta,
                ModTarget::Osc1Level => vals.osc_level[0] += delta,
                ModTarget::Osc2Level => vals.osc_level[1] += delta,
                ModTarget::Osc3Level => vals.osc_level[2] += delta,
                ModTarget::Osc1Shape => vals.osc_shape[0] += delta,
                ModTarget::Osc2Shape => vals.osc_shape[1] += delta,
                ModTarget::Osc3Shape => vals.osc_shape[2] += delta,
                ModTarget::Osc1Skew => vals.osc_skew[0] += delta,
                ModTarget::Osc2Skew => vals.osc_skew[1] += delta,
                ModTarget::Osc3Skew => vals.osc_skew[2] += delta,
                ModTarget::Osc1Formant => vals.osc_formant[0] += delta,
                ModTarget::Osc2Formant => vals.osc_formant[1] += delta,
                ModTarget::Osc3Formant => vals.osc_formant[2] += delta,
                ModTarget::Filter1Cutoff => vals.f1_cutoff += delta,
                ModTarget::Filter1Resonance => vals.f1_resonance += delta,
                ModTarget::Filter1EgAmount => vals.f1_eg_amount += delta,
                ModTarget::Filter1Drive => vals.f1_drive += delta,
                ModTarget::Filter2Cutoff => vals.f2_cutoff += delta,
                ModTarget::Filter2Resonance => vals.f2_resonance += delta,
                ModTarget::Filter2EgAmount => vals.f2_eg_amount += delta,
                ModTarget::Filter2Drive => vals.f2_drive += delta,
                ModTarget::AmpAttack => vals.amp_attack += delta,
                ModTarget::AmpDecay => vals.amp_decay += delta,
                ModTarget::AmpSustain => vals.amp_sustain += delta,
                ModTarget::AmpRelease => vals.amp_release += delta,
                ModTarget::FilterAttack => vals.filter_attack += delta,
                ModTarget::FilterDecay => vals.filter_decay += delta,
                ModTarget::FilterSustain => vals.filter_sustain += delta,
                ModTarget::FilterRelease => vals.filter_release += delta,
                ModTarget::PitchAttack => vals.pitch_attack += delta,
                ModTarget::PitchDecay => vals.pitch_decay += delta,
                ModTarget::PitchSustain => vals.pitch_sustain += delta,
                ModTarget::PitchRelease => vals.pitch_release += delta,
                ModTarget::Lfo1Rate => vals.lfo1_rate += delta,
                ModTarget::Lfo1Amount => vals.lfo1_amount += delta,
                ModTarget::Lfo1Deform => vals.lfo1_deform += delta,
                ModTarget::Lfo2Rate => vals.lfo2_rate += delta,
                ModTarget::Lfo2Amount => vals.lfo2_amount += delta,
                ModTarget::Lfo2Deform => vals.lfo2_deform += delta,
                ModTarget::Lfo3Rate => vals.lfo3_rate += delta,
                ModTarget::Lfo3Amount => vals.lfo3_amount += delta,
                ModTarget::Lfo3Deform => vals.lfo3_deform += delta,
                ModTarget::Lfo4Rate => vals.lfo4_rate += delta,
                ModTarget::Lfo4Amount => vals.lfo4_amount += delta,
                ModTarget::Lfo4Deform => vals.lfo4_deform += delta,
                ModTarget::Lfo5Rate => vals.lfo5_rate += delta,
                ModTarget::Lfo5Amount => vals.lfo5_amount += delta,
                ModTarget::Lfo5Deform => vals.lfo5_deform += delta,
                ModTarget::Lfo6Rate => vals.lfo6_rate += delta,
                ModTarget::Lfo6Amount => vals.lfo6_amount += delta,
                ModTarget::Lfo6Deform => vals.lfo6_deform += delta,
                ModTarget::Lfo1Phase => vals.lfo1_phase += delta,
                ModTarget::Lfo2Phase => vals.lfo2_phase += delta,
                ModTarget::Lfo3Phase => vals.lfo3_phase += delta,
                ModTarget::Lfo4Phase => vals.lfo4_phase += delta,
                ModTarget::Lfo5Phase => vals.lfo5_phase += delta,
                ModTarget::Lfo6Phase => vals.lfo6_phase += delta,
                ModTarget::OutputVolume => vals.output_volume += delta,
                ModTarget::OutputPan => vals.output_pan += delta,
                ModTarget::OutputWidth => vals.output_width += delta,
                ModTarget::NoiseLevel => vals.noise_level += delta,
                ModTarget::WaveshaperDrive => vals.waveshaper_drive += delta,
                ModTarget::Portamento => vals.portamento += delta,
                ModTarget::CharacterCutoff => vals.character_cutoff += delta,
                ModTarget::FilterBalance => vals.filter_balance += delta,
                ModTarget::OscFmDepth => vals.osc_fm_depth += delta,
                ModTarget::Osc1Sync => vals.osc_sync[0] += delta,
                ModTarget::Osc2Sync => vals.osc_sync[1] += delta,
                ModTarget::Osc3Sync => vals.osc_sync[2] += delta,
                // Depth-modulation targets are handled in the first pass
                ModTarget::ModRoute1Depth
                | ModTarget::ModRoute2Depth
                | ModTarget::ModRoute3Depth
                | ModTarget::ModRoute4Depth
                | ModTarget::ModRoute5Depth
                | ModTarget::ModRoute6Depth
                | ModTarget::ModRoute7Depth
                | ModTarget::ModRoute8Depth
                | ModTarget::ModRoute9Depth
                | ModTarget::ModRoute10Depth
                | ModTarget::ModRoute11Depth
                | ModTarget::ModRoute12Depth => {}
            }
        }

        vals
    }

    pub fn process_block(
        &mut self,
        out_l: &mut [f32],
        out_r: &mut [f32],
        audio_in_l: Option<&[f32]>,
        audio_in_r: Option<&[f32]>,
    ) {
        let frames = out_l.len().min(out_r.len());
        if frames == 0 {
            return;
        }

        // Compute effective portamento time (seconds)
        let portamento_time = if self.params.portamento_sync && self.tempo_bpm > 0.0 {
            // portamento is in beats; convert to seconds
            self.params.portamento * 60.0 / self.tempo_bpm
        } else {
            self.params.portamento
        };

        // Pre-compute filter coefficients for the block using current modulation state
        // (approximation: filter coeffs are constant over the block)
        let f1_cutoff_base = if self.params.filter1.enabled {
            let base = self.params.filter1.cutoff_hz;
            let eg_mod = self.filter_eg_output * self.params.filter1.eg_amount * 10000.0;
            let lfo_mod = self.lfo1_output * 5000.0;
            let key_track = self.params.filter1.key_tracking * (self.note as f32 - 60.0) * 50.0;
            (base + eg_mod + lfo_mod + key_track).clamp(20.0, 20000.0)
        } else {
            20000.0
        };
        let f1_res_base = if self.params.filter1.enabled {
            self.params.filter1.resonance.clamp(0.01, 10.0)
        } else {
            0.7
        };
        self.filter1_l
            .prepare_block(f1_cutoff_base, f1_res_base, frames);
        self.filter1_r
            .prepare_block(f1_cutoff_base, f1_res_base, frames);

        let f2_cutoff_base = if self.params.filter2.enabled {
            let base = if self.params.f2_cutoff_offset {
                f1_cutoff_base * (self.params.filter2.cutoff_hz / 10000.0).clamp(0.2, 5.0)
            } else {
                self.params.filter2.cutoff_hz
            };
            let eg_mod = self.filter_eg_output * self.params.filter2.eg_amount * 10000.0;
            let lfo_mod = self.lfo2_output * 5000.0;
            let key_track = self.params.filter2.key_tracking * (self.note as f32 - 60.0) * 50.0;
            (base + eg_mod + lfo_mod + key_track).clamp(20.0, 20000.0)
        } else {
            20000.0
        };
        let f2_res_base = if self.params.filter2.enabled {
            if self.params.f2_res_link {
                f1_res_base
            } else {
                self.params.filter2.resonance.clamp(0.01, 10.0)
            }
        } else {
            0.7
        };
        self.filter2_l
            .prepare_block(f2_cutoff_base, f2_res_base, frames);
        self.filter2_r
            .prepare_block(f2_cutoff_base, f2_res_base, frames);

        for i in 0..frames {
            let audio_l = audio_in_l.map(|b| b[i]).unwrap_or(0.0);
            let audio_r = audio_in_r.map(|b| b[i]).unwrap_or(0.0);
            // Portamento / Glissando
            let freq_diff = self.target_freq - self.current_freq;
            if portamento_time > 0.0 && freq_diff.abs() > 0.01 {
                if self.params.glissando {
                    // Semitone-quantized portamento
                    let current_note = freq_to_note(self.current_freq);
                    let target_note = freq_to_note(self.target_freq).round();
                    let note_diff = target_note - current_note;
                    if note_diff.abs() > 0.01 {
                        let rate = 1.0 / (portamento_time * self.sample_rate);
                        let step = note_diff.signum() * rate.min(1.0);
                        let next_note = if note_diff > 0.0 {
                            (current_note + step).min(target_note)
                        } else {
                            (current_note + step).max(target_note)
                        };
                        self.current_freq =
                            note_to_freq(next_note as u8, &self.tuning, &self.mts_esp);
                    } else {
                        self.current_freq =
                            note_to_freq(target_note as u8, &self.tuning, &self.mts_esp);
                    }
                } else {
                    match self.params.portamento_curve {
                        PortamentoCurve::Linear => {
                            let rate = 1.0 / (portamento_time * self.sample_rate);
                            self.current_freq += freq_diff * rate.min(1.0);
                        }
                        PortamentoCurve::Exponential => {
                            let rate = 1.0 / (portamento_time * self.sample_rate);
                            self.current_freq += freq_diff
                                * rate.min(1.0)
                                * (self.current_freq / self.target_freq.max(1.0));
                        }
                        PortamentoCurve::ConstantTime => {
                            let rate = 1.0 / (portamento_time * self.sample_rate);
                            let log_diff = (self.target_freq / self.current_freq).ln();
                            self.current_freq *= (log_diff * rate).exp();
                        }
                    }
                }
            } else {
                self.current_freq = self.target_freq;
            }

            // Portamento retrigger at scale degrees
            if self.params.portamento_retrigger && portamento_time > 0.0 && freq_diff.abs() > 0.01 {
                let current_degree = freq_to_scale_degree(self.current_freq, &self.tuning);
                if current_degree != self.last_scale_degree {
                    self.amp_eg.trigger();
                    self.filter_eg.trigger();
                    self.pitch_eg.trigger();
                    self.last_scale_degree = current_degree;
                }
            } else {
                self.last_scale_degree = freq_to_scale_degree(self.current_freq, &self.tuning);
            }

            // Compute modulation values using previous sample's LFO/EG outputs
            let mods = self.compute_mod_values();

            // Apply modulated params to LFOs
            self.lfo1
                .set_rate_hz((self.params.lfo1.rate_hz + mods.lfo1_rate).max(0.001));
            self.lfo1
                .set_amount((self.params.lfo1.amount + mods.lfo1_amount).clamp(0.0, 1.0));
            self.lfo1
                .set_deform((self.params.lfo1.deform + mods.lfo1_deform).clamp(-1.0, 1.0));
            self.lfo1.phase_offset = (self.params.lfo1.start_phase + mods.lfo1_phase).fract();
            self.lfo2
                .set_rate_hz((self.params.lfo2.rate_hz + mods.lfo2_rate).max(0.001));
            self.lfo2
                .set_amount((self.params.lfo2.amount + mods.lfo2_amount).clamp(0.0, 1.0));
            self.lfo2
                .set_deform((self.params.lfo2.deform + mods.lfo2_deform).clamp(-1.0, 1.0));
            self.lfo2.phase_offset = (self.params.lfo2.start_phase + mods.lfo2_phase).fract();
            self.lfo3
                .set_rate_hz((self.params.lfo3.rate_hz + mods.lfo3_rate).max(0.001));
            self.lfo3
                .set_amount((self.params.lfo3.amount + mods.lfo3_amount).clamp(0.0, 1.0));
            self.lfo3
                .set_deform((self.params.lfo3.deform + mods.lfo3_deform).clamp(-1.0, 1.0));
            self.lfo3.phase_offset = (self.params.lfo3.start_phase + mods.lfo3_phase).fract();
            self.lfo4
                .set_rate_hz((self.params.lfo4.rate_hz + mods.lfo4_rate).max(0.001));
            self.lfo4
                .set_amount((self.params.lfo4.amount + mods.lfo4_amount).clamp(0.0, 1.0));
            self.lfo4
                .set_deform((self.params.lfo4.deform + mods.lfo4_deform).clamp(-1.0, 1.0));
            self.lfo4.phase_offset = (self.params.lfo4.start_phase + mods.lfo4_phase).fract();
            self.lfo5
                .set_rate_hz((self.params.lfo5.rate_hz + mods.lfo5_rate).max(0.001));
            self.lfo5
                .set_amount((self.params.lfo5.amount + mods.lfo5_amount).clamp(0.0, 1.0));
            self.lfo5
                .set_deform((self.params.lfo5.deform + mods.lfo5_deform).clamp(-1.0, 1.0));
            self.lfo5.phase_offset = (self.params.lfo5.start_phase + mods.lfo5_phase).fract();
            self.lfo6
                .set_rate_hz((self.params.lfo6.rate_hz + mods.lfo6_rate).max(0.001));
            self.lfo6
                .set_amount((self.params.lfo6.amount + mods.lfo6_amount).clamp(0.0, 1.0));
            self.lfo6
                .set_deform((self.params.lfo6.deform + mods.lfo6_deform).clamp(-1.0, 1.0));
            self.lfo6.phase_offset = (self.params.lfo6.start_phase + mods.lfo6_phase).fract();

            // Apply modulated params to envelopes
            self.amp_eg
                .set_attack((self.params.amp_eg.attack + mods.amp_attack).max(0.0));
            self.amp_eg
                .set_decay((self.params.amp_eg.decay + mods.amp_decay).max(0.0));
            self.amp_eg
                .set_sustain((self.params.amp_eg.sustain + mods.amp_sustain).clamp(0.0, 1.0));
            self.amp_eg
                .set_release((self.params.amp_eg.release + mods.amp_release).max(0.0));
            self.filter_eg
                .set_attack((self.params.filter_eg.attack + mods.filter_attack).max(0.0));
            self.filter_eg
                .set_decay((self.params.filter_eg.decay + mods.filter_decay).max(0.0));
            self.filter_eg
                .set_sustain((self.params.filter_eg.sustain + mods.filter_sustain).clamp(0.0, 1.0));
            self.filter_eg
                .set_release((self.params.filter_eg.release + mods.filter_release).max(0.0));
            self.pitch_eg
                .set_attack((self.params.pitch_eg.attack + mods.pitch_attack).max(0.0));
            self.pitch_eg
                .set_decay((self.params.pitch_eg.decay + mods.pitch_decay).max(0.0));
            self.pitch_eg
                .set_sustain((self.params.pitch_eg.sustain + mods.pitch_sustain).clamp(0.0, 1.0));
            self.pitch_eg
                .set_release((self.params.pitch_eg.release + mods.pitch_release).max(0.0));

            // Advance modulation sources with modulated params
            self.lfo1_output = self.lfo1.next();
            self.lfo2_output = self.lfo2.next();
            self.lfo3_output = self.lfo3.next();
            self.lfo4_output = self.lfo4.next();
            self.lfo5_output = self.lfo5.next();
            self.lfo6_output = self.lfo6.next();

            // Step sequencer trigmask: retrigger envelopes on step change
            let lfos = [
                (&self.lfo1, self.params.lfo1.shape),
                (&self.lfo2, self.params.lfo2.shape),
                (&self.lfo3, self.params.lfo3.shape),
                (&self.lfo4, self.params.lfo4.shape),
                (&self.lfo5, self.params.lfo5.shape),
                (&self.lfo6, self.params.lfo6.shape),
            ];
            for (lfo, shape) in &lfos {
                if *shape == LfoShape::StepSeq && lfo.step_changed {
                    let mask = 1u16 << lfo.stepseq.step_index.min(15);
                    if self.params.step_seq_trig_amp & mask != 0 {
                        self.amp_eg.trigger();
                    }
                    if self.params.step_seq_trig_filter & mask != 0 {
                        self.filter_eg.trigger();
                    }
                    if self.params.step_seq_trig_pitch & mask != 0 {
                        self.pitch_eg.trigger();
                    }
                }
                if *shape == LfoShape::Mseg && lfo.mseg_seg_changed {
                    let mask = 1u16 << lfo.mseg_prev_seg.min(15);
                    if self.params.mseg_retrig_amp & mask != 0 {
                        self.amp_eg.trigger();
                    }
                    if self.params.mseg_retrig_filter & mask != 0 {
                        self.filter_eg.trigger();
                    }
                    if self.params.mseg_retrig_pitch & mask != 0 {
                        self.pitch_eg.trigger();
                    }
                }
            }

            self.amp_eg_output = self.amp_eg.next();
            self.filter_eg_output = self.filter_eg.next();
            self.pitch_eg_output = self.pitch_eg.next();

            // Update oscillator frequencies
            self.update_oscillator_freqs(&mods);

            // Generate oscillator samples (with FM routing applied during generation)
            let mut osc_samples = [(0.0f32, 0.0f32); 3];

            match self.params.osc_fm_mode {
                OscFmMode::Osc2To1 => {
                    // Generate osc2 first, then osc1 with FM from osc2, then osc3
                    if self.params.oscs[1].enabled {
                        let sync = (self.params.oscs[1].sync + mods.osc_sync[1]).clamp(0.0, 1.0);
                        self.oscillators[1].set_sync_amount(sync);
                        osc_samples[1] = self.oscillators[1].next(0.0, audio_l, audio_r);
                    }
                    if self.params.oscs[0].enabled {
                        let sync = (self.params.oscs[0].sync + mods.osc_sync[0]).clamp(0.0, 1.0);
                        self.oscillators[0].set_sync_amount(sync);
                        let depth = ((self.params.osc_fm_depth + mods.osc_fm_depth)
                            * self.params.oscs[0].fm_depth)
                            .clamp(0.0, 1.0);
                        let fm_in = (osc_samples[1].0 + osc_samples[1].1) * depth * 20.0;
                        osc_samples[0] = self.oscillators[0].next(fm_in, audio_l, audio_r);
                    }
                    if self.params.oscs[2].enabled {
                        let sync = (self.params.oscs[2].sync + mods.osc_sync[2]).clamp(0.0, 1.0);
                        self.oscillators[2].set_sync_amount(sync);
                        osc_samples[2] = self.oscillators[2].next(0.0, audio_l, audio_r);
                    }
                }
                OscFmMode::Osc3To1 => {
                    // Generate osc3 first, then osc1 with FM from osc3, then osc2
                    if self.params.oscs[2].enabled {
                        let sync = (self.params.oscs[2].sync + mods.osc_sync[2]).clamp(0.0, 1.0);
                        self.oscillators[2].set_sync_amount(sync);
                        osc_samples[2] = self.oscillators[2].next(0.0, audio_l, audio_r);
                    }
                    if self.params.oscs[0].enabled {
                        let sync = (self.params.oscs[0].sync + mods.osc_sync[0]).clamp(0.0, 1.0);
                        self.oscillators[0].set_sync_amount(sync);
                        let depth = ((self.params.osc_fm_depth + mods.osc_fm_depth)
                            * self.params.oscs[0].fm_depth)
                            .clamp(0.0, 1.0);
                        let fm_in = (osc_samples[2].0 + osc_samples[2].1) * depth * 20.0;
                        osc_samples[0] = self.oscillators[0].next(fm_in, audio_l, audio_r);
                    }
                    if self.params.oscs[1].enabled {
                        let sync = (self.params.oscs[1].sync + mods.osc_sync[1]).clamp(0.0, 1.0);
                        self.oscillators[1].set_sync_amount(sync);
                        osc_samples[1] = self.oscillators[1].next(0.0, audio_l, audio_r);
                    }
                }
                OscFmMode::Osc3To2 => {
                    // Generate osc3 first, then osc2 with FM from osc3, then osc1
                    if self.params.oscs[2].enabled {
                        let sync = (self.params.oscs[2].sync + mods.osc_sync[2]).clamp(0.0, 1.0);
                        self.oscillators[2].set_sync_amount(sync);
                        osc_samples[2] = self.oscillators[2].next(0.0, audio_l, audio_r);
                    }
                    if self.params.oscs[1].enabled {
                        let sync = (self.params.oscs[1].sync + mods.osc_sync[1]).clamp(0.0, 1.0);
                        self.oscillators[1].set_sync_amount(sync);
                        let depth = ((self.params.osc_fm_depth + mods.osc_fm_depth)
                            * self.params.oscs[1].fm_depth)
                            .clamp(0.0, 1.0);
                        let fm_in = (osc_samples[2].0 + osc_samples[2].1) * depth * 20.0;
                        osc_samples[1] = self.oscillators[1].next(fm_in, audio_l, audio_r);
                    }
                    if self.params.oscs[0].enabled {
                        let sync = (self.params.oscs[0].sync + mods.osc_sync[0]).clamp(0.0, 1.0);
                        self.oscillators[0].set_sync_amount(sync);
                        osc_samples[0] = self.oscillators[0].next(0.0, audio_l, audio_r);
                    }
                }
                _ => {
                    // Default order: osc1, osc2, osc3
                    if self.params.oscs[0].enabled {
                        let sync = (self.params.oscs[0].sync + mods.osc_sync[0]).clamp(0.0, 1.0);
                        self.oscillators[0].set_sync_amount(sync);
                        osc_samples[0] = self.oscillators[0].next(0.0, audio_l, audio_r);
                    }
                    if self.params.oscs[1].enabled {
                        let sync = (self.params.oscs[1].sync + mods.osc_sync[1]).clamp(0.0, 1.0);
                        self.oscillators[1].set_sync_amount(sync);
                        let fm_in = match self.params.osc_fm_mode {
                            OscFmMode::Osc1To2 | OscFmMode::Osc1To2To3 | OscFmMode::Osc1To3 => {
                                let depth = ((self.params.osc_fm_depth + mods.osc_fm_depth)
                                    * self.params.oscs[1].fm_depth)
                                    .clamp(0.0, 1.0);
                                (osc_samples[0].0 + osc_samples[0].1) * depth * 20.0
                            }
                            _ => 0.0,
                        };
                        osc_samples[1] = self.oscillators[1].next(fm_in, audio_l, audio_r);
                    }
                    if self.params.oscs[2].enabled {
                        let sync = (self.params.oscs[2].sync + mods.osc_sync[2]).clamp(0.0, 1.0);
                        self.oscillators[2].set_sync_amount(sync);
                        let fm_in = match self.params.osc_fm_mode {
                            OscFmMode::Osc2To3 | OscFmMode::Osc1To2To3 => {
                                let depth = ((self.params.osc_fm_depth + mods.osc_fm_depth)
                                    * self.params.oscs[2].fm_depth)
                                    .clamp(0.0, 1.0);
                                (osc_samples[1].0 + osc_samples[1].1) * depth * 20.0
                            }
                            OscFmMode::Osc1To3 => {
                                let depth = ((self.params.osc_fm_depth + mods.osc_fm_depth)
                                    * self.params.oscs[2].fm_depth)
                                    .clamp(0.0, 1.0);
                                (osc_samples[0].0 + osc_samples[0].1) * depth * 20.0
                            }
                            _ => 0.0,
                        };
                        osc_samples[2] = self.oscillators[2].next(fm_in, audio_l, audio_r);
                    }
                }
            }

            // Ring modulation / combinator applied after generation
            if self.params.osc_fm_mode == OscFmMode::Ring1x2 {
                if self.params.oscs[0].enabled && self.params.oscs[1].enabled {
                    let mode = self.params.ring12_combinator;
                    let ring_l = apply_combinator(osc_samples[0].0, osc_samples[1].0, mode);
                    let ring_r = apply_combinator(osc_samples[0].1, osc_samples[1].1, mode);
                    osc_samples[0].0 = ring_l;
                    osc_samples[0].1 = ring_r;
                }
            } else if self.params.osc_fm_mode == OscFmMode::Ring2x3
                && self.params.oscs[1].enabled
                && self.params.oscs[2].enabled
            {
                let mode = self.params.ring23_combinator;
                let ring_l = apply_combinator(osc_samples[1].0, osc_samples[2].0, mode);
                let ring_r = apply_combinator(osc_samples[1].1, osc_samples[2].1, mode);
                osc_samples[1].0 = ring_l;
                osc_samples[1].1 = ring_r;
            }

            // Mix oscillators with modulation and per-source filter routing
            let mut f1_mix_l = 0.0f32;
            let mut f1_mix_r = 0.0f32;
            let mut f2_mix_l = 0.0f32;
            let mut f2_mix_r = 0.0f32;

            // Solo logic: if any source is soloed, only soloed sources pass through
            let any_osc_soloed = self.params.oscs.iter().any(|o| o.solo);
            let noise_soloed = self.params.noise.solo;
            let any_soloed = any_osc_soloed || noise_soloed;

            for (idx, osc_sample) in osc_samples.iter_mut().enumerate() {
                let osc = &self.params.oscs[idx];
                if !osc.enabled {
                    continue;
                }
                let passes = if any_soloed { osc.solo } else { !osc.mute };
                if passes {
                    let level = (osc.level + mods.osc_level[idx]).clamp(0.0, 2.0);
                    let s_l = osc_sample.0 * level;
                    let s_r = osc_sample.1 * level;
                    match osc.route {
                        OscRoute::Filter1 => {
                            f1_mix_l += s_l;
                            f1_mix_r += s_r;
                        }
                        OscRoute::Filter2 => {
                            f2_mix_l += s_l;
                            f2_mix_r += s_r;
                        }
                        _ => {
                            f1_mix_l += s_l;
                            f1_mix_r += s_r;
                            f2_mix_l += s_l;
                            f2_mix_r += s_r;
                        }
                    }
                }
            }

            // Add noise
            let noise = &self.params.noise;
            if noise.enabled {
                let passes = if any_soloed { noise.solo } else { !noise.mute };
                if passes {
                    let noise_level = (noise.level + mods.noise_level).clamp(0.0, 2.0);
                    self.noise.stereo = noise.stereo;
                    let (noise_l, noise_r) = self.noise.next_stereo();
                    let s_l = noise_l * noise_level;
                    let s_r = noise_r * noise_level;
                    match noise.route {
                        OscRoute::Filter1 => {
                            f1_mix_l += s_l;
                            f1_mix_r += s_r;
                        }
                        OscRoute::Filter2 => {
                            f2_mix_l += s_l;
                            f2_mix_r += s_r;
                        }
                        _ => {
                            f1_mix_l += s_l;
                            f1_mix_r += s_r;
                            f2_mix_l += s_l;
                            f2_mix_r += s_r;
                        }
                    }
                }
            }

            let per_source_routing = self.params.oscs.iter().any(|o| o.route != OscRoute::Both)
                || self.params.noise.route != OscRoute::Both;

            // Character filter on each mix
            let char_cutoff =
                (self.params.character_cutoff + mods.character_cutoff).clamp(20.0, 20000.0);
            self.character.cutoff_hz = char_cutoff;
            self.character2.cutoff_hz = char_cutoff;
            let (f1_char_l, f1_char_r) = self.character.process(f1_mix_l, f1_mix_r);
            let (f2_char_l, f2_char_r) = self.character2.process(f2_mix_l, f2_mix_r);

            // Total mix for backward-compatible path
            let sample_l = f1_mix_l + f2_mix_l;
            let sample_r = f1_mix_r + f2_mix_r;
            let (char_out_l, char_out_r) = if per_source_routing {
                (f1_char_l, f1_char_r)
            } else {
                self.character.process(sample_l, sample_r)
            };

            // Pre-filter gain
            let pfg = self.params.pre_filter_gain.clamp(0.0, 2.0);
            let (pre_filter_l, pre_filter_r, f2_pre_l, f2_pre_r) = if per_source_routing {
                let f1_pre_l = f1_char_l * pfg;
                let f1_pre_r = f1_char_r * pfg;
                let mut f2_pre_l = f2_char_l * pfg;
                let mut f2_pre_r = f2_char_r * pfg;
                // Lowcut on both buses
                let lowcut_hz = self.params.lowcut_hz.clamp(20.0, 20000.0);
                let slope = self.params.lowcut_slope.clamp(1, 4) as usize;
                let coeff = (std::f32::consts::PI * lowcut_hz / self.sample_rate).sin() * 2.0;
                let coeff = coeff.min(1.0);
                let mut f1_out_l = f1_pre_l;
                let mut f1_out_r = f1_pre_r;
                for i in 0..slope {
                    let out_l = f1_out_l - self.lowcut_states_l[i];
                    self.lowcut_states_l[i] += coeff * out_l;
                    f1_out_l = out_l;
                    let out_r = f1_out_r - self.lowcut_states_r[i];
                    self.lowcut_states_r[i] += coeff * out_r;
                    f1_out_r = out_r;
                }
                for i in 0..slope {
                    let out_l = f2_pre_l - self.lowcut_states_l2[i];
                    self.lowcut_states_l2[i] += coeff * out_l;
                    f2_pre_l = out_l;
                    let out_r = f2_pre_r - self.lowcut_states_r2[i];
                    self.lowcut_states_r2[i] += coeff * out_r;
                    f2_pre_r = out_r;
                }
                (f1_out_l, f1_out_r, f2_pre_l, f2_pre_r)
            } else {
                let pre_filter_l = char_out_l * pfg;
                let pre_filter_r = char_out_r * pfg;
                let lowcut_hz = self.params.lowcut_hz.clamp(20.0, 20000.0);
                let slope = self.params.lowcut_slope.clamp(1, 4) as usize;
                let coeff = (std::f32::consts::PI * lowcut_hz / self.sample_rate).sin() * 2.0;
                let coeff = coeff.min(1.0);
                let mut pre_filter_l = pre_filter_l;
                let mut pre_filter_r = pre_filter_r;
                for i in 0..slope {
                    let out_l = pre_filter_l - self.lowcut_states_l[i];
                    self.lowcut_states_l[i] += coeff * out_l;
                    pre_filter_l = out_l;
                    let out_r = pre_filter_r - self.lowcut_states_r[i];
                    self.lowcut_states_r[i] += coeff * out_r;
                    pre_filter_r = out_r;
                }
                (pre_filter_l, pre_filter_r, pre_filter_l, pre_filter_r)
            };

            // Global filter block feedback
            let fb = self.params.filter_feedback.clamp(-1.0, 1.0);
            let fb_amount = fb.abs();
            let pre_filter_l = pre_filter_l + self.filter_feedback_prev_l * fb_amount;
            let pre_filter_r = pre_filter_r + self.filter_feedback_prev_r * fb_amount;

            // Compute filter parameters
            let f1_enabled = self.params.filter1.enabled;
            let f2_enabled = self.params.filter2.enabled;

            let _f1_cutoff = if f1_enabled {
                let base = self.params.filter1.cutoff_hz;
                let eg_mod = self.filter_eg_output
                    * (self.params.filter1.eg_amount + mods.f1_eg_amount)
                    * 10000.0;
                let has_lfo1_f1 = self.params.modulations.iter().any(|r| {
                    r.active && r.source == ModSource::Lfo1 && r.target == ModTarget::Filter1Cutoff
                });
                let lfo_mod = if has_lfo1_f1 {
                    0.0
                } else {
                    self.lfo1_output * 5000.0
                };
                let key_track = self.params.filter1.key_tracking * (self.note as f32 - 60.0) * 50.0;
                (base + eg_mod + lfo_mod + key_track + mods.f1_cutoff).clamp(20.0, 20000.0)
            } else {
                20000.0
            };
            let _f1_res = if f1_enabled {
                (self.params.filter1.resonance + mods.f1_resonance).clamp(0.01, 10.0)
            } else {
                0.7
            };
            let f1_drive = if f1_enabled {
                (self.params.filter1.drive + mods.f1_drive).clamp(0.0, 1.0)
            } else {
                0.0
            };

            let _f2_cutoff = if f2_enabled {
                let base = if self.params.f2_cutoff_offset {
                    _f1_cutoff * (self.params.filter2.cutoff_hz / 10000.0).clamp(0.2, 5.0)
                } else {
                    self.params.filter2.cutoff_hz
                };
                let eg_mod = self.filter_eg_output
                    * (self.params.filter2.eg_amount + mods.f2_eg_amount)
                    * 10000.0;
                let has_lfo2_f2 = self.params.modulations.iter().any(|r| {
                    r.active && r.source == ModSource::Lfo2 && r.target == ModTarget::Filter2Cutoff
                });
                let lfo_mod = if has_lfo2_f2 {
                    0.0
                } else {
                    self.lfo2_output * 5000.0
                };
                let key_track = self.params.filter2.key_tracking * (self.note as f32 - 60.0) * 50.0;
                (base + eg_mod + lfo_mod + key_track + mods.f2_cutoff).clamp(20.0, 20000.0)
            } else {
                20000.0
            };
            let _f2_res = if f2_enabled {
                if self.params.f2_res_link {
                    _f1_res
                } else {
                    (self.params.filter2.resonance + mods.f2_resonance).clamp(0.01, 10.0)
                }
            } else {
                0.7
            };
            let f2_drive = if f2_enabled {
                (self.params.filter2.drive + mods.f2_drive).clamp(0.0, 1.0)
            } else {
                0.0
            };

            let ws_active =
                self.params.waveshaper.enabled && self.params.waveshaper.shape != Waveshape::Off;
            let ws_drive = if ws_active {
                (self.params.waveshaper.drive + mods.waveshaper_drive).clamp(0.0, 1.0)
            } else {
                0.0
            };

            // Apply filter routing
            let (mut char_l, mut char_r, f1_out_l, f1_out_r) = if per_source_routing {
                // Per-source routing: F1 bus → F1 filter, F2 bus → F2 filter, then sum → WS
                let mut f1_l = pre_filter_l;
                let mut f1_r = pre_filter_r;
                let mut f2_l = f2_pre_l;
                let mut f2_r = f2_pre_r;
                if f1_enabled {
                    self.filter1_l.set_drive(f1_drive);
                    self.filter1_r.set_drive(f1_drive);
                    f1_l = self.filter1_l.process(f1_l);
                    f1_r = self.filter1_r.process(f1_r);
                }
                if f2_enabled {
                    self.filter2_l.set_drive(f2_drive);
                    self.filter2_r.set_drive(f2_drive);
                    f2_l = self.filter2_l.process(f2_l);
                    f2_r = self.filter2_r.process(f2_r);
                }
                let sum_l = f1_l + f2_l;
                let sum_r = f1_r + f2_r;
                let (ws_l, ws_r) = if ws_active {
                    let mut ws = self.waveshaper.clone();
                    ws.drive = ws_drive;
                    (ws.process(sum_l), ws.process(sum_r))
                } else {
                    (sum_l, sum_r)
                };
                (ws_l, ws_r, f1_l, f1_r)
            } else {
                match self.params.filter_routing {
                    FilterRouting::Series => {
                        // S1: F1 → WS → F2
                        let mut s_l = pre_filter_l;
                        let mut s_r = pre_filter_r;
                        if f1_enabled {
                            self.filter1_l.set_drive(f1_drive);
                            self.filter1_r.set_drive(f1_drive);
                            s_l = self.filter1_l.process(s_l);
                            s_r = self.filter1_r.process(s_r);
                        }
                        let (ws_l, ws_r) = if ws_active {
                            let mut ws = self.waveshaper.clone();
                            ws.drive = ws_drive;
                            (ws.process(s_l), ws.process(s_r))
                        } else {
                            (s_l, s_r)
                        };
                        let mut out_l = ws_l;
                        let mut out_r = ws_r;
                        if f2_enabled {
                            self.filter2_l.set_drive(f2_drive);
                            self.filter2_r.set_drive(f2_drive);
                            out_l = self.filter2_l.process(out_l);
                            out_r = self.filter2_r.process(out_r);
                        }
                        (out_l, out_r, ws_l, ws_r)
                    }
                    FilterRouting::Parallel => {
                        // D1: F1 || F2 → sum → WS
                        let mut f1_l = pre_filter_l;
                        let mut f1_r = pre_filter_r;
                        let mut f2_l = pre_filter_l;
                        let mut f2_r = pre_filter_r;
                        if f1_enabled {
                            self.filter1_l.set_drive(f1_drive);
                            self.filter1_r.set_drive(f1_drive);
                            f1_l = self.filter1_l.process(f1_l);
                            f1_r = self.filter1_r.process(f1_r);
                        }
                        if f2_enabled {
                            self.filter2_l.set_drive(f2_drive);
                            self.filter2_r.set_drive(f2_drive);
                            f2_l = self.filter2_l.process(f2_l);
                            f2_r = self.filter2_r.process(f2_r);
                        }
                        let sum_l = (f1_l + f2_l) * 0.5;
                        let sum_r = (f1_r + f2_r) * 0.5;
                        let (ws_l, ws_r) = if ws_active {
                            let mut ws = self.waveshaper.clone();
                            ws.drive = ws_drive;
                            (ws.process(sum_l), ws.process(sum_r))
                        } else {
                            (sum_l, sum_r)
                        };
                        (ws_l, ws_r, f1_l, f1_r)
                    }
                    FilterRouting::Wide => {
                        // Wide: S2 doubled — F2 → WS → F1 on both channels
                        let mut s_l = pre_filter_l;
                        let mut s_r = pre_filter_r;
                        if f2_enabled {
                            self.filter2_l.set_drive(f2_drive);
                            self.filter2_r.set_drive(f2_drive);
                            s_l = self.filter2_l.process(s_l);
                            s_r = self.filter2_r.process(s_r);
                        }
                        let (ws_l, ws_r) = if ws_active {
                            let mut ws = self.waveshaper.clone();
                            ws.drive = ws_drive;
                            (ws.process(s_l), ws.process(s_r))
                        } else {
                            (s_l, s_r)
                        };
                        let mut out_l = ws_l;
                        let mut out_r = ws_r;
                        if f1_enabled {
                            self.filter1_l.set_drive(f1_drive);
                            self.filter1_r.set_drive(f1_drive);
                            out_l = self.filter1_l.process(out_l);
                            out_r = self.filter1_r.process(out_r);
                        }
                        (out_l, out_r, ws_l, ws_r)
                    }
                    FilterRouting::Split => {
                        // Stereo: L→F1, R→F2 → WS
                        let mut f1_l = pre_filter_l;
                        let f1_r = pre_filter_r;
                        let f2_l = pre_filter_l;
                        let mut f2_r = pre_filter_r;
                        if f1_enabled {
                            self.filter1_l.set_drive(f1_drive);
                            self.filter1_r.set_drive(f1_drive);
                            f1_l = self.filter1_l.process(f1_l);
                            self.filter1_r.process(f1_r);
                        }
                        if f2_enabled {
                            self.filter2_l.set_drive(f2_drive);
                            self.filter2_r.set_drive(f2_drive);
                            self.filter2_l.process(f2_l);
                            f2_r = self.filter2_r.process(f2_r);
                        }
                        let stereo_l = f1_l;
                        let stereo_r = f2_r;
                        let (ws_l, ws_r) = if ws_active {
                            let mut ws = self.waveshaper.clone();
                            ws.drive = ws_drive;
                            (ws.process(stereo_l), ws.process(stereo_r))
                        } else {
                            (stereo_l, stereo_r)
                        };
                        (ws_l, ws_r, stereo_l, stereo_r)
                    }
                    FilterRouting::Serial2 => {
                        // F2 → WS → F1
                        let mut s_l = pre_filter_l;
                        let mut s_r = pre_filter_r;
                        if f2_enabled {
                            self.filter2_l.set_drive(f2_drive);
                            self.filter2_r.set_drive(f2_drive);
                            s_l = self.filter2_l.process(s_l);
                            s_r = self.filter2_r.process(s_r);
                        }
                        let (ws_l, ws_r) = if ws_active {
                            let mut ws = self.waveshaper.clone();
                            ws.drive = ws_drive;
                            (ws.process(s_l), ws.process(s_r))
                        } else {
                            (s_l, s_r)
                        };
                        let mut out_l = ws_l;
                        let mut out_r = ws_r;
                        if f1_enabled {
                            self.filter1_l.set_drive(f1_drive);
                            self.filter1_r.set_drive(f1_drive);
                            out_l = self.filter1_l.process(out_l);
                            out_r = self.filter1_r.process(out_r);
                        }
                        (out_l, out_r, ws_l, ws_r)
                    }
                    FilterRouting::Serial3 => {
                        // S3 approximation: F1→WS in series with F2 in parallel
                        let mut s_l = pre_filter_l;
                        let mut s_r = pre_filter_r;
                        if f1_enabled {
                            self.filter1_l.set_drive(f1_drive);
                            self.filter1_r.set_drive(f1_drive);
                            s_l = self.filter1_l.process(s_l);
                            s_r = self.filter1_r.process(s_r);
                        }
                        let (ws_l, ws_r) = if ws_active {
                            let mut ws = self.waveshaper.clone();
                            ws.drive = ws_drive;
                            (ws.process(s_l), ws.process(s_r))
                        } else {
                            (s_l, s_r)
                        };
                        let mut f2_l = pre_filter_l;
                        let mut f2_r = pre_filter_r;
                        if f2_enabled {
                            self.filter2_l.set_drive(f2_drive);
                            self.filter2_r.set_drive(f2_drive);
                            f2_l = self.filter2_l.process(f2_l);
                            f2_r = self.filter2_r.process(f2_r);
                        }
                        let out_l = ws_l * 0.7 + f2_l * 0.3;
                        let out_r = ws_r * 0.7 + f2_r * 0.3;
                        (out_l, out_r, ws_l, ws_r)
                    }
                    FilterRouting::Dual2 => {
                        // F1(WS) || F2 → sum
                        let mut f1_l = pre_filter_l;
                        let mut f1_r = pre_filter_r;
                        let mut f2_l = pre_filter_l;
                        let mut f2_r = pre_filter_r;
                        if f1_enabled {
                            self.filter1_l.set_drive(f1_drive);
                            self.filter1_r.set_drive(f1_drive);
                            f1_l = self.filter1_l.process(f1_l);
                            f1_r = self.filter1_r.process(f1_r);
                        }
                        let (f1_ws_l, f1_ws_r) = if ws_active {
                            let mut ws = self.waveshaper.clone();
                            ws.drive = ws_drive;
                            (ws.process(f1_l), ws.process(f1_r))
                        } else {
                            (f1_l, f1_r)
                        };
                        if f2_enabled {
                            self.filter2_l.set_drive(f2_drive);
                            self.filter2_r.set_drive(f2_drive);
                            f2_l = self.filter2_l.process(f2_l);
                            f2_r = self.filter2_r.process(f2_r);
                        }
                        let out_l = (f1_ws_l + f2_l) * 0.5;
                        let out_r = (f1_ws_r + f2_r) * 0.5;
                        (out_l, out_r, f1_ws_l, f1_ws_r)
                    }
                    FilterRouting::Ring => {
                        // F1 × F2 → WS
                        let mut f1_l = pre_filter_l;
                        let mut f1_r = pre_filter_r;
                        let mut f2_l = pre_filter_l;
                        let mut f2_r = pre_filter_r;
                        if f1_enabled {
                            self.filter1_l.set_drive(f1_drive);
                            self.filter1_r.set_drive(f1_drive);
                            f1_l = self.filter1_l.process(f1_l);
                            f1_r = self.filter1_r.process(f1_r);
                        }
                        if f2_enabled {
                            self.filter2_l.set_drive(f2_drive);
                            self.filter2_r.set_drive(f2_drive);
                            f2_l = self.filter2_l.process(f2_l);
                            f2_r = self.filter2_r.process(f2_r);
                        }
                        let ring_l = f1_l * f2_l;
                        let ring_r = f1_r * f2_r;
                        let (ws_l, ws_r) = if ws_active {
                            let mut ws = self.waveshaper.clone();
                            ws.drive = ws_drive;
                            (ws.process(ring_l), ws.process(ring_r))
                        } else {
                            (ring_l, ring_r)
                        };
                        (ws_l, ws_r, f1_l, f1_r)
                    }
                }
            };

            // Apply filter balance after routing
            let balance = (self.params.filter_balance + mods.filter_balance).clamp(-1.0, 1.0);
            let f2_mix = (balance + 1.0) * 0.5; // -1 -> 0, 0 -> 0.5, 1 -> 1
            let f1_mix = 1.0 - f2_mix;
            char_l = f1_out_l * f1_mix + char_l * f2_mix;
            char_r = f1_out_r * f1_mix + char_r * f2_mix;

            // Update filter feedback state (before VCA)
            self.filter_feedback_prev_l = char_l;
            self.filter_feedback_prev_r = char_r;

            // Apply amplitude envelope, velocity, and volume
            let vol = (self.params.volume + mods.output_volume).clamp(0.0, 2.0);
            let vca_level = self.params.vca_level.clamp(0.0, 2.0);
            let vel_sense = self.params.vca_velsense.clamp(0.0, 1.0);
            let effective_vel = 1.0 - vel_sense + vel_sense * self.velocity;
            char_l *= self.amp_eg_output * effective_vel * vol * vca_level;
            char_r *= self.amp_eg_output * effective_vel * vol * vca_level;

            // Pan and width
            let pan = (self.params.pan + mods.output_pan).clamp(-1.0, 1.0);
            let width = (self.params.width + mods.output_width).clamp(-1.0, 1.0);

            let pan_l = (1.0 - pan) * 0.5f32.sqrt();
            let pan_r = (1.0 + pan) * 0.5f32.sqrt();

            let mid = (char_l + char_r) * 0.5;
            let side = (char_l - char_r) * 0.5 * (1.0 + width);

            out_l[i] = (mid + side) * pan_l;
            out_r[i] = (mid - side) * pan_r;

            self.sample_counter += 1;
        }

        if !self.amp_eg.is_active() && !self.gate {
            self.active = false;
        }
    }

    pub fn retarget_note(&mut self, note: u8) {
        self.note = note;
        self.target_freq = note_to_freq(note, &self.tuning, &self.mts_esp);
        self.update_oscillator_freqs(&ModValues::default());
    }

    pub fn update_oscillator_freqs(&mut self, mods: &ModValues) {
        // Smooth pitch bend
        if self.params.pitch_bend_smooth > 0.0 {
            let g = self.params.pitch_bend_smooth * 0.1; // smoothing coefficient
            self.pitch_bend_smooth_state += (self.pitch_bend - self.pitch_bend_smooth_state) * g;
        } else {
            self.pitch_bend_smooth_state = self.pitch_bend;
        }

        let pitch_eg_mod = self.pitch_eg_output * 2.0; // ±2 octaves
        let pb_range = if self.pitch_bend_smooth_state >= 0.0 {
            if self.params.pitch_bend_up > 0.0 {
                self.params.pitch_bend_up
            } else {
                self.params.pitch_bend_range
            }
        } else {
            if self.params.pitch_bend_down > 0.0 {
                self.params.pitch_bend_down
            } else {
                self.params.pitch_bend_range
            }
        };
        let pb_mul = 2.0f32.powf(self.pitch_bend_smooth_state * pb_range / 12.0);
        let current_freq = self.current_freq;

        // Update drift
        let drift_amount = self.params.drift_amount;
        for idx in 0..3 {
            if drift_amount > 0.0 {
                self.drift_smooth[idx] -= 1.0 / (self.sample_rate * 0.1);
                if self.drift_smooth[idx] <= 0.0 {
                    self.drift_smooth[idx] = 1.0;
                    self.drift_target[idx] = random::<f32>() * 2.0 - 1.0;
                }
                let step = 1.0 / (self.sample_rate * 0.05);
                self.drift_phase[idx] += (self.drift_target[idx] - self.drift_phase[idx]) * step;
            } else {
                self.drift_phase[idx] = 0.0;
            }
        }

        for idx in 0..3 {
            let settings = &self.params.oscs[idx];
            let base_freq = current_freq;
            let octave_mul = 2.0f32.powi(settings.octave as i32);
            let semitone_mul = 2.0f32.powf(settings.semitone as f32 / 12.0);
            let fine_mul = 2.0f32.powf(settings.fine / 1200.0);
            let pitch_mod_mul = 2.0f32.powf(mods.osc_pitch[idx] + pitch_eg_mod);
            let drift_cents = self.drift_phase[idx] * drift_amount * 20.0; // up to 20 cents drift
            let drift_mul = 2.0f32.powf(drift_cents / 1200.0);

            let freq = base_freq
                * octave_mul
                * semitone_mul
                * fine_mul
                * pb_mul
                * pitch_mod_mul
                * drift_mul;
            self.oscillators[idx].set_freq_hz(freq);

            // Apply shape/skew/formant modulation
            let shape = (settings.shape + mods.osc_shape[idx]).clamp(0.0, 1.0);
            let skew = (settings.skew + mods.osc_skew[idx]).clamp(-1.0, 1.0);
            let formant = (settings.formant + mods.osc_formant[idx]).clamp(0.25, 4.0);
            self.oscillators[idx].set_shape(shape);
            self.oscillators[idx].set_skew(skew);
            self.oscillators[idx].set_formant(formant);
        }
    }
}

#[inline]
fn note_to_freq(note: u8, tuning: &Tuning, mts_esp: &Option<Arc<Mutex<MtsEspClient>>>) -> f32 {
    if let Some(client) = mts_esp
        && let Some(freq) = client.lock().note_to_frequency(note, 0)
    {
        return freq;
    }
    tuning.note_to_freq(note)
}

/// Find the nearest scale degree index for a given frequency and tuning.
#[inline]
fn freq_to_scale_degree(freq: f32, tuning: &Tuning) -> i32 {
    let root_freq = 440.0 * 2.0f32.powf((tuning.root_midi_note as f32 - 69.0) / 12.0);
    let cents = 1200.0 * (freq / root_freq.max(1e-6)).log2();
    let octave_cents = tuning.octave_cents();
    let scale_len = tuning.num_degrees();
    if scale_len == 0 || octave_cents <= 0.0 {
        return 0;
    }
    let octave = (cents / octave_cents).floor() as i32;
    let cents_in_octave = cents - octave as f32 * octave_cents;
    let mut best_idx = 0usize;
    let mut best_diff = cents_in_octave.abs();
    for i in 1..tuning.degrees.len() {
        let diff = (cents_in_octave - tuning.degrees[i]).abs();
        if diff < best_diff {
            best_diff = diff;
            best_idx = i;
        }
    }
    octave * scale_len as i32 + best_idx as i32
}

#[inline]
fn freq_to_note(freq: f32) -> f32 {
    69.0 + 12.0 * (freq / 440.0).log2()
}
