/// Reverb reverb — ported from Airwindows Reverb.
///
/// Stereo reverb built from three allpass-like delay blocks,
/// cross-feedback between channels, vibrato predelay, and
/// input/output lowpass filters.
pub struct Reverb {
    // Delay buffers (maximum sizes from original C++ arrays)
    buf_il: Vec<f64>,
    buf_ir: Vec<f64>,
    buf_jl: Vec<f64>,
    buf_jr: Vec<f64>,
    buf_kl: Vec<f64>,
    buf_kr: Vec<f64>,
    buf_ll: Vec<f64>,
    buf_lr: Vec<f64>,
    buf_al: Vec<f64>,
    buf_ar: Vec<f64>,
    buf_bl: Vec<f64>,
    buf_br: Vec<f64>,
    buf_cl: Vec<f64>,
    buf_cr: Vec<f64>,
    buf_dl: Vec<f64>,
    buf_dr: Vec<f64>,
    buf_el: Vec<f64>,
    buf_er: Vec<f64>,
    buf_fl: Vec<f64>,
    buf_fr: Vec<f64>,
    buf_gl: Vec<f64>,
    buf_gr: Vec<f64>,
    buf_hl: Vec<f64>,
    buf_hr: Vec<f64>,
    buf_ml: Vec<f64>,
    buf_mr: Vec<f64>,

    // Write positions
    pos_i: usize,
    pos_j: usize,
    pos_k: usize,
    pos_l: usize,
    pos_a: usize,
    pos_b: usize,
    pos_c: usize,
    pos_d: usize,
    pos_e: usize,
    pos_f: usize,
    pos_g: usize,
    pos_h: usize,
    pos_m: usize,

    // Filter states
    iir_al: f64,
    iir_ar: f64,
    iir_bl: f64,
    iir_br: f64,

    // Feedback states
    feedback_al: f64,
    feedback_ar: f64,
    feedback_bl: f64,
    feedback_br: f64,
    feedback_cl: f64,
    feedback_cr: f64,
    feedback_dl: f64,
    feedback_dr: f64,

    // Cycle / interpolation
    sample_rate: f64,
    cycle: usize,
    cycle_end: usize,
    last_ref_l: [f64; 5],
    last_ref_r: [f64; 5],

    // Vibrato
    vib_m: f64,
    old_fpd: f64,

    // Dither PRNG
    fpd_l: u32,
    fpd_r: u32,
}

impl Default for Reverb {
    fn default() -> Self {
        Self {
            buf_il: vec![0.0; 6479],
            buf_ir: vec![0.0; 6479],
            buf_jl: vec![0.0; 3659],
            buf_jr: vec![0.0; 3659],
            buf_kl: vec![0.0; 1719],
            buf_kr: vec![0.0; 1719],
            buf_ll: vec![0.0; 679],
            buf_lr: vec![0.0; 679],
            buf_al: vec![0.0; 9699],
            buf_ar: vec![0.0; 9699],
            buf_bl: vec![0.0; 5999],
            buf_br: vec![0.0; 5999],
            buf_cl: vec![0.0; 2319],
            buf_cr: vec![0.0; 2319],
            buf_dl: vec![0.0; 939],
            buf_dr: vec![0.0; 939],
            buf_el: vec![0.0; 15219],
            buf_er: vec![0.0; 15219],
            buf_fl: vec![0.0; 8459],
            buf_fr: vec![0.0; 8459],
            buf_gl: vec![0.0; 4539],
            buf_gr: vec![0.0; 4539],
            buf_hl: vec![0.0; 3199],
            buf_hr: vec![0.0; 3199],
            buf_ml: vec![0.0; 3110],
            buf_mr: vec![0.0; 3110],

            pos_i: 0,
            pos_j: 0,
            pos_k: 0,
            pos_l: 0,
            pos_a: 0,
            pos_b: 0,
            pos_c: 0,
            pos_d: 0,
            pos_e: 0,
            pos_f: 0,
            pos_g: 0,
            pos_h: 0,
            pos_m: 0,

            iir_al: 0.0,
            iir_ar: 0.0,
            iir_bl: 0.0,
            iir_br: 0.0,

            feedback_al: 0.0,
            feedback_ar: 0.0,
            feedback_bl: 0.0,
            feedback_br: 0.0,
            feedback_cl: 0.0,
            feedback_cr: 0.0,
            feedback_dl: 0.0,
            feedback_dr: 0.0,

            sample_rate: 44100.0,
            cycle: 0,
            cycle_end: 1,
            last_ref_l: [0.0; 5],
            last_ref_r: [0.0; 5],

            vib_m: 3.0,
            old_fpd: 429496.7295,

            fpd_l: loop {
                let v: u32 = rand::random();
                if v >= 16386 {
                    break v;
                }
            },
            fpd_r: loop {
                let v: u32 = rand::random();
                if v >= 16386 {
                    break v;
                }
            },
        }
    }
}

