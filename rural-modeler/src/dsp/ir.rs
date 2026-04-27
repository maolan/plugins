use crate::dsp::core::Buffer;

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
        let mut reader =
            hound::WavReader::open(path).map_err(|err| format!("failed to open IR wav: {err}"))?;
        let spec = reader.spec();
        let source_sr = spec.sample_rate as f32;
        let channels = spec.channels as usize;
        if channels != 1 {
            return Err(format!(
                "IR file '{path}' is not mono ({} channels). Only mono IRs are supported.",
                channels
            ));
        }
        // Align with NAM's WAV support matrix used for IR loading:
        // - IEEE float: 32-bit only
        // - PCM int: 16, 24, or 32-bit only
        match (spec.sample_format, spec.bits_per_sample) {
            (hound::SampleFormat::Float, 32) => {}
            (hound::SampleFormat::Int, 16 | 24 | 32) => {}
            (hound::SampleFormat::Float, bits) => {
                return Err(format!(
                    "unsupported float WAV bit depth: {bits} (expected 32-bit float)"
                ));
            }
            (hound::SampleFormat::Int, bits) => {
                return Err(format!(
                    "unsupported PCM WAV bit depth: {bits} (expected 16/24/32-bit PCM)"
                ));
            }
        }

        let mut mono = Vec::new();
        match spec.sample_format {
            hound::SampleFormat::Float => {
                let mut frame = Vec::with_capacity(channels);
                for sample in reader.samples::<f32>() {
                    frame.push(sample.map_err(|err| format!("failed reading IR samples: {err}"))?);
                    if frame.len() == channels {
                        let avg = frame.iter().copied().sum::<f32>() / channels as f32;
                        mono.push(avg);
                        frame.clear();
                    }
                }
            }
            hound::SampleFormat::Int => {
                let scale = (1_i64 << (spec.bits_per_sample.saturating_sub(1) as u32)) as f32;
                let mut frame = Vec::with_capacity(channels);
                for sample in reader.samples::<i32>() {
                    frame.push(
                        sample.map_err(|err| format!("failed reading IR samples: {err}"))? as f32
                            / scale.max(1.0),
                    );
                    if frame.len() == channels {
                        let avg = frame.iter().copied().sum::<f32>() / channels as f32;
                        mono.push(avg);
                        frame.clear();
                    }
                }
            }
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

    /// Recompute weights when the host sample rate changes.
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
                // C++ pads with a zero at the start and end before resampling.
                // This ensures the first sample of the raw audio is included in
                // the output and the interpolation tails off to zero naturally.
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
        for (dst, src) in self
            .weights
            .iter_mut()
            .rev()
            .zip(resampled.iter().take(ir_length))
        {
            *dst = gain * *src;
        }
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
            let mut sum = 0.0;
            for (j, weight) in self.weights.iter().enumerate() {
                sum += *weight * buffer.get(i as isize - j as isize);
            }
            *out = sum;
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
    let mut time = time_increment; // Start at second sample (cubic needs boundary)
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
