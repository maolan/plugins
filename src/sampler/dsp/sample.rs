use std::path::Path;
use std::sync::Arc;

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

    fn read_zoh(&self, phase: f64) -> (f32, f32) {
        let idx = phase.round() as usize;
        let idx = idx.min(self.frames.saturating_sub(1));
        (self.data_l[idx], self.data_r[idx])
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
        let center = phase;
        let half_window = 8;
        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;
        let mut weight_sum = 0.0f32;

        for i in -half_window..=half_window {
            let idx_f = center + i as f64;
            let idx = idx_f as usize;
            if idx >= frames {
                continue;
            }
            let t = (center - idx_f) as f32;
            let weight = if t.abs() < 1e-6 {
                1.0
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
    a0 - a1 * (std::f32::consts::PI * x).cos() + a2 * (2.0 * std::f32::consts::PI * x).cos()
        - a3 * (3.0 * std::f32::consts::PI * x).cos()
}

#[derive(Debug)]
pub enum LoadError {
    Ffmpeg(String),
    NoAudioStream,
    EmptySample,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Ffmpeg(e) => write!(f, "ffmpeg error: {e}"),
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

pub fn load_audio(path: &Path) -> Result<Arc<Sample>, LoadError> {
    let _ = ffmpeg_next::init();

    let mut input =
        ffmpeg_next::format::input(path).map_err(|e| LoadError::Ffmpeg(e.to_string()))?;

    let stream = input
        .streams()
        .best(ffmpeg_next::media::Type::Audio)
        .ok_or(LoadError::NoAudioStream)?;
    let stream_index = stream.index();
    let rate = stream.rate();
    let sample_rate = rate.numerator() as f32 / rate.denominator().max(1) as f32;

    let context = ffmpeg_next::codec::Context::from_parameters(stream.parameters())
        .map_err(|e| LoadError::Ffmpeg(e.to_string()))?;
    let mut decoder = context
        .decoder()
        .audio()
        .map_err(|e| LoadError::Ffmpeg(e.to_string()))?;

    let target_format = ffmpeg_next::format::Sample::F32(ffmpeg_next::format::sample::Type::Planar);
    let target_layout = ffmpeg_next::util::channel_layout::ChannelLayout::STEREO;

    let mut resampler = ffmpeg_next::software::resampling::context::Context::get(
        decoder.format(),
        decoder.channel_layout(),
        decoder.rate(),
        target_format,
        target_layout,
        decoder.rate(),
    )
    .map_err(|e| LoadError::Ffmpeg(e.to_string()))?;

    let mut data_l: Vec<f32> = Vec::new();
    let mut data_r: Vec<f32> = Vec::new();

    let mut decoded = ffmpeg_next::util::frame::audio::Audio::empty();
    let mut resampled = ffmpeg_next::util::frame::audio::Audio::empty();

    for (stream, packet) in input.packets() {
        if stream.index() != stream_index {
            continue;
        }
        decoder
            .send_packet(&packet)
            .map_err(|e| LoadError::Ffmpeg(e.to_string()))?;
        while decoder.receive_frame(&mut decoded).is_ok() {
            resampler
                .run(&decoded, &mut resampled)
                .map_err(|e| LoadError::Ffmpeg(e.to_string()))?;
            push_planar_f32(&resampled, &mut data_l, &mut data_r);
        }
    }

    decoder
        .send_eof()
        .map_err(|e| LoadError::Ffmpeg(e.to_string()))?;
    while decoder.receive_frame(&mut decoded).is_ok() {
        resampler
            .run(&decoded, &mut resampled)
            .map_err(|e| LoadError::Ffmpeg(e.to_string()))?;
        push_planar_f32(&resampled, &mut data_l, &mut data_r);
    }

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

fn push_planar_f32(frame: &ffmpeg_next::util::frame::Audio, l: &mut Vec<f32>, r: &mut Vec<f32>) {
    let channels = frame.channels() as usize;
    let samples = frame.samples();
    if channels == 0 || samples == 0 {
        return;
    }

    let plane_size = samples * std::mem::size_of::<f32>();

    if channels == 1 {
        let plane = frame.data(0);
        if plane.len() >= plane_size {
            let slice: &[f32] =
                unsafe { std::slice::from_raw_parts(plane.as_ptr() as *const f32, samples) };
            l.extend_from_slice(slice);
            r.extend_from_slice(slice);
        }
    } else {
        for (ch, buf) in [(0, l), (1, r)] {
            let plane = frame.data(ch);
            if plane.len() >= plane_size {
                let slice: &[f32] =
                    unsafe { std::slice::from_raw_parts(plane.as_ptr() as *const f32, samples) };
                buf.extend_from_slice(slice);
            }
        }
    }
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
}
