#![allow(dead_code)]

//! Low-frequency oscillator with multiple shapes.
//!
//! Shapes: Sine, Triangle, Saw, Ramp, Square, Sample & Hold, Noise, Envelope, StepSeq, MSEG.

use rand::random;
use std::f32::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfoShape {
    Sine = 0,
    Triangle = 1,
    Saw = 2,
    Ramp = 3,
    Square = 4,
    SampleHold = 5,
    Noise = 6,
    Envelope = 7,
    StepSeq = 8,
    Mseg = 9,
}

impl LfoShape {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => LfoShape::Sine,
            1 => LfoShape::Triangle,
            2 => LfoShape::Saw,
            3 => LfoShape::Ramp,
            4 => LfoShape::Square,
            5 => LfoShape::SampleHold,
            6 => LfoShape::Noise,
            7 => LfoShape::Envelope,
            8 => LfoShape::StepSeq,
            _ => LfoShape::Mseg,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfoTriggerMode {
    FreeRun = 0,
    KeyTrigger = 1,
    Random = 2,
}

impl LfoTriggerMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => LfoTriggerMode::FreeRun,
            1 => LfoTriggerMode::KeyTrigger,
            _ => LfoTriggerMode::Random,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfoSyncMode {
    Free = 0,
    Tempo = 1,
}

impl LfoSyncMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => LfoSyncMode::Free,
            _ => LfoSyncMode::Tempo,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfoSyncDivision {
    One1 = 0,    // 1/1
    One2 = 1,    // 1/2
    One4 = 2,    // 1/4
    One8 = 3,    // 1/8
    One16 = 4,   // 1/16
    One32 = 5,   // 1/32
    One64 = 6,   // 1/64
    One1d = 7,   // 1/1 dotted
    One2d = 8,   // 1/2 dotted
    One4d = 9,   // 1/4 dotted
    One8d = 10,  // 1/8 dotted
    One16d = 11, // 1/16 dotted
    One1t = 12,  // 1/1 triplet
    One2t = 13,  // 1/2 triplet
    One4t = 14,  // 1/4 triplet
    One8t = 15,  // 1/8 triplet
    One16t = 16, // 1/16 triplet
}

impl LfoSyncDivision {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => LfoSyncDivision::One1,
            1 => LfoSyncDivision::One2,
            2 => LfoSyncDivision::One4,
            3 => LfoSyncDivision::One8,
            4 => LfoSyncDivision::One16,
            5 => LfoSyncDivision::One32,
            6 => LfoSyncDivision::One64,
            7 => LfoSyncDivision::One1d,
            8 => LfoSyncDivision::One2d,
            9 => LfoSyncDivision::One4d,
            10 => LfoSyncDivision::One8d,
            11 => LfoSyncDivision::One16d,
            12 => LfoSyncDivision::One1t,
            13 => LfoSyncDivision::One2t,
            14 => LfoSyncDivision::One4t,
            15 => LfoSyncDivision::One8t,
            _ => LfoSyncDivision::One16t,
        }
    }

    /// Convert division to multiplier relative to a quarter note.
    /// e.g. 1/4 = 1.0, 1/8 = 0.5, 1/2 = 2.0
    pub fn to_multiplier(self) -> f32 {
        match self {
            LfoSyncDivision::One1 => 4.0,
            LfoSyncDivision::One2 => 2.0,
            LfoSyncDivision::One4 => 1.0,
            LfoSyncDivision::One8 => 0.5,
            LfoSyncDivision::One16 => 0.25,
            LfoSyncDivision::One32 => 0.125,
            LfoSyncDivision::One64 => 0.0625,
            LfoSyncDivision::One1d => 4.0 * 1.5,
            LfoSyncDivision::One2d => 2.0 * 1.5,
            LfoSyncDivision::One4d => 1.0 * 1.5,
            LfoSyncDivision::One8d => 0.5 * 1.5,
            LfoSyncDivision::One16d => 0.25 * 1.5,
            LfoSyncDivision::One1t => 4.0 * (2.0 / 3.0),
            LfoSyncDivision::One2t => 2.0 * (2.0 / 3.0),
            LfoSyncDivision::One4t => 1.0 * (2.0 / 3.0),
            LfoSyncDivision::One8t => 0.5 * (2.0 / 3.0),
            LfoSyncDivision::One16t => 0.25 * (2.0 / 3.0),
        }
    }

    pub fn to_rate_hz(self, tempo_bpm: f32) -> f32 {
        // rate = tempo / 60 * beats_per_cycle
        // For a quarter note at 120 BPM: 120/60 * 1 = 2 Hz
        let beats = self.to_multiplier();
        (tempo_bpm / 60.0) * beats
    }
}

