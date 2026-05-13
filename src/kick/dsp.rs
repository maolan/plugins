//! Maolan Kick — kick drum synthesizer DSP core.
//!
//! Architecture: offline synthesis model (same as geonkick).
//! On MIDI note-on the kick is rendered into a buffer; the audio thread
//! plays back from that buffer.  Double-buffered atomic swap keeps the
//! audio and synthesis threads independent.

use crate::{kick::simd_kick, simd};

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct EnvPoint {
    pub t: f32, // 0..1 normalized time within the kick duration
    pub v: f32, // 0..1 normalized value
}

impl EnvPoint {
    pub const fn new(t: f32, v: f32) -> Self {
        Self { t, v }
    }
}

/// Multi-point envelope evaluated with linear interpolation.
#[derive(Debug, Clone)]
pub struct Envelope {
    points: Vec<EnvPoint>,
}

impl Default for Envelope {
    fn default() -> Self {
        Self {
            points: vec![EnvPoint::new(0.0, 1.0), EnvPoint::new(1.0, 0.0)],
        }
    }
}

impl Envelope {
    pub fn new(points: Vec<EnvPoint>) -> Self {
        let mut env = Self { points };
        env.sort_and_dedup();
        env
    }

    pub fn with_default_adsr(attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        let total = attack + decay + release;
        if total <= 0.0 {
            return Self::default();
        }
        let mut points = vec![
            EnvPoint::new(0.0, 0.0),
            EnvPoint::new(attack / total, 1.0),
            EnvPoint::new((attack + decay) / total, sustain.clamp(0.0, 1.0)),
            EnvPoint::new(1.0, 0.0),
        ];
        if attack <= 0.0 {
            points.remove(0);
        }
        Self::new(points)
    }

    fn sort_and_dedup(&mut self) {
        self.points.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
        self.points.dedup_by(|a, b| (a.t - b.t).abs() < 1.0e-6);
    }

    /// Evaluate envelope at normalized time `t` (0..1).
    pub fn value(&self, t: f32) -> f32 {
        if self.points.is_empty() {
            return 0.0;
        }
        if t <= self.points[0].t {
            return self.points[0].v;
        }
        if t >= self.points.last().unwrap().t {
            return self.points.last().unwrap().v;
        }
        for i in 1..self.points.len() {
            let p1 = &self.points[i - 1];
            let p2 = &self.points[i];
            if t >= p1.t && t <= p2.t {
                let dt = p2.t - p1.t;
                if dt < 1.0e-9 {
                    return p1.v;
                }
                let frac = (t - p1.t) / dt;
                return p1.v + frac * (p2.v - p1.v);
            }
        }
        self.points.last().unwrap().v
    }

    /// Fill `out` with envelope values for each sample.
    /// `dt_per_sample` is the normalized time step per sample (= 1.0 / num_samples).
    pub fn fill_buffer(&self, out: &mut [f32], dt_per_sample: f32) {
        for (i, s) in out.iter_mut().enumerate() {
            *s = self.value(i as f32 * dt_per_sample);
        }
    }
}

// ---------------------------------------------------------------------------
// Waveform
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Waveform {
    Sine = 0,
    Square = 1,
    Triangle = 2,
    Saw = 3,
}

impl Waveform {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Waveform::Square,
            2 => Waveform::Triangle,
            3 => Waveform::Saw,
            _ => Waveform::Sine,
        }
    }
}

/// Phase-accumulator oscillator with pitch and amplitude envelopes.
pub struct Oscillator {
    pub waveform: Waveform,
    pub base_freq_hz: f32,
    pub amplitude: f32,
    pub phase: f32,
    pub pitch_env: Envelope,
    pub amp_env: Envelope,
    pub sample_rate: f32,
}

