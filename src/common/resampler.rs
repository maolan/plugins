//! Offline sample-rate conversion for sample buffers.

use super::audio_file::{AudioFile, LoadError};

/// Resampling quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResampleQuality {
    /// Fast linear interpolation for small ratio differences; falls back to
    /// cubic Lagrange for larger ratios. Suitable for bulk loading where
    /// speed matters more than perfect quality.
    Fast,
    /// High-quality windowed-sinc interpolation. Suitable for musical sample
    /// libraries and large transpositions.
    Best,
}

/// Resample a single channel buffer from `src_rate` to `dst_rate`.
pub fn resample_buffer(
    input: &[f32],
    src_rate: f64,
    dst_rate: f64,
    quality: ResampleQuality,
) -> Vec<f32> {
    if (src_rate - dst_rate).abs() < 0.1 || input.is_empty() {
        return input.to_vec();
    }

    let ratio = src_rate / dst_rate;
    let output_len = (input.len() as f64 / ratio).ceil() as usize;
    if output_len == 0 {
        return Vec::new();
    }

    match quality {
        ResampleQuality::Fast => resample_fast(input, ratio, output_len),
        ResampleQuality::Best => resample_sinc(input, ratio, output_len),
    }
}

/// Resample every channel in an `AudioFile` to `target_sample_rate`.
pub fn resample_audio_file(
    file: &mut AudioFile,
    target_sample_rate: f32,
    quality: ResampleQuality,
) -> Result<(), LoadError> {
    if (file.sample_rate - target_sample_rate).abs() < f32::EPSILON {
        return Ok(());
    }

    let src_rate = file.sample_rate as f64;
    let dst_rate = target_sample_rate as f64;

    let mut new_channels = Vec::with_capacity(file.channel_count());
    for ch in &file.channels {
        new_channels.push(resample_buffer(ch, src_rate, dst_rate, quality));
    }

    let (peak, rms) = super::audio_file::compute_stats(&new_channels);
    file.channels = new_channels;
    file.sample_rate = target_sample_rate;
    file.peak = peak;
    file.rms = rms;
    Ok(())
}

fn resample_fast(input: &[f32], ratio: f64, output_len: usize) -> Vec<f32> {
    let ratio_error = (ratio - 1.0).abs();
    let mut output = Vec::with_capacity(output_len);

    if ratio_error < 0.15 {
        for i in 0..output_len {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = input.get(idx).copied().unwrap_or(0.0);
            let b = input.get(idx + 1).copied().unwrap_or(0.0);
            output.push(a + (b - a) * frac);
        }
    } else {
        for i in 0..output_len {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;

            let y0 = input.get(idx.saturating_sub(1)).copied().unwrap_or(0.0);
            let y1 = input.get(idx).copied().unwrap_or(0.0);
            let y2 = input.get(idx + 1).copied().unwrap_or(0.0);
            let y3 = input.get(idx + 2).copied().unwrap_or(0.0);

            output.push(lagrange_interpolate(y0, y1, y2, y3, frac));
        }
    }

    output
}

fn lagrange_interpolate(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c0 = y1;
    let c1 = y2 - y0 * (1.0 / 3.0) - y1 * 0.5 - y3 * (1.0 / 6.0);
    let c2 = (y2 + y0) * 0.5 - y1;
    let c3 = (y2 - y0) * 0.5 + (y1 - y3) * (1.0 / 6.0);
    ((c3 * t + c2) * t + c1) * t + c0
}

fn resample_sinc(input: &[f32], ratio: f64, output_len: usize) -> Vec<f32> {
    let half_window = 8;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let phase = i as f64 * ratio;
        let base = phase.floor() as isize;
        let frac = (phase - base as f64) as f32;

        let mut sum = 0.0f32;
        let mut weight_sum = 0.0f32;

        for j in -half_window..=half_window {
            let idx = base + j;
            if idx < 0 || idx >= input.len() as isize {
                continue;
            }
            let idx = idx as usize;
            let t = frac - j as f32;
            let weight = if t.abs() < 1e-6 {
                window_blackman_harris(0.0, half_window as f32)
            } else {
                let pi_t = std::f32::consts::PI * t;
                (pi_t.sin() / pi_t) * window_blackman_harris(j as f32, half_window as f32)
            };
            sum += input[idx] * weight;
            weight_sum += weight;
        }

        output.push(if weight_sum > 1e-6 {
            sum / weight_sum
        } else {
            *input.get(base as usize).unwrap_or(&0.0)
        });
    }

    output
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_same_rate_is_noop() {
        let input = vec![1.0, 2.0, 3.0, 4.0];
        let out = resample_buffer(&input, 48000.0, 48000.0, ResampleQuality::Fast);
        assert_eq!(out, input);
    }

    #[test]
    fn best_same_rate_is_noop() {
        let input = vec![1.0, 2.0, 3.0, 4.0];
        let out = resample_buffer(&input, 48000.0, 48000.0, ResampleQuality::Best);
        assert_eq!(out, input);
    }

    #[test]
    fn best_preserves_dc() {
        let input = vec![0.5; 128];
        let out = resample_buffer(&input, 48000.0, 44100.0, ResampleQuality::Best);
        assert!(
            out.iter().all(|&s| (s - 0.5).abs() < 0.01),
            "DC should be preserved"
        );
    }

    #[test]
    fn fast_preserves_dc() {
        let input = vec![0.5; 1024];
        let out = resample_buffer(&input, 48000.0, 44100.0, ResampleQuality::Fast);
        // Fast interpolation is approximate; ignore boundary frames.
        let check = &out[16..out.len().saturating_sub(16)];
        assert!(
            check.iter().all(|&s| (s - 0.5).abs() < 0.02),
            "DC should be approximately preserved in interior frames"
        );
    }
}
