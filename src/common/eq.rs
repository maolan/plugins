//! 3-band parametric EQ using biquad filters.
//!
//! Bands: low-shelf, peaking (mid), high-shelf. Each band has independent
//! frequency, Q, and gain. The implementation is based on the standard
//! Audio EQ Cookbook biquad coefficients.

use std::f32::consts::PI;

/// Biquad filter coefficients (normalized, a0 = 1).
#[derive(Debug, Clone, Copy, Default)]
pub struct BiquadCoefficients {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

/// Per-channel biquad state.
#[derive(Debug, Clone, Copy, Default)]
pub struct BiquadState {
    pub x1: f32,
    pub x2: f32,
    pub y1: f32,
    pub y2: f32,
}

/// Single biquad section.
#[derive(Debug, Clone, Copy, Default)]
pub struct Biquad {
    coeffs: BiquadCoefficients,
    state: BiquadState,
}

impl Biquad {
    pub fn set_coeffs(&mut self, coeffs: BiquadCoefficients) {
        self.coeffs = coeffs;
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let y = self.coeffs.b0 * input
            + self.coeffs.b1 * self.state.x1
            + self.coeffs.b2 * self.state.x2
            - self.coeffs.a1 * self.state.y1
            - self.coeffs.a2 * self.state.y2;
        self.state.x2 = self.state.x1;
        self.state.x1 = input;
        self.state.y2 = self.state.y1;
        self.state.y1 = y;
        y
    }

