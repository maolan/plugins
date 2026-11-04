use std::io::Read;
use std::path::Path;

use crate::common::audio_file::{AudioFile, LoadError, decode_file};

pub const MAX_WTABLE_SIZE: usize = 4096;
pub const MAX_SUBTABLES: usize = 512;
pub const MAX_MIPMAP_LEVELS: usize = 16;

#[derive(Debug, Clone)]
pub struct Wavetable {
    pub size: usize,
    pub n_tables: usize,
    pub size_po2: usize,
    pub flags: u16,
    pub dt: f32,
    pub is_sample: bool,
    pub is_loop: bool,
    pub is_int16: bool,
    pub is_full16: bool,
    pub has_metadata: bool,
    pub metadata: Option<String>,

    pub frames: Vec<Vec<f32>>,

    pub mipmaps: Vec<Vec<Vec<f32>>>,
}

impl Default for Wavetable {
    fn default() -> Self {
        Self {
            size: 2048,
            n_tables: 1,
            size_po2: 11,
            flags: 0,
            dt: 1.0 / 2048.0,
            is_sample: false,
            is_loop: false,
            is_int16: false,
            is_full16: false,
            has_metadata: false,
            metadata: None,
            frames: vec![vec![0.0; 2048]],
            mipmaps: Vec::new(),
        }
    }
}

impl Wavetable {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 12 {
            return None;
        }
        let tag = &bytes[0..4];
        if tag != b"vawt" {
            return None;
        }

        let wave_size = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        let wave_count = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
        let flags = u16::from_le_bytes([bytes[10], bytes[11]]);

        if !(2..=MAX_WTABLE_SIZE).contains(&wave_size) {
            return None;
        }
        if !(1..=MAX_SUBTABLES).contains(&wave_count) {
            return None;
        }
        if !wave_size.is_power_of_two() {
            return None;
        }

        let is_sample = (flags & 0x0001) != 0;
        let is_loop = (flags & 0x0002) != 0;
        let is_int16 = (flags & 0x0004) != 0;
        let is_full16 = (flags & 0x0008) != 0;
        let has_metadata = (flags & 0x0010) != 0;

        let data_offset = 12;
        let mut frames = Vec::with_capacity(wave_count);

        if is_int16 {
            let expected = data_offset + wave_size * wave_count * 2;
            if bytes.len() < expected {
                return None;
            }
            let scale = if is_full16 {
                1.0 / 32768.0
            } else {
                1.0 / 16384.0
            };
            for t in 0..wave_count {
                let mut frame = vec![0.0f32; wave_size];
                for (s, frame_slot) in frame.iter_mut().enumerate().take(wave_size) {
                    let idx = data_offset + (t * wave_size + s) * 2;
                    let val = i16::from_le_bytes([bytes[idx], bytes[idx + 1]]) as f32;
                    *frame_slot = val * scale;
                }
                frames.push(frame);
            }
        } else {
            let expected = data_offset + wave_size * wave_count * 4;
            if bytes.len() < expected {
                return None;
            }
            for t in 0..wave_count {
                let mut frame = vec![0.0f32; wave_size];
                for (s, frame_slot) in frame.iter_mut().enumerate().take(wave_size) {
                    let idx = data_offset + (t * wave_size + s) * 4;
                    *frame_slot = f32::from_le_bytes([
                        bytes[idx],
                        bytes[idx + 1],
                        bytes[idx + 2],
                        bytes[idx + 3],
                    ]);
                }
                frames.push(frame);
            }
        }

        let metadata = if has_metadata {
            let data_end = if is_int16 {
                data_offset + wave_size * wave_count * 2
            } else {
                data_offset + wave_size * wave_count * 4
            };
            if bytes.len() > data_end {
                let meta_bytes = &bytes[data_end..];

                let mut end = meta_bytes.len();
                for (i, &b) in meta_bytes.iter().enumerate() {
                    if b == 0 {
                        end = i;
                        break;
                    }
                }
                String::from_utf8(meta_bytes[..end].to_vec()).ok()
            } else {
                None
            }
        } else {
            None
        };

        let size_po2 = wave_size.trailing_zeros() as usize;
        let dt = 1.0 / wave_size as f32;