// ---------------------------------------------------------------------------
// Step Sequencer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct StepSequencer {
    pub steps: [f32; 16],
    pub loop_start: usize,
    pub loop_end: usize,
    pub shuffle: f32,
    pub smoothness: f32,
    pub step_index: usize,
    pub step_changed: bool,
    shuffle_toggle: bool,
    last_value: f32,
    next_value: f32,
    step_fraction: f32,
}

impl Default for StepSequencer {
    fn default() -> Self {
        Self {
            steps: [0.0; 16],
            loop_start: 0,
            loop_end: 15,
            shuffle: 0.0,
            smoothness: 1.0,
            step_index: 0,
            step_changed: false,
            shuffle_toggle: false,
            last_value: 0.0,
            next_value: 0.0,
            step_fraction: 0.0,
        }
    }
}

impl StepSequencer {
    pub fn reset(&mut self) {
        self.step_index = self.loop_start;
        self.step_changed = false;
        self.shuffle_toggle = false;
        self.last_value = self.steps[self.loop_start];
        self.next_value = self.steps[self.loop_start];
        self.step_fraction = 0.0;
    }

    pub fn next(&mut self, phase: f32, _phase_inc: f32) -> f32 {
        let steps_total = (self.loop_end - self.loop_start + 1) as f32;
        let step_pos = phase * steps_total;

        // Apply shuffle (swing): delay odd-numbered steps
        let shuffled_step_pos = if self.shuffle > 0.0 {
            let pair_idx = (step_pos / 2.0).floor();
            let in_pair = step_pos - pair_idx * 2.0;
            let swing = 0.5 + self.shuffle * 0.5; // 0.5..1.0 for first step of pair
            let warped = if in_pair < 1.0 {
                in_pair * swing
            } else {
                swing + (in_pair - 1.0) * (2.0 - swing)
            };
            pair_idx * 2.0 + warped
        } else {
            step_pos
        };

        let target_step = (shuffled_step_pos as usize + self.loop_start).min(self.loop_end);
        let frac = shuffled_step_pos - target_step as f32;

        if target_step != self.step_index {
            self.step_index = target_step;
            self.step_changed = true;
            self.last_value = self.next_value;
            self.next_value = self.steps[target_step];
        } else {
            self.step_changed = false;
        }

        // Interpolation between steps controlled by smoothness
        // smoothness = 0.0: step/hold, 1.0: linear, >1.0: smooth cubic
        let interp_frac = if self.smoothness <= 0.0 {
            0.0
        } else if self.smoothness >= 1.0 {
            let s = self.smoothness.min(2.0) - 1.0;
            // Blend linear toward smoothstep
            let smooth = frac * frac * (3.0 - 2.0 * frac);
            frac * (1.0 - s) + smooth * s
        } else {
            frac * self.smoothness
        };
        self.last_value * (1.0 - interp_frac) + self.next_value * interp_frac
    }
}

// ---------------------------------------------------------------------------
// MSEG Curve Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsegCurve {
    Linear = 0,
    SlowEnd = 1,
    SlowStart = 2,
    Sqrt = 3,
    Cubic = 4,
    SmoothStep = 5,
    SCurve = 6,
    StepHold = 7,
    QuadraticBezier = 8,
    SineSegment = 9,
    Stairs = 10,
    Brownian = 11,
    SquareWave = 12,
    TriangleWave = 13,
    SawtoothWave = 14,
    Bump = 15,
}

