const MAX_LOOKAHEAD_MS: f64 = 20.0;
const MIN_SAMPLE_RATE: f64 = 1.0;

#[derive(Debug, Clone, Copy)]
pub struct LimiterParams {
    pub boost: f64,
    pub ceiling: f64,
    pub lookahead_ms: f64,
    pub attack_ms: f64,
    pub release_ms: f64,
    pub link_transients: f64,
    pub link_release: f64,
    pub output_gain: f64,
}

pub struct Limiter {
    sample_rate: f64,
    delay_l: Vec<f32>,
    delay_r: Vec<f32>,
    write_pos: usize,
    gain_l: f64,
    gain_r: f64,
    reduction_l_db: f32,
    reduction_r_db: f32,
}

impl Default for Limiter {
    fn default() -> Self {
        let mut limiter = Self {
            sample_rate: 48_000.0,
            delay_l: Vec::new(),
            delay_r: Vec::new(),
            write_pos: 0,
            gain_l: 1.0,
            gain_r: 1.0,
            reduction_l_db: 0.0,
            reduction_r_db: 0.0,
        };
        limiter.resize_delay();
        limiter
    }
}

impl Limiter {
    pub fn set_sample_rate(&mut self, sr: f64) {
        self.sample_rate = sr.max(MIN_SAMPLE_RATE);
        self.resize_delay();
    }

    pub fn reset(&mut self) {
        self.delay_l.fill(0.0);
        self.delay_r.fill(0.0);
        self.write_pos = 0;
        self.gain_l = 1.0;
        self.gain_r = 1.0;
        self.reduction_l_db = 0.0;
        self.reduction_r_db = 0.0;
    }

    pub fn latency_samples_for(sample_rate: f64, lookahead_ms: f64) -> u32 {
        (sample_rate.max(MIN_SAMPLE_RATE) * lookahead_ms.clamp(0.0, MAX_LOOKAHEAD_MS) / 1000.0)
            .round() as u32
    }

    pub fn latency_samples(&self, lookahead_ms: f64) -> u32 {
        Self::latency_samples_for(self.sample_rate, lookahead_ms)
    }

    pub fn gain_reduction_db(&self) -> [f32; 2] {
        [self.reduction_l_db, self.reduction_r_db]
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32], params: &LimiterParams) {
        if left.is_empty() || right.is_empty() {
            self.reduction_l_db = 0.0;
            self.reduction_r_db = 0.0;
            return;
        }

        self.ensure_delay_capacity();

        let input_gain = db_to_gain(params.boost);
        let ceiling = db_to_gain(params.ceiling).max(1.0e-9);
        let hidden_gain = db_to_gain(hidden_ceiling_drive_db(params.ceiling)) as f32;
        let output_gain = db_to_gain(params.output_gain) as f32;
        let delay_samples = self
            .latency_samples(params.lookahead_ms)
            .min(self.delay_l.len().saturating_sub(1) as u32) as usize;
        let attack_step = raised_cosine_step(params.attack_ms, self.sample_rate);
        let release_step = raised_cosine_step(params.release_ms, self.sample_rate);
        let transient_link = (params.link_transients / 100.0).clamp(0.0, 1.0);
        let release_link = (params.link_release / 100.0).clamp(0.0, 1.0);
        let mut max_reduction_l_db = 0.0_f32;
        let mut max_reduction_r_db = 0.0_f32;

        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            let input_l = *left as f64 * input_gain;
            let input_r = *right as f64 * input_gain;

            let target_l = target_gain(input_l, ceiling);
            let target_r = target_gain(input_r, ceiling);
            let linked_target = target_l.min(target_r);
            let target_l = lerp(target_l, linked_target, transient_link);
            let target_r = lerp(target_r, linked_target, transient_link);

            let next_l = smooth_gain(self.gain_l, target_l, attack_step, release_step);
            let next_r = smooth_gain(self.gain_r, target_r, attack_step, release_step);
            let release_floor = next_l.min(next_r);

            self.gain_l = if next_l > self.gain_l {
                lerp(next_l, release_floor, release_link)
            } else {
                next_l
            };
            self.gain_r = if next_r > self.gain_r {
                lerp(next_r, release_floor, release_link)
            } else {
                next_r
            };

            let delayed_l = delay_sample(
                &mut self.delay_l,
                input_l as f32,
                self.write_pos,
                delay_samples,
            );
            let delayed_r = delay_sample(
                &mut self.delay_r,
                input_r as f32,
                self.write_pos,
                delay_samples,
            );
            self.write_pos += 1;
            if self.write_pos >= self.delay_l.len() {
                self.write_pos = 0;
            }

            let limited_l = (delayed_l as f64 * self.gain_l).clamp(-ceiling, ceiling) as f32;
            let limited_r = (delayed_r as f64 * self.gain_r).clamp(-ceiling, ceiling) as f32;
            max_reduction_l_db = max_reduction_l_db.max(actual_reduction_db(delayed_l, limited_l));
            max_reduction_r_db = max_reduction_r_db.max(actual_reduction_db(delayed_r, limited_r));
            *left = limited_l * hidden_gain * output_gain;
            *right = limited_r * hidden_gain * output_gain;
        }

        self.reduction_l_db = max_reduction_l_db;
        self.reduction_r_db = max_reduction_r_db;
    }

    fn resize_delay(&mut self) {
        let len = Self::latency_samples_for(self.sample_rate, MAX_LOOKAHEAD_MS) as usize + 1;
        self.delay_l.resize(len.max(1), 0.0);
        self.delay_r.resize(len.max(1), 0.0);
        self.write_pos = self.write_pos.min(self.delay_l.len().saturating_sub(1));
    }

    fn ensure_delay_capacity(&mut self) {
        let required = Self::latency_samples_for(self.sample_rate, MAX_LOOKAHEAD_MS) as usize + 1;
        if self.delay_l.len() < required {
            self.resize_delay();
        }
    }
}