        let mut wt = Self {
            size: wave_size,
            n_tables: wave_count,
            size_po2,
            flags,
            dt,
            is_sample,
            is_loop,
            is_int16,
            is_full16,
            has_metadata,
            metadata,
            frames,
            mipmaps: Vec::new(),
        };

        wt.build_mipmaps();
        Some(wt)
    }

    pub fn from_file(path: &str) -> Option<Self> {
        let mut file = std::fs::File::open(path).ok()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        Self::from_bytes(&bytes)
    }

    /// Build a wavetable from decoded audio (any format `decode_file` handles).
    ///
    /// Multichannel files are mono-mixed. The audio is sliced into power-of-two
    /// tables of up to [`MAX_WTABLE_SIZE`] samples; a final partial slice is
    /// padded by looping the slice so the table remains a seamless cycle. Short
    /// files are looped into a single 2048-sample table. The result is peak
    /// normalized to 1.0 and marked as oscillator (not sample) data.
    pub fn from_audio_file(file: &AudioFile) -> Option<Self> {
        let frames_total = file.frames();
        if frames_total == 0 || file.channel_count() == 0 {
            return None;
        }

        let ch_count = file.channel_count() as f32;
        let mono: Vec<f32> = (0..frames_total)
            .map(|i| file.channels.iter().map(|ch| ch[i]).sum::<f32>() / ch_count)
            .collect();

        let size = if frames_total < 2048 {
            2048
        } else {
            MAX_WTABLE_SIZE
        };
        let n_tables = frames_total.div_ceil(size);
        if n_tables > MAX_SUBTABLES {
            return None;
        }

        let mut frames = Vec::with_capacity(n_tables);
        for t in 0..n_tables {
            let start = t * size;
            let slice_len = (frames_total - start).min(size);
            let mut table = vec![0.0f32; size];
            for (s, slot) in table.iter_mut().enumerate() {
                *slot = mono[start + (s % slice_len)];
            }
            frames.push(table);
        }

        let mut peak = 0.0f32;
        for table in &frames {
            for &s in table {
                peak = peak.max(s.abs());
            }
        }
        if peak > 1.0e-8 {
            let scale = 1.0 / peak;
            for table in &mut frames {
                for s in table.iter_mut() {
                    *s *= scale;
                }
            }
        }

        let mut wt = Self {
            size,
            n_tables,
            size_po2: size.trailing_zeros() as usize,
            flags: 0,
            dt: 1.0 / size as f32,
            is_sample: false,
            is_loop: true,
            is_int16: false,
            is_full16: false,
            has_metadata: false,
            metadata: None,
            frames,
            mipmaps: Vec::new(),
        };
        wt.build_mipmaps();
        Some(wt)
    }

    /// Decode an audio file with symphonia and build a wavetable from it.
    pub fn from_wav_file(path: &Path) -> Result<Self, LoadError> {
        let file = decode_file(path)?;
        Self::from_audio_file(&file).ok_or(LoadError::EmptySample)
    }

    /// Load a wavetable from any supported file: `.wt`/`.vawt` native format or
    /// a decodable audio file (wav/flac/mp3/...).
    pub fn from_file_any(path: &Path) -> Result<Self, LoadError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        match ext.as_deref() {
            Some("wt" | "vawt") => {
                let bytes = std::fs::read(path).map_err(|e| LoadError::Decode(e.to_string()))?;
                Self::from_bytes(&bytes).ok_or_else(|| {
                    LoadError::Decode(format!("invalid wavetable file {}", path.display()))
                })
            }
            _ => Self::from_wav_file(path),
        }
    }

    /// Build band-limited mipmaps by FFT brick-wall decimation: each level is
    /// the full-size spectrum truncated to the level's Nyquist and inverse
    /// transformed at the level size, so no harmonic that would alias at the
    /// corresponding playback rate survives (cf. Surge
    /// `WavetableOscillator.cpp` mipmapping).
    pub(crate) fn build_mipmaps(&mut self) {
        use rustfft::{FftPlanner, num_complex::Complex};

        let n = self.size;
        if n < 4 || !n.is_power_of_two() {
            // Fallback for degenerate sizes: naive 2-tap averaging.
            let mut levels = vec![self.frames.clone()];
            let mut current_size = n;
            while current_size > 2 {
                current_size /= 2;
                let prev = &levels[levels.len() - 1];
                let mut level = Vec::with_capacity(self.n_tables);
                for prev_frame in prev.iter().take(self.n_tables) {
                    let mut new_frame = vec![0.0f32; current_size];
                    for (i, slot) in new_frame.iter_mut().enumerate() {
                        *slot = (prev_frame[i * 2] + prev_frame[i * 2 + 1]) * 0.5;
                    }
                    level.push(new_frame);
                }
                levels.push(level);
            }
            self.mipmaps = levels;
            return;
        }

        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(n);
        let mut spectra: Vec<Vec<Complex<f32>>> = Vec::with_capacity(self.n_tables);
        let mut buf = vec![Complex { re: 0.0, im: 0.0 }; n];
        let scratch_len = fft.get_inplace_scratch_len();
        let mut scratch = vec![Complex { re: 0.0, im: 0.0 }; scratch_len];
        for frame in &self.frames {
            for (b, &s) in buf.iter_mut().zip(frame.iter()) {
                b.re = s;
                b.im = 0.0;
            }
            fft.process_with_scratch(&mut buf, &mut scratch);
            spectra.push(buf.clone());
        }

        let mut levels = vec![self.frames.clone()];
        let mut current_size = n;
        while current_size > 2 {
            current_size /= 2;
            let ifft = planner.plan_fft_inverse(current_size);
            let iscratch_len = ifft.get_inplace_scratch_len();
            if scratch.len() < iscratch_len {
                scratch.resize(iscratch_len, Complex { re: 0.0, im: 0.0 });
            }
            let mut level = Vec::with_capacity(self.n_tables);
            for spectrum in &spectra {
                // Keep the low harmonics (original bin indices) up to the
                // level's Nyquist and evaluate the truncated series at the
                // level's sample points.
                let mut y = vec![Complex { re: 0.0, im: 0.0 }; current_size];
                for (k, yk) in y.iter_mut().enumerate().take(current_size / 2 + 1) {
                    *yk = spectrum[k];
                }
                // Hermitian completion; the Nyquist bin must be real.
                for k in 1..current_size / 2 {
                    y[current_size - k] = y[k].conj();
                }
                y[current_size / 2].im = 0.0;
                ifft.process_with_scratch(&mut y, &mut scratch[..iscratch_len]);
                // rustfft's inverse transform is unnormalized; divide by the
                // ORIGINAL size so the level is the band-limited signal
                // resampled at the level's points (unit amplitude preserved).
                level.push(y.iter().map(|c| c.re / n as f32).collect());
            }
            levels.push(level);
        }

        self.mipmaps = levels;
    }

    /// Select the deepest mipmap whose full bandwidth still fits below
    /// Nyquist for the given playback phase increment (`freq / sample_rate`,
    /// already scaled by any formant factor). A level of size `M` supports
    /// `M / 2` harmonics, so it aliases only when `M / 2 < 1 / (2 * rate)`,
    /// i.e. `log2(rate * size)` levels of decimation are safe.
    pub fn select_mipmap(&self, rate: f32) -> usize {
        let max_level = self.mipmaps.len().saturating_sub(1) as i32;
        if rate <= 0.0 || max_level <= 0 {
            return 0;
        }
        let want = (rate * self.size as f32).log2().floor() as i32;
        want.clamp(0, max_level) as usize
    }

    pub fn read(&self, frame: usize, phase: f32, mipmap: usize) -> f32 {
        let mipmap = mipmap.min(self.mipmaps.len().saturating_sub(1));
        let frame = frame.min(self.n_tables.saturating_sub(1));
        let data = &self.mipmaps[mipmap][frame];
        let size = data.len();
        if size == 0 {
            return 0.0;
        }

        let phase = phase.fract().abs();
        let pos = phase * size as f32;
        let idx = pos as usize;
        let frac = pos - idx as f32;
        let idx2 = (idx + 1) % size;

        data[idx % size] * (1.0 - frac) + data[idx2] * frac
    }

    pub fn read_cubic(&self, frame: usize, phase: f32, mipmap: usize) -> f32 {
        let mipmap = mipmap.min(self.mipmaps.len().saturating_sub(1));
        let frame = frame.min(self.n_tables.saturating_sub(1));
        let data = &self.mipmaps[mipmap][frame];
        let size = data.len();
        if size == 0 {
            return 0.0;
        }

        let phase = phase.fract().abs();
        let pos = phase * size as f32;
        let idx = pos as usize;
        let frac = pos - idx as f32;

        let i0 = (idx + size - 1) % size;
        let i1 = idx % size;
        let i2 = (idx + 1) % size;
        let i3 = (idx + 2) % size;

        let y0 = data[i0];
        let y1 = data[i1];
        let y2 = data[i2];
        let y3 = data[i3];

        let frac2 = frac * frac;
        let frac3 = frac2 * frac;

        let a0 = -0.5 * y0 + 1.5 * y1 - 1.5 * y2 + 0.5 * y3;
        let a1 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let a2 = -0.5 * y0 + 0.5 * y2;
        let a3 = y1;

        a0 * frac3 + a1 * frac2 + a2 * frac + a3
    }

    pub fn read_morph(&self, frame: f32, phase: f32, mipmap: usize) -> f32 {
        let frame_a = frame as usize;
        let frame_b = (frame_a + 1).min(self.n_tables.saturating_sub(1));
        let frac = frame - frame_a as f32;

        let a = self.read_cubic(frame_a, phase, mipmap);
        let b = self.read_cubic(frame_b, phase, mipmap);
        a * (1.0 - frac) + b * frac
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_default_wavetable() {
        let wt = Wavetable::default();
        assert_eq!(wt.size, 2048);
        assert_eq!(wt.n_tables, 1);
    }

    fn sine_audio_file(frames: usize, cycles: usize) -> AudioFile {
        let data: Vec<f32> = (0..frames)
            .map(|i| {
                0.5 * (2.0 * std::f32::consts::PI * cycles as f32 * i as f32 / frames as f32).sin()
            })
            .collect();
        AudioFile {
            path: String::new(),
            sample_rate: 48_000.0,
            original_sample_rate: 48_000.0,
            channels: vec![data],
            source_channels: vec![0],
            peak: 0.5,
            rms: 0.35,
        }
    }

    #[test]
    fn from_audio_file_short_file_loops_to_2048() {
        let file = sine_audio_file(1000, 10);
        let wt = Wavetable::from_audio_file(&file).expect("wavetable");
        assert_eq!(wt.size, 2048);
        assert_eq!(wt.n_tables, 1);
        assert!(!wt.is_sample);
        assert!(wt.is_loop);
        assert_eq!(wt.frames.len(), 1);

        // Peak normalized to 1.0.
        let peak = wt.frames[0].iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!((peak - 1.0).abs() < 1.0e-4, "peak {peak}");

        // Reads are finite across the whole cycle.
        for i in 0..64 {
            let v = wt.read(0, i as f32 / 64.0, 0);
            assert!(v.is_finite(), "non-finite read at {i}");
        }
    }

    #[test]
    fn from_audio_file_long_file_sliced_into_tables() {
        let file = sine_audio_file(4096 * 3 + 500, 40);
        let wt = Wavetable::from_audio_file(&file).expect("wavetable");
        assert_eq!(wt.size, MAX_WTABLE_SIZE);
        assert_eq!(wt.n_tables, 4);
        for table in &wt.frames {
            assert!(table.iter().all(|s| s.is_finite()));
        }
    }

    #[test]
    fn from_audio_file_empty_is_none() {
        let file = sine_audio_file(0, 0);
        assert!(Wavetable::from_audio_file(&file).is_none());
    }

    fn table_wavetable(size: usize, table: Vec<f32>) -> Wavetable {
        Wavetable {
            size,
            n_tables: 1,
            size_po2: size.trailing_zeros() as usize,
            frames: vec![table],
            ..Default::default()
        }
    }

    #[test]
    fn mipmaps_are_fft_band_limited() {
        // A sine at 100 cycles per table has all its energy at bin 100.
        // Levels whose Nyquist bin is below 100 must not contain it.
        let size = 2048;
        let table: Vec<f32> = (0..size)
            .map(|i| (2.0 * std::f32::consts::PI * 100.0 * i as f32 / size as f32).sin())
            .collect();
        let mut wt = table_wavetable(size, table);
        wt.build_mipmaps();

        let peak_at = |level: usize| -> f32 {
            wt.mipmaps[level][0]
                .iter()
                .map(|s| s.abs())
                .fold(0.0f32, f32::max)
        };

        // Level 0 is the full-resolution frame.
        assert!((peak_at(0) - 1.0).abs() < 1.0e-4);
        // Size 256 (level 3): Nyquist bin 128 > 100, sine survives.
        assert!((peak_at(3) - 1.0).abs() < 1.0e-3, "peak {}", peak_at(3));
        // Size 128 (level 4): Nyquist bin 64 < 100, sine is brick-walled away.
        assert!(peak_at(4) < 1.0e-4, "peak {}", peak_at(4));
        // Deeper levels stay empty too.
        assert!(peak_at(wt.mipmaps.len() - 1) < 1.0e-4);
    }

    #[test]
    fn mipmap_low_harmonic_survives_all_levels() {
        // A fundamental-only table (4 cycles) must pass through every level
        // down to size 8 (Nyquist bin 4); only the size-2 and size-4 levels
        // may brick-wall it.
        let size = 2048;
        let table: Vec<f32> = (0..size)
            .map(|i| (2.0 * std::f32::consts::PI * 4.0 * i as f32 / size as f32).sin())
            .collect();
        let mut wt = table_wavetable(size, table);
        wt.build_mipmaps();
        for (level, mipmap) in wt
            .mipmaps
            .iter()
            .enumerate()
            .take(wt.mipmaps.len().saturating_sub(4) + 1)
        {
            let peak = mipmap[0].iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            assert!((peak - 1.0).abs() < 1.0e-3, "level {level} peak {peak}");
        }
    }

    #[test]
    fn select_mipmap_is_size_aware() {
        let size = 2048;
        let table: Vec<f32> = (0..size)
            .map(|i| (2.0 * std::f32::consts::PI * i as f32 / size as f32).sin())
            .collect();
        let mut wt = table_wavetable(size, table);
        wt.build_mipmaps();
        let max_level = wt.mipmaps.len() - 1;

        // Very low pitch: full-resolution table.
        assert_eq!(wt.select_mipmap(0.0005), 0);
        // Nyquist-safe deepest level for the given rate: log2(rate * size).
        assert_eq!(wt.select_mipmap(0.25), 9);
        // Rate above Nyquist of even the smallest table: clamp to deepest.
        assert_eq!(wt.select_mipmap(2.0), max_level);
        assert_eq!(wt.select_mipmap(0.0), 0);
    }

    #[test]
    fn vawt_round_trip() {
        let size = 256;
        let count = 3;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"vawt");
        bytes.extend_from_slice(&(size as u32).to_le_bytes());
        bytes.extend_from_slice(&(count as u16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        for t in 0..count {
            for s in 0..size {
                bytes.extend_from_slice(&((t * size + s) as f32).to_le_bytes());
            }
        }

        let wt = Wavetable::from_bytes(&bytes).expect("parse vawt");
        assert_eq!(wt.size, size);
        assert_eq!(wt.n_tables, count);
        assert!((wt.frames[2][10] - (2 * size + 10) as f32).abs() < 1.0e-6);
        assert!(!wt.mipmaps.is_empty());
    }

    #[test]
    fn from_wav_file_decodes_audio() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("wavetable-test-{nanos}.wav"));

        let frames = 512;
        let mut file = std::fs::File::create(&path).expect("create wav");
        let data_bytes = (frames * 2) as u32;
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + data_bytes).to_le_bytes()).unwrap();
        file.write_all(b"WAVE").unwrap();
        file.write_all(b"fmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&48_000u32.to_le_bytes()).unwrap();
        file.write_all(&(48_000u32 * 2).to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&data_bytes.to_le_bytes()).unwrap();
        for i in 0..frames {
            let v = (i16::MAX as f32
                * 0.25
                * (2.0 * std::f32::consts::PI * 8.0 * i as f32 / frames as f32).sin())
                as i16;
            file.write_all(&v.to_le_bytes()).unwrap();
        }
        drop(file);

        let wt = Wavetable::from_wav_file(&path).expect("decode wav");
        let _ = std::fs::remove_file(&path);
        assert_eq!(wt.size, 2048);
        assert_eq!(wt.n_tables, 1);
        assert!(wt.frames[0].iter().all(|s| s.is_finite()));
        assert!(wt.frames[0].iter().any(|s| s.abs() > 0.5));
    }
}
