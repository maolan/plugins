const FP_OLD: f64 = 0.618_033_988_749_894_8;
const FP_NEW: f64 = 1.0 - FP_OLD;
const BUFFER_SIZE: usize = 22_200;
const HALF_BUFFER: usize = 11_020;

// ---------------------------------------------------------------------------
// MaximizerVintage
// ---------------------------------------------------------------------------
pub struct MaximizerVintage {
    last_sample_l: f64,
    last_sample_r: f64,
    b_l: [f32; BUFFER_SIZE],
    b_r: [f32; BUFFER_SIZE],
    gcount: i32,
    lows_l: f64,
    lows_r: f64,
    refclip_l: f64,
    refclip_r: f64,
    iir_lows_al: f64,
    iir_lows_ar: f64,
    iir_lows_bl: f64,
    iir_lows_br: f64,
    fpd_l: u32,
    fpd_r: u32,
    sample_rate: f64,
}

impl Default for MaximizerVintage {
    fn default() -> Self {
        Self {
            last_sample_l: 0.0,
            last_sample_r: 0.0,
            b_l: [0.0; BUFFER_SIZE],
            b_r: [0.0; BUFFER_SIZE],
            gcount: HALF_BUFFER as i32,
            lows_l: 0.0,
            lows_r: 0.0,
            refclip_l: 0.99,
            refclip_r: 0.99,
            iir_lows_al: 0.0,
            iir_lows_ar: 0.0,
            iir_lows_bl: 0.0,
            iir_lows_br: 0.0,
            fpd_l: rand::random(),
            fpd_r: rand::random(),
            sample_rate: 48_000.0,
        }
    }
}

impl MaximizerVintage {
    pub fn set_sample_rate(&mut self, sr: f64) {
        self.sample_rate = sr;
    }

