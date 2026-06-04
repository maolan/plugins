#![allow(dead_code)]

//! Oscillator implementations inspired by Surge XT.
//!
//! - Classic: Saw, Square, Pulse, Triangle using polyBLEP
//! - Sine: Pure sine with FM
//! - FM2: 2-operator FM synthesis
//! - Wavetable: Direct lookup with frame morphing and mipmaps
//! - Window: Windowed wavetable lookup

use std::f32::consts::PI;

use super::{Filter, FilterType, TwistOsc};
use crate::common::wavetable::Wavetable;

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

#[inline]
pub fn note_to_freq(midi_note: f32) -> f32 {
    440.0 * 2.0f32.powf((midi_note - 69.0) / 12.0)
}

#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return 0.0;
    }
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[inline]
fn soft_clip(x: f32) -> f32 {
    x.clamp(-1.0, 1.0)
}

// ---------------------------------------------------------------------------
// Unison voice
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct UnisonVoice {
    pub phase: f32,
    pub phase_inc: f32,
    pub pan: f32,
}

impl UnisonVoice {
    pub fn new(phase: f32, phase_inc: f32, pan: f32) -> Self {
        Self {
            phase,
            phase_inc,
            pan,
        }
    }
}

// ---------------------------------------------------------------------------
// Oscillator types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OscType {
    Classic = 0,
    Sine = 1,
    Fm2 = 2,
    Wavetable = 3,
    Window = 4,
    Modern = 5,
    ShNoise = 6,
    String = 7,
    Fm3 = 8,
    Alias = 9,
    Twist = 10,
    AudioInput = 11,
}

impl OscType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => OscType::Classic,
            1 => OscType::Sine,
            2 => OscType::Fm2,
            3 => OscType::Wavetable,
            4 => OscType::Window,
            5 => OscType::Modern,
            6 => OscType::ShNoise,
            7 => OscType::String,
            8 => OscType::Fm3,
            9 => OscType::Alias,
            10 => OscType::Twist,
            _ => OscType::AudioInput,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassicWaveform {
    Saw = 0,
    Square = 1,
    Pulse = 2,
    Triangle = 3,
}

impl ClassicWaveform {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => ClassicWaveform::Saw,
            1 => ClassicWaveform::Square,
            2 => ClassicWaveform::Pulse,
            _ => ClassicWaveform::Triangle,
        }
    }

    pub fn next(self) -> Self {
        match self {
            ClassicWaveform::Saw => ClassicWaveform::Square,
            ClassicWaveform::Square => ClassicWaveform::Pulse,
            ClassicWaveform::Pulse => ClassicWaveform::Triangle,
            ClassicWaveform::Triangle => ClassicWaveform::Saw,
        }
    }
}

// ---------------------------------------------------------------------------
// Classic Oscillator (polyBLEP-based)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ClassicOsc {
    sample_rate: f32,
    freq_hz: f32,
    waveform: ClassicWaveform,
    pulse_width: f32,
    sub_level: f32,
    sub_octave: i8,
    sub_phase: f32,
    waveform_morph: f32,
    width2: f32,
    unison_voices: usize,
    unison_detune: f32,
    unison_spread: f32,
    voices: Vec<UnisonVoice>,
    sync_amount: f32,
    sync_phases: Vec<f32>,
}

impl ClassicOsc {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            freq_hz: 440.0,
            waveform: ClassicWaveform::Saw,
            pulse_width: 0.5,
            sub_level: 0.0,
            sub_octave: -1,
            sub_phase: 0.0,
            waveform_morph: 0.0,
            width2: 0.5,
            unison_voices: 1,
            unison_detune: 0.0,
            unison_spread: 1.0,
            voices: vec![UnisonVoice::new(0.0, 0.0, 0.5)],
            sync_amount: 0.0,
            sync_phases: vec![0.0],
        }
    }

    pub fn set_sync_amount(&mut self, amount: f32) {
        self.sync_amount = amount.clamp(0.0, 1.0);
    }

    pub fn set_freq_hz(&mut self, freq: f32) {
        self.freq_hz = freq.max(0.1);
        self.update_voices();
    }

    pub fn set_waveform(&mut self, wf: ClassicWaveform) {
        self.waveform = wf;
    }

    pub fn set_pulse_width(&mut self, pw: f32) {
        self.pulse_width = pw.clamp(0.01, 0.99);
    }

    pub fn set_sub_level(&mut self, level: f32) {
        self.sub_level = level.clamp(0.0, 1.0);
    }

    pub fn set_sub_octave(&mut self, oct: i8) {
        self.sub_octave = match oct {
            1 => -2,
            _ => -1,
        };
    }

    pub fn set_waveform_morph(&mut self, morph: f32) {
        self.waveform_morph = morph.clamp(0.0, 1.0);
    }

    pub fn set_width2(&mut self, w: f32) {
        self.width2 = w.clamp(0.01, 0.99);
    }

    pub fn set_unison(&mut self, voices: usize, detune: f32) {
        self.unison_voices = voices.clamp(1, 16);
        self.unison_detune = detune.clamp(0.0, 1.0);
        self.voices.resize_with(self.unison_voices, || {
            UnisonVoice::new(rand::random(), 0.0, 0.5)
        });
        self.sync_phases
            .resize_with(self.unison_voices, rand::random);
        self.update_voices();
    }

    pub fn set_unison_spread(&mut self, spread: f32) {
        self.unison_spread = spread.clamp(0.0, 1.0);
        self.update_voices();
    }

    fn update_voices(&mut self) {
        let base_inc = self.freq_hz / self.sample_rate;
        let detune_scale = self.unison_detune * 0.05; // semitones max
        let n = self.unison_voices;
        for (i, voice) in self.voices.iter_mut().enumerate() {
            let offset = if n > 1 {
                (i as f32 / (n as f32 - 1.0) - 0.5) * 2.0 * detune_scale
            } else {
                0.0
            };
            voice.phase_inc = base_inc * 2.0f32.powf(offset / 12.0);
            voice.pan = if n > 1 {
                i as f32 / (n as f32 - 1.0)
            } else {
                0.5
            };
        }
    }

    pub fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.phase = rand::random();
        }
        for sp in &mut self.sync_phases {
            *sp = rand::random();
        }
    }

    pub fn reset_to_zero(&mut self) {
        for voice in &mut self.voices {
            voice.phase = 0.0;
        }
        for sp in &mut self.sync_phases {
            *sp = 0.0;
        }
    }

    fn generate_waveform(wf: ClassicWaveform, pulse_width: f32, t: f32, vdt: f32) -> f32 {
        match wf {
            ClassicWaveform::Saw => {
                let mut v = 2.0 * t - 1.0;
                v -= poly_blep(t, vdt);
                v
            }
            ClassicWaveform::Square => {
                let mut v = if t < 0.5 { 1.0 } else { -1.0 };
                v += poly_blep(t, vdt);
                v -= poly_blep((t + 0.5) % 1.0, vdt);
                v
            }
            ClassicWaveform::Pulse => {
                let pw = pulse_width;
                let mut v = if t < pw { 1.0 } else { -1.0 };
                v += poly_blep(t, vdt);
                v -= poly_blep((t + 1.0 - pw) % 1.0, vdt);
                v
            }
            ClassicWaveform::Triangle => {
                if vdt > 0.25 {
                    (t * 2.0 * PI).sin()
                } else {
                    let out = (2.0 * (t + 0.25)).fract() * 2.0 - 1.0;
                    out.abs() * 2.0 - 1.0
                }
            }
        }
    }

    pub fn next(&mut self, fm_input: f32) -> (f32, f32) {
        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;
        let _dt = self.freq_hz / self.sample_rate;
        let fm_shift = fm_input * 0.05;
        let morph = self.waveform_morph;
        let current_wf = self.waveform;
        let next_wf = self.waveform.next();
        let pulse_width = self.pulse_width;

        for (i, voice) in self.voices.iter_mut().enumerate() {
            let t = (voice.phase + fm_shift).fract();
            let t = if t < 0.0 { t + 1.0 } else { t };
            let vdt = voice.phase_inc;

            let current = Self::generate_waveform(current_wf, pulse_width, t, vdt);
            let next = Self::generate_waveform(next_wf, pulse_width, t, vdt);
            let mut out = current * (1.0 - morph) + next * morph;

            // Hard sync: advance sync phase at higher frequency, reset voice phase on wrap
            if self.sync_amount > 0.0 {
                let sync_ratio = 1.0 + self.sync_amount * 15.0;
                let sync_inc = vdt * sync_ratio;
                self.sync_phases[i] += sync_inc;
                while self.sync_phases[i] >= 1.0 {
                    self.sync_phases[i] -= 1.0;
                    voice.phase = 0.0;
                }
            }

            voice.phase += vdt;
            while voice.phase >= 1.0 {
                voice.phase -= 1.0;
            }

            out = soft_clip(out);
            let pan = voice.pan;
            sum_l += out * (1.0 - pan);
            sum_r += out * pan;
        }

        // Sub-oscillator: octave-down square wave
        if self.sub_level > 0.0 {
            let sub_ratio = 2.0f32.powi(self.sub_octave as i32); // -1 -> 0.5, -2 -> 0.25
            let sub_inc = self.freq_hz * sub_ratio / self.sample_rate;
            self.sub_phase += sub_inc;
            while self.sub_phase >= 1.0 {
                self.sub_phase -= 1.0;
            }
            let sub_out = if self.sub_phase < self.width2 {
                1.0
            } else {
                -1.0
            };
            sum_l += sub_out * self.sub_level;
            sum_r += sub_out * self.sub_level;
        }

        let atten = 1.0 / (self.unison_voices as f32).sqrt();
        (sum_l * atten, sum_r * atten)
    }
}

// ---------------------------------------------------------------------------
// Sine Oscillator
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SineShaperMode {
    // -----------------------------------------------------------------------
    // Original 9 shapes (indices 0–8 preserved for backward compatibility)
    // -----------------------------------------------------------------------
    Off = 0,
    HalfRect = 1,
    FullRect = 2,
    Squared = 3,
    Cubed = 4,
    Abs = 5,
    SoftClip = 6,
    Fold = 7,
    Wrap = 8,

    // -----------------------------------------------------------------------
    // Surge XT sine wave shapes 1–23 (indices 9–31)
    // Based on quadrant transforms of sin/cos pairs.
    // -----------------------------------------------------------------------
    /// sign(sin)·sin²
    SignSin2 = 9,
    /// sin² positive half-wave
    Sin2PosHalf = 10,
    /// sin(2x) positive half-wave
    Sin2xPosHalf = 11,
    /// Mode1(2x) positive half-wave = sign(sin2x)·sin²(2x) positive half
    Mode1_2xPos = 12,
    /// |sin(2x)| positive half-wave
    AbsSin2xPos = 13,
    /// sin²(2x) positive half-wave
    Sin2_2xPos = 14,
    /// 2·sin−1 positive half, −1 otherwise
    TwoSinMinus1Pos = 15,
    /// sin in Q2+Q4 only
    SinQ24 = 16,
    /// sin in Q1+Q3 only
    SinQ13 = 17,
    /// 2·sin²−1 positive half, −1 otherwise
    TwoSin2Minus1Pos = 18,
    /// sin(2x)·sign(cos)
    Sin2xSignCos = 19,
    /// sin(2x) in Q1+Q3, 0 in Q2+Q4
    Sin2xQ13 = 20,
    /// |cos(2x)| positive half
    AbsCos2xPos = 21,
    /// 1−sin in Q1, −1−sin in Q4, 0 in Q2+Q3
    OneMinusSinQ14 = 22,
    /// 1−sin in Q1, cos−1 in Q4, 0 in Q2+Q3
    OneMinusSinCosQ14 = 23,
    /// 1−sin in Q1+Q2, −1−sin in Q3+Q4
    OneMinusSinQ12 = 24,
    /// sin(2x) in Q1, cos in Q2, −sin(2x) in Q3, sin(2x) in Q4
    MixQ = 25,
    /// sin(2x) in Q1, −sin(4x) in Q2, sin in Q3+Q4
    Mix2 = 26,
    /// sin in Q1+Q3, 1 in Q2, −1 in Q4
    SinQ13Sat = 27,
    /// 1 in Q1, sin in Q2+Q4, −1 in Q3
    SinQ24Sat = 28,
    /// sin where cos≥0 (Q1+Q4)
    SinCosPos = 29,
    /// sin where cos≤0 (Q2+Q3)
    SinCosNeg = 30,
    /// 1−sin in Q1+Q2, sin in Q3+Q4
    OneMinusSinMix = 31,
}

impl SineShaperMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => SineShaperMode::Off,
            1 => SineShaperMode::HalfRect,
            2 => SineShaperMode::FullRect,
            3 => SineShaperMode::Squared,
            4 => SineShaperMode::Cubed,
            5 => SineShaperMode::Abs,
            6 => SineShaperMode::SoftClip,
            7 => SineShaperMode::Fold,
            8 => SineShaperMode::Wrap,
            9 => SineShaperMode::SignSin2,
            10 => SineShaperMode::Sin2PosHalf,
            11 => SineShaperMode::Sin2xPosHalf,
            12 => SineShaperMode::Mode1_2xPos,
            13 => SineShaperMode::AbsSin2xPos,
            14 => SineShaperMode::Sin2_2xPos,
            15 => SineShaperMode::TwoSinMinus1Pos,
            16 => SineShaperMode::SinQ24,
            17 => SineShaperMode::SinQ13,
            18 => SineShaperMode::TwoSin2Minus1Pos,
            19 => SineShaperMode::Sin2xSignCos,
            20 => SineShaperMode::Sin2xQ13,
            21 => SineShaperMode::AbsCos2xPos,
            22 => SineShaperMode::OneMinusSinQ14,
            23 => SineShaperMode::OneMinusSinCosQ14,
            24 => SineShaperMode::OneMinusSinQ12,
            25 => SineShaperMode::MixQ,
            26 => SineShaperMode::Mix2,
            27 => SineShaperMode::SinQ13Sat,
            28 => SineShaperMode::SinQ24Sat,
            29 => SineShaperMode::SinCosPos,
            30 => SineShaperMode::SinCosNeg,
            _ => SineShaperMode::OneMinusSinMix,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SineOsc {
    sample_rate: f32,
    phase: f32,
    freq_hz: f32,
    fm_amount: f32,
    pm_mode: bool,
    shaper_mode: SineShaperMode,
    feedback: f32,
    lowcut: Filter,
    highcut: Filter,
    unison_voices: usize,
    unison_detune: f32,
    voices: Vec<UnisonVoice>,
    voice_feedback: Vec<f32>,
}

