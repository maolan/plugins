pub struct SingleEndedTriode {
    fpd_l: u32,
    fpd_r: u32,
    postsine: f64,
}

impl Default for SingleEndedTriode {
    fn default() -> Self {
        Self {
            fpd_l: rand::random(),
            fpd_r: rand::random(),
            postsine: 0.5f64.sin(),
        }
    }
}

impl SingleEndedTriode {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        triode_param: f64,
        class_ab: f64,
        class_b: f64,
        wet_param: f64,
    ) {
        let intensity = triode_param.powi(2) * 8.0;
        let triode = intensity;
        let intensity = intensity + 0.001;
        let softcrossover = class_ab.powi(3) / 8.0;
        let hardcrossover = class_b.powi(7) / 8.0;
        let wet = wet_param;

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

            if triode > 0.0 {
                input_l *= intensity;
                input_r *= intensity;
                input_l -= 0.5;
                input_r -= 0.5;

                let mut bridgerectifier = input_l.abs();
                if bridgerectifier > std::f64::consts::FRAC_PI_2 {
                    bridgerectifier = std::f64::consts::FRAC_PI_2;
                }
                bridgerectifier = bridgerectifier.sin();
                if input_l > 0.0 {
                    input_l = bridgerectifier;
                } else {
                    input_l = -bridgerectifier;
                }

                bridgerectifier = input_r.abs();
                if bridgerectifier > std::f64::consts::FRAC_PI_2 {
                    bridgerectifier = std::f64::consts::FRAC_PI_2;
                }
                bridgerectifier = bridgerectifier.sin();
                if input_r > 0.0 {
                    input_r = bridgerectifier;
                } else {
                    input_r = -bridgerectifier;
                }

                input_l += self.postsine;
                input_r += self.postsine;
                input_l /= intensity;
                input_r /= intensity;
            }

            if softcrossover > 0.0 {
                let mut bridgerectifier = input_l.abs();
                if bridgerectifier > 0.0 {
                    bridgerectifier -= softcrossover * (bridgerectifier + bridgerectifier.sqrt());
                }
                if bridgerectifier < 0.0 {
                    bridgerectifier = 0.0;
                }
                if input_l > 0.0 {
                    input_l = bridgerectifier;
                } else {
                    input_l = -bridgerectifier;
                }

                bridgerectifier = input_r.abs();
                if bridgerectifier > 0.0 {
                    bridgerectifier -= softcrossover * (bridgerectifier + bridgerectifier.sqrt());
                }
                if bridgerectifier < 0.0 {
                    bridgerectifier = 0.0;
                }
                if input_r > 0.0 {
                    input_r = bridgerectifier;
                } else {
                    input_r = -bridgerectifier;
                }
            }

            if hardcrossover > 0.0 {
                let mut bridgerectifier = input_l.abs();
                bridgerectifier -= hardcrossover;
                if bridgerectifier < 0.0 {
                    bridgerectifier = 0.0;
                }
                if input_l > 0.0 {
                    input_l = bridgerectifier;
                } else {
                    input_l = -bridgerectifier;
                }

                bridgerectifier = input_r.abs();
                bridgerectifier -= hardcrossover;
                if bridgerectifier < 0.0 {
                    bridgerectifier = 0.0;
                }
                if input_r > 0.0 {
                    input_r = bridgerectifier;
                } else {
                    input_r = -bridgerectifier;
                }
            }

            if wet != 1.0 {
                input_l = input_l * wet + dry_l * (1.0 - wet);
                input_r = input_r * wet + dry_r * (1.0 - wet);
            }

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
