use std::sync::Arc;

use crate::sampler::dsp::sample::{InterpolationMode, Sample};

/// Offline sample-rate conversion for sampler zones.
///
/// Produces a new `Sample` at `target_sample_rate` by reading the source
/// through the sinc interpolator. This lets zones be pre-rendered to the
/// project sample rate instead of relying solely on realtime playback
/// increment, which improves quality for large transpositions.
pub fn resample(sample: &Sample, target_sample_rate: f32) -> Arc<Sample> {
    if (sample.sample_rate - target_sample_rate).abs() < f32::EPSILON {
        return Arc::new(sample.clone());
    }

    let ratio = sample.sample_rate as f64 / target_sample_rate as f64;
    let out_frames = ((sample.frames as f64) / ratio).ceil().max(1.0) as usize;

    let mut data_l = Vec::with_capacity(out_frames);
    let mut data_r = Vec::with_capacity(out_frames);

    for i in 0..out_frames {
        let phase = i as f64 * ratio;
        let (l, r) = sample.read(phase, InterpolationMode::Sinc);
        data_l.push(l);
        data_r.push(r);
    }

    let (peak, rms) = compute_stats(&data_l, &data_r);

    Arc::new(Sample {
        sample_rate: target_sample_rate,
        data_l,
        data_r,
        frames: out_frames,
        peak,
        rms,
    })
}

fn compute_stats(left: &[f32], right: &[f32]) -> (f32, f32) {
    let mut peak = 0.0f32;
    let mut sum_sq = 0.0f64;
    let mut count = 0usize;
    for &s in left.iter().chain(right.iter()) {
        let abs = s.abs();
        if abs > peak {
            peak = abs;
        }
        sum_sq += (s as f64) * (s as f64);
        count += 1;
    }
    let rms = ((sum_sq / count.max(1) as f64) as f32).sqrt();
    (peak, rms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resample_preserves_dc() {
        let sample = Sample {
            sample_rate: 48000.0,
            data_l: vec![0.5; 100],
            data_r: vec![0.5; 100],
            frames: 100,
            peak: 0.5,
            rms: 0.5,
        };

        let resampled = resample(&sample, 44100.0);
        assert!((resampled.sample_rate - 44100.0).abs() < 0.001);
        assert!(
            resampled.frames > 80 && resampled.frames < 100,
            "expected fewer frames at lower sample rate"
        );
        assert!(
            resampled.data_l.iter().all(|&s| (s - 0.5).abs() < 0.01),
            "DC should be preserved"
        );
    }

    #[test]
    fn test_resample_noop_at_same_rate() {
        let sample = Sample {
            sample_rate: 48000.0,
            data_l: vec![1.0; 64],
            data_r: vec![1.0; 64],
            frames: 64,
            peak: 1.0,
            rms: 1.0,
        };

        let resampled = resample(&sample, 48000.0);
        assert_eq!(resampled.frames, 64);
    }
}
