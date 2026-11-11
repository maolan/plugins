#[derive(Debug, Clone, Copy)]
pub struct TransientParams {
    pub attack_db: f32,
    pub sustain_db: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub dry_wet: f32,
    pub output_gain_db: f32,
}

#[derive(Debug, Clone)]
pub struct TransientShaper {
    sample_rate: f32,
    fast_envelope: f32,
    slow_envelope: f32,
}

impl TransientShaper {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: sample_rate.max(1.0),
            fast_envelope: 0.0,
            slow_envelope: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.fast_envelope = 0.0;
        self.slow_envelope = 0.0;
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &TransientParams,
    ) {
        let fast_coefficient = time_coefficient(params.attack_ms, self.sample_rate);
        let slow_coefficient = time_coefficient(params.release_ms, self.sample_rate);
        let output_gain = db_to_gain(params.output_gain_db);
        let mix = params.dry_wet.clamp(0.0, 1.0);

        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            let dry_left = *left;
            let dry_right = *right;
            let level = dry_left.abs().max(dry_right.abs());
            self.fast_envelope += fast_coefficient * (level - self.fast_envelope);
            self.slow_envelope += slow_coefficient * (level - self.slow_envelope);

            let fast_db = gain_to_db(self.fast_envelope.max(1.0e-6));
            let slow_db = gain_to_db(self.slow_envelope.max(1.0e-6));
            let transient_db = fast_db - slow_db;
            let strength = (transient_db.abs() / 12.0).clamp(0.0, 1.0);
            let gain_db = if transient_db >= 0.0 {
                params.attack_db * strength
            } else {
                params.sustain_db * strength
            };
            let gain = db_to_gain(gain_db);

            *left = (dry_left * gain * mix + dry_left * (1.0 - mix)) * output_gain;
            *right = (dry_right * gain * mix + dry_right * (1.0 - mix)) * output_gain;
        }
    }

    pub fn process_mono(&mut self, buffer: &mut [f32], params: &TransientParams) {
        for sample in buffer {
            let mut right = *sample;
            self.process_stereo(
                std::slice::from_mut(sample),
                std::slice::from_mut(&mut right),
                params,
            );
        }
    }
}

fn time_coefficient(time_ms: f32, sample_rate: f32) -> f32 {
    1.0 - (-1.0 / (time_ms.max(0.01) * 0.001 * sample_rate)).exp()
}

fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db * 0.05)
}

fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.log10()
}

#[cfg(test)]
mod tests {
    use super::{TransientParams, TransientShaper};

    fn params() -> TransientParams {
        TransientParams {
            attack_db: 0.0,
            sustain_db: 0.0,
            attack_ms: 1.0,
            release_ms: 100.0,
            dry_wet: 1.0,
            output_gain_db: 0.0,
        }
    }

    #[test]
    fn neutral_settings_preserve_audio() {
        let mut shaper = TransientShaper::new(48_000.0);
        let original = [0.0, 0.5, -0.25, 0.1];
        let mut left = original;
        let mut right = original;
        shaper.process_stereo(&mut left, &mut right, &params());
        assert_eq!(left, original);
        assert_eq!(right, original);
    }

    #[test]
    fn positive_attack_boosts_transient() {
        let mut shaper = TransientShaper::new(48_000.0);
        let mut params = params();
        params.attack_db = 12.0;
        let mut left = [1.0; 16];
        let mut right = [1.0; 16];
        shaper.process_stereo(&mut left, &mut right, &params);
        assert!(left[0] > 1.0);
        assert!(right[0] > 1.0);
    }

    #[test]
    fn negative_sustain_reduces_decay() {
        let mut shaper = TransientShaper::new(1_000.0);
        let mut params = params();
        params.sustain_db = -12.0;
        let mut input = vec![1.0; 200];
        input.extend(std::iter::repeat_n(0.25, 100));
        let original = input.clone();
        let mut right = input.clone();
        shaper.process_stereo(&mut input, &mut right, &params);
        assert!(
            input[250] < original[250],
            "processed {}, original {}",
            input[250],
            original[250]
        );
        assert!(right[250] < original[250]);
    }

    #[test]
    fn stereo_detector_is_linked() {
        let mut shaper = TransientShaper::new(48_000.0);
        let mut params = params();
        params.attack_db = 12.0;
        let mut left = [0.25; 8];
        let mut right = [1.0; 8];
        shaper.process_stereo(&mut left, &mut right, &params);
        assert!(left[0] > 0.25);
        assert!(right[0] > 1.0);
        assert!((left[0] / 0.25 - right[0]).abs() < 1.0e-5);
    }
}
