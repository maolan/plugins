#[derive(Debug, Clone, Copy)]
pub struct DeEsserParams {
    pub intensity: f64,
    pub sharpness: f64,
    pub depth: f64,
    pub filter: f64,
    pub monitor: bool,
}

#[derive(Debug, Clone)]
pub struct DeEsser {
    sample_rate: f64,
    s_l: [f64; 41],
    m_l: [f64; 41],
    s_r: [f64; 41],
    m_r: [f64; 41],
    ratio_a_l: f64,
    ratio_b_l: f64,
    iir_sample_a_l: f64,
    iir_sample_b_l: f64,
    ratio_a_r: f64,
    ratio_b_r: f64,
    iir_sample_a_r: f64,
    iir_sample_b_r: f64,
    flip: bool,
    fpd_l: u32,
    fpd_r: u32,
}

impl Default for DeEsser {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            s_l: [0.0; 41],
            m_l: [0.0; 41],
            s_r: [0.0; 41],
            m_r: [0.0; 41],
            ratio_a_l: 1.0,
            ratio_b_l: 1.0,
            iir_sample_a_l: 0.0,
            iir_sample_b_l: 0.0,
            ratio_a_r: 1.0,
            ratio_b_r: 1.0,
            iir_sample_a_r: 0.0,
            iir_sample_b_r: 0.0,
            flip: true,
            fpd_l: rand::random(),
            fpd_r: rand::random(),
        }
    }
}

impl DeEsser {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            ..Default::default()
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.sample_rate);
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32], params: &DeEsserParams) {
        let overallscale = self.sample_rate / 44_100.0;
        let intensity = params.intensity.powi(5) * (8192.0 / overallscale);
        let mut sharpness = (params.sharpness * 40.0).round() as usize;
        if sharpness < 2 {
            sharpness = 2;
        }
        let speed = 0.1 / sharpness as f64;
        let depth = 1.0 / (params.depth + 0.0001);
        let iir_amount = params.filter;

        for (sample_l, sample_r) in left.iter_mut().zip(right.iter_mut()) {
            let mut input_l = *sample_l as f64;
            let mut input_r = *sample_r as f64;

            if input_l.abs() < 1.18e-23 {
                input_l = self.fpd_l as f64 * 1.18e-17;
            }
            if input_r.abs() < 1.18e-23 {
                input_r = self.fpd_r as f64 * 1.18e-17;
            }

            let dry_l = input_l;
            let dry_r = input_r;

            // Shift sample history
            self.s_l[0] = input_l;
            self.s_r[0] = input_r;
            for x in (1..=sharpness).rev() {
                self.s_l[x] = self.s_l[x - 1];
                self.s_r[x] = self.s_r[x - 1];
            }

            // Build slew-of-slew products
            self.m_l[1] = (self.s_l[1] - self.s_l[2]) * ((self.s_l[1] - self.s_l[2]) / 1.3);
            self.m_r[1] = (self.s_r[1] - self.s_r[2]) * ((self.s_r[1] - self.s_r[2]) / 1.3);
            for x in (2..sharpness).rev() {
                self.m_l[x] =
                    (self.s_l[x] - self.s_l[x + 1]) * ((self.s_l[x - 1] - self.s_l[x]) / 1.3);
                self.m_r[x] =
                    (self.s_r[x] - self.s_r[x + 1]) * ((self.s_r[x - 1] - self.s_r[x]) / 1.3);
            }

            // Compute sense from chained differences
            let mut sense_l =
                (self.m_l[1] - self.m_l[2]).abs() * sharpness as f64 * sharpness as f64;
            let mut sense_r =
                (self.m_r[1] - self.m_r[2]).abs() * sharpness as f64 * sharpness as f64;
            for x in (1..sharpness).rev() {
                let mult_l =
                    (self.m_l[x] - self.m_l[x + 1]).abs() * sharpness as f64 * sharpness as f64;
                if mult_l < 1.0 {
                    sense_l *= mult_l;
                }
                let mult_r =
                    (self.m_r[x] - self.m_r[x + 1]).abs() * sharpness as f64 * sharpness as f64;
                if mult_r < 1.0 {
                    sense_r *= mult_r;
                }
            }

            sense_l = 1.0 + intensity * intensity * sense_l;
            if sense_l > intensity {
                sense_l = intensity;
            }
            sense_r = 1.0 + intensity * intensity * sense_r;
            if sense_r > intensity {
                sense_r = intensity;
            }

            if self.flip {
                self.iir_sample_a_l =
                    self.iir_sample_a_l * (1.0 - iir_amount) + input_l * iir_amount;
                self.iir_sample_a_r =
                    self.iir_sample_a_r * (1.0 - iir_amount) + input_r * iir_amount;
                self.ratio_a_l = self.ratio_a_l * (1.0 - speed) + sense_l * speed;
                self.ratio_a_r = self.ratio_a_r * (1.0 - speed) + sense_r * speed;
                if self.ratio_a_l > depth {
                    self.ratio_a_l = depth;
                }
                if self.ratio_a_r > depth {
                    self.ratio_a_r = depth;
                }
                if self.ratio_a_l > 1.0 {
                    input_l =
                        self.iir_sample_a_l + (input_l - self.iir_sample_a_l) / self.ratio_a_l;
                }
                if self.ratio_a_r > 1.0 {
                    input_r =
                        self.iir_sample_a_r + (input_r - self.iir_sample_a_r) / self.ratio_a_r;
                }
            } else {
                self.iir_sample_b_l =
                    self.iir_sample_b_l * (1.0 - iir_amount) + input_l * iir_amount;
                self.iir_sample_b_r =
                    self.iir_sample_b_r * (1.0 - iir_amount) + input_r * iir_amount;
                self.ratio_b_l = self.ratio_b_l * (1.0 - speed) + sense_l * speed;
                self.ratio_b_r = self.ratio_b_r * (1.0 - speed) + sense_r * speed;
                if self.ratio_b_l > depth {
                    self.ratio_b_l = depth;
                }
                if self.ratio_b_r > depth {
                    self.ratio_b_r = depth;
                }
                if self.ratio_b_l > 1.0 {
                    input_l =
                        self.iir_sample_b_l + (input_l - self.iir_sample_b_l) / self.ratio_b_l;
                }
                if self.ratio_b_r > 1.0 {
                    input_r =
                        self.iir_sample_b_r + (input_r - self.iir_sample_b_r) / self.ratio_b_r;
                }
            }
            self.flip = !self.flip;

            if params.monitor {
                input_l = dry_l - input_l;
                input_r = dry_r - input_r;
            }

            // Dither
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

            *sample_l = input_l as f32;
            *sample_r = input_r as f32;
        }
    }
}
