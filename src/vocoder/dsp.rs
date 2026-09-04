const BANDS: usize = 24;
const RELEASE: f32 = 0.992;
const Q: f32 = 14.0;

#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    w1: f32,
    w2: f32,
    band_gain: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self {
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            w1: 0.0,
            w2: 0.0,
            band_gain: 0.0,
        }
    }
}

pub struct VocoderParams {
    pub spectral_shift: f64,
    pub dry_wet: f64,
}

pub struct Vocoder {
    sample_rate: f32,
    last_shift: f32,
    filters_l: [Biquad; BANDS],
    filters_r: [Biquad; BANDS],
    env_l: [f32; BANDS],
    env_r: [f32; BANDS],
}

impl Default for Vocoder {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            last_shift: -1.0,
            filters_l: [Biquad::default(); BANDS],
            filters_r: [Biquad::default(); BANDS],
            env_l: [0.0; BANDS],
            env_r: [0.0; BANDS],
        }
    }
}

impl Vocoder {
    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate as f32;
    }

    pub fn reset(&mut self) {
        self.last_shift = -1.0;
        self.filters_l = [Biquad::default(); BANDS];
        self.filters_r = [Biquad::default(); BANDS];
        self.env_l = [0.0; BANDS];
        self.env_r = [0.0; BANDS];
    }

    fn setup_filter(filter: &mut Biquad, freq: f32, sr: f32, band_index: usize) {
        let mut f = freq;
        if f > sr * 0.45 {
            f = sr * 0.45;
        }
        if f < 40.0 {
            f = 40.0;
        }

        let omega = 2.0 * std::f32::consts::PI * f / sr;
        let sn = omega.sin();
        let cs = omega.cos();
        let alpha = sn / (2.0 * Q);

        let a0 = 1.0 + alpha;
        filter.b0 = alpha / a0;
        filter.b1 = 0.0;
        filter.b2 = -alpha / a0;
        filter.a1 = (-2.0 * cs) / a0;
        filter.a2 = (1.0 - alpha) / a0;

        let dampening = 1.0 / (1.0 + band_index as f32 * 0.1);
        filter.band_gain = 12.0 * dampening;
    }

    fn update_filters(&mut self, shift: f32) {
        if (shift - self.last_shift).abs() < f32::EPSILON {
            return;
        }
        for i in 0..BANDS {
            let base_freq = 60.0 * 1.25_f32.powf(i as f32 * 1.4);
            let freq = base_freq * shift;
            Self::setup_filter(&mut self.filters_l[i], freq, self.sample_rate, i);
            self.filters_r[i] = self.filters_l[i];
        }
        self.last_shift = shift;
    }

    #[inline]
    fn fast_limit(x: f32) -> f32 {
        if x > 1.0 {
            1.0
        } else if x < -1.0 {
            -1.0
        } else {
            x - (0.333_333_3 * x * x * x)
        }
    }

    #[inline]
    fn process_sample(filters: &mut [Biquad; BANDS], env: &mut [f32; BANDS], input: f32) -> f32 {
        let mut vocoded_sum = 0.0f32;
        for b in 0..BANDS {
            let f = &mut filters[b];
            let w0 = input - f.a1 * f.w1 - f.a2 * f.w2;
            let band_sample = f.b0 * w0 + f.b1 * f.w1 + f.b2 * f.w2;
            f.w2 = f.w1;
            f.w1 = w0;

            let abs_s = band_sample.abs();
            if abs_s > env[b] {
                env[b] = abs_s;
            } else {
                env[b] *= RELEASE;
            }

            vocoded_sum += band_sample * env[b] * f.band_gain;
        }
        vocoded_sum
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32], params: &VocoderParams) {
        let frames = left.len().min(right.len());
        let shift = params.spectral_shift.clamp(0.5, 4.0) as f32;
        let wet = params.dry_wet.clamp(0.0, 1.0) as f32;
        let dry = 1.0 - wet;

        self.update_filters(shift);

        for i in 0..frames {
            let in_l = left[i];
            let in_r = right[i];

            let wet_l = Self::fast_limit(Self::process_sample(
                &mut self.filters_l,
                &mut self.env_l,
                in_l,
            ));
            let wet_r = Self::fast_limit(Self::process_sample(
                &mut self.filters_r,
                &mut self.env_r,
                in_r,
            ));

            left[i] = (in_l * dry) + (wet_l * wet);
            right[i] = (in_r * dry) + (wet_r * wet);
        }
    }

    pub fn process_mono(&mut self, buffer: &mut [f32], params: &VocoderParams) {
        let shift = params.spectral_shift.clamp(0.5, 4.0) as f32;
        let wet = params.dry_wet.clamp(0.0, 1.0) as f32;
        let dry = 1.0 - wet;

        self.update_filters(shift);

        for sample in buffer.iter_mut() {
            let input = *sample;
            let wet_out = Self::fast_limit(Self::process_sample(
                &mut self.filters_l,
                &mut self.env_l,
                input,
            ));
            *sample = (input * dry) + (wet_out * wet);
        }
    }
}
