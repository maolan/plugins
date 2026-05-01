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
    fn set_peaking(&mut self, sample_rate: f32, freq_hz: f32, q: f32, gain_db: f32) {
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

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    fn set_low_shelf(&mut self, sample_rate: f32, freq_hz: f32, gain_db: f32) {
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

    fn set_high_shelf(&mut self, sample_rate: f32, freq_hz: f32, gain_db: f32) {
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq_hz.clamp(10.0, sample_rate * 0.45) / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 * 0.5 * (2.0_f32).sqrt();
        let beta = 2.0 * a.sqrt() * alpha;

        let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + beta);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - beta);
        let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + beta;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - beta;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

#[derive(Debug, Clone)]
pub struct Equalizer {
    sample_rate: f32,
    input_gain_lin: f32,
    output_gain_lin: f32,
    parametric_bypass: bool,
    graphic_bypass: bool,

    para_l: [Biquad; 32],
    para_r: [Biquad; 32],
    graphic_l: [Biquad; 32],
    graphic_r: [Biquad; 32],

    para_freq: [f32; 32],
    para_gain: [f32; 32],
    para_q: [f32; 32],
    graphic_gain: [f32; 32],
}

impl Default for Equalizer {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            input_gain_lin: 1.0,
            output_gain_lin: 1.0,
            parametric_bypass: false,
            graphic_bypass: false,
            para_l: [Biquad::default(); 32],
            para_r: [Biquad::default(); 32],
            graphic_l: [Biquad::default(); 32],
            graphic_r: [Biquad::default(); 32],
            para_freq: [1000.0; 32],
            para_gain: [0.0; 32],
            para_q: [1.0; 32],
            graphic_gain: [0.0; 32],
        }
    }
}

impl Equalizer {
    pub fn new(sample_rate: f32) -> Self {
        let mut eq = Self {
            sample_rate,
            ..Self::default()
        };
        let centers = graphic_centers();
        for (i, f) in centers.iter().copied().enumerate() {
            eq.para_freq[i] = f;
        }
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
    pub fn set_parametric_bypass(&mut self, bypass: bool) {
        self.parametric_bypass = bypass;
    }
    pub fn set_graphic_bypass(&mut self, bypass: bool) {
        self.graphic_bypass = bypass;
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

    pub fn set_graphic_gain(&mut self, idx: usize, gain: f32) {
        if idx >= 32 {
            return;
        }
        self.graphic_gain[idx] = gain;
        self.update_graphic_band(idx);
    }

    fn rebuild_filters(&mut self) {
        for i in 0..32 {
            self.update_para_band(i);
            self.update_graphic_band(i);
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
            self.graphic_l[idx].set_peaking(self.sample_rate, centers[idx], q, self.graphic_gain[idx]);
            self.graphic_r[idx].set_peaking(self.sample_rate, centers[idx], q, self.graphic_gain[idx]);
        }
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32]) {
        let frames = left.len().min(right.len());
        for i in 0..frames {
            let mut l = left[i] * self.input_gain_lin;
            let mut r = right[i] * self.input_gain_lin;

            if !self.parametric_bypass {
                for b in 0..32 {
                    l = self.para_l[b].process(l);
                    r = self.para_r[b].process(r);
                }
            }

            if !self.graphic_bypass {
                for b in 0..32 {
                    l = self.graphic_l[b].process(l);
                    r = self.graphic_r[b].process(r);
                }
            }

            left[i] = l * self.output_gain_lin;
            right[i] = r * self.output_gain_lin;
        }
    }

    pub fn process_mono(&mut self, buffer: &mut [f32]) {
        for s in buffer.iter_mut() {
            let mut l = *s;
            let mut r = *s;
            self.process_stereo(std::slice::from_mut(&mut l), std::slice::from_mut(&mut r));
            *s = 0.5 * (l + r);
        }
    }
}

fn graphic_centers() -> [f32; 32] {
    [
        16.0, 20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0,
        315.0, 400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0,
        4000.0, 5000.0, 6300.0, 8000.0, 10000.0, 12500.0, 16000.0, 20000.0,
    ]
}

fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db * 0.05)
}
