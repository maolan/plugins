#![allow(dead_code)]

//! ADSR envelope generator.
//!
//! Based on Surge XT's digital envelope model with exponential segments.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeMode {
    Digital,
    Analog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeRetriggerMode {
    Reset = 0,
    Continue = 1,
}

impl EnvelopeRetriggerMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => EnvelopeRetriggerMode::Reset,
            _ => EnvelopeRetriggerMode::Continue,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackShape {
    Convex = 0,
    Linear = 1,
    Concave = 2,
}

impl AttackShape {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => AttackShape::Linear,
            2 => AttackShape::Concave,
            _ => AttackShape::Convex,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecayReleaseShape {
    Linear = 0,
    Quadratic = 1,
    Cubic = 2,
}

impl DecayReleaseShape {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => DecayReleaseShape::Quadratic,
            2 => DecayReleaseShape::Cubic,
            _ => DecayReleaseShape::Linear,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AdsrEnvelope {
    sample_rate: f32,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    mode: EnvelopeMode,
    attack_shape: AttackShape,
    decay_shape: DecayReleaseShape,
    release_shape: DecayReleaseShape,
    retrigger_mode: EnvelopeRetriggerMode,
    tempo_sync: bool,
    tempo_bpm: f32,
    uber_release: f32,
    gated_release: bool,
    correct_analog_mode: bool,

    // State
    phase: f32,
    decay_phase: f32,
    release_phase: f32,
    output: f32,
    gate: bool,
    released: bool,
    release_start_level: f32,
}

impl AdsrEnvelope {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            attack: 0.0,
            decay: 0.0,
            sustain: 1.0,
            release: 0.0,
            mode: EnvelopeMode::Digital,
            attack_shape: AttackShape::Convex,
            decay_shape: DecayReleaseShape::Linear,
            release_shape: DecayReleaseShape::Linear,
            retrigger_mode: EnvelopeRetriggerMode::Reset,
            tempo_sync: false,
            tempo_bpm: 120.0,
            uber_release: 0.0,
            gated_release: false,
            correct_analog_mode: false,
            phase: 0.0,
            decay_phase: 0.0,
            release_phase: 0.0,
            output: 0.0,
            gate: false,
            released: false,
            release_start_level: 0.0,
        }
    }

    pub fn set_params(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.attack = attack.max(0.0);
        self.decay = decay.max(0.0);
        self.sustain = sustain.clamp(0.0, 1.0);
        self.release = release.max(0.0);
    }

    pub fn set_mode(&mut self, mode: EnvelopeMode) {
        self.mode = mode;
    }

    pub fn set_shapes(&mut self, attack: AttackShape, decay: DecayReleaseShape, release: DecayReleaseShape) {
        self.attack_shape = attack;
        self.decay_shape = decay;
        self.release_shape = release;
    }

    pub fn set_retrigger_mode(&mut self, mode: EnvelopeRetriggerMode) {
        self.retrigger_mode = mode;
    }

    pub fn set_attack(&mut self, attack: f32) {
        self.attack = attack.max(0.0);
    }

    pub fn set_decay(&mut self, decay: f32) {
        self.decay = decay.max(0.0);
    }

    pub fn set_sustain(&mut self, sustain: f32) {
        self.sustain = sustain.clamp(0.0, 1.0);
    }

    pub fn set_release(&mut self, release: f32) {
        self.release = release.max(0.0);
    }

    pub fn set_tempo_sync(&mut self, sync: bool) {
        self.tempo_sync = sync;
    }

    pub fn set_tempo(&mut self, tempo_bpm: f32) {
        self.tempo_bpm = tempo_bpm;
    }

    pub fn set_uber_release(&mut self, uber: f32) {
        self.uber_release = uber.clamp(0.0, 1.0);
    }

    pub fn set_gated_release(&mut self, gated: bool) {
        self.gated_release = gated;
    }

    pub fn set_correct_analog_mode(&mut self, v: bool) {
        self.correct_analog_mode = v;
    }

