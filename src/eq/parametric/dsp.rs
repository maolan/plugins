use crate::eq::common::dsp::{Biquad, db_to_gain};

#[derive(Debug, Clone)]
pub struct ParametricEqualizer {
    sample_rate: f32,
    input_gain_lin: f32,
    output_gain_lin: f32,
    bypass: bool,

    para_l: Vec<Biquad>,
    para_r: Vec<Biquad>,

    para_freq: Vec<f32>,
    para_gain: Vec<f32>,
    para_q: Vec<f32>,
    para_on: Vec<bool>,
    active_bands: Vec<usize>,
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
            active_bands: Vec::new(),
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
        for f in &mut self.para_l {
            f.reset();
        }
        for f in &mut self.para_r {
            f.reset();
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

    pub fn set_para_band(&mut self, idx: usize, freq: f32, gain: f32, q: f32, on: bool) {
        if !on && idx >= self.para_on.len() {
            return;
        }

        if idx >= self.para_on.len() {
            let new_len = idx + 1;
            self.para_l.resize(new_len, Biquad::default());
            self.para_r.resize(new_len, Biquad::default());
            self.para_freq.resize(new_len, 1000.0);
            self.para_gain.resize(new_len, 0.0);
            self.para_q.resize(new_len, 1.0);
            self.para_on.resize(new_len, false);
        }

        self.para_freq[idx] = freq;
        self.para_gain[idx] = gain;
        self.para_q[idx] = q;
        self.para_on[idx] = on;
        self.update_para_band(idx);
        self.rebuild_active_bands();
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
        self.para_l[idx].set_peaking(
            self.sample_rate,
            self.para_freq[idx],
            self.para_q[idx],
            self.para_gain[idx],
        );
        self.para_r[idx].set_peaking(
            self.sample_rate,
            self.para_freq[idx],
            self.para_q[idx],
            self.para_gain[idx],
        );
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.bypass {
            return;
        }
        let frames = left.len().min(right.len());
        crate::simd::mul_inplace(&mut left[..frames], self.input_gain_lin);
        crate::simd::mul_inplace(&mut right[..frames], self.input_gain_lin);
        for &b in self.active_bands.iter() {
            self.para_l[b].process_inplace(&mut left[..frames]);
            self.para_r[b].process_inplace(&mut right[..frames]);
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
            self.para_l[b].process_inplace(buffer);
        }
        crate::simd::mul_inplace(buffer, self.output_gain_lin);
    }
}
