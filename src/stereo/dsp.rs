#[derive(Debug, Clone, Copy)]
pub struct StereoParams {
    pub output_gain_db: f64,
    pub boost: f64,
    pub low_gain: f64,
    pub mid_gain: f64,
    pub high_gain: f64,
    pub low_delay: f64,
    pub mid_delay: f64,
    pub high_delay: f64,
    pub solo_low: bool,
    pub solo_mid: bool,
    pub solo_high: bool,
    pub x1: f64,
    pub x2: f64,
    pub strength: f64,
    pub monitor_mode: u8,
    pub bypass: bool,
    pub gain_on: bool,
    pub delay_on: bool,
    pub character_on: bool,
    pub density: f64,
    pub focus: f64,
    pub amount: f64,
}

#[derive(Debug, Clone)]
pub struct Mild {
    p: Vec<f64>,
    count: i32,
    fpd_l: u32,
    fpd_r: u32,
    sample_rate: f64,
    temp_dry_l: Vec<f32>,
    temp_dry_r: Vec<f32>,
    temp_wet_l: Vec<f32>,
    temp_wet_r: Vec<f32>,
}

impl Default for Mild {
    fn default() -> Self {
        Self {
            p: vec![0.0; 4099],
            count: 2048,
            fpd_l: rand::random(),
            fpd_r: rand::random(),
            sample_rate: 48_000.0,
            temp_dry_l: vec![0.0; 1024],
            temp_dry_r: vec![0.0; 1024],
            temp_wet_l: vec![0.0; 1024],
            temp_wet_r: vec![0.0; 1024],
        }
    }
}

impl Mild {
    pub fn reset(&mut self) {
        self.p.fill(0.0);
        self.count = 2048;
        self.fpd_l = rand::random();
        self.fpd_r = rand::random();
        self.temp_dry_l.fill(0.0);
        self.temp_dry_r.fill(0.0);
        self.temp_wet_l.fill(0.0);
        self.temp_wet_r.fill(0.0);
    }

