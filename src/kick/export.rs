use std::fs::File;
use std::io;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use oxideav_core::{
    AudioFrame, CodecId, CodecParameters, Frame, MediaType, Packet, RuntimeContext, SampleFormat,
    StreamInfo, TimeBase,
};

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

    match format {
        "wav" => export_wav(path, left, right, sample_rate, channels),
        "flac" => export_flac(path, left, right, sample_rate, channels),
        _ => Err(format!("unsupported export format: {format}").into()),
    }
}

fn interleave_stereo(left: &[f32], right: &[f32], channels: u16) -> Vec<f32> {
    let frames = left.len().min(right.len());
    let ch = channels as usize;
    let mut out = Vec::with_capacity(frames * ch);
    for i in 0..frames {
        if channels == 1 {
            out.push((left[i] + right[i]) * 0.5);
        } else {
            out.push(left[i]);
            out.push(right[i]);
        }
    }
    out
}

fn export_wav(
    path: &Path,
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
    channels: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let samples = interleave_stereo(left, right, channels);
    let bytes = pack_f32_to_bytes(&samples, SampleFormat::F32)?;

    let mut ctx = RuntimeContext::new();
    oxideav_basic::register(&mut ctx);

    let stream = audio_stream_info(
        "pcm_f32le",
        channels as usize,
        sample_rate,
        SampleFormat::F32,
        None,
    );
    let file = File::create(path)?;
    let output: Box<dyn oxideav_core::WriteSeek> = Box::new(file);
    let mut mux = ctx
        .containers
        .open_muxer("wav", output, std::slice::from_ref(&stream))
        .map_err(oxideav_err_to_io)?;
    mux.write_header().map_err(oxideav_err_to_io)?;
    let packet = Packet::new(0, TimeBase::new(1, sample_rate as i64), bytes);
    mux.write_packet(&packet).map_err(oxideav_err_to_io)?;
    mux.write_trailer().map_err(oxideav_err_to_io)?;
    Ok(())
}

fn export_flac(
    path: &Path,
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
    channels: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let samples = interleave_stereo(left, right, channels);
    let sample_format = SampleFormat::S32;
    let bytes = pack_f32_to_bytes(&samples, sample_format)?;

    let mut ctx = RuntimeContext::new();
    oxideav_flac::register(&mut ctx);

    let params = audio_codec_params("flac", channels as usize, sample_rate, sample_format, None);
    let mut enc = ctx
        .codecs
        .first_encoder(&params)
        .map_err(oxideav_err_to_io)?;

    let frame = AudioFrame {
        samples: (samples.len() / channels as usize) as u32,
        pts: Some(0),
        data: vec![bytes],
    };
    enc.send_frame(&Frame::Audio(frame))
        .map_err(oxideav_err_to_io)?;
    enc.flush().map_err(oxideav_err_to_io)?;

    let mut packets = Vec::new();
    loop {
        match enc.receive_packet() {
            Ok(p) => packets.push(p),
            Err(oxideav_core::Error::NeedMore) | Err(oxideav_core::Error::Eof) => break,
            Err(e) => return Err(oxideav_err_to_io(e).into()),
        }
    }

    let stream = StreamInfo {
        index: 0,
        time_base: TimeBase::new(1, sample_rate as i64),
        duration: None,
        start_time: Some(0),
        params: enc.output_params().clone(),
    };

    let file = File::create(path)?;
    let output: Box<dyn oxideav_core::WriteSeek> = Box::new(file);
    let mut mux = ctx
        .containers
        .open_muxer("flac", output, std::slice::from_ref(&stream))
        .map_err(oxideav_err_to_io)?;
    mux.write_header().map_err(oxideav_err_to_io)?;
    for pkt in &packets {
        mux.write_packet(pkt).map_err(oxideav_err_to_io)?;
    }
    mux.write_trailer().map_err(oxideav_err_to_io)?;
    Ok(())
}

