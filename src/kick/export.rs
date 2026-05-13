//! Audio export using ffmpeg CLI (same pattern as daw/engine).

use std::path::Path;
use std::process::Command;

/// Export interleaved f32 buffer to an audio file.
/// `format` can be "wav", "flac", "ogg", "mp3".
/// `channels` can be 1 (mono) or 2 (stereo).
/// Uses hound for WAV and ffmpeg CLI for everything else.
pub fn export_audio(
    path: &Path,
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
    format: &str,
    channels: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let frames = left.len().min(right.len());
    if frames == 0 {
        return Ok(());
    }

    if format == "wav" {
        return export_wav(path, left, right, sample_rate, channels);
    }

    // Write temp WAV, then convert with ffmpeg
    let temp_path = path.with_extension("tmp.wav");
    export_wav(&temp_path, left, right, sample_rate, channels)?;

    let codec = match format {
        "flac" => "flac",
        "ogg" | "oga" => "libvorbis",
        "mp3" => "libmp3lame",
        _ => "copy",
    };

    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            temp_path.to_str().unwrap_or("temp.wav"),
            "-c:a",
            codec,
            path.to_str().unwrap_or("output"),
        ])
        .status()?;

    let _ = std::fs::remove_file(&temp_path);

    if !status.success() {
        return Err("ffmpeg conversion failed".into());
    }
    Ok(())
}

fn export_wav(
    path: &Path,
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
    channels: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let frames = left.len().min(right.len());
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for i in 0..frames {
        if channels == 1 {
            let mono = (left[i] + right[i]) * 0.5;
            writer.write_sample(mono)?;
        } else {
            writer.write_sample(left[i])?;
            writer.write_sample(right[i])?;
        }
    }
    writer.finalize()?;
    Ok(())
}

/// Export an SFZ descriptor referencing an audio file.
pub fn export_sfz(
    sfz_path: &Path,
    sample_path: &str,
    midi_note: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = format!("<region> sample={sample_path} key={midi_note}\n");
    std::fs::write(sfz_path, content)?;
    Ok(())
}

/// Audio decode result: (left_channel, right_channel, sample_rate).
pub type AudioDecodeResult = Result<(Vec<f32>, Vec<f32>, u32), Box<dyn std::error::Error>>;

/// Decode any audio file to interleaved stereo f32 using ffmpeg CLI.
pub fn decode_audio_to_f32(path: &Path) -> AudioDecodeResult {
    let temp_path = path.with_extension("tmp_decoded.wav");

    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            path.to_str().unwrap_or(""),
            "-ac",
            "2",
            "-ar",
            "48000",
            "-c:a",
            "pcm_f32le",
            temp_path.to_str().unwrap_or("temp.wav"),
        ])
        .status()?;

    if !status.success() {
        return Err("ffmpeg decode failed".into());
    }

    let mut reader = hound::WavReader::open(&temp_path)?;
    let sample_rate = reader.spec().sample_rate;
    let samples: Vec<f32> = reader.samples::<f32>().filter_map(|s| s.ok()).collect();
    let _ = std::fs::remove_file(&temp_path);

    let mut left = Vec::with_capacity(samples.len() / 2);
    let mut right = Vec::with_capacity(samples.len() / 2);
    for chunk in samples.chunks_exact(2) {
        left.push(chunk[0]);
        right.push(chunk[1]);
    }

    Ok((left, right, sample_rate))
}
