use std::sync::Arc;

use crate::sampler::dsp::sample::{InterpolationMode, Sample};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GrainWindow {
    #[default]
    Hann,
}

struct Grain {
    active: bool,
    start_position: f64,
    phase: f64,
    size_samples: usize,
    age: usize,
}

/// A simple granular cloud generator for sampler sources.
///
/// Grains are spawned at a configurable density, each reading a short segment
/// of the sample with independent pitch and a Hann window envelope. The
/// processor writes stereo output into the supplied buffers.
pub struct GranularEngine {
    sample: Arc<Sample>,
    sample_rate: f32,
    grain_size_samples: usize,
    density: f32,
    position: f64,
    pitch_semitones: f32,
    jitter: f32,
    window: GrainWindow,
    interpolation: InterpolationMode,
    grains: Vec<Grain>,
    samples_until_next_grain: f64,
}

impl GranularEngine {
    pub fn new(sample: Arc<Sample>, sample_rate: f32) -> Self {
        Self {
            sample,
            sample_rate,
            grain_size_samples: 1,
            density: 0.0,
            position: 0.0,
            pitch_semitones: 0.0,
            jitter: 0.0,
            window: GrainWindow::Hann,
            interpolation: InterpolationMode::Sinc,
            grains: Vec::new(),
            samples_until_next_grain: 0.0,
        }
    }

    pub fn set_grain_size_ms(&mut self, ms: f32) {
        let samples = (ms.max(1.0) * self.sample_rate / 1000.0).ceil() as usize;
        self.grain_size_samples = samples.max(1);
    }

    pub fn set_density(&mut self, grains_per_second: f32) {
        self.density = grains_per_second.max(0.0);
        self.samples_until_next_grain = 0.0;
    }

    pub fn set_position(&mut self, position: f64) {
        self.position = position.clamp(0.0, 1.0);
    }

    pub fn set_pitch(&mut self, semitones: f32) {
        self.pitch_semitones = semitones;
    }

    pub fn set_jitter(&mut self, jitter: f32) {
        self.jitter = jitter.clamp(0.0, 1.0);
    }

    pub fn set_window(&mut self, window: GrainWindow) {
        self.window = window;
    }

    pub fn set_interpolation(&mut self, mode: InterpolationMode) {
        self.interpolation = mode;
    }

    pub fn process_block(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        for s in out_l.iter_mut() {
            *s = 0.0;
        }
        for s in out_r.iter_mut() {
            *s = 0.0;
        }

        if self.density <= 0.0 || self.grain_size_samples == 0 {
            return;
        }

        let frames = self.sample.frames;
        if frames == 0 {
            return;
        }

        let interval_samples = self.sample_rate as f64 / self.density as f64;
        let pitch_ratio = 2.0f64.powf(self.pitch_semitones as f64 / 12.0);
        let increment = pitch_ratio * (self.sample.sample_rate as f64 / self.sample_rate as f64);

        for i in 0..out_l.len() {
            self.samples_until_next_grain -= 1.0;
            if self.samples_until_next_grain <= 0.0 {
                self.spawn_grain(frames);
                self.samples_until_next_grain += interval_samples;
            }

            let mut sum_l = 0.0f32;
            let mut sum_r = 0.0f32;

            for grain in &mut self.grains {
                if !grain.active {
                    continue;
                }

                let phase = grain.start_position + grain.phase;
                let (l, r) = self
                    .sample
                    .read_with_increment(phase, increment, self.interpolation);
                let window_amp = Self::window_amplitude(grain.age, grain.size_samples, self.window);
                sum_l += l * window_amp;
                sum_r += r * window_amp;

                grain.phase += increment;
                grain.age += 1;
                if grain.age >= grain.size_samples {
                    grain.active = false;
                }
            }

            out_l[i] = sum_l;
            out_r[i] = sum_r;
        }
    }

    fn spawn_grain(&mut self, frames: usize) {
        let base_position = self.position * frames as f64;
        let jitter = if self.jitter > 0.0 {
            (rand::random::<f32>() * 2.0 - 1.0) as f64 * self.jitter as f64 * frames as f64
        } else {
            0.0
        };
        let start_position = (base_position + jitter).clamp(0.0, frames.saturating_sub(1) as f64);

        // Find a free grain slot or reuse the oldest inactive one.
        if let Some(grain) = self.grains.iter_mut().find(|g| !g.active) {
            *grain = Grain {
                active: true,
                start_position,
                phase: 0.0,
                size_samples: self.grain_size_samples,
                age: 0,
            };
        } else {
            self.grains.push(Grain {
                active: true,
                start_position,
                phase: 0.0,
                size_samples: self.grain_size_samples,
                age: 0,
            });
        }
    }

    fn window_amplitude(age: usize, size: usize, window: GrainWindow) -> f32 {
        if size <= 1 {
            return 1.0;
        }
        match window {
            GrainWindow::Hann => {
                let x = age as f64 / (size - 1) as f64;
                (0.5 - 0.5 * (2.0 * std::f64::consts::PI * x).cos()) as f32
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sample() -> Arc<Sample> {
        Arc::new(Sample {
            sample_rate: 48000.0,
            data_l: (0..48000).map(|i| (i as f32 * 0.1).sin()).collect(),
            data_r: (0..48000).map(|i| (i as f32 * 0.1).sin()).collect(),
            frames: 48000,
            peak: 1.0,
            rms: 0.7,
        })
    }

    #[test]
    fn test_granular_produces_output() {
        let sample = make_sample();
        let mut engine = GranularEngine::new(sample, 48000.0);
        engine.set_grain_size_ms(50.0);
        engine.set_density(20.0);
        engine.set_position(0.1);
        engine.set_pitch(0.0);

        let mut out_l = vec![0.0f32; 256];
        let mut out_r = vec![0.0f32; 256];
        engine.process_block(&mut out_l, &mut out_r);

        assert!(
            out_l.iter().any(|&s| s != 0.0),
            "granular engine should produce non-silent output"
        );
        assert!(out_l.iter().all(|&s| s.is_finite()));
        assert!(out_r.iter().all(|&s| s.is_finite()));
    }

    #[test]
    fn test_granular_silences_when_density_zero() {
        let sample = make_sample();
        let mut engine = GranularEngine::new(sample, 48000.0);
        engine.set_grain_size_ms(50.0);
        engine.set_density(0.0);

        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        engine.process_block(&mut out_l, &mut out_r);

        assert!(out_l.iter().all(|&s| s == 0.0));
        assert!(out_r.iter().all(|&s| s == 0.0));
    }
}
