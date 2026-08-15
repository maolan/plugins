use std::{fs::File, path::Path, sync::Arc};

use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_NULL, DecoderOptions},
    errors::Error as SymphoniaError,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

/// An audio file decoded into non-interleaved `f32` channels.
///
/// This is the shared representation used by Drust, Sampler, and any future
/// sample-based plugins. It is intentionally simple: a list of channel buffers,
/// sample rate, and basic loudness statistics.
#[derive(Debug, Clone)]
pub struct AudioFile {
    pub path: String,
    pub sample_rate: f32,
    pub original_sample_rate: f32,
    /// Decoded channels in non-interleaved order.
    pub channels: Vec<Vec<f32>>,
    /// For files where only a subset of channels was requested, this maps
    /// source channel index -> index in `channels`. Otherwise it is
    /// `0..channels.len()`.
    pub source_channels: Vec<usize>,
    pub peak: f32,
    pub rms: f32,
}

impl AudioFile {
    /// Number of frames (samples per channel).
    pub fn frames(&self) -> usize {
        self.channels.first().map(Vec::len).unwrap_or(0)
    }

    /// Number of decoded channels.
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Find the local index of an originally requested source channel.
    pub fn loaded_channel_index(&self, source_channel: usize) -> Option<usize> {
        self.source_channels
            .iter()
            .position(|&ch| ch == source_channel)
    }

    /// Return a new `AudioFile` containing only the requested source channels.
    ///
    /// This is used by Drust to load individual microphone channels from a
    /// multi-channel drum sample.
    pub fn extract_channels(&self, channels_to_extract: &[usize]) -> Result<Self, LoadError> {
        if channels_to_extract.is_empty() {
            return Err(LoadError::Decode(
                "extract_channels called with empty channel list".to_string(),
            ));
        }

        let mut channels = Vec::with_capacity(channels_to_extract.len());
        for &source_ch in channels_to_extract {
            let local_idx = self
                .loaded_channel_index(source_ch)
                .ok_or_else(|| LoadError::Decode(format!("channel {source_ch} not loaded")))?;
            channels.push(self.channels[local_idx].clone());
        }

        let (peak, rms) = compute_stats(&channels);
        Ok(Self {
            path: self.path.clone(),
            sample_rate: self.sample_rate,
            original_sample_rate: self.original_sample_rate,
            source_channels: channels_to_extract.to_vec(),
            channels,
            peak,
            rms,
        })
    }

    /// Convert to a stereo `AudioFile`.
    ///
    /// - Mono files are duplicated to left/right.
    /// - Stereo files are returned unchanged.
    /// - Files with more than two channels return an error.
    pub fn into_stereo(self) -> Result<Self, LoadError> {
        let channels = match self.channel_count() {
            0 => return Err(LoadError::EmptySample),
            1 => {
                let mono = self.channels.into_iter().next().unwrap();
                vec![mono.clone(), mono]
            }
            2 => self.channels,
            n => {
                return Err(LoadError::Decode(format!(
                    "into_stereo called with {n} channels"
                )));
            }
        };

        let (peak, rms) = compute_stats(&channels);
        Ok(Self {
            path: self.path,
            sample_rate: self.sample_rate,
            original_sample_rate: self.original_sample_rate,
            source_channels: vec![0, 1],
            channels,
            peak,
            rms,
        })
    }

    /// Split a stereo file into left/right buffers.
    ///
    /// Panics if the file is not stereo.
    pub fn into_stereo_buffers(self) -> (Vec<f32>, Vec<f32>) {
        assert_eq!(self.channel_count(), 2, "expected stereo file");
        let mut iter = self.channels.into_iter();
        (iter.next().unwrap(), iter.next().unwrap())
    }
}

#[derive(Debug)]
pub enum LoadError {
    Decode(String),
    NoAudioStream,
    EmptySample,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Decode(e) => write!(f, "decode error: {e}"),
            LoadError::NoAudioStream => write!(f, "no audio stream found"),
            LoadError::EmptySample => write!(f, "sample contains no audio data"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Decode an audio file with symphonia.
///
/// All decodeable channels are returned in non-interleaved order. If only a
/// subset of channels is needed, use [`AudioFile::extract_channels`] afterward.
pub fn decode_file(path: &Path) -> Result<AudioFile, LoadError> {
    decode_with_symphonia(path, None)
}

/// Decode an audio file and optionally extract specific channels during decode.
///
/// Channel extraction during decode is slightly more memory-efficient because
/// only the requested channels are kept.
pub fn decode_file_channels(
    path: &Path,
    channels_to_extract: Option<&[usize]>,
) -> Result<AudioFile, LoadError> {
    decode_with_symphonia(path, channels_to_extract)
}

fn decode_with_symphonia(
    path: &Path,
    channels_to_extract: Option<&[usize]>,
) -> Result<AudioFile, LoadError> {
    let file = File::open(path).map_err(|e| LoadError::Decode(e.to_string()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();
    let decoder_opts = DecoderOptions::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|e| LoadError::Decode(format!("probe error: {e}")))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .or_else(|| format.tracks().first())
        .ok_or(LoadError::NoAudioStream)?;

    let file_channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1);
    if file_channels == 0 {
        return Err(LoadError::NoAudioStream);
    }

    if let Some(channels) = channels_to_extract {
        for &ch in channels {
            if ch >= file_channels {
                return Err(LoadError::Decode(format!(
                    "channel {ch} out of range (file has {file_channels} channels)"
                )));
            }
        }
    }

    let sample_rate = track.codec_params.sample_rate.unwrap_or(48_000) as f32;
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &decoder_opts)
        .map_err(|e| LoadError::Decode(format!("decoder init error: {e}")))?;

    let mut sample_buf = None;
    let mut interleaved = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => return Err(LoadError::Decode(format!("read error: {e}"))),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = decoder
            .decode(&packet)
            .map_err(|e| LoadError::Decode(format!("decode error: {e}")))?;

        if sample_buf.is_none() {
            let spec = *decoded.spec();
            sample_buf = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        }
        let buf = sample_buf.as_mut().unwrap();
        buf.copy_interleaved_ref(decoded);
        interleaved.extend_from_slice(buf.samples());
    }