    pub fn reset(&mut self) {
        *self = Self {
            sample_rate: self.sample_rate,
            ..Default::default()
        };
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        boost: f64,
        soften: f64,
        enhance: f64,
        mode: u32,
    ) {
        let overallscale = self.sample_rate / 44_100.0;
        let input_gain = 10.0_f64.powf((boost * 18.0) / 20.0);
        let softness = soften * FP_NEW;
        let hardness = 1.0 - softness;
        let highslift = 0.307 * enhance;
        let adjust = highslift.powi(3) * 0.416;
        let subslift = 0.796 * enhance;
        let calibsubs = subslift / 53.0;
        let invcalibsubs = 1.0 - calibsubs;
        let subs = 0.81 + (calibsubs * 2.0);
        let mode = mode.min(2) + 1;

        let offset_h1 = 1.84 * overallscale;
        let offset_h2 = offset_h1 * 1.9;
        let offset_h3 = offset_h1 * 2.7;
        let offset_l1 = 612.0 * overallscale;
        let offset_l2 = offset_l1 * 2.0;

        let ref_h1 = offset_h1.floor() as i32;
        let ref_h2 = offset_h2.floor() as i32;
        let ref_h3 = offset_h3.floor() as i32;
        let ref_l1 = offset_l1.floor() as i32;
        let ref_l2 = offset_l2.floor() as i32;

        let fraction_h1 = offset_h1 - offset_h1.floor();
        let fraction_h2 = offset_h2 - offset_h2.floor();
        let fraction_h3 = offset_h3 - offset_h3.floor();
        let minus_h1 = 1.0 - fraction_h1;
        let minus_h2 = 1.0 - fraction_h2;
        let minus_h3 = 1.0 - fraction_h3;

        for (input_l, input_r) in left.iter_mut().zip(right.iter_mut()) {
            let mut sample_l = *input_l as f64;
            let mut sample_r = *input_r as f64;

            if sample_l.abs() < 1.18e-23 {
                sample_l = self.fpd_l as f64 * 1.18e-17;
            }
            if sample_r.abs() < 1.18e-23 {
                sample_r = self.fpd_r as f64 * 1.18e-17;
            }

            if input_gain != 1.0 {
                sample_l *= input_gain;
                sample_r *= input_gain;
            }

            let overshoot_l = (sample_l.abs() - self.refclip_l).max(0.0);
            let overshoot_r = (sample_r.abs() - self.refclip_r).max(0.0);

            if self.gcount < 0 || self.gcount > HALF_BUFFER as i32 {
                self.gcount = HALF_BUFFER as i32;
            }
            let count = self.gcount as usize;
            self.b_l[count] = overshoot_l as f32;
            self.b_l[count + HALF_BUFFER] = overshoot_l as f32;
            self.b_r[count] = overshoot_r as f32;
            self.b_r[count + HALF_BUFFER] = overshoot_r as f32;
            self.gcount -= 1;

            let mut highs_l = 0.0;
            let mut highs_r = 0.0;

            if highslift > 0.0 {
                let temp = count + ref_h3 as usize;
                highs_l = -(self.b_l[temp] as f64 * minus_h3);
                highs_l -= self.b_l[temp + 1] as f64;
                highs_l -= self.b_l[temp + 2] as f64 * fraction_h3;
                highs_l += ((self.b_l[temp] - self.b_l[temp + 1])
                    - (self.b_l[temp + 1] - self.b_l[temp + 2])) as f64
                    / 50.0;
                highs_l *= adjust;

                highs_r = -(self.b_r[temp] as f64 * minus_h3);
                highs_r -= self.b_r[temp + 1] as f64;
                highs_r -= self.b_r[temp + 2] as f64 * fraction_h3;
                highs_r += ((self.b_r[temp] - self.b_r[temp + 1])
                    - (self.b_r[temp + 1] - self.b_r[temp + 2])) as f64
                    / 50.0;
                highs_r *= adjust;

                let temp = count + ref_h2 as usize;
                highs_l += self.b_l[temp] as f64 * minus_h2;
                highs_l += self.b_l[temp + 1] as f64;
                highs_l += self.b_l[temp + 2] as f64 * fraction_h2;
                highs_l -= ((self.b_l[temp] - self.b_l[temp + 1])
                    - (self.b_l[temp + 1] - self.b_l[temp + 2])) as f64
                    / 50.0;
                highs_l *= adjust;

                highs_r += self.b_r[temp] as f64 * minus_h2;
                highs_r += self.b_r[temp + 1] as f64;
                highs_r += self.b_r[temp + 2] as f64 * fraction_h2;
                highs_r -= ((self.b_r[temp] - self.b_r[temp + 1])
                    - (self.b_r[temp + 1] - self.b_r[temp + 2])) as f64
                    / 50.0;
                highs_r *= adjust;

                let temp = count + ref_h1 as usize;
                highs_l -= self.b_l[temp] as f64 * minus_h1;
                highs_l -= self.b_l[temp + 1] as f64;
                highs_l -= self.b_l[temp + 2] as f64 * fraction_h1;
                highs_l += ((self.b_l[temp] - self.b_l[temp + 1])
                    - (self.b_l[temp + 1] - self.b_l[temp + 2])) as f64
                    / 50.0;
                highs_l *= adjust;

                highs_r -= self.b_r[temp] as f64 * minus_h1;
                highs_r -= self.b_r[temp + 1] as f64;
                highs_r -= self.b_r[temp + 2] as f64 * fraction_h1;
                highs_r += ((self.b_r[temp] - self.b_r[temp + 1])
                    - (self.b_r[temp + 1] - self.b_r[temp + 2])) as f64
                    / 50.0;
                highs_r *= adjust;
            }

            let mut bridgerectifier = (highs_l.abs() * hardness).sin();
            highs_l = if highs_l > 0.0 {
                bridgerectifier
            } else {
                -bridgerectifier
            };

            bridgerectifier = (highs_r.abs() * hardness).sin();
            highs_r = if highs_r > 0.0 {
                bridgerectifier
            } else {
                -bridgerectifier
            };

            if subslift > 0.0 {
                self.lows_l *= subs;
                self.lows_r *= subs;

                let temp = count + ref_l1 as usize;
                self.lows_l -= self.b_l[temp + 127] as f64;
                self.lows_l -= self.b_l[temp + 113] as f64;
                self.lows_l -= self.b_l[temp + 109] as f64;
                self.lows_l -= self.b_l[temp + 107] as f64;
                self.lows_l -= self.b_l[temp + 103] as f64;
                self.lows_l -= self.b_l[temp + 101] as f64;
                self.lows_l -= self.b_l[temp + 97] as f64;
                self.lows_l -= self.b_l[temp + 89] as f64;
                self.lows_l -= self.b_l[temp + 83] as f64;
                self.lows_l -= self.b_l[temp + 79] as f64;
                self.lows_l -= self.b_l[temp + 73] as f64;
                self.lows_l -= self.b_l[temp + 71] as f64;
                self.lows_l -= self.b_l[temp + 67] as f64;
                self.lows_l -= self.b_l[temp + 61] as f64;
                self.lows_l -= self.b_l[temp + 59] as f64;
                self.lows_l -= self.b_l[temp + 53] as f64;
                self.lows_l -= self.b_l[temp + 47] as f64;
                self.lows_l -= self.b_l[temp + 43] as f64;
                self.lows_l -= self.b_l[temp + 41] as f64;
                self.lows_l -= self.b_l[temp + 37] as f64;
                self.lows_l -= self.b_l[temp + 31] as f64;
                self.lows_l -= self.b_l[temp + 29] as f64;
                self.lows_l -= self.b_l[temp + 23] as f64;
                self.lows_l -= self.b_l[temp + 19] as f64;
                self.lows_l -= self.b_l[temp + 17] as f64;
                self.lows_l -= self.b_l[temp + 13] as f64;
                self.lows_l -= self.b_l[temp + 11] as f64;
                self.lows_l -= self.b_l[temp + 7] as f64;
                self.lows_l -= self.b_l[temp + 5] as f64;
                self.lows_l -= self.b_l[temp + 3] as f64;
                self.lows_l -= self.b_l[temp + 2] as f64;
                self.lows_l -= self.b_l[temp + 1] as f64;

                self.lows_r -= self.b_r[temp + 127] as f64;
                self.lows_r -= self.b_r[temp + 113] as f64;
                self.lows_r -= self.b_r[temp + 109] as f64;
                self.lows_r -= self.b_r[temp + 107] as f64;
                self.lows_r -= self.b_r[temp + 103] as f64;
                self.lows_r -= self.b_r[temp + 101] as f64;
                self.lows_r -= self.b_r[temp + 97] as f64;
                self.lows_r -= self.b_r[temp + 89] as f64;
                self.lows_r -= self.b_r[temp + 83] as f64;
                self.lows_r -= self.b_r[temp + 79] as f64;
                self.lows_r -= self.b_r[temp + 73] as f64;
                self.lows_r -= self.b_r[temp + 71] as f64;
                self.lows_r -= self.b_r[temp + 67] as f64;
                self.lows_r -= self.b_r[temp + 61] as f64;
                self.lows_r -= self.b_r[temp + 59] as f64;
                self.lows_r -= self.b_r[temp + 53] as f64;
                self.lows_r -= self.b_r[temp + 47] as f64;
                self.lows_r -= self.b_r[temp + 43] as f64;
                self.lows_r -= self.b_r[temp + 41] as f64;
                self.lows_r -= self.b_r[temp + 37] as f64;
                self.lows_r -= self.b_r[temp + 31] as f64;
                self.lows_r -= self.b_r[temp + 29] as f64;
                self.lows_r -= self.b_r[temp + 23] as f64;
                self.lows_r -= self.b_r[temp + 19] as f64;
                self.lows_r -= self.b_r[temp + 17] as f64;
                self.lows_r -= self.b_r[temp + 13] as f64;
                self.lows_r -= self.b_r[temp + 11] as f64;
                self.lows_r -= self.b_r[temp + 7] as f64;
                self.lows_r -= self.b_r[temp + 5] as f64;
                self.lows_r -= self.b_r[temp + 3] as f64;
                self.lows_r -= self.b_r[temp + 2] as f64;
                self.lows_r -= self.b_r[temp + 1] as f64;

                self.lows_l *= subs * subs;
                self.lows_r *= subs * subs;

                let temp = count + ref_l2 as usize;
                self.lows_l += self.b_l[temp + 127] as f64;
                self.lows_l += self.b_l[temp + 113] as f64;
                self.lows_l += self.b_l[temp + 109] as f64;
                self.lows_l += self.b_l[temp + 107] as f64;
                self.lows_l += self.b_l[temp + 103] as f64;
                self.lows_l += self.b_l[temp + 101] as f64;
                self.lows_l += self.b_l[temp + 97] as f64;
                self.lows_l += self.b_l[temp + 89] as f64;
                self.lows_l += self.b_l[temp + 83] as f64;
                self.lows_l += self.b_l[temp + 79] as f64;
                self.lows_l += self.b_l[temp + 73] as f64;
                self.lows_l += self.b_l[temp + 71] as f64;
                self.lows_l += self.b_l[temp + 67] as f64;
                self.lows_l += self.b_l[temp + 61] as f64;
                self.lows_l += self.b_l[temp + 59] as f64;
                self.lows_l += self.b_l[temp + 53] as f64;
                self.lows_l += self.b_l[temp + 47] as f64;
                self.lows_l += self.b_l[temp + 43] as f64;
                self.lows_l += self.b_l[temp + 41] as f64;
                self.lows_l += self.b_l[temp + 37] as f64;
                self.lows_l += self.b_l[temp + 31] as f64;
                self.lows_l += self.b_l[temp + 29] as f64;
                self.lows_l += self.b_l[temp + 23] as f64;
                self.lows_l += self.b_l[temp + 19] as f64;
                self.lows_l += self.b_l[temp + 17] as f64;
                self.lows_l += self.b_l[temp + 13] as f64;
                self.lows_l += self.b_l[temp + 11] as f64;
                self.lows_l += self.b_l[temp + 7] as f64;
                self.lows_l += self.b_l[temp + 5] as f64;
                self.lows_l += self.b_l[temp + 3] as f64;
                self.lows_l += self.b_l[temp + 2] as f64;
                self.lows_l += self.b_l[temp + 1] as f64;

                self.lows_r += self.b_r[temp + 127] as f64;
                self.lows_r += self.b_r[temp + 113] as f64;
                self.lows_r += self.b_r[temp + 109] as f64;
                self.lows_r += self.b_r[temp + 107] as f64;
                self.lows_r += self.b_r[temp + 103] as f64;
                self.lows_r += self.b_r[temp + 101] as f64;
                self.lows_r += self.b_r[temp + 97] as f64;
                self.lows_r += self.b_r[temp + 89] as f64;
                self.lows_r += self.b_r[temp + 83] as f64;
                self.lows_r += self.b_r[temp + 79] as f64;
                self.lows_r += self.b_r[temp + 73] as f64;
                self.lows_r += self.b_r[temp + 71] as f64;
                self.lows_r += self.b_r[temp + 67] as f64;
                self.lows_r += self.b_r[temp + 61] as f64;
                self.lows_r += self.b_r[temp + 59] as f64;
                self.lows_r += self.b_r[temp + 53] as f64;
                self.lows_r += self.b_r[temp + 47] as f64;
                self.lows_r += self.b_r[temp + 43] as f64;
                self.lows_r += self.b_r[temp + 41] as f64;
                self.lows_r += self.b_r[temp + 37] as f64;
                self.lows_r += self.b_r[temp + 31] as f64;
                self.lows_r += self.b_r[temp + 29] as f64;
                self.lows_r += self.b_r[temp + 23] as f64;
                self.lows_r += self.b_r[temp + 19] as f64;
                self.lows_r += self.b_r[temp + 17] as f64;
                self.lows_r += self.b_r[temp + 13] as f64;
                self.lows_r += self.b_r[temp + 11] as f64;
                self.lows_r += self.b_r[temp + 7] as f64;
                self.lows_r += self.b_r[temp + 5] as f64;
                self.lows_r += self.b_r[temp + 3] as f64;
                self.lows_r += self.b_r[temp + 2] as f64;
                self.lows_r += self.b_r[temp + 1] as f64;

                self.lows_l *= subs;
                self.lows_r *= subs;
            }

            bridgerectifier = (self.lows_l.abs() * softness).sin();
            self.lows_l = if self.lows_l > 0.0 {
                bridgerectifier
            } else {
                -bridgerectifier
            };

            bridgerectifier = (self.lows_r.abs() * softness).sin();
            self.lows_r = if self.lows_r > 0.0 {
                bridgerectifier
            } else {
                -bridgerectifier
            };

            self.iir_lows_al = (self.iir_lows_al * invcalibsubs) + (self.lows_l * calibsubs);
            self.lows_l = self.iir_lows_al;
            bridgerectifier = self.lows_l.abs().sin();
            self.lows_l = if self.lows_l > 0.0 {
                bridgerectifier
            } else {
                -bridgerectifier
            };

            self.iir_lows_ar = (self.iir_lows_ar * invcalibsubs) + (self.lows_r * calibsubs);
            self.lows_r = self.iir_lows_ar;
            bridgerectifier = self.lows_r.abs().sin();
            self.lows_r = if self.lows_r > 0.0 {
                bridgerectifier
            } else {
                -bridgerectifier
            };

            self.iir_lows_bl = (self.iir_lows_bl * invcalibsubs) + (self.lows_l * calibsubs);
            self.lows_l = self.iir_lows_bl;
            bridgerectifier = self.lows_l.abs().sin() * 2.0;
            self.lows_l = if self.lows_l > 0.0 {
                bridgerectifier
            } else {
                -bridgerectifier
            };

            self.iir_lows_br = (self.iir_lows_br * invcalibsubs) + (self.lows_r * calibsubs);
            self.lows_r = self.iir_lows_br;
            bridgerectifier = self.lows_r.abs().sin() * 2.0;
            self.lows_r = if self.lows_r > 0.0 {
                bridgerectifier
            } else {
                -bridgerectifier
            };

            if highslift > 0.0 {
                sample_l += highs_l * (1.0 - sample_l.abs() * hardness);
            }
            if subslift > 0.0 {
                sample_l += self.lows_l * (1.0 - sample_l.abs() * softness);
            }

            if highslift > 0.0 {
                sample_r += highs_r * (1.0 - sample_r.abs() * hardness);
            }
            if subslift > 0.0 {
                sample_r += self.lows_r * (1.0 - sample_r.abs() * softness);
            }

            if sample_l > self.refclip_l && self.refclip_l > 0.9 {
                self.refclip_l -= 0.01;
            }
            if sample_l < -self.refclip_l && self.refclip_l > 0.9 {
                self.refclip_l -= 0.01;
            }
            if self.refclip_l < 0.99 {
                self.refclip_l += 0.000_01;
            }

            if sample_r > self.refclip_r && self.refclip_r > 0.9 {
                self.refclip_r -= 0.01;
            }
            if sample_r < -self.refclip_r && self.refclip_r > 0.9 {
                self.refclip_r -= 0.01;
            }
            if self.refclip_r < 0.99 {
                self.refclip_r += 0.000_01;
            }

            if self.last_sample_l >= self.refclip_l {
                if sample_l < self.refclip_l {
                    self.last_sample_l = self.refclip_l * hardness + sample_l * softness;
                } else {
                    self.last_sample_l = self.refclip_l;
                }
            }
            if self.last_sample_r >= self.refclip_r {
                if sample_r < self.refclip_r {
                    self.last_sample_r = self.refclip_r * hardness + sample_r * softness;
                } else {
                    self.last_sample_r = self.refclip_r;
                }
            }
            if self.last_sample_l <= -self.refclip_l {
                if sample_l > -self.refclip_l {
                    self.last_sample_l = -self.refclip_l * hardness + sample_l * softness;
                } else {
                    self.last_sample_l = -self.refclip_l;
                }
            }
            if self.last_sample_r <= -self.refclip_r {
                if sample_r > -self.refclip_r {
                    self.last_sample_r = -self.refclip_r * hardness + sample_r * softness;
                } else {
                    self.last_sample_r = -self.refclip_r;
                }
            }

            if sample_l > self.refclip_l {
                if self.last_sample_l < self.refclip_l {
                    sample_l = self.refclip_l * hardness + self.last_sample_l * softness;
                } else {
                    sample_l = self.refclip_l;
                }
            }
            if sample_r > self.refclip_r {
                if self.last_sample_r < self.refclip_r {
                    sample_r = self.refclip_r * hardness + self.last_sample_r * softness;
                } else {
                    sample_r = self.refclip_r;
                }
            }
            if sample_l < -self.refclip_l {
                if self.last_sample_l > -self.refclip_l {
                    sample_l = -self.refclip_l * hardness + self.last_sample_l * softness;
                } else {
                    sample_l = -self.refclip_l;
                }
            }
            if sample_r < -self.refclip_r {
                if self.last_sample_r > -self.refclip_r {
                    sample_r = -self.refclip_r * hardness + self.last_sample_r * softness;
                } else {
                    sample_r = -self.refclip_r;
                }
            }

            self.last_sample_l = sample_l;
            self.last_sample_r = sample_r;

            match mode {
                1 => {}
                2 => {
                    sample_l /= input_gain;
                    sample_r /= input_gain;
                }
                3 => {
                    sample_l = overshoot_l + highs_l + self.lows_l;
                    sample_r = overshoot_r + highs_r + self.lows_r;
                }
                _ => {}
            }

            sample_l = sample_l.clamp(-self.refclip_l, self.refclip_l);
            sample_r = sample_r.clamp(-self.refclip_r, self.refclip_r);

            let mut expon = sample_l.abs().log2().floor() as i32;
            self.fpd_l ^= self.fpd_l << 13;
            self.fpd_l ^= self.fpd_l >> 17;
            self.fpd_l ^= self.fpd_l << 5;
            sample_l +=
                (self.fpd_l as f64 - 0x7fff_ffffu32 as f64) * 5.5e-36 * 2.0_f64.powi(expon + 62);

            expon = sample_r.abs().log2().floor() as i32;
            self.fpd_r ^= self.fpd_r << 13;
            self.fpd_r ^= self.fpd_r >> 17;
            self.fpd_r ^= self.fpd_r << 5;
            sample_r +=
                (self.fpd_r as f64 - 0x7fff_ffffu32 as f64) * 5.5e-36 * 2.0_f64.powi(expon + 62);

            *input_l = sample_l as f32;
            *input_r = sample_r as f32;
        }
    }
}

