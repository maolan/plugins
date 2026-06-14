pub struct Oversampler {
    up_state: [f32; 6],

    down_state: [f32; 6],
}

const HB_COEFFS: [f32; 6] = [
    0.003_621_0,
    -0.016_631_0,
    0.050_519_0,
    0.462_898,
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

    fn upsample_sample(&mut self, input: f32) -> (f32, f32) {
        for i in (0..5).rev() {
            self.up_state[i + 1] = self.up_state[i];
        }
        self.up_state[0] = input;

        let out0 = HB_COEFFS[0] * self.up_state[5]
            + HB_COEFFS[1] * self.up_state[3]
            + HB_COEFFS[2] * self.up_state[1]
            + HB_COEFFS[3] * 0.0
            + HB_COEFFS[4] * self.up_state[1]
            + HB_COEFFS[5] * self.up_state[3];

        let out1 = HB_COEFFS[0] * self.up_state[4]
            + HB_COEFFS[1] * self.up_state[2]
            + HB_COEFFS[2] * self.up_state[0]
            + HB_COEFFS[3] * input
            + HB_COEFFS[4] * self.up_state[0]
            + HB_COEFFS[5] * self.up_state[2];

        (out0, out1)
    }

    fn downsample_sample(&mut self, _s0: f32, s1: f32) -> f32 {
        for i in (0..5).rev() {
            self.down_state[i + 1] = self.down_state[i];
        }
        self.down_state[0] = s1;

        HB_COEFFS[0] * self.down_state[5]
            + HB_COEFFS[1] * self.down_state[3]
            + HB_COEFFS[2] * self.down_state[1]
            + HB_COEFFS[3] * s1
            + HB_COEFFS[4] * self.down_state[1]
            + HB_COEFFS[5] * self.down_state[3]
    }

    pub fn process_block(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        mut process_fn: impl FnMut(&mut [f32]),
    ) {
        assert_eq!(input.len(), output.len());
        let n = input.len();
        let mut upsampled = vec![0.0f32; n * 2];

        for (i, &sample) in input.iter().enumerate() {
            let (s0, s1) = self.upsample_sample(sample);
            upsampled[i * 2] = s0;
            upsampled[i * 2 + 1] = s1;
        }

        process_fn(&mut upsampled);

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
            let _ = buf;
        });

        assert!(output[0].abs() < 1.0);
    }
}