impl Reverb {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        let overallscale = sample_rate / 44100.0;
        self.cycle_end = (overallscale.floor() as usize).clamp(1, 4);
        if self.cycle >= self.cycle_end {
            self.cycle = self.cycle_end - 1;
        }
    }

    fn read_delay(buf: &[f64], pos: usize) -> f64 {
        buf[pos]
    }

    fn write_delay(buf: &mut [f64], pos: usize, value: f64) {
        buf[pos] = value;
    }

    fn advance(pos: usize, len: usize) -> usize {
        let next = pos + 1;
        if next >= len { 0 } else { next }
    }

    fn read_interpolated(buf: &[f64], pos: f64, len: usize) -> f64 {
        let base = pos.floor() as usize % len;
        let frac = pos - pos.floor();
        let next = (base + 1) % len;
        buf[base] * (1.0 - frac) + buf[next] * frac
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        replace: f64,
        brightness: f64,
        detune: f64,
        bigness: f64,
        dry_wet: f64,
    ) {
        let regen = 0.0625 + (1.0 - replace) * 0.0625;
        let attenuate = (1.0 - (regen / 0.125)) * 1.333;
        let overallscale = self.sample_rate / 44100.0;
        let lowpass = (1.00001 - (1.0 - brightness)).powi(2) / overallscale.sqrt();
        let drift = detune.powi(3) * 0.001;
        let size = bigness * 1.77 + 0.1;
        let wet = 1.0 - (1.0 - dry_wet).powi(3);

        let delay_i = ((3407.0 * size) as usize).clamp(1, 6478);
        let delay_j = ((1823.0 * size) as usize).clamp(1, 3658);
        let delay_k = ((859.0 * size) as usize).clamp(1, 1718);
        let delay_l = ((331.0 * size) as usize).clamp(1, 678);
        let delay_a = ((4801.0 * size) as usize).clamp(1, 9698);
        let delay_b = ((2909.0 * size) as usize).clamp(1, 5998);
        let delay_c = ((1153.0 * size) as usize).clamp(1, 2318);
        let delay_d = ((461.0 * size) as usize).clamp(1, 938);
        let delay_e = ((7607.0 * size) as usize).clamp(1, 15218);
        let delay_f = ((4217.0 * size) as usize).clamp(1, 8458);
        let delay_g = ((2269.0 * size) as usize).clamp(1, 4538);
        let delay_h = ((1597.0 * size) as usize).clamp(1, 3198);
        let delay_m = 256usize;

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

            // Vibrato
            self.vib_m += self.old_fpd * drift;
            if self.vib_m > std::f64::consts::TAU {
                self.vib_m = 0.0;
                self.old_fpd = 0.429_496_729_5 + (self.fpd_l as f64 * 0.000_000_000_061_8);
            }

            // Predelay with vibrato
            Self::write_delay(&mut self.buf_ml, self.pos_m, input_l * attenuate);
            Self::write_delay(&mut self.buf_mr, self.pos_m, input_r * attenuate);
            self.pos_m = Self::advance(self.pos_m, delay_m + 1);

            let offset_ml = (self.vib_m.sin() + 1.0) * 127.0;
            let offset_mr = ((self.vib_m + std::f64::consts::FRAC_PI_2).sin() + 1.0) * 127.0;
            input_l =
                Self::read_interpolated(&self.buf_ml, self.pos_m as f64 + offset_ml, delay_m + 1);
            input_r =
                Self::read_interpolated(&self.buf_mr, self.pos_m as f64 + offset_mr, delay_m + 1);

            // Initial lowpass
            self.iir_al = self.iir_al * (1.0 - lowpass) + input_l * lowpass;
            input_l = self.iir_al;
            self.iir_ar = self.iir_ar * (1.0 - lowpass) + input_r * lowpass;
            input_r = self.iir_ar;

            self.cycle += 1;
            if self.cycle >= self.cycle_end {
                // Reverb network tick
                self.cycle = 0;

                // Block 1: I/J/K/L with cross-feedback
                Self::write_delay(
                    &mut self.buf_il,
                    self.pos_i,
                    input_l + self.feedback_ar * regen,
                );
                Self::write_delay(
                    &mut self.buf_jl,
                    self.pos_j,
                    input_l + self.feedback_br * regen,
                );
                Self::write_delay(
                    &mut self.buf_kl,
                    self.pos_k,
                    input_l + self.feedback_cr * regen,
                );
                Self::write_delay(
                    &mut self.buf_ll,
                    self.pos_l,
                    input_l + self.feedback_dr * regen,
                );
                Self::write_delay(
                    &mut self.buf_ir,
                    self.pos_i,
                    input_r + self.feedback_al * regen,
                );
                Self::write_delay(
                    &mut self.buf_jr,
                    self.pos_j,
                    input_r + self.feedback_bl * regen,
                );
                Self::write_delay(
                    &mut self.buf_kr,
                    self.pos_k,
                    input_r + self.feedback_cl * regen,
                );
                Self::write_delay(
                    &mut self.buf_lr,
                    self.pos_l,
                    input_r + self.feedback_dl * regen,
                );

                self.pos_i = Self::advance(self.pos_i, delay_i + 1);
                self.pos_j = Self::advance(self.pos_j, delay_j + 1);
                self.pos_k = Self::advance(self.pos_k, delay_k + 1);
                self.pos_l = Self::advance(self.pos_l, delay_l + 1);

                let out_il = Self::read_delay(&self.buf_il, self.pos_i);
                let out_jl = Self::read_delay(&self.buf_jl, self.pos_j);
                let out_kl = Self::read_delay(&self.buf_kl, self.pos_k);
                let out_ll = Self::read_delay(&self.buf_ll, self.pos_l);
                let out_ir = Self::read_delay(&self.buf_ir, self.pos_i);
                let out_jr = Self::read_delay(&self.buf_jr, self.pos_j);
                let out_kr = Self::read_delay(&self.buf_kr, self.pos_k);
                let out_lr = Self::read_delay(&self.buf_lr, self.pos_l);

                // Block 2: A/B/C/D
                Self::write_delay(
                    &mut self.buf_al,
                    self.pos_a,
                    out_il - (out_jl + out_kl + out_ll),
                );
                Self::write_delay(
                    &mut self.buf_bl,
                    self.pos_b,
                    out_jl - (out_il + out_kl + out_ll),
                );
                Self::write_delay(
                    &mut self.buf_cl,
                    self.pos_c,
                    out_kl - (out_il + out_jl + out_ll),
                );
                Self::write_delay(
                    &mut self.buf_dl,
                    self.pos_d,
                    out_ll - (out_il + out_jl + out_kl),
                );
                Self::write_delay(
                    &mut self.buf_ar,
                    self.pos_a,
                    out_ir - (out_jr + out_kr + out_lr),
                );
                Self::write_delay(
                    &mut self.buf_br,
                    self.pos_b,
                    out_jr - (out_ir + out_kr + out_lr),
                );
                Self::write_delay(
                    &mut self.buf_cr,
                    self.pos_c,
                    out_kr - (out_ir + out_jr + out_lr),
                );
                Self::write_delay(
                    &mut self.buf_dr,
                    self.pos_d,
                    out_lr - (out_ir + out_jr + out_kr),
                );

                self.pos_a = Self::advance(self.pos_a, delay_a + 1);
                self.pos_b = Self::advance(self.pos_b, delay_b + 1);
                self.pos_c = Self::advance(self.pos_c, delay_c + 1);
                self.pos_d = Self::advance(self.pos_d, delay_d + 1);

                let out_al = Self::read_delay(&self.buf_al, self.pos_a);
                let out_bl = Self::read_delay(&self.buf_bl, self.pos_b);
                let out_cl = Self::read_delay(&self.buf_cl, self.pos_c);
                let out_dl = Self::read_delay(&self.buf_dl, self.pos_d);
                let out_ar = Self::read_delay(&self.buf_ar, self.pos_a);
                let out_br = Self::read_delay(&self.buf_br, self.pos_b);
                let out_cr = Self::read_delay(&self.buf_cr, self.pos_c);
                let out_dr = Self::read_delay(&self.buf_dr, self.pos_d);

                // Block 3: E/F/G/H
                Self::write_delay(
                    &mut self.buf_el,
                    self.pos_e,
                    out_al - (out_bl + out_cl + out_dl),
                );
                Self::write_delay(
                    &mut self.buf_fl,
                    self.pos_f,
                    out_bl - (out_al + out_cl + out_dl),
                );
                Self::write_delay(
                    &mut self.buf_gl,
                    self.pos_g,
                    out_cl - (out_al + out_bl + out_dl),
                );
                Self::write_delay(
                    &mut self.buf_hl,
                    self.pos_h,
                    out_dl - (out_al + out_bl + out_cl),
                );
                Self::write_delay(
                    &mut self.buf_er,
                    self.pos_e,
                    out_ar - (out_br + out_cr + out_dr),
                );
                Self::write_delay(
                    &mut self.buf_fr,
                    self.pos_f,
                    out_br - (out_ar + out_cr + out_dr),
                );
                Self::write_delay(
                    &mut self.buf_gr,
                    self.pos_g,
                    out_cr - (out_ar + out_br + out_dr),
                );
                Self::write_delay(
                    &mut self.buf_hr,
                    self.pos_h,
                    out_dr - (out_ar + out_br + out_cr),
                );

                self.pos_e = Self::advance(self.pos_e, delay_e + 1);
                self.pos_f = Self::advance(self.pos_f, delay_f + 1);
                self.pos_g = Self::advance(self.pos_g, delay_g + 1);
                self.pos_h = Self::advance(self.pos_h, delay_h + 1);

                let out_el = Self::read_delay(&self.buf_el, self.pos_e);
                let out_fl = Self::read_delay(&self.buf_fl, self.pos_f);
                let out_gl = Self::read_delay(&self.buf_gl, self.pos_g);
                let out_hl = Self::read_delay(&self.buf_hl, self.pos_h);
                let out_er = Self::read_delay(&self.buf_er, self.pos_e);
                let out_fr = Self::read_delay(&self.buf_fr, self.pos_f);
                let out_gr = Self::read_delay(&self.buf_gr, self.pos_g);
                let out_hr = Self::read_delay(&self.buf_hr, self.pos_h);

                // Feedback
                self.feedback_al = out_el - (out_fl + out_gl + out_hl);
                self.feedback_bl = out_fl - (out_el + out_gl + out_hl);
                self.feedback_cl = out_gl - (out_el + out_fl + out_hl);
                self.feedback_dl = out_hl - (out_el + out_fl + out_gl);
                self.feedback_ar = out_er - (out_fr + out_gr + out_hr);
                self.feedback_br = out_fr - (out_er + out_gr + out_hr);
                self.feedback_cr = out_gr - (out_er + out_fr + out_hr);
                self.feedback_dr = out_hr - (out_er + out_fr + out_gr);

                let sum_l = (out_el + out_fl + out_gl + out_hl) / 8.0;
                let sum_r = (out_er + out_fr + out_gr + out_hr) / 8.0;

                // Interpolation setup for between-cycle samples
                match self.cycle_end {
                    4 => {
                        self.last_ref_l[0] = self.last_ref_l[4];
                        self.last_ref_l[2] = (self.last_ref_l[0] + sum_l) / 2.0;
                        self.last_ref_l[1] = (self.last_ref_l[0] + self.last_ref_l[2]) / 2.0;
                        self.last_ref_l[3] = (self.last_ref_l[2] + sum_l) / 2.0;
                        self.last_ref_l[4] = sum_l;
                        self.last_ref_r[0] = self.last_ref_r[4];
                        self.last_ref_r[2] = (self.last_ref_r[0] + sum_r) / 2.0;
                        self.last_ref_r[1] = (self.last_ref_r[0] + self.last_ref_r[2]) / 2.0;
                        self.last_ref_r[3] = (self.last_ref_r[2] + sum_r) / 2.0;
                        self.last_ref_r[4] = sum_r;
                    }
                    3 => {
                        self.last_ref_l[0] = self.last_ref_l[3];
                        self.last_ref_l[2] =
                            (self.last_ref_l[0] + self.last_ref_l[0] + sum_l) / 3.0;
                        self.last_ref_l[1] = (self.last_ref_l[0] + sum_l + sum_l) / 3.0;
                        self.last_ref_l[3] = sum_l;
                        self.last_ref_r[0] = self.last_ref_r[3];
                        self.last_ref_r[2] =
                            (self.last_ref_r[0] + self.last_ref_r[0] + sum_r) / 3.0;
                        self.last_ref_r[1] = (self.last_ref_r[0] + sum_r + sum_r) / 3.0;
                        self.last_ref_r[3] = sum_r;
                    }
                    2 => {
                        self.last_ref_l[0] = self.last_ref_l[2];
                        self.last_ref_l[1] = (self.last_ref_l[0] + sum_l) / 2.0;
                        self.last_ref_l[2] = sum_l;
                        self.last_ref_r[0] = self.last_ref_r[2];
                        self.last_ref_r[1] = (self.last_ref_r[0] + sum_r) / 2.0;
                        self.last_ref_r[2] = sum_r;
                    }
                    _ => {
                        self.last_ref_l[0] = sum_l;
                        self.last_ref_r[0] = sum_r;
                    }
                }

                input_l = self.last_ref_l[0];
                input_r = self.last_ref_r[0];
            } else {
                // Between reverb ticks: read from interpolation table
                input_l = self.last_ref_l[self.cycle];
                input_r = self.last_ref_r[self.cycle];
            }

            // End lowpass
            self.iir_bl = self.iir_bl * (1.0 - lowpass) + input_l * lowpass;
            input_l = self.iir_bl;
            self.iir_br = self.iir_br * (1.0 - lowpass) + input_r * lowpass;
            input_r = self.iir_br;

            // Dry/wet
            if wet < 1.0 {
                input_l = input_l * wet + dry_l * (1.0 - wet);
                input_r = input_r * wet + dry_r * (1.0 - wet);
            }

            // Dither
            let expon_l = if input_l == 0.0 {
                0
            } else {
                input_l.abs().log2().floor() as i32
            };
            self.fpd_l ^= self.fpd_l << 13;
            self.fpd_l ^= self.fpd_l >> 17;
            self.fpd_l ^= self.fpd_l << 5;
            input_l +=
                (self.fpd_l as f64 - 0x7fff_ffffu32 as f64) * 5.5e-36 * 2.0_f64.powi(expon_l + 62);

            let expon_r = if input_r == 0.0 {
                0
            } else {
                input_r.abs().log2().floor() as i32
            };
            self.fpd_r ^= self.fpd_r << 13;
            self.fpd_r ^= self.fpd_r >> 17;
            self.fpd_r ^= self.fpd_r << 5;
            input_r +=
                (self.fpd_r as f64 - 0x7fff_ffffu32 as f64) * 5.5e-36 * 2.0_f64.powi(expon_r + 62);

            *sample_l = input_l as f32;
            *sample_r = input_r as f32;
        }
    }
}
