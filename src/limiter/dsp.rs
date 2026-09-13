use crate::common::true_peak::{TRUE_PEAK_LATENCY, TRUE_PEAK_TAPS, TruePeakDetector};

const MAX_LOOKAHEAD_MS: f64 = 20.0;
const MIN_SAMPLE_RATE: f64 = 1.0;

const WINDOW_BUFFER_SECONDS: f64 = 0.015289025731886;

const RELEASE_DB_RANGE: f64 = 24.0;

const ATTACK_ASYMMETRY: f64 = 8.0;

const JERK_FACTOR: f64 = 0.5;

const KNEE_DB: f64 = 0.12;
const MIN_DETECTOR_LEVEL: f64 = 1.0e-12;
const DETECTOR_ALIGNMENT_SAMPLES: u32 = TRUE_PEAK_LATENCY;

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
    pub window: f64,
    pub oversampling: f64,
}

fn oversample_factor(value: f64) -> usize {
    1usize << (value.round().clamp(0.0, 3.0) as usize)
}

fn shape_weight(x: f64) -> f64 {
    let w = 0.5 - 0.5 * (std::f64::consts::PI * x).cos();
    w.sqrt() * w
}

struct ChannelState {
    delay: Vec<f32>,

    ring: Vec<f64>,
    shape_lut: Vec<f64>,
    detector: TruePeakDetector,
    gain_db: f64,
    velocity: f64,
}

impl ChannelState {
    fn new() -> Self {
        Self {
            delay: Vec::new(),
            ring: Vec::new(),
            shape_lut: build_shape_lut(4097),
            detector: TruePeakDetector::new(1),
            gain_db: 0.0,
            velocity: 0.0,
        }
    }

    fn reset(&mut self) {
        self.delay.fill(0.0);
        self.ring.fill(1.0);
        self.detector.reset();
        self.gain_db = 0.0;
        self.velocity = 0.0;
    }

    fn set_detector_factor(&mut self, factor: usize, position: usize) {
        if self.detector.factor() == factor {
            return;
        }
        let mut detector = TruePeakDetector::new(factor);
        if !self.delay.is_empty() {
            let write_pos = position % self.delay.len();
            for offset in (1..=TRUE_PEAK_TAPS).rev() {
                let index = (write_pos + self.delay.len() - offset) % self.delay.len();
                detector.detect(self.delay[index]);
            }
        }
        self.detector = detector;
    }

    fn shape_weight(&self, len: usize, index: usize) -> f64 {
        let lut = &self.shape_lut;

        let position = index as f64 * (lut.len() - 1) as f64 / (len - 1) as f64;
        let lower = position as usize;
        let upper = (lower + 1).min(lut.len() - 1);
        lerp(lut[lower], lut[upper], position - lower as f64)
    }

    fn paint(
        &mut self,
        write_pos: usize,
        ring_mask: usize,
        gain: f64,
        depth_db: f64,
        overshoot_ratio: f64,
        tail: &TailParams,
    ) {
        let n_time = (depth_db / tail.rate_db_per_sample).ceil() as usize;
        let overshoot = (overshoot_ratio * 2.0).min(1.0);
        let nbuf_reduced = tail.window_buffer_samples - tail.window_buffer_samples / 32;
        let n_overshoot = (overshoot * overshoot * nbuf_reduced as f64
            + tail.window_buffer_samples as f64 / 32.0)
            .ceil() as usize;
        let len = n_time
            .max(n_overshoot)
            .max(tail.min_window_samples)
            .min(tail.window_buffer_samples)
            .max(2);
        let depth_linear = 1.0 - gain;
        for k in 0..len {
            let correction = self.shape_weight(len, k) * depth_linear;

            if correction < tail.epsilon {
                break;
            }
            let index = (write_pos + k) & ring_mask;
            let candidate = 1.0 - correction;
            if candidate < self.ring[index] {
                self.ring[index] = candidate;
            }
        }
    }

