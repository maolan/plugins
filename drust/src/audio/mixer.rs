/// Simple channel mixer that accumulates samples into output buffers.
#[derive(Debug, Default)]
pub struct ChannelMixer {
    pub buffers: Vec<Vec<f32>>,
}

impl ChannelMixer {
    pub fn new(num_channels: usize, max_frames: usize) -> Self {
        let mut buffers = Vec::with_capacity(num_channels);
        for _ in 0..num_channels {
            buffers.push(vec![0.0f32; max_frames]);
        }
        Self { buffers }
    }

    pub fn clear(&mut self, frames: usize) {
        for buf in &mut self.buffers {
            if buf.len() < frames {
                buf.resize(frames, 0.0);
            }
            buf[..frames].fill(0.0);
        }
    }

    pub fn add(&mut self, channel: usize, offset: usize, samples: &[f32], gain: f32) {
        if channel >= self.buffers.len() {
            return;
        }
        let buf = &mut self.buffers[channel];
        for (i, &s) in samples.iter().enumerate() {
            let idx = offset + i;
            if idx < buf.len() {
                buf[idx] += s * gain;
            }
        }
    }

    pub fn buffer(&self, channel: usize) -> Option<&[f32]> {
        self.buffers.get(channel).map(|b| b.as_slice())
    }
}