impl Oscillator {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            waveform: Waveform::Sine,
            base_freq_hz: 150.0,
            amplitude: 0.8,
            phase: 0.0,
            pitch_env: Envelope::with_default_adsr(0.001, 0.08, 0.0, 0.05),
            amp_env: Envelope::with_default_adsr(0.001, 0.2, 0.0, 0.05),
            sample_rate,
        }
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Render the oscillator into `out` for `num_samples`.
    pub fn render(&mut self, out: &mut [f32], num_samples: usize) {
        let dt = 1.0 / num_samples.max(1) as f32;
        let mut pitch_buf = vec![0.0f32; num_samples];
        let mut amp_buf = vec![0.0f32; num_samples];
        self.pitch_env.fill_buffer(&mut pitch_buf, dt);
        self.amp_env.fill_buffer(&mut amp_buf, dt);

        let two_pi = 2.0 * std::f32::consts::PI;
        let sr = self.sample_rate;
        let base = self.base_freq_hz;
        let amp_scale = self.amplitude;

        match self.waveform {
            Waveform::Sine => {
                self.render_sine(
                    out,
                    num_samples,
                    &pitch_buf,
                    &amp_buf,
                    base,
                    sr,
                    two_pi,
                    amp_scale,
                );
            }
            Waveform::Square => {
                for i in 0..num_samples {
                    let freq = base * pitch_buf[i];
                    let phase_inc = freq * two_pi / sr;
                    self.phase += phase_inc;
                    while self.phase > two_pi {
                        self.phase -= two_pi;
                    }
                    let raw = if self.phase < std::f32::consts::PI {
                        1.0
                    } else {
                        -1.0
                    };
                    out[i] = raw * amp_scale * amp_buf[i];
                }
            }
            Waveform::Triangle => {
                for i in 0..num_samples {
                    let freq = base * pitch_buf[i];
                    let phase_inc = freq * two_pi / sr;
                    self.phase += phase_inc;
                    while self.phase > two_pi {
                        self.phase -= two_pi;
                    }
                    let p = self.phase;
                    let raw = if p < std::f32::consts::PI {
                        -1.0 + (2.0 / std::f32::consts::PI) * p
                    } else {
                        3.0 - (2.0 / std::f32::consts::PI) * p
                    };
                    out[i] = raw * amp_scale * amp_buf[i];
                }
            }
            Waveform::Saw => {
                for i in 0..num_samples {
                    let freq = base * pitch_buf[i];
                    let phase_inc = freq * two_pi / sr;
                    self.phase += phase_inc;
                    while self.phase > two_pi {
                        self.phase -= two_pi;
                    }
                    let p = self.phase;
                    let raw = if p < std::f32::consts::PI {
                        (1.0 / std::f32::consts::PI) * p
                    } else {
                        (1.0 / std::f32::consts::PI) * p - 2.0
                    };
                    out[i] = raw * amp_scale * amp_buf[i];
                }
            }
        }
    }

    /// SIMD-accelerated sine rendering using `wide::f32x4`.
    #[allow(clippy::too_many_arguments)]
    fn render_sine(
        &mut self,
        out: &mut [f32],
        num_samples: usize,
        pitch_buf: &[f32],
        amp_buf: &[f32],
        base: f32,
        sr: f32,
        two_pi: f32,
        amp_scale: f32,
    ) {
        let mut phases = vec![0.0f32; num_samples];
        for i in 0..num_samples {
            let freq = base * pitch_buf[i];
            let phase_inc = freq * two_pi / sr;
            self.phase += phase_inc;
            while self.phase > two_pi {
                self.phase -= two_pi;
            }
            phases[i] = self.phase;
        }

        // SIMD sin computation with wide::f32x4
        let mut i = 0;
        while i + 4 <= num_samples {
            let p = wide::f32x4::from([phases[i], phases[i + 1], phases[i + 2], phases[i + 3]]);
            let a = wide::f32x4::from([amp_buf[i], amp_buf[i + 1], amp_buf[i + 2], amp_buf[i + 3]]);
            let s = p.sin();
            let r = s * a * wide::f32x4::splat(amp_scale);
            let arr = r.to_array();
            out[i] = arr[0];
            out[i + 1] = arr[1];
            out[i + 2] = arr[2];
            out[i + 3] = arr[3];
            i += 4;
        }
        for j in i..num_samples {
            out[j] = phases[j].sin() * amp_scale * amp_buf[j];
        }
    }
}

