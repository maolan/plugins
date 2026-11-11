#[derive(Debug, Clone, Copy)]
pub struct BitcrusherParams {
    pub bit_depth: f32,
    pub downsample: f32,
    pub drive_db: f32,
    pub dry_wet: f32,
    pub output_gain_db: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct Channel {
    held: f32,
    remaining: u32,
}

impl Channel {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn process(&mut self, input: f32, levels: f32, downsample: u32, drive: f32) -> f32 {
        if self.remaining == 0 {
            let driven = (input * drive).clamp(-1.0, 1.0);
            self.held = (driven * levels).round() / levels;
            self.remaining = downsample - 1;
        } else {
            self.remaining -= 1;
        }
        self.held
    }
}

#[derive(Debug, Clone, Default)]
pub struct Bitcrusher {
    left: Channel,
    right: Channel,
}

impl Bitcrusher {
    pub fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &BitcrusherParams,
    ) {
        let levels = quantization_levels(params.bit_depth);
        let downsample = params.downsample.round().clamp(1.0, 128.0) as u32;
        let drive = db_to_gain(params.drive_db);
        let output_gain = db_to_gain(params.output_gain_db);
        let mix = params.dry_wet.clamp(0.0, 1.0);
        self.left.remaining = self.left.remaining.min(downsample - 1);
        self.right.remaining = self.right.remaining.min(downsample - 1);

        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            let dry_left = *left;
            let dry_right = *right;
            let crushed_left = self.left.process(dry_left, levels, downsample, drive);
            let crushed_right = self.right.process(dry_right, levels, downsample, drive);
            *left = (crushed_left * mix + dry_left * (1.0 - mix)) * output_gain;
            *right = (crushed_right * mix + dry_right * (1.0 - mix)) * output_gain;
        }
    }

    pub fn process_mono(&mut self, buffer: &mut [f32], params: &BitcrusherParams) {
        let levels = quantization_levels(params.bit_depth);
        let downsample = params.downsample.round().clamp(1.0, 128.0) as u32;
        let drive = db_to_gain(params.drive_db);
        let output_gain = db_to_gain(params.output_gain_db);
        let mix = params.dry_wet.clamp(0.0, 1.0);
        self.left.remaining = self.left.remaining.min(downsample - 1);

        for sample in buffer {
            let dry = *sample;
            let crushed = self.left.process(dry, levels, downsample, drive);
            *sample = (crushed * mix + dry * (1.0 - mix)) * output_gain;
        }
    }
}

fn quantization_levels(bit_depth: f32) -> f32 {
    2.0_f32.powf(bit_depth.round().clamp(1.0, 24.0) - 1.0)
}

fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db * 0.05)
}

#[cfg(test)]
mod tests {
    use super::{Bitcrusher, BitcrusherParams};

    fn params() -> BitcrusherParams {
        BitcrusherParams {
            bit_depth: 8.0,
            downsample: 1.0,
            drive_db: 0.0,
            dry_wet: 1.0,
            output_gain_db: 0.0,
        }
    }

    #[test]
    fn quantizes_to_selected_bit_depth() {
        let mut crusher = Bitcrusher::default();
        let mut params = params();
        params.bit_depth = 2.0;
        let mut input = [0.3, -0.3, 0.8, -0.8];
        crusher.process_mono(&mut input, &params);
        assert_eq!(input, [0.5, -0.5, 1.0, -1.0]);
    }

    #[test]
    fn downsampling_holds_each_quantized_sample() {
        let mut crusher = Bitcrusher::default();
        let mut params = params();
        params.downsample = 3.0;
        let mut input = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6];
        crusher.process_mono(&mut input, &params);
        let first = input[0];
        assert_eq!(input[1], first);
        assert_eq!(input[2], first);
        assert_ne!(input[3], first);
        assert_eq!(input[4], input[3]);
        assert_eq!(input[5], input[3]);
    }

    #[test]
    fn dry_mix_preserves_input() {
        let mut crusher = Bitcrusher::default();
        let mut params = params();
        params.dry_wet = 0.0;
        let original = [0.1, -0.2, 0.3, -0.4];
        let mut input = original;
        crusher.process_mono(&mut input, &params);
        assert_eq!(input, original);
    }

    #[test]
    fn stereo_channels_hold_independently() {
        let mut crusher = Bitcrusher::default();
        let mut params = params();
        params.downsample = 3.0;
        let mut left = [0.25, 0.0, 0.0];
        let mut right = [-0.25, 0.0, 0.0];
        crusher.process_stereo(&mut left, &mut right, &params);
        assert!(left.iter().all(|sample| *sample > 0.0));
        assert!(right.iter().all(|sample| *sample < 0.0));
    }
}