impl SineOsc {
    pub fn new(sample_rate: f32) -> Self {
        let mut lowcut = Filter::new(FilterType::Highpass12dB, sample_rate);
        lowcut.set_params(20.0, 0.7);
        lowcut.prepare_block(20.0, 0.7, 1);
        let mut highcut = Filter::new(FilterType::Lowpass12dB, sample_rate);
        highcut.set_params(20000.0, 0.7);
        highcut.prepare_block(20000.0, 0.7, 1);
        Self {
            sample_rate,
            phase: 0.0,
            freq_hz: 440.0,
            fm_amount: 0.0,
            pm_mode: false,
            shaper_mode: SineShaperMode::Off,
            feedback: 0.0,
            lowcut,
            highcut,
            unison_voices: 1,
            unison_detune: 0.0,
            voices: vec![UnisonVoice::new(0.0, 0.0, 0.5)],
            voice_feedback: vec![0.0],
        }
    }

    pub fn set_freq_hz(&mut self, freq: f32) {
        self.freq_hz = freq.max(0.1);
        self.update_voices();
    }

    pub fn set_fm_amount(&mut self, amount: f32) {
        self.fm_amount = amount.clamp(0.0, 1.0);
    }

    pub fn set_pm_mode(&mut self, pm: bool) {
        self.pm_mode = pm;
    }

    pub fn set_shaper_mode(&mut self, mode: SineShaperMode) {
        self.shaper_mode = mode;
    }

    pub fn set_feedback(&mut self, feedback: f32) {
        self.feedback = feedback.clamp(0.0, 1.0);
    }

    pub fn set_lowcut(&mut self, freq: f32) {
        let f = freq.clamp(20.0, 20000.0);
        self.lowcut.set_params(f, 0.7);
        self.lowcut.prepare_block(f, 0.7, 1);
    }

    pub fn set_highcut(&mut self, freq: f32) {
        let f = freq.clamp(20.0, 20000.0);
        self.highcut.set_params(f, 0.7);
        self.highcut.prepare_block(f, 0.7, 1);
    }

    pub fn set_unison(&mut self, voices: usize, detune: f32) {
        self.unison_voices = voices.clamp(1, 16);
        self.unison_detune = detune.clamp(0.0, 1.0);
        self.voices.resize_with(self.unison_voices, || {
            UnisonVoice::new(rand::random(), 0.0, 0.5)
        });
        self.voice_feedback.resize(self.unison_voices, 0.0);
        self.update_voices();
    }

    fn update_voices(&mut self) {
        let base_inc = self.freq_hz / self.sample_rate;
        let detune_scale = self.unison_detune * 0.05;
        let n = self.unison_voices;
        for (i, voice) in self.voices.iter_mut().enumerate() {
            let offset = if n > 1 {
                (i as f32 / (n as f32 - 1.0) - 0.5) * 2.0 * detune_scale
            } else {
                0.0
            };
            voice.phase_inc = base_inc * 2.0f32.powf(offset / 12.0);
            voice.pan = if n > 1 {
                i as f32 / (n as f32 - 1.0)
            } else {
                0.5
            };
        }
    }

    pub fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.phase = rand::random();
        }
        for fb in &mut self.voice_feedback {
            *fb = 0.0;
        }
    }

    pub fn reset_to_zero(&mut self) {
        for voice in &mut self.voices {
            voice.phase = 0.0;
        }
        for fb in &mut self.voice_feedback {
            *fb = 0.0;
        }
    }

    pub fn next(&mut self, fm_input: f32) -> (f32, f32) {
        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;

        for (i, voice) in self.voices.iter_mut().enumerate() {
            let fm = fm_input * self.fm_amount * 10.0;
            let fb = self.voice_feedback[i] * self.feedback * 2.0 * PI;

            let out = if self.pm_mode {
                // Phase modulation: fm + feedback shift phase directly
                let phase = (voice.phase + fm + fb).fract();
                let phase = if phase < 0.0 { phase + 1.0 } else { phase };
                (phase * 2.0 * PI).sin()
            } else {
                // Frequency modulation: fm shifts increment, feedback shifts phase
                let modulated_inc = voice.phase_inc * (1.0 + fm);
                let fb_phase = (voice.phase + fb / (2.0 * PI)).fract();
                let fb_phase = if fb_phase < 0.0 {
                    fb_phase + 1.0
                } else {
                    fb_phase
                };
                let v = (fb_phase * 2.0 * PI).sin();
                voice.phase += modulated_inc;
                while voice.phase >= 1.0 {
                    voice.phase -= 1.0;
                }
                v
            };
            self.voice_feedback[i] = out;

            // Apply shaper (32 Surge XT sine wave shapes)
            let shaped = if self.shaper_mode == SineShaperMode::Off {
                out
            } else {
                // Compute sin, cos, and derived values for quadrant-based shapes
                let s = out; // out is already sin(phase)
                let c = (voice.phase * 2.0 * PI).cos();
                let s2 = 2.0 * s * c; // sin(2x)
                let c2 = c * c - s * s; // cos(2x)
                let s4 = 2.0 * s2 * c2; // sin(4x)
                let q1 = s >= 0.0 && c >= 0.0;
                let q2 = s >= 0.0 && c < 0.0;
                let q3 = s < 0.0 && c < 0.0;
                let q4 = s < 0.0 && c >= 0.0;

                match self.shaper_mode {
                    SineShaperMode::Off => out,
                    SineShaperMode::HalfRect => s.max(0.0),
                    SineShaperMode::FullRect => s.abs(),
                    SineShaperMode::Squared => s * s,
                    SineShaperMode::Cubed => s * s * s,
                    SineShaperMode::Abs => s.abs() * 2.0 - 1.0,
                    SineShaperMode::SoftClip => {
                        let x = s * 1.5;
                        x.clamp(-1.0, 1.0)
                    }
                    SineShaperMode::Fold => {
                        let x = s * 1.5;
                        (x + 1.0 - ((x + 1.0) * 0.5).fract() * 2.0).fract() * 2.0 - 1.0
                    }
                    SineShaperMode::Wrap => {
                        let x = s * 1.5;
                        (x + 1.0).fract() * 2.0 - 1.0
                    }
                    // sign(sin)·sin²
                    SineShaperMode::SignSin2 => s.abs() * s,
                    // sin² positive half-wave
                    SineShaperMode::Sin2PosHalf => {
                        if s >= 0.0 {
                            s * s
                        } else {
                            0.0
                        }
                    }
                    // sin(2x) positive half-wave
                    SineShaperMode::Sin2xPosHalf => {
                        if s >= 0.0 {
                            s2
                        } else {
                            0.0
                        }
                    }
                    // Mode1(2x) positive half = sign(sin2x)·sin²(2x) when sin≥0
                    SineShaperMode::Mode1_2xPos => {
                        if s >= 0.0 {
                            s2.abs() * s2
                        } else {
                            0.0
                        }
                    }
                    // |sin(2x)| positive half-wave
                    SineShaperMode::AbsSin2xPos => {
                        if s >= 0.0 {
                            s2.abs()
                        } else {
                            0.0
                        }
                    }
                    // sin²(2x) positive half-wave
                    SineShaperMode::Sin2_2xPos => {
                        if s >= 0.0 {
                            s2 * s2
                        } else {
                            0.0
                        }
                    }
                    // 2·sin−1 positive half, −1 otherwise
                    SineShaperMode::TwoSinMinus1Pos => {
                        if s >= 0.0 {
                            2.0 * s - 1.0
                        } else {
                            -1.0
                        }
                    }
                    // sin in Q2+Q4 only
                    SineShaperMode::SinQ24 => {
                        if q2 || q4 {
                            s
                        } else {
                            0.0
                        }
                    }
                    // sin in Q1+Q3 only
                    SineShaperMode::SinQ13 => {
                        if q1 || q3 {
                            s
                        } else {
                            0.0
                        }
                    }
                    // 2·sin²−1 positive half, −1 otherwise
                    SineShaperMode::TwoSin2Minus1Pos => {
                        if s >= 0.0 {
                            2.0 * s * s - 1.0
                        } else {
                            -1.0
                        }
                    }
                    // sin(2x)·sign(cos)
                    SineShaperMode::Sin2xSignCos => s2 * c.signum(),
                    // sin(2x) in Q1+Q3, 0 in Q2+Q4
                    SineShaperMode::Sin2xQ13 => {
                        if q1 || q3 {
                            s2
                        } else {
                            0.0
                        }
                    }
                    // |cos(2x)| positive half
                    SineShaperMode::AbsCos2xPos => {
                        if s >= 0.0 {
                            c2.abs()
                        } else {
                            0.0
                        }
                    }
                    // 1−sin in Q1, −1−sin in Q4, 0 in Q2+Q3
                    SineShaperMode::OneMinusSinQ14 => {
                        if q1 {
                            1.0 - s
                        } else if q4 {
                            -1.0 - s
                        } else {
                            0.0
                        }
                    }
                    // 1−sin in Q1, cos−1 in Q4, 0 in Q2+Q3
                    SineShaperMode::OneMinusSinCosQ14 => {
                        if q1 {
                            1.0 - s
                        } else if q4 {
                            c - 1.0
                        } else {
                            0.0
                        }
                    }
                    // 1−sin in Q1+Q2, −1−sin in Q3+Q4
                    SineShaperMode::OneMinusSinQ12 => {
                        if q1 || q2 {
                            1.0 - s
                        } else {
                            -1.0 - s
                        }
                    }
                    // sin(2x) in Q1, cos in Q2, −sin(2x) in Q3, sin(2x) in Q4
                    SineShaperMode::MixQ => {
                        if q1 {
                            s2
                        } else if q2 {
                            c
                        } else if q3 {
                            -s2
                        } else {
                            s2
                        }
                    }
                    // sin(2x) in Q1, −sin(4x) in Q2, sin in Q3+Q4
                    SineShaperMode::Mix2 => {
                        if q1 {
                            s2
                        } else if q2 {
                            -s4
                        } else {
                            s
                        }
                    }
                    // sin in Q1+Q3, 1 in Q2, −1 in Q4
                    SineShaperMode::SinQ13Sat => {
                        if q1 || q3 {
                            s
                        } else if q2 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                    // 1 in Q1, sin in Q2+Q4, −1 in Q3
                    SineShaperMode::SinQ24Sat => {
                        if q1 {
                            1.0
                        } else if q3 {
                            -1.0
                        } else {
                            s
                        }
                    }
                    // sin where cos≥0 (Q1+Q4)
                    SineShaperMode::SinCosPos => {
                        if c >= 0.0 {
                            s
                        } else {
                            0.0
                        }
                    }
                    // sin where cos≤0 (Q2+Q3)
                    SineShaperMode::SinCosNeg => {
                        if c <= 0.0 {
                            s
                        } else {
                            0.0
                        }
                    }
                    // 1−sin in Q1+Q2, sin in Q3+Q4
                    SineShaperMode::OneMinusSinMix => {
                        if q1 || q2 {
                            1.0 - s
                        } else {
                            s
                        }
                    }
                }
            };

            if self.pm_mode {
                voice.phase += voice.phase_inc;
                while voice.phase >= 1.0 {
                    voice.phase -= 1.0;
                }
            }

            let pan = voice.pan;
            sum_l += shaped * (1.0 - pan);
            sum_r += shaped * pan;
        }

        let atten = 1.0 / (self.unison_voices as f32).sqrt();
        let out_l = self.lowcut.process(sum_l * atten);
        let out_l = self.highcut.process(out_l);
        let out_r = self.lowcut.process(sum_r * atten);
        let out_r = self.highcut.process(out_r);
        (out_l, out_r)
    }
}

// ---------------------------------------------------------------------------
// FM2 Oscillator (2-operator FM)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fm2FeedbackMode {
    Classic = 0,
    Averaged = 1,
}

impl Fm2FeedbackMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Fm2FeedbackMode::Classic,
            _ => Fm2FeedbackMode::Averaged,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Fm2Osc {
    sample_rate: f32,
    carrier_phase: f32,
    modulator_phase: f32,
    freq_hz: f32,
    ratio: f32,
    depth: f32,
    feedback: f32,
    m12offset: f32,
    m12phase: f32,
    feedback_mode: Fm2FeedbackMode,
    unison_voices: usize,
    unison_detune: f32,
    voice_phases: Vec<(f32, f32)>,
    voice_feedback: Vec<f32>,
    voice_feedback_prev: Vec<f32>,
}

