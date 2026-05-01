use std::f32::consts::PI;

#[derive(Debug, Clone, Copy)]
pub struct BiquadCoefficients {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BiquadState {
    pub x1: f32,
    pub x2: f32,
    pub y1: f32,
    pub y2: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Biquad {
    coeffs: BiquadCoefficients,
    state: BiquadState,
}

impl Default for Biquad {
    fn default() -> Self {
        Self {
            coeffs: BiquadCoefficients {
                b0: 1.0,
                b1: 0.0,
                b2: 0.0,
                a1: 0.0,
                a2: 0.0,
            },
            state: BiquadState::default(),
        }
    }
}

impl Biquad {
    pub fn set_coeffs(&mut self, coeffs: BiquadCoefficients) {
        self.coeffs = coeffs;
    }

    pub fn process_block(&mut self, block: &mut [f32]) {
        for x in block.iter_mut() {
            let y = self.coeffs.b0 * *x
                + self.coeffs.b1 * self.state.x1
                + self.coeffs.b2 * self.state.x2
                - self.coeffs.a1 * self.state.y1
                - self.coeffs.a2 * self.state.y2;
            self.state.x2 = self.state.x1;
            self.state.x1 = *x;
            self.state.y2 = self.state.y1;
            self.state.y1 = y;
            *x = y;
        }
    }
}

fn normalize(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> BiquadCoefficients {
    BiquadCoefficients {
        b0: b0 / a0,
        b1: b1 / a0,
        b2: b2 / a0,
        a1: a1 / a0,
        a2: a2 / a0,
    }
}

pub fn low_shelf(sample_rate: f32, frequency: f32, q: f32, gain_db: f32) -> BiquadCoefficients {
    let a = 10.0_f32.powf(gain_db / 40.0);
    let w0 = 2.0 * PI * frequency / sample_rate.max(1.0);
    let alpha = w0.sin() / (2.0 * q.max(1.0e-5));
    let cos_w0 = w0.cos();
    let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
    normalize(
        a * ((a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha),
        2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0),
        a * ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha),
        (a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha,
        -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0),
        (a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha,
    )
}

pub fn peaking(sample_rate: f32, frequency: f32, q: f32, gain_db: f32) -> BiquadCoefficients {
    let a = 10.0_f32.powf(gain_db / 40.0);
    let w0 = 2.0 * PI * frequency / sample_rate.max(1.0);
    let alpha = w0.sin() / (2.0 * q.max(1.0e-5));
    let cos_w0 = w0.cos();
    normalize(
        1.0 + alpha * a,
        -2.0 * cos_w0,
        1.0 - alpha * a,
        1.0 + alpha / a,
        -2.0 * cos_w0,
        1.0 - alpha / a,
    )
}

pub fn high_shelf(sample_rate: f32, frequency: f32, q: f32, gain_db: f32) -> BiquadCoefficients {
    let a = 10.0_f32.powf(gain_db / 40.0);
    let w0 = 2.0 * PI * frequency / sample_rate.max(1.0);
    let alpha = w0.sin() / (2.0 * q.max(1.0e-5));
    let cos_w0 = w0.cos();
    let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
    normalize(
        a * ((a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha),
        -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0),
        a * ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha),
        (a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha,
        2.0 * ((a - 1.0) - (a + 1.0) * cos_w0),
        (a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha,
    )
}

#[derive(Debug, Clone, Copy)]
pub struct OnePoleHighPass {
    alpha: f32,
    x1: f32,
    y1: f32,
}

impl Default for OnePoleHighPass {
    fn default() -> Self {
        Self {
            alpha: 1.0,
            x1: 0.0,
            y1: 0.0,
        }
    }
}

impl OnePoleHighPass {
    pub fn set_frequency(&mut self, sample_rate: f32, frequency: f32) {
        let c = 2.0 * PI * frequency / sample_rate.max(1.0);
        self.alpha = 1.0 / (c + 1.0);
    }

    pub fn process_block(&mut self, block: &mut [f32]) {
        for x in block.iter_mut() {
            let y = self.alpha * self.y1 + self.alpha * (*x - self.x1);
            self.x1 = *x;
            self.y1 = y;
            *x = y;
        }
    }
}
