pub struct Saturator {
    previous_sample_l: f64,
    previous_sample_r: f64,
    fpd_l: u32,
    fpd_r: u32,
}

impl Default for Saturator {
    fn default() -> Self {
        Self {
            previous_sample_l: 0.0,
            previous_sample_r: 0.0,
            fpd_l: rand::random(),
            fpd_r: rand::random(),
        }
    }
}

impl Saturator {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32], intensity: f64) {
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

            input_l = input_l.sin();
            let apply_l = ((self.previous_sample_l + input_l).abs() / 2.0) * intensity;
            input_l = dry_l * (1.0 - apply_l) + input_l * apply_l;
            self.previous_sample_l = dry_l.sin();

            input_r = input_r.sin();
            let apply_r = ((self.previous_sample_r + input_r).abs() / 2.0) * intensity;
            input_r = dry_r * (1.0 - apply_r) + input_r * apply_r;
            self.previous_sample_r = dry_r.sin();

            // dither
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
