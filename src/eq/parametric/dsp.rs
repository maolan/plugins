use crate::eq::common::dsp::{Biquad, db_to_gain};

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
