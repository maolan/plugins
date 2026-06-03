#![allow(dead_code)]

//! Noise generator with filter.

use rand::random;

use super::filter::{Filter, FilterType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseType {
    White = 0,
    Pink = 1,
    Brown = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseColorMode {
    Tilt = 0,
    Legacy = 1,
}

impl NoiseType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => NoiseType::White,
            1 => NoiseType::Pink,
            _ => NoiseType::Brown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NoiseGenerator {
    sample_rate: f32,
    pub noise_type: NoiseType,
    pub level: f32,
    pub color: f32,
    pub filter: Filter,
    pub filter_enabled: bool,
    pub stereo: bool,
    pub color_mode: NoiseColorMode,
    // Pink noise state
    pink_b: [f32; 7],
    pink_state: f32,
    // Brown noise state
    brown_state: f32,
    // Color filter state (1-pole lowpass for spectral tilt)
    color_state: f32,
}

impl NoiseGenerator {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            noise_type: NoiseType::White,
            level: 0.0,
            color: 0.5,
            filter: Filter::new(FilterType::Lowpass, sample_rate),
            filter_enabled: false,
            stereo: false,
            color_mode: NoiseColorMode::Tilt,
            pink_b: [0.0; 7],
            pink_state: 0.0,
            brown_state: 0.0,
            color_state: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.pink_b = [0.0; 7];
        self.pink_state = 0.0;
        self.brown_state = 0.0;
        self.color_state = 0.0;
        self.filter.reset();
    }

    pub fn next(&mut self) -> f32 {
        let raw = match self.noise_type {
            NoiseType::White => random::<f32>() * 2.0 - 1.0,
            NoiseType::Pink => {
                // Paul Kellet's refined pink noise generator
                let white = random::<f32>() * 2.0 - 1.0;
                self.pink_b[0] = 0.99886 * self.pink_b[0] + white * 0.0555179;
                self.pink_b[1] = 0.99332 * self.pink_b[1] + white * 0.0750759;
                self.pink_b[2] = 0.96900 * self.pink_b[2] + white * 0.1538520;
                self.pink_b[3] = 0.86650 * self.pink_b[3] + white * 0.3104856;
                self.pink_b[4] = 0.55000 * self.pink_b[4] + white * 0.5329522;
                self.pink_b[5] = -0.7616 * self.pink_b[5] - white * 0.0168980;
                let sum = self.pink_b[0]
                    + self.pink_b[1]
                    + self.pink_b[2]
                    + self.pink_b[3]
                    + self.pink_b[4]
                    + self.pink_b[5]
                    + self.pink_b[6]
                    + white * 0.5362;
                self.pink_b[6] = white * 0.115926;
                sum * 0.11
            }
            NoiseType::Brown => {
                let white = random::<f32>() * 2.0 - 1.0;
                self.brown_state = (self.brown_state + white * 0.02).clamp(-1.0, 1.0);
                self.brown_state * 3.5
            }
        };

        // Apply color filter
        let colored = match self.color_mode {
            NoiseColorMode::Tilt => {
                // 1-pole lowpass for spectral tilt
                // color = 0.0: dark (cutoff ~50Hz), color = 1.0: bright (no filtering)
                if self.color < 1.0 {
                    let cutoff = 50.0 * 10.0f32.powf(self.color * 2.6);
                    let g = (std::f32::consts::PI * cutoff / self.sample_rate).tan();
                    let g = g / (1.0 + g);
                    self.color_state += g * (raw - self.color_state);
                    self.color_state
                } else {
                    raw
                }
            }
            NoiseColorMode::Legacy => {
                // Legacy: correlated noise using 1-pole LP with different curve
                if self.color < 1.0 {
                    let cutoff = 100.0 * 20.0f32.powf(self.color * 2.0);
                    let g = (std::f32::consts::PI * cutoff / self.sample_rate).tan();
                    let g = g / (1.0 + g);
                    self.color_state += g * (raw - self.color_state);
                    self.color_state * 1.5 // legacy has more gain
                } else {
                    raw
                }
            }
        };

        let filtered = if self.filter_enabled {
            self.filter.process(colored)
        } else {
            colored
        };

        filtered * self.level
    }

    pub fn next_stereo(&mut self) -> (f32, f32) {
        if self.stereo {
            (self.next(), self.next())
        } else {
            let s = self.next();
            (s, s)
        }
    }

    pub fn process_block(&mut self, out: &mut [f32]) {
        for sample in out.iter_mut() {
            *sample = self.next();
        }
    }
}