fn db_to_gain(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}

fn hidden_ceiling_drive_db(ceiling_db: f64) -> f64 {
    -ceiling_db.clamp(-90.0, 0.0)
}

fn target_gain(sample: f64, ceiling: f64) -> f64 {
    let peak = sample.abs();
    if peak > ceiling { ceiling / peak } else { 1.0 }
}

fn raised_cosine_step(time_ms: f64, sample_rate: f64) -> f64 {
    if time_ms <= 0.0 {
        return 1.0;
    }
    let samples = (time_ms * sample_rate.max(MIN_SAMPLE_RATE) / 1000.0).max(1.0);
    let linear_step = 1.0 / samples;
    let cosine_step = 0.5 - 0.5 * (std::f64::consts::PI * linear_step.clamp(0.0, 1.0)).cos();
    cosine_step.max(linear_step)
}

fn smooth_gain(current: f64, target: f64, attack_step: f64, release_step: f64) -> f64 {
    let step = if target < current {
        attack_step
    } else {
        release_step
    };
    current + (target - current) * step
}

fn lerp(a: f64, b: f64, amount: f64) -> f64 {
    a + (b - a) * amount
}

fn actual_reduction_db(input: f32, output: f32) -> f32 {
    let input = input.abs();
    if input <= 1.0e-12 {
        return 0.0;
    }
    let gain = (output.abs() / input).clamp(1.0e-12, 1.0);
    (-20.0 * gain.log10()).clamp(0.0, 60.0)
}

fn delay_sample(buffer: &mut [f32], input: f32, write_pos: usize, delay_samples: usize) -> f32 {
    if delay_samples == 0 {
        buffer[write_pos] = input;
        return input;
    }
    let read_pos = (write_pos + buffer.len() - delay_samples.min(buffer.len() - 1)) % buffer.len();
    let output = buffer[read_pos];
    buffer[write_pos] = input;
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> LimiterParams {
        LimiterParams {
            boost: 0.0,
            ceiling: -1.0,
            lookahead_ms: 1.0,
            attack_ms: 0.0,
            release_ms: 50.0,
            link_transients: 100.0,
            link_release: 100.0,
            output_gain: 0.0,
        }
    }

    #[test]
    fn limiter_bounds_loud_samples() {
        let mut limiter = Limiter::default();
        limiter.set_sample_rate(48_000.0);
        let mut left = vec![2.0; 256];
        let mut right = vec![-2.0; 256];
        let mut params = params();
        params.lookahead_ms = 0.0;
        limiter.process_stereo(&mut left, &mut right, &params);
        let post_makeup_ceiling =
            db_to_gain(params.ceiling + hidden_ceiling_drive_db(params.ceiling)) as f32;
        assert!(
            left.iter()
                .chain(&right)
                .all(|sample| sample.abs() <= post_makeup_ceiling + 1.0e-6)
        );
    }

    #[test]
    fn full_transient_link_applies_left_peak_to_right() {
        let mut limiter = Limiter::default();
        limiter.set_sample_rate(48_000.0);
        let mut left = vec![2.0; 32];
        let mut right = vec![0.5; 32];
        let mut params = params();
        params.lookahead_ms = 0.0;
        params.link_transients = 100.0;
        limiter.process_stereo(&mut left, &mut right, &params);
        assert!(right[0] < 0.5);
    }

    #[test]
    fn zero_transient_link_leaves_quiet_side_unreduced() {
        let mut limiter = Limiter::default();
        limiter.set_sample_rate(48_000.0);
        let mut left = vec![2.0; 32];
        let mut right = vec![0.5; 32];
        let mut params = params();
        params.lookahead_ms = 0.0;
        params.link_transients = 0.0;
        limiter.process_stereo(&mut left, &mut right, &params);
        let expected = 0.5 * db_to_gain(hidden_ceiling_drive_db(params.ceiling)) as f32;
        assert!((right[0] - expected).abs() < 1.0e-6);
    }

    #[test]
    fn lower_ceiling_adds_hidden_drive_after_limiting() {
        let mut limiter = Limiter::default();
        limiter.set_sample_rate(48_000.0);
        let mut unchanged_left = vec![0.05; 32];
        let mut unchanged_right = vec![0.05; 32];
        let mut driven_left = unchanged_left.clone();
        let mut driven_right = unchanged_right.clone();
        let mut params = params();
        params.lookahead_ms = 0.0;
        params.ceiling = 0.0;
        limiter.process_stereo(&mut unchanged_left, &mut unchanged_right, &params);

        limiter.reset();
        params.ceiling = -12.0;
        limiter.process_stereo(&mut driven_left, &mut driven_right, &params);

        assert!(driven_left[0] > unchanged_left[0]);
        assert!(driven_right[0] > unchanged_right[0]);
        assert!(
            driven_left
                .iter()
                .all(|sample| sample.abs() <= 1.0 + 1.0e-6)
        );
        assert!(
            driven_right
                .iter()
                .all(|sample| sample.abs() <= 1.0 + 1.0e-6)
        );
    }

    #[test]
    fn lookahead_reports_latency_in_samples() {
        assert_eq!(Limiter::latency_samples_for(48_000.0, 1.0), 48);
    }
}
