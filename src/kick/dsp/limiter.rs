//! Per-instrument limiter — peak-hold style with fast attack.

/// Simple peak limiter with threshold and release.
#[derive(Debug, Clone, Copy)]
pub struct Limiter {
    pub threshold_db: f32,
    pub release_ms: f32,
    // Internal state
    gain_reduction_db: f32,
    sample_rate: f32,
}

impl Default for Limiter {
    fn default() -> Self {
        Self {
            threshold_db: 0.0,
            release_ms: 50.0,
            gain_reduction_db: 0.0,
            sample_rate: 48000.0,
        }
    }
}

impl Limiter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            threshold_db: 0.0,
            release_ms: 50.0,
            gain_reduction_db: 0.0,
            sample_rate,
        }
    }

    pub fn reset(&mut self) {
        self.gain_reduction_db = 0.0;
    }

    /// Process a block of samples in-place.
    pub fn process_block(&mut self, buf: &mut [f32]) {
        if self.threshold_db >= 6.0 {
            // Threshold at +6 dB or above effectively disables limiting
            return;
        }
        let threshold_lin = db_to_linear(self.threshold_db);
        let release_coeff = if self.release_ms > 0.0 {
            (-1000.0 / (self.release_ms * self.sample_rate)).exp()
        } else {
            0.0
        };

        for s in buf.iter_mut() {
            let abs_x = s.abs();
            let needed_gr = if abs_x > threshold_lin {
                linear_to_db(threshold_lin / abs_x)
            } else {
                0.0
            };

            // Instant attack, smooth release
            if needed_gr < self.gain_reduction_db {
                self.gain_reduction_db = needed_gr;
            } else {
                self.gain_reduction_db =
                    self.gain_reduction_db * release_coeff + needed_gr * (1.0 - release_coeff);
            }

            let gain = db_to_linear(self.gain_reduction_db);
            *s *= gain;
        }
    }
}

#[inline]
fn db_to_linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

#[inline]
fn linear_to_db(lin: f32) -> f32 {
    if lin <= 0.0 {
        -100.0
    } else {
        20.0 * lin.log10()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_noop_above_threshold() {
        let mut lim = Limiter::new(48000.0);
        lim.threshold_db = 0.0;
        let mut buf = vec![0.5, 0.6, 0.4];
        lim.process_block(&mut buf);
        // All samples below 1.0 (0 dB), should be unchanged
        assert!((buf[0] - 0.5).abs() < 1.0e-6);
        assert!((buf[1] - 0.6).abs() < 1.0e-6);
    }

    #[test]
    fn limiter_reduces_peaks() {
        let mut lim = Limiter::new(48000.0);
        lim.threshold_db = -6.0; // 0.5 linear
        let mut buf = vec![0.8, 0.9, 0.3];
        lim.process_block(&mut buf);
        // Peak should be pulled down toward 0.5
        let peak = buf.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!(peak <= 0.51, "peak should be limited: {peak}");
    }
}
