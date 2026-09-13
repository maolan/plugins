//! True-peak detector front end: polyphase windowed-sinc interpolation of
//! the input signal at 2x/4x/8x, reporting the maximum absolute
//! (sub-)sample. This catches inter-sample peaks that a base-rate peak
//! detector misses, the same job the AL-1 limiter's oversampled detector
//! does (it runs its gain computer at the oversampled rate).
//!
//! The interpolator is a classic polyphase FIR: every phase `p` is a
//! windowed-sinc delayed by `p / factor` samples. Support is
//! `TAPS` input samples, so the detector latency is a fixed, integer
//! `TAPS / 2` input samples for any factor — easy to compensate in the
//! host-reported latency.

/// Input samples of support per phase. Group delay is exactly `TAPS / 2`
/// input samples at every supported factor.
pub const TRUE_PEAK_TAPS: usize = 16;
/// Detector latency in input samples (must equal `TRUE_PEAK_TAPS / 2`).
pub const TRUE_PEAK_LATENCY: u32 = (TRUE_PEAK_TAPS / 2) as u32;

/// Fraction of the input Nyquist frequency the interpolator passes.
const PASSBAND: f64 = 0.9;

fn blackman(u: f64, half_width: f64) -> f64 {
    let t = (u + half_width) / (2.0 * half_width);
    if !(0.0..=1.0).contains(&t) {
        return 0.0;
    }
    0.5 - 0.5 * (2.0 * std::f64::consts::PI * t).cos()
}

fn phase_taps(factor: usize, phase: usize) -> [f32; TRUE_PEAK_TAPS] {
    let half_width = TRUE_PEAK_TAPS as f64 * 0.5;
    let frac = phase as f64 / factor as f64;
    let mut taps = [0.0_f64; TRUE_PEAK_TAPS];
    let mut sum = 0.0_f64;
    for (k, tap) in taps.iter_mut().enumerate() {
        // Position of this tap relative to the sample we reconstruct:
        // output phase `p` of input sample n approximates x(n - D + p/factor)
        // with D = TRUE_PEAK_TAPS / 2 (the group delay).
        let u = k as f64 - TRUE_PEAK_TAPS as f64 * 0.5 - frac;
        let sinc = if u.abs() < 1.0e-12 {
            PASSBAND
        } else {
            (std::f64::consts::PI * PASSBAND * u).sin() / (std::f64::consts::PI * u)
        };
        *tap = sinc * blackman(u, half_width);
        sum += *tap;
    }
    if sum.abs() > 1.0e-12 {
        for tap in &mut taps {
            *tap /= sum;
        }
    }
    taps.map(|tap| tap as f32)
}

/// Polyphase interpolator producing `factor` sub-samples per input sample.
pub struct TruePeakDetector {
    factor: usize,
    taps: Vec<[f32; TRUE_PEAK_TAPS]>,
    state: [f32; TRUE_PEAK_TAPS],
    position: usize,
}

impl TruePeakDetector {
    /// `factor` must be 1 (passthrough) or a power of two in 2..=8.
    pub fn new(factor: usize) -> Self {
        let factor = if factor >= 8 {
            8
        } else if factor >= 4 {
            4
        } else if factor >= 2 {
            2
        } else {
            1
        };
        let taps = (0..factor).map(|phase| phase_taps(factor, phase)).collect();
        Self {
            factor,
            taps,
            state: [0.0; TRUE_PEAK_TAPS],
            position: 0,
        }
    }

    pub fn factor(&self) -> usize {
        self.factor
    }

    /// Fixed detector latency in input samples (0 when disabled).
    pub fn latency_samples(&self) -> u32 {
        if self.factor > 1 {
            TRUE_PEAK_LATENCY
        } else {
            0
        }
    }

    pub fn reset(&mut self) {
        self.state = [0.0; TRUE_PEAK_TAPS];
        self.position = 0;
    }

    /// Detect the peak of the interpolated signal around `x`.
    /// Returns the maximum absolute sub-sample value.
    pub fn detect(&mut self, x: f32) -> f64 {
        if self.factor == 1 {
            return x.abs() as f64;
        }
        self.state[self.position] = x;
        let mask = TRUE_PEAK_TAPS - 1;
        let mut peak = 0.0_f32;
        for phase_taps in &self.taps {
            let mut acc = 0.0_f32;
            for (k, &tap) in phase_taps.iter().enumerate() {
                let index = (self.position + TRUE_PEAK_TAPS - k) & mask;
                acc += tap * self.state[index];
            }
            peak = peak.max(acc.abs());
        }
        self.position = (self.position + 1) & mask;
        peak as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_impulse(factor: usize) -> Vec<f32> {
        let mut detector = TruePeakDetector::new(factor);
        (0..64)
            .map(|i| {
                let x = if i == 0 { 1.0 } else { 0.0 };
                detector.detect(x) as f32
            })
            .collect()
    }

    #[test]
    fn impulse_peak_arrives_after_group_delay() {
        let out = run_impulse(4);
        let (argmax, max) = out
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(i, &v)| (i, v))
            .unwrap();
        assert_eq!(argmax, TRUE_PEAK_LATENCY as usize);
        // The antialias filter passes ~0.9 of a full-band impulse; in-band
        // signals reconstruct at unity (see catches_inter_sample_peak).
        assert!(max > 0.85, "impulse peak {max} too small");
    }

    #[test]
    fn catches_inter_sample_peak() {
        // Half-Nyquist sine shifted by half a sample: base-rate samples only
        // reach 0.7071 while the true (inter-sample) peak is 1.0.
        let sample_rate = 48_000.0_f64;
        let freq = 12_000.0_f64;
        let n = 4096;
        let signal: Vec<f32> = (0..n)
            .map(|i| {
                (2.0 * std::f64::consts::PI * freq * (i as f64 + 0.5) / sample_rate).sin() as f32
            })
            .collect();
        let base_peak = signal.iter().map(|&x| x.abs()).fold(0.0_f32, f32::max);

        let mut detector = TruePeakDetector::new(4);
        let mut os_peak = 0.0_f64;
        for &x in &signal {
            os_peak = os_peak.max(detector.detect(x));
        }
        assert!(
            (base_peak - std::f32::consts::FRAC_1_SQRT_2).abs() < 1.0e-3,
            "test signal sanity: {base_peak}"
        );
        assert!(
            os_peak > 0.97,
            "true-peak detector should see ~1.0, got {os_peak}"
        );
    }

    #[test]
    fn disabled_detector_reports_absolute_sample_peak() {
        let mut detector = TruePeakDetector::new(1);
        assert_eq!(detector.latency_samples(), 0);
        for i in 0..128 {
            let x = (i as f64 * 0.1).sin();
            assert!((detector.detect(x as f32) - x.abs()).abs() <= 1.0e-7);
        }
    }

    #[test]
    fn reset_clears_history() {
        let mut detector = TruePeakDetector::new(8);
        for _ in 0..64 {
            detector.detect(1.0);
        }
        detector.reset();
        let peak = detector.detect(0.0);
        assert!(peak < 1.0e-6, "history leaked after reset: {peak}");
    }
}
