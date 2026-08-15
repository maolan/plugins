use std::{collections::HashMap, path::Path};

use rayon::prelude::*;

use crate::common::{
    audio_file::{AudioFile, decode_file_channels},
    resampler::ResampleQuality,
};

/// Drust re-exports the common audio file type under its historical name.
pub type LoadedAudioFile = AudioFile;

/// Load specific channels from an audio file.
///
/// This is a thin wrapper around the common decoder that maps errors to
/// strings for the existing Drust loading pipeline.
pub fn load_wav_channels(
    path: &Path,
    channels_to_extract: &[usize],
) -> Result<LoadedAudioFile, String> {
    decode_file_channels(path, Some(channels_to_extract)).map_err(|e| e.to_string())
}

/// Resample a single channel buffer from `src_rate` to `dst_rate`.
///
/// Kept as a Drust-facing convenience that uses the common fast resampler.
pub fn resample_buffer(input: &[f32], src_rate: f64, dst_rate: f64) -> Vec<f32> {
    if (src_rate - dst_rate).abs() < 0.1 {
        return input.to_vec();
    }
    resample_buffer_internal(input, src_rate, dst_rate, ResampleQuality::Fast)
}

fn resample_buffer_internal(
    input: &[f32],
    src_rate: f64,
    dst_rate: f64,
    quality: ResampleQuality,
) -> Vec<f32> {
    crate::common::resampler::resample_buffer(input, src_rate, dst_rate, quality)
}

/// Load all audio files referenced by a drum kit and resample them to the host
/// sample rate.
pub fn load_kit_audio(
    _kit_dir: &Path,
    kit: &crate::drust::drumkit::DrumKit,
    host_rate: f32,
) -> Result<HashMap<String, LoadedAudioFile>, String> {
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

    for channels in files.values_mut() {
        channels.sort_unstable();
        channels.dedup();
    }

    let results: Vec<Result<(String, LoadedAudioFile), String>> =
        super::load_pool().install(|| {
            files
                .into_par_iter()
                .map(|(path, channels)| {
                    let mut file = load_wav_channels(Path::new(&path), &channels)
                        .map_err(|e| e.to_string())?;
                    if (file.original_sample_rate - host_rate).abs() > 0.1 {
                        for ch in &mut file.channels {
                            *ch = crate::common::resampler::resample_buffer(
                                ch,
                                file.original_sample_rate as f64,
                                host_rate as f64,
                                ResampleQuality::Fast,
                            );
                        }
                        file.sample_rate = host_rate;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::File,
        io::Write,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn write_test_wav(path: &Path) {
        let frames: [[i16; 4]; 3] = [
            [1000, 10_000, -1000, -10_000],
            [2000, 20_000, -2000, -20_000],
            [3000, 30_000, -3000, -30_000],
        ];
        let channel_count = frames[0].len() as u16;
        let data_bytes =
            (frames.len() * channel_count as usize * std::mem::size_of::<i16>()) as u32;
        let mut file = File::create(path).expect("create wav");

        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + data_bytes).to_le_bytes()).unwrap();
        file.write_all(b"WAVE").unwrap();
        file.write_all(b"fmt ").unwrap();
        file.write_all(&16_u32.to_le_bytes()).unwrap();
        file.write_all(&1_u16.to_le_bytes()).unwrap();
        file.write_all(&channel_count.to_le_bytes()).unwrap();
        file.write_all(&48_000_u32.to_le_bytes()).unwrap();
        file.write_all(&(48_000_u32 * channel_count as u32 * 2).to_le_bytes())
            .unwrap();
        file.write_all(&(channel_count * 2).to_le_bytes()).unwrap();
        file.write_all(&16_u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&data_bytes.to_le_bytes()).unwrap();
        for frame in frames {
            for sample in frame {
                file.write_all(&sample.to_le_bytes()).unwrap();
            }
        }
    }

    fn temp_wav_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("drust-audio-file-test-{nanos}.wav"))
    }

    #[test]
    fn load_wav_channels_preserves_interleaved_channel_order() {
        let path = temp_wav_path();
        write_test_wav(&path);

        let loaded = load_wav_channels(&path, &[0, 1]).expect("load wav");
        let _ = std::fs::remove_file(&path);

        assert_eq!(loaded.source_channels, [0, 1]);
        assert_eq!(loaded.channels.len(), 2);
        assert_eq!(loaded.channels[0].len(), 3);
        assert_eq!(loaded.channels[1].len(), 3);

        let scale = i16::MAX as f32;
        let left: Vec<i16> = loaded.channels[0]
            .iter()
            .map(|sample| (sample * scale).round() as i16)
            .collect();
        let right: Vec<i16> = loaded.channels[1]
            .iter()
            .map(|sample| (sample * scale).round() as i16)
            .collect();

        assert_samples_near(&left, &[1000, 2000, 3000]);
        assert_samples_near(&right, &[10_000, 20_000, 30_000]);
    }

    #[test]
    fn load_wav_channels_preserves_source_channel_mapping() {
        let path = temp_wav_path();
        write_test_wav(&path);

        let loaded = load_wav_channels(&path, &[2, 3]).expect("load wav");
        let _ = std::fs::remove_file(&path);

        assert_eq!(loaded.source_channels, [2, 3]);
        assert_eq!(loaded.loaded_channel_index(2), Some(0));
        assert_eq!(loaded.loaded_channel_index(3), Some(1));
        assert_eq!(loaded.loaded_channel_index(1), None);

        let scale = i16::MAX as f32;
        let ch2: Vec<i16> = loaded.channels[loaded.loaded_channel_index(2).unwrap()]
            .iter()
            .map(|sample| (sample * scale).round() as i16)
            .collect();
        let ch3: Vec<i16> = loaded.channels[loaded.loaded_channel_index(3).unwrap()]
            .iter()
            .map(|sample| (sample * scale).round() as i16)
            .collect();

        assert_samples_near(&ch2, &[-1000, -2000, -3000]);
        assert_samples_near(&ch3, &[-10_000, -20_000, -30_000]);
    }

    fn assert_samples_near(actual: &[i16], expected: &[i16]) {
        assert_eq!(actual.len(), expected.len());
        for (&actual, &expected) in actual.iter().zip(expected) {
            assert!(
                (i32::from(actual) - i32::from(expected)).abs() <= 1,
                "sample {actual} differs from expected {expected}",
            );
        }
    }
}