impl Fm2Osc {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            carrier_phase: 0.0,
            modulator_phase: 0.0,
            freq_hz: 440.0,
            ratio: 1.0,
            depth: 1.0,
            feedback: 0.0,
            m12offset: 0.0,
            m12phase: 0.0,
            feedback_mode: Fm2FeedbackMode::Classic,
            unison_voices: 1,
            unison_detune: 0.0,
            voice_phases: vec![(0.0, 0.0)],
            voice_feedback: vec![0.0],
            voice_feedback_prev: vec![0.0],
        }
    }

    pub fn set_freq_hz(&mut self, freq: f32) {
        self.freq_hz = freq.max(0.1);
    }

    pub fn set_ratio(&mut self, ratio: f32) {
        self.ratio = ratio.max(0.01);
    }

    pub fn set_depth(&mut self, depth: f32) {
        self.depth = depth;
    }

    pub fn set_feedback(&mut self, feedback: f32) {
        self.feedback = feedback.clamp(0.0, 1.0);
    }

    pub fn set_m12offset(&mut self, offset: f32) {
        self.m12offset = offset;
    }

    pub fn set_m12phase(&mut self, phase: f32) {
        self.m12phase = phase.clamp(0.0, 1.0);
    }

    pub fn set_feedback_mode(&mut self, mode: Fm2FeedbackMode) {
        self.feedback_mode = mode;
    }

    pub fn set_unison(&mut self, voices: usize, detune: f32) {
        self.unison_voices = voices.clamp(1, 16);
        self.unison_detune = detune.clamp(0.0, 1.0);
        self.voice_phases
            .resize_with(self.unison_voices, || (rand::random(), rand::random()));
        self.voice_feedback.resize(self.unison_voices, 0.0);
        self.voice_feedback_prev.resize(self.unison_voices, 0.0);
    }

    pub fn reset(&mut self) {
        self.carrier_phase = 0.0;
        self.modulator_phase = 0.0;
        for vp in &mut self.voice_phases {
            vp.0 = rand::random();
            vp.1 = rand::random();
        }
        for fb in &mut self.voice_feedback {
            *fb = 0.0;
        }
        for fb in &mut self.voice_feedback_prev {
            *fb = 0.0;
        }
    }

    pub fn reset_to_zero(&mut self) {
        self.carrier_phase = 0.0;
        self.modulator_phase = 0.0;
        for vp in &mut self.voice_phases {
            vp.0 = 0.0;
            vp.1 = 0.0;
        }
        for fb in &mut self.voice_feedback {
            *fb = 0.0;
        }
        for fb in &mut self.voice_feedback_prev {
            *fb = 0.0;
        }
    }

    pub fn next(&mut self) -> (f32, f32) {
        let mut sum = 0.0f32;
        let carrier_dt = self.freq_hz / self.sample_rate;
        let modulator_dt = carrier_dt * self.ratio + self.m12offset / self.sample_rate;
        let detune_scale = self.unison_detune * 0.05;
        let m12phase = self.m12phase * 2.0 * PI;

        for i in 0..self.unison_voices {
            let (cp, mp) = &mut self.voice_phases[i];
            let offset = if self.unison_voices > 1 {
                (i as f32 / (self.unison_voices as f32 - 1.0) - 0.5) * 2.0 * detune_scale
            } else {
                0.0
            };
            let detune_mul = 2.0f32.powf(offset / 12.0);

            let fb = match self.feedback_mode {
                Fm2FeedbackMode::Classic => self.voice_feedback[i] * self.feedback * 2.0 * PI,
                Fm2FeedbackMode::Averaged => {
                    (self.voice_feedback[i] + self.voice_feedback_prev[i])
                        * 0.5
                        * self.feedback
                        * 2.0
                        * PI
                }
            };
            let modulator = ((*mp * 2.0 * PI + m12phase + fb).sin()) * self.depth;
            let modulated_freq = carrier_dt * detune_mul * (1.0 + modulator);

            let out = (*cp * 2.0 * PI).sin();

            *cp += modulated_freq;
            *mp += modulator_dt * detune_mul;
            self.voice_feedback_prev[i] = self.voice_feedback[i];
            self.voice_feedback[i] = out;

            while *cp >= 1.0 {
                *cp -= 1.0;
            }
            while *mp >= 1.0 {
                *mp -= 1.0;
            }

            sum += out;
        }

        let atten = 1.0 / (self.unison_voices as f32).sqrt();
        (sum * atten, sum * atten)
    }
}

// ---------------------------------------------------------------------------
// FM3 Oscillator (3-operator FM)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fm3FeedbackMode {
    Classic = 0,
    Averaged = 1,
}

impl Fm3FeedbackMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Fm3FeedbackMode::Classic,
            _ => Fm3FeedbackMode::Averaged,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Fm3Osc {
    sample_rate: f32,
    phases: [f32; 3],
    freq_hz: f32,
    algorithm: u8,
    ratio2: f32,
    ratio3: f32,
    depth2: f32,
    depth3: f32,
    m3_abs_freq: f32,
    feedback: f32,
    feedback_mode: Fm3FeedbackMode,
    unison_voices: usize,
    unison_detune: f32,
    voice_phases: Vec<[f32; 3]>,
    voice_feedback: Vec<f32>,
    voice_feedback_prev: Vec<f32>,
}

impl Fm3Osc {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            phases: [0.0; 3],
            freq_hz: 440.0,
            algorithm: 0,
            ratio2: 1.0,
            ratio3: 1.0,
            depth2: 1.0,
            depth3: 1.0,
            m3_abs_freq: 0.0,
            feedback: 0.0,
            feedback_mode: Fm3FeedbackMode::Classic,
            unison_voices: 1,
            unison_detune: 0.0,
            voice_phases: vec![[0.0; 3]],
            voice_feedback: vec![0.0],
            voice_feedback_prev: vec![0.0],
        }
    }

    pub fn set_freq_hz(&mut self, freq: f32) {
        self.freq_hz = freq.max(0.1);
    }

    pub fn set_algorithm(&mut self, algo: u8) {
        self.algorithm = algo.min(3);
    }

    pub fn set_ratio2(&mut self, ratio: f32) {
        self.ratio2 = ratio.max(0.01);
    }

    pub fn set_ratio3(&mut self, ratio: f32) {
        self.ratio3 = ratio.max(0.01);
    }

    pub fn set_depth2(&mut self, depth: f32) {
        self.depth2 = depth;
    }

    pub fn set_depth3(&mut self, depth: f32) {
        self.depth3 = depth;
    }

    pub fn set_m3_abs_freq(&mut self, freq: f32) {
        self.m3_abs_freq = freq.max(0.0);
    }

    pub fn set_feedback(&mut self, feedback: f32) {
        self.feedback = feedback.clamp(0.0, 1.0);
    }

    pub fn set_feedback_mode(&mut self, mode: Fm3FeedbackMode) {
        self.feedback_mode = mode;
    }

    pub fn set_unison(&mut self, voices: usize, detune: f32) {
        self.unison_voices = voices.clamp(1, 16);
        self.unison_detune = detune.clamp(0.0, 1.0);
        self.voice_phases.resize_with(self.unison_voices, || {
            [rand::random(), rand::random(), rand::random()]
        });
        self.voice_feedback.resize(self.unison_voices, 0.0);
        self.voice_feedback_prev.resize(self.unison_voices, 0.0);
    }

    pub fn reset(&mut self) {
        self.phases = [0.0; 3];
        for vp in &mut self.voice_phases {
            for p in vp.iter_mut() {
                *p = rand::random();
            }
        }
        for fb in &mut self.voice_feedback {
            *fb = 0.0;
        }
        for fb in &mut self.voice_feedback_prev {
            *fb = 0.0;
        }
    }

    pub fn reset_to_zero(&mut self) {
        self.phases = [0.0; 3];
        for vp in &mut self.voice_phases {
            *vp = [0.0; 3];
        }
        for fb in &mut self.voice_feedback {
            *fb = 0.0;
        }
        for fb in &mut self.voice_feedback_prev {
            *fb = 0.0;
        }
    }

    pub fn next(&mut self) -> (f32, f32) {
        let mut sum = 0.0f32;
        let base_dt = self.freq_hz / self.sample_rate;
        let detune_scale = self.unison_detune * 0.05;

        for i in 0..self.unison_voices {
            let detune_mul = if self.unison_voices > 1 {
                let offset =
                    (i as f32 / (self.unison_voices as f32 - 1.0) - 0.5) * 2.0 * detune_scale;
                2.0f32.powf(offset / 12.0)
            } else {
                1.0
            };

            let vp = &mut self.voice_phases[i];
            let dt1 = base_dt * detune_mul;
            let dt2 = dt1 * self.ratio2;
            let dt3 = if self.m3_abs_freq > 0.1 {
                self.m3_abs_freq / self.sample_rate
            } else {
                dt1 * self.ratio3
            };

            let fb = match self.feedback_mode {
                Fm3FeedbackMode::Classic => self.voice_feedback[i] * self.feedback * 2.0 * PI,
                Fm3FeedbackMode::Averaged => {
                    (self.voice_feedback[i] + self.voice_feedback_prev[i])
                        * 0.5
                        * self.feedback
                        * 2.0
                        * PI
                }
            };

            // Generate operators
            let op3 = (vp[2] * 2.0 * PI).sin();
            let op2 = (vp[1] * 2.0 * PI).sin();
            let op1 = (vp[0] * 2.0 * PI).sin();

            let out = match self.algorithm {
                0 => {
                    // Cascade: 3 → 2 → 1
                    let mod2 = op3 * self.depth3;
                    let mod1 = op2 * self.depth2 + mod2;
                    (vp[0] * 2.0 * PI + mod1 * 2.0 * PI + fb).sin()
                }
                1 => {
                    // Stack: (2 + 3) → 1
                    let mod1 = op2 * self.depth2 + op3 * self.depth3;
                    (vp[0] * 2.0 * PI + mod1 * 2.0 * PI + fb).sin()
                }
                2 => {
                    // 3 → 2, 2 → 1
                    let mod2 = op3 * self.depth3;
                    let mod1 = (vp[1] * 2.0 * PI + mod2 * 2.0 * PI).sin();
                    (vp[0] * 2.0 * PI + mod1 * self.depth2 * 2.0 * PI + fb).sin()
                }
                _ => {
                    // Independent sum
                    op1 + op2 * self.depth2 + op3 * self.depth3
                }
            };

            self.voice_feedback_prev[i] = self.voice_feedback[i];
            self.voice_feedback[i] = out;

            vp[0] += dt1;
            vp[1] += dt2;
            vp[2] += dt3;
            while vp[0] >= 1.0 {
                vp[0] -= 1.0;
            }
            while vp[1] >= 1.0 {
                vp[1] -= 1.0;
            }
            while vp[2] >= 1.0 {
                vp[2] -= 1.0;
            }

            sum += out;
        }

        let atten = 1.0 / (self.unison_voices as f32).sqrt();
        (sum * atten, sum * atten)
    }
}

// ---------------------------------------------------------------------------
// Wavetable Oscillator
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WavetableOsc {
    sample_rate: f32,
    freq_hz: f32,
    phase: f32,
    shape: f32,    // morph position 0..1
    skew: f32,     // horizontal phase distortion
    skew_v: f32,   // vertical skew (amplitude shaping)
    saturate: f32, // pre-filter saturation
    formant: f32,
    keytrack: f32,
    unison_voices: usize,
    unison_detune: f32,
    voice_phases: Vec<f32>,
    sampler_mode: u8,
    sample_position: f32,
    pub wavetable: Option<Box<Wavetable>>,
}

