use rayon::prelude::*;
use std::{collections::HashMap, path::Path};

/// Loaded and deinterleaved audio data from a WAV file.
#[derive(Debug, Clone)]
pub struct LoadedAudioFile {
    pub path: String,
    pub sample_rate: u32,
    pub original_sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}

/// Load a WAV file and return deinterleaved channel data.
/// `channels_to_extract` is a list of 0-indexed channel indices to extract.
pub fn load_wav_channels(
    path: &Path,
    channels_to_extract: &[usize],
) -> Result<LoadedAudioFile, String> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|e| format!("Failed to open WAV {}: {e}", path.display()))?;

    let spec = reader.spec();
    let file_channels = spec.channels as usize;
    let sample_rate = spec.sample_rate;

    // Read all samples as f32.
    let all_samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read float samples: {e}"))?,
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            let max_val = ((1i64 << (bits - 1)) - 1) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max_val))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("Failed to read int samples: {e}"))?
        }
    };

    let frame_count = all_samples.len() / file_channels;

    // Deinterleave and extract only requested channels.
    let mut channels = Vec::with_capacity(channels_to_extract.len());
    for &ch in channels_to_extract {
        if ch >= file_channels {
            return Err(format!(
                "Channel {ch} out of range (file has {file_channels} channels)",
            ));
        }
        let mut data = Vec::with_capacity(frame_count);
        for frame in 0..frame_count {
            data.push(all_samples[frame * file_channels + ch]);
        }
        channels.push(data);
    }

    Ok(LoadedAudioFile {
        path: path.to_string_lossy().into_owned(),
        sample_rate,
        original_sample_rate: sample_rate,
        channels,
    })
}

/// Lagrange 4-point interpolation for high-quality resampling.
fn lagrange_interpolate(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c0 = y1;
    let c1 = y2 - y0 * (1.0 / 3.0) - y1 * 0.5 - y3 * (1.0 / 6.0);
    let c2 = (y2 + y0) * 0.5 - y1;
    let c3 = (y2 - y0) * 0.5 + (y1 - y3) * (1.0 / 6.0);
    ((c3 * t + c2) * t + c1) * t + c0
}

/// Resample a buffer using linear interpolation for small ratio differences
/// and Lagrange 4-point for larger differences.
pub fn resample_buffer(input: &[f32], src_rate: f64, dst_rate: f64) -> Vec<f32> {
    if (src_rate - dst_rate).abs() < 0.1 {
        return input.to_vec();
    }
    let ratio = src_rate / dst_rate;
    let output_len = (input.len() as f64 / ratio).ceil() as usize;
    if output_len == 0 {
        return Vec::new();
    }

    let ratio_error = (ratio - 1.0).abs();
    let mut output = Vec::with_capacity(output_len);

    if ratio_error < 0.15 {
        // Fast linear interpolation for common conversions (44.1 <-> 48kHz).
        for i in 0..output_len {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = input.get(idx).copied().unwrap_or(0.0);
            let b = input.get(idx + 1).copied().unwrap_or(0.0);
            output.push(a + (b - a) * frac);
        }
    } else {
        // High-quality Lagrange for larger differences.
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

/// Load all unique WAV files referenced by a drumkit, resampling to host rate.
/// Returns a map from absolute file path to loaded audio data.
pub fn load_kit_audio(
    _kit_dir: &Path,
    kit: &crate::drust::drumkit::DrumKit,
    host_rate: f32,
) -> Result<HashMap<String, LoadedAudioFile>, String> {
    // Collect all unique (absolute_path, channel_indices) pairs.
    let mut files: HashMap<String, Vec<usize>> = HashMap::new();

    for instrument in &kit.instruments {
        for sample in &instrument.samples {
            for af in &sample.audiofiles {
                files
                    .entry(af.abs_path.clone())
                    .or_default()
                    .push(af.filechannel);
            }
        }
    }

    // Deduplicate and sort channel indices per file.
    for channels in files.values_mut() {
        channels.sort_unstable();
        channels.dedup();
    }

    // Load each file and resample to host rate in parallel.
    let results: Vec<Result<(String, LoadedAudioFile), String>> =
        super::load_pool().install(|| {
            files
                .into_par_iter()
                .map(|(path, channels)| {
                    let mut file = load_wav_channels(Path::new(&path), &channels)
                        .map_err(|e| e.to_string())?;
                    if (file.original_sample_rate as f64 - host_rate as f64).abs() > 0.1 {
                        for ch in &mut file.channels {
                            *ch = resample_buffer(
                                ch,
                                file.original_sample_rate as f64,
                                host_rate as f64,
                            );
                        }
                        file.sample_rate = host_rate as u32;
                    }
                    Ok((path, file))
                })
                .collect()
        });

    let mut loaded = HashMap::with_capacity(results.len());
    for result in results {
        let (path, file) = result?;
        loaded.insert(path, file);
    }

    Ok(loaded)
}
