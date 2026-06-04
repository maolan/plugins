//! Wavetable loading and storage.
//!
//! Supports Surge XT's .wt file format:
//! - Tag: 'vawt' (big-endian text)
//! - wave_size: u32 LE (power of 2, 2-4096)
//! - wave_count: u16 LE (1-512)
//! - flags: u16 LE
//! - data: float32 or int16

use std::io::Read;

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
    /// [frame][sample] interleaved float data
    pub frames: Vec<Vec<f32>>,
    /// Mipmaps: [level][frame][sample]
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
    /// Load a .wt file from raw bytes.
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
                // Find null terminator
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

    /// Load from a file path.
    pub fn from_file(path: &str) -> Option<Self> {
        let mut file = std::fs::File::open(path).ok()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        Self::from_bytes(&bytes)
    }

    fn build_mipmaps(&mut self) {
        let mut levels = Vec::new();
        levels.push(self.frames.clone());

        let mut current_size = self.size;
        while current_size > 2 {
            current_size /= 2;
            let prev = &levels[levels.len() - 1];
            let mut level = Vec::with_capacity(self.n_tables);
            for (_frame_idx, prev_frame) in prev.iter().enumerate().take(self.n_tables) {
                let mut new_frame = vec![0.0f32; current_size];
                for i in 0..current_size {
                    new_frame[i] = (prev_frame[i * 2] + prev_frame[i * 2 + 1]) * 0.5;
                }
                level.push(new_frame);
            }
            levels.push(level);
        }

        self.mipmaps = levels;
    }

    /// Get the appropriate mipmap level for a given playback rate.
    /// `rate` is phase increment per sample (0..1).
    pub fn select_mipmap(&self, rate: f32) -> usize {
        let mut level = 0;
        let mut threshold = 0.5;
        while rate > threshold && level + 1 < self.mipmaps.len() {
            level += 1;
            threshold *= 0.5;
        }
        level
    }

    /// Read a sample from a specific frame and mipmap with linear interpolation.
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

    /// Read with 4-point cubic interpolation for higher quality.
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

        // Catmull-Rom cubic interpolation
        let frac2 = frac * frac;
        let frac3 = frac2 * frac;

        let a0 = -0.5 * y0 + 1.5 * y1 - 1.5 * y2 + 0.5 * y3;
        let a1 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let a2 = -0.5 * y0 + 0.5 * y2;
        let a3 = y1;

        a0 * frac3 + a1 * frac2 + a2 * frac + a3
    }

    /// Read with crossfade between two frames.
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

    #[test]
    fn test_default_wavetable() {
        let wt = Wavetable::default();
        assert_eq!(wt.size, 2048);
        assert_eq!(wt.n_tables, 1);
    }
}
