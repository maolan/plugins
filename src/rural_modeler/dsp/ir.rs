use crate::rural_modeler::dsp::core::Buffer;
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(Debug, Clone, Default)]
pub struct ImpulseResponse {
    raw_samples: Vec<f32>,
    raw_sample_rate: f32,
    target_sample_rate: f32,
    weights: Vec<f32>,
    buffer: Option<Buffer>,
}

impl ImpulseResponse {
    pub fn from_wav(path: &str, target_sample_rate: f32) -> Result<Self, String> {
        let file = File::open(path).map_err(|err| format!("failed to open IR file: {err}"))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = Path::new(path).extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let format_opts = FormatOptions::default();
        let metadata_opts = MetadataOptions::default();
        let decoder_opts = DecoderOptions::default();

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &format_opts, &metadata_opts)
            .map_err(|err| format!("failed to probe IR file: {err}"))?;
        let mut format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .or_else(|| format.tracks().first())
            .ok_or_else(|| "no usable audio track in IR file".to_string())?;

        let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1);
        if channels == 0 {
            return Err("no audio stream in IR file".to_string());
        }

        let source_sr = track.codec_params.sample_rate.unwrap_or(48_000) as f32;
        let track_id = track.id;

        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &decoder_opts)
            .map_err(|err| format!("failed to create IR decoder: {err}"))?;

        let mut sample_buf = None;
        let mut interleaved = Vec::new();

        loop {
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(err) => return Err(format!("failed reading IR packets: {err}")),
            };

            if packet.track_id() != track_id {
                continue;
            }

            let decoded = decoder
                .decode(&packet)
                .map_err(|err| format!("failed decoding IR packet: {err}"))?;

            if sample_buf.is_none() {
                let spec = *decoded.spec();
                sample_buf = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
            }
            let buf = sample_buf.as_mut().unwrap();
            buf.copy_planar_ref(decoded);
            interleaved.extend_from_slice(buf.samples());
        }

        if interleaved.is_empty() {
            return Err("IR file contains no samples".to_string());
        }

        // Downmix to mono by averaging channels. If the file is already mono
        // this is a no-op.
        let mut mono = Vec::with_capacity(interleaved.len() / channels.max(1));
        for frame in interleaved.chunks(channels.max(1)) {
            let avg = frame.iter().copied().sum::<f32>() / channels.max(1) as f32;
            mono.push(avg);
        }

        let mut ir = Self {
            raw_samples: mono,
            raw_sample_rate: source_sr,
            target_sample_rate,
            weights: Vec::new(),
            buffer: None,
        };
        ir.rebuild_weights();
        ir.reset();
        Ok(ir)
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        if (self.target_sample_rate - sample_rate).abs() < f32::EPSILON {
            return;
        }
        self.target_sample_rate = sample_rate;
        self.rebuild_weights();
        self.reset();
    }

    fn rebuild_weights(&mut self) {
        let resampled = if (self.raw_sample_rate - self.target_sample_rate).abs() < f32::EPSILON {
            self.raw_samples.clone()
        } else {
            {
                let mut padded = Vec::with_capacity(self.raw_samples.len() + 2);
                padded.push(0.0);
                padded.extend_from_slice(&self.raw_samples);
                padded.push(0.0);
                resample_cubic(&padded, self.raw_sample_rate, self.target_sample_rate)
            }
        };

        let ir_length = resampled.len().min(8192);
        let gain = 10.0_f32.powf(-18.0 * 0.05) * 48_000.0 / self.target_sample_rate.max(1.0);
        self.weights.resize(ir_length, 0.0);
        crate::simd::copy_scaled_inplace(
            &mut self.weights[..ir_length],
            &resampled[..ir_length],
            gain,
        );
    }

    pub fn reset(&mut self) {
        self.buffer = if self.weights.is_empty() {
            None
        } else {
            Some(Buffer::new(self.weights.len()))
        };
    }

    pub fn process_block(&mut self, block: &mut [f32]) {
        let Some(buffer) = self.buffer.as_mut() else {
            return;
        };
        buffer.update_buffers(block);
        for (i, out) in block.iter_mut().enumerate() {
            let history = buffer.history_slice(i);
            *out = crate::simd::dot_product(&self.weights, history);
        }
        buffer.advance(block.len());
    }
}

fn resample_cubic(input: &[f32], src_rate: f32, dst_rate: f32) -> Vec<f32> {
    if input.is_empty() {
        return Vec::new();
    }
    if (src_rate - dst_rate).abs() < f32::EPSILON {
        return input.to_vec();
    }

    let time_increment = 1.0 / src_rate;
    let resampled_time_increment = 1.0 / dst_rate;
    let mut time = time_increment;
    let end_time = (input.len() - 1) as f32 * time_increment;

    let mut output = Vec::new();
    while time < end_time {
        let index = (time / time_increment).floor() as usize;
        let frac = (time - index as f32 * time_increment) / time_increment;

        let p0 = if index == 0 {
            input[0]
        } else {
            input[index - 1]
        };
        let p1 = input[index];
        let p2 = if index + 1 >= input.len() {
            input[input.len() - 1]
        } else {
            input[index + 1]
        };
        let p3 = if index + 2 >= input.len() {
            input[input.len() - 1]
        } else {
            input[index + 2]
        };

        let value = cubic_interpolate(p0, p1, p2, p3, frac);
        output.push(value);
        time += resampled_time_increment;
    }
    output
}

fn cubic_interpolate(p0: f32, p1: f32, p2: f32, p3: f32, x: f32) -> f32 {
    p1 + 0.5
        * x
        * (p2 - p0 + x * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + x * (3.0 * (p1 - p2) + p3 - p0)))
}
