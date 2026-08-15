use std::sync::Arc;

use crate::common::resampler::{ResampleQuality, resample_buffer};
use crate::sampler::dsp::sample::Sample;

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

    let data_l = resample_buffer(
        &sample.data_l,
        sample.sample_rate as f64,
        target_sample_rate as f64,
        ResampleQuality::Best,
    );
    let data_r = resample_buffer(
        &sample.data_r,
        sample.sample_rate as f64,
        target_sample_rate as f64,
        ResampleQuality::Best,
    );

    let out_frames = data_l.len();
    let channels = vec![data_l, data_r];
    let (peak, rms) = crate::common::audio_file::compute_stats(&channels);
    let mut iter = channels.into_iter();

    Arc::new(Sample {
        sample_rate: target_sample_rate,
        data_l: iter.next().unwrap(),
        data_r: iter.next().unwrap(),
        frames: out_frames,
        peak,
        rms,
    })
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