fn pack_f32_to_bytes(samples: &[f32], format: SampleFormat) -> io::Result<Vec<u8>> {
    let bytes_per_sample = format.bytes_per_sample();
    let mut out = Vec::with_capacity(samples.len().saturating_mul(bytes_per_sample));
    for &sample in samples {
        let s = sample.clamp(-1.0, 1.0);
        match format {
            SampleFormat::S32 => {
                let scale = 8_388_607.0;
                let q = (s * scale).round().clamp(-8_388_608.0, 8_388_607.0) as i32;
                out.extend_from_slice(&q.to_le_bytes());
            }
            SampleFormat::F32 => {
                out.extend_from_slice(&s.to_le_bytes());
            }
            _ => {
                return Err(io::Error::other(format!(
                    "unsupported kick export sample format {format:?}"
                )));
            }
        }
    }
    Ok(out)
}

fn audio_codec_params(
    codec_id: &str,
    channels: usize,
    sample_rate: u32,
    sample_format: SampleFormat,
    bit_rate: Option<u64>,
) -> CodecParameters {
    let mut params = CodecParameters::audio(CodecId::new(codec_id));
    params.media_type = MediaType::Audio;
    params.channels = Some(channels as u16);
    params.sample_rate = Some(sample_rate);
    params.sample_format = Some(sample_format);
    if let Some(br) = bit_rate {
        params.bit_rate = Some(br);
    }
    params
}

fn audio_stream_info(
    codec_id: &str,
    channels: usize,
    sample_rate: u32,
    sample_format: SampleFormat,
    bit_rate: Option<u64>,
) -> StreamInfo {
    let params = audio_codec_params(codec_id, channels, sample_rate, sample_format, bit_rate);
    StreamInfo {
        index: 0,
        time_base: TimeBase::new(1, sample_rate as i64),
        duration: None,
        start_time: Some(0),
        params,
    }
}

fn oxideav_err_to_io(e: oxideav_core::Error) -> io::Error {
    io::Error::other(format!("OxideAV error: {e}"))
}

pub fn export_sfz(
    sfz_path: &Path,
    sample_path: &str,
    midi_note: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = format!("<region> sample={sample_path} key={midi_note}\n");
    std::fs::write(sfz_path, content)?;
    Ok(())
}

pub type AudioDecodeResult = Result<(Vec<f32>, Vec<f32>, u32), Box<dyn std::error::Error>>;

pub fn decode_audio_to_f32(path: &Path) -> AudioDecodeResult {
    decode_with_symphonia(path)
}

fn decode_with_symphonia(path: &Path) -> AudioDecodeResult {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();
    let decoder_opts = DecoderOptions::default();

    let probed =
        symphonia::default::get_probe().format(&hint, mss, &format_opts, &metadata_opts)?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .or_else(|| format.tracks().first())
        .ok_or("no usable audio track")?;

    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1);
    if channels == 0 {
        return Err("no audio stream".into());
    }
    if channels > 2 {
        return Err(format!("unsupported channel count: {channels}").into());
    }

    let sample_rate = track.codec_params.sample_rate.unwrap_or(48_000);
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &decoder_opts)?;

    let mut sample_buf = None;
    let mut interleaved = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => return Err(e.into()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = decoder.decode(&packet)?;

        if sample_buf.is_none() {
            let spec = *decoded.spec();
            sample_buf = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        }
        let buf = sample_buf.as_mut().unwrap();
        buf.copy_planar_ref(decoded);
        interleaved.extend_from_slice(buf.samples());
    }

    if interleaved.is_empty() {
        return Err("no samples decoded".into());
    }

    let mut left = Vec::with_capacity(interleaved.len() / channels.max(1));
    let mut right = Vec::with_capacity(interleaved.len() / channels.max(1));

    if channels == 1 {
        for s in interleaved {
            left.push(s);
            right.push(s);
        }
    } else {
        for chunk in interleaved.chunks(2) {
            left.push(chunk[0]);
            right.push(chunk[1]);
        }
    }

    Ok((left, right, sample_rate))
}
