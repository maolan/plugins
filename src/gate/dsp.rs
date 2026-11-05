const MIN_LEVEL: f32 = 1.0e-10;
const DETECTOR_TIME_SECONDS: f32 = 0.005;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateStatus {
    Closed = 0,
    Holding = 1,
    Open = 2,
}

#[derive(Debug, Clone, Copy)]
pub struct GateParams {
    pub threshold_db: f32,
    pub ratio: f32,
    pub range_db: f32,
    pub attack_ms: f32,
    pub hold_ms: f32,
    pub release_ms: f32,
    pub hysteresis_db: f32,
    pub sidechain_hpf_hz: f32,
    pub sidechain_lpf_hz: f32,
    pub monitor: bool,
}

impl Default for GateParams {
    fn default() -> Self {
        Self {
            threshold_db: -50.0,
            ratio: 4.0,
            range_db: 80.0,
            attack_ms: 1.0,
            hold_ms: 50.0,
            release_ms: 150.0,
            hysteresis_db: 3.0,
            sidechain_hpf_hz: 20.0,
            sidechain_lpf_hz: 20_000.0,
            monitor: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GateMeter {
    pub detector_db: f32,
    pub gain_reduction_db: f32,
    pub status: GateStatus,
}

#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
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

impl Biquad {
    fn set_coefficients(&mut self, b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) {
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    fn set_highpass(&mut self, cutoff_hz: f32, sample_rate: f32) {
        let cutoff_hz = cutoff_hz.clamp(1.0, sample_rate * 0.45);
        let w0 = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / 2.0_f32.sqrt();
        self.set_coefficients(
            (1.0 + cos) * 0.5,
            -(1.0 + cos),
            (1.0 + cos) * 0.5,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        );
    }

    fn set_lowpass(&mut self, cutoff_hz: f32, sample_rate: f32) {
        let cutoff_hz = cutoff_hz.clamp(1.0, sample_rate * 0.45);
        let w0 = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / 2.0_f32.sqrt();
        self.set_coefficients(
            (1.0 - cos) * 0.5,
            1.0 - cos,
            (1.0 - cos) * 0.5,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        );
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.b0 * input + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

#[derive(Debug, Clone)]
pub struct Gate {
    sample_rate: f32,
    hpf_l: Biquad,
    hpf_r: Biquad,
    lpf_l: Biquad,
    lpf_r: Biquad,
    detector_level: f32,
    gain_db: f32,
    hold_samples_remaining: usize,
    status: GateStatus,
    initialized: bool,
    detector_coefficient: f32,
    attack_coefficient: f32,
    release_coefficient: f32,
    hold_samples: usize,
    configured_hpf_hz: f32,
    configured_lpf_hz: f32,
    configured_attack_ms: f32,
    configured_release_ms: f32,
    configured_hold_ms: f32,
}

impl Default for Gate {
    fn default() -> Self {
        let sample_rate = 48_000.0;
        Self {
            sample_rate,
            hpf_l: Biquad::default(),
            hpf_r: Biquad::default(),
            lpf_l: Biquad::default(),
            lpf_r: Biquad::default(),
            detector_level: MIN_LEVEL,
            gain_db: 0.0,
            hold_samples_remaining: 0,
            status: GateStatus::Closed,
            initialized: false,
            detector_coefficient: time_coefficient(DETECTOR_TIME_SECONDS, sample_rate),
            attack_coefficient: 1.0,
            release_coefficient: 1.0,
            hold_samples: 0,
            configured_hpf_hz: f32::NAN,
            configured_lpf_hz: f32::NAN,
            configured_attack_ms: f32::NAN,
            configured_release_ms: f32::NAN,
            configured_hold_ms: f32::NAN,
        }
    }
}

impl Gate {
    pub fn new(sample_rate: f32) -> Self {
        let mut gate = Self::default();
        gate.set_sample_rate(sample_rate);
        gate
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.detector_coefficient = time_coefficient(DETECTOR_TIME_SECONDS, self.sample_rate);
        self.configured_hpf_hz = f32::NAN;
        self.configured_lpf_hz = f32::NAN;
        self.configured_attack_ms = f32::NAN;
        self.configured_release_ms = f32::NAN;
        self.configured_hold_ms = f32::NAN;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.hpf_l.reset();
        self.hpf_r.reset();
        self.lpf_l.reset();
        self.lpf_r.reset();
        self.detector_level = MIN_LEVEL;
        self.gain_db = 0.0;
        self.hold_samples_remaining = 0;
        self.status = GateStatus::Closed;
        self.initialized = false;
    }

    fn configure(&mut self, params: &GateParams) {
        let hpf_hz = params.sidechain_hpf_hz.max(1.0);
        let lpf_hz = params.sidechain_lpf_hz.max(1.0);
        if self.configured_hpf_hz != hpf_hz {
            self.hpf_l.set_highpass(hpf_hz, self.sample_rate);
            self.hpf_r.set_highpass(hpf_hz, self.sample_rate);
            self.configured_hpf_hz = hpf_hz;
        }
        if self.configured_lpf_hz != lpf_hz {
            self.lpf_l.set_lowpass(lpf_hz, self.sample_rate);
            self.lpf_r.set_lowpass(lpf_hz, self.sample_rate);
            self.configured_lpf_hz = lpf_hz;
        }
        if self.configured_attack_ms != params.attack_ms {
            self.attack_coefficient = time_coefficient(params.attack_ms * 0.001, self.sample_rate);
            self.configured_attack_ms = params.attack_ms;
        }
        if self.configured_release_ms != params.release_ms {
            self.release_coefficient =
                time_coefficient(params.release_ms * 0.001, self.sample_rate);
            self.configured_release_ms = params.release_ms;
        }
        if self.configured_hold_ms != params.hold_ms {
            self.hold_samples =
                (params.hold_ms.max(0.0) * 0.001 * self.sample_rate).round() as usize;
            self.configured_hold_ms = params.hold_ms;
        }
    }

    fn expansion_gain_db(level_db: f32, close_threshold_db: f32, ratio: f32, range_db: f32) -> f32 {
        if level_db >= close_threshold_db {
            return 0.0;
        }
        let ratio = ratio.max(1.0);
        let reduction = (close_threshold_db - level_db) * (1.0 - ratio.recip());
        -reduction.min(range_db.max(0.0))
    }

    fn target_gain_db(&mut self, level_db: f32, params: &GateParams) -> f32 {
        let close_threshold_db = params.threshold_db - params.hysteresis_db.max(0.0);
        match self.status {
            GateStatus::Open => {
                if level_db >= close_threshold_db {
                    0.0
                } else if self.hold_samples > 0 {
                    self.status = GateStatus::Holding;
                    self.hold_samples_remaining = self.hold_samples;
                    self.target_gain_db(level_db, params)
                } else {
                    self.status = GateStatus::Closed;
                    Self::expansion_gain_db(
                        level_db,
                        close_threshold_db,
                        params.ratio,
                        params.range_db,
                    )
                }
            }
            GateStatus::Holding => {
                if level_db >= close_threshold_db {
                    self.status = GateStatus::Open;
                    self.hold_samples_remaining = 0;
                    0.0
                } else if self.hold_samples_remaining > 0 {
                    self.hold_samples_remaining -= 1;
                    0.0
                } else {
                    self.status = GateStatus::Closed;
                    Self::expansion_gain_db(
                        level_db,
                        close_threshold_db,
                        params.ratio,
                        params.range_db,
                    )
                }
            }
            GateStatus::Closed => {
                if level_db >= params.threshold_db {
                    self.status = GateStatus::Open;
                    self.hold_samples_remaining = 0;
                    0.0
                } else {
                    Self::expansion_gain_db(
                        level_db,
                        close_threshold_db,
                        params.ratio,
                        params.range_db,
                    )
                }
            }
        }
    }

    fn smooth_gain(&mut self, target_db: f32) -> f32 {
        if !self.initialized {
            self.gain_db = target_db;
            self.initialized = true;
            return self.gain_db;
        }
        let coefficient = if target_db > self.gain_db {
            self.attack_coefficient
        } else {
            self.release_coefficient
        };
        self.gain_db += coefficient * (target_db - self.gain_db);
        self.gain_db
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        detector_left: &[f32],
        detector_right: &[f32],
        params: &GateParams,
    ) -> GateMeter {
        self.configure(params);
        let frames = left
            .len()
            .min(right.len())
            .min(detector_left.len())
            .min(detector_right.len());
        let mut detector_db = 20.0 * self.detector_level.max(MIN_LEVEL).log10();

        for i in 0..frames {
            let input_l = finite_or_zero(left[i]);
            let input_r = finite_or_zero(right[i]);
            let key_l = self
                .lpf_l
                .process(self.hpf_l.process(finite_or_zero(detector_left[i])));
            let key_r = self
                .lpf_r
                .process(self.hpf_r.process(finite_or_zero(detector_right[i])));
            let peak = key_l.abs().max(key_r.abs()).max(MIN_LEVEL);
            self.detector_level += self.detector_coefficient * (peak - self.detector_level);
            self.detector_level = self.detector_level.clamp(MIN_LEVEL, 1.0e6);
            detector_db = 20.0 * self.detector_level.log10();

            let target_db = self.target_gain_db(detector_db, params);
            let gain_db = self.smooth_gain(target_db);
            if params.monitor {
                left[i] = key_l;
                right[i] = key_r;
            } else {
                let gain = 10.0_f32.powf(gain_db * 0.05);
                left[i] = input_l * gain;
                right[i] = input_r * gain;
            }
        }

        GateMeter {
            detector_db,
            gain_reduction_db: -self.gain_db,
            status: self.status,
        }
    }
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn time_coefficient(time_seconds: f32, sample_rate: f32) -> f32 {
    if time_seconds <= 0.0 {
        1.0
    } else {
        let samples = (time_seconds * sample_rate).max(1.0);
        let target = 1.0 - std::f32::consts::FRAC_1_SQRT_2;
        1.0 - (target.ln() / samples).exp()
    }
}

#[cfg(test)]
mod tests {
    use super::{Gate, GateParams, GateStatus};

    fn params() -> GateParams {
        GateParams {
            threshold_db: -20.0,
            attack_ms: 0.0,
            hold_ms: 0.0,
            release_ms: 0.0,
            ..GateParams::default()
        }
    }

    fn key_signal(input: f32, frames: usize, sample_rate: f32) -> Vec<f32> {
        let frequency = sample_rate * 0.05;
        (0..frames)
            .map(|i| {
                input * (2.0 * std::f32::consts::PI * frequency * i as f32 / sample_rate).sin()
            })
            .collect()
    }

    fn process_constant(gate: &mut Gate, input: f32, frames: usize, params: &GateParams) {
        let mut left = key_signal(input, frames, gate.sample_rate);
        let mut right = key_signal(input, frames, gate.sample_rate);
        let detector_left = left.clone();
        let detector_right = right.clone();
        gate.process_stereo(
            &mut left,
            &mut right,
            &detector_left,
            &detector_right,
            params,
        );
    }

    #[test]
    fn expansion_is_ratio_and_range_limited() {
        assert_eq!(Gate::expansion_gain_db(-40.0, -20.0, 4.0, 80.0), -15.0);
        assert_eq!(Gate::expansion_gain_db(-100.0, -20.0, 4.0, 30.0), -30.0);
        assert_eq!(Gate::expansion_gain_db(-10.0, -20.0, 4.0, 80.0), 0.0);
        assert_eq!(Gate::expansion_gain_db(-40.0, -20.0, 1.0, 80.0), 0.0);
    }

    #[test]
    fn loud_signal_passes_and_silence_reaches_full_range() {
        let mut gate = Gate::new(48_000.0);
        let params = GateParams {
            ratio: 100.0,
            ..params()
        };
        process_constant(&mut gate, 1.0, 4096, &params);
        let mut left = vec![1.0; 64];
        let mut right = vec![1.0; 64];
        let detector_left = key_signal(1.0, 64, 48_000.0);
        let detector_right = key_signal(1.0, 64, 48_000.0);
        let meter = gate.process_stereo(
            &mut left,
            &mut right,
            &detector_left,
            &detector_right,
            &params,
        );
        assert_eq!(meter.status, GateStatus::Open);
        assert!(left.iter().all(|&sample| (sample - 1.0).abs() < 1.0e-4));
        assert!(right.iter().all(|&sample| (sample - 1.0).abs() < 1.0e-4));

        process_constant(&mut gate, 0.0, 4096, &params);
        let left = vec![0.0; 1];
        let right = vec![0.0; 1];
        let mut output_l = vec![1.0];
        let mut output_r = vec![1.0];
        let meter = gate.process_stereo(&mut output_l, &mut output_r, &left, &right, &params);
        assert!(
            meter.gain_reduction_db >= 79.9,
            "meter: {meter:?}, output: {}",
            output_l[0]
        );
        assert!(output_l[0].abs() < 1.0e-3);
        assert!(output_r[0].abs() < 1.0e-3);
    }

    #[test]
    fn hold_is_counted_in_samples() {
        let mut gate = Gate::new(1000.0);
        let params = GateParams {
            hold_ms: 50.0,
            ..params()
        };
        process_constant(&mut gate, 1.0, 1024, &params);
        assert_eq!(gate.status, GateStatus::Open);

        let left = vec![0.0; 50];
        let right = vec![0.0; 50];
        let mut output_l = vec![1.0; 50];
        let mut output_r = vec![1.0; 50];
        let meter = gate.process_stereo(&mut output_l, &mut output_r, &left, &right, &params);
        assert_eq!(meter.status, GateStatus::Holding);
        assert!(meter.gain_reduction_db.abs() < 1.0e-4);

        let mut output_l = vec![1.0; 128];
        let mut output_r = vec![1.0; 128];
        let meter = gate.process_stereo(&mut output_l, &mut output_r, &left, &right, &params);
        assert_eq!(meter.status, GateStatus::Closed);
    }

    #[test]
    fn hysteresis_requires_crossing_both_thresholds() {
        let mut gate = Gate::new(1000.0);
        let params = GateParams {
            hysteresis_db: 3.0,
            ..params()
        };
        process_constant(&mut gate, 1.0, 1024, &params);
        gate.detector_level = 10.0_f32.powf(-21.0 / 20.0);
        assert_eq!(gate.target_gain_db(-21.0, &params), 0.0);
        assert_eq!(gate.status, GateStatus::Open);

        gate.status = GateStatus::Closed;
        gate.target_gain_db(-21.0, &params);
        assert_eq!(gate.status, GateStatus::Closed);
        gate.target_gain_db(-19.0, &params);
        assert_eq!(gate.status, GateStatus::Open);
    }

    #[test]
    fn stereo_channels_share_one_gain_envelope() {
        let mut gate = Gate::new(48_000.0);
        let params = params();
        let detector_left = key_signal(1.0, 4096, 48_000.0);
        let detector_right = vec![0.0; 4096];
        let mut left = vec![1.0; 4096];
        let mut right = vec![1.0; 4096];
        gate.process_stereo(
            &mut left,
            &mut right,
            &detector_left,
            &detector_right,
            &params,
        );
        let last = left.len() - 1;
        assert!((left[last] - right[last]).abs() < 1.0e-6);
        assert!(right[last] > 0.99);
    }

    #[test]
    fn sidechain_highpass_rejects_low_frequency_key() {
        let params = GateParams {
            sidechain_hpf_hz: 1000.0,
            ..params()
        };
        let mut low_gate = Gate::new(48_000.0);
        let mut high_gate = Gate::new(48_000.0);
        let low: Vec<f32> = (0..8192)
            .map(|i| (2.0 * std::f32::consts::PI * 50.0 * i as f32 / 48_000.0).sin())
            .collect();
        let high: Vec<f32> = (0..8192)
            .map(|i| (2.0 * std::f32::consts::PI * 5000.0 * i as f32 / 48_000.0).sin())
            .collect();
        let mut output_l = vec![1.0; 8192];
        let mut output_r = vec![1.0; 8192];
        let low_meter = low_gate.process_stereo(&mut output_l, &mut output_r, &low, &low, &params);
        let high_meter =
            high_gate.process_stereo(&mut output_l, &mut output_r, &high, &high, &params);
        assert_eq!(low_meter.status, GateStatus::Closed);
        assert_eq!(high_meter.status, GateStatus::Open);
    }

    #[test]
    fn monitor_outputs_filtered_key_instead_of_gated_audio() {
        let mut gate = Gate::new(48_000.0);
        let params = GateParams {
            monitor: true,
            sidechain_hpf_hz: 1000.0,
            ..params()
        };
        let key = vec![0.0; 256];
        let mut left = vec![1.0; 256];
        let mut right = vec![1.0; 256];
        gate.process_stereo(&mut left, &mut right, &key, &key, &params);
        assert!(left.iter().all(|&sample| sample == 0.0));
        assert!(right.iter().all(|&sample| sample == 0.0));
    }

    #[test]
    fn non_finite_inputs_are_sanitized() {
        let mut gate = Gate::new(48_000.0);
        let params = params();
        let key = vec![f32::NAN, f32::INFINITY, 0.0, 0.0];
        let mut left = vec![f32::NAN, f32::NEG_INFINITY, 1.0, 0.0];
        let mut right = left.clone();
        gate.process_stereo(&mut left, &mut right, &key, &key, &params);
        assert!(left.iter().all(|sample| sample.is_finite()));
        assert!(right.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn reset_restores_closed_uninitialized_state() {
        let mut gate = Gate::new(48_000.0);
        process_constant(&mut gate, 1.0, 4096, &params());
        assert_eq!(gate.status, GateStatus::Open);
        gate.reset();
        assert_eq!(gate.status, GateStatus::Closed);
        assert!(!gate.initialized);
        assert_eq!(gate.hold_samples_remaining, 0);
    }
}
