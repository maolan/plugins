use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpolationMode {
    Zoh = 0,

    Linear = 1,

    Sinc = 2,
}

impl InterpolationMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => InterpolationMode::Zoh,
            2 => InterpolationMode::Sinc,
            _ => InterpolationMode::Linear,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Sample {
    pub sample_rate: f32,
    pub data_l: Vec<f32>,
    pub data_r: Vec<f32>,
    pub frames: usize,

    pub peak: f32,

    pub rms: f32,
}

impl Sample {
    pub fn silent(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            data_l: vec![0.0],
            data_r: vec![0.0],
            frames: 1,
            peak: 0.0,
            rms: 0.0,
        }
    }

    pub fn read(&self, phase: f64, mode: InterpolationMode) -> (f32, f32) {
        match mode {
            InterpolationMode::Zoh => self.read_zoh(phase),
            InterpolationMode::Linear => self.read_linear(phase),
            InterpolationMode::Sinc => self.read_sinc(phase),
        }
    }

    pub fn read_with_increment(
        &self,
        phase: f64,
        increment: f64,
        mode: InterpolationMode,
    ) -> (f32, f32) {
        match mode {
            InterpolationMode::Zoh => self.read_zohaa(phase, increment),
            InterpolationMode::Linear => self.read_linear(phase),
            InterpolationMode::Sinc => self.read_sinc(phase),
        }
    }

    fn read_zoh(&self, phase: f64) -> (f32, f32) {
        let idx = phase.round() as usize;
        let idx = idx.min(self.frames.saturating_sub(1));
        (self.data_l[idx], self.data_r[idx])
    }

    fn read_zohaa(&self, phase: f64, increment: f64) -> (f32, f32) {
        let frames = self.frames;
        if frames == 0 {
            return (0.0, 0.0);
        }
        let inc = increment.abs();
        if inc <= 1.0 {
            return self.read_zoh(phase);
        }
        let start = phase.floor() as isize;
        let end = (phase + increment).floor() as isize;
        let (low, high) = if increment >= 0.0 {
            (start, end)
        } else {
            (end, start)
        };
        let mut sum_l = 0.0f64;
        let mut sum_r = 0.0f64;
        let mut count = 0usize;
        for i in low..=high {
            let idx = i.clamp(0, frames.saturating_sub(1) as isize) as usize;
            sum_l += self.data_l[idx] as f64;
            sum_r += self.data_r[idx] as f64;
            count += 1;
        }
        if count > 0 {
            let scale = 1.0 / count as f64;
            ((sum_l * scale) as f32, (sum_r * scale) as f32)
        } else {
            self.read_zoh(phase)
        }
    }

    pub fn read_linear(&self, phase: f64) -> (f32, f32) {
        let frames = self.frames;
        if frames == 0 {
            return (0.0, 0.0);
        }
        let idx = phase as usize;
        let frac = (phase - idx as f64) as f32;
        let idx0 = idx.min(frames - 1);
        let idx1 = (idx + 1).min(frames - 1);

        let l0 = self.data_l[idx0];
        let l1 = self.data_l[idx1];
        let r0 = self.data_r[idx0];
        let r1 = self.data_r[idx1];

        (l0 + (l1 - l0) * frac, r0 + (r1 - r0) * frac)
    }

    pub fn normalize_peak(&mut self, target_peak: f32) {
        if self.peak > 1e-10 {
            let scale = target_peak / self.peak;
            for s in &mut self.data_l {
                *s *= scale;
            }
            for s in &mut self.data_r {
                *s *= scale;
            }
            self.peak = target_peak;
            self.rms *= scale;
        }
    }

    pub fn normalize_rms(&mut self, target_rms: f32) {
        if self.rms > 1e-10 {
            let scale = target_rms / self.rms;
            for s in &mut self.data_l {
                *s *= scale;
            }
            for s in &mut self.data_r {
                *s *= scale;
            }
            self.peak *= scale;
            self.rms = target_rms;
        }
    }

    fn read_sinc(&self, phase: f64) -> (f32, f32) {
        let frames = self.frames;
        if frames == 0 {
            return (0.0, 0.0);
        }
        let half_window = 8;
        let base = phase.floor() as isize;
        let frac = (phase - base as f64) as f32;
        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;
        let mut weight_sum = 0.0f32;

        for i in -half_window..=half_window {
            let idx = base + i;
            if idx < 0 || idx >= frames as isize {
                continue;
            }
            let idx = idx as usize;
            let t = frac - i as f32;
            let weight = if t.abs() < 1e-6 {
                window_blackman_harris(0.0, half_window as f32)
            } else {
                let pi_t = std::f32::consts::PI * t;
                (pi_t.sin() / pi_t) * window_blackman_harris(i as f32, half_window as f32)
            };
            sum_l += self.data_l[idx] * weight;
            sum_r += self.data_r[idx] * weight;
            weight_sum += weight;
        }

        if weight_sum > 1e-6 {
            (sum_l / weight_sum, sum_r / weight_sum)
        } else {
            self.read_linear(phase)
        }
    }
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