impl WavetableOsc {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            freq_hz: 440.0,
            phase: 0.0,
            shape: 0.0,
            skew: 0.0,
            skew_v: 0.0,
            saturate: 0.0,
            formant: 1.0,
            keytrack: 0.0,
            unison_voices: 1,
            unison_detune: 0.0,
            voice_phases: vec![0.0],
            sampler_mode: 0,
            sample_position: 0.0,
            wavetable: None,
        }
    }

    pub fn set_freq_hz(&mut self, freq: f32) {
        self.freq_hz = freq.max(0.1);
    }

    pub fn set_shape(&mut self, shape: f32) {
        self.shape = shape.clamp(0.0, 1.0);
    }

    pub fn set_skew(&mut self, skew: f32) {
        self.skew = skew.clamp(-1.0, 1.0);
    }

    pub fn set_formant(&mut self, formant: f32) {
        self.formant = formant.clamp(0.25, 4.0);
    }

    pub fn set_keytrack(&mut self, keytrack: f32) {
        self.keytrack = keytrack.clamp(-1.0, 1.0);
    }

    pub fn set_skew_v(&mut self, skew_v: f32) {
        self.skew_v = skew_v.clamp(-1.0, 1.0);
    }

    pub fn set_saturate(&mut self, saturate: f32) {
        self.saturate = saturate.clamp(0.0, 1.0);
    }

    pub fn set_sampler_mode(&mut self, mode: u8) {
        self.sampler_mode = mode.min(2);
        if self.sampler_mode > 0 {
            self.sample_position = 0.0;
        }
    }

    pub fn set_unison(&mut self, voices: usize, detune: f32) {
        self.unison_voices = voices.clamp(1, 16);
        self.unison_detune = detune.clamp(0.0, 1.0);
        self.voice_phases
            .resize_with(self.unison_voices, rand::random::<f32>);
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
        for vp in &mut self.voice_phases {
            *vp = rand::random();
        }
    }

    pub fn reset_to_zero(&mut self) {
        self.phase = 0.0;
        for vp in &mut self.voice_phases {
            *vp = 0.0;
        }
    }

    pub fn next(&mut self, fm_input: f32) -> (f32, f32) {
        // Sampler mode: single-frame wavetable plays as one-shot or looped sample
        if self.sampler_mode > 0 {
            let is_single_frame = match self.wavetable.as_ref() {
                Some(wt) => wt.n_tables == 1,
                None => return (0.0, 0.0),
            };
            if is_single_frame {
                return self.next_sampler();
            }
        }

        let wt = match &self.wavetable {
            Some(wt) => wt,
            None => return (0.0, 0.0),
        };

        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;
        let base_inc = self.freq_hz / self.sample_rate;
        let detune_scale = self.unison_detune * 0.05;
        let fm_shift = fm_input * 0.05;

        // Mipmap selection based on playback rate
        let mipmap = wt.select_mipmap(base_inc);

        // Frame morphing with keytracking
        let max_frame = (wt.n_tables.saturating_sub(1)) as f32;
        let base_morph = self.shape * max_frame;
        let keytrack_offset = self.keytrack * (self.freq_hz / 440.0).log2() * max_frame * 0.5;
        let morph_pos = (base_morph + keytrack_offset).clamp(0.0, max_frame * 0.99999);

        for i in 0..self.unison_voices {
            let offset = if self.unison_voices > 1 {
                (i as f32 / (self.unison_voices as f32 - 1.0) - 0.5) * 2.0 * detune_scale
            } else {
                0.0
            };
            let detune_mul = 2.0f32.powf(offset / 12.0);
            let phase_inc = base_inc * detune_mul;

            let phase = self.voice_phases[i];

            // Phase distortion (horizontal skew)
            let distorted_phase = if self.skew != 0.0 {
                let xt = phase;
                let s = self.skew;
                xt + s * 4.0 * xt * (xt - 1.0) * (2.0 * xt - 1.0) * 1.299038f32
            } else {
                phase
            };

            let formant_phase = ((distorted_phase + fm_shift) * self.formant).fract();
            let formant_phase = if formant_phase < 0.0 {
                formant_phase + 1.0
            } else {
                formant_phase
            };
            let mut sample = wt.read_morph(morph_pos, formant_phase, mipmap);

            // Vertical skew (amplitude shaping)
            if self.skew_v != 0.0 {
                let sv = self.skew_v;
                sample = sample
                    + sv * 4.0
                        * sample
                        * (sample.abs() - 1.0)
                        * (2.0 * sample.abs() - 1.0)
                        * 1.299038f32;
            }

            // Saturation
            if self.saturate > 0.0 {
                let sat = self.saturate * 5.0;
                sample = sample * (1.0 + sat) / (1.0 + sat * sample.abs());
            }

            self.voice_phases[i] += phase_inc;
            while self.voice_phases[i] >= 1.0 {
                self.voice_phases[i] -= 1.0;
            }

            let pan = if self.unison_voices > 1 {
                i as f32 / (self.unison_voices as f32 - 1.0)
            } else {
                0.5
            };
            sum_l += sample * (1.0 - pan);
            sum_r += sample * pan;
        }

        let atten = 1.0 / (self.unison_voices as f32).sqrt();
        (sum_l * atten, sum_r * atten)
    }

    fn next_sampler(&mut self) -> (f32, f32) {
        let wt = match &self.wavetable {
            Some(wt) => wt,
            None => return (0.0, 0.0),
        };
        let inc = self.freq_hz / self.sample_rate;
        let mipmap = wt.select_mipmap(inc);

        self.sample_position += inc;
        let pos = self.sample_position;

        if self.sampler_mode == 1 {
            // One-shot: silence after end
            if pos >= 1.0 {
                return (0.0, 0.0);
            }
        } else {
            // Loop: wrap around
            if pos >= 1.0 {
                self.sample_position -= 1.0;
            }
        }

        let phase = self.sample_position.fract().clamp(0.0, 1.0);
        let sample = wt.read_morph(0.0, phase, mipmap);
        (sample, sample)
    }
}

// ---------------------------------------------------------------------------
// Window Oscillator
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowType {
    Sine = 0,
    Hanning = 1,
    Hamming = 2,
    Blackman = 3,
    Triangle = 4,
    Cosine = 5,
    Sawtooth = 6,
    Square = 7,
    Rectangle = 8,
}

impl WindowType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => WindowType::Hanning,
            2 => WindowType::Hamming,
            3 => WindowType::Blackman,
            4 => WindowType::Triangle,
            5 => WindowType::Cosine,
            6 => WindowType::Sawtooth,
            7 => WindowType::Square,
            8 => WindowType::Rectangle,
            _ => WindowType::Sine,
        }
    }

    fn sample(&self, phase: f32) -> f32 {
        let p = phase.clamp(0.0, 1.0);
        let two_pi_p = 2.0 * std::f32::consts::PI * p;
        match self {
            WindowType::Sine => (std::f32::consts::PI * p).sin(),
            WindowType::Hanning => 0.5 * (1.0 - two_pi_p.cos()),
            WindowType::Hamming => 0.54 - 0.46 * two_pi_p.cos(),
            WindowType::Blackman => {
                0.42 - 0.5 * two_pi_p.cos() + 0.08 * (4.0 * std::f32::consts::PI * p).cos()
            }
            WindowType::Triangle => 1.0 - (2.0 * p - 1.0).abs(),
            WindowType::Cosine => (std::f32::consts::PI * (p - 0.5)).cos(),
            WindowType::Sawtooth => 1.0 - 2.0 * p,
            WindowType::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            WindowType::Rectangle => {
                if p < 0.25 {
                    1.0
                } else if p < 0.75 {
                    -1.0
                } else {
                    1.0
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct WindowOsc {
    sample_rate: f32,
    freq_hz: f32,
    phase: f32,
    shape: f32,
    formant: f32,
    window_type: WindowType,
    lowcut: Filter,
    highcut: Filter,
    unison_voices: usize,
    unison_detune: f32,
    voice_phases: Vec<f32>,
    pub wavetable: Option<Box<Wavetable>>,
    pub window_wt: Option<Box<Wavetable>>,
}

impl WindowOsc {
    pub fn new(sample_rate: f32) -> Self {
        let mut lowcut = Filter::new(FilterType::Highpass12dB, sample_rate);
        lowcut.set_params(20.0, 0.7);
        lowcut.prepare_block(20.0, 0.7, 1);
        let mut highcut = Filter::new(FilterType::Lowpass12dB, sample_rate);
        highcut.set_params(20000.0, 0.7);
        highcut.prepare_block(20000.0, 0.7, 1);
        Self {
            sample_rate,
            freq_hz: 440.0,
            phase: 0.0,
            shape: 0.0,
            formant: 1.0,
            window_type: WindowType::Sine,
            lowcut,
            highcut,
            unison_voices: 1,
            unison_detune: 0.0,
            voice_phases: vec![0.0],
            wavetable: None,
            window_wt: None,
        }
    }

    pub fn set_freq_hz(&mut self, freq: f32) {
        self.freq_hz = freq.max(0.1);
    }

    pub fn set_shape(&mut self, shape: f32) {
        self.shape = shape.clamp(0.0, 1.0);
    }

    pub fn set_formant(&mut self, formant: f32) {
        self.formant = formant.clamp(0.25, 4.0);
    }

    pub fn set_window_type(&mut self, wt: WindowType) {
        self.window_type = wt;
    }

    pub fn set_lowcut(&mut self, freq: f32) {
        let f = freq.clamp(20.0, 20000.0);
        self.lowcut.set_params(f, 0.7);
        self.lowcut.prepare_block(f, 0.7, 1);
    }

    pub fn set_highcut(&mut self, freq: f32) {
        let f = freq.clamp(20.0, 20000.0);
        self.highcut.set_params(f, 0.7);
        self.highcut.prepare_block(f, 0.7, 1);
    }

    pub fn set_unison(&mut self, voices: usize, detune: f32) {
        self.unison_voices = voices.clamp(1, 16);
        self.unison_detune = detune.clamp(0.0, 1.0);
        self.voice_phases
            .resize_with(self.unison_voices, rand::random::<f32>);
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
        for vp in &mut self.voice_phases {
            *vp = rand::random();
        }
    }

    pub fn reset_to_zero(&mut self) {
        self.phase = 0.0;
        for vp in &mut self.voice_phases {
            *vp = 0.0;
        }
    }

    pub fn next(&mut self, fm_input: f32) -> (f32, f32) {
        let wt = match &self.wavetable {
            Some(wt) => wt,
            None => return (0.0, 0.0),
        };
        let use_builtin_window = self.window_wt.is_none();

        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;
        let base_inc = self.freq_hz / self.sample_rate;
        let detune_scale = self.unison_detune * 0.05;
        let fm_shift = fm_input * 0.05;

        let mipmap = wt.select_mipmap(base_inc);
        let win_mipmap = self.window_wt.as_ref().map(|w| w.select_mipmap(base_inc));

        let max_frame = (wt.n_tables.saturating_sub(1)) as f32;
        let morph_pos = self.shape * max_frame * 0.99999;

        for i in 0..self.unison_voices {
            let offset = if self.unison_voices > 1 {
                (i as f32 / (self.unison_voices as f32 - 1.0) - 0.5) * 2.0 * detune_scale
            } else {
                0.0
            };
            let detune_mul = 2.0f32.powf(offset / 12.0);
            let phase_inc = base_inc * detune_mul;

            let phase = self.voice_phases[i];
            let fm_phase = (phase + fm_shift).fract();
            let fm_phase = if fm_phase < 0.0 {
                fm_phase + 1.0
            } else {
                fm_phase
            };

            // Window lookup (built-in or external wavetable)
            let win_sample = if use_builtin_window {
                self.window_type.sample(fm_phase)
            } else if let Some(w) = &self.window_wt {
                w.read(0, fm_phase, win_mipmap.unwrap_or(0))
            } else {
                0.0
            };

            // Wavetable lookup with formant shift
            let formant_phase = (fm_phase * self.formant).fract();
            let wave_sample = wt.read_morph(morph_pos, formant_phase, mipmap);

            // Multiply window and wave
            let sample = wave_sample * win_sample;

            self.voice_phases[i] += phase_inc;
            while self.voice_phases[i] >= 1.0 {
                self.voice_phases[i] -= 1.0;
            }

            let pan = if self.unison_voices > 1 {
                i as f32 / (self.unison_voices as f32 - 1.0)
            } else {
                0.5
            };
            sum_l += sample * (1.0 - pan);
            sum_r += sample * pan;
        }

        let atten = 1.0 / (self.unison_voices as f32).sqrt();
        let out_l = self.lowcut.process(sum_l * atten);
        let out_l = self.highcut.process(out_l);
        let out_r = self.lowcut.process(sum_r * atten);
        let out_r = self.highcut.process(out_r);
        (out_l, out_r)
    }
}

// ---------------------------------------------------------------------------
// Modern Oscillator (supersaw-style)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModernSubWaveform {
    Square = 0,
    Triangle = 1,
    Saw = 2,
}

impl ModernSubWaveform {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => ModernSubWaveform::Triangle,
            2 => ModernSubWaveform::Saw,
            _ => ModernSubWaveform::Square,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModernOsc {
    sample_rate: f32,
    freq_hz: f32,
    detune: f32,    // 0..1 controls spread of the 7 saws
    width: f32,     // -1..1 stereo width
    sub_mix: f32,   // 0..1 sub-oscillator mix
    sub_octave: i8, // -1 or -2
    sub_waveform: ModernSubWaveform,
    sub_one: bool,    // drops sub an additional octave
    phases: [f32; 8], // 7 saws + 1 sub
}

impl ModernOsc {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            freq_hz: 440.0,
            detune: 0.2,
            width: 0.0,
            sub_mix: 0.0,
            sub_octave: -1,
            sub_waveform: ModernSubWaveform::Square,
            sub_one: false,
            phases: [0.0; 8],
        }
    }

    pub fn set_freq_hz(&mut self, freq: f32) {
        self.freq_hz = freq.max(0.1);
    }

    pub fn set_detune(&mut self, detune: f32) {
        self.detune = detune.clamp(0.0, 1.0);
    }

    pub fn set_width(&mut self, width: f32) {
        self.width = width.clamp(-1.0, 1.0);
    }

    pub fn set_sub_mix(&mut self, sub_mix: f32) {
        self.sub_mix = sub_mix.clamp(0.0, 1.0);
    }

    pub fn set_sub_octave(&mut self, oct: i8) {
        self.sub_octave = match oct {
            1 => -2,
            _ => -1,
        };
    }

    pub fn set_sub_waveform(&mut self, wf: ModernSubWaveform) {
        self.sub_waveform = wf;
    }

    pub fn set_sub_one(&mut self, v: bool) {
        self.sub_one = v;
    }

    pub fn reset(&mut self) {
        for p in &mut self.phases {
            *p = rand::random();
        }
    }

    pub fn reset_to_zero(&mut self) {
        for p in &mut self.phases {
            *p = 0.0;
        }
    }

    pub fn next(&mut self) -> (f32, f32) {
        let base_inc = self.freq_hz / self.sample_rate;
        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;

        // 7 saw waves with progressive detuning
        // Indices 0..6 = saws, index 7 = sub
        let detune_cents = self.detune * 50.0; // up to 50 cents
        let detunes = [
            0.0f32,
            detune_cents * 0.25,
            detune_cents * 0.5,
            detune_cents * 0.75,
            -detune_cents * 0.25,
            -detune_cents * 0.5,
            -detune_cents * 0.75,
            0.0, // sub doesn't use detune
        ];

        for (i, detune) in detunes.iter().enumerate().take(7) {
            let dt = base_inc * 2.0f32.powf(*detune / 1200.0);
            let phase = self.phases[i];
            let t = phase;
            let vdt = dt;

            let mut out = 2.0 * t - 1.0;
            out -= poly_blep(t, vdt);
            out = soft_clip(out);

            self.phases[i] += dt;
            while self.phases[i] >= 1.0 {
                self.phases[i] -= 1.0;
            }

            // Stereo width: center voice stays centered, others spread
            let pan = if i == 0 {
                0.5
            } else {
                let spread = self.width.abs();
                let side = if i <= 3 { 1.0 } else { -1.0 };
                0.5 + side * spread * 0.5
            };
            sum_l += out * (1.0 - pan);
            sum_r += out * pan;
        }

        // Sub-oscillator at -1 or -2 octave (additional -1 with sub_one)
        if self.sub_mix > 0.0 {
            let sub_ratio = 2.0f32.powi(self.sub_octave as i32);
            let extra_div = if self.sub_one { 2.0 } else { 1.0 };
            let sub_inc = base_inc * sub_ratio / extra_div;
            let sub_phase = self.phases[7];
            let sub_out = match self.sub_waveform {
                ModernSubWaveform::Square => {
                    if sub_phase < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                ModernSubWaveform::Triangle => 1.0 - 4.0 * (sub_phase - 0.5).abs(),
                ModernSubWaveform::Saw => {
                    let mut v = 2.0 * sub_phase - 1.0;
                    v -= poly_blep(sub_phase, sub_inc);
                    v
                }
            };
            self.phases[7] += sub_inc;
            while self.phases[7] >= 1.0 {
                self.phases[7] -= 1.0;
            }
            sum_l += sub_out * self.sub_mix;
            sum_r += sub_out * self.sub_mix;
        }

        let total_voices = 7.0 + self.sub_mix;
        let atten = 1.0 / total_voices.sqrt();
        (sum_l * atten, sum_r * atten)
    }
}

// ---------------------------------------------------------------------------
// S&H Noise Oscillator
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ShNoiseOsc {
    sample_rate: f32,
    freq_hz: f32,
    phase: f32,
    last_value: f32,
    correlation: f32,
    width: f32,
    sync: f32,
    pub unison_voices: usize,
    pub unison_detune: f32,
    voice_phases: Vec<f32>,
    voice_values: Vec<f32>,
    voice_sync_phases: Vec<f32>,
    lowcut: Filter,
    highcut: Filter,
}

impl ShNoiseOsc {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            freq_hz: 440.0,
            phase: 0.0,
            last_value: 0.0,
            correlation: 0.0,
            width: 0.5,
            sync: 0.0,
            unison_voices: 1,
            unison_detune: 0.0,
            voice_phases: vec![0.0],
            voice_values: vec![0.0],
            voice_sync_phases: vec![0.0],
            lowcut: Filter::new(FilterType::Highpass12dB, sample_rate),
            highcut: Filter::new(FilterType::Lowpass12dB, sample_rate),
        }
    }

    pub fn set_freq_hz(&mut self, freq: f32) {
        self.freq_hz = freq.max(0.1);
    }

    pub fn set_correlation(&mut self, corr: f32) {
        self.correlation = corr.clamp(-1.0, 1.0);
    }

    pub fn set_width(&mut self, width: f32) {
        self.width = width.clamp(0.0, 1.0);
    }

    pub fn set_sync(&mut self, sync: f32) {
        self.sync = sync.clamp(0.0, 1.0);
    }

    pub fn set_lowcut(&mut self, freq: f32) {
        let f = freq.clamp(20.0, 20000.0);
        self.lowcut.set_params(f, 0.7);
        self.lowcut.prepare_block(f, 0.7, 1);
    }

    pub fn set_highcut(&mut self, freq: f32) {
        let f = freq.clamp(20.0, 20000.0);
        self.highcut.set_params(f, 0.7);
        self.highcut.prepare_block(f, 0.7, 1);
    }

    pub fn set_unison(&mut self, voices: usize, detune: f32) {
        self.unison_voices = voices.clamp(1, 16);
        self.unison_detune = detune.clamp(0.0, 1.0);
        self.voice_phases
            .resize_with(self.unison_voices, rand::random);
        self.voice_values
            .resize_with(self.unison_voices, || rand::random::<f32>() * 2.0 - 1.0);
        self.voice_sync_phases.resize(self.unison_voices, 0.0);
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
        for i in 0..self.unison_voices {
            self.voice_phases[i] = rand::random();
            self.voice_values[i] = rand::random::<f32>() * 2.0 - 1.0;
            self.voice_sync_phases[i] = 0.0;
        }
    }

    pub fn reset_to_zero(&mut self) {
        self.phase = 0.0;
        for i in 0..self.unison_voices {
            self.voice_phases[i] = 0.0;
            self.voice_values[i] = 0.0;
            self.voice_sync_phases[i] = 0.0;
        }
    }

    pub fn next(&mut self) -> (f32, f32) {
        let base_inc = self.freq_hz / self.sample_rate;
        let detune_scale = self.unison_detune * 0.05;
        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;

        for i in 0..self.unison_voices {
            let offset = if self.unison_voices > 1 {
                (i as f32 / (self.unison_voices as f32 - 1.0) - 0.5) * 2.0 * detune_scale
            } else {
                0.0
            };
            let detune_mul = 2.0f32.powf(offset / 12.0);
            let phase_inc = base_inc * detune_mul;

            // Hard sync: sync phase runs at higher frequency, resets main phase on wrap
            if self.sync > 0.0 {
                let sync_ratio = 1.0 + self.sync * 15.0;
                self.voice_sync_phases[i] += phase_inc * sync_ratio;
                if self.voice_sync_phases[i] >= 1.0 {
                    self.voice_sync_phases[i] -= 1.0;
                    self.voice_phases[i] = 0.0;
                }
            }

            // Width controls the pulse-width of S&H steps
            let step_threshold = 0.5 + self.width * 0.5;
            self.voice_phases[i] += phase_inc;
            if self.voice_phases[i] >= step_threshold {
                self.voice_phases[i] -= step_threshold;
                let raw = rand::random::<f32>() * 2.0 - 1.0;
                // Correlation: blend new random with previous value
                let corr = self.correlation;
                self.voice_values[i] = corr * self.voice_values[i] + (1.0 - corr.abs()) * raw;
            }

            let pan = if self.unison_voices > 1 {
                i as f32 / (self.unison_voices as f32 - 1.0)
            } else {
                0.5
            };
            sum_l += self.voice_values[i] * (1.0 - pan);
            sum_r += self.voice_values[i] * pan;
        }

        let atten = 1.0 / (self.unison_voices as f32).sqrt();
        let out_l = self.lowcut.process(sum_l * atten);
        let out_l = self.highcut.process(out_l);
        let out_r = self.lowcut.process(sum_r * atten);
        let out_r = self.highcut.process(out_r);
        (out_l, out_r)
    }
}

// ---------------------------------------------------------------------------
// String Oscillator (Karplus-Strong physical modeling)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExciterType {
    // Burst exciters (one-shot)
    Noise = 0,
    Impulse = 1,
    Pluck = 2,
    Hammer = 3,
    Bow = 4,
    PinkNoise = 5,
    SineBurst = 6,
    RampBurst = 7,
    TriangleBurst = 8,
    SquareBurst = 9,
    SweepBurst = 10,
    LongNoise = 11,
    LongPinkNoise = 12,
    HalfSine = 13,
    TwoSine = 14,
    CosineBurst = 15,
    SceneAInput = 16,
}

