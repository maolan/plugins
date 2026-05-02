use std::f64::consts::PI;

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
// ImagerWide
// ---------------------------------------------------------------------------
#[derive(Clone, Copy)]
struct Biquad {
    freq: f64,
    q: f64,
    a0: f64,
    a1: f64,
    a2: f64,
    b1: f64,
    b2: f64,
    z1: f64,
    z2: f64,
    z1r: f64,
    z2r: f64,
}

impl Default for Biquad {
    fn default() -> Self {
        Self {
            freq: 0.0,
            q: 0.0,
            a0: 0.0,
            a1: 0.0,
            a2: 0.0,
            b1: 0.0,
            b2: 0.0,
            z1: 0.0,
            z2: 0.0,
            z1r: 0.0,
            z2r: 0.0,
        }
    }
}

impl Biquad {
    fn set_params(&mut self, freq: f64, q: f64) {
        self.freq = freq;
        self.q = q;
        let k = (PI * freq).tan();
        let norm = 1.0 / (1.0 + k / q + k * k);
        self.a0 = k / q * norm;
        self.a1 = 0.0;
        self.a2 = -self.a0;
        self.b1 = 2.0 * (k * k - 1.0) * norm;
        self.b2 = (1.0 - k / q + k * k) * norm;
    }

    fn _process_mono(&mut self, input: f64) -> f64 {
        let out = input * self.a0 + self.z1;
        self.z1 = input * self.a1 + self.z2 - self.b1 * out;
        self.z2 = input * self.a2 - self.b2 * out;
        out
    }

    fn process_left(&mut self, input: f64) -> f64 {
        let out = input * self.a0 + self.z1;
        self.z1 = input * self.a1 + self.z2 - self.b1 * out;
        self.z2 = input * self.a2 - self.b2 * out;
        out
    }

    fn process_right(&mut self, input: f64) -> f64 {
        let out = input * self.a0 + self.z1r;
        self.z1r = input * self.a1 + self.z2r - self.b1 * out;
        self.z2r = input * self.a2 - self.b2 * out;
        out
    }
}

pub struct ImagerWideParams {
    pub center: f64,
    pub space: f64,
    pub level: f64,
    pub q_param: f64,
    pub wet: f64,
}

pub struct ImagerWide {
    biquad_m2: Biquad,
    biquad_m7: Biquad,
    biquad_m10: Biquad,
    biquad_l3: Biquad,
    biquad_l7: Biquad,
    biquad_r3: Biquad,
    biquad_r7: Biquad,
    biquad_s3: Biquad,
    biquad_s5: Biquad,
    fpd_l: u32,
    fpd_r: u32,
    sample_rate: f64,
}

impl Default for ImagerWide {
    fn default() -> Self {
        Self {
            biquad_m2: Biquad::default(),
            biquad_m7: Biquad::default(),
            biquad_m10: Biquad::default(),
            biquad_l3: Biquad::default(),
            biquad_l7: Biquad::default(),
            biquad_r3: Biquad::default(),
            biquad_r7: Biquad::default(),
            biquad_s3: Biquad::default(),
            biquad_s5: Biquad::default(),
            fpd_l: rand::random(),
            fpd_r: rand::random(),
            sample_rate: 48_000.0,
        }
    }
}

impl ImagerWide {
    pub fn reset(&mut self) {
        *self = Self {
            sample_rate: self.sample_rate,
            ..Default::default()
        };
    }

