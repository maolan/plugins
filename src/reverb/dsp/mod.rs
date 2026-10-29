mod delay;
mod filters;
mod modes;
mod modulation;
mod network;
mod parameters;

use network::{Coefficients, Network};
use parameters::Controls;

use super::params::{PARAMS, ParamId};

pub struct Reverb {
    network: Network,
    sample_rate: f64,
    mix_gains: Option<[f32; 2]>,
}

impl Default for Reverb {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

impl Reverb {
    pub fn new(sample_rate: f64) -> Self {
        let sample_rate = valid_rate(sample_rate);
        Self {
            network: Network::new(sample_rate as f32),
            sample_rate,
            mix_gains: None,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        let sample_rate = valid_rate(sample_rate);
        if self.sample_rate != sample_rate {
            *self = Self::new(sample_rate);
        }
    }

    pub fn reset(&mut self) {
        self.network.reset();
        self.mix_gains = None;
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32], dry_wet: f64) {
        let mut values = PARAMS.map(|def| def.default);
        values[ParamId::DryWet.as_index()] = dry_wet;
        self.render(left, right, &values);
    }

    pub(crate) fn render(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        values: &[f64; ParamId::COUNT],
    ) {
        let controls = Controls::from_values(values);
        let target = Coefficients::new(controls, self.sample_rate as f32);
        let target_mix = mix_gains(controls.dry_wet);
        let mut mix = self.mix_gains.unwrap_or(target_mix);
        let smoothing = 1.0 - (-1000.0 / (self.sample_rate as f32 * 50.0)).exp();
        let frames = left.len().min(right.len());

        for (left, right) in left[..frames].iter_mut().zip(&mut right[..frames]) {
            let dry = [
                if left.is_finite() { *left } else { 0.0 },
                if right.is_finite() { *right } else { 0.0 },
            ];
            for (gain, target) in mix.iter_mut().zip(target_mix) {
                *gain += (target - *gain) * smoothing;
            }
            let wet = self.network.process(dry, &target, smoothing);
            *left = dry[0] * mix[1] + wet[0] * mix[0];
            *right = dry[1] * mix[1] + wet[1] * mix[0];
        }
        self.mix_gains = Some(mix);
    }
}

fn mix_gains(mix: f32) -> [f32; 2] {
    let angle = mix.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2;
    let (wet, dry) = angle.sin_cos();
    [
        if wet.abs() < 1e-7 { 0.0 } else { wet },
        if dry.abs() < 1e-7 { 0.0 } else { dry },
    ]
}

fn valid_rate(rate: f64) -> f64 {
    if rate.is_finite() {
        rate.clamp(8_000.0, 384_000.0).round()
    } else {
        48_000.0
    }
}
