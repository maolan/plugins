pub struct Monitoring {
    // NJAD
    byn_l: [f64; 13],
    byn_r: [f64; 13],
    noise_shaping_l: f64,
    noise_shaping_r: f64,

    // PeaksOnly allpasses (also used by Cans)
    a_l: Vec<f64>,
    a_r: Vec<f64>,
    b_l: Vec<f64>,
    b_r: Vec<f64>,
    c_l: Vec<f64>,
    c_r: Vec<f64>,
    d_l: Vec<f64>,
    d_r: Vec<f64>,
    ax: usize,
    bx: usize,
    cx: usize,
    dx: usize,

    // SlewOnly
    last_sample_l: f64,
    last_sample_r: f64,

    // SubsOnly + Cans
    iir_l: [f64; 26],
    iir_r: [f64; 26],

    // Bandpasses
    biquad_l: [f64; 11],
    biquad_r: [f64; 11],

    fpd_l: u32,
    fpd_r: u32,
}

impl Default for Monitoring {
    fn default() -> Self {
        let byn = [
            1000.0, 301.0, 176.0, 125.0, 97.0, 79.0, 67.0, 58.0, 51.0, 46.0, 1000.0, 0.0, 0.0,
        ];
        Self {
            byn_l: byn,
            byn_r: byn,
            noise_shaping_l: 0.0,
            noise_shaping_r: 0.0,
            a_l: vec![0.0; 1503],
            a_r: vec![0.0; 1503],
            b_l: vec![0.0; 1503],
            b_r: vec![0.0; 1503],
            c_l: vec![0.0; 1503],
            c_r: vec![0.0; 1503],
            d_l: vec![0.0; 1503],
            d_r: vec![0.0; 1503],
            ax: 1,
            bx: 1,
            cx: 1,
            dx: 1,
            last_sample_l: 0.0,
            last_sample_r: 0.0,
            iir_l: [0.0; 26],
            iir_r: [0.0; 26],
            biquad_l: [0.0; 11],
            biquad_r: [0.0; 11],
            fpd_l: rand::random(),
            fpd_r: rand::random(),
        }
    }
}

