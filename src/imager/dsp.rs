// ---------------------------------------------------------------------------
// ImagerMild
// ---------------------------------------------------------------------------
pub struct ImagerMild {
    p: Vec<f64>,
    count: i32,
    fpd_l: u32,
    fpd_r: u32,
    sample_rate: f64,
}

impl Default for ImagerMild {
    fn default() -> Self {
        Self {
            p: vec![0.0; 4099],
            count: 2048,
            fpd_l: rand::random(),
            fpd_r: rand::random(),
            sample_rate: 48_000.0,
        }
    }
}

impl ImagerMild {
    pub fn reset(&mut self) {
        self.p.fill(0.0);
        self.count = 2048;
        self.fpd_l = rand::random();
        self.fpd_r = rand::random();
    }

    pub fn set_sample_rate(&mut self, sr: f64) {
        self.sample_rate = sr;
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        width: f64,
        focus: f64,
        amount: f64,
    ) {
        let overallscale = self.sample_rate / 44_100.0;
        let densityside = width * 2.0 - 1.0;
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

            input_l = dry_l * (1.0 - wet) + (mid + side) * wet;
            input_r = dry_r * (1.0 - wet) + (mid - side) * wet;

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

// ---------------------------------------------------------------------------
// Imager dispatcher
// ---------------------------------------------------------------------------
pub struct ImagerParams {
    pub width: f64,
    pub focus: f64,
    pub amount: f64,
}

#[derive(Default)]
pub struct Imager {
    pub imager_mild: ImagerMild,
}

impl Imager {
    pub fn set_sample_rate(&mut self, sr: f64) {
        self.imager_mild.set_sample_rate(sr);
    }

    pub fn reset(&mut self) {
        self.imager_mild.reset();
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32], params: &ImagerParams) {
        self.imager_mild
            .process_stereo(left, right, params.width, params.focus, params.amount);
    }
}