    pub fn set_sample_rate(&mut self, sr: f64) {
        self.sample_rate = sr;
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &ImagerWideParams,
    ) {
        let sample_rate = self.sample_rate.max(22_000.0);
        let ImagerWideParams {
            center,
            space,
            level,
            q_param,
            wet,
        } = *params;

        self.biquad_m2.set_params(2000.0 / sample_rate, 0.0);
        self.biquad_m7.set_params(7000.0 / sample_rate, 0.0);
        self.biquad_m10.set_params(10000.0 / sample_rate, 0.0);
        self.biquad_l3.set_params(3000.0 / sample_rate, 0.0);
        self.biquad_l7.set_params(7000.0 / sample_rate, 0.0);
        self.biquad_r3.set_params(3000.0 / sample_rate, 0.0);
        self.biquad_r7.set_params(7000.0 / sample_rate, 0.0);
        self.biquad_s3.set_params(3000.0 / sample_rate, 0.0);
        self.biquad_s5.set_params(5000.0 / sample_rate, 0.0);

        let focus_m = 15.0 - center * 10.0;
        let focus_s = 21.0 - space * 15.0;
        let q = q_param + 0.25;
        let mut gain_m = center * 2.0;
        let gain_s = space * 2.0;
        if gain_s > 1.0 {
            gain_m /= gain_s;
        }
        gain_m = gain_m.clamp(0.0, 1.0);

        self.biquad_m2.q = focus_m * 0.25 * q;
        self.biquad_m7.q = focus_m * q;
        self.biquad_m10.q = focus_m * q;
        self.biquad_s3.q = focus_m * q;
        self.biquad_s5.q = focus_m * q;

        self.biquad_l3.q = focus_s * q;
        self.biquad_l7.q = focus_s * q;
        self.biquad_r3.q = focus_s * q;
        self.biquad_r7.q = focus_s * q;

        // recompute coefficients with updated Q
        self.biquad_m2
            .set_params(self.biquad_m2.freq, self.biquad_m2.q);
        self.biquad_m7
            .set_params(self.biquad_m7.freq, self.biquad_m7.q);
        self.biquad_m10
            .set_params(self.biquad_m10.freq, self.biquad_m10.q);
        self.biquad_l3
            .set_params(self.biquad_l3.freq, self.biquad_l3.q);
        self.biquad_l7
            .set_params(self.biquad_l7.freq, self.biquad_l7.q);
        self.biquad_r3
            .set_params(self.biquad_r3.freq, self.biquad_r3.q);
        self.biquad_r7
            .set_params(self.biquad_r7.freq, self.biquad_r7.q);
        self.biquad_s3
            .set_params(self.biquad_s3.freq, self.biquad_s3.q);
        self.biquad_s5
            .set_params(self.biquad_s5.freq, self.biquad_s5.q);

        let depth_m = center.powi(2) * 2.0;
        let depth_s = space.powi(2) * 2.0;

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
            input_r = input_r.sin();

            let mut mid = input_l + input_r;
            let rawmid = mid * 0.5;
            let mut side = input_l - input_r;
            let boostside = side * depth_s;

            let m2_sample = self.biquad_m2.process_left(mid);
            let m7_sample = -self.biquad_m7.process_left(mid) * 2.0;
            let m10_sample = -self.biquad_m10.process_left(mid) * 2.0;

            let s3_sample = self.biquad_s3.process_left(side) * 2.0;
            let s5_sample = -self.biquad_s5.process_left(side) * 5.0;

            mid = (m2_sample + m7_sample + m10_sample) * depth_m;
            side = (s3_sample + s5_sample + boostside) * depth_s;

            let ms_out_l = (mid + side) / 2.0;
            let ms_out_r = (mid - side) / 2.0;

            let iso_l = input_l - rawmid;
            let iso_r = input_r - rawmid;

            let l3_sample = self.biquad_l3.process_left(iso_l);
            let r3_sample = self.biquad_r3.process_right(iso_r);
            let l7_sample = self.biquad_l7.process_left(iso_l) * 3.0;
            let r7_sample = self.biquad_r7.process_right(iso_r) * 3.0;

            let processing_l = ms_out_l + (l3_sample + l7_sample) * depth_s;
            let processing_r = ms_out_r + (r3_sample + r7_sample) * depth_s;

            mid = input_l + input_r;
            side = input_l - input_r;

            mid *= gain_m;
            side *= gain_s;
            side = side.clamp(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
            side = side.sin();
            side *= gain_s;

            input_l = (mid + side) / 2.0 + processing_l;
            input_r = (mid - side) / 2.0 + processing_r;

            if level < 1.0 {
                input_l *= level;
                input_r *= level;
            }

            input_l = input_l.clamp(-1.0, 1.0);
            input_r = input_r.clamp(-1.0, 1.0);

            input_l = input_l.asin();
            input_r = input_r.asin();

            if wet < 1.0 {
                input_l = input_l * wet + dry_l * (1.0 - wet);
                input_r = input_r * wet + dry_r * (1.0 - wet);
            }

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
// Aggressive
// ---------------------------------------------------------------------------
pub struct Aggressive {
    iir_sample_a: f64,
    iir_sample_b: f64,
    flip: bool,
    fpd_l: u32,
    fpd_r: u32,
    sample_rate: f64,
}

impl Default for Aggressive {
    fn default() -> Self {
        Self {
            iir_sample_a: 0.0,
            iir_sample_b: 0.0,
            flip: false,
            fpd_l: rand::random(),
            fpd_r: rand::random(),
            sample_rate: 48_000.0,
        }
    }
}

impl Aggressive {
    pub fn reset(&mut self) {
        *self = Self {
            sample_rate: self.sample_rate,
            ..Default::default()
        };
    }

    pub fn set_sample_rate(&mut self, sr: f64) {
        self.sample_rate = sr;
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        wide: f64,
        mono_bs: f64,
        c_squish: f64,
    ) {
        let overallscale = self.sample_rate / 44_100.0;
        let stereowide = wide;
        let centersquish = c_squish;
        let density = stereowide * 2.4;
        let sustain = 1.0 - (1.0 / (1.0 + density / 7.0));
        let iir_amount = mono_bs.powi(3) / overallscale;
        let tight = -0.333_333_333_333_33;

        for (sample_l, sample_r) in left.iter_mut().zip(right.iter_mut()) {
            let mut input_l = *sample_l as f64;
            let mut input_r = *sample_r as f64;

            if input_l.abs() < 1.18e-23 {
                input_l = self.fpd_l as f64 * 1.18e-17;
            }
            if input_r.abs() < 1.18e-23 {
                input_r = self.fpd_r as f64 * 1.18e-17;
            }

            let mut mid = input_l + input_r;
            let mut side = input_l - input_r;

            // High Impact
            let mut count = density;
            while count > 1.0 {
                let mut bridgerectifier = (side.abs() * std::f64::consts::FRAC_PI_2)
                    .clamp(0.0, std::f64::consts::FRAC_PI_2);
                bridgerectifier = bridgerectifier.sin();
                if side > 0.0 {
                    side = bridgerectifier;
                } else {
                    side = -bridgerectifier;
                }
                count -= 1.0;
            }

            let mut bridgerectifier =
                (side.abs() * std::f64::consts::FRAC_PI_2).clamp(0.0, std::f64::consts::FRAC_PI_2);
            bridgerectifier = bridgerectifier.sin();
            if side > 0.0 {
                side = side * (1.0 - count) + bridgerectifier * count;
            } else {
                side = side * (1.0 - count) - bridgerectifier * count;
            }

            bridgerectifier =
                (side.abs() * std::f64::consts::FRAC_PI_2).clamp(0.0, std::f64::consts::FRAC_PI_2);
            bridgerectifier = (1.0 - bridgerectifier.cos()) * PI;
            if side > 0.0 {
                side = side * (1.0 - sustain) + bridgerectifier * sustain;
            } else {
                side = side * (1.0 - sustain) - bridgerectifier * sustain;
            }

            // Highpass
            let mut offset = 0.666_666_666_666_666 + (1.0 - side.abs()) * tight;
            offset = offset.clamp(0.0, 1.0);
            if self.flip {
                self.iir_sample_a =
                    self.iir_sample_a * (1.0 - offset * iir_amount) + side * (offset * iir_amount);
                side -= self.iir_sample_a;
            } else {
                self.iir_sample_b =
                    self.iir_sample_b * (1.0 - offset * iir_amount) + side * (offset * iir_amount);
                side -= self.iir_sample_b;
            }
            self.flip = !self.flip;

            // Mid saturating
            bridgerectifier =
                (mid.abs() / 1.273_239_544_735_162).clamp(0.0, std::f64::consts::FRAC_PI_2);
            bridgerectifier = bridgerectifier.sin() * 1.273_239_544_735_162;
            if mid > 0.0 {
                mid = mid * (1.0 - centersquish) + bridgerectifier * centersquish;
            } else {
                mid = mid * (1.0 - centersquish) - bridgerectifier * centersquish;
            }

            input_l = (mid + side) / 2.0;
            input_r = (mid - side) / 2.0;

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
    pub resonance: f64,
    pub mix: f64,
}

#[derive(Default)]
pub struct Imager {
    pub imager_mild: ImagerMild,
    pub imager_wide: ImagerWide,
    pub aggressive: Aggressive,
}

impl Imager {
    pub fn set_sample_rate(&mut self, sr: f64) {
        self.imager_mild.set_sample_rate(sr);
        self.imager_wide.set_sample_rate(sr);
        self.aggressive.set_sample_rate(sr);
    }

    pub fn reset(&mut self) {
        self.imager_mild.reset();
        self.imager_wide.reset();
        self.aggressive.reset();
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        mode: u32,
        params: &ImagerParams,
    ) {
        match mode {
            0 => self.imager_mild.process_stereo(
                left,
                right,
                params.width,
                params.focus,
                params.amount,
            ),
            1 => self.imager_wide.process_stereo(
                left,
                right,
                &ImagerWideParams {
                    center: params.width,
                    space: params.focus,
                    level: params.amount,
                    q_param: params.resonance,
                    wet: params.mix,
                },
            ),
            2 => self.aggressive.process_stereo(
                left,
                right,
                params.width,
                params.focus,
                params.amount,
            ),
            _ => {}
        }
    }
}