impl Monitoring {
    pub fn reset(&mut self) {
        self.byn_l = [
            1000.0, 301.0, 176.0, 125.0, 97.0, 79.0, 67.0, 58.0, 51.0, 46.0, 1000.0, 0.0, 0.0,
        ];
        self.byn_r = [
            1000.0, 301.0, 176.0, 125.0, 97.0, 79.0, 67.0, 58.0, 51.0, 46.0, 1000.0, 0.0, 0.0,
        ];
        self.noise_shaping_l = 0.0;
        self.noise_shaping_r = 0.0;
        self.a_l.fill(0.0);
        self.a_r.fill(0.0);
        self.b_l.fill(0.0);
        self.b_r.fill(0.0);
        self.c_l.fill(0.0);
        self.c_r.fill(0.0);
        self.d_l.fill(0.0);
        self.d_r.fill(0.0);
        self.ax = 1;
        self.bx = 1;
        self.cx = 1;
        self.dx = 1;
        self.last_sample_l = 0.0;
        self.last_sample_r = 0.0;
        self.iir_l.fill(0.0);
        self.iir_r.fill(0.0);
        self.biquad_l.fill(0.0);
        self.biquad_r.fill(0.0);
        self.fpd_l = rand::random();
        self.fpd_r = rand::random();
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        mode: u32,
        sample_rate: f64,
    ) {
        let processing = mode.min(16) as usize;
        let overallscale = sample_rate / 44100.0;

        let am = (149.0 * overallscale) as usize;
        let bm = (179.0 * overallscale) as usize;
        let cm = (191.0 * overallscale) as usize;
        let dm = (223.0 * overallscale) as usize;

        let (freq_l, q_l) = if processing == 7 {
            (0.0385 / overallscale, 0.0825)
        } else if processing == 11 {
            (0.1245 / overallscale, 0.46)
        } else {
            (0.0375 / overallscale, 0.1575)
        };
        self.setup_biquad(freq_l, q_l, true);

        let (freq_r, q_r) = if processing == 7 {
            (0.0385 / overallscale, 0.0825)
        } else if processing == 11 {
            (0.1245 / overallscale, 0.46)
        } else {
            (0.0375 / overallscale, 0.1575)
        };
        self.setup_biquad(freq_r, q_r, false);

        let scale = if processing == 1 { 32768.0 } else { 8388608.0 };

        for (sample_l, sample_r) in left.iter_mut().zip(right.iter_mut()) {
            let mut input_l = *sample_l as f64;
            let mut input_r = *sample_r as f64;

            if input_l.abs() < 1.18e-23 {
                input_l = self.fpd_l as f64 * 1.18e-17;
            }
            if input_r.abs() < 1.18e-23 {
                input_r = self.fpd_r as f64 * 1.18e-17;
            }

            match processing {
                0 | 1 => {
                    // NJAD dither handled after match
                }
                2 => {
                    // PeaksOnly
                    (input_l, input_r) = Self::peaks_allpass_stereo(
                        &mut self.a_l,
                        &mut self.a_r,
                        &mut self.ax,
                        am,
                        input_l,
                        input_r,
                    );
                    (input_l, input_r) = Self::peaks_allpass_stereo(
                        &mut self.b_l,
                        &mut self.b_r,
                        &mut self.bx,
                        bm,
                        input_l,
                        input_r,
                    );
                    (input_l, input_r) = Self::peaks_allpass_stereo(
                        &mut self.c_l,
                        &mut self.c_r,
                        &mut self.cx,
                        cm,
                        input_l,
                        input_r,
                    );
                    (input_l, input_r) = Self::peaks_allpass_stereo(
                        &mut self.d_l,
                        &mut self.d_r,
                        &mut self.dx,
                        dm,
                        input_l,
                        input_r,
                    );
                    input_l *= 0.63679;
                    input_r *= 0.63679;
                }
                3 => {
                    let trim = std::f64::consts::LN_10;
                    let mut slew = (input_l - self.last_sample_l) * trim;
                    self.last_sample_l = input_l;
                    input_l = slew.clamp(-1.0, 1.0);
                    slew = (input_r - self.last_sample_r) * trim;
                    self.last_sample_r = input_r;
                    input_r = slew.clamp(-1.0, 1.0);
                }
                4 => {
                    let iir_amount = (2250.0 / 44100.0) / overallscale;
                    input_l = Self::subs_only(&mut self.iir_l, iir_amount, input_l);
                    input_r = Self::subs_only(&mut self.iir_r, iir_amount, input_r);
                }
                5 | 6 => {
                    let mut mid = input_l + input_r;
                    let mut side = input_l - input_r;
                    if processing == 5 {
                        side = 0.0;
                    } else {
                        mid = 0.0;
                    }
                    input_l = (mid + side) / 2.0;
                    input_r = (mid - side) / 2.0;
                }
                7..=11 => {
                    if processing == 9 {
                        input_r = (input_l + input_r) * 0.5;
                        input_l = 0.0;
                    }
                    if processing == 10 {
                        input_l = (input_l + input_r) * 0.5;
                        input_r = 0.0;
                    }
                    if processing == 11 {
                        let m = (input_l + input_r) * 0.5;
                        input_l = m;
                        input_r = m;
                    }
                    input_l = input_l.sin();
                    input_r = input_r.sin();
                    input_l = self.run_biquad(input_l, true);
                    input_r = self.run_biquad(input_r, false);
                    input_l = input_l.clamp(-1.0, 1.0);
                    input_r = input_r.clamp(-1.0, 1.0);
                    input_l = input_l.asin();
                    input_r = input_r.asin();
                }
                12..=15 => {
                    let gain_stages = [0.855, 0.748, 0.713, 0.680];
                    let crossfeed_gain = [0.125, 0.25, 0.30, 0.35];
                    let idx = processing - 12;
                    input_l *= gain_stages[idx];
                    input_r *= gain_stages[idx];

                    input_l = input_l.sin();
                    input_r = input_r.sin();

                    let mut dry_l = input_l;
                    let mut dry_r = input_r;

                    let bass = (processing * processing) as f64 * 0.00001 / overallscale;

                    let mut mid = input_l + input_r;
                    let mut side = input_l - input_r;
                    self.iir_l[0] = self.iir_l[0] * (1.0 - bass * 0.618) + side * bass * 0.618;
                    side -= self.iir_l[0];
                    input_l = (mid + side) / 2.0;
                    input_r = (mid - side) / 2.0;

                    (input_l, input_r) = Self::cans_allpass_stereo(
                        &mut self.a_l,
                        &mut self.a_r,
                        &mut self.ax,
                        am,
                        input_l,
                        input_r,
                    );

                    input_l *= crossfeed_gain[idx];
                    input_r *= crossfeed_gain[idx];

                    dry_l += input_r;
                    dry_r += input_l;

                    (input_l, input_r) = Self::cans_allpass_stereo(
                        &mut self.d_l,
                        &mut self.d_r,
                        &mut self.dx,
                        dm,
                        input_l,
                        input_r,
                    );

                    input_l *= 0.25;
                    input_r *= 0.25;

                    dry_l += input_r;
                    dry_r += input_l;

                    input_l = dry_l;
                    input_r = dry_r;

                    mid = input_l + input_r;
                    side = input_l - input_r;
                    self.iir_r[0] = self.iir_r[0] * (1.0 - bass) + side * bass;
                    side -= self.iir_r[0];
                    input_l = (mid + side) / 2.0;
                    input_r = (mid - side) / 2.0;

                    input_l = input_l.clamp(-1.0, 1.0);
                    input_r = input_r.clamp(-1.0, 1.0);
                    input_l = input_l.asin();
                    input_r = input_r.asin();
                }
                16 => {
                    let mid = (input_l + input_r) * 0.5;
                    input_l = -mid;
                    input_r = mid;
                }
                _ => {}
            }

            if processing <= 1 {
                input_l *= scale;
                input_r *= scale;
                let output_l = Self::njad(input_l, &mut self.byn_l, &mut self.noise_shaping_l);
                let output_r = Self::njad(input_r, &mut self.byn_r, &mut self.noise_shaping_r);
                input_l = output_l / scale;
                input_r = output_r / scale;
                input_l = input_l.clamp(-1.0, 1.0);
                input_r = input_r.clamp(-1.0, 1.0);
            }

            *sample_l = input_l as f32;
            *sample_r = input_r as f32;
        }
    }