    if interleaved.is_empty() {
        return Err(LoadError::EmptySample);
    }

    let frames = interleaved.len() / file_channels;
    let channel_indices: Vec<usize> = match channels_to_extract {
        Some(ch) => ch.to_vec(),
        None => (0..file_channels).collect(),
    };

    let channels: Vec<Vec<f32>> = channel_indices
        .iter()
        .map(|&source_ch| {
            let mut data = Vec::with_capacity(frames);
            for frame in 0..frames {
                data.push(interleaved[frame * file_channels + source_ch]);
            }
            data
        })
        .collect();

    let (peak, rms) = compute_stats(&channels);

    Ok(AudioFile {
        path: path.to_string_lossy().into_owned(),
        sample_rate,
        original_sample_rate: sample_rate,
        source_channels: channel_indices,
        channels,
        peak,
        rms,
    })
}

pub fn compute_stats(channels: &[Vec<f32>]) -> (f32, f32) {
    let mut peak = 0.0f32;
    let mut sum_sq = 0.0f64;
    let mut count = 0usize;
    for ch in channels {
        for &s in ch {
            let abs = s.abs();
            if abs > peak {
                peak = abs;
            }
            sum_sq += (s as f64) * (s as f64);
            count += 1;
        }
    }
    let rms = if count > 0 {
        ((sum_sq / count as f64) as f32).sqrt()
    } else {
        0.0
    };
    (peak, rms)
}

/// Shared wrapper used by the Sampler sample cache.
pub type SharedAudioFile = Arc<AudioFile>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::File,
        io::Write,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn write_test_wav(path: &Path, channels: u16, samples: &[Vec<i16>]) {
        let data_bytes = (samples.len() * channels as usize * std::mem::size_of::<i16>()) as u32;
        let mut file = File::create(path).expect("create wav");

        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + data_bytes).to_le_bytes()).unwrap();
        file.write_all(b"WAVE").unwrap();
        file.write_all(b"fmt ").unwrap();
        file.write_all(&16_u32.to_le_bytes()).unwrap();
        file.write_all(&1_u16.to_le_bytes()).unwrap();
        file.write_all(&channels.to_le_bytes()).unwrap();
        file.write_all(&48_000_u32.to_le_bytes()).unwrap();
        file.write_all(&(48_000_u32 * channels as u32 * 2).to_le_bytes())
            .unwrap();
        file.write_all(&(channels * 2).to_le_bytes()).unwrap();
        file.write_all(&16_u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&data_bytes.to_le_bytes()).unwrap();
        for frame in samples {
            for sample in frame.iter().take(channels as usize) {
                file.write_all(&sample.to_le_bytes()).unwrap();
            }
        }
    }

    fn temp_wav_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("common-audio-file-test-{nanos}.wav"))
    }

    #[test]
    fn decode_stereo_preserves_channel_order() {
        let path = temp_wav_path();
        let frames: Vec<Vec<i16>> =
            vec![vec![1000, 10_000], vec![2000, 20_000], vec![3000, 30_000]];
        write_test_wav(&path, 2, &frames);

        let file = decode_file(&path).expect("decode");
        let _ = std::fs::remove_file(&path);

        assert_eq!(file.channel_count(), 2);
        assert_eq!(file.frames(), 3);
        assert_eq!(file.source_channels, [0, 1]);

        let scale = i16::MAX as f32;
        let left: Vec<i16> = file.channels[0]
            .iter()
            .map(|s| (s * scale).round() as i16)
            .collect();
        let right: Vec<i16> = file.channels[1]
            .iter()
            .map(|s| (s * scale).round() as i16)
            .collect();

        assert_samples_near(&left, &[1000, 2000, 3000]);
        assert_samples_near(&right, &[10_000, 20_000, 30_000]);
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

    #[test]
    fn decode_channels_extracts_subset() {
        let path = temp_wav_path();
        let frames: Vec<Vec<i16>> = vec![
            vec![1000, 10_000, -1000, -10_000],
            vec![2000, 20_000, -2000, -20_000],
            vec![3000, 30_000, -3000, -30_000],
        ];
        write_test_wav(&path, 4, &frames);

        let file = decode_file_channels(&path, Some(&[2, 3])).expect("decode");
        let _ = std::fs::remove_file(&path);

        assert_eq!(file.source_channels, [2, 3]);
        assert_eq!(file.loaded_channel_index(2), Some(0));
        assert_eq!(file.loaded_channel_index(3), Some(1));
        assert_eq!(file.loaded_channel_index(1), None);

        let scale = i16::MAX as f32;
        let ch2: Vec<i16> = file.channels[file.loaded_channel_index(2).unwrap()]
            .iter()
            .map(|s| (s * scale).round() as i16)
            .collect();
        assert_eq!(ch2, &[-1000, -2000, -3000]);
    }

    #[test]
    fn into_stereo_duplicates_mono() {
        let path = temp_wav_path();
        let frames: Vec<Vec<i16>> = vec![vec![1000], vec![2000], vec![3000]];
        write_test_wav(&path, 1, &frames);

        let file = decode_file(&path)
            .expect("decode")
            .into_stereo()
            .expect("stereo");
        let _ = std::fs::remove_file(&path);

        assert_eq!(file.channel_count(), 2);
        assert_eq!(file.channels[0], file.channels[1]);
    }
}
