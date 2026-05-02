use crate::eq::common::dsp::{Biquad, db_to_gain};

#[derive(Debug, Clone)]
pub struct ParametricEqualizer {
    sample_rate: f32,
    input_gain_lin: f32,
    output_gain_lin: f32,
    bypass: bool,

    para_l: [Biquad; 32],
    para_r: [Biquad; 32],

    para_freq: [f32; 32],
    para_gain: [f32; 32],
    para_q: [f32; 32],
}

impl Default for ParametricEqualizer {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            input_gain_lin: 1.0,
            output_gain_lin: 1.0,
            bypass: false,
            para_l: [Biquad::default(); 32],
            para_r: [Biquad::default(); 32],
            para_freq: [1000.0; 32],
            para_gain: [0.0; 32],
            para_q: [1.0; 32],
        }
    }
}

impl ParametricEqualizer {
    pub fn new(sample_rate: f32) -> Self {
        let mut eq = Self {
            sample_rate,
            ..Self::default()
        };
        eq.rebuild_filters();
        eq
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

    pub fn set_para_band(&mut self, idx: usize, freq: f32, gain: f32, q: f32) {
        if idx >= 32 {
            return;
        }
        self.para_freq[idx] = freq;
        self.para_gain[idx] = gain;
        self.para_q[idx] = q;
        self.update_para_band(idx);
    }

    fn rebuild_filters(&mut self) {
        for i in 0..32 {
            self.update_para_band(i);
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
        for i in 0..frames {
            let mut l = left[i] * self.input_gain_lin;
            let mut r = right[i] * self.input_gain_lin;

            for b in 0..32 {
                l = self.para_l[b].process(l);
                r = self.para_r[b].process(r);
            }

            left[i] = l * self.output_gain_lin;
            right[i] = r * self.output_gain_lin;
        }
    }

    pub fn process_mono(&mut self, buffer: &mut [f32]) {
        if self.bypass {
            return;
        }
        for s in buffer.iter_mut() {
            let mut l = *s * self.input_gain_lin;
            for b in 0..32 {
                l = self.para_l[b].process(l);
            }
            *s = l * self.output_gain_lin;
        }
    }
}