// ---------------------------------------------------------------------------
// Noise
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NoiseType {
    White = 0,
    Pink = 1,
}

impl NoiseType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => NoiseType::Pink,
            _ => NoiseType::White,
        }
    }
}

/// xoshiro128+ PRNG (fast, good quality for audio).
pub struct Rng {
    s: [u32; 4],
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut s = [0u32; 4];
        let mut z = seed.wrapping_add(0x9e3779b97f4a7c15);
        for item in &mut s {
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            z = z ^ (z >> 31);
            *item = z as u32;
        }
        Self { s }
    }

    #[inline]
    fn next_u32(&mut self) -> u32 {
        let result = self.s[0].wrapping_add(self.s[3]);
        let t = self.s[1] << 9;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(11);
        result
    }

    /// Next f32 in [-1, 1].
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Paul Kellet's economy pinking filter.
pub struct PinkNoise {
    b0: f32,
    b1: f32,
    b2: f32,
    b3: f32,
    b4: f32,
    b5: f32,
    b6: f32,
}

impl Default for PinkNoise {
    fn default() -> Self {
        Self {
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            b3: 0.0,
            b4: 0.0,
            b5: 0.0,
            b6: 0.0,
        }
    }
}

impl PinkNoise {
    #[inline]
    pub fn next(&mut self, white: f32) -> f32 {
        self.b0 = 0.99886 * self.b0 + white * 0.0555179;
        self.b1 = 0.99332 * self.b1 + white * 0.0750759;
        self.b2 = 0.96900 * self.b2 + white * 0.153_852;
        self.b3 = 0.86650 * self.b3 + white * 0.3104856;
        self.b4 = 0.55000 * self.b4 + white * 0.5329522;
        self.b5 = -0.7616 * self.b5 - white * 0.0168980;
        let out =
            self.b0 + self.b1 + self.b2 + self.b3 + self.b4 + self.b5 + self.b6 + white * 0.5362;
        self.b6 = white * 0.115926;
        out * 0.11 // normalize roughly to [-1, 1]
    }
}

/// Noise generator with selectable white/pink noise, amplitude envelope and optional filter.
pub struct NoiseGenerator {
    pub amplitude: f32,
    pub density: f32,
    pub noise_type: NoiseType,
    pub amp_env: Envelope,
    rng: Rng,
    pink: PinkNoise,
}

impl NoiseGenerator {
    pub fn new() -> Self {
        Self {
            amplitude: 0.3,
            density: 0.5,
            noise_type: NoiseType::White,
            amp_env: Envelope::with_default_adsr(0.0, 0.03, 0.0, 0.02),
            rng: Rng::new(0x123456789ABCDEF0),
            pink: PinkNoise::default(),
        }
    }

    pub fn reset(&mut self) {
        self.rng = Rng::new(0x123456789ABCDEF0);
        self.pink = PinkNoise::default();
    }

    pub fn render(&mut self, out: &mut [f32], num_samples: usize) {
        let dt = 1.0 / num_samples.max(1) as f32;
        let mut env_buf = vec![0.0f32; num_samples];
        self.amp_env.fill_buffer(&mut env_buf, dt);

        let thresh = (1.0 - self.density) * u32::MAX as f32;
        for i in 0..num_samples {
            let sample = if self.rng.next_u32() as f32 >= thresh {
                let white = self.rng.next_f32();
                match self.noise_type {
                    NoiseType::White => white,
                    NoiseType::Pink => self.pink.next(white),
                }
            } else {
                0.0
            };
            out[i] = sample * self.amplitude * env_buf[i];
        }
    }
}

// ---------------------------------------------------------------------------
// Biquad Filter
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FilterType {
    Lowpass = 0,
    Highpass = 1,
    Bandpass = 2,
}

impl FilterType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => FilterType::Highpass,
            2 => FilterType::Bandpass,
            _ => FilterType::Lowpass,
        }
    }
}

/// Standard biquad IIR filter (Direct Form 1).
#[derive(Debug, Clone, Copy)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
}