    fn peaks_allpass_stereo(
        buf_l: &mut [f64],
        buf_r: &mut [f64],
        idx: &mut usize,
        size: usize,
        l: f64,
        r: f64,
    ) -> (f64, f64) {
        let mut sample_l = l.clamp(-1.0, 1.0).asin();
        let mut sample_r = r.clamp(-1.0, 1.0).asin();

        let mut temp = idx.saturating_sub(1);
        if temp == 0 || temp > size {
            temp = size;
        }

        sample_l -= buf_l[temp] * 0.5;
        sample_r -= buf_r[temp] * 0.5;
        buf_l[*idx] = sample_l;
        buf_r[*idx] = sample_r;
        sample_l *= 0.5;
        sample_r *= 0.5;

        *idx = idx.saturating_sub(1);
        if *idx == 0 || *idx > size {
            *idx = size;
        }

        sample_l += buf_l[*idx];
        sample_r += buf_r[*idx];

        (
            sample_l.clamp(-1.0, 1.0).asin(),
            sample_r.clamp(-1.0, 1.0).asin(),
        )
    }

    fn cans_allpass_stereo(
        buf_l: &mut [f64],
        buf_r: &mut [f64],
        idx: &mut usize,
        size: usize,
        l: f64,
        r: f64,
    ) -> (f64, f64) {
        let mut temp = idx.saturating_sub(1);
        if temp == 0 || temp > size {
            temp = size;
        }

        let mut sample_l = l - buf_l[temp] * 0.5;
        let mut sample_r = r - buf_r[temp] * 0.5;
        buf_l[*idx] = sample_l;
        buf_r[*idx] = sample_r;
        sample_l *= 0.5;
        sample_r *= 0.5;

        *idx = idx.saturating_sub(1);
        if *idx == 0 || *idx > size {
            *idx = size;
        }

        sample_l += buf_l[*idx] * 0.5;
        sample_r += buf_r[*idx] * 0.5;

        if *idx == size {
            sample_l += buf_l[0] * 0.5;
            sample_r += buf_r[0] * 0.5;
        } else {
            sample_l += buf_l[*idx + 1] * 0.5;
            sample_r += buf_r[*idx + 1] * 0.5;
        }

        (sample_l, sample_r)
    }

