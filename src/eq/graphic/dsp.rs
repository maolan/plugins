use crate::eq::common::dsp::{Biquad, db_to_gain, graphic_centers};

#[derive(Debug, Clone)]
pub struct GraphicEqualizer {
    sample_rate: f32,
    input_gain_lin: f32,
    output_gain_lin: f32,
    bypass: bool,

    graphic_l: [Biquad; 32],
    graphic_r: [Biquad; 32],
    graphic_gain: [f32; 32],
}

impl Default for GraphicEqualizer {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            input_gain_lin: 1.0,
            output_gain_lin: 1.0,
            bypass: false,
            graphic_l: [Biquad::default(); 32],
            graphic_r: [Biquad::default(); 32],
            graphic_gain: [0.0; 32],
        }
    }
}

impl GraphicEqualizer {
    pub fn new(sample_rate: f32) -> Self {
        let mut eq = Self {
            sample_rate,
            ..Self::default()
        };
        eq.rebuild_filters();
        eq
    }

    pub fn reset(&mut self) {
        for f in &mut self.graphic_l {
            f.reset();
        }
        for f in &mut self.graphic_r {
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

    pub fn set_graphic_gain(&mut self, idx: usize, gain: f32) {
        if idx >= 32 {
            return;
        }
        self.graphic_gain[idx] = gain;
        self.update_graphic_band(idx);
    }

    fn rebuild_filters(&mut self) {
        for i in 0..32 {
            self.update_graphic_band(i);
        }
    }

    fn update_graphic_band(&mut self, idx: usize) {
        let centers = graphic_centers();
        if idx == 0 {
            let edge = (centers[0] * centers[1]).sqrt();
            self.graphic_l[idx].set_low_shelf(self.sample_rate, edge, self.graphic_gain[idx]);
            self.graphic_r[idx].set_low_shelf(self.sample_rate, edge, self.graphic_gain[idx]);
        } else if idx == 31 {
            let edge = (centers[30] * centers[31]).sqrt();
            self.graphic_l[idx].set_high_shelf(self.sample_rate, edge, self.graphic_gain[idx]);
            self.graphic_r[idx].set_high_shelf(self.sample_rate, edge, self.graphic_gain[idx]);
        } else {
            let q = 1.2;
            self.graphic_l[idx].set_peaking(
                self.sample_rate,
                centers[idx],
                q,
                self.graphic_gain[idx],
            );
            self.graphic_r[idx].set_peaking(
                self.sample_rate,
                centers[idx],
                q,
                self.graphic_gain[idx],
            );
        }
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
                l = self.graphic_l[b].process(l);
                r = self.graphic_r[b].process(r);
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
                l = self.graphic_l[b].process(l);
            }
            *s = l * self.output_gain_lin;
        }
    }
}