impl Biquad {
    pub fn set_lowpass(&mut self, cutoff_hz: f32, sample_rate: f32, q: f32) {
        let w0 = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q.max(1.0e-6));
        let a0 = 1.0 + alpha;
        self.b0 = ((1.0 - cw) * 0.5) / a0;
        self.b1 = (1.0 - cw) / a0;
        self.b2 = ((1.0 - cw) * 0.5) / a0;
        self.a1 = (-2.0 * cw) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    pub fn set_highpass(&mut self, cutoff_hz: f32, sample_rate: f32, q: f32) {
        let w0 = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q.max(1.0e-6));
        let a0 = 1.0 + alpha;
        self.b0 = ((1.0 + cw) * 0.5) / a0;
        self.b1 = -(1.0 + cw) / a0;
        self.b2 = ((1.0 + cw) * 0.5) / a0;
        self.a1 = (-2.0 * cw) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    pub fn set_bandpass(&mut self, cutoff_hz: f32, sample_rate: f32, q: f32) {
        let w0 = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q.max(1.0e-6));
        let a0 = 1.0 + alpha;
        self.b0 = alpha / a0;
        self.b1 = 0.0;
        self.b2 = -alpha / a0;
        self.a1 = (-2.0 * cw) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    pub fn process_block(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            *s = self.process(*s);
        }
    }
}

// ---------------------------------------------------------------------------
// Synthesizer
// ---------------------------------------------------------------------------

const MAX_KICK_SAMPLES: usize = 192_000; // 4 seconds @ 48kHz

/// Double-buffered kick synthesizer.
pub struct KickSynthesizer {
    pub oscillator: Oscillator,
    pub noise: NoiseGenerator,
    pub filter: Biquad,
    pub filter_type: FilterType,
    pub filter_cutoff_hz: f32,
    pub filter_q: f32,
    pub master_filter: Biquad,
    pub master_filter_type: FilterType,
    pub master_filter_cutoff_hz: f32,
    pub master_filter_q: f32,
    pub distortion: f32,
    pub output_gain_db: f32,
    pub length_ms: f32,
    pub sample_rate: f32,

    // Double buffers: [0] = playing, [1] = synthesis target
    buffers: [Vec<f32>; 2],
    active_buffer: usize,
    num_samples: usize,
    playback_pos: usize,
    is_playing: bool,
}

impl KickSynthesizer {
    pub fn new(sample_rate: f32) -> Self {
        let buf0 = vec![0.0f32; MAX_KICK_SAMPLES];
        let buf1 = vec![0.0f32; MAX_KICK_SAMPLES];
        let mut s = Self {
            oscillator: Oscillator::new(sample_rate),
            noise: NoiseGenerator::new(),
            filter: Biquad::default(),
            filter_type: FilterType::Lowpass,
            filter_cutoff_hz: 8000.0,
            filter_q: 0.7,
            master_filter: Biquad::default(),
            master_filter_type: FilterType::Lowpass,
            master_filter_cutoff_hz: 20000.0,
            master_filter_q: 0.7,
            distortion: 0.0,
            output_gain_db: 0.0,
            length_ms: 300.0,
            sample_rate,
            buffers: [buf0, buf1],
            active_buffer: 0,
            num_samples: 0,
            playback_pos: 0,
            is_playing: false,
        };
        s.update_filter();
        s.update_master_filter();
        s
    }

    fn update_filter(&mut self) {
        match self.filter_type {
            FilterType::Lowpass => {
                self.filter
                    .set_lowpass(self.filter_cutoff_hz, self.sample_rate, self.filter_q);
            }
            FilterType::Highpass => {
                self.filter
                    .set_highpass(self.filter_cutoff_hz, self.sample_rate, self.filter_q);
            }
            FilterType::Bandpass => {
                self.filter
                    .set_bandpass(self.filter_cutoff_hz, self.sample_rate, self.filter_q);
            }
        }
    }