    #[allow(dead_code)]
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.decay_phase = 0.0;
        self.release_phase = 0.0;
        self.output = 0.0;
        self.gate = false;
        self.released = false;
        self.release_start_level = 0.0;
    }

    pub fn trigger(&mut self) {
        self.gate = true;
        self.released = false;
        match self.retrigger_mode {
            EnvelopeRetriggerMode::Reset => {
                self.phase = 0.0;
                self.decay_phase = 0.0;
                self.release_phase = 0.0;
                self.output = 0.0;
            }
            EnvelopeRetriggerMode::Continue => {
                self.phase = 0.0;
                self.decay_phase = 0.0;
                self.release_phase = 0.0;
                // Scale phase so attack continues from current level
                if self.output > 0.0 && self.attack > 0.0 {
                    self.phase = self.output.clamp(0.0, 1.0);
                }
            }
        }
    }

    pub fn release(&mut self) {
        self.gate = false;
        if !self.gated_release {
            self.released = true;
            self.release_start_level = self.output;
            self.release_phase = 0.0;
        }
    }

    /// Process one sample and return the envelope value.
    pub fn next(&mut self) -> f32 {
        let beat_to_sec = |beats: f32| -> f32 {
            if self.tempo_sync && self.tempo_bpm > 0.0 {
                beats * 60.0 / self.tempo_bpm
            } else {
                beats
            }
        };
        let attack = beat_to_sec(self.attack);
        let decay = beat_to_sec(self.decay);
        let release = beat_to_sec(self.release);

        if self.gate || (self.gated_release && !self.released) {
            // Attack phase
            if self.phase < 1.0 {
                if attack <= 0.0 {
                    self.phase = 1.0;
                    self.output = 1.0;
                } else if self.mode == EnvelopeMode::Analog {
                    // Capacitor charging model: exponential approach to 1.0
                    let time_const = if self.correct_analog_mode { 7.0 } else { 5.0 };
                    let attack_coef = 1.0 - (-time_const / (attack * self.sample_rate)).exp();
                    self.output += (1.0 - self.output) * attack_coef;
                    if self.output >= 0.999 {
                        self.output = 1.0;
                        self.phase = 1.0;
                    }
                } else {
                    let attack_rate = 1.0 / (attack * self.sample_rate);
                    self.phase += attack_rate;
                    if self.phase >= 1.0 {
                        self.phase = 1.0;
                        self.output = 1.0;
                    } else {
                        self.output = self.attack_shape_value(self.phase);
                    }
                }
            } else {
                // Decay phase
                let decay_rate = if decay > 0.0 {
                    1.0 / (decay * self.sample_rate)
                } else {
                    1.0
                };
                let target = self.sustain;
                if self.mode == EnvelopeMode::Analog {
                    // Exponential decay toward sustain
                    let time_const = if self.correct_analog_mode { 7.0 } else { 5.0 };
                    let coef = (-decay_rate * time_const).exp();
                    self.output = target + (self.output - target) * coef;
                } else {
                    // Digital mode with curve shaping
                    self.decay_phase += decay_rate;
                    if self.decay_phase >= 1.0 {
                        self.decay_phase = 1.0;
                        self.output = target;
                    } else {
                        let shaped = self.decay_shape_value(self.decay_phase);
                        self.output = target + (1.0 - target) * (1.0 - shaped);
                    }
                }
            }
        } else if self.released {
            // Release phase
            if self.release <= 0.0 || self.output <= 0.0 {
                self.output = 0.0;
                self.released = false;
            } else {
                let release_rate = 1.0 / (release * self.sample_rate);
                let uber_mul = 1.0 + self.uber_release * 9.0;
                let threshold = self.release_start_level * 0.1;
                let use_uber = self.uber_release > 0.0 && self.output < threshold;
                if self.mode == EnvelopeMode::Analog {
                    let time_const = if self.correct_analog_mode { 7.0 } else { 5.0 };
                    let coef = if use_uber {
                        (-release_rate * time_const * uber_mul).exp()
                    } else {
                        (-release_rate * time_const).exp()
                    };
                    self.output *= coef;
                } else {
                    // Digital mode with curve shaping
                    let effective_rate = if use_uber {
                        release_rate * uber_mul
                    } else {
                        release_rate
                    };
                    self.release_phase += effective_rate;
                    if self.release_phase >= 1.0 {
                        self.output = 0.0;
                        self.released = false;
                    } else {
                        let shaped = self.release_shape_value(self.release_phase);
                        self.output = self.release_start_level * (1.0 - shaped);
                    }
                }
            }
        } else {
            self.output = 0.0;
        }
        self.output
    }

    /// Process a block of samples into `out`.
    #[allow(dead_code)]
    pub fn process_block(&mut self, out: &mut [f32]) {
        for sample in out.iter_mut() {
            *sample = self.next();
        }
    }

    pub fn is_active(&self) -> bool {
        self.output > 1.0e-6 || self.gate
    }

    fn attack_shape_value(&self, phase: f32) -> f32 {
        let p = phase.clamp(0.0, 1.0);
        match self.attack_shape {
            AttackShape::Convex => p.sqrt(),
            AttackShape::Linear => p,
            AttackShape::Concave => p * p,
        }
    }

    fn decay_shape_value(&self, phase: f32) -> f32 {
        let p = phase.clamp(0.0, 1.0);
        match self.decay_shape {
            DecayReleaseShape::Linear => p,
            DecayReleaseShape::Quadratic => p * p,
            DecayReleaseShape::Cubic => p * p * p,
        }
    }

    fn release_shape_value(&self, phase: f32) -> f32 {
        let p = phase.clamp(0.0, 1.0);
        match self.release_shape {
            DecayReleaseShape::Linear => p,
            DecayReleaseShape::Quadratic => p * p,
            DecayReleaseShape::Cubic => p * p * p,
        }
    }
}
