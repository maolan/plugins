#[derive(Debug, Clone, Copy)]
pub struct WidenerParams {
    pub output_gain_db: f64,
    pub boost: f64,
    pub low: f64,
    pub mid: f64,
    pub high: f64,
    pub solo_low: bool,
    pub solo_mid: bool,
    pub solo_high: bool,
    pub x1: f64,
    pub x2: f64,
    pub strength: f64,
    pub monitor_mode: u8,
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
pub struct Widener {
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
    smooth_low: ParamSmoother,
    smooth_mid: ParamSmoother,
    smooth_high: ParamSmoother,
    smooth_x1: ParamSmoother,
    smooth_x2: ParamSmoother,
    smooth_strength: ParamSmoother,
    smooth_output: ParamSmoother,
    smooth_boost: ParamSmoother,
}

impl Default for Widener {
    fn default() -> Self {
        let sample_rate = 48_000.0;
        let x1 = 400.0;
        let x2 = 4000.0;
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
            smooth_low: ParamSmoother::new(100.0),
            smooth_mid: ParamSmoother::new(100.0),
            smooth_high: ParamSmoother::new(100.0),
            smooth_x1: ParamSmoother::new(x1),
            smooth_x2: ParamSmoother::new(x2),
            smooth_strength: ParamSmoother::new(5.0),
            smooth_output: ParamSmoother::new(0.0),
            smooth_boost: ParamSmoother::new(1.0),
        };
        s.update_band_maxima_from_sample_rate();
        s.rebuild_band_luts();
        s.update_smoothing_coeff();
        s
    }
}

impl Widener {
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
        self.update_band_maxima_from_sample_rate();
        self.rebuild_band_luts();
        self.smooth_low.reset(100.0);
        self.smooth_mid.reset(100.0);
        self.smooth_high.reset(100.0);
        self.smooth_x1.reset(self.x1);
        self.smooth_x2.reset(self.x2);
        self.smooth_strength.reset(5.0);
        self.smooth_output.reset(0.0);
        self.smooth_boost.reset(1.0);
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

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32], params: &WidenerParams) {
        let nyquist = (self.sample_rate * 0.5 - 1.0).max(21.0);
        self.smooth_low.target = params.low.clamp(0.0, GAIN_DEN);
        self.smooth_mid.target = params.mid.clamp(0.0, GAIN_DEN);
        self.smooth_high.target = params.high.clamp(0.0, GAIN_DEN);
        self.smooth_strength.target = params.strength.clamp(STRENGTH_MIN, STRENGTH_MAX);
        self.smooth_output.target = params.output_gain_db;
        self.smooth_boost.target = params.boost.clamp(0.0, 4.0);
        self.smooth_x1.target = params.x1.clamp(20.0, nyquist - 1.0);
        self.smooth_x2.target = params.x2.clamp(self.smooth_x1.target + 1.0, nyquist);
        let x1 = self.smooth_x1.target;
        let x2 = self.smooth_x2.target;
        if (x1 - self.x1).abs() > 1e-6 || (x2 - self.x2).abs() > 1e-6 {
            self.rebuild_filters(x1, x2);
            self.update_band_maxima_from_sample_rate();
            self.rebuild_band_luts();
        }
        let any_solo = params.solo_low || params.solo_mid || params.solo_high;

        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let low = Self::step_smoother(&mut self.smooth_low, self.smooth_coeff);
            let mid = Self::step_smoother(&mut self.smooth_mid, self.smooth_coeff);
            let high = Self::step_smoother(&mut self.smooth_high, self.smooth_coeff);
            let strength = Self::step_smoother(&mut self.smooth_strength, self.smooth_coeff);
            let out_gain_db = Self::step_smoother(&mut self.smooth_output, self.smooth_coeff);
            let boost = Self::step_smoother(&mut self.smooth_boost, self.smooth_coeff);

            let low_w =
                strength_shaped_width_from_lut(low, strength, &self.low_lut_a, &self.low_lut_b);
            let mid_w =
                strength_shaped_width_from_lut(mid, strength, &self.mid_lut_a, &self.mid_lut_b);
            let high_w =
                strength_shaped_width_from_lut(high, strength, &self.high_lut_a, &self.high_lut_b);
            let out_gain = 10.0_f64.powf(out_gain_db / 20.0);

            let in_l = *l as f64;
            let in_r = *r as f64;

            let low_l = self.lp_x1_l.process(in_l);
            let low_r = self.lp_x1_r.process(in_r);

            let mh_l = self.hp_x1_l.process(in_l);
            let mh_r = self.hp_x1_r.process(in_r);

            let mid_l = self.lp_x2_l.process(mh_l);
            let mid_r = self.lp_x2_r.process(mh_r);

            let high_l = self.hp_x2_l.process(mh_l);
            let high_r = self.hp_x2_r.process(mh_r);

            let (mut low_l, mut low_r) = ms_width(low_l, low_r, low_w);
            let (mut mid_l, mut mid_r) = ms_width(mid_l, mid_r, mid_w);
            let (mut high_l, mut high_r) = ms_width(high_l, high_r, high_w);

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

            let wet_l = (low_l + mid_l + high_l) * out_gain;
            let wet_r = (low_r + mid_r + high_r) * out_gain;
            let mut out_l = wet_l;
            let mut out_r = wet_r;
            let global_w = (low_w + mid_w + high_w) / 3.0;
            let global_boost = (1.0 + (global_w - 1.0) * 3.0 * boost).clamp(0.0, 8.0);
            let gmid = 0.5 * (out_l + out_r);
            let gside = 0.5 * (out_l - out_r) * global_boost;
            out_l = gmid + gside;
            out_r = gmid - gside;

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

            *l = Self::flush_denormal(out_l) as f32;
            *r = Self::flush_denormal(out_r) as f32;
        }
    }
}

fn ms_width(left: f64, right: f64, width: f64) -> (f64, f64) {
    let mid = (left + right) * 0.5;
    let side = (left - right) * 0.5;
    let side_scaled = side * width.clamp(0.0, STRENGTH_SHAPE_SCALE);
    (mid + side_scaled, mid - side_scaled)
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
