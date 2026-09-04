const FORMANTS: [[f32; 3]; 5] = [
    [600.0, 1040.0, 2250.0], // A
    [400.0, 1620.0, 2400.0], // E
    [250.0, 1750.0, 2600.0], // I
    [400.0, 750.0, 2400.0],  // O
    [350.0, 600.0, 2400.0],  // U
];

#[derive(Debug, Clone, Copy)]
struct Biquad {
    a0: f32,
    a1: f32,
    a2: f32,
    b1: f32,
    b2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self {
            a0: 0.0,
            a1: 0.0,
            a2: 0.0,
            b1: 0.0,
            b2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
}

pub struct FormantParams {
    pub vowel: f64,
    pub sharpness: f64,
    pub output_gain_db: f64,
}

pub struct Formant {
    filters_l: [Biquad; 3],
    filters_r: [Biquad; 3],
    sample_rate: f32,
}

impl Default for Formant {
    fn default() -> Self {
        Self {
            filters_l: [Biquad::default(); 3],
            filters_r: [Biquad::default(); 3],
            sample_rate: 48_000.0,
        }
    }
}

impl Formant {
    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate as f32;
    }

    pub fn reset(&mut self) {
        self.filters_l = [Biquad::default(); 3];
        self.filters_r = [Biquad::default(); 3];
    }

    fn setup_bandpass(filter: &mut Biquad, freq: f32, q: f32, sr: f32) {
        let omega = 2.0 * std::f32::consts::PI * freq / sr;
        let sn = omega.sin();
        let cs = omega.cos();
        let alpha = sn / (2.0 * q);
        let a0_inv = 1.0 / (1.0 + alpha);

        filter.a0 = alpha * a0_inv;
        filter.a1 = 0.0;
        filter.a2 = -alpha * a0_inv;
        filter.b1 = -2.0 * cs * a0_inv;
        filter.b2 = (1.0 - alpha) * a0_inv;
    }

    #[inline]
    fn process_biquad(filter: &mut Biquad, input: f32) -> f32 {
        let mut out = filter.a0 * input + filter.a1 * filter.x1 + filter.a2 * filter.x2
            - filter.b1 * filter.y1
            - filter.b2 * filter.y2;
        if out.abs() < 1e-15 {
            out = 0.0;
        }
        filter.x2 = filter.x1;
        filter.x1 = input;
        filter.y2 = filter.y1;
        filter.y1 = out;
        out
    }

    fn update_filters(&mut self, vowel: f32, sharpness: f32) {
        let vowel_ptr = vowel.clamp(0.0, 4.0);
        let mut v1 = vowel_ptr.floor() as usize;
        if v1 > 3 {
            v1 = 3;
        }
        let v2 = v1 + 1;
        let blend = vowel_ptr - v1 as f32;

        for (filter_l, (&freq1, &freq2)) in self
            .filters_l
            .iter_mut()
            .zip(FORMANTS[v1].iter().zip(FORMANTS[v2].iter()))
        {
            let freq = freq1 + blend * (freq2 - freq1);
            Self::setup_bandpass(filter_l, freq, sharpness, self.sample_rate);
        }
        self.filters_r = self.filters_l;
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32], params: &FormantParams) {
        let frames = left.len().min(right.len());
        let vowel = params.vowel.clamp(0.0, 4.0) as f32;
        let sharpness = params.sharpness.clamp(2.0, 40.0) as f32;
        let gain = 10.0f32.powf(params.output_gain_db.clamp(-60.0, 20.0) as f32 / 20.0);

        self.update_filters(vowel, sharpness);

        for i in 0..frames {
            let in_l = left[i];
            let in_r = right[i];
            let mut out_l = 0.0f32;
            let mut out_r = 0.0f32;
            for j in 0..3 {
                out_l += Self::process_biquad(&mut self.filters_l[j], in_l);
                out_r += Self::process_biquad(&mut self.filters_r[j], in_r);
            }
            left[i] = out_l * gain;
            right[i] = out_r * gain;
        }
    }
}