    fn consume_target_db(&mut self, read_pos: usize, ring_mask: usize) -> f64 {
        let index = read_pos & ring_mask;
        let linear = self.ring[index].max(MIN_DETECTOR_LEVEL);
        self.ring[index] = 1.0;
        20.0 * linear.log10()
    }
}

struct TailParams {
    min_window_samples: usize,

    window_buffer_samples: usize,

    rate_db_per_sample: f64,

    epsilon: f64,
}

pub struct Limiter {
    sample_rate: f64,
    window_buffer_samples: usize,
    position: usize,
    left: ChannelState,
    right: ChannelState,
    ring_mask: usize,
    detector_factor: usize,
    reduction_l_db: f32,
    reduction_r_db: f32,
}

impl Default for Limiter {
    fn default() -> Self {
        let mut limiter = Self {
            sample_rate: 48_000.0,
            window_buffer_samples: 2,
            position: 0,
            left: ChannelState::new(),
            right: ChannelState::new(),
            ring_mask: 1,
            detector_factor: 1,
            reduction_l_db: 0.0,
            reduction_r_db: 0.0,
        };
        limiter.configure();
        limiter
    }
}

impl Limiter {
    pub fn set_sample_rate(&mut self, sr: f64) {
        self.sample_rate = sr.max(MIN_SAMPLE_RATE);
        self.configure();
    }

    pub fn reset(&mut self) {
        self.position = 0;
        self.left.reset();
        self.right.reset();
        self.reduction_l_db = 0.0;
        self.reduction_r_db = 0.0;
    }

    fn lookahead_samples_for(sample_rate: f64, lookahead_ms: f64) -> u32 {
        (sample_rate.max(MIN_SAMPLE_RATE) * lookahead_ms.clamp(0.0, MAX_LOOKAHEAD_MS) / 1000.0)
            .round() as u32
    }

