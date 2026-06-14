pub struct EnvelopeFollower {
    sample_rate: f32,
    attack_coeff: f32,
    release_coeff: f32,
    envelope: f32,
    stereo_link: bool,
}

impl EnvelopeFollower {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            attack_coeff: 0.0,
            release_coeff: 0.0,
            envelope: 0.0,
            stereo_link: true,
        }
    }

    pub fn set_attack(&mut self, seconds: f32) {
        self.attack_coeff = Self::time_to_coeff(seconds, self.sample_rate);
    }

    pub fn set_release(&mut self, seconds: f32) {
        self.release_coeff = Self::time_to_coeff(seconds, self.sample_rate);
    }

    pub fn set_stereo_link(&mut self, link: bool) {
        self.stereo_link = link;
    }

    fn time_to_coeff(seconds: f32, sample_rate: f32) -> f32 {
        if seconds <= 0.0 {
            0.0
        } else {
            (-1.0 / (seconds * sample_rate)).exp()
        }
    }

    pub fn reset(&mut self) {
        self.envelope = 0.0;
    }

    pub fn process(&mut self, input_l: f32, input_r: f32) -> f32 {
        let input = if self.stereo_link {
            input_l.abs().max(input_r.abs())
        } else {
            (input_l.abs() + input_r.abs()) * 0.5
        };

        let coeff = if input > self.envelope {
            self.attack_coeff
        } else {
            self.release_coeff
        };

        self.envelope = coeff * self.envelope + (1.0 - coeff) * input;
        self.envelope
    }

    pub fn process_block(&mut self, block_l: &[f32], block_r: &[f32]) -> f32 {
        assert_eq!(block_l.len(), block_r.len());
        for (&l, &r) in block_l.iter().zip(block_r.iter()) {
            self.process(l, r);
        }
        self.envelope
    }

    pub fn envelope(&self) -> f32 {
        self.envelope
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envelope_follower_attack() {
        let mut ef = EnvelopeFollower::new(48000.0);
        ef.set_attack(0.001);
        ef.set_release(0.1);

        for _ in 0..480 {
            ef.process(1.0, 1.0);
        }

        assert!(ef.envelope() > 0.9);
    }

    #[test]
    fn test_envelope_follower_release() {
        let mut ef = EnvelopeFollower::new(48000.0);
        ef.set_attack(0.001);
        ef.set_release(0.001);

        for _ in 0..4800 {
            ef.process(1.0, 1.0);
        }
        assert!(ef.envelope() > 0.99);

        for _ in 0..4800 {
            ef.process(0.0, 0.0);
        }
        assert!(ef.envelope() < 0.01);
    }
}