    pub fn set_sample_rate(&mut self, sr: f64) {
        self.sample_rate = sr;
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        density: f64,
        focus: f64,
        amount: f64,
    ) {
        let overallscale = self.sample_rate / 44_100.0;
        let densityside = density * 2.0 - 1.0;
        let densitymid = focus * 2.0 - 1.0;
        let wet = amount * 0.5;

        let mut offset = (densityside - densitymid) / 2.0;
        if offset > 0.0 {
            offset = offset.sin();
        }
        if offset < 0.0 {
            offset = -(-offset).sin();
        }
        offset = -(offset.powi(4) * 20.0 * overallscale);
        let near = offset.abs().floor() as i32;
        let far_level = offset.abs() - near as f64;
        let far = near + 1;
        let near_level = 1.0 - far_level;

        let frames = left.len().min(right.len());
        if self.temp_dry_l.len() < frames {
            let new_len = frames.next_power_of_two();
            self.temp_dry_l.resize(new_len, 0.0);
            self.temp_dry_r.resize(new_len, 0.0);
            self.temp_wet_l.resize(new_len, 0.0);
            self.temp_wet_r.resize(new_len, 0.0);
        }

        for i in 0..frames {
            let mut input_l = left[i] as f64;
            let mut input_r = right[i] as f64;

            if input_l.abs() < 1.18e-23 {
                input_l = self.fpd_l as f64 * 1.18e-17;
            }
            if input_r.abs() < 1.18e-23 {
                input_r = self.fpd_r as f64 * 1.18e-17;
            }

            let dry_l = input_l;
            let dry_r = input_r;

            let mut mid = input_l + input_r;
            let mut side = input_l - input_r;

            if densityside != 0.0 {
                let out = densityside.abs();
                let mut bridgerectifier = (side.abs() * std::f64::consts::FRAC_PI_2)
                    .clamp(0.0, std::f64::consts::FRAC_PI_2);
                if densityside > 0.0 {
                    bridgerectifier = bridgerectifier.sin();
                } else {
                    bridgerectifier = 1.0 - bridgerectifier.cos();
                }
                if side > 0.0 {
                    side = side * (1.0 - out) + bridgerectifier * out;
                } else {
                    side = side * (1.0 - out) - bridgerectifier * out;
                }
            }

            if densitymid != 0.0 {
                let out = densitymid.abs();
                let mut bridgerectifier = (mid.abs() * std::f64::consts::FRAC_PI_2)
                    .clamp(0.0, std::f64::consts::FRAC_PI_2);
                if densitymid > 0.0 {
                    bridgerectifier = bridgerectifier.sin();
                } else {
                    bridgerectifier = 1.0 - bridgerectifier.cos();
                }
                if mid > 0.0 {
                    mid = mid * (1.0 - out) + bridgerectifier * out;
                } else {
                    mid = mid * (1.0 - out) - bridgerectifier * out;
                }
            }

            if self.count < 1 || self.count > 2048 {
                self.count = 2048;
            }
            let count = self.count as usize;

            if offset > 0.0 {
                self.p[count] = mid;
                self.p[count + 2048] = mid;
                mid = self.p[count + near as usize] * near_level;
                mid += self.p[count + far as usize] * far_level;
            }

            if offset < 0.0 {
                self.p[count] = side;
                self.p[count + 2048] = side;
                side = self.p[count + near as usize] * near_level;
                side += self.p[count + far as usize] * far_level;
            }
            self.count -= 1;

            self.temp_dry_l[i] = dry_l as f32;
            self.temp_dry_r[i] = dry_r as f32;
            self.temp_wet_l[i] = (mid + side) as f32;
            self.temp_wet_r[i] = (mid - side) as f32;
        }

        let wet_f = wet as f32;
        let dry_f = (1.0 - wet) as f32;
        left[..frames].copy_from_slice(&self.temp_wet_l[..frames]);
        right[..frames].copy_from_slice(&self.temp_wet_r[..frames]);
        crate::simd::mul_inplace(&mut left[..frames], wet_f);
        crate::simd::mul_inplace(&mut right[..frames], wet_f);
        crate::simd::add_scaled_inplace(&mut left[..frames], &self.temp_dry_l[..frames], dry_f);
        crate::simd::add_scaled_inplace(&mut right[..frames], &self.temp_dry_r[..frames], dry_f);

        for i in 0..frames {
            let mut input_l = left[i] as f64;
            let mut input_r = right[i] as f64;
            let mut expon = input_l.abs().log2().floor() as i32;
            self.fpd_l ^= self.fpd_l << 13;
            self.fpd_l ^= self.fpd_l >> 17;
            self.fpd_l ^= self.fpd_l << 5;
            input_l +=
                (self.fpd_l as f64 - 0x7fff_ffffu32 as f64) * 5.5e-36 * 2.0_f64.powi(expon + 62);

            expon = input_r.abs().log2().floor() as i32;
            self.fpd_r ^= self.fpd_r << 13;
            self.fpd_r ^= self.fpd_r >> 17;
            self.fpd_r ^= self.fpd_r << 5;
            input_r +=
                (self.fpd_r as f64 - 0x7fff_ffffu32 as f64) * 5.5e-36 * 2.0_f64.powi(expon + 62);

            left[i] = input_l as f32;
            right[i] = input_r as f32;
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ParamSmoother {
    current: f64,
    target: f64,
}

impl ParamSmoother {
    fn new(value: f64) -> Self {
        Self {
            current: value,
            target: value,
        }
    }

    fn reset(&mut self, value: f64) {
        self.current = value;
        self.target = value;
    }
}

#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn lowpass(freq: f64, sample_rate: f64) -> Self {
        let nyquist = 0.5 * sample_rate.max(1.0);
        let freq = freq.clamp(10.0, nyquist - 1.0);
        let k = (std::f64::consts::PI * freq / sample_rate.max(1.0)).tan();
        let sqrt2 = std::f64::consts::SQRT_2;
        let kk = k * k;
        let norm = 1.0 / (kk + k * sqrt2 + 1.0);
        Self {
            b0: kk * norm,
            b1: 2.0 * kk * norm,
            b2: kk * norm,
            a1: 2.0 * (kk - 1.0) * norm,
            a2: (kk - k * sqrt2 + 1.0) * norm,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn highpass(freq: f64, sample_rate: f64) -> Self {
        let nyquist = 0.5 * sample_rate.max(1.0);
        let freq = freq.clamp(10.0, nyquist - 1.0);
        let k = (std::f64::consts::PI * freq / sample_rate.max(1.0)).tan();
        let sqrt2 = std::f64::consts::SQRT_2;
        let kk = k * k;
        let norm = 1.0 / (kk + k * sqrt2 + 1.0);
        Self {
            b0: 1.0 * norm,
            b1: -2.0 * norm,
            b2: 1.0 * norm,
            a1: 2.0 * (kk - 1.0) * norm,
            a2: (kk - k * sqrt2 + 1.0) * norm,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        let w = input - self.a1 * self.z1 - self.a2 * self.z2;
        let output = self.b0 * w + self.b1 * self.z1 + self.b2 * self.z2;
        self.z2 = self.z1;
        self.z1 = w;
        output
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

#[derive(Debug, Clone, Copy)]
struct Lr4 {
    stage1: Biquad,
    stage2: Biquad,
}

impl Lr4 {
    fn lowpass(freq: f64, sample_rate: f64) -> Self {
        Self {
            stage1: Biquad::lowpass(freq, sample_rate),
            stage2: Biquad::lowpass(freq, sample_rate),
        }
    }

    fn highpass(freq: f64, sample_rate: f64) -> Self {
        Self {
            stage1: Biquad::highpass(freq, sample_rate),
            stage2: Biquad::highpass(freq, sample_rate),
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        self.stage2.process(self.stage1.process(input))
    }

    fn reset(&mut self) {
        self.stage1.reset();
        self.stage2.reset();
    }
}

#[derive(Debug, Clone)]
struct DelayLine {
    buffer: Vec<f64>,
    write_idx: usize,
}

impl DelayLine {
    fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size.max(1)],
            write_idx: 0,
        }
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_idx = 0;
    }

    fn write(&mut self, sample: f64) {
        self.buffer[self.write_idx] = sample;
        self.write_idx += 1;
        if self.write_idx >= self.buffer.len() {
            self.write_idx = 0;
        }
    }

    fn read(&self, delay_samples: f64) -> f64 {
        let len = self.buffer.len();
        let read_pos = (self.write_idx as f64 - delay_samples).rem_euclid(len as f64);
        let i0 = read_pos.floor() as usize;
        let frac = read_pos - read_pos.floor();
        let s0 = self.buffer[i0 % len];
        let s1 = self.buffer[(i0 + 1) % len];
        s0 + frac * (s1 - s0)
    }
}

#[derive(Debug, Clone)]
pub struct Stereo {
    sample_rate: f64,
    x1: f64,
    x2: f64,
    lp_x1_l: Lr4,
    lp_x1_r: Lr4,
    hp_x1_l: Lr4,
    hp_x1_r: Lr4,
    lp_x2_l: Lr4,
    lp_x2_r: Lr4,
    hp_x2_l: Lr4,
    hp_x2_r: Lr4,
    bypass_mix: f64,
    bypass_target: f64,
    bypass_step: f64,
    bypass_samples_left: u32,
    bypass_ramp_to_wet: u32,
    bypass_ramp_to_dry: u32,
    delay_low: DelayLine,
    delay_mid: DelayLine,
    delay_high: DelayLine,
    low_max_a: usize,
    low_max_b: usize,
    mid_max_a: usize,
    mid_max_b: usize,
    high_max_a: usize,
    high_max_b: usize,
    low_lut_a: Vec<f32>,
    low_lut_b: Vec<f32>,
    mid_lut_a: Vec<f32>,
    mid_lut_b: Vec<f32>,
    high_lut_a: Vec<f32>,
    high_lut_b: Vec<f32>,
    smooth_coeff: f64,
    smooth_low_gain: ParamSmoother,
    smooth_mid_gain: ParamSmoother,
    smooth_high_gain: ParamSmoother,
    smooth_low_delay: ParamSmoother,
    smooth_mid_delay: ParamSmoother,
    smooth_high_delay: ParamSmoother,
    smooth_x1: ParamSmoother,
    smooth_x2: ParamSmoother,
    smooth_strength: ParamSmoother,
    smooth_output: ParamSmoother,
    smooth_boost: ParamSmoother,
    mild: Mild,
    section_out_l: Vec<f32>,
    section_out_r: Vec<f32>,
}

impl Default for Stereo {
    fn default() -> Self {
        let sample_rate = 48_000.0;
        let x1 = 400.0;
        let x2 = 4000.0;
        let max_delay = ((STRENGTH_MAX * sample_rate) / 1000.0).ceil() as usize + 2;
        let mut s = Self {
            sample_rate,
            x1,
            x2,
            lp_x1_l: Lr4::lowpass(x1, sample_rate),
            lp_x1_r: Lr4::lowpass(x1, sample_rate),
            hp_x1_l: Lr4::highpass(x1, sample_rate),
            hp_x1_r: Lr4::highpass(x1, sample_rate),
            lp_x2_l: Lr4::lowpass(x2, sample_rate),
            lp_x2_r: Lr4::lowpass(x2, sample_rate),
            hp_x2_l: Lr4::highpass(x2, sample_rate),
            hp_x2_r: Lr4::highpass(x2, sample_rate),
            bypass_mix: 1.0,
            bypass_target: 1.0,
            bypass_step: 0.0,
            bypass_samples_left: 0,
            bypass_ramp_to_wet: BYPASS_RAMP_TO_WET,
            bypass_ramp_to_dry: BYPASS_RAMP_TO_DRY,
            delay_low: DelayLine::new(max_delay),
            delay_mid: DelayLine::new(max_delay),
            delay_high: DelayLine::new(max_delay),
            low_max_a: 960,
            low_max_b: 960,
            mid_max_a: 960,
            mid_max_b: 960,
            high_max_a: 960,
            high_max_b: 960,
            low_lut_a: Vec::new(),
            low_lut_b: Vec::new(),
            mid_lut_a: Vec::new(),
            mid_lut_b: Vec::new(),
            high_lut_a: Vec::new(),
            high_lut_b: Vec::new(),
            smooth_coeff: 0.0,
            smooth_low_gain: ParamSmoother::new(50.0),
            smooth_mid_gain: ParamSmoother::new(50.0),
            smooth_high_gain: ParamSmoother::new(50.0),
            smooth_low_delay: ParamSmoother::new(50.0),
            smooth_mid_delay: ParamSmoother::new(50.0),
            smooth_high_delay: ParamSmoother::new(50.0),
            smooth_x1: ParamSmoother::new(x1),
            smooth_x2: ParamSmoother::new(x2),
            smooth_strength: ParamSmoother::new(5.0),
            smooth_output: ParamSmoother::new(0.0),
            smooth_boost: ParamSmoother::new(1.0),
            mild: Mild::default(),
            section_out_l: vec![0.0; 1024],
            section_out_r: vec![0.0; 1024],
        };
        s.update_band_maxima_from_sample_rate();
        s.rebuild_band_luts();
        s.update_smoothing_coeff();
        s
    }
}

impl Stereo {
    fn update_smoothing_coeff(&mut self) {
        let tau_seconds = 0.005_f64;
        let sr = self.sample_rate.max(1.0);
        self.smooth_coeff = (-1.0 / (tau_seconds * sr)).exp();
    }

    fn step_smoother(s: &mut ParamSmoother, coeff: f64) -> f64 {
        s.current = s.target + (s.current - s.target) * coeff;
        s.current
    }

    fn flush_denormal(v: f64) -> f64 {
        if !v.is_finite() || v.abs() < 1.0e-30 {
            0.0
        } else {
            v
        }
    }

    fn update_band_maxima_from_sample_rate(&mut self) {
        let approx_len = ((self.sample_rate * STRENGTH_MAX) / 1000.0) as isize;
        let len = approx_len.max(4) as usize;
        self.low_max_a = len;
        self.low_max_b = len;
        self.mid_max_a = len;
        self.mid_max_b = len;
        self.high_max_a = len;
        self.high_max_b = len;
    }

    fn rebuild_band_luts(&mut self) {
        self.low_lut_a = build_band_lut(self.low_max_a, 1.0);
        self.low_lut_b = build_band_lut(self.low_max_b, 0.85);
        self.mid_lut_a = build_band_lut(self.mid_max_a, 1.0);
        self.mid_lut_b = build_band_lut(self.mid_max_b, 0.9);
        self.high_lut_a = build_band_lut(self.high_max_a, 1.0);
        self.high_lut_b = build_band_lut(self.high_max_b, 0.95);
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate.max(1.0);
        self.update_smoothing_coeff();
        self.update_band_maxima_from_sample_rate();
        self.rebuild_band_luts();
        self.rebuild_filters(self.x1, self.x2);
        let max_delay = ((STRENGTH_MAX * self.sample_rate) / 1000.0).ceil() as usize + 2;
        self.delay_low = DelayLine::new(max_delay);
        self.delay_mid = DelayLine::new(max_delay);
        self.delay_high = DelayLine::new(max_delay);
        self.mild.set_sample_rate(sample_rate);
    }

    pub fn reset(&mut self) {
        self.lp_x1_l.reset();
        self.lp_x1_r.reset();
        self.hp_x1_l.reset();
        self.hp_x1_r.reset();
        self.lp_x2_l.reset();
        self.lp_x2_r.reset();
        self.hp_x2_l.reset();
        self.hp_x2_r.reset();
        self.delay_low.reset();
        self.delay_mid.reset();
        self.delay_high.reset();
        self.mild.reset();
        self.section_out_l.fill(0.0);
        self.section_out_r.fill(0.0);
        self.update_band_maxima_from_sample_rate();
        self.rebuild_band_luts();
        self.smooth_low_gain.reset(50.0);
        self.smooth_mid_gain.reset(50.0);
        self.smooth_high_gain.reset(50.0);
        self.smooth_low_delay.reset(50.0);
        self.smooth_mid_delay.reset(50.0);
        self.smooth_high_delay.reset(50.0);
        self.smooth_x1.reset(self.x1);
        self.smooth_x2.reset(self.x2);
        self.smooth_strength.reset(5.0);
        self.smooth_output.reset(0.0);
        self.smooth_boost.reset(1.0);
        self.bypass_mix = 1.0;
        self.bypass_target = 1.0;
        self.bypass_step = 0.0;
        self.bypass_samples_left = 0;
    }

    fn rebuild_filters(&mut self, x1: f64, x2: f64) {
        self.x1 = x1;
        self.x2 = x2;
        self.lp_x1_l = Lr4::lowpass(x1, self.sample_rate);
        self.lp_x1_r = Lr4::lowpass(x1, self.sample_rate);
        self.hp_x1_l = Lr4::highpass(x1, self.sample_rate);
        self.hp_x1_r = Lr4::highpass(x1, self.sample_rate);
        self.lp_x2_l = Lr4::lowpass(x2, self.sample_rate);
        self.lp_x2_r = Lr4::lowpass(x2, self.sample_rate);
        self.hp_x2_l = Lr4::highpass(x2, self.sample_rate);
        self.hp_x2_r = Lr4::highpass(x2, self.sample_rate);
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32], params: &StereoParams) {
        let target = if params.bypass { 0.0 } else { 1.0 };
        let diff = target - self.bypass_mix;
        let abs_eps = 1.175_494_350_822_287_5e-38_f64;
        let rel_eps = 1.192_092_895_507_812_5e-7_f64;
        let cur_mag = self.bypass_mix.abs();
        let min_step = abs_eps.max(cur_mag * rel_eps);

        if diff.abs() > min_step {
            self.bypass_target = target;
            let ramp = if target >= BOOL_THRESHOLD {
                self.bypass_ramp_to_wet
            } else {
                self.bypass_ramp_to_dry
            };
            if ramp <= 1 {
                self.bypass_mix = self.bypass_target;
                self.bypass_step = 0.0;
                self.bypass_samples_left = 0;
            } else {
                self.bypass_samples_left = ramp;
                self.bypass_step = (self.bypass_target - self.bypass_mix) / (ramp as f64);
            }
        }

        let nyquist = (self.sample_rate * 0.5 - 1.0).max(21.0);
        let gain_on = params.gain_on;
        let delay_on = params.delay_on;
        let split_active = gain_on || delay_on;
        self.smooth_low_gain.target = params.low_gain.clamp(0.0, GAIN_DEN);
        self.smooth_mid_gain.target = params.mid_gain.clamp(0.0, GAIN_DEN);
        self.smooth_high_gain.target = params.high_gain.clamp(0.0, GAIN_DEN);
        self.smooth_low_delay.target = params.low_delay.clamp(0.0, 100.0);
        self.smooth_mid_delay.target = params.mid_delay.clamp(0.0, 100.0);
        self.smooth_high_delay.target = params.high_delay.clamp(0.0, 100.0);
        self.smooth_strength.target = params.strength.clamp(STRENGTH_MIN, STRENGTH_MAX);
        self.smooth_output.target = params.output_gain_db;
        self.smooth_boost.target = if gain_on {
            params.boost.clamp(0.0, 4.0)
        } else {
            1.0
        };
        self.smooth_x1.target = params.x1.clamp(20.0, nyquist - 1.0);
        self.smooth_x2.target = params.x2.clamp(self.smooth_x1.target + 1.0, nyquist);
        let x1 = self.smooth_x1.target;
        let x2 = self.smooth_x2.target;
        if split_active && ((x1 - self.x1).abs() > 1e-6 || (x2 - self.x2).abs() > 1e-6) {
            self.rebuild_filters(x1, x2);
            self.update_band_maxima_from_sample_rate();
            self.rebuild_band_luts();
        }
        let any_solo = params.solo_low || params.solo_mid || params.solo_high;
        let frames = left.len().min(right.len());
        if self.section_out_l.len() < frames {
            let new_len = frames.next_power_of_two();
            self.section_out_l.resize(new_len, 0.0);
            self.section_out_r.resize(new_len, 0.0);
        }

        for i in 0..frames {
            let low_gain = Self::step_smoother(&mut self.smooth_low_gain, self.smooth_coeff);
            let mid_gain = Self::step_smoother(&mut self.smooth_mid_gain, self.smooth_coeff);
            let high_gain = Self::step_smoother(&mut self.smooth_high_gain, self.smooth_coeff);
            let low_delay = Self::step_smoother(&mut self.smooth_low_delay, self.smooth_coeff);
            let mid_delay = Self::step_smoother(&mut self.smooth_mid_delay, self.smooth_coeff);
            let high_delay = Self::step_smoother(&mut self.smooth_high_delay, self.smooth_coeff);
            let strength = Self::step_smoother(&mut self.smooth_strength, self.smooth_coeff);
            let boost = Self::step_smoother(&mut self.smooth_boost, self.smooth_coeff);

            let in_l = left[i] as f64;
            let in_r = right[i] as f64;
            let (mut out_l, mut out_r) = (in_l, in_r);

            if split_active {
                let low_w = strength_shaped_width_from_lut(
                    low_gain * 2.0,
                    strength,
                    &self.low_lut_a,
                    &self.low_lut_b,
                );
                let mid_w = strength_shaped_width_from_lut(
                    mid_gain * 2.0,
                    strength,
                    &self.mid_lut_a,
                    &self.mid_lut_b,
                );
                let high_w = strength_shaped_width_from_lut(
                    high_gain * 2.0,
                    strength,
                    &self.high_lut_a,
                    &self.high_lut_b,
                );
                let delay_samples = (strength * self.sample_rate) / 1000.0;

                let low_l = self.lp_x1_l.process(in_l);
                let low_r = self.lp_x1_r.process(in_r);

                let mh_l = self.hp_x1_l.process(in_l);
                let mh_r = self.hp_x1_r.process(in_r);

                let mid_l = self.lp_x2_l.process(mh_l);
                let mid_r = self.lp_x2_r.process(mh_r);

                let high_l = self.hp_x2_l.process(mh_l);
                let high_r = self.hp_x2_r.process(mh_r);

                let low_gain_a = if gain_on { low_gain / 100.0 } else { 0.0 };
                let mid_gain_a = if gain_on { mid_gain / 100.0 } else { 0.0 };
                let high_gain_a = if gain_on { high_gain / 100.0 } else { 0.0 };
                let low_delay_a = if delay_on { low_delay / 100.0 } else { 0.0 };
                let mid_delay_a = if delay_on { mid_delay / 100.0 } else { 0.0 };
                let high_delay_a = if delay_on { high_delay / 100.0 } else { 0.0 };

                let (low_w_l, low_w_r) = ms_width(low_l, low_r, low_w);
                let (mid_w_l, mid_w_r) = ms_width(mid_l, mid_r, mid_w);
                let (high_w_l, high_w_r) = ms_width(high_l, high_r, high_w);

                let (low_delay_l, low_delay_r) = bandwidth_shuffler(
                    low_l,
                    low_r,
                    &mut self.delay_low,
                    delay_samples,
                    low_delay / 100.0,
                );
                let (mid_delay_l, mid_delay_r) = bandwidth_shuffler(
                    mid_l,
                    mid_r,
                    &mut self.delay_mid,
                    delay_samples,
                    mid_delay / 100.0,
                );
                let (high_delay_l, high_delay_r) = bandwidth_shuffler(
                    high_l,
                    high_r,
                    &mut self.delay_high,
                    delay_samples,
                    high_delay / 100.0,
                );

                let mut low_l =
                    low_l + (low_w_l - low_l) * low_gain_a + (low_delay_l - low_l) * low_delay_a;
                let mut low_r =
                    low_r + (low_w_r - low_r) * low_gain_a + (low_delay_r - low_r) * low_delay_a;
                let mut mid_l =
                    mid_l + (mid_w_l - mid_l) * mid_gain_a + (mid_delay_l - mid_l) * mid_delay_a;
                let mut mid_r =
                    mid_r + (mid_w_r - mid_r) * mid_gain_a + (mid_delay_r - mid_r) * mid_delay_a;
                let mut high_l = high_l
                    + (high_w_l - high_l) * high_gain_a
                    + (high_delay_l - high_l) * high_delay_a;
                let mut high_r = high_r
                    + (high_w_r - high_r) * high_gain_a
                    + (high_delay_r - high_r) * high_delay_a;

                if any_solo {
                    if !params.solo_low {
                        low_l = 0.0;
                        low_r = 0.0;
                    }
                    if !params.solo_mid {
                        mid_l = 0.0;
                        mid_r = 0.0;
                    }
                    if !params.solo_high {
                        high_l = 0.0;
                        high_r = 0.0;
                    }
                }

                out_l = low_l + mid_l + high_l;
                out_r = low_r + mid_r + high_r;

                if gain_on {
                    let global_w = (low_w + mid_w + high_w) / 3.0;
                    let global_boost = (1.0 + (global_w - 1.0) * 0.25 * boost).clamp(0.0, 8.0);
                    let gmid = 0.5 * (out_l + out_r);
                    let gside = 0.5 * (out_l - out_r) * global_boost;
                    out_l = gmid + gside;
                    out_r = gmid - gside;
                }
            }

            match params.monitor_mode {
                1 => {
                    let mono = 0.5 * (out_l + out_r);
                    out_l = mono;
                    out_r = mono;
                }
                2 => {
                    let side = 0.5 * (out_l - out_r);
                    out_l = side;
                    out_r = -side;
                }
                _ => {}
            }

            if self.bypass_samples_left > 0 {
                self.bypass_mix += self.bypass_step;
                self.bypass_samples_left -= 1;
                if self.bypass_samples_left == 0 {
                    self.bypass_mix = self.bypass_target;
                }
            }
            let wet = self.bypass_mix;
            out_l = in_l * (1.0 - wet) + out_l * wet;
            out_r = in_r * (1.0 - wet) + out_r * wet;

            self.section_out_l[i] = out_l as f32;
            self.section_out_r[i] = out_r as f32;
        }

        if params.character_on {
            self.mild.process_stereo(
                &mut self.section_out_l[..frames],
                &mut self.section_out_r[..frames],
                params.density.clamp(0.0, 1.0),
                params.focus.clamp(0.0, 1.0),
                params.amount.clamp(0.0, 1.0),
            );
        }

        for i in 0..frames {
            let out_gain_db = Self::step_smoother(&mut self.smooth_output, self.smooth_coeff);
            let out_gain = 10.0_f64.powf(out_gain_db / 20.0);
            let out_l = self.section_out_l[i] as f64 * out_gain;
            let out_r = self.section_out_r[i] as f64 * out_gain;
            left[i] = Self::flush_denormal(out_l) as f32;
            right[i] = Self::flush_denormal(out_r) as f32;
        }
    }
}

fn ms_width(left: f64, right: f64, width: f64) -> (f64, f64) {
    let mid = (left + right) * 0.5;
    let side = (left - right) * 0.5;
    let side_scaled = side * width.clamp(0.0, STRENGTH_SHAPE_SCALE);
    (mid + side_scaled, mid - side_scaled)
}

fn bandwidth_shuffler(
    left: f64,
    right: f64,
    delay: &mut DelayLine,
    delay_samples: f64,
    amount: f64,
) -> (f64, f64) {
    let mid = (left + right) * 0.5;
    delay.write(mid);
    let delayed = delay.read(delay_samples);
    let scaled = delayed * 0.5 * amount.clamp(0.0, 1.0);
    (left - scaled, right + scaled)
}

#[derive(Debug, Clone, Copy)]
struct BandLookupState {
    idx_a: usize,
    frac_a: f64,
    idx_b: usize,
    frac_b: f64,
}

fn build_band_lut(len: usize, flavor: f64) -> Vec<f32> {
    let n = len.max(4);
    let mut lut = vec![0.0_f32; n];
    for (i, v) in lut.iter_mut().enumerate() {
        let x = (i as f64) / ((n - 1) as f64);
        let y = (1.0 + x * STRENGTH_SHAPE_SCALE).ln() / (1.0 + STRENGTH_SHAPE_SCALE).ln();
        let shaped = y.powf(flavor);
        *v = shaped as f32;
    }
    lut
}

fn strength_shaped_width_from_lut(width: f64, strength: f64, lut_a: &[f32], lut_b: &[f32]) -> f64 {
    let max_a = lut_a.len().clamp(2, usize::MAX);
    let max_b = lut_b.len().clamp(2, usize::MAX);
    let width_base = 1.0 + ((width.clamp(0.0, GAIN_DEN) * GAIN_NUM) / GAIN_DEN) / GAIN_NORM;
    let w = (width_base / STRENGTH_SHAPE_SCALE).clamp(0.0, 1.0);
    let s = ((strength.clamp(STRENGTH_MIN, STRENGTH_MAX) - STRENGTH_MIN)
        / (STRENGTH_MAX - STRENGTH_MIN))
        .clamp(0.0, 1.0);

    let state = compute_band_lookup_state(w, max_a, max_b);
    let a0 = lut_a[state.idx_a] as f64;
    let a1 = lut_a[(state.idx_a + 1).min(lut_a.len() - 1)] as f64;
    let b0 = lut_b[state.idx_b] as f64;
    let b1 = lut_b[(state.idx_b + 1).min(lut_b.len() - 1)] as f64;
    let table_a = a0 + (a1 - a0) * state.frac_a;
    let table_b = b0 + (b1 - b0) * state.frac_b;
    let table_norm = 0.5 * (table_a + table_b);
    let shape = (0.5 + table_norm).clamp(0.0, 2.0);
    let gain = 1.0 + (width_base - 1.0) * shape * (1.0 + 4.0 * s);
    gain.clamp(0.0, 8.0)
}

fn compute_band_lookup_state(width_norm: f64, max_a: usize, max_b: usize) -> BandLookupState {
    let scaled = width_norm.clamp(0.0, 1.0);

    let pos_a = scaled * ((max_a - 2) as f64);
    let mut idx_a = (pos_a as u32) as usize;
    let mut frac_a = pos_a - idx_a as f64;
    let lim_a = max_a.saturating_sub(2);
    if idx_a > lim_a {
        idx_a = lim_a;
        frac_a = 0.0;
    }

    let pos_b = scaled * ((max_b - 2) as f64);
    let mut idx_b = (pos_b as u32) as usize;
    let mut frac_b = pos_b - idx_b as f64;
    let lim_b = max_b.saturating_sub(2);
    if idx_b > lim_b {
        idx_b = lim_b;
        frac_b = 0.0;
    }

    BandLookupState {
        idx_a,
        frac_a,
        idx_b,
        frac_b,
    }
}
const GAIN_NUM: f64 = 150.0;
const GAIN_DEN: f64 = 200.0;
const GAIN_NORM: f64 = 100.0;
const STRENGTH_MIN: f64 = 1.0;
const STRENGTH_MAX: f64 = 20.0;
const STRENGTH_SHAPE_SCALE: f64 = 2.5;
const BOOL_THRESHOLD: f64 = 0.5;
const BYPASS_RAMP_TO_WET: u32 = 1272;
const BYPASS_RAMP_TO_DRY: u32 = 756;