impl MsegCurve {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => MsegCurve::SlowEnd,
            2 => MsegCurve::SlowStart,
            3 => MsegCurve::Sqrt,
            4 => MsegCurve::Cubic,
            5 => MsegCurve::SmoothStep,
            6 => MsegCurve::SCurve,
            7 => MsegCurve::StepHold,
            8 => MsegCurve::QuadraticBezier,
            9 => MsegCurve::SineSegment,
            10 => MsegCurve::Stairs,
            11 => MsegCurve::Brownian,
            12 => MsegCurve::SquareWave,
            13 => MsegCurve::TriangleWave,
            14 => MsegCurve::SawtoothWave,
            15 => MsegCurve::Bump,
            _ => MsegCurve::Linear,
        }
    }

    /// Apply curve shaping to a normalized 0..1 fraction.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            MsegCurve::Linear => t,
            MsegCurve::SlowEnd => t * t,
            MsegCurve::SlowStart => 1.0 - (1.0 - t) * (1.0 - t),
            MsegCurve::Sqrt => t.sqrt(),
            MsegCurve::Cubic => t * t * t,
            MsegCurve::SmoothStep => t * t * (3.0 - 2.0 * t),
            MsegCurve::SCurve => {
                // Ken Perlin's smootherstep
                t * t * t * (t * (6.0 * t - 15.0) + 10.0)
            }
            MsegCurve::StepHold => 0.0,
            MsegCurve::QuadraticBezier => {
                // Quadratic Bezier with control point at (0.5, 0.5)
                let mt = 1.0 - t;
                mt * mt * 0.0 + 2.0 * mt * t * 0.5 + t * t * 1.0
            }
            MsegCurve::SineSegment => ((t - 0.5) * PI).sin() * 0.5 + 0.5,
            MsegCurve::Stairs => {
                // Step function: 2–10 steps based on t
                let steps = 4.0f32;
                (t * steps).floor() / steps
            }
            MsegCurve::Brownian => {
                // Approximate Brownian bridge with deterministic noise
                let n1 = ((t * 7.3).sin() * (t * 13.7).cos()).abs();
                let n2 = ((t * 11.1).sin() * (t * 17.3).cos()).abs();
                t * (1.0 - n1) + n2 * 0.3
            }
            MsegCurve::SquareWave => {
                if t < 0.5 {
                    0.0
                } else {
                    1.0
                }
            }
            MsegCurve::TriangleWave => 1.0 - (2.0 * t - 1.0).abs(),
            MsegCurve::SawtoothWave => t,
            MsegCurve::Bump => (-((t - 0.5) * 4.0).powi(2)).exp(),
        }
    }
}

// ---------------------------------------------------------------------------
// MSEG Envelope
// ---------------------------------------------------------------------------

pub const MSEG_MAX_NODES: usize = 128;
pub const MSEG_MAX_SEGMENTS: usize = MSEG_MAX_NODES - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsegLoopMode {
    Loop = 0,
    OneShot = 1,
    GatedLoop = 2,
}

impl MsegLoopMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => MsegLoopMode::OneShot,
            2 => MsegLoopMode::GatedLoop,
            _ => MsegLoopMode::Loop,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MsegEnvelope {
    pub nodes: [f32; MSEG_MAX_NODES],
    pub curves: [MsegCurve; MSEG_MAX_SEGMENTS],
    pub loop_start: usize,
    pub loop_end: usize,
    pub loop_mode: MsegLoopMode,
}

impl Default for MsegEnvelope {
    fn default() -> Self {
        Self {
            nodes: [0.0; MSEG_MAX_NODES],
            curves: [MsegCurve::Linear; MSEG_MAX_SEGMENTS],
            loop_start: 0,
            loop_end: MSEG_MAX_NODES - 1,
            loop_mode: MsegLoopMode::Loop,
        }
    }
}

impl MsegEnvelope {
    pub fn current_segment(&self, phase: f32) -> usize {
        let loop_start = self.loop_start.min(MSEG_MAX_NODES - 1);
        let loop_end = self.loop_end.min(MSEG_MAX_NODES - 1).max(loop_start);
        let loop_len = loop_end.saturating_sub(loop_start);
        if loop_len == 0 {
            return 0;
        }
        let node_pos = loop_start as f32 + phase * loop_len as f32;
        let node_idx = node_pos as usize;
        node_idx.min(loop_end - 1).min(MSEG_MAX_SEGMENTS - 1)
    }