// ---------------------------------------------------------------------------
// MaximizerModern
// ---------------------------------------------------------------------------
pub struct MaximizerModern {
    last_sample_l: [f64; 8],
    last_sample_r: [f64; 8],
    intermediate_l: [[f64; 8]; 17],
    intermediate_r: [[f64; 8]; 17],
    was_pos_clip_l: [bool; 8],
    was_neg_clip_l: [bool; 8],
    was_pos_clip_r: [bool; 8],
    was_neg_clip_r: [bool; 8],
    fpd_l: u32,
    fpd_r: u32,
    sample_rate: f64,
}

impl Default for MaximizerModern {
    fn default() -> Self {
        Self {
            last_sample_l: [0.0; 8],
            last_sample_r: [0.0; 8],
            intermediate_l: [[0.0; 8]; 17],
            intermediate_r: [[0.0; 8]; 17],
            was_pos_clip_l: [false; 8],
            was_neg_clip_l: [false; 8],
            was_pos_clip_r: [false; 8],
            was_neg_clip_r: [false; 8],
            fpd_l: rand::random(),
            fpd_r: rand::random(),
            sample_rate: 48_000.0,
        }
    }
}

impl MaximizerModern {
    pub fn set_sample_rate(&mut self, sr: f64) {
        self.sample_rate = sr;
    }