    fn update_master_filter(&mut self) {
        match self.master_filter_type {
            FilterType::Lowpass => {
                self.master_filter.set_lowpass(
                    self.master_filter_cutoff_hz,
                    self.sample_rate,
                    self.master_filter_q,
                );
            }
            FilterType::Highpass => {
                self.master_filter.set_highpass(
                    self.master_filter_cutoff_hz,
                    self.sample_rate,
                    self.master_filter_q,
                );
            }
            FilterType::Bandpass => {
                self.master_filter.set_bandpass(
                    self.master_filter_cutoff_hz,
                    self.sample_rate,
                    self.master_filter_q,
                );
            }
        }
    }

    pub fn set_filter_type(&mut self, ty: FilterType) {
        self.filter_type = ty;
        self.update_filter();
    }

    pub fn set_filter_cutoff(&mut self, cutoff: f32) {
        self.filter_cutoff_hz = cutoff;
        self.update_filter();
    }

    pub fn set_filter_q(&mut self, q: f32) {
        self.filter_q = q;
        self.update_filter();
    }

    pub fn set_master_filter_type(&mut self, ty: FilterType) {
        self.master_filter_type = ty;
        self.update_master_filter();
    }

    pub fn set_master_filter_cutoff(&mut self, cutoff: f32) {
        self.master_filter_cutoff_hz = cutoff;
        self.update_master_filter();
    }

    pub fn set_master_filter_q(&mut self, q: f32) {
        self.master_filter_q = q;
        self.update_master_filter();
    }

    /// Trigger synthesis.  Call from any thread; synthesis happens immediately
    /// and the buffer is atomically swapped for playback.
    pub fn trigger(&mut self, velocity: f32) {
        let num_samples = ((self.length_ms * 0.001) * self.sample_rate) as usize;
        let num_samples = num_samples.clamp(1, MAX_KICK_SAMPLES);
        self.num_samples = num_samples;

        let synth_idx = 1 - self.active_buffer;
        let buf = &mut self.buffers[synth_idx][..num_samples];
        buf.fill(0.0);

        self.oscillator.reset();
        self.noise.reset();
        self.filter.reset();
        self.master_filter.reset();

        // Render oscillator layer
        self.oscillator.render(buf, num_samples);

        // Render noise layer into a temp buffer, then mix in
        let mut noise_buf = vec![0.0f32; num_samples];
        self.noise.render(&mut noise_buf, num_samples);

        // Apply filter to noise
        self.filter.process_block(&mut noise_buf);

        // Mix noise into main buffer (SIMD-optimized)
        simd::add_inplace(buf, &noise_buf);

        // Apply master filter to combined signal
        self.master_filter.process_block(buf);

        // Apply master distortion
        if self.distortion > 1.0e-6 {
            let drive = self.distortion * 10.0;
            for s in buf.iter_mut() {
                *s = (*s * drive).tanh();
            }
        }

        // Apply output gain
        let gain_lin = db_to_linear(self.output_gain_db);
        if (gain_lin - 1.0).abs() > 1.0e-6 {
            simd_kick::mul_gain_inplace(buf, gain_lin);
        }

        // Apply velocity scaling
        let vel = velocity.clamp(0.0, 1.0);
        if vel < 1.0 {
            simd_kick::mul_gain_inplace(buf, vel);
        }

        // Hard limit to [-1, 1]
        simd_kick::clip_inplace(buf, 1.0);

        // Swap buffer
        self.active_buffer = synth_idx;
        self.playback_pos = 0;
        self.is_playing = true;
    }

    /// Read `frames` samples from the playing buffer into `out`.
    /// Returns number of frames actually written (may be less than `out.len()` if kick ended).
    pub fn read(&mut self, out: &mut [f32]) -> usize {
        if !self.is_playing {
            out.fill(0.0);
            return out.len();
        }
        let buf = &self.buffers[self.active_buffer];
        let mut written = 0;
        for s in out.iter_mut() {
            if self.playback_pos < self.num_samples {
                *s = buf[self.playback_pos];
                self.playback_pos += 1;
                written += 1;
            } else {
                *s = 0.0;
                self.is_playing = false;
            }
        }
        written
    }

    /// Copy the currently active buffer into `dst` for display.
    pub fn copy_active_buffer(&self, dst: &mut [f32]) -> usize {
        let buf = &self.buffers[self.active_buffer];
        let n = dst.len().min(self.num_samples);
        dst[..n].copy_from_slice(&buf[..n]);
        n
    }

