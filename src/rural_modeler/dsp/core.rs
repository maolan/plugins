pub fn amp_to_power_db(power: f32) -> f32 {
    10.0 * power.max(1.0e-12).log10()
}

pub const NAM_DEFAULT_MAX_BUFFER_SIZE: usize = 4096;

pub trait Dsp {
    fn process_block(&mut self, input: &[f32], output: &mut [f32]);

    fn reset(&mut self);

    fn prewarm(&mut self, samples: usize) {
        if samples == 0 {
            return;
        }
        let buffer_size = self.max_buffer_size().max(1);
        let input = vec![0.0f32; buffer_size];
        let mut output = vec![0.0f32; buffer_size];
        let mut processed = 0;
        while processed < samples {
            let block = buffer_size.min(samples - processed);
            self.process_block(&input[..block], &mut output[..block]);
            processed += block;
        }
    }

    fn reset_and_prewarm(&mut self, sample_rate: f32, max_buffer_size: usize) {
        self.set_external_sample_rate(sample_rate);
        self.set_max_buffer_size(max_buffer_size);
        self.reset();
    }

    fn process_sample(&mut self, input: f32) -> f32 {
        let mut out = [0.0f32];
        self.process_block(&[input], &mut out);
        out[0]
    }

    fn expected_sample_rate(&self) -> Option<f32>;

    fn external_sample_rate(&self) -> Option<f32> {
        None
    }

    fn set_external_sample_rate(&mut self, _rate: f32) {}

    fn num_input_channels(&self) -> usize;

    fn num_output_channels(&self) -> usize;

    fn max_buffer_size(&self) -> usize {
        NAM_DEFAULT_MAX_BUFFER_SIZE
    }

    fn set_max_buffer_size(&mut self, _size: usize) {}

    fn loudness(&self) -> Option<f32> {
        None
    }

    fn input_level(&self) -> Option<f32> {
        None
    }

    fn output_level(&self) -> Option<f32> {
        None
    }

    fn set_loudness(&mut self, _loudness: f32) {}

    fn set_input_level(&mut self, _level: f32) {}

    fn set_output_level(&mut self, _level: f32) {}
}

pub fn disable_denormals() -> DenormalGuard {
    DenormalGuard::new()
}

pub struct DenormalGuard {
    #[cfg(target_arch = "x86_64")]
    old_csr: u32,
}

impl DenormalGuard {
    fn new() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            let old_csr = unsafe {
                let mut mxcsr: u32 = 0;
                std::arch::asm!("stmxcsr [{0}]", in(reg) &mut mxcsr, options(nostack, preserves_flags));
                mxcsr
            };
            const FTZ: u32 = 0x8000;
            const DAZ: u32 = 0x0040;
            let new_csr = old_csr | FTZ | DAZ;
            unsafe {
                std::arch::asm!("ldmxcsr [{0}]", in(reg) &new_csr, options(nostack, preserves_flags));
            }
            Self { old_csr }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            Self {}
        }
    }
}

impl Drop for DenormalGuard {
    fn drop(&mut self) {
        #[cfg(target_arch = "x86_64")]
        {
            unsafe {
                std::arch::asm!("ldmxcsr [{0}]", in(reg) &self.old_csr, options(nostack, preserves_flags));
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Buffer {
    receptive_field: usize,
    input_buffer: Vec<f32>,
    input_buffer_offset: usize,
}

impl Buffer {
    pub fn new(receptive_field: usize) -> Self {
        let size = 32 * receptive_field.max(1);
        Self {
            receptive_field,
            input_buffer: vec![0.0; size],
            input_buffer_offset: receptive_field,
        }
    }

    pub fn reset(&mut self) {
        self.input_buffer.fill(0.0);
        self.input_buffer_offset = self.receptive_field;
    }

    pub fn update_buffers(&mut self, input: &[f32]) {
        let num_frames = input.len();
        let min_size = self.receptive_field + 32 * num_frames;
        if self.input_buffer.len() < min_size {
            let mut new_size = self.input_buffer.len().max(1);
            while new_size < min_size {
                new_size *= 2;
            }
            self.input_buffer.resize(new_size, 0.0);
        }

        if self.input_buffer_offset + num_frames >= self.input_buffer.len() {
            self.rewind_buffers();
        }

        self.input_buffer[self.input_buffer_offset..self.input_buffer_offset + num_frames]
            .copy_from_slice(input);
    }

    pub fn rewind_buffers(&mut self) {
        let rf = self.receptive_field;
        let offset = self.input_buffer_offset;
        for i in 0..rf {
            self.input_buffer[i] = self.input_buffer[offset - rf + i];
        }
        self.input_buffer_offset = rf;
    }

    pub fn advance(&mut self, num_frames: usize) {
        self.input_buffer_offset += num_frames;
    }

    pub fn history_slice(&self, output_index: usize) -> &[f32] {
        let base = self.input_buffer_offset - self.receptive_field + 1 + output_index;
        &self.input_buffer[base..base + self.receptive_field]
    }
}

#[derive(Debug, Clone)]
pub struct SampleRing {
    capacity: usize,
    channels: usize,
    data: Vec<f32>,
    next_frame: usize,
}

impl SampleRing {
    #[inline]
    pub fn new(channels: usize, capacity: usize) -> Self {
        Self {
            capacity,
            channels,
            data: vec![0.0; channels * capacity],
            next_frame: 0,
        }
    }

    #[inline]
    pub fn reset(&mut self) {
        self.data.fill(0.0);
        self.next_frame = 0;
    }

    #[inline]
    pub fn push(&mut self, sample: &[f32]) {
        if self.capacity == 0 || self.channels == 0 {
            return;
        }

        let start = self.next_frame * self.channels;
        let end = start + self.channels;
        if sample.len() == self.channels {
            self.data[start..end].copy_from_slice(sample);
        } else {
            let dst = &mut self.data[start..end];
            dst.fill(0.0);
            let copy_len = dst.len().min(sample.len());
            dst[..copy_len].copy_from_slice(&sample[..copy_len]);
        }
        self.next_frame = (self.next_frame + 1) % self.capacity;
    }

    #[inline]
    pub fn get_delay(&self, delay: usize) -> &[f32] {
        if self.capacity == 0 || self.channels == 0 {
            return &[];
        }
        let delay = delay.min(self.capacity.saturating_sub(1));
        let frame = (self.next_frame + self.capacity - 1 - delay) % self.capacity;
        let start = frame * self.channels;
        let end = start + self.channels;
        if end > self.data.len() {
            return &[];
        }
        &self.data[start..end]
    }
}

#[cfg(test)]
mod tests {
    use super::Buffer;

    #[test]
    fn buffer_rewinds_when_write_reaches_end_capacity() {
        let mut buffer = Buffer::new(2);

        let block = vec![0.0f32; 62];
        buffer.update_buffers(&block);
        buffer.advance(block.len());
        buffer.update_buffers(&[1.0]);
        assert!(
            std::panic::catch_unwind(|| buffer.history_slice(0)).is_ok(),
            "reading history after rewind-at-capacity should not panic"
        );
    }
}
