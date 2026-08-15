use std::path::Path;
use std::sync::Arc;

use crate::common::audio_file::{LoadError, decode_file};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpolationMode {
    Zoh = 0,

    Linear = 1,

    Sinc = 2,
}

impl InterpolationMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => InterpolationMode::Zoh,
            2 => InterpolationMode::Sinc,
            _ => InterpolationMode::Linear,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Sample {
    pub sample_rate: f32,
    pub data_l: Vec<f32>,
    pub data_r: Vec<f32>,
    pub frames: usize,

    pub peak: f32,

    pub rms: f32,
}

impl Sample {
    pub fn silent(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            data_l: vec![0.0],
            data_r: vec![0.0],
            frames: 1,
            peak: 0.0,
            rms: 0.0,
        }
    }

    pub fn read(&self, phase: f64, mode: InterpolationMode) -> (f32, f32) {
        match mode {
            InterpolationMode::Zoh => self.read_zoh(phase),
            InterpolationMode::Linear => self.read_linear(phase),
            InterpolationMode::Sinc => self.read_sinc(phase),
        }
    }

    pub fn read_with_increment(
        &self,
        phase: f64,
        increment: f64,
        mode: InterpolationMode,
    ) -> (f32, f32) {
        match mode {
            InterpolationMode::Zoh => self.read_zohaa(phase, increment),
            InterpolationMode::Linear => self.read_linear(phase),
            InterpolationMode::Sinc => self.read_sinc(phase),
        }
    }

    fn read_zoh(&self, phase: f64) -> (f32, f32) {
        let idx = phase.round() as usize;
        let idx = idx.min(self.frames.saturating_sub(1));
        (self.data_l[idx], self.data_r[idx])
    }

    fn read_zohaa(&self, phase: f64, increment: f64) -> (f32, f32) {
        let frames = self.frames;
        if frames == 0 {
            return (0.0, 0.0);
        }
        let inc = increment.abs();
        if inc <= 1.0 {
            return self.read_zoh(phase);
        }
        let start = phase.floor() as isize;
        let end = (phase + increment).floor() as isize;
        let (low, high) = if increment >= 0.0 {
            (start, end)
        } else {
            (end, start)
        };
        let mut sum_l = 0.0f64;
        let mut sum_r = 0.0f64;
        let mut count = 0usize;
        for i in low..=high {
            let idx = i.clamp(0, frames.saturating_sub(1) as isize) as usize;
            sum_l += self.data_l[idx] as f64;
            sum_r += self.data_r[idx] as f64;
            count += 1;
        }
        if count > 0 {
            let scale = 1.0 / count as f64;
            ((sum_l * scale) as f32, (sum_r * scale) as f32)
        } else {
            self.read_zoh(phase)
        }
    }

    pub fn read_linear(&self, phase: f64) -> (f32, f32) {
        let frames = self.frames;
        if frames == 0 {
            return (0.0, 0.0);
        }
        let idx = phase as usize;
        let frac = (phase - idx as f64) as f32;
        let idx0 = idx.min(frames - 1);
        let idx1 = (idx + 1).min(frames - 1);

        let l0 = self.data_l[idx0];
        let l1 = self.data_l[idx1];
        let r0 = self.data_r[idx0];
        let r1 = self.data_r[idx1];

        (l0 + (l1 - l0) * frac, r0 + (r1 - r0) * frac)
    }

    pub fn normalize_peak(&mut self, target_peak: f32) {
        if self.peak > 1e-10 {
            let scale = target_peak / self.peak;
            for s in &mut self.data_l {
                *s *= scale;
            }
            for s in &mut self.data_r {
                *s *= scale;
            }
            self.peak = target_peak;
            self.rms *= scale;
        }
    }

    pub fn normalize_rms(&mut self, target_rms: f32) {
        if self.rms > 1e-10 {
            let scale = target_rms / self.rms;
            for s in &mut self.data_l {
                *s *= scale;
            }
            for s in &mut self.data_r {
                *s *= scale;
            }
            self.peak *= scale;
            self.rms = target_rms;
        }
    }

    fn read_sinc(&self, phase: f64) -> (f32, f32) {
        let frames = self.frames;
        if frames == 0 {
            return (0.0, 0.0);
        }
        let half_window = 8;
        let base = phase.floor() as isize;
        let frac = (phase - base as f64) as f32;
        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;
        let mut weight_sum = 0.0f32;

        for i in -half_window..=half_window {
            let idx = base + i;
            if idx < 0 || idx >= frames as isize {
                continue;
            }
            let idx = idx as usize;
            let t = frac - i as f32;
            let weight = if t.abs() < 1e-6 {
                window_blackman_harris(0.0, half_window as f32)
            } else {
                let pi_t = std::f32::consts::PI * t;
                (pi_t.sin() / pi_t) * window_blackman_harris(i as f32, half_window as f32)
            };
            sum_l += self.data_l[idx] * weight;
            sum_r += self.data_r[idx] * weight;
            weight_sum += weight;
        }

        if weight_sum > 1e-6 {
            (sum_l / weight_sum, sum_r / weight_sum)
        } else {
            self.read_linear(phase)
        }
    }
}

