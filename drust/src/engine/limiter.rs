/// Simple brickwall limiter with attack/release envelope.
#[derive(Debug, Clone)]
pub struct Limiter {
    threshold: f32,
    attack_coeff: f32,
    release_coeff: f32,
    gain_reduction: f32,
    enabled: bool,
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new(-3.0, 0.001, 0.01)
    }
}

impl Limiter {
    pub fn new(threshold_db: f32, attack_ms: f32, release_ms: f32) -> Self {
        let sr = 44100.0f32; // will be updated on first process
        Self {
            threshold: 10.0f32.powf(threshold_db * 0.05),
            attack_coeff: 1.0 - (-1000.0 / (attack_ms * sr)).exp(),
            release_coeff: 1.0 - (-1000.0 / (release_ms * sr)).exp(),
            gain_reduction: 1.0,
            enabled: true,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn set_threshold_db(&mut self, threshold_db: f32) {
        self.threshold = 10.0f32.powf(threshold_db * 0.05);
    }

    pub fn reset(&mut self) {
        self.gain_reduction = 1.0;
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        let attack_ms = 1.0;
        let release_ms = 10.0;
        self.attack_coeff = 1.0 - (-1000.0 / (attack_ms * sr)).exp();
        self.release_coeff = 1.0 - (-1000.0 / (release_ms * sr)).exp();
    }

    pub fn process(&mut self, buffers: &mut [Vec<f32>], frames: usize) {
        if !self.enabled || buffers.is_empty() {
            return;
        }
        let mut slices: Vec<&mut [f32]> = buffers.iter_mut().map(|b| b.as_mut_slice()).collect();
        self.process_slices(&mut slices, frames);
    }

    pub fn process_slices(&mut self, slices: &mut [&mut [f32]], frames: usize) {
        if !self.enabled || slices.is_empty() {
            return;
        }

        for i in 0..frames {
            let mut peak = 0.0f32;
            for s in slices.iter() {
                if i < s.len() {
                    peak = peak.max(s[i].abs());
                }
            }

            let target_gr = if peak > self.threshold {
                self.threshold / peak
            } else {
                1.0
            };

            if target_gr < self.gain_reduction {
                self.gain_reduction += (target_gr - self.gain_reduction) * self.attack_coeff;
            } else {
                self.gain_reduction += (target_gr - self.gain_reduction) * self.release_coeff;
            }

            for s in slices.iter_mut() {
                if i < s.len() {
                    s[i] *= self.gain_reduction;
                }
            }
        }
    }
}