    pub fn latency_samples_for(sample_rate: f64, lookahead_ms: f64, _oversampling: f64) -> u32 {
        Self::lookahead_samples_for(sample_rate, lookahead_ms) + DETECTOR_ALIGNMENT_SAMPLES
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

        let input_gain = db_to_gain(params.boost);
        let ceiling = db_to_gain(params.ceiling);
        if ceiling <= 0.0 {
            for sample in left.iter_mut().chain(right.iter_mut()) {
                *sample = 0.0;
            }
            self.reduction_l_db = 0.0;
            self.reduction_r_db = 0.0;
            return;
        }
        let hidden_gain = db_to_gain(hidden_ceiling_drive_db(params.ceiling)) as f32;
        let output_gain = db_to_gain(params.output_gain) as f32;
        let factor = oversample_factor(params.oversampling);
        if factor != self.detector_factor {
            self.detector_factor = factor;
            self.left.set_detector_factor(factor, self.position);
            self.right.set_detector_factor(factor, self.position);
        }

        let lookahead_samples = Self::lookahead_samples_for(self.sample_rate, params.lookahead_ms);
        let detector_latency = self.left.detector.latency_samples() as usize;
        let alignment_samples = DETECTOR_ALIGNMENT_SAMPLES as usize;
        let delay_samples = lookahead_samples as usize + alignment_samples;
        let transient_link = (params.link_transients / 100.0).clamp(0.0, 1.0);
        let release_link = (params.link_release / 100.0).clamp(0.0, 1.0);
        let window_fraction = (params.window / 100.0).clamp(0.0, 1.0);
        let tail = TailParams {
            min_window_samples: ((window_fraction * self.window_buffer_samples as f64).ceil()
                as usize)
                .min(self.window_buffer_samples),
            window_buffer_samples: self.window_buffer_samples,
            rate_db_per_sample: release_db_per_sample(params.release_ms, self.sample_rate),
            epsilon: tighten_epsilon(window_fraction),
        };
        let (attack_rate, release_rate) = controller_rates(params, self.sample_rate);
        let knee_threshold = ceiling * db_to_gain(KNEE_DB * 0.5);

        let mut min_gain_l = 1.0_f64;
        let mut min_gain_r = 1.0_f64;

        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            let input_l = *left as f64 * input_gain;
            let input_r = *right as f64 * input_gain;
            let detector_l = self.left.detector.detect(input_l as f32);
            let detector_r = self.right.detector.detect(input_r as f32);

            if detector_l > knee_threshold {
                let gain = (ceiling / detector_l).min(1.0);
                self.left.paint(
                    self.position,
                    self.ring_mask,
                    gain,
                    -20.0 * gain.log10(),
                    (detector_l - ceiling) / ceiling,
                    &tail,
                );
            }
            if detector_r > knee_threshold {
                let gain = (ceiling / detector_r).min(1.0);
                self.right.paint(
                    self.position,
                    self.ring_mask,
                    gain,
                    -20.0 * gain.log10(),
                    (detector_r - ceiling) / ceiling,
                    &tail,
                );
            }

            let read_pos = self
                .position
                .wrapping_sub(delay_samples)
                .wrapping_add(detector_latency)
                & self.ring_mask;
            let mut target_l = self.left.consume_target_db(read_pos, self.ring_mask);
            let mut target_r = self.right.consume_target_db(read_pos, self.ring_mask);

            let linked_target = target_l.min(target_r);
            target_l = lerp(target_l, linked_target, transient_link);
            target_r = lerp(target_r, linked_target, transient_link);

            let previous_l = self.left.gain_db;
            let previous_r = self.right.gain_db;
            let next_l = control_discrete(&mut self.left, target_l, attack_rate, release_rate);
            let next_r = control_discrete(&mut self.right, target_r, attack_rate, release_rate);

            let release_floor = next_l.min(next_r);
            let gain_l = if next_l > previous_l {
                lerp(next_l, release_floor, release_link)
            } else {
                next_l
            };
            let gain_r = if next_r > previous_r {
                lerp(next_r, release_floor, release_link)
            } else {
                next_r
            };
            self.left.gain_db = gain_l;
            self.right.gain_db = gain_r;
            let linear_l = db_to_gain(gain_l).min(1.0);
            let linear_r = db_to_gain(gain_r).min(1.0);
            min_gain_l = min_gain_l.min(linear_l);
            min_gain_r = min_gain_r.min(linear_r);

            let delay_pos_l = self.position % self.left.delay.len();
            let delay_pos_r = self.position % self.right.delay.len();
            let delayed_l = delay_sample(
                &mut self.left.delay,
                input_l as f32,
                delay_pos_l,
                delay_samples,
            );
            let delayed_r = delay_sample(
                &mut self.right.delay,
                input_r as f32,
                delay_pos_r,
                delay_samples,
            );
            self.position = (self.position + 1) & self.ring_mask;

            let limited_l = (delayed_l as f64 * linear_l).clamp(-ceiling, ceiling) as f32;
            let limited_r = (delayed_r as f64 * linear_r).clamp(-ceiling, ceiling) as f32;
            *left = limited_l * hidden_gain * output_gain;
            *right = limited_r * hidden_gain * output_gain;
        }

        self.reduction_l_db = reduction_db(min_gain_l);
        self.reduction_r_db = reduction_db(min_gain_r);
    }

    fn configure(&mut self) {
        let delay_len =
            Self::latency_samples_for(self.sample_rate, MAX_LOOKAHEAD_MS, 3.0) as usize + 1;
        self.window_buffer_samples =
            (self.sample_rate * WINDOW_BUFFER_SECONDS).ceil().max(2.0) as usize;
        let ring_len = (delay_len + self.window_buffer_samples + 2).next_power_of_two();
        self.ring_mask = ring_len - 1;
        self.position = 0;
        for channel in [&mut self.left, &mut self.right] {
            channel.delay.clear();

            channel.delay.resize(ring_len, 0.0);
            channel.ring.clear();
            channel.ring.resize(ring_len, 1.0);
        }
    }
}

fn build_shape_lut(len: usize) -> Vec<f64> {
    let denom = (len - 1).max(1) as f64;
    (0..len)
        .map(|i| {
            let x = ((len - 1 - i) as f64 / denom).min(1.0);
            shape_weight(x)
        })
        .collect()
}