impl ExciterType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => ExciterType::Impulse,
            2 => ExciterType::Pluck,
            3 => ExciterType::Hammer,
            4 => ExciterType::Bow,
            5 => ExciterType::PinkNoise,
            6 => ExciterType::SineBurst,
            7 => ExciterType::RampBurst,
            8 => ExciterType::TriangleBurst,
            9 => ExciterType::SquareBurst,
            10 => ExciterType::SweepBurst,
            11 => ExciterType::LongNoise,
            12 => ExciterType::LongPinkNoise,
            13 => ExciterType::HalfSine,
            14 => ExciterType::TwoSine,
            15 => ExciterType::CosineBurst,
            16 => ExciterType::SceneAInput,
            _ => ExciterType::Noise,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StringOsc {
    sample_rate: f32,
    freq_hz: f32,
    buffer: Vec<f32>,
    write_pos: usize,
    buffer2: Vec<f32>,
    write_pos2: usize,
    damping: f32,
    pickup_pos: f32,
    stereo_spread: f32,
    exciter: ExciterType,
    prev_out: f32,
    prev_out2: f32,
    stiffness: f32,
    compliance: f32,
    stiffness_state: f32,
    stiffness_state2: f32,
    tone_lp: Filter,
    tone_hp: Filter,
    tone_lp2: Filter,
    tone_hp2: Filter,
    dual_detune: f32,
    dual_decay: f32,
    oversample: bool,
}

impl StringOsc {
    pub fn new(sample_rate: f32) -> Self {
        let max_size = ((sample_rate * 2.0) / 20.0) as usize + 4;
        Self {
            sample_rate,
            freq_hz: 440.0,
            buffer: vec![0.0; max_size],
            write_pos: 0,
            buffer2: vec![0.0; max_size],
            write_pos2: 0,
            damping: 0.5,
            pickup_pos: 0.5,
            stereo_spread: 0.0,
            exciter: ExciterType::Noise,
            prev_out: 0.0,
            prev_out2: 0.0,
            stiffness: 0.0,
            compliance: 0.0,
            stiffness_state: 0.0,
            stiffness_state2: 0.0,
            tone_lp: Filter::new(FilterType::Lowpass12dB, sample_rate),
            tone_hp: Filter::new(FilterType::Highpass12dB, sample_rate),
            tone_lp2: Filter::new(FilterType::Lowpass12dB, sample_rate),
            tone_hp2: Filter::new(FilterType::Highpass12dB, sample_rate),
            dual_detune: 0.0,
            dual_decay: 0.5,
            oversample: false,
        }
    }

    pub fn set_freq_hz(&mut self, freq: f32) {
        self.freq_hz = freq.max(20.0);
    }

    pub fn set_damping(&mut self, damping: f32) {
        self.damping = damping.clamp(0.0, 1.0);
    }

    pub fn set_pickup_pos(&mut self, pos: f32) {
        self.pickup_pos = pos.clamp(0.0, 1.0);
    }

    pub fn set_stereo_spread(&mut self, spread: f32) {
        self.stereo_spread = spread.clamp(0.0, 1.0);
    }

    pub fn set_exciter(&mut self, exciter: ExciterType) {
        self.exciter = exciter;
    }

    pub fn set_stiffness(&mut self, stiffness: f32) {
        self.stiffness = stiffness.clamp(0.0, 1.0);
    }

    pub fn set_compliance(&mut self, compliance: f32) {
        self.compliance = compliance.clamp(0.0, 1.0);
    }

    pub fn set_oversample(&mut self, os: bool) {
        self.oversample = os;
    }

    pub fn set_tone_lp(&mut self, freq: f32) {
        let f = freq.clamp(20.0, 20000.0);
        self.tone_lp.set_params(f, 0.7);
        self.tone_lp.prepare_block(f, 0.7, 1);
        self.tone_lp2.set_params(f, 0.7);
        self.tone_lp2.prepare_block(f, 0.7, 1);
    }

    pub fn set_tone_hp(&mut self, freq: f32) {
        let f = freq.clamp(20.0, 20000.0);
        self.tone_hp.set_params(f, 0.7);
        self.tone_hp.prepare_block(f, 0.7, 1);
        self.tone_hp2.set_params(f, 0.7);
        self.tone_hp2.prepare_block(f, 0.7, 1);
    }

    pub fn set_dual_detune(&mut self, detune: f32) {
        self.dual_detune = detune.clamp(0.0, 1.0);
    }

    pub fn set_dual_decay(&mut self, decay: f32) {
        self.dual_decay = decay.clamp(0.0, 1.0);
    }