fn compute_stats(data: &[f32]) -> (f32, f32) {
    let mut peak = 0.0f32;
    let mut sum_sq = 0.0f64;
    for &s in data {
        let abs = s.abs();
        if abs > peak {
            peak = abs;
        }
        sum_sq += (s as f64) * (s as f64);
    }
    let rms = ((sum_sq / data.len().max(1) as f64) as f32).sqrt();
    (peak, rms)
}

fn build_sample(
    sample_rate: f32,
    data_l: Vec<f32>,
    mut data_r: Vec<f32>,
) -> Result<Arc<Sample>, LoadError> {
    let frames = data_l.len();
    if frames == 0 {
        return Err(LoadError::EmptySample);
    }

    data_r.resize(frames, 0.0);

    let (peak_l, rms_l) = compute_stats(&data_l);
    let (peak_r, rms_r) = compute_stats(&data_r);
    let peak = peak_l.max(peak_r);
    let rms = (rms_l + rms_r) / 2.0;

    Ok(Arc::new(Sample {
        sample_rate,
        data_l,
        data_r,
        frames,
        peak,
        rms,
    }))
}

pub fn load_audio(path: &Path) -> Result<Arc<Sample>, LoadError> {
    decode_with_symphonia(path)
}

fn decode_with_symphonia(path: &Path) -> Result<Arc<Sample>, LoadError> {
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

    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1);
    if channels == 0 {
        return Err(LoadError::NoAudioStream);
    }
    if channels > 2 {
        return Err(LoadError::Decode(format!(
            "unsupported channel count: {channels}"
        )));
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
        buf.copy_planar_ref(decoded);
        interleaved.extend_from_slice(buf.samples());
    }

    if interleaved.is_empty() {
        return Err(LoadError::EmptySample);
    }

    let mut data_l = Vec::with_capacity(interleaved.len() / channels.max(1));
    let mut data_r = Vec::with_capacity(interleaved.len() / channels.max(1));

    if channels == 1 {
        for s in interleaved {
            data_l.push(s);
            data_r.push(s);
        }
    } else {
        for chunk in interleaved.chunks(2) {
            data_l.push(chunk[0]);
            data_r.push(chunk[1]);
        }
    }

    build_sample(sample_rate, data_l, data_r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_silent_sample() {
        let s = Sample::silent(48000.0);
        assert_eq!(s.frames, 1);
        assert_eq!(s.read(0.0, InterpolationMode::Linear), (0.0, 0.0));
    }

    #[test]
    fn test_linear_interpolation() {
        let s = Sample {
            sample_rate: 44100.0,
            data_l: vec![0.0, 1.0, 0.0],
            data_r: vec![0.0, 0.5, 0.0],
            frames: 3,
            peak: 1.0,
            rms: 0.0,
        };
        let (l, r) = s.read(0.5, InterpolationMode::Linear);
        assert!((l - 0.5).abs() < 0.001);
        assert!((r - 0.25).abs() < 0.001);
    }

    #[test]
    fn test_zoh_interpolation() {
        let s = Sample {
            sample_rate: 44100.0,
            data_l: vec![0.0, 1.0, 0.0],
            data_r: vec![0.0, 0.5, 0.0],
            frames: 3,
            peak: 1.0,
            rms: 0.0,
        };

        let (l, _r) = s.read(0.4, InterpolationMode::Zoh);
        assert_eq!(l, 0.0);

        let (l, _r) = s.read(0.6, InterpolationMode::Zoh);
        assert_eq!(l, 1.0);
    }

    #[test]
    fn test_zohaa_averages_window() {
        let s = Sample {
            sample_rate: 44100.0,
            data_l: vec![0.0, 1.0, 2.0, 3.0, 4.0],
            data_r: vec![0.0, 1.0, 2.0, 3.0, 4.0],
            frames: 5,
            peak: 4.0,
            rms: 0.0,
        };
        let (l, _r) = s.read_with_increment(0.0, 2.0, InterpolationMode::Zoh);
        assert!(
            (l - 1.0).abs() < 0.001,
            "expected average of 0,1,2, got {l}"
        );
    }

    #[test]
    fn test_zohaa_falls_back_to_zoh_for_small_increment() {
        let s = Sample {
            sample_rate: 44100.0,
            data_l: vec![0.0, 1.0, 0.0],
            data_r: vec![0.0, 0.5, 0.0],
            frames: 3,
            peak: 1.0,
            rms: 0.0,
        };
        let (l, _r) = s.read_with_increment(0.6, 0.5, InterpolationMode::Zoh);
        assert_eq!(l, 1.0);
    }

    #[test]
    fn test_sinc_dc_preserve() {
        let s = Sample {
            sample_rate: 44100.0,
            data_l: vec![1.0; 64],
            data_r: vec![1.0; 64],
            frames: 64,
            peak: 1.0,
            rms: 1.0,
        };
        for phase in [0.0f64, 0.5, 10.5, 31.75] {
            let (l, r) = s.read(phase, InterpolationMode::Sinc);
            assert!((l - 1.0).abs() < 0.001, "sinc DC l at {phase}: {l}");
            assert!((r - 1.0).abs() < 0.001, "sinc DC r at {phase}: {r}");
        }
    }
}
