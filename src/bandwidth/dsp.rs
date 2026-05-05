#[derive(Debug, Clone, Copy)]
pub struct BandwidthParams {
    pub low_width: f64,
    pub mid_width: f64,
    pub high_width: f64,
    pub mix: f64,
}

/// Biquad filter using Transposed Direct Form 2.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn lowpass(freq: f64, sample_rate: f64) -> Self {
        let wo = std::f64::consts::TAU * freq / sample_rate;
        let cosw = wo.cos();
        let sinw = wo.sin();
        let q = 0.5_f64.sqrt();
        let alpha = sinw / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: ((1.0 - cosw) * 0.5) / a0,
            b1: (1.0 - cosw) / a0,
            b2: ((1.0 - cosw) * 0.5) / a0,
            a1: (-2.0 * cosw) / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn highpass(freq: f64, sample_rate: f64) -> Self {
        let wo = std::f64::consts::TAU * freq / sample_rate;
        let cosw = wo.cos();
        let sinw = wo.sin();
        let q = 0.5_f64.sqrt();
        let alpha = sinw / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: ((1.0 + cosw) * 0.5) / a0,
            b1: (-(1.0 + cosw)) / a0,
            b2: ((1.0 + cosw) * 0.5) / a0,
            a1: (-2.0 * cosw) / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// 2nd-order allpass with the same poles as a Butterworth at `freq`.
    fn allpass(freq: f64, sample_rate: f64) -> Self {
        let wo = std::f64::consts::TAU * freq / sample_rate;
        let cosw = wo.cos();
        let sinw = wo.sin();
        let q = 0.5_f64.sqrt();
        let alpha = sinw / (2.0 * q);
        let a0 = 1.0 + alpha;
        let a1 = (-2.0 * cosw) / a0;
        let a2 = (1.0 - alpha) / a0;
        Self {
            b0: a2,
            b1: a1,
            b2: 1.0,
            a1,
            a2,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// High-shelf. `gain_db` can be positive (boost) or negative (cut).
    fn highshelf(freq: f64, gain_db: f64, sample_rate: f64) -> Self {
        let wo = std::f64::consts::TAU * freq / sample_rate;
        let cosw = wo.cos();
        let a = 10.0_f64.powf(gain_db / 40.0);
        let q = 0.5_f64.sqrt();
        let sqrt_a_2 = 2.0 * a.sqrt() * q;
        let a0 = (a + 1.0) - (a - 1.0) * cosw + sqrt_a_2;
        Self {
            b0: (a * ((a + 1.0) + (a - 1.0) * cosw + sqrt_a_2)) / a0,
            b1: (-2.0 * a * ((a - 1.0) + (a + 1.0) * cosw)) / a0,
            b2: (a * ((a + 1.0) + (a - 1.0) * cosw - sqrt_a_2)) / a0,
            a1: (2.0 * ((a - 1.0) - (a + 1.0) * cosw)) / a0,
            a2: ((a + 1.0) - (a - 1.0) * cosw - sqrt_a_2) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        let w = input - self.a1 * self.z1 - self.a2 * self.z2;
        let output = self.b0 * w + self.b1 * self.z1 + self.b2 * self.z2;
        self.z2 = self.z1;
        self.z1 = w;
        output
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// LR4 = two cascaded 2nd-order Butterworth filters.
#[derive(Debug, Clone, Copy)]
struct Lr4 {
    stage1: Biquad,
    stage2: Biquad,
}

impl Lr4 {
    fn lowpass(freq: f64, sample_rate: f64) -> Self {
        Self {
            stage1: Biquad::lowpass(freq, sample_rate),
            stage2: Biquad::lowpass(freq, sample_rate),
        }
    }

    fn highpass(freq: f64, sample_rate: f64) -> Self {
        Self {
            stage1: Biquad::highpass(freq, sample_rate),
            stage2: Biquad::highpass(freq, sample_rate),
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        self.stage2.process(self.stage1.process(input))
    }

    fn reset(&mut self) {
        self.stage1.reset();
        self.stage2.reset();
    }
}

/// LR4 allpass compensation filter (two cascaded allpass biquads).
#[derive(Debug, Clone, Copy)]
struct Lr4Allpass {
    stage1: Biquad,
    stage2: Biquad,
}

impl Lr4Allpass {
    fn new(freq: f64, sample_rate: f64) -> Self {
        Self {
            stage1: Biquad::allpass(freq, sample_rate),
            stage2: Biquad::allpass(freq, sample_rate),
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        self.stage2.process(self.stage1.process(input))
    }

    fn reset(&mut self) {
        self.stage1.reset();
        self.stage2.reset();
    }
}

/// 1st-order all-pass filter.
#[derive(Debug, Clone, Copy)]
struct Allpass1st {
    coeff: f64,
    z1: f64,
}

impl Allpass1st {
    fn new(coeff: f64) -> Self {
        Self { coeff, z1: 0.0 }
    }

    fn process(&mut self, input: f64) -> f64 {
        let output = self.coeff * input + self.z1;
        self.z1 = input - self.coeff * output;
        output
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
    }
}

use crate::common::modulated_delay::ModulatedDelay;

/// Subtle side-channel saturation for Mid and High bands.
#[inline]
fn side_sat(x: f64) -> f64 {
    (x * 1.5).tanh()
}

#[derive(Debug, Clone)]
pub struct Bandwidth {
    sample_rate: f64,

    // Crossovers
    lp_low: Lr4,
    hp_low: Lr4,
    lp_high: Lr4,
    hp_high: Lr4,

    // Allpass compensation for low band (matches 5kHz LR4 phase)
    ap_comp: Lr4Allpass,

    // Low band: HPF on side channel at 100Hz (single biquad = 12dB/oct)
    side_hpf: Biquad,

    // Mid band: 3-stage all-pass diffusion on L/R (different coeffs)
    ap_mid_l: [Allpass1st; 3],
    ap_mid_r: [Allpass1st; 3],

    // High band: 3-stage all-pass diffusion on L/R (different coeffs)
    ap_high_l: [Allpass1st; 3],
    ap_high_r: [Allpass1st; 3],

    // High band: variable high-shelf on side
    shelf: Biquad,

    // Global side-channel chorus for shimmer/width generation
    mod_delay: ModulatedDelay,
}

impl Bandwidth {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            lp_low: Lr4::lowpass(300.0, sample_rate),
            hp_low: Lr4::highpass(300.0, sample_rate),
            lp_high: Lr4::lowpass(5000.0, sample_rate),
            hp_high: Lr4::highpass(5000.0, sample_rate),
            ap_comp: Lr4Allpass::new(5000.0, sample_rate),
            side_hpf: Biquad::highpass(100.0, sample_rate),
            ap_mid_l: [
                Allpass1st::new(0.35),
                Allpass1st::new(-0.45),
                Allpass1st::new(0.55),
            ],
            ap_mid_r: [
                Allpass1st::new(0.40),
                Allpass1st::new(-0.50),
                Allpass1st::new(0.60),
            ],
            ap_high_l: [
                Allpass1st::new(0.30),
                Allpass1st::new(-0.40),
                Allpass1st::new(0.50),
            ],
            ap_high_r: [
                Allpass1st::new(0.35),
                Allpass1st::new(-0.45),
                Allpass1st::new(0.55),
            ],
            shelf: Biquad::highshelf(10000.0, 0.0, sample_rate),
            mod_delay: ModulatedDelay::new(sample_rate),
        }
    }

    pub fn reset(&mut self) {
        self.lp_low.reset();
        self.hp_low.reset();
        self.lp_high.reset();
        self.hp_high.reset();
        self.ap_comp.reset();
        self.side_hpf.reset();
        for ap in &mut self.ap_mid_l {
            ap.reset();
        }
        for ap in &mut self.ap_mid_r {
            ap.reset();
        }
        for ap in &mut self.ap_high_l {
            ap.reset();
        }
        for ap in &mut self.ap_high_r {
            ap.reset();
        }
        self.shelf.reset();
        self.mod_delay.reset();
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &BandwidthParams,
    ) {
        let low_side_gain = 1.0 + params.low_width;
        let mid_side_gain = 1.0 + params.mid_width * 0.5;
        let high_side_gain = 1.0 + params.high_width * 0.5;
        let mix = params.mix;

        // Constant-power M/S scaling: m² + s² = 2  (unitary transform)
        let low_mid_scale = (2.0 - low_side_gain * low_side_gain).sqrt().max(0.0);
        let mid_mid_scale = (2.0 - mid_side_gain * mid_side_gain).sqrt().max(0.0);
        let high_mid_scale = (2.0 - high_side_gain * high_side_gain).sqrt().max(0.0);

        // High-band shelf gain: 0 dB to +3 dB
        let shelf_gain_db = params.high_width * 3.0;
        self.shelf = Biquad::highshelf(10000.0, shelf_gain_db, self.sample_rate);

        let sqrt2 = std::f64::consts::SQRT_2;

        for (sample_l, sample_r) in left.iter_mut().zip(right.iter_mut()) {
            let input_l = *sample_l as f64;
            let input_r = *sample_r as f64;

            // --- Tri-band LR4 crossover ---
            let low_l = self.lp_low.process(input_l);
            let low_r = self.lp_low.process(input_r);

            let mid_high_l = self.hp_low.process(input_l);
            let mid_high_r = self.hp_low.process(input_r);

            let mid_l = self.lp_high.process(mid_high_l);
            let mid_r = self.lp_high.process(mid_high_r);

            let high_l = self.hp_high.process(mid_high_l);
            let high_r = self.hp_high.process(mid_high_r);

            // Compensate low band phase to match 5kHz crossover
            let low_l = self.ap_comp.process(low_l);
            let low_r = self.ap_comp.process(low_r);

            // --- Low band: M/S, HPF on side, gain ---
            let low_m = (low_l + low_r) / sqrt2;
            let mut low_s = (low_l - low_r) / sqrt2;
            low_s = self.side_hpf.process(low_s);
            low_s *= low_side_gain;
            let low_out_l = (low_m * low_mid_scale + low_s) / sqrt2;
            let low_out_r = (low_m * low_mid_scale - low_s) / sqrt2;

            // --- Mid band: 3-stage all-pass on L/R, then M/S gain + tanh saturation ---
            let mut mid_ap_l = mid_l;
            let mut mid_ap_r = mid_r;
            for ap in &mut self.ap_mid_l {
                mid_ap_l = ap.process(mid_ap_l);
            }
            for ap in &mut self.ap_mid_r {
                mid_ap_r = ap.process(mid_ap_r);
            }
            let mid_m = (mid_ap_l + mid_ap_r) / sqrt2;
            let mut mid_s = (mid_ap_l - mid_ap_r) / sqrt2;
            mid_s *= mid_side_gain;
            mid_s = side_sat(mid_s);
            let mid_out_l = (mid_m * mid_mid_scale + mid_s) / sqrt2;
            let mid_out_r = (mid_m * mid_mid_scale - mid_s) / sqrt2;

            // --- High band: 3-stage all-pass on L/R, then M/S, shelf, gain + tanh saturation ---
            let mut high_ap_l = high_l;
            let mut high_ap_r = high_r;
            for ap in &mut self.ap_high_l {
                high_ap_l = ap.process(high_ap_l);
            }
            for ap in &mut self.ap_high_r {
                high_ap_r = ap.process(high_ap_r);
            }
            let high_m = (high_ap_l + high_ap_r) / sqrt2;
            let mut high_s = (high_ap_l - high_ap_r) / sqrt2;
            high_s = self.shelf.process(high_s);
            high_s *= high_side_gain;
            high_s = side_sat(high_s);
            let high_out_l = (high_m * high_mid_scale + high_s) / sqrt2;
            let high_out_r = (high_m * high_mid_scale - high_s) / sqrt2;

            // --- Sum bands ---
            let wet_l = low_out_l + mid_out_l + high_out_l;
            let wet_r = low_out_r + mid_out_r + high_out_r;

            // --- Global side-channel chorus for shimmer ---
            let wet_m = (wet_l + wet_r) / sqrt2;
            let mut wet_s = (wet_l - wet_r) / sqrt2;
            wet_s = self.mod_delay.process(wet_s);
            let chorused_l = (wet_m + wet_s) / sqrt2;
            let chorused_r = (wet_m - wet_s) / sqrt2;

            // --- Dry/wet mix ---
            let out_l = input_l * (1.0 - mix) + chorused_l * mix;
            let out_r = input_r * (1.0 - mix) + chorused_r * mix;

            *sample_l = out_l as f32;
            *sample_r = out_r as f32;
        }
    }
}
