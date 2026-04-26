use std::path::Path;

#[derive(Debug, Clone)]
pub struct AudioFile {
    pub path: String,
    pub sample_rate: u32,
    pub data: Vec<f32>,
    pub channels: u16,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("WAV error: {0}")]
    Wav(#[from] hound::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl AudioFile {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let path = path.as_ref();
        let reader = hound::WavReader::open(path)?;
        let spec = reader.spec();
        let channels = spec.channels;
        let sample_rate = spec.sample_rate;

        let mut data = Vec::new();
        match spec.sample_format {
            hound::SampleFormat::Float => {
                for sample in reader.into_samples::<f32>() {
                    data.push(sample?);
                }
            }
            hound::SampleFormat::Int => {
                let bits = spec.bits_per_sample;
                let max_val = ((1_i64 << (bits - 1)) as f32);
                for sample in reader.into_samples::<i32>() {
                    data.push(sample? as f32 / max_val);
                }
            }
        }

        Ok(Self {
            path: path.display().to_string(),
            sample_rate,
            data,
            channels,
        })
    }

    /// Read a single frame (interleaved) at the given position.
    pub fn frame(&self, pos: usize) -> &[f32] {
        let start = pos * self.channels as usize;
        let end = start + self.channels as usize;
        &self.data[start.min(self.data.len())..end.min(self.data.len())]
    }

    pub fn num_frames(&self) -> usize {
        self.data.len() / self.channels as usize
    }
}
