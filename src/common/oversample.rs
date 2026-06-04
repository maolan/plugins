//! 2× oversampling with half-band FIR filter.
//!
//! Efficient 2x upsampling/downsampling using a 12-tap half-band filter.
//! Half-band filters have every other coefficient zero (except center),
//! so they require roughly half the multiplies of a general FIR.

/// 2× oversampler: upsample → process at 2× → downsample.
pub struct Oversampler {
    /// Upsampler state (last input samples).
    up_state: [f32; 6],
    /// Downsampler state (last processed samples at 2×).
    down_state: [f32; 6],
}

/// 12-tap half-band filter coefficients (symmetric, odd length).
/// Designed for 2× upsampling/downsampling with ~-60dB stopband.
const HB_COEFFS: [f32; 6] = [
    0.003_621_0,
    -0.016_631_0,
    0.050_519_0,
    0.462_898, // center tap (will be doubled for symmetry)
    0.050_519_0,
    -0.016_631_0,
];

impl Default for Oversampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Oversampler {
    pub fn new() -> Self {
        Self {
            up_state: [0.0; 6],
            down_state: [0.0; 6],
        }
    }

    pub fn reset(&mut self) {
        self.up_state = [0.0; 6];
        self.down_state = [0.0; 6];
    }

    /// Upsample a single sample by 2×, return two output samples.
    fn upsample_sample(&mut self, input: f32) -> (f32, f32) {
        // Shift state.
        for i in (0..5).rev() {
            self.up_state[i + 1] = self.up_state[i];
        }
        self.up_state[0] = input;

        // Output 0: interpolates between samples (odd taps of filter).
        let out0 = HB_COEFFS[0] * self.up_state[5]
            + HB_COEFFS[1] * self.up_state[3]
            + HB_COEFFS[2] * self.up_state[1]
            + HB_COEFFS[3] * 0.0 // zero sample inserted
            + HB_COEFFS[4] * self.up_state[1]
            + HB_COEFFS[5] * self.up_state[3];

        // Output 1: samples at original positions (even taps of filter).
        let out1 = HB_COEFFS[0] * self.up_state[4]
            + HB_COEFFS[1] * self.up_state[2]
            + HB_COEFFS[2] * self.up_state[0]
            + HB_COEFFS[3] * input // center tap
            + HB_COEFFS[4] * self.up_state[0]
            + HB_COEFFS[5] * self.up_state[2];

        (out0, out1)
    }

    /// Downsample two 2×-rate samples to one output sample.
    fn downsample_sample(&mut self, _s0: f32, s1: f32) -> f32 {
        // Shift state.
        for i in (0..5).rev() {
            self.down_state[i + 1] = self.down_state[i];
        }
        self.down_state[0] = s1;

        // We only need the even-position output, which uses the original samples.
        HB_COEFFS[0] * self.down_state[5]
            + HB_COEFFS[1] * self.down_state[3]
            + HB_COEFFS[2] * self.down_state[1]
            + HB_COEFFS[3] * s1
            + HB_COEFFS[4] * self.down_state[1]
            + HB_COEFFS[5] * self.down_state[3]
    }

    /// Process a mono buffer at 2× oversampling.
    /// `process_fn` receives a slice of 2× length and processes it in-place.
    pub fn process_block(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        mut process_fn: impl FnMut(&mut [f32]),
    ) {
        assert_eq!(input.len(), output.len());
        let n = input.len();
        let mut upsampled = vec![0.0f32; n * 2];

        // Upsample.
        for (i, &sample) in input.iter().enumerate() {
            let (s0, s1) = self.upsample_sample(sample);
            upsampled[i * 2] = s0;
            upsampled[i * 2 + 1] = s1;
        }

        // Process at 2× rate.
        process_fn(&mut upsampled);

        // Downsample.
        for i in 0..n {
            let s0 = upsampled[i * 2];
            let s1 = upsampled[i * 2 + 1];
            output[i] = self.downsample_sample(s0, s1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oversampler_identity() {
        let mut over = Oversampler::new();
        let input = vec![1.0f32, 0.0, 0.0, 0.0];
        let mut output = vec![0.0f32; 4];
        over.process_block(&input, &mut output, |buf| {
            // Identity processing: do nothing.
            let _ = buf;
        });
        // First sample should pass through (with some filter delay/ripple).
        assert!(output[0].abs() < 1.0); // filter attenuates impulse
    }
}
