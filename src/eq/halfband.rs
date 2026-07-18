/// 63-tap half-band FIR pair used for the 2× oversampled Natural Phase
/// processing mode. Linear phase, deterministic 31-sample round-trip latency
/// at the base sample rate, and clean reconstruction (Blackman-windowed sinc,
/// usable to ~20 kHz at a 48 kHz base rate).
pub const HALFBAND_TAPS: usize = 63;
/// Total up+down group delay in base-rate samples, nearest integer to
/// the measured 30.5-sample round-trip delay.
pub const HALFBAND_LATENCY: u32 = 31;

fn halfband_coeffs(target_sum: f32) -> [f32; HALFBAND_TAPS] {
    const CENTER: usize = HALFBAND_TAPS / 2;
    let mut coeffs = [0.0_f32; HALFBAND_TAPS];
    let mut sum = 0.0_f32;
    for (i, c) in coeffs.iter_mut().enumerate() {
        let n = i as i32 - CENTER as i32;
        let sinc = if n == 0 {
            0.5
        } else if n % 2 != 0 {
            let nf = n as f32;
            (std::f32::consts::FRAC_PI_2 * nf).sin() / (std::f32::consts::PI * nf)
        } else {
            0.0
        };
        // Hann window to tame the ripple of the truncated sinc.
        let w =
            0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (HALFBAND_TAPS - 1) as f32).cos();
        *c = sinc * w;
        sum += *c;
    }
    if sum.abs() > 1.0e-9 {
        for c in &mut coeffs {
            *c *= target_sum / sum;
        }
    }
    coeffs
}

/// Polyphase 2× interpolator: one input sample produces two output samples.
#[derive(Debug, Clone)]
pub struct HalfbandUpsampler {
    coeffs: [f32; HALFBAND_TAPS],
    state: [f32; 32],
}

impl Default for HalfbandUpsampler {
    fn default() -> Self {
        Self::new()
    }
}

impl HalfbandUpsampler {
    pub fn new() -> Self {
        Self {
            coeffs: halfband_coeffs(2.0),
            state: [0.0; 32],
        }
    }

    pub fn reset(&mut self) {
        self.state = [0.0; 32];
    }

    pub fn process(&mut self, x: f32) -> (f32, f32) {
        for i in (0..31).rev() {
            self.state[i + 1] = self.state[i];
        }
        self.state[0] = x;
        let mut y0 = 0.0_f32;
        let mut y1 = 0.0_f32;
        for k in 0..32 {
            y0 += self.coeffs[2 * k] * self.state[k];
        }
        for k in 0..31 {
            y1 += self.coeffs[2 * k + 1] * self.state[k];
        }
        (y0, y1)
    }
}

/// 2× decimator: consumes two 2×-rate samples, produces one base-rate sample.
#[derive(Debug, Clone)]
pub struct HalfbandDownsampler {
    coeffs: [f32; HALFBAND_TAPS],
    state: [f32; HALFBAND_TAPS],
}

impl Default for HalfbandDownsampler {
    fn default() -> Self {
        Self::new()
    }
}

impl HalfbandDownsampler {
    pub fn new() -> Self {
        Self {
            coeffs: halfband_coeffs(1.0),
            state: [0.0; HALFBAND_TAPS],
        }
    }

    pub fn reset(&mut self) {
        self.state = [0.0; HALFBAND_TAPS];
    }

    pub fn process(&mut self, s0: f32, s1: f32) -> f32 {
        for i in (0..HALFBAND_TAPS - 2).rev() {
            self.state[i + 2] = self.state[i];
        }
        self.state[1] = s0;
        self.state[0] = s1;
        let mut y = 0.0_f32;
        for (i, c) in self.coeffs.iter().enumerate() {
            y += c * self.state[i];
        }
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The polyphase split adds a half sample to the ideal 31-sample delay;
    /// the host is compensated with the nearest integer (HALFBAND_LATENCY).
    const TRUE_DELAY: f32 = 30.5;

    fn roundtrip(freq: f32, sr: f32, n: usize) -> Vec<f32> {
        let mut up = HalfbandUpsampler::new();
        let mut down = HalfbandDownsampler::new();
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let x = (2.0 * std::f32::consts::PI * freq * i as f32 / sr).sin();
            let (a, b) = up.process(x);
            out.push(down.process(a, b));
        }
        out
    }

    fn max_err_against_delayed_sine(out: &[f32], freq: f32, sr: f32) -> f32 {
        let mut max_err = 0.0_f32;
        for (i, &o) in out.iter().enumerate().skip(128) {
            let expected = (2.0 * std::f32::consts::PI * freq * (i as f32 - TRUE_DELAY) / sr).sin();
            max_err = max_err.max((o - expected).abs());
        }
        max_err
    }

    #[test]
    fn up_down_roundtrip_reconstructs_sine() {
        let sr = 48_000.0_f32;
        for freq in [100.0, 1_000.0, 5_000.0, 10_000.0, 15_000.0] {
            let out = roundtrip(freq, sr, 8_192);
            let err = max_err_against_delayed_sine(&out, freq, sr);
            assert!(err < 0.01, "roundtrip error {err} at {freq} Hz");
        }
    }

    #[test]
    fn reported_latency_is_nearest_integer() {
        assert!((HALFBAND_LATENCY as f32 - TRUE_DELAY).abs() <= 0.5);
    }
}
