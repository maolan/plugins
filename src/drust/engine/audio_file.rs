use rayon::prelude::*;
use std::{collections::HashMap, fs::File, path::Path};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(Debug, Clone)]
pub struct LoadedAudioFile {
    pub path: String,
    pub sample_rate: u32,
    pub original_sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}

pub fn load_wav_channels(
    path: &Path,
    channels_to_extract: &[usize],
) -> Result<LoadedAudioFile, String> {
    let file = File::open(path)
        .map_err(|e| format!("Failed to open audio file {}: {e}", path.display()))?;
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
        .map_err(|e| format!("Failed to probe audio file {}: {e}", path.display()))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .or_else(|| format.tracks().first())
        .ok_or_else(|| format!("No usable audio track in {}", path.display()))?;

    let file_channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1);
    if file_channels == 0 {
        return Err(format!("No audio stream in {}", path.display()));
    }
    let sample_rate = track.codec_params.sample_rate.unwrap_or(48_000);
    let track_id = track.id;

    for &ch in channels_to_extract {
        if ch >= file_channels {
            return Err(format!(
                "Channel {ch} out of range (file has {file_channels} channels)",
            ));
        }
    }

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &decoder_opts)
        .map_err(|e| format!("Failed to create decoder for {}: {e}", path.display()))?;

    let mut sample_buf = None;
    let mut all_samples = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => {
                return Err(format!(
                    "Failed reading packets from {}: {e}",
                    path.display()
                ));
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = decoder
            .decode(&packet)
            .map_err(|e| format!("Failed decoding packet from {}: {e}", path.display()))?;

        if sample_buf.is_none() {
            let spec = *decoded.spec();
            sample_buf = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        }
        let buf = sample_buf.as_mut().unwrap();
        buf.copy_planar_ref(decoded);
        all_samples.extend_from_slice(buf.samples());
    }

    let frame_count = all_samples.len() / file_channels;

    let channels: Vec<Vec<f32>> = channels_to_extract
        .par_iter()
        .map(|&ch| {
            let mut data = Vec::with_capacity(frame_count);
            for frame in 0..frame_count {
                data.push(all_samples[frame * file_channels + ch]);
            }
            data
        })
        .collect();

    Ok(LoadedAudioFile {
        path: path.to_string_lossy().into_owned(),
        sample_rate,
        original_sample_rate: sample_rate,
        channels,
    })
}

fn lagrange_interpolate(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c0 = y1;
    let c1 = y2 - y0 * (1.0 / 3.0) - y1 * 0.5 - y3 * (1.0 / 6.0);
    let c2 = (y2 + y0) * 0.5 - y1;
    let c3 = (y2 - y0) * 0.5 + (y1 - y3) * (1.0 / 6.0);
    ((c3 * t + c2) * t + c1) * t + c0
}

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