    pub fn reset(&mut self) {
        *self = Self {
            sample_rate: self.sample_rate,
            ..Default::default()
        };
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        boost: f64,
        ceiling: f64,
        mode: u32,
    ) {
        let overallscale = self.sample_rate / 44_100.0;
        let mut spacing = overallscale.floor() as usize;
        spacing = spacing.clamp(1, 16);
        let input_gain = 10.0_f64.powf((boost * 18.0) / 20.0);
        let ceiling_val = (1.0 + ceiling * 0.235_947_33) * 0.5;
        let mode = mode.min(7) + 1;
        let mut stage_setting = mode as i32 - 2;
        if stage_setting < 1 {
            stage_setting = 1;
        }
        let stage_input_gain = ((input_gain - 1.0) / stage_setting as f64) + 1.0;
        let hardness = 0.618_033_988_749_894;
        let softness = 0.381_966_011_250_105;

        for (sample_l, sample_r) in left.iter_mut().zip(right.iter_mut()) {
            let mut input_l = *sample_l as f64;
            let mut input_r = *sample_r as f64;

            if input_l.abs() < 1.18e-23 {
                input_l = self.fpd_l as f64 * 1.18e-17;
            }
            if input_r.abs() < 1.18e-23 {
                input_r = self.fpd_r as f64 * 1.18e-17;
            }

            let mut overshoot_l = 0.0;
            let mut overshoot_r = 0.0;
            input_l *= 1.618_033_988_749_894;
            input_r *= 1.618_033_988_749_894;

            for stage in 0..stage_setting as usize {
                if stage_input_gain != 1.0 {
                    input_l *= stage_input_gain;
                    input_r *= stage_input_gain;
                }
                if stage == 0 {
                    overshoot_l = input_l.abs() - 1.618_033_988_749_894;
                    if overshoot_l < 0.0 {
                        overshoot_l = 0.0;
                    }
                    overshoot_r = input_r.abs() - 1.618_033_988_749_894;
                    if overshoot_r < 0.0 {
                        overshoot_r = 0.0;
                    }
                }

                input_l = input_l.clamp(-4.0, 4.0);
                input_r = input_r.clamp(-4.0, 4.0);

                let diff_l = input_l - self.last_sample_l[stage];
                let diff_r = input_r - self.last_sample_r[stage];
                if diff_l > hardness {
                    input_l = self.last_sample_l[stage] + hardness;
                }
                if diff_l < -hardness {
                    input_l = self.last_sample_l[stage] - hardness;
                }
                if diff_r > hardness {
                    input_r = self.last_sample_r[stage] + hardness;
                }
                if diff_r < -hardness {
                    input_r = self.last_sample_r[stage] - hardness;
                }

                // ClipOnly2 left
                if self.was_pos_clip_l[stage] {
                    if input_l < self.last_sample_l[stage] {
                        self.last_sample_l[stage] = 1.0 + input_l * softness;
                    } else {
                        self.last_sample_l[stage] = hardness + self.last_sample_l[stage] * hardness;
                    }
                }
                self.was_pos_clip_l[stage] = false;
                if input_l > 1.618_033_988_749_894 {
                    self.was_pos_clip_l[stage] = true;
                    input_l = 1.0 + self.last_sample_l[stage] * softness;
                }

                if self.was_neg_clip_l[stage] {
                    if input_l > self.last_sample_l[stage] {
                        self.last_sample_l[stage] = -1.0 + input_l * softness;
                    } else {
                        self.last_sample_l[stage] =
                            -hardness + self.last_sample_l[stage] * hardness;
                    }
                }
                self.was_neg_clip_l[stage] = false;
                if input_l < -1.618_033_988_749_894 {
                    self.was_neg_clip_l[stage] = true;
                    input_l = -1.0 + self.last_sample_l[stage] * softness;
                }

                self.intermediate_l[spacing][stage] = input_l;
                input_l = self.last_sample_l[stage];
                for x in (1..=spacing).rev() {
                    self.intermediate_l[x - 1][stage] = self.intermediate_l[x][stage];
                }
                self.last_sample_l[stage] = self.intermediate_l[0][stage];

                // ClipOnly2 right
                if self.was_pos_clip_r[stage] {
                    if input_r < self.last_sample_r[stage] {
                        self.last_sample_r[stage] = 1.0 + input_r * softness;
                    } else {
                        self.last_sample_r[stage] = hardness + self.last_sample_r[stage] * hardness;
                    }
                }
                self.was_pos_clip_r[stage] = false;
                if input_r > 1.618_033_988_749_894 {
                    self.was_pos_clip_r[stage] = true;
                    input_r = 1.0 + self.last_sample_r[stage] * softness;
                }

                if self.was_neg_clip_r[stage] {
                    if input_r > self.last_sample_r[stage] {
                        self.last_sample_r[stage] = -1.0 + input_r * softness;
                    } else {
                        self.last_sample_r[stage] =
                            -hardness + self.last_sample_r[stage] * hardness;
                    }
                }
                self.was_neg_clip_r[stage] = false;
                if input_r < -1.618_033_988_749_894 {
                    self.was_neg_clip_r[stage] = true;
                    input_r = -1.0 + self.last_sample_r[stage] * softness;
                }

                self.intermediate_r[spacing][stage] = input_r;
                input_r = self.last_sample_r[stage];
                for x in (1..=spacing).rev() {
                    self.intermediate_r[x - 1][stage] = self.intermediate_r[x][stage];
                }
                self.last_sample_r[stage] = self.intermediate_r[0][stage];
            }

            match mode {
                1 => {}
                2 => {
                    input_l /= stage_input_gain;
                    input_r /= stage_input_gain;
                }
                3 => {
                    input_l = overshoot_l;
                    input_r = overshoot_r;
                }
                _ => {}
            }

            input_l *= ceiling_val;
            input_r *= ceiling_val;

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
// Maximizer dispatcher
// ---------------------------------------------------------------------------
#[derive(Default)]
pub struct Maximizer {
    pub maximizer_vintage: MaximizerVintage,
    pub maximizer_modern: MaximizerModern,
}

pub struct MaximizerParams {
    pub variant: u32,
    pub boost: f64,
    pub soften: f64,
    pub enhance: f64,
    pub ceiling: f64,
    pub mode: u32,
}

impl Maximizer {
    pub fn set_sample_rate(&mut self, sr: f64) {
        self.maximizer_vintage.set_sample_rate(sr);
        self.maximizer_modern.set_sample_rate(sr);
    }

    pub fn reset(&mut self) {
        self.maximizer_vintage.reset();
        self.maximizer_modern.reset();
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &MaximizerParams,
    ) {
        match params.variant {
            0 => self.maximizer_vintage.process_stereo(
                left,
                right,
                params.boost,
                params.soften,
                params.enhance,
                params.mode,
            ),
            1 => self.maximizer_modern.process_stereo(
                left,
                right,
                params.boost,
                params.ceiling,
                params.mode,
            ),
            _ => {}
        }
    }
}