fn window_blackman_harris(n: f32, half_width: f32) -> f32 {
    let x = n / half_width;
    let a0 = 0.35875;
    let a1 = 0.48829;
    let a2 = 0.14128;
    let a3 = 0.01168;
    a0 + a1 * (std::f32::consts::PI * x).cos()
        + a2 * (2.0 * std::f32::consts::PI * x).cos()
        + a3 * (3.0 * std::f32::consts::PI * x).cos()
}

pub fn load_audio(path: &Path) -> Result<Arc<Sample>, LoadError> {
    let audio_file = decode_file(path)?.into_stereo()?;
    let sample_rate = audio_file.sample_rate;
    let (data_l, data_r) = audio_file.into_stereo_buffers();
    let frames = data_l.len();
    if frames == 0 {
        return Err(LoadError::EmptySample);
    }

    let channels = vec![data_l.clone(), data_r.clone()];
    let (peak, rms) = crate::common::audio_file::compute_stats(&channels);

    Ok(Arc::new(Sample {
        sample_rate,
        data_l,
        data_r,
        frames,
        peak,
        rms,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_silent_sample() {
        let s = Sample::silent(48000.0);
        assert_eq!(s.frames, 1);
        assert_eq!(s.read(0.0, InterpolationMode::Linear), (0.0, 0.0));
    }

    #[test]
    fn test_linear_interpolation() {
        let s = Sample {
            sample_rate: 44100.0,
            data_l: vec![0.0, 1.0, 0.0],
            data_r: vec![0.0, 0.5, 0.0],
            frames: 3,
            peak: 1.0,
            rms: 0.0,
        };
        let (l, r) = s.read(0.5, InterpolationMode::Linear);
        assert!((l - 0.5).abs() < 0.001);
        assert!((r - 0.25).abs() < 0.001);
    }

    #[test]
    fn test_zoh_interpolation() {
        let s = Sample {
            sample_rate: 44100.0,
            data_l: vec![0.0, 1.0, 0.0],
            data_r: vec![0.0, 0.5, 0.0],
            frames: 3,
            peak: 1.0,
            rms: 0.0,
        };

        let (l, _r) = s.read(0.4, InterpolationMode::Zoh);
        assert_eq!(l, 0.0);

        let (l, _r) = s.read(0.6, InterpolationMode::Zoh);
        assert_eq!(l, 1.0);
    }

    #[test]
    fn test_zohaa_averages_window() {
        let s = Sample {
            sample_rate: 44100.0,
            data_l: vec![0.0, 1.0, 2.0, 3.0, 4.0],
            data_r: vec![0.0, 1.0, 2.0, 3.0, 4.0],
            frames: 5,
            peak: 4.0,
            rms: 0.0,
        };
        let (l, _r) = s.read_with_increment(0.0, 2.0, InterpolationMode::Zoh);
        assert!(
            (l - 1.0).abs() < 0.001,
            "expected average of 0,1,2, got {l}"
        );
    }

    #[test]
    fn test_zohaa_falls_back_to_zoh_for_small_increment() {
        let s = Sample {
            sample_rate: 44100.0,
            data_l: vec![0.0, 1.0, 0.0],
            data_r: vec![0.0, 0.5, 0.0],
            frames: 3,
            peak: 1.0,
            rms: 0.0,
        };
        let (l, _r) = s.read_with_increment(0.6, 0.5, InterpolationMode::Zoh);
        assert_eq!(l, 1.0);
    }

    #[test]
    fn test_sinc_dc_preserve() {
        let s = Sample {
            sample_rate: 44100.0,
            data_l: vec![1.0; 64],
            data_r: vec![1.0; 64],
            frames: 64,
            peak: 1.0,
            rms: 1.0,
        };
        for phase in [0.0f64, 0.5, 10.5, 31.75] {
            let (l, r) = s.read(phase, InterpolationMode::Sinc);
            assert!((l - 1.0).abs() < 0.001, "sinc DC l at {phase}: {l}");
            assert!((r - 1.0).abs() < 0.001, "sinc DC r at {phase}: {r}");
        }
    }
}