    pub fn reset(&mut self) {
        let len = self.buffer.len();
        let sr = self.sample_rate;
        let freq = self.freq_hz;
        match self.exciter {
            ExciterType::Noise => {
                for s in self.buffer.iter_mut() {
                    *s = rand::random::<f32>() * 2.0 - 1.0;
                }
            }
            ExciterType::Impulse => {
                self.buffer.fill(0.0);
                if len > 0 {
                    self.buffer[0] = 1.0;
                }
            }
            ExciterType::Pluck => {
                let decay_len = (len as f32 * 0.3) as usize;
                for (i, s) in self.buffer.iter_mut().enumerate() {
                    if i < decay_len {
                        let t = i as f32 / decay_len as f32;
                        *s = (1.0 - t) * (rand::random::<f32>() * 2.0 - 1.0);
                    } else {
                        *s = 0.0;
                    }
                }
            }
            ExciterType::Hammer => {
                self.buffer.fill(0.0);
                let burst_len = (len as f32 * 0.15) as usize;
                for (i, s) in self.buffer.iter_mut().enumerate() {
                    if i < burst_len {
                        let t = i as f32 / burst_len as f32;
                        *s = (1.0 - t * t)
                            * (i as f32 * 2.0 * std::f32::consts::PI / burst_len as f32).sin();
                    }
                }
            }
            ExciterType::Bow => {
                for (i, s) in self.buffer.iter_mut().enumerate() {
                    let t = i as f32 / len as f32;
                    *s = (t * 2.0 - 1.0) * 0.3;
                }
            }
            ExciterType::PinkNoise => {
                let mut _white = 0.0f32;
                let mut _pink = 0.0f32;
                let mut b0 = 0.0f32;
                let mut b1 = 0.0f32;
                let mut b2 = 0.0f32;
                for s in self.buffer.iter_mut() {
                    _white = rand::random::<f32>() * 2.0 - 1.0;
                    b0 = 0.99886 * b0 + _white * 0.0555179;
                    b1 = 0.99332 * b1 + _white * 0.0750759;
                    b2 = 0.96900 * b2 + _white * 0.153_852;
                    _pink = b0 + b1 + b2 + _white * 0.5362;
                    *s = _pink * 0.3;
                }
            }
            ExciterType::SineBurst => {
                self.buffer.fill(0.0);
                let burst_len = (len as f32 * 0.2) as usize;
                for (i, s) in self.buffer.iter_mut().enumerate() {
                    if i < burst_len {
                        let t = i as f32 / burst_len as f32;
                        *s = (1.0 - t) * (i as f32 * 2.0 * std::f32::consts::PI * freq / sr).sin();
                    }
                }
            }
            ExciterType::RampBurst => {
                self.buffer.fill(0.0);
                let burst_len = (len as f32 * 0.25) as usize;
                for (i, s) in self.buffer.iter_mut().enumerate() {
                    if i < burst_len {
                        let t = i as f32 / burst_len as f32;
                        *s = (1.0 - t) * (t * 2.0 - 1.0);
                    }
                }
            }
            ExciterType::TriangleBurst => {
                self.buffer.fill(0.0);
                let burst_len = (len as f32 * 0.25) as usize;
                for (i, s) in self.buffer.iter_mut().enumerate() {
                    if i < burst_len {
                        let t = i as f32 / burst_len as f32;
                        *s = (1.0 - t) * (1.0 - 4.0 * (t - 0.5).abs());
                    }
                }
            }
            ExciterType::SquareBurst => {
                self.buffer.fill(0.0);
                let burst_len = (len as f32 * 0.25) as usize;
                for (i, s) in self.buffer.iter_mut().enumerate() {
                    if i < burst_len {
                        let t = i as f32 / burst_len as f32;
                        *s = (1.0 - t) * if t < 0.5 { 1.0 } else { -1.0 };
                    }
                }
            }
            ExciterType::SweepBurst => {
                self.buffer.fill(0.0);
                let burst_len = (len as f32 * 0.3) as usize;
                for (i, s) in self.buffer.iter_mut().enumerate() {
                    if i < burst_len {
                        let t = i as f32 / burst_len as f32;
                        let sweep_freq = freq * (0.5 + t * 4.0);
                        *s = (1.0 - t)
                            * (i as f32 * 2.0 * std::f32::consts::PI * sweep_freq / sr).sin();
                    }
                }
            }
            ExciterType::LongNoise => {
                let decay_len = (len as f32 * 0.6) as usize;
                for (i, s) in self.buffer.iter_mut().enumerate() {
                    if i < decay_len {
                        let t = i as f32 / decay_len as f32;
                        *s = (1.0 - t) * (rand::random::<f32>() * 2.0 - 1.0);
                    } else {
                        *s = 0.0;
                    }
                }
            }
            ExciterType::LongPinkNoise => {
                let decay_len = (len as f32 * 0.6) as usize;
                let mut _white = 0.0f32;
                let mut _pink = 0.0f32;
                let mut b0 = 0.0f32;
                let mut b1 = 0.0f32;
                let mut b2 = 0.0f32;
                for (i, s) in self.buffer.iter_mut().enumerate() {
                    if i < decay_len {
                        let t = i as f32 / decay_len as f32;
                        _white = rand::random::<f32>() * 2.0 - 1.0;
                        b0 = 0.99886 * b0 + _white * 0.0555179;
                        b1 = 0.99332 * b1 + _white * 0.0750759;
                        b2 = 0.96900 * b2 + _white * 0.153_852;
                        _pink = b0 + b1 + b2 + _white * 0.5362;
                        *s = (1.0 - t) * _pink * 0.3;
                    } else {
                        *s = 0.0;
                    }
                }
            }
            ExciterType::HalfSine => {
                self.buffer.fill(0.0);
                let burst_len = (len as f32 * 0.2) as usize;
                for (i, s) in self.buffer.iter_mut().enumerate() {
                    if i < burst_len {
                        let t = i as f32 / burst_len as f32;
                        *s = (1.0 - t) * (t * std::f32::consts::PI).sin();
                    }
                }
            }
            ExciterType::TwoSine => {
                self.buffer.fill(0.0);
                let burst_len = (len as f32 * 0.25) as usize;
                for (i, s) in self.buffer.iter_mut().enumerate() {
                    if i < burst_len {
                        let t = i as f32 / burst_len as f32;
                        *s = (1.0 - t) * (t * 2.0 * std::f32::consts::PI * 2.0).sin();
                    }
                }
            }
            ExciterType::CosineBurst => {
                self.buffer.fill(0.0);
                let burst_len = (len as f32 * 0.2) as usize;
                for (i, s) in self.buffer.iter_mut().enumerate() {
                    if i < burst_len {
                        let t = i as f32 / burst_len as f32;
                        *s = (1.0 - t) * (t * 2.0 * std::f32::consts::PI).cos();
                    }
                }
            }
            ExciterType::SceneAInput => {
                // Architecturally blocked: no multi-scene audio routing.
                // Falls back to silence; would require Scene A output buffer
                // to be passed into StringOsc::reset() at voice trigger time.
                self.buffer.fill(0.0);
            }
        }
        self.buffer2.copy_from_slice(&self.buffer);
        self.prev_out = 0.0;
        self.prev_out2 = 0.0;
        self.stiffness_state = 0.0;
        self.stiffness_state2 = 0.0;
    }

    pub fn reset_to_zero(&mut self) {
        self.buffer.fill(0.0);
        self.buffer2.fill(0.0);
        self.prev_out = 0.0;
        self.prev_out2 = 0.0;
    }

    pub fn next(&mut self) -> (f32, f32) {
        let os_factor = if self.oversample { 2.0 } else { 1.0 };
        let mut acc_l = 0.0;
        let mut acc_r = 0.0;

        for _ in 0..os_factor as usize {
            // --- String 1 ---
            let delay_samples = self.sample_rate * os_factor / self.freq_hz;
            let read_offset_l = delay_samples * (1.0 - self.pickup_pos);
            let read_offset_r = delay_samples * (1.0 - self.pickup_pos + self.stereo_spread * 0.1);

            let delayed_l =
                Self::read_interpolated_buf(&self.buffer, self.write_pos, read_offset_l);
            let delayed_r =
                Self::read_interpolated_buf(&self.buffer, self.write_pos, read_offset_r);

            let damp_factor1 = self.damping * 0.5;
            let avg_delayed1 = (delayed_l + delayed_r) * 0.5;
            let damped1 = avg_delayed1 * (1.0 - damp_factor1) + self.prev_out * damp_factor1;
            self.prev_out = avg_delayed1;

            let mut out1 = damped1;
            if self.stiffness > 0.0 || self.compliance > 0.0 {
                let sr = self.sample_rate * os_factor;
                let fc = (self.freq_hz * 2.0).clamp(100.0, sr * 0.45);
                let omega = (std::f32::consts::PI * fc / sr).tan();
                if self.stiffness > 0.0 {
                    let a = 10.0f32.powf(self.stiffness * 6.0 / 20.0);
                    let sqrt_a = a.sqrt();
                    let b0 = a * ((a + 1.0) + (a - 1.0) * 1.0 + 2.0 * sqrt_a * omega);
                    let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * 1.0);
                    let b2 = a * ((a + 1.0) + (a - 1.0) * 1.0 - 2.0 * sqrt_a * omega);
                    let a0 = (a + 1.0) - (a - 1.0) * 1.0 + 2.0 * sqrt_a * omega;
                    let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * 1.0);
                    let a2 = (a + 1.0) - (a - 1.0) * 1.0 - 2.0 * sqrt_a * omega;
                    let b0 = b0 / a0;
                    let b1 = b1 / a0;
                    let b2 = b2 / a0;
                    let a1 = a1 / a0;
                    let a2 = a2 / a0;
                    let new_out = b0 * out1 + b1 * self.stiffness_state + b2 * self.stiffness_state
                        - a1 * self.stiffness_state
                        - a2 * self.stiffness_state;
                    self.stiffness_state = out1;
                    out1 = new_out;
                }
                if self.compliance > 0.0 {
                    let a = 10.0f32.powf(-self.compliance * 6.0 / 20.0);
                    let sqrt_a = a.sqrt();
                    let b0 = a * ((a + 1.0) - (a - 1.0) * 1.0 + 2.0 * sqrt_a * omega);
                    let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * 1.0);
                    let b2 = a * ((a + 1.0) + (a - 1.0) * 1.0 - 2.0 * sqrt_a * omega);
                    let a0 = (a + 1.0) + (a - 1.0) * 1.0 + 2.0 * sqrt_a * omega;
                    let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * 1.0);
                    let a2 = (a + 1.0) + (a - 1.0) * 1.0 - 2.0 * sqrt_a * omega;
                    let b0 = b0 / a0;
                    let b1 = b1 / a0;
                    let b2 = b2 / a0;
                    let a1 = a1 / a0;
                    let a2 = a2 / a0;
                    let new_out = b0 * out1 + b1 * self.stiffness_state + b2 * self.stiffness_state
                        - a1 * self.stiffness_state
                        - a2 * self.stiffness_state;
                    self.stiffness_state = out1;
                    out1 = new_out;
                }
            }
            out1 *= 0.995;
            self.buffer[self.write_pos] = out1;
            self.write_pos = (self.write_pos + 1) % self.buffer.len();

            let out_l1 = self.tone_lp.process(delayed_l);
            let out_l1 = self.tone_hp.process(out_l1);
            let out_r1 = self.tone_lp.process(delayed_r);
            let out_r1 = self.tone_hp.process(out_r1);

            // --- String 2 ---
            let freq2 = self.freq_hz * 2.0f32.powf(self.dual_detune / 12.0);
            let delay_samples2 = self.sample_rate * os_factor / freq2.max(20.0);
            let read_offset_l2 = delay_samples2 * (1.0 - self.pickup_pos);
            let read_offset_r2 =
                delay_samples2 * (1.0 - self.pickup_pos + self.stereo_spread * 0.1);

            let delayed_l2 =
                Self::read_interpolated_buf(&self.buffer2, self.write_pos2, read_offset_l2);
            let delayed_r2 =
                Self::read_interpolated_buf(&self.buffer2, self.write_pos2, read_offset_r2);

            let damp_factor2 = self.dual_decay * 0.5;
            let avg_delayed2 = (delayed_l2 + delayed_r2) * 0.5;
            let damped2 = avg_delayed2 * (1.0 - damp_factor2) + self.prev_out2 * damp_factor2;
            self.prev_out2 = avg_delayed2;

            let mut out2 = damped2;
            if self.stiffness > 0.0 || self.compliance > 0.0 {
                let sr = self.sample_rate * os_factor;
                let fc = (freq2 * 2.0).clamp(100.0, sr * 0.45);
                let omega = (std::f32::consts::PI * fc / sr).tan();
                if self.stiffness > 0.0 {
                    let a = 10.0f32.powf(self.stiffness * 6.0 / 20.0);
                    let sqrt_a = a.sqrt();
                    let b0 = a * ((a + 1.0) + (a - 1.0) * 1.0 + 2.0 * sqrt_a * omega);
                    let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * 1.0);
                    let b2 = a * ((a + 1.0) + (a - 1.0) * 1.0 - 2.0 * sqrt_a * omega);
                    let a0 = (a + 1.0) - (a - 1.0) * 1.0 + 2.0 * sqrt_a * omega;
                    let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * 1.0);
                    let a2 = (a + 1.0) - (a - 1.0) * 1.0 - 2.0 * sqrt_a * omega;
                    let b0 = b0 / a0;
                    let b1 = b1 / a0;
                    let b2 = b2 / a0;
                    let a1 = a1 / a0;
                    let a2 = a2 / a0;
                    let new_out =
                        b0 * out2 + b1 * self.stiffness_state2 + b2 * self.stiffness_state2
                            - a1 * self.stiffness_state2
                            - a2 * self.stiffness_state2;
                    self.stiffness_state2 = out2;
                    out2 = new_out;
                }
                if self.compliance > 0.0 {
                    let a = 10.0f32.powf(-self.compliance * 6.0 / 20.0);
                    let sqrt_a = a.sqrt();
                    let b0 = a * ((a + 1.0) - (a - 1.0) * 1.0 + 2.0 * sqrt_a * omega);
                    let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * 1.0);
                    let b2 = a * ((a + 1.0) + (a - 1.0) * 1.0 - 2.0 * sqrt_a * omega);
                    let a0 = (a + 1.0) + (a - 1.0) * 1.0 + 2.0 * sqrt_a * omega;
                    let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * 1.0);
                    let a2 = (a + 1.0) + (a - 1.0) * 1.0 - 2.0 * sqrt_a * omega;
                    let b0 = b0 / a0;
                    let b1 = b1 / a0;
                    let b2 = b2 / a0;
                    let a1 = a1 / a0;
                    let a2 = a2 / a0;
                    let new_out =
                        b0 * out2 + b1 * self.stiffness_state2 + b2 * self.stiffness_state2
                            - a1 * self.stiffness_state2
                            - a2 * self.stiffness_state2;
                    self.stiffness_state2 = out2;
                    out2 = new_out;
                }
            }
            out2 *= 0.995;
            self.buffer2[self.write_pos2] = out2;
            self.write_pos2 = (self.write_pos2 + 1) % self.buffer2.len();

            let out_l2 = self.tone_lp2.process(delayed_l2);
            let out_l2 = self.tone_hp2.process(out_l2);
            let out_r2 = self.tone_lp2.process(delayed_r2);
            let out_r2 = self.tone_hp2.process(out_r2);

            // --- Mix ---
            let mix2 = self.dual_detune * 0.5;
            let mix1 = 1.0 - mix2;
            acc_l += out_l1 * mix1 + out_l2 * mix2;
            acc_r += out_r1 * mix1 + out_r2 * mix2;
        }

        (acc_l / os_factor, acc_r / os_factor)
    }
    fn read_interpolated_buf(buffer: &[f32], write_pos: usize, offset: f32) -> f32 {
        let read_pos_f = write_pos as f32 + buffer.len() as f32 - offset;
        let read_pos = (read_pos_f as usize) % buffer.len();
        let read_pos2 = (read_pos + 1) % buffer.len();
        let frac = read_pos_f - read_pos_f.floor();
        buffer[read_pos] * (1.0 - frac) + buffer[read_pos2] * frac
    }

    fn read_interpolated(&self, offset: f32) -> f32 {
        Self::read_interpolated_buf(&self.buffer, self.write_pos, offset)
    }
}