fn control_discrete(
    channel: &mut ChannelState,
    target_db: f64,
    attack_rate: f64,
    release_rate: f64,
) -> f64 {
    let error = target_db - channel.gain_db;
    let desired = error.clamp(-attack_rate, release_rate);
    let jerk =
        (desired - channel.velocity).clamp(-attack_rate * JERK_FACTOR, release_rate * JERK_FACTOR);
    channel.velocity = (channel.velocity + jerk).clamp(-attack_rate, release_rate);
    let delta = channel.velocity.clamp(-attack_rate, release_rate);

    let next = channel.gain_db + delta;
    if delta < 0.0 {
        next.max(target_db)
    } else {
        next.min(target_db)
    }
}

fn controller_rates(params: &LimiterParams, sample_rate: f64) -> (f64, f64) {
    let attack_samples = (params.attack_ms.max(0.0) * sample_rate / 1000.0).max(1.0);
    let release_samples = (params.release_ms.max(0.0) * sample_rate / 1000.0).max(1.0);
    let release_rate = RELEASE_DB_RANGE / release_samples;
    let attack_rate = ATTACK_ASYMMETRY * RELEASE_DB_RANGE / attack_samples;
    (attack_rate, release_rate)
}

fn release_db_per_sample(release_ms: f64, sample_rate: f64) -> f64 {
    let release_samples = (release_ms.max(0.0) * sample_rate / 1000.0).max(1.0);
    RELEASE_DB_RANGE / release_samples
}

fn tighten_epsilon(window_fraction: f64) -> f64 {
    (7.515425574427005 * (1.0 - window_fraction))
        .exp()
        .mul_add(0.0005446181917397087, 0.0)
        .max(1.0e-6)
}

fn reduction_db(min_gain: f64) -> f32 {
    if min_gain >= 1.0 {
        0.0
    } else {
        (-20.0 * min_gain.max(MIN_DETECTOR_LEVEL).log10()).clamp(0.0, 60.0) as f32
    }
}

fn db_to_gain(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}

fn hidden_ceiling_drive_db(ceiling_db: f64) -> f64 {
    -ceiling_db.clamp(-90.0, 0.0)
}