    fn subs_only(iir: &mut [f64; 26], amount: f64, mut input: f64) -> f64 {
        let mut gain = 1.42;
        for item in iir.iter_mut().take(25) {
            *item = *item * (1.0 - amount) + input * amount;
            input = *item;
            input *= gain;
            gain = ((gain - 1.0) * 0.75) + 1.0;
            input = input.clamp(-1.0, 1.0);
        }
        iir[25] = iir[25] * (1.0 - amount) + input * amount;
        input = iir[25];
        input.clamp(-1.0, 1.0)
    }

    fn setup_biquad(&mut self, freq: f64, q: f64, is_left: bool) {
        let k = (std::f64::consts::PI * freq).tan();
        let norm = 1.0 / (1.0 + k / q + k * k);
        let b2 = k / q * norm;
        let b4 = -b2;
        let b5 = 2.0 * (k * k - 1.0) * norm;
        let b6 = (1.0 - k / q + k * k) * norm;
        if is_left {
            self.biquad_l[2] = b2;
            self.biquad_l[4] = b4;
            self.biquad_l[5] = b5;
            self.biquad_l[6] = b6;
        } else {
            self.biquad_r[2] = b2;
            self.biquad_r[4] = b4;
            self.biquad_r[5] = b5;
            self.biquad_r[6] = b6;
        }
    }

    fn run_biquad(&mut self, input: f64, is_left: bool) -> f64 {
        if is_left {
            let temp = input * self.biquad_l[2] + self.biquad_l[7];
            self.biquad_l[7] = -temp * self.biquad_l[5] + self.biquad_l[8];
            self.biquad_l[8] = input * self.biquad_l[4] - temp * self.biquad_l[6];
            temp
        } else {
            let temp = input * self.biquad_r[2] + self.biquad_r[7];
            self.biquad_r[7] = -temp * self.biquad_r[5] + self.biquad_r[8];
            self.biquad_r[8] = input * self.biquad_r[4] - temp * self.biquad_r[6];
            temp
        }
    }

    fn njad(input: f64, byn: &mut [f64; 13], noise_shaping: &mut f64) -> f64 {
        let dry = input;
        let sample = input - *noise_shaping;

        let mut benfordize = sample.floor();
        while benfordize >= 1.0 {
            benfordize /= 10.0;
        }
        while benfordize < 1.0 && benfordize > 0.000_000_1 {
            benfordize *= 10.0;
        }
        let hotbin_a = benfordize.floor() as usize;

        let mut total_a = 0.0;
        let mut cutbins = false;
        if hotbin_a > 0 && hotbin_a < 10 {
            byn[hotbin_a] += 1.0;
            if byn[hotbin_a] > 982.0 {
                cutbins = true;
            }
            total_a += 301.0 - byn[1];
            total_a += 176.0 - byn[2];
            total_a += 125.0 - byn[3];
            total_a += 97.0 - byn[4];
            total_a += 79.0 - byn[5];
            total_a += 67.0 - byn[6];
            total_a += 58.0 - byn[7];
            total_a += 51.0 - byn[8];
            total_a += 46.0 - byn[9];
            byn[hotbin_a] -= 1.0;
        }

        let mut benfordize = sample.ceil();
        while benfordize >= 1.0 {
            benfordize /= 10.0;
        }
        while benfordize < 1.0 && benfordize > 0.000_000_1 {
            benfordize *= 10.0;
        }
        let hotbin_b = benfordize.floor() as usize;

        let mut total_b = 0.0;
        if hotbin_b > 0 && hotbin_b < 10 {
            byn[hotbin_b] += 1.0;
            if byn[hotbin_b] > 982.0 {
                cutbins = true;
            }
            total_b += 301.0 - byn[1];
            total_b += 176.0 - byn[2];
            total_b += 125.0 - byn[3];
            total_b += 97.0 - byn[4];
            total_b += 79.0 - byn[5];
            total_b += 67.0 - byn[6];
            total_b += 58.0 - byn[7];
            total_b += 51.0 - byn[8];
            total_b += 46.0 - byn[9];
            byn[hotbin_b] -= 1.0;
        }

        let output = if total_a < total_b {
            byn[hotbin_a] += 1.0;
            sample.floor()
        } else {
            byn[hotbin_b] += 1.0;
            sample.floor() + 1.0
        };

        if cutbins {
            for item in byn.iter_mut().take(11).skip(1) {
                *item *= 0.99;
            }
        }

        *noise_shaping += output - dry;
        *noise_shaping = noise_shaping.clamp(-sample.abs(), sample.abs());
        output
    }
}