// ---------------------------------------------------------------------------
// Alias Oscillator (intentionally gritty digital oscillator)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AliasWaveform {
    Saw = 0,
    Sine = 1,
    Pulse = 2,
    Triangle = 3,
    Noise = 4,
    Square = 5,
    Tx2 = 6,
    Tx3 = 7,
    Tx4 = 8,
    Tx5 = 9,
    Tx6 = 10,
    Tx7 = 11,
    Tx8 = 12,
    Additive = 13,
    Ramp = 14,
    AliasMem = 15,
}

impl AliasWaveform {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => AliasWaveform::Saw,
            1 => AliasWaveform::Sine,
            2 => AliasWaveform::Pulse,
            3 => AliasWaveform::Triangle,
            4 => AliasWaveform::Noise,
            5 => AliasWaveform::Square,
            6 => AliasWaveform::Tx2,
            7 => AliasWaveform::Tx3,
            8 => AliasWaveform::Tx4,
            9 => AliasWaveform::Tx5,
            10 => AliasWaveform::Tx6,
            11 => AliasWaveform::Tx7,
            12 => AliasWaveform::Tx8,
            13 => AliasWaveform::Additive,
            14 => AliasWaveform::Ramp,
            _ => AliasWaveform::AliasMem,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AliasWaveform::Saw => "Saw",
            AliasWaveform::Sine => "Sine",
            AliasWaveform::Pulse => "Pulse",
            AliasWaveform::Triangle => "Triangle",
            AliasWaveform::Noise => "Noise",
            AliasWaveform::Square => "Square",
            AliasWaveform::Tx2 => "TX2",
            AliasWaveform::Tx3 => "TX3",
            AliasWaveform::Tx4 => "TX4",
            AliasWaveform::Tx5 => "TX5",
            AliasWaveform::Tx6 => "TX6",
            AliasWaveform::Tx7 => "TX7",
            AliasWaveform::Tx8 => "TX8",
            AliasWaveform::Additive => "Additive",
            AliasWaveform::Ramp => "Ramp",
            AliasWaveform::AliasMem => "AliasMem",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AliasOsc {
    sample_rate: f32,
    freq_hz: f32,
    quant_bits: f32,
    decim_factor: f32,
    dirty: f32,
    waveform: AliasWaveform,
    wrap: bool,
    mask: u8,
    threshold: f32,
    hold_value: f32,
    hold_counter: usize,
    unison_voices: usize,
    unison_detune: f32,
    voice_phases: Vec<f32>,
    ring_buffer: [f32; 256],
    ring_pos: usize,
    lcg_state: u32,
    partial_amplitudes: [f32; 16],
}

impl AliasOsc {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            freq_hz: 440.0,
            quant_bits: 16.0,
            decim_factor: 1.0,
            dirty: 0.0,
            waveform: AliasWaveform::Saw,
            wrap: false,
            mask: 0,
            threshold: 0.0,
            hold_value: 0.0,
            hold_counter: 0,
            unison_voices: 1,
            unison_detune: 0.0,
            voice_phases: vec![0.0],
            ring_buffer: [0.0f32; 256],
            ring_pos: 0,
            lcg_state: 0xACE1_ACE1,
            partial_amplitudes: [
                1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
        }
    }

    pub fn set_freq_hz(&mut self, freq: f32) {
        self.freq_hz = freq.max(0.1);
    }

    pub fn set_quant_bits(&mut self, bits: f32) {
        self.quant_bits = bits.clamp(1.0, 16.0);
    }

    pub fn set_decim_factor(&mut self, factor: f32) {
        self.decim_factor = factor.clamp(1.0, 64.0);
    }

    pub fn set_dirty(&mut self, dirty: f32) {
        self.dirty = dirty.clamp(0.0, 1.0);
    }

    pub fn set_waveform(&mut self, waveform: AliasWaveform) {
        self.waveform = waveform;
    }

    pub fn set_wrap(&mut self, wrap: bool) {
        self.wrap = wrap;
    }

    pub fn set_mask(&mut self, mask: f32) {
        self.mask = mask as u8;
    }

    pub fn set_threshold(&mut self, threshold: f32) {
        self.threshold = threshold.clamp(0.0, 1.0);
    }

    pub fn set_unison(&mut self, voices: usize, detune: f32) {
        self.unison_voices = voices.clamp(1, 16);
        self.unison_detune = detune.clamp(0.0, 1.0);
        self.voice_phases
            .resize_with(self.unison_voices, rand::random);
    }

    pub fn set_partial_amplitude(&mut self, index: usize, amplitude: f32) {
        if index < 16 {
            self.partial_amplitudes[index] = amplitude.clamp(0.0, 1.0);
        }
    }

    pub fn reset(&mut self) {
        self.hold_value = 0.0;
        self.hold_counter = 0;
        self.ring_pos = 0;
        self.ring_buffer = [0.0f32; 256];
        for vp in &mut self.voice_phases {
            *vp = rand::random();
        }
    }

    pub fn reset_to_zero(&mut self) {
        self.hold_value = 0.0;
        self.hold_counter = 0;
        self.ring_pos = 0;
        self.ring_buffer = [0.0f32; 256];
        for vp in &mut self.voice_phases {
            *vp = 0.0;
        }
    }

    fn lcg_noise(&mut self) -> f32 {
        self.lcg_state = self.lcg_state.wrapping_mul(1103515245).wrapping_add(12345);
        ((self.lcg_state >> 16) as f32 / 32768.0) - 1.0
    }

    fn waveform_value(&mut self, p: f32) -> f32 {
        let two_pi = 2.0 * std::f32::consts::PI;
        match self.waveform {
            AliasWaveform::Saw => p * 2.0 - 1.0,
            AliasWaveform::Sine => (p * two_pi).sin(),
            AliasWaveform::Pulse => {
                if p < 0.25 {
                    1.0
                } else {
                    -1.0
                }
            }
            AliasWaveform::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
            AliasWaveform::Noise => self.lcg_noise(),
            AliasWaveform::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            AliasWaveform::Tx2 => {
                let s = (p * two_pi).sin();
                s + s.abs() * 0.5
            }
            AliasWaveform::Tx3 => {
                let s = (p * two_pi).sin();
                s + s.max(0.0)
            }
            AliasWaveform::Tx4 => {
                let s = (p * two_pi).sin();
                s + s * s * 0.5
            }
            AliasWaveform::Tx5 => {
                let s = (p * two_pi).sin();
                s + s.powi(3) * 0.5
            }
            AliasWaveform::Tx6 => {
                let s = (p * two_pi).sin();
                s + s.abs().powi(2) * 0.5
            }
            AliasWaveform::Tx7 => {
                let s = (p * two_pi).sin();
                s + s.abs().powi(3) * 0.5
            }
            AliasWaveform::Tx8 => {
                let s = (p * two_pi).sin();
                s + (p * 2.0 - 1.0) * 0.5
            }
            AliasWaveform::Additive => {
                let mut sum = 0.0f32;
                let mut total_amp = 0.0f32;
                for h in 1..=16 {
                    let amp = self.partial_amplitudes[h - 1];
                    if amp > 0.0 {
                        sum += (p * h as f32 * two_pi).sin() * amp;
                        total_amp += amp;
                    }
                }
                if total_amp > 0.0 {
                    sum / total_amp
                } else {
                    0.0
                }
            }
            AliasWaveform::Ramp => 1.0 - p * 2.0,
            AliasWaveform::AliasMem => {
                let read_offset = ((self.dirty * 254.0) as usize + 1).clamp(1, 255);
                let read_pos = (self.ring_pos + 256 - read_offset) & 0xFF;
                self.ring_buffer[read_pos]
            }
        }
    }

    pub fn next(&mut self) -> (f32, f32) {
        let base_inc = self.freq_hz / self.sample_rate;
        let detune_scale = self.unison_detune * 0.05;
        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;

        let levels = 2.0f32.powf(self.quant_bits - 1.0);
        let hold_every = self.decim_factor as usize;

        for i in 0..self.unison_voices {
            let offset = if self.unison_voices > 1 {
                (i as f32 / (self.unison_voices as f32 - 1.0) - 0.5) * 2.0 * detune_scale
            } else {
                0.0
            };
            let detune_mul = 2.0f32.powf(offset / 12.0);
            let phase_inc = base_inc * detune_mul;

            self.voice_phases[i] += phase_inc;
            while self.voice_phases[i] >= 1.0 {
                self.voice_phases[i] -= 1.0;
            }

            let mut p = self.voice_phases[i];
            // Dirty: phase distortion before waveform generation
            if self.dirty > 0.0 && self.waveform != AliasWaveform::AliasMem {
                let dirty_phase = p + self.dirty * 0.25 * (p * 2.0 * std::f32::consts::PI).sin();
                p = dirty_phase.fract();
                if p < 0.0 {
                    p += 1.0;
                }
            }

            let raw = self.waveform_value(p);

            // Bit crush (quantize)
            let crushed = (raw * levels).round() / levels;

            // Decimation (sample-and-hold)
            let mut out = if hold_every <= 1 {
                crushed
            } else {
                if i == 0 {
                    self.hold_counter += 1;
                    if self.hold_counter >= hold_every {
                        self.hold_counter = 0;
                        self.hold_value = crushed;
                    }
                }
                self.hold_value
            };

            // Wrap: modulo-fold output back into [-1, 1]
            if self.wrap {
                out = ((out + 1.0).rem_euclid(2.0)) - 1.0;
            }
            // Mask: bitwise AND on integer-scaled sample
            if self.mask != 0 {
                let scaled = (out * 127.0) as i32;
                let masked = scaled & (self.mask as i32);
                out = masked as f32 / 127.0;
            }
            // Threshold: zero out samples below threshold
            if self.threshold > 0.0 && out.abs() < self.threshold {
                out = 0.0;
            }

            let pan = if self.unison_voices > 1 {
                i as f32 / (self.unison_voices as f32 - 1.0)
            } else {
                0.5
            };
            sum_l += out * (1.0 - pan);
            sum_r += out * pan;
        }

        // Write mono output to ring buffer for AliasMem mode
        if self.waveform == AliasWaveform::AliasMem {
            let mono = (sum_l + sum_r) * 0.5 / (self.unison_voices as f32).sqrt();
            self.ring_buffer[self.ring_pos] = mono;
            self.ring_pos = (self.ring_pos + 1) & 0xFF;
        }

        let atten = 1.0 / (self.unison_voices as f32).sqrt();
        (sum_l * atten, sum_r * atten)
    }
}

// ---------------------------------------------------------------------------
// Audio Input Oscillator
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AudioInputOsc {
    sample_rate: f32,
    gain: f32,
    lo_cut: f32,
    hi_cut: f32,
    hp_state_l: f32,
    hp_state_r: f32,
    lp_state_l: f32,
    lp_state_r: f32,
}

impl AudioInputOsc {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            gain: 1.0,
            lo_cut: 20.0,
            hi_cut: 20000.0,
            hp_state_l: 0.0,
            hp_state_r: 0.0,
            lp_state_l: 0.0,
            lp_state_r: 0.0,
        }
    }

    pub fn set_freq_hz(&mut self, _freq: f32) {
        // No-op: audio input frequency is determined by external source
    }

    pub fn reset(&mut self) {
        self.hp_state_l = 0.0;
        self.hp_state_r = 0.0;
        self.lp_state_l = 0.0;
        self.lp_state_r = 0.0;
    }

    pub fn reset_to_zero(&mut self) {
        self.reset();
    }

    pub fn set_gain(&mut self, v: f32) {
        self.gain = v.clamp(0.0, 4.0);
    }

    pub fn set_lo_cut(&mut self, v: f32) {
        self.lo_cut = v.clamp(20.0, 20000.0);
    }

    pub fn set_hi_cut(&mut self, v: f32) {
        self.hi_cut = v.clamp(20.0, 20000.0);
    }

    #[inline]
    fn hp_coeff(&self) -> f32 {
        let c = (std::f32::consts::PI * self.lo_cut / self.sample_rate).sin() * 2.0;
        c.min(1.0)
    }

    #[inline]
    fn lp_coeff(&self) -> f32 {
        let c = (std::f32::consts::PI * self.hi_cut / self.sample_rate).sin() * 2.0;
        c.min(1.0)
    }

    pub fn next(&mut self, _fm_input: f32, audio_in_l: f32, audio_in_r: f32) -> (f32, f32) {
        let mut l = audio_in_l * self.gain;
        let mut r = audio_in_r * self.gain;

        // High-pass (lo-cut)
        let hp_c = self.hp_coeff();
        let hp_out_l = l - self.hp_state_l;
        self.hp_state_l += hp_c * hp_out_l;
        l = hp_out_l;
        let hp_out_r = r - self.hp_state_r;
        self.hp_state_r += hp_c * hp_out_r;
        r = hp_out_r;

        // Low-pass (hi-cut)
        let lp_c = self.lp_coeff();
        self.lp_state_l += lp_c * (l - self.lp_state_l);
        l = self.lp_state_l;
        self.lp_state_r += lp_c * (r - self.lp_state_r);
        r = self.lp_state_r;

        (l, r)
    }
}

// ---------------------------------------------------------------------------
// Unified Oscillator
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Oscillator {
    Classic(ClassicOsc),
    Sine(SineOsc),
    Fm2(Fm2Osc),
    Fm3(Fm3Osc),
    Wavetable(WavetableOsc),
    Window(WindowOsc),
    Modern(ModernOsc),
    ShNoise(ShNoiseOsc),
    String(StringOsc),
    Alias(Box<AliasOsc>),
    Twist(TwistOsc),
    AudioInput(AudioInputOsc),
}