    pub fn evaluate(&self, phase: f32) -> f32 {
        let loop_start = self.loop_start.min(MSEG_MAX_NODES - 1);
        let loop_end = self.loop_end.min(MSEG_MAX_NODES - 1).max(loop_start);
        let loop_len = loop_end.saturating_sub(loop_start);
        if loop_len == 0 {
            return self.nodes[loop_start];
        }

        let node_pos = loop_start as f32 + phase * loop_len as f32;
        let node_idx = node_pos as usize;
        let frac = node_pos - node_idx as f32;
        let node_idx = node_idx.min(loop_end - 1);
        let next_idx = (node_idx + 1).min(loop_end);

        let y0 = self.nodes[node_idx];
        let y1 = self.nodes[next_idx];

        // Apply per-segment curve
        let seg_idx = node_idx.min(MSEG_MAX_SEGMENTS - 1);
        let curved_frac = self.curves[seg_idx].apply(frac);

        y0 * (1.0 - curved_frac) + y1 * curved_frac
    }
}

// ---------------------------------------------------------------------------
// LFO
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Lfo {
    sample_rate: f32,
    phase: f32,
    rate_hz: f32,
    shape: LfoShape,
    pub trigger_mode: LfoTriggerMode,
    pub sync_mode: LfoSyncMode,
    pub sync_division: LfoSyncDivision,
    pub amount: f32,
    pub start_phase: f32,
    deform: f32,
    deform_type: u8,
    unipolar: bool,

    // State
    last_value: f32,
    noise_state: f32,
    noise_target: f32,
    noise_smooth: f32,

    // Envelope state
    env_phase: f32,
    env_state: EnvState,
    env_delay: f32,
    env_attack: f32,
    env_hold: f32,
    env_decay: f32,
    env_sustain: f32,
    env_release: f32,
    env_release_start_level: f32,
    gate: bool,

    // Step sequencer
    pub stepseq: StepSequencer,
    pub step_changed: bool,

    // MSEG envelope
    pub mseg: MsegEnvelope,

    // Phase offset for 0-rate scrubbing / phase modulation
    pub phase_offset: f32,

    // Host sync
    song_pos_beats: f64,

    // Tempo sync
    tempo_bpm: f32,
    env_tempo_sync: bool,

    // MSEG independent phase for loop modes
    mseg_phase: f32,
    pub mseg_seg_changed: bool,
    pub mseg_prev_seg: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvState {
    Delay,
    Attack,
    Hold,
    Sustain,
    Release,
    Idle,
}

impl Lfo {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            phase: 0.0,
            rate_hz: 1.0,
            shape: LfoShape::Sine,
            trigger_mode: LfoTriggerMode::KeyTrigger,
            sync_mode: LfoSyncMode::Free,
            sync_division: LfoSyncDivision::One4,
            amount: 0.0,
            start_phase: 0.0,
            deform: 0.0,
            deform_type: 0,
            unipolar: false,
            last_value: 0.0,
            noise_state: 0.0,
            noise_target: 0.0,
            noise_smooth: 0.0,
            env_phase: 0.0,
            env_state: EnvState::Idle,
            env_delay: 0.0,
            env_attack: 0.01,
            env_hold: 0.0,
            env_decay: 0.2,
            env_sustain: 1.0,
            env_release: 0.3,
            env_release_start_level: 0.0,
            gate: false,
            stepseq: StepSequencer::default(),
            step_changed: false,
            mseg: MsegEnvelope::default(),
            phase_offset: 0.0,
            song_pos_beats: 0.0,
            tempo_bpm: 120.0,
            env_tempo_sync: false,
            mseg_phase: 0.0,
            mseg_seg_changed: false,
            mseg_prev_seg: 0,
        }
    }

    pub fn set_rate_hz(&mut self, rate: f32) {
        self.rate_hz = rate.max(0.001);
    }

    pub fn set_shape(&mut self, shape: LfoShape) {
        self.shape = shape;
    }

    pub fn set_deform(&mut self, deform: f32) {
        self.deform = deform.clamp(-1.0, 1.0);
    }

    pub fn set_deform_type(&mut self, deform_type: u8) {
        self.deform_type = deform_type.min(2);
    }

    pub fn set_amount(&mut self, amount: f32) {
        self.amount = amount;
    }

    pub fn set_unipolar(&mut self, unipolar: bool) {
        self.unipolar = unipolar;
    }

    pub fn set_sync_mode(&mut self, mode: LfoSyncMode) {
        self.sync_mode = mode;
    }

    pub fn set_sync_division(&mut self, division: LfoSyncDivision) {
        self.sync_division = division;
    }

    pub fn set_start_phase(&mut self, phase: f32) {
        self.start_phase = phase.clamp(0.0, 1.0);
    }

    pub fn set_trigger_mode(&mut self, mode: LfoTriggerMode) {
        self.trigger_mode = mode;
    }

    pub fn set_tempo(&mut self, tempo_bpm: f32) {
        self.tempo_bpm = tempo_bpm;
        if self.sync_mode == LfoSyncMode::Tempo {
            self.rate_hz = self.sync_division.to_rate_hz(tempo_bpm).max(0.001);
        }
    }

    pub fn set_env_tempo_sync(&mut self, sync: bool) {
        self.env_tempo_sync = sync;
    }

    pub fn set_song_pos_beats(&mut self, pos: f64) {
        self.song_pos_beats = pos;
    }

    pub fn set_env_params(
        &mut self,
        delay: f32,
        attack: f32,
        hold: f32,
        decay: f32,
        sustain: f32,
        release: f32,
    ) {
        self.env_delay = delay.max(0.0);
        self.env_attack = attack.max(0.0);
        self.env_hold = hold.max(0.0);
        self.env_decay = decay.max(0.0);
        self.env_sustain = sustain.clamp(0.0, 1.0);
        self.env_release = release.max(0.0);
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.phase = match self.trigger_mode {
            LfoTriggerMode::FreeRun => self.phase,
            LfoTriggerMode::KeyTrigger => self.start_phase,
            LfoTriggerMode::Random => random::<f32>(),
        };
        self.env_phase = 0.0;
        self.env_state = EnvState::Delay;
        self.env_release_start_level = 0.0;
        self.gate = true;
        self.step_changed = false;
        self.phase_offset = 0.0;
        self.stepseq.reset();
        self.mseg_phase = 0.0;
    }

    pub fn release(&mut self) {
        self.gate = false;
        if self.env_state != EnvState::Idle {
            self.env_release_start_level = self.get_env_value();
            self.env_state = EnvState::Release;
            self.env_phase = 0.0;
        }
    }

    /// Current LFO value without advancing.
    pub fn value(&self) -> f32 {
        self.last_value
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f32 {
        let phase_inc = self.rate_hz / self.sample_rate;
        // Host-synced FreeRun: derive phase from transport when in FreeRun + Tempo
        if self.trigger_mode == LfoTriggerMode::FreeRun && self.sync_mode == LfoSyncMode::Tempo {
            let beats_per_cycle = self.sync_division.to_multiplier();
            self.phase = ((self.song_pos_beats as f32 * beats_per_cycle) + self.start_phase) % 1.0;
            if self.phase < 0.0 {
                self.phase += 1.0;
            }
        } else {
            self.phase += phase_inc;
            while self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }

        let effective_phase = (self.phase + self.phase_offset).fract();
        let effective_phase = if effective_phase < 0.0 {
            effective_phase + 1.0
        } else {
            effective_phase
        };

        let raw = match self.shape {
            LfoShape::Sine => {
                let angle = effective_phase * 2.0 * PI;
                let mut v = angle.sin();
                if self.deform != 0.0 {
                    let d = self.deform;
                    match self.deform_type {
                        1 => {
                            // Sinusoidal bend
                            v = v + d * (v * PI).sin() * 0.5;
                        }
                        2 => {
                            // Offset sinusoidal
                            v *= 1.0 + d * (angle * 2.0).cos();
                        }
                        _ => {
                            // Quadratic warping
                            let a = d * 0.5;
                            v = v - a * v * v + a;
                            v = v - a * v * v + a;
                        }
                    }
                }
                v
            }
            LfoShape::Triangle => {
                let p = effective_phase;
                let tri = if p < 0.25 {
                    4.0 * p
                } else if p < 0.75 {
                    2.0 - 4.0 * p
                } else {
                    4.0 * p - 4.0
                };
                if self.deform != 0.0 {
                    let d = self.deform;
                    match self.deform_type {
                        1 => {
                            // Quadratic bend
                            tri.signum() * tri.abs().powf(1.0 + d)
                        }
                        2 => {
                            // Cubic bend
                            tri.signum() * tri.abs().powf(1.0 + d * 2.0)
                        }
                        _ => {
                            // Triangle→saw morph
                            let saw = 2.0 * p - 1.0;
                            tri + (saw - tri) * d
                        }
                    }
                } else {
                    tri
                }
            }
            LfoShape::Saw => {
                let p = effective_phase;
                let mut v = 2.0 * p - 1.0;
                if self.deform != 0.0 {
                    let d = self.deform;
                    v = match self.deform_type {
                        1 => {
                            // Power curve (symmetric)
                            v.signum() * v.abs().powf(1.0 - d * 0.5)
                        }
                        2 => {
                            // Step approximation: quantize to N steps
                            let steps = (2.0 + d.abs() * 14.0).max(2.0);
                            (v * steps).round() / steps
                        }
                        _ => {
                            // Exponential curve shaping
                            if d > 0.0 {
                                let a = d;
                                if v >= 0.0 {
                                    v.powf(1.0 - a * 0.5)
                                } else {
                                    -((-v).powf(1.0 - a * 0.5))
                                }
                            } else {
                                let a = -d;
                                v.signum() * v.abs().powf(1.0 + a * 2.0)
                            }
                        }
                    };
                }
                v
            }
            LfoShape::Ramp => 1.0 - 2.0 * effective_phase,
            LfoShape::Square => {
                let pw = 0.5 + self.deform * 0.5;
                if effective_phase < pw.clamp(0.01, 0.99) {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoShape::SampleHold => {
                if phase_inc > effective_phase {
                    self.last_value = random::<f32>() * 2.0 - 1.0;
                }
                self.last_value
            }
            LfoShape::Noise => {
                if self.noise_smooth <= 0.0 {
                    self.noise_target = random::<f32>() * 2.0 - 1.0;
                    self.noise_smooth = 1.0;
                }
                let d = self.deform;
                let step = match self.deform_type {
                    1 => {
                        // Interpolated glide: slower, smoother transitions
                        let correlation = 1.0 * (1.0 + d.abs() * 3.0);
                        phase_inc * correlation
                    }
                    2 => {
                        // Brownian walk: small random steps
                        let correlation = 8.0 * (1.0 + d.abs());
                        phase_inc * correlation
                    }
                    _ => {
                        // Correlated noise
                        let correlation = 4.0 * (1.0 + d * 0.75);
                        phase_inc * correlation
                    }
                };
                self.noise_smooth -= step;
                if self.noise_smooth < 0.0 {
                    self.noise_smooth = 0.0;
                }
                self.noise_state += (self.noise_target - self.noise_state) * step.min(1.0);
                self.noise_state
            }
            LfoShape::Envelope => {
                self.update_env();
                let env = self.get_env_value();
                let deform = self.deform.clamp(-1.0, 1.0);
                if deform.abs() > 0.001 {
                    match self.deform_type {
                        1 => {
                            // Exponential warp
                            let exp = (deform * 3.0).exp();
                            1.0 - (-env * exp).exp()
                        }
                        2 => {
                            // Noise perturbation: add jitter to envelope
                            let jitter = (random::<f32>() - 0.5) * deform.abs() * 0.2;
                            (env + jitter).clamp(0.0, 1.0)
                        }
                        _ => {
                            // Power function warp
                            let exp = 1.0 + deform * 2.0;
                            if exp > 0.0 { env.powf(exp) } else { env }
                        }
                    }
                } else {
                    env
                }
            }
            LfoShape::StepSeq => {
                // Deform controls step sequencer smoothness: -1..1 → 0..2
                self.stepseq.smoothness = 1.0 + self.deform;
                let v = self.stepseq.next(effective_phase, phase_inc);
                self.step_changed = self.stepseq.step_changed;
                v
            }
            LfoShape::Mseg => {
                // Advance MSEG phase independently with loop mode support
                let phase_inc = self.rate_hz / self.sample_rate;
                match self.mseg.loop_mode {
                    MsegLoopMode::Loop => {
                        self.mseg_phase += phase_inc;
                        while self.mseg_phase >= 1.0 {
                            self.mseg_phase -= 1.0;
                        }
                    }
                    MsegLoopMode::OneShot => {
                        self.mseg_phase += phase_inc;
                        if self.mseg_phase > 1.0 {
                            self.mseg_phase = 1.0;
                        }
                    }
                    MsegLoopMode::GatedLoop => {
                        if self.gate {
                            self.mseg_phase += phase_inc;
                            while self.mseg_phase >= 1.0 {
                                self.mseg_phase -= 1.0;
                            }
                        } else {
                            // After release, play to end and hold
                            self.mseg_phase += phase_inc;
                            if self.mseg_phase > 1.0 {
                                self.mseg_phase = 1.0;
                            }
                        }
                    }
                }
                let seg = self.mseg.current_segment(self.mseg_phase);
                self.mseg_seg_changed = seg != self.mseg_prev_seg;
                self.mseg_prev_seg = seg;
                self.mseg.evaluate(self.mseg_phase)
            }
        };

        // Apply envelope if not envelope shape
        let env_val = if self.shape != LfoShape::Envelope {
            self.update_env();
            self.get_env_value()
        } else {
            1.0
        };

        let mut out = raw * env_val * self.amount;
        if self.unipolar {
            out = (out + 1.0) * 0.5;
        }
        out
    }

    fn update_env(&mut self) {
        let dt = 1.0 / self.sample_rate;
        // Convert beat values to seconds if tempo sync is enabled
        let beat_to_sec = |beats: f32| -> f32 {
            if self.env_tempo_sync && self.tempo_bpm > 0.0 {
                beats * 60.0 / self.tempo_bpm
            } else {
                beats
            }
        };
        let env_delay = beat_to_sec(self.env_delay);
        let env_attack = beat_to_sec(self.env_attack);
        let env_hold = beat_to_sec(self.env_hold);
        let _env_decay = beat_to_sec(self.env_decay);
        let env_release = beat_to_sec(self.env_release);

        match self.env_state {
            EnvState::Delay => {
                if env_delay > 0.0 {
                    self.env_phase += dt / env_delay;
                    if self.env_phase >= 1.0 {
                        self.env_phase = 0.0;
                        self.env_state = EnvState::Attack;
                    }
                } else {
                    self.env_state = EnvState::Attack;
                }
            }
            EnvState::Attack => {
                if env_attack > 0.0 {
                    self.env_phase += dt / env_attack;
                    if self.env_phase >= 1.0 {
                        self.env_phase = 0.0;
                        self.env_state = EnvState::Hold;
                    }
                } else {
                    self.env_state = EnvState::Hold;
                }
            }
            EnvState::Hold => {
                if env_hold > 0.0 {
                    self.env_phase += dt / env_hold;
                    if self.env_phase >= 1.0 {
                        self.env_phase = 0.0;
                        self.env_state = EnvState::Sustain;
                    }
                } else {
                    self.env_state = EnvState::Sustain;
                }
            }
            EnvState::Sustain => {
                if !self.gate {
                    self.env_state = EnvState::Release;
                    self.env_phase = 0.0;
                }
            }
            EnvState::Release => {
                if env_release > 0.0 {
                    self.env_phase += dt / env_release;
                    if self.env_phase >= 1.0 {
                        self.env_state = EnvState::Idle;
                    }
                } else {
                    self.env_state = EnvState::Idle;
                }
            }
            EnvState::Idle => {}
        }
    }

    fn get_env_value(&self) -> f32 {
        match self.env_state {
            EnvState::Delay => 0.0,
            EnvState::Attack => self.env_phase,
            EnvState::Hold => 1.0,
            EnvState::Sustain => 1.0,
            EnvState::Release => self.env_release_start_level * (1.0 - self.env_phase),
            EnvState::Idle => 0.0,
        }
    }

    pub fn process_block(&mut self, out: &mut [f32]) {
        for sample in out.iter_mut() {
            *sample = self.next();
        }
    }
}