    pub fn reset(&mut self) {
        self.state = BiquadState::default();
    }
}

fn normalize(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> BiquadCoefficients {
    BiquadCoefficients {
        b0: b0 / a0,
        b1: b1 / a0,
        b2: b2 / a0,
        a1: a1 / a0,
        a2: a2 / a0,
    }
}

fn low_shelf_coeffs(sample_rate: f32, frequency: f32, q: f32, gain_db: f32) -> BiquadCoefficients {
    let a = 10.0_f32.powf(gain_db / 40.0);
    let w0 = 2.0 * PI * frequency / sample_rate.max(1.0);
    let alpha = w0.sin() / (2.0 * q.max(1.0e-5));
    let cos_w0 = w0.cos();
    let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
    normalize(
        a * ((a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha),
        2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0),
        a * ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha),
        (a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha,
        -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0),
        (a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha,
    )
}

fn peaking_coeffs(sample_rate: f32, frequency: f32, q: f32, gain_db: f32) -> BiquadCoefficients {
    let a = 10.0_f32.powf(gain_db / 40.0);
    let w0 = 2.0 * PI * frequency / sample_rate.max(1.0);
    let alpha = w0.sin() / (2.0 * q.max(1.0e-5));
    let cos_w0 = w0.cos();
    normalize(
        1.0 + alpha * a,
        -2.0 * cos_w0,
        1.0 - alpha * a,
        1.0 + alpha / a,
        -2.0 * cos_w0,
        1.0 - alpha / a,
    )
}

fn high_shelf_coeffs(sample_rate: f32, frequency: f32, q: f32, gain_db: f32) -> BiquadCoefficients {
    let a = 10.0_f32.powf(gain_db / 40.0);
    let w0 = 2.0 * PI * frequency / sample_rate.max(1.0);
    let alpha = w0.sin() / (2.0 * q.max(1.0e-5));
    let cos_w0 = w0.cos();
    let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
    normalize(
        a * ((a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha),
        -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0),
        a * ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha),
        (a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha,
        2.0 * ((a - 1.0) - (a + 1.0) * cos_w0),
        (a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha,
    )
}

/// 3-band parametric EQ: low-shelf, peaking (mid), high-shelf.
/// Processes stereo signals with independent biquads per channel.
#[derive(Debug, Clone)]
pub struct Eq3Band {
    sample_rate: f32,
    low_shelf_l: Biquad,
    low_shelf_r: Biquad,
    peak_l: Biquad,
    peak_r: Biquad,
    high_shelf_l: Biquad,
    high_shelf_r: Biquad,
    low_freq: f32,
    low_q: f32,
    low_gain: f32,
    mid_freq: f32,
    mid_q: f32,
    mid_gain: f32,
    high_freq: f32,
    high_q: f32,
    high_gain: f32,
}

impl Eq3Band {
    pub fn new(sample_rate: f32) -> Self {
        let mut eq = Self {
            sample_rate,
            low_shelf_l: Biquad::default(),
            low_shelf_r: Biquad::default(),
            peak_l: Biquad::default(),
            peak_r: Biquad::default(),
            high_shelf_l: Biquad::default(),
            high_shelf_r: Biquad::default(),
            low_freq: 250.0,
            low_q: 0.7,
            low_gain: 0.0,
            mid_freq: 1000.0,
            mid_q: 0.7,
            mid_gain: 0.0,
            high_freq: 4000.0,
            high_q: 0.7,
            high_gain: 0.0,
        };
        eq.update_coeffs();
        eq
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_coeffs();
    }

    pub fn set_params(
        &mut self,
        low_freq: f32,
        low_gain: f32,
        mid_freq: f32,
        mid_gain: f32,
        high_freq: f32,
        high_gain: f32,
    ) {
        self.low_freq = low_freq.clamp(20.0, 20000.0);
        self.low_gain = low_gain.clamp(-18.0, 18.0);
        self.mid_freq = mid_freq.clamp(20.0, 20000.0);
        self.mid_gain = mid_gain.clamp(-18.0, 18.0);
        self.high_freq = high_freq.clamp(20.0, 20000.0);
        self.high_gain = high_gain.clamp(-18.0, 18.0);
        self.update_coeffs();
    }

    fn update_coeffs(&mut self) {
        let low = low_shelf_coeffs(self.sample_rate, self.low_freq, self.low_q, self.low_gain);
        let mid = peaking_coeffs(self.sample_rate, self.mid_freq, self.mid_q, self.mid_gain);
        let high = high_shelf_coeffs(
            self.sample_rate,
            self.high_freq,
            self.high_q,
            self.high_gain,
        );
        self.low_shelf_l.set_coeffs(low);
        self.low_shelf_r.set_coeffs(low);
        self.peak_l.set_coeffs(mid);
        self.peak_r.set_coeffs(mid);
        self.high_shelf_l.set_coeffs(high);
        self.high_shelf_r.set_coeffs(high);
    }

    pub fn reset(&mut self) {
        self.low_shelf_l.reset();
        self.low_shelf_r.reset();
        self.peak_l.reset();
        self.peak_r.reset();
        self.high_shelf_l.reset();
        self.high_shelf_r.reset();
    }

    pub fn process_block(&mut self, buf_l: &mut [f32], buf_r: &mut [f32]) {
        for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
            let mut sl = self.low_shelf_l.process(*l);
            let mut sr = self.low_shelf_r.process(*r);
            sl = self.peak_l.process(sl);
            sr = self.peak_r.process(sr);
            *l = self.high_shelf_l.process(sl);
            *r = self.high_shelf_r.process(sr);
        }
    }

    pub fn process_sample(&mut self, l: f32, r: f32) -> (f32, f32) {
        let sl = self.low_shelf_l.process(l);
        let sr = self.low_shelf_r.process(r);
        let ml = self.peak_l.process(sl);
        let mr = self.peak_r.process(sr);
        (self.high_shelf_l.process(ml), self.high_shelf_r.process(mr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eq3band_default_flat() {
        let mut eq = Eq3Band::new(48000.0);
        let mut l = vec![1.0f32; 64];
        let mut r = vec![1.0f32; 64];
        eq.process_block(&mut l, &mut r);
        // With 0 dB gain on all bands, DC should pass through ~1.0.
        assert!((l[63] - 1.0).abs() < 0.01, "expected ~1.0, got {}", l[63]);
        assert!((r[63] - 1.0).abs() < 0.01, "expected ~1.0, got {}", r[63]);
    }

    #[test]
    fn test_eq3band_low_boost() {
        let mut eq = Eq3Band::new(48000.0);
        eq.set_params(250.0, 6.0, 1000.0, 0.0, 4000.0, 0.0);
        // DC-ish low-frequency signal: low shelf should boost it.
        let mut l = vec![0.5f32; 256];
        let mut r = vec![0.5f32; 256];
        let before = l[10];
        eq.process_block(&mut l, &mut r);
        let after = l[10];
        // +6 dB shelf should increase a low-frequency / DC signal.
        assert!(
            after.abs() > before.abs() * 1.1,
            "expected boost, before={} after={}",
            before,
            after
        );
    }
}
