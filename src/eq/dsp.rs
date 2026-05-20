#[derive(Debug, Clone)]
pub struct ParametricEqualizer {
    sample_rate: f32,
    input_gain_lin: f32,
    output_gain_lin: f32,
    bypass: bool,

    para_l: Vec<Vec<Biquad>>,
    para_r: Vec<Vec<Biquad>>,

    para_freq: Vec<f32>,
    para_gain: Vec<f32>,
    para_q: Vec<f32>,
    para_on: Vec<bool>,
    para_type: Vec<u8>,
    para_slope: Vec<u8>,
    active_bands: Vec<usize>,
    pub listen_band: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
pub struct BandParams {
    pub freq: f32,
    pub gain: f32,
    pub q: f32,
    pub on: bool,
    pub typ: u8,
    pub slope: u8,
}

impl Default for ParametricEqualizer {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            input_gain_lin: 1.0,
            output_gain_lin: 1.0,
            bypass: false,
            para_l: Vec::new(),
            para_r: Vec::new(),
            para_freq: Vec::new(),
            para_gain: Vec::new(),
            para_q: Vec::new(),
            para_on: Vec::new(),
            para_type: Vec::new(),
            para_slope: Vec::new(),
            active_bands: Vec::new(),
            listen_band: None,
        }
    }
}