fn lerp(a: f64, b: f64, amount: f64) -> f64 {
    a + (b - a) * amount
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
            window: 25.0,
            oversampling: 0.0,
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
        let latency =
            Limiter::latency_samples_for(48_000.0, params.lookahead_ms, params.oversampling)
                as usize;
        assert!(right[latency] < 0.5);
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
        let latency =
            Limiter::latency_samples_for(48_000.0, params.lookahead_ms, params.oversampling)
                as usize;
        assert!((right[latency] - expected).abs() < 1.0e-6);
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

        let latency =
            Limiter::latency_samples_for(48_000.0, params.lookahead_ms, params.oversampling)
                as usize;
        assert!(driven_left[latency] > unchanged_left[latency]);
        assert!(driven_right[latency] > unchanged_right[latency]);
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
        assert_eq!(Limiter::latency_samples_for(48_000.0, 1.0, 0.0), 48 + 8);
        assert_eq!(Limiter::latency_samples_for(48_000.0, 1.0, 1.0), 48 + 8);
        assert_eq!(Limiter::latency_samples_for(48_000.0, 1.0, 2.0), 48 + 8);
        assert_eq!(Limiter::latency_samples_for(48_000.0, 1.0, 3.0), 48 + 8);
    }

    #[test]
    fn quiet_audio_stays_continuous_across_buffer_wraps() {
        for oversampling in [0.0, 1.0, 2.0, 3.0] {
            for lookahead_ms in [0.0, 1.0, 20.0] {
                let mut limiter = Limiter::default();
                let mut params = params();
                params.ceiling = 0.0;
                params.oversampling = oversampling;
                params.lookahead_ms = lookahead_ms;
                let input: Vec<f32> = (0..12_000)
                    .map(|i| 0.25 * (i as f32 * 0.057).sin())
                    .collect();
                let mut left = input.clone();
                let mut right = input.clone();
                for (left, right) in left.chunks_mut(127).zip(right.chunks_mut(127)) {
                    limiter.process_stereo(left, right, &params);
                }
                let latency =
                    Limiter::latency_samples_for(48_000.0, lookahead_ms, oversampling) as usize;
                for i in latency..input.len() {
                    assert_eq!(
                        left[i],
                        input[i - latency],
                        "sample {i}, oversampling {oversampling}, lookahead {lookahead_ms}"
                    );
                    assert_eq!(right[i], input[i - latency]);
                }
            }
        }
    }

    #[test]
    fn negative_peaks_engage_gain_reduction() {
        let mut limiter = Limiter::default();
        let mut params = params();
        params.link_transients = 0.0;
        params.link_release = 0.0;
        let mut left = vec![2.0; 4096];
        let mut right = vec![-2.0; 4096];
        limiter.process_stereo(&mut left, &mut right, &params);
        let reduction = limiter.gain_reduction_db();
        assert!(reduction[1] > 1.0);
        assert_eq!(reduction[0], reduction[1]);
        for (left, right) in left.iter().zip(&right) {
            assert_eq!(*left, -*right);
        }
    }

    #[test]
    fn switching_oversampling_keeps_delayed_audio_continuous() {
        let sample_rate = 48_000.0;
        let mut limiter = Limiter::default();
        limiter.set_sample_rate(sample_rate);
        let mut params = params();
        params.ceiling = 0.0;
        params.lookahead_ms = 0.0;
        params.link_transients = 0.0;
        params.link_release = 0.0;

        let input: Vec<f32> = (0..512)
            .map(|i| {
                let phase = 2.0 * std::f64::consts::PI * 440.0 * i as f64 / sample_rate;
                (0.25 * phase.sin()) as f32
            })
            .collect();
        let mut output = Vec::with_capacity(input.len());

        for (i, &sample) in input.iter().enumerate() {
            if i == 256 {
                params.oversampling = 3.0;
            }
            let mut left = [sample];
            let mut right = [sample];
            limiter.process_stereo(&mut left, &mut right, &params);
            output.push(left[0]);
        }

        let latency = Limiter::latency_samples_for(sample_rate, params.lookahead_ms, 0.0) as usize;
        for i in latency..output.len() {
            let expected = input[i - latency];
            assert!(
                (output[i] - expected).abs() < 1.0e-6,
                "oversampling switch changed delayed audio at sample {i}: {} vs {expected}",
                output[i]
            );
        }
    }

    #[test]
    fn true_peak_detection_engages_on_inter_sample_peaks() {
        let sample_rate = 48_000.0;
        let freq = 12_000.0;
        let amplitude = 1.1;
        let n = 4096;
        let signal: Vec<f32> = (0..n)
            .map(|i| {
                (amplitude
                    * (2.0 * std::f64::consts::PI * freq * (i as f64 + 0.5) / sample_rate).sin())
                    as f32
            })
            .collect();

        let run = |oversampling: f64| {
            let mut limiter = Limiter::default();
            limiter.set_sample_rate(sample_rate);
            let mut left = signal.clone();
            let mut right = signal.clone();
            let mut params = params();
            params.lookahead_ms = 0.0;
            params.attack_ms = 0.0;
            params.release_ms = 100.0;
            params.link_transients = 0.0;
            params.link_release = 0.0;
            params.oversampling = oversampling;
            limiter.process_stereo(&mut left, &mut right, &params);
            limiter.gain_reduction_db()
        };

        assert!(
            run(0.0)[0] < 0.05,
            "base-rate detector should stay silent, got {:?}",
            run(0.0)
        );

        let reduction = run(3.0)[0];
        assert!(
            reduction > 1.0,
            "true-peak detector should engage, got {reduction}"
        );
    }

    #[test]
    fn bounds_steady_state_output() {
        let mut limiter = Limiter::default();
        limiter.set_sample_rate(48_000.0);
        let mut left = vec![1.5; 4096];
        let mut right = vec![1.5; 4096];
        let mut params = params();
        params.lookahead_ms = 0.0;
        params.attack_ms = 1.0;
        params.release_ms = 100.0;
        limiter.process_stereo(&mut left, &mut right, &params);
        let post_makeup_ceiling =
            db_to_gain(params.ceiling + hidden_ceiling_drive_db(params.ceiling)) as f32;
        assert!(
            left.iter()
                .chain(&right)
                .all(|sample| sample.abs() <= post_makeup_ceiling + 1.0e-6),
            "limiter exceeded the ceiling"
        );
    }

    #[test]
    fn window_sets_minimum_envelope_tail_length() {
        let mut impulse = vec![0.0_f32; 2048];
        impulse[0] = 1.0;
        let run = |window: f64| {
            let mut limiter = Limiter::default();
            limiter.set_sample_rate(48_000.0);
            let mut left = impulse.clone();
            let mut right = impulse.clone();
            let mut params = params();
            params.lookahead_ms = 0.0;
            params.attack_ms = 0.0;
            params.release_ms = 999.0;
            params.window = window;
            limiter.process_stereo(&mut left, &mut right, &params);
            limiter.gain_reduction_db()
        };
        let wide = run(100.0);
        let narrow = run(0.0);
        assert!(
            wide[0] > 0.05,
            "wide window should sustain GR, got {wide:?}"
        );
        assert!(
            narrow[0] < 0.05,
            "zero window should release immediately, got {narrow:?}"
        );
    }

    #[test]
    fn envelope_ring_does_not_ghost_across_wraparound() {
        let mut limiter = Limiter::default();
        limiter.set_sample_rate(48_000.0);
        let mut params = params();
        params.lookahead_ms = 0.0;
        params.attack_ms = 0.0;
        params.release_ms = 1.0;
        params.link_transients = 0.0;
        params.link_release = 0.0;
        params.window = 100.0;

        let mut block1 = vec![0.0_f32; 2048];
        block1[0] = 2.0;

        let mut left = block1.clone();
        let mut right = block1.clone();
        limiter.process_stereo(&mut left, &mut right, &params);
        let first = limiter.gain_reduction_db();
        assert!(first[0] > 1.0, "first block should limit, got {first:?}");

        let mut left = vec![0.0_f32; 2048];
        let mut right = vec![0.0_f32; 2048];
        limiter.process_stereo(&mut left, &mut right, &params);
        let second = limiter.gain_reduction_db();
        assert!(
            second[0] < 0.1,
            "ghost reduction after wraparound: {second:?}"
        );
    }

    #[test]
    fn discrete_controller_never_overshoots_target() {
        let mut channel = ChannelState::new();
        let attack_rate = 4.0;
        let release_rate = 0.5;
        let mut target = 0.0;
        for i in 0..512 {
            if i == 16 {
                target = -7.0;
            }
            if i == 300 {
                target = 0.0;
            }
            let before = channel.gain_db;
            let gain = control_discrete(&mut channel, target, attack_rate, release_rate);

            if gain < before {
                assert!(gain >= target - 1.0e-9, "attack crossed target: {gain}");
            } else if gain > before {
                assert!(gain <= target + 1.0e-9, "release crossed target: {gain}");
            }
            channel.gain_db = gain;
        }
    }

    #[test]
    fn adaptive_shape_lookup_preserves_envelopes() {
        let channel = ChannelState::new();
        for len in [2, 23, 147, 734, 2936] {
            for (i, expected) in build_shape_lut(len).into_iter().enumerate() {
                let actual = channel.shape_weight(len, i);
                assert!((actual - expected).abs() < 2.0e-7);
            }
        }
    }

    #[test]
    fn shape_lut_matches_al1_weights() {
        let len = 65;
        let cosine = build_shape_lut(len);
        assert!((cosine[0] - 1.0).abs() < 1.0e-12);
        assert!(cosine[len - 1].abs() < 1.0e-12);

        let mid = 0.5_f64.powf(1.5);
        assert!((cosine[len / 2] - mid).abs() < 1.0e-12);
    }
}