    pub fn num_samples(&self) -> usize {
        self.num_samples
    }
}

#[inline]
pub fn db_to_linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_default() {
        let env = Envelope::default();
        assert!((env.value(0.0) - 1.0).abs() < 1.0e-6);
        assert!((env.value(1.0) - 0.0).abs() < 1.0e-6);
        assert!(env.value(0.5) > 0.4 && env.value(0.5) < 0.6);
    }

    #[test]
    fn envelope_adsr() {
        let env = Envelope::with_default_adsr(10.0, 50.0, 0.5, 40.0);
        assert!(env.value(0.0).abs() < 1.0e-6);
        assert!((env.value(10.0 / 100.0) - 1.0).abs() < 1.0e-6);
        assert!((env.value(60.0 / 100.0) - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn oscillator_sine_render() {
        let mut osc = Oscillator::new(48000.0);
        osc.base_freq_hz = 100.0;
        osc.amplitude = 1.0;
        osc.pitch_env = Envelope::new(vec![EnvPoint::new(0.0, 1.0), EnvPoint::new(1.0, 1.0)]);
        osc.amp_env = Envelope::new(vec![EnvPoint::new(0.0, 1.0), EnvPoint::new(1.0, 1.0)]);
        let mut buf = vec![0.0f32; 480];
        osc.render(&mut buf, 480);
        // 480 samples @ 48kHz = 10ms = 1 cycle of 100Hz
        // Peak should be close to 1.0
        let peak = buf.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!(peak > 0.95 && peak <= 1.0, "peak = {peak}");
    }

    #[test]
    fn biquad_lowpass_attenuates_high_freq() {
        let mut filter = Biquad::default();
        filter.set_lowpass(1000.0, 48000.0, 0.7);
        // Impulse response energy should be less than input
        let mut energy_in = 0.0f32;
        let mut energy_out = 0.0f32;
        for i in 0..100 {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let y = filter.process(x);
            energy_in += x * x;
            energy_out += y * y;
        }
        assert!(energy_out < energy_in);
    }

    #[test]
    fn kick_synthesizer_trigger_and_read() {
        let mut synth = KickSynthesizer::new(48000.0);
        synth.length_ms = 10.0;
        synth.trigger(1.0);
        assert!(synth.num_samples() > 0);
        let mut out = vec![0.0f32; 64];
        let written = synth.read(&mut out);
        assert_eq!(written, 64);
        // Should have non-zero samples
        let sum: f32 = out.iter().map(|s| s.abs()).sum();
        assert!(sum > 0.0);
    }

    #[test]
    fn kick_synthesizer_velocity_scaling() {
        let mut synth1 = KickSynthesizer::new(48000.0);
        let mut synth2 = KickSynthesizer::new(48000.0);
        synth1.length_ms = 10.0;
        synth2.length_ms = 10.0;
        synth1.trigger(1.0);
        synth2.trigger(0.5);

        let mut buf1 = vec![0.0f32; 480];
        let mut buf2 = vec![0.0f32; 480];
        synth1.read(&mut buf1);
        synth2.read(&mut buf2);

        let peak1 = buf1.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        let peak2 = buf2.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!(
            (peak1 - 2.0 * peak2).abs() < 0.1,
            "peak1={peak1} peak2={peak2}"
        );
    }

    #[test]
    fn pink_noise_spectrum() {
        let mut pink = PinkNoise::default();
        let mut sum = 0.0f32;
        for _ in 0..1000 {
            sum += pink.next(1.0).abs();
        }
        // Pink noise should be bounded (roughly same energy as white over time)
        assert!(sum < 5000.0);
        assert!(sum > 100.0);
    }

    #[test]
    fn db_to_linear_accuracy() {
        assert!((db_to_linear(0.0) - 1.0).abs() < 1.0e-6);
        assert!((db_to_linear(-6.0) - 0.501187).abs() < 1.0e-4);
        assert!((db_to_linear(6.0) - 1.99526).abs() < 1.0e-4);
    }
}