impl Oscillator {
    pub fn new(osc_type: OscType, sample_rate: f32) -> Self {
        match osc_type {
            OscType::Classic => Oscillator::Classic(ClassicOsc::new(sample_rate)),
            OscType::Sine => Oscillator::Sine(SineOsc::new(sample_rate)),
            OscType::Fm2 => Oscillator::Fm2(Fm2Osc::new(sample_rate)),
            OscType::Fm3 => Oscillator::Fm3(Fm3Osc::new(sample_rate)),
            OscType::Wavetable => Oscillator::Wavetable(WavetableOsc::new(sample_rate)),
            OscType::Window => Oscillator::Window(WindowOsc::new(sample_rate)),
            OscType::Modern => Oscillator::Modern(ModernOsc::new(sample_rate)),
            OscType::ShNoise => Oscillator::ShNoise(ShNoiseOsc::new(sample_rate)),
            OscType::String => Oscillator::String(StringOsc::new(sample_rate)),
            OscType::Alias => Oscillator::Alias(Box::new(AliasOsc::new(sample_rate))),
            OscType::Twist => Oscillator::Twist(TwistOsc::new(sample_rate)),
            OscType::AudioInput => Oscillator::AudioInput(AudioInputOsc::new(sample_rate)),
        }
    }

    pub fn osc_type(&self) -> OscType {
        match self {
            Oscillator::Classic(_) => OscType::Classic,
            Oscillator::Sine(_) => OscType::Sine,
            Oscillator::Fm2(_) => OscType::Fm2,
            Oscillator::Fm3(_) => OscType::Fm3,
            Oscillator::Wavetable(_) => OscType::Wavetable,
            Oscillator::Window(_) => OscType::Window,
            Oscillator::Modern(_) => OscType::Modern,
            Oscillator::ShNoise(_) => OscType::ShNoise,
            Oscillator::String(_) => OscType::String,
            Oscillator::Alias(_) => OscType::Alias,
            Oscillator::Twist(_) => OscType::Twist,
            Oscillator::AudioInput(_) => OscType::AudioInput,
        }
    }

    pub fn set_freq_hz(&mut self, freq: f32) {
        match self {
            Oscillator::Classic(o) => o.set_freq_hz(freq),
            Oscillator::Sine(o) => o.set_freq_hz(freq),
            Oscillator::Fm2(o) => o.set_freq_hz(freq),
            Oscillator::Fm3(o) => o.set_freq_hz(freq),
            Oscillator::Wavetable(o) => o.set_freq_hz(freq),
            Oscillator::Window(o) => o.set_freq_hz(freq),
            Oscillator::Modern(o) => o.set_freq_hz(freq),
            Oscillator::ShNoise(o) => o.set_freq_hz(freq),
            Oscillator::String(o) => o.set_freq_hz(freq),
            Oscillator::Alias(o) => o.set_freq_hz(freq),
            Oscillator::Twist(o) => o.set_freq_hz(freq),
            Oscillator::AudioInput(o) => o.set_freq_hz(freq),
        }
    }

    pub fn reset(&mut self) {
        match self {
            Oscillator::Classic(o) => o.reset(),
            Oscillator::Sine(o) => o.reset(),
            Oscillator::Fm2(o) => o.reset(),
            Oscillator::Fm3(o) => o.reset(),
            Oscillator::Wavetable(o) => o.reset(),
            Oscillator::Window(o) => o.reset(),
            Oscillator::Modern(o) => o.reset(),
            Oscillator::ShNoise(o) => o.reset(),
            Oscillator::String(o) => o.reset(),
            Oscillator::Alias(o) => o.reset(),
            Oscillator::Twist(o) => o.reset(),
            Oscillator::AudioInput(o) => o.reset(),
        }
    }

    pub fn reset_to_zero(&mut self) {
        match self {
            Oscillator::Classic(o) => o.reset_to_zero(),
            Oscillator::Sine(o) => o.reset_to_zero(),
            Oscillator::Fm2(o) => o.reset_to_zero(),
            Oscillator::Fm3(o) => o.reset_to_zero(),
            Oscillator::Wavetable(o) => o.reset_to_zero(),
            Oscillator::Window(o) => o.reset_to_zero(),
            Oscillator::Modern(o) => o.reset_to_zero(),
            Oscillator::ShNoise(o) => o.reset_to_zero(),
            Oscillator::String(o) => o.reset_to_zero(),
            Oscillator::Alias(o) => o.reset_to_zero(),
            Oscillator::Twist(o) => o.reset_to_zero(),
            Oscillator::AudioInput(o) => o.reset_to_zero(),
        }
    }

    /// Returns stereo pair (left, right).
    pub fn next(&mut self, fm_input: f32, audio_in_l: f32, audio_in_r: f32) -> (f32, f32) {
        match self {
            Oscillator::Classic(o) => o.next(fm_input),
            Oscillator::Sine(o) => o.next(fm_input),
            Oscillator::Fm2(o) => o.next(),
            Oscillator::Fm3(o) => o.next(),
            Oscillator::Wavetable(o) => o.next(fm_input),
            Oscillator::Window(o) => o.next(fm_input),
            Oscillator::Modern(o) => o.next(),
            Oscillator::ShNoise(o) => o.next(),
            Oscillator::String(o) => o.next(),
            Oscillator::Alias(o) => o.next(),
            Oscillator::Twist(o) => o.next(fm_input),
            Oscillator::AudioInput(o) => o.next(fm_input, audio_in_l, audio_in_r),
        }
    }

    pub fn set_unison(&mut self, voices: usize, detune: f32) {
        match self {
            Oscillator::Classic(o) => o.set_unison(voices, detune),
            Oscillator::Sine(o) => o.set_unison(voices, detune),
            Oscillator::Fm2(o) => o.set_unison(voices, detune),
            Oscillator::Fm3(o) => o.set_unison(voices, detune),
            Oscillator::Wavetable(o) => o.set_unison(voices, detune),
            Oscillator::Window(o) => o.set_unison(voices, detune),
            Oscillator::ShNoise(o) => o.set_unison(voices, detune),
            Oscillator::Alias(o) => o.set_unison(voices, detune),
            Oscillator::Twist(o) => o.set_unison(voices, detune),
            Oscillator::Modern(_o) => {
                // Modern osc has fixed 7 voices + sub; ignore unison params
            }
            Oscillator::String(_o) => {
                // String osc is monophonic; ignore unison params
            }
            Oscillator::AudioInput(_o) => {
                // Audio input has no unison
            }
        }
    }

    pub fn set_unison_spread(&mut self, spread: f32) {
        if let Oscillator::Classic(o) = self {
            o.set_unison_spread(spread)
        }
    }

    pub fn set_shape(&mut self, shape: f32) {
        match self {
            Oscillator::Classic(o) => o.set_pulse_width(shape),
            Oscillator::Sine(o) => o.set_fm_amount(shape),
            Oscillator::Fm2(o) => o.set_ratio(shape * 4.0 + 0.5),
            Oscillator::Fm3(o) => o.set_algorithm((shape * 3.0) as u8),
            Oscillator::Wavetable(o) => o.set_shape(shape),
            Oscillator::Window(o) => o.set_shape(shape),
            Oscillator::Modern(o) => o.set_detune(shape),
            Oscillator::String(o) => o.set_damping(shape),
            Oscillator::Alias(o) => o.set_quant_bits(shape * 15.0 + 1.0),
            Oscillator::Twist(o) => o.set_model(super::TwistModel::from_u8((shape * 5.0) as u8)),
            Oscillator::ShNoise(_o) => {}
            Oscillator::AudioInput(o) => o.set_gain(shape * 4.0),
        }
    }

    pub fn set_skew(&mut self, skew: f32) {
        match self {
            Oscillator::Classic(o) => o.set_waveform_morph(skew.clamp(0.0, 1.0)),
            Oscillator::Fm2(o) => o.set_depth(skew.clamp(0.0, 1.0)),
            Oscillator::Fm3(o) => o.set_ratio2((skew + 1.0) * 2.0 + 0.25),
            Oscillator::Wavetable(o) => o.set_skew(skew),
            Oscillator::Modern(o) => o.set_width(skew),
            Oscillator::String(o) => o.set_pickup_pos(skew),
            Oscillator::Alias(o) => o.set_decim_factor(skew * 63.0 + 1.0),
            Oscillator::Sine(o) => o.set_feedback(skew.clamp(0.0, 1.0)),
            Oscillator::Twist(o) => {
                o.set_harmonics(skew);
                o.set_timbre(skew);
            }
            Oscillator::AudioInput(o) => {
                let norm = (skew + 1.0) * 0.5;
                let freq = 20.0 * (1000.0f32).powf(norm);
                o.set_lo_cut(freq);
            }
            _ => {}
        }
    }

    pub fn set_formant(&mut self, formant: f32) {
        match self {
            Oscillator::Window(o) => o.set_formant(formant),
            Oscillator::Modern(o) => o.set_sub_mix((formant - 0.25) / 3.75),
            Oscillator::Fm3(o) => o.set_ratio3(formant * 4.0 + 0.25),
            Oscillator::Alias(o) => o.set_dirty(formant),
            Oscillator::Wavetable(o) => o.set_formant(formant),
            Oscillator::Twist(o) => o.set_morph(formant),
            Oscillator::String(o) => o.set_stiffness(formant),
            Oscillator::AudioInput(o) => {
                let freq = (formant * 5000.0).clamp(20.0, 20000.0);
                o.set_hi_cut(freq);
            }
            _ => {}
        }
    }

    pub fn set_keytrack(&mut self, keytrack: f32) {
        if let Oscillator::Wavetable(o) = self {
            o.set_keytrack(keytrack)
        }
    }

    pub fn set_sync_amount(&mut self, amount: f32) {
        if let Oscillator::Classic(o) = self {
            o.set_sync_amount(amount)
        }
    }

    pub fn set_window_type(&mut self, wt: WindowType) {
        if let Oscillator::Window(o) = self {
            o.set_window_type(wt)
        }
    }

    pub fn set_sub_waveform(&mut self, wf: ModernSubWaveform) {
        if let Oscillator::Modern(o) = self {
            o.set_sub_waveform(wf)
        }
    }

    pub fn set_sub_one(&mut self, v: bool) {
        if let Oscillator::Modern(o) = self {
            o.set_sub_one(v)
        }
    }

    pub fn set_partial_amplitude(&mut self, index: usize, amplitude: f32) {
        if let Oscillator::Alias(o) = self {
            o.set_partial_amplitude(index, amplitude)
        }
    }

    pub fn set_stereo_spread(&mut self, spread: f32) {
        if let Oscillator::String(o) = self {
            o.set_stereo_spread(spread)
        }
    }

    pub fn set_exciter(&mut self, exciter: ExciterType) {
        if let Oscillator::String(o) = self {
            o.set_exciter(exciter)
        }
    }

    pub fn set_fm_amount(&mut self, amount: f32) {
        if let Oscillator::Sine(o) = self {
            o.set_fm_amount(amount)
        }
    }

    pub fn set_pm_mode(&mut self, pm: bool) {
        if let Oscillator::Sine(o) = self {
            o.set_pm_mode(pm)
        }
    }

    pub fn set_shaper_mode(&mut self, mode: SineShaperMode) {
        if let Oscillator::Sine(o) = self {
            o.set_shaper_mode(mode)
        }
    }

    pub fn set_ratio(&mut self, ratio: f32) {
        if let Oscillator::Fm2(o) = self {
            o.set_ratio(ratio)
        }
    }

    pub fn set_depth(&mut self, depth: f32) {
        if let Oscillator::Fm2(o) = self {
            o.set_depth(depth)
        }
    }

    pub fn set_sh_noise_correlation(&mut self, corr: f32) {
        if let Oscillator::ShNoise(o) = self {
            o.set_correlation(corr)
        }
    }

    pub fn set_sh_noise_width(&mut self, width: f32) {
        if let Oscillator::ShNoise(o) = self {
            o.set_width(width)
        }
    }

    pub fn set_sh_noise_sync(&mut self, sync: f32) {
        if let Oscillator::ShNoise(o) = self {
            o.set_sync(sync)
        }
    }

    pub fn set_sh_noise_lowcut(&mut self, freq: f32) {
        if let Oscillator::ShNoise(o) = self {
            o.set_lowcut(freq)
        }
    }

    pub fn set_sh_noise_highcut(&mut self, freq: f32) {
        if let Oscillator::ShNoise(o) = self {
            o.set_highcut(freq)
        }
    }

    pub fn set_width2(&mut self, w: f32) {
        if let Oscillator::Classic(o) = self {
            o.set_width2(w)
        }
    }

    pub fn set_skew_v(&mut self, skew_v: f32) {
        if let Oscillator::Wavetable(o) = self {
            o.set_skew_v(skew_v)
        }
    }

    pub fn set_saturate(&mut self, saturate: f32) {
        if let Oscillator::Wavetable(o) = self {
            o.set_saturate(saturate)
        }
    }

    pub fn set_tone_lp(&mut self, freq: f32) {
        if let Oscillator::String(o) = self {
            o.set_tone_lp(freq)
        }
    }

    pub fn set_tone_hp(&mut self, freq: f32) {
        if let Oscillator::String(o) = self {
            o.set_tone_hp(freq)
        }
    }

    pub fn set_sampler_mode(&mut self, mode: u8) {
        if let Oscillator::Wavetable(o) = self {
            o.set_sampler_mode(mode)
        }
    }

    pub fn set_dual_detune(&mut self, detune: f32) {
        if let Oscillator::String(o) = self {
            o.set_dual_detune(detune)
        }
    }

    pub fn set_dual_decay(&mut self, decay: f32) {
        if let Oscillator::String(o) = self {
            o.set_dual_decay(decay)
        }
    }

    pub fn set_oversample(&mut self, os: bool) {
        if let Oscillator::String(o) = self {
            o.set_oversample(os)
        }
    }
}
