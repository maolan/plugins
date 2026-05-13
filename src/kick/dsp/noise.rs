//! Noise generator with white, pink, and brownian noise.

use super::envelope::Envelope;
use super::filter::{FilterType, SvfFilter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NoiseType {
    White = 0,
    Pink = 1,
    Brownian = 2,
}

impl NoiseType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => NoiseType::Pink,
            2 => NoiseType::Brownian,
            _ => NoiseType::White,
        }
    }
}

/// xoshiro128+ PRNG.
#[derive(Clone)]
pub struct Rng {
    s: [u32; 4],
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut s = [0u32; 4];
        let mut z = seed.wrapping_add(0x9e3779b97f4a7c15);
        for item in &mut s {
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            z = z ^ (z >> 31);
            *item = z as u32;
        }
        Self { s }
    }

    #[inline]
    fn next_u32(&mut self) -> u32 {
        let result = self.s[0].wrapping_add(self.s[3]);
        let t = self.s[1] << 9;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(11);
        result
    }

    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Paul Kellet's economy pinking filter.
#[derive(Clone)]
pub struct PinkNoise {
    b0: f32,
    b1: f32,
    b2: f32,
    b3: f32,
    b4: f32,
    b5: f32,
    b6: f32,
}

impl Default for PinkNoise {
    fn default() -> Self {
        Self {
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            b3: 0.0,
            b4: 0.0,
            b5: 0.0,
            b6: 0.0,
        }
    }
}

impl PinkNoise {
    #[inline]
    pub fn next(&mut self, white: f32) -> f32 {
        self.b0 = 0.99886 * self.b0 + white * 0.0555179;
        self.b1 = 0.99332 * self.b1 + white * 0.0750759;
        self.b2 = 0.96900 * self.b2 + white * 0.153_852;
        self.b3 = 0.86650 * self.b3 + white * 0.3104856;
        self.b4 = 0.55000 * self.b4 + white * 0.5329522;
        self.b5 = -0.7616 * self.b5 - white * 0.0168980;
        let out =
            self.b0 + self.b1 + self.b2 + self.b3 + self.b4 + self.b5 + self.b6 + white * 0.5362;
        self.b6 = white * 0.115926;
        out * 0.11
    }
}

/// Brownian noise (integrated white noise with leakage).
#[derive(Clone)]
pub struct BrownNoise {
    y: f32,
}

impl Default for BrownNoise {
    fn default() -> Self {
        Self { y: 0.0 }
    }
}

impl BrownNoise {
    #[inline]
    pub fn next(&mut self, white: f32) -> f32 {
        self.y = (self.y + white * 0.02).clamp(-1.0, 1.0);
        self.y
    }
}

/// Noise generator with selectable type, amplitude envelope, density envelope, and filter.
#[derive(Clone)]
pub struct NoiseGenerator {
    pub amplitude: f32,
    pub density: f32,
    pub noise_type: NoiseType,
    pub amp_env: Envelope,
    pub density_env: Envelope,
    pub filter: SvfFilter,
    pub filter_type: FilterType,
    pub filter_cutoff_hz: f32,
    pub filter_q: f32,
    rng: Rng,
    pink: PinkNoise,
    brown: BrownNoise,
}

impl NoiseGenerator {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            amplitude: 0.3,
            density: 0.5,
            noise_type: NoiseType::White,
            amp_env: Envelope::with_default_adsr(0.0, 0.03, 0.0, 0.02),
            density_env: Envelope::with_default_adsr(0.0, 0.03, 1.0, 0.02),
            filter: SvfFilter::new(sample_rate, FilterType::Lowpass, 8000.0, 0.7),
            filter_type: FilterType::Lowpass,
            filter_cutoff_hz: 8000.0,
            filter_q: 0.7,
            rng: Rng::new(0x123456789ABCDEF0),
            pink: PinkNoise::default(),
            brown: BrownNoise::default(),
        }
    }

    pub fn reset(&mut self) {
        self.rng = Rng::new(0x123456789ABCDEF0);
        self.pink = PinkNoise::default();
        self.brown = BrownNoise::default();
        self.filter.reset();
    }

    pub fn render(&mut self, out: &mut [f32], num_samples: usize) {
        let dt = 1.0 / num_samples.max(1) as f32;
        let mut env_buf = vec![0.0f32; num_samples];
        let mut density_buf = vec![0.0f32; num_samples];
        self.amp_env.fill_buffer(&mut env_buf, dt);
        self.density_env.fill_buffer(&mut density_buf, dt);

        let thresh = (1.0 - self.density) * u32::MAX as f32;
        for i in 0..num_samples {
            let sample = if self.rng.next_u32() as f32 >= thresh {
                let white = self.rng.next_f32();
                match self.noise_type {
                    NoiseType::White => white,
                    NoiseType::Pink => self.pink.next(white),
                    NoiseType::Brownian => self.brown.next(white),
                }
            } else {
                0.0
            };
            out[i] = sample * self.amplitude * env_buf[i] * density_buf[i];
        }

        // Apply filter to noise
        self.filter.filter_type = self.filter_type;
        self.filter.set_params(self.filter_cutoff_hz, self.filter_q);
        self.filter.process_block(out);
    }
}
