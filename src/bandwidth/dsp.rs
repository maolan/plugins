#[derive(Debug, Clone, Copy)]
pub struct BandwidthParams {
    pub low_width: f64,
    pub mid_width: f64,
    pub high_width: f64,
    pub depth: f64,
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

#[derive(Debug, Clone)]
pub struct Bandwidth {
    lp_low: Lr4,
    hp_low: Lr4,
    lp_high: Lr4,
    hp_high: Lr4,
}

impl Bandwidth {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            lp_low: Lr4::lowpass(300.0, sample_rate),
            hp_low: Lr4::highpass(300.0, sample_rate),
            lp_high: Lr4::lowpass(5000.0, sample_rate),
            hp_high: Lr4::highpass(5000.0, sample_rate),
        }
    }

    pub fn reset(&mut self) {
        self.lp_low.reset();
        self.hp_low.reset();
        self.lp_high.reset();
        self.hp_high.reset();
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &BandwidthParams,
    ) {
        let low_width = params.low_width;
        let mid_width = params.mid_width;
        let high_width = params.high_width;
        let mix = params.mix;
        let depth = params.depth;

        // Aggressive saturation drive to match InTheMix harmonics.
        let drive = depth * 2.0;
        // Makeup gain: InTheMix adds ~+1.0 to +1.5 dB.
        let makeup = 1.0 + depth * 0.35;

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

            // --- Saturation per band (independent L/R for width) ---
            let sat_low_l = Self::saturate(low_l, drive);
            let sat_low_r = Self::saturate(low_r, drive);
            let sat_mid_l = Self::saturate(mid_l, drive * 0.9);
            let sat_mid_r = Self::saturate(mid_r, drive * 0.9);
            let sat_high_l = Self::saturate(high_l, drive * 0.6);
            let sat_high_r = Self::saturate(high_r, drive * 0.6);

            // --- M/S width per band ---
            let (low_l, low_r) = Self::ms_width(sat_low_l, sat_low_r, low_width);
            let (mid_l, mid_r) = Self::ms_width(sat_mid_l, sat_mid_r, mid_width);
            let (high_l, high_r) = Self::ms_width(sat_high_l, sat_high_r, high_width);

            // --- Sum bands ---
            let wet_l = (low_l + mid_l + high_l) * makeup;
            let wet_r = (low_r + mid_r + high_r) * makeup;

            // --- Dry/wet mix ---
            let out_l = input_l * (1.0 - mix) + wet_l * mix;
            let out_r = input_r * (1.0 - mix) + wet_r * mix;

            *sample_l = out_l as f32;
            *sample_r = out_r as f32;
        }
    }

    /// M/S width: boost the side signal.
    /// On nearly-mono material the side is tiny, so we also add a small
    /// amount of the saturated signal to the side to ensure width exists.
    fn ms_width(left: f64, right: f64, width: f64) -> (f64, f64) {
        let mid = (left + right) * 0.5;
        let side = (left - right) * 0.5;
        // Boost side.  At width=1 the side is doubled.
        let side_boosted = side * (1.0 + width);
        (mid + side_boosted, mid - side_boosted)
    }

    /// Very aggressive asymmetric hard-clipper.
    /// The signal is driven hard into a low threshold with asymmetric
    /// positive/negative clipping.  This produces strong even and odd
    /// harmonics comparable to InTheMix Bandwidth.
    fn saturate(x: f64, drive: f64) -> f64 {
        if drive < 0.001 {
            return x;
        }
        let input = x * (1.0 + drive * 10.0);
        // Strong bias for even harmonics.
        let bias = 0.1 * drive;
        let driven = input + bias;
        // Very low threshold = heavy distortion.
        let pos_thresh = 0.15;
        let neg_thresh = -0.25;
        let clipped = if driven > pos_thresh {
            pos_thresh
        } else if driven < neg_thresh {
            neg_thresh
        } else {
            driven
        };
        // Remove bias and apply heavy makeup.
        (clipped - bias) * (1.0 + drive * 3.0)
    }
}