impl ParametricEqualizer {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            ..Self::default()
        }
    }

    pub fn reset(&mut self) {
        for chain in &mut self.para_l {
            for f in chain {
                f.reset();
            }
        }
        for chain in &mut self.para_r {
            for f in chain {
                f.reset();
            }
        }
    }

    pub fn set_input_gain_db(&mut self, db: f32) {
        self.input_gain_lin = db_to_gain(db);
    }
    pub fn set_output_gain_db(&mut self, db: f32) {
        self.output_gain_lin = db_to_gain(db);
    }
    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn set_para_band(&mut self, idx: usize, params: BandParams) {
        if !params.on && idx >= self.para_on.len() {
            return;
        }

        if idx >= self.para_on.len() {
            let new_len = idx + 1;
            self.para_l.resize(new_len, Vec::new());
            self.para_r.resize(new_len, Vec::new());
            self.para_freq.resize(new_len, 1000.0);
            self.para_gain.resize(new_len, 0.0);
            self.para_q.resize(new_len, 1.0);
            self.para_on.resize(new_len, false);
            self.para_type.resize(new_len, 1);
            self.para_slope.resize(new_len, 0);
        }

        self.para_freq[idx] = params.freq;
        self.para_gain[idx] = params.gain;
        self.para_q[idx] = params.q;
        self.para_on[idx] = params.on;
        self.para_type[idx] = params.typ;
        self.para_slope[idx] = params.slope;
        self.update_para_band(idx);
        self.rebuild_active_bands();
    }

    pub fn set_listen_band(&mut self, band: Option<usize>) {
        self.listen_band = band;
    }

    fn rebuild_active_bands(&mut self) {
        self.active_bands.clear();
        for i in 0..self.para_on.len() {
            if self.para_on[i] {
                self.active_bands.push(i);
            }
        }
    }

    fn update_para_band(&mut self, idx: usize) {
        let n = match self.para_slope.get(idx).copied().unwrap_or(0) {
            1 => 2,
            2 => 4,
            3 => 8,
            _ => 1,
        };
        self.para_l[idx].resize(n, Biquad::default());
        self.para_r[idx].resize(n, Biquad::default());

        for bq in &mut self.para_l[idx] {
            match self.para_type[idx] {
                0 => bq.set_lowpass(self.sample_rate, self.para_freq[idx], self.para_q[idx]),
                2 => bq.set_highpass(self.sample_rate, self.para_freq[idx], self.para_q[idx]),
                _ => bq.set_peaking(
                    self.sample_rate,
                    self.para_freq[idx],
                    self.para_q[idx],
                    self.para_gain[idx],
                ),
            }
        }
        for bq in &mut self.para_r[idx] {
            match self.para_type[idx] {
                0 => bq.set_lowpass(self.sample_rate, self.para_freq[idx], self.para_q[idx]),
                2 => bq.set_highpass(self.sample_rate, self.para_freq[idx], self.para_q[idx]),
                _ => bq.set_peaking(
                    self.sample_rate,
                    self.para_freq[idx],
                    self.para_q[idx],
                    self.para_gain[idx],
                ),
            }
        }
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.bypass {
            return;
        }
        let frames = left.len().min(right.len());
        crate::simd::mul_inplace(&mut left[..frames], self.input_gain_lin);
        crate::simd::mul_inplace(&mut right[..frames], self.input_gain_lin);
        for &b in self.active_bands.iter() {
            for bq in &mut self.para_l[b] {
                bq.process_inplace(&mut left[..frames]);
            }
            for bq in &mut self.para_r[b] {
                bq.process_inplace(&mut right[..frames]);
            }
        }
        crate::simd::mul_inplace(&mut left[..frames], self.output_gain_lin);
        crate::simd::mul_inplace(&mut right[..frames], self.output_gain_lin);
    }

    pub fn process_mono(&mut self, buffer: &mut [f32]) {
        if self.bypass {
            return;
        }
        crate::simd::mul_inplace(buffer, self.input_gain_lin);
        for &b in self.active_bands.iter() {
            for bq in &mut self.para_l[b] {
                bq.process_inplace(buffer);
            }
        }
        crate::simd::mul_inplace(buffer, self.output_gain_lin);
    }

    pub fn update_para_band_gain(&mut self, idx: usize, gain_db: f32) {
        if idx >= self.para_on.len() || !self.para_on[idx] {
            return;
        }
        self.para_gain[idx] = gain_db;
        self.update_para_band(idx);
    }

    pub fn process_stereo_without_band(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        skip: usize,
    ) {
        if self.bypass {
            return;
        }
        let frames = left.len().min(right.len());
        crate::simd::mul_inplace(&mut left[..frames], self.input_gain_lin);
        crate::simd::mul_inplace(&mut right[..frames], self.input_gain_lin);
        for &b in self.active_bands.iter() {
            if b == skip {
                continue;
            }
            for bq in &mut self.para_l[b] {
                bq.process_inplace(&mut left[..frames]);
            }
            for bq in &mut self.para_r[b] {
                bq.process_inplace(&mut right[..frames]);
            }
        }
        crate::simd::mul_inplace(&mut left[..frames], self.output_gain_lin);
        crate::simd::mul_inplace(&mut right[..frames], self.output_gain_lin);
    }

    pub fn process_mono_without_band(&mut self, buffer: &mut [f32], skip: usize) {
        if self.bypass {
            return;
        }
        crate::simd::mul_inplace(buffer, self.input_gain_lin);
        for &b in self.active_bands.iter() {
            if b == skip {
                continue;
            }
            for bq in &mut self.para_l[b] {
                bq.process_inplace(buffer);
            }
        }
        crate::simd::mul_inplace(buffer, self.output_gain_lin);
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct Biquad {
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
    pub fn set_peaking(&mut self, sample_rate: f32, freq_hz: f32, q: f32, gain_db: f32) {
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq_hz.clamp(20.0, sample_rate * 0.45) / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q.clamp(0.1, 24.0));

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline(always)]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    pub fn process_inplace(&mut self, buffer: &mut [f32]) {
        let b0 = self.b0;
        let b1 = self.b1;
        let b2 = self.b2;
        let a1 = self.a1;
        let a2 = self.a2;
        let mut x1 = self.x1;
        let mut x2 = self.x2;
        let mut y1 = self.y1;
        let mut y2 = self.y2;

        for x in buffer.iter_mut() {
            let input = *x;
            let y = b0 * input + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
            x2 = x1;
            x1 = input;
            y2 = y1;
            y1 = y;
            *x = y;
        }

        self.x1 = x1;
        self.x2 = x2;
        self.y1 = y1;
        self.y2 = y2;
    }

    pub fn set_low_shelf(&mut self, sample_rate: f32, freq_hz: f32, gain_db: f32) {
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq_hz.clamp(10.0, sample_rate * 0.45) / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 * 0.5 * (2.0_f32).sqrt();
        let beta = 2.0 * a.sqrt() * alpha;

        let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w0 + beta);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - beta);
        let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + beta;
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) + (a - 1.0) * cos_w0 - beta;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    pub fn set_high_shelf(&mut self, sample_rate: f32, freq_hz: f32, gain_db: f32) {
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq_hz.clamp(10.0, sample_rate * 0.45) / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 * 0.5 * (2.0_f32).sqrt();
        let beta = 2.0 * a.sqrt() * alpha;

        let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + beta);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - beta);
        let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + beta;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - beta;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    pub fn set_lowpass(&mut self, sample_rate: f32, freq_hz: f32, q: f32) {
        let w0 = 2.0 * std::f32::consts::PI * freq_hz.clamp(20.0, sample_rate * 0.45) / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q.clamp(0.1, 24.0));

        let b0 = (1.0 - cos_w0) / 2.0;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    pub fn set_highpass(&mut self, sample_rate: f32, freq_hz: f32, q: f32) {
        let w0 = 2.0 * std::f32::consts::PI * freq_hz.clamp(20.0, sample_rate * 0.45) / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q.clamp(0.1, 24.0));

        let b0 = (1.0 + cos_w0) / 2.0;
        let b1 = -(1.0 + cos_w0);
        let b2 = (1.0 + cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    pub fn magnitude_db(&self, freq_hz: f32, sample_rate: f32) -> f32 {
        let w = 2.0 * std::f32::consts::PI * freq_hz.clamp(1.0, sample_rate * 0.499) / sample_rate;
        let c = w.cos();
        let c2 = (2.0 * w).cos();
        let s = w.sin();
        let s2 = (2.0 * w).sin();

        // Evaluate numerator |B(e^jw)|^2  =  (b0 + b1*c + b2*c2)^2 + (b1*s + b2*s2)^2
        let num_re = self.b0 + self.b1 * c + self.b2 * c2;
        let num_im = self.b1 * s + self.b2 * s2;
        let num = num_re * num_re + num_im * num_im;

        // Evaluate denominator |A(e^jw)|^2  =  (1 + a1*c + a2*c2)^2 + (a1*s + a2*s2)^2
        let den_re = 1.0 + self.a1 * c + self.a2 * c2;
        let den_im = self.a1 * s + self.a2 * s2;
        let den = den_re * den_re + den_im * den_im;

        let mag_sq = num / den.max(1.0e-24);
        10.0 * mag_sq.max(1.0e-24).log10()
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db * 0.05)
}
