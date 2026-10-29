use super::{
    delay::DelayLine,
    filters::{OnePole, lowpass_coefficient},
    modes::settings,
    modulation::Modulation,
    parameters::Controls,
};

const BASE_DELAYS_48K: [f32; 8] = [
    1423.0, 2390.0, 3791.0, 4497.0, 8154.0, 10937.0, 13488.0, 16444.0,
];
const DIFFUSION_SECONDS: [f32; 4] = [0.0031, 0.0059, 0.0107, 0.0173];
const EARLY_SECONDS: [f32; 4] = [0.0071, 0.0113, 0.0197, 0.0299];
const OUTPUT_TAPS: [[f32; 8]; 2] = [
    [-0.325, 0.325, 0.2433, -0.325, 0.325, -0.325, 0.2433, -0.325],
    [0.325, -0.325, -0.2433, 0.325, -0.325, 0.325, -0.2433, 0.325],
];

#[derive(Clone, Copy)]
pub(super) struct Coefficients {
    delays: [f32; 8],
    diffusion_delays: [f32; 8],
    early_delays: [f32; 8],
    mid_gain: [f32; 8],
    bass_gain: [f32; 8],
    predelay: f32,
    diffusion: f32,
    early_gain: f32,
    late_gain: f32,
    bass_filter: f32,
    high_filter: f32,
    high_shelf: f32,
    low_cut: f32,
    high_cut: f32,
    mod_rate: f32,
    mod_depth: f32,
    rotation_sin: f32,
    rotation_cos: f32,
    drive: f32,
    duration: f32,
    nonlinear: bool,
}

impl Coefficients {
    pub(super) fn new(values: Controls, sample_rate: f32) -> Self {
        let mode = settings(values.mode);
        let size_scale = (0.3 + 0.7 * values.size) * mode.delay_scale;
        let attack_scale = (28.0 + 255.0 * values.attack) / 283.0;
        let delay_scale = size_scale * attack_scale * sample_rate / 48_000.0;
        let delays = BASE_DELAYS_48K.map(|delay| (delay * delay_scale).max(32.0));
        let decay = (values.decay_seconds * mode.decay_scale).max(0.05);
        let mid_gain = delays.map(|delay| 0.001_f32.powf(delay / (sample_rate * decay)));
        let bass_gain = delays
            .map(|delay| 0.001_f32.powf(delay / (sample_rate * decay * values.bass_multiplier)));
        let diffusion_delays = std::array::from_fn(|index| {
            let channel = index / 4;
            let stage = index % 4;
            DIFFUSION_SECONDS[stage] * sample_rate * size_scale * (1.0 + channel as f32 * 0.071)
        });
        let early_delays = std::array::from_fn(|index| {
            let channel = index / 4;
            let tap = index % 4;
            EARLY_SECONDS[tap] * sample_rate * size_scale * (1.0 + channel as f32 * 0.071)
        });
        let rotation = mode.rotation * (0.25 + 0.75 * values.late);
        let (rotation_sin, rotation_cos) = rotation.sin_cos();
        Self {
            delays,
            diffusion_delays,
            early_delays,
            mid_gain,
            bass_gain,
            predelay: (values.predelay_ms * sample_rate * 0.001).ceil(),
            diffusion: ((0.15 + 0.65 * values.early) * mode.diffusion).min(0.8),
            early_gain: values.early * mode.early_gain,
            late_gain: values.late * mode.late_gain,
            bass_filter: lowpass_coefficient(values.bass_frequency, sample_rate),
            high_filter: lowpass_coefficient(
                values.high_frequency * mode.damping_scale,
                sample_rate,
            ),
            high_shelf: 10.0_f32.powf(values.high_shelf_db / 20.0),
            low_cut: lowpass_coefficient(values.low_cut, sample_rate),
            high_cut: lowpass_coefficient(values.high_cut, sample_rate),
            mod_rate: values.mod_rate * mode.mod_rate,
            mod_depth: sample_rate * 0.0012 * values.mod_depth * mode.mod_depth,
            rotation_sin,
            rotation_cos,
            drive: mode.drive,
            duration: decay * sample_rate,
            nonlinear: mode.nonlinear,
        }
    }

    fn approach(&mut self, target: &Self, amount: f32) {
        for index in 0..8 {
            self.mid_gain[index] += (target.mid_gain[index] - self.mid_gain[index]) * amount;
            self.bass_gain[index] += (target.bass_gain[index] - self.bass_gain[index]) * amount;
        }
        for (current, target) in [
            (&mut self.delays, &target.delays),
            (&mut self.diffusion_delays, &target.diffusion_delays),
            (&mut self.early_delays, &target.early_delays),
        ] {
            for (current, target) in current.iter_mut().zip(target) {
                *current += (target - *current) * amount;
            }
        }
        macro_rules! smooth {
            ($($field:ident),* $(,)?) => {
                $(self.$field += (target.$field - self.$field) * amount;)*
            };
        }
        smooth!(
            predelay,
            diffusion,
            early_gain,
            late_gain,
            bass_filter,
            high_filter,
            high_shelf,
            low_cut,
            high_cut,
            mod_rate,
            mod_depth,
            rotation_sin,
            rotation_cos,
            drive,
            duration,
        );
        self.nonlinear = target.nonlinear;
    }
}

pub(super) struct Network {
    sample_rate: f32,
    predelay: [DelayLine; 2],
    diffusion: [DelayLine; 8],
    early: [DelayLine; 2],
    lines: [DelayLine; 8],
    bass: [OnePole; 8],
    damping: [OnePole; 8],
    output_low: [OnePole; 2],
    output_high: [OnePole; 2],
    modulation: Modulation,
    current: Option<Coefficients>,
    nonlinear_remaining: f32,
    nonlinear_duration: f32,
}

impl Network {
    pub(super) fn new(sample_rate: f32) -> Self {
        let max_delay = (sample_rate * 0.5) as usize + 8;
        let max_line = (16_444.0 * sample_rate / 48_000.0 * 1.7) as usize + 64;
        let max_diffusion = (sample_rate * 0.04) as usize + 8;
        let max_early = (sample_rate * 0.07) as usize + 8;
        Self {
            sample_rate,
            predelay: std::array::from_fn(|_| DelayLine::new(max_delay)),
            diffusion: std::array::from_fn(|_| DelayLine::new(max_diffusion)),
            early: std::array::from_fn(|_| DelayLine::new(max_early)),
            lines: std::array::from_fn(|_| DelayLine::new(max_line)),
            bass: [OnePole::default(); 8],
            damping: [OnePole::default(); 8],
            output_low: [OnePole::default(); 2],
            output_high: [OnePole::default(); 2],
            modulation: Modulation::new(),
            current: None,
            nonlinear_remaining: 0.0,
            nonlinear_duration: 1.0,
        }
    }

    pub(super) fn reset(&mut self) {
        for delay in self
            .predelay
            .iter_mut()
            .chain(&mut self.diffusion)
            .chain(&mut self.early)
            .chain(&mut self.lines)
        {
            delay.clear();
        }
        for filter in self
            .bass
            .iter_mut()
            .chain(&mut self.damping)
            .chain(&mut self.output_low)
            .chain(&mut self.output_high)
        {
            filter.clear();
        }
        self.modulation.clear();
        self.current = None;
        self.nonlinear_remaining = 0.0;
        self.nonlinear_duration = 1.0;
    }

    pub(super) fn process(
        &mut self,
        input: [f32; 2],
        target: &Coefficients,
        smoothing: f32,
    ) -> [f32; 2] {
        let coefficients = self.current.get_or_insert(*target);
        coefficients.approach(target, smoothing);
        let c = *coefficients;
        let mut diffused = [0.0; 2];
        let mut wet = [0.0; 2];

        for channel in 0..2 {
            let delayed = self.predelay[channel].read(c.predelay.max(1.0));
            self.predelay[channel].write(input[channel]);
            let mut value = delayed;
            for stage in 0..4 {
                let index = channel * 4 + stage;
                let gain = c.diffusion * if stage % 2 == 0 { 0.7 } else { 0.5 };
                value =
                    self.diffusion[index].allpass(value, c.diffusion_delays[index], gain.min(0.8));
            }
            diffused[channel] = value;
            self.early[channel].write(value);
            for tap in 0..4 {
                let index = channel * 4 + tap;
                let sign = if (tap + channel) % 2 == 0 { 1.0 } else { -1.0 };
                wet[channel] += self.early[channel].read(c.early_delays[index]) * sign * 0.25;
            }
            wet[channel] *= c.early_gain;
        }

        let modulation = self
            .modulation
            .next(c.mod_rate, c.mod_depth, self.sample_rate);
        let mut feedback = [0.0; 8];
        for (index, value) in feedback.iter_mut().enumerate() {
            let delayed = self.lines[index].read(c.delays[index] + 1.0 + modulation[index]);
            let low = self.bass[index].process(delayed, c.bass_filter);
            let weighted =
                delayed * c.mid_gain[index] + low * (c.bass_gain[index] - c.mid_gain[index]);
            let damped = self.damping[index].process(weighted, c.high_filter);
            *value = damped + (weighted - damped) * c.high_shelf;
            for channel in 0..2 {
                wet[channel] += *value * OUTPUT_TAPS[channel][index] * c.late_gain;
            }
        }

        for pair in feedback.as_chunks_mut::<2>().0 {
            let (a, b) = (pair[0], pair[1]);
            pair[0] = c.rotation_cos * a + c.rotation_sin * b;
            pair[1] = c.rotation_cos * b - c.rotation_sin * a;
        }
        let sum = feedback.iter().sum::<f32>();
        for (index, value) in feedback.iter_mut().enumerate() {
            let mut mixed = *value - sum * 0.25 + diffused[index % 2] * 0.25;
            if c.drive > 1.0 {
                mixed = (mixed * c.drive).tanh() / c.drive;
            }
            self.lines[index].write(if mixed.abs() < 1e-25 { 0.0 } else { mixed });
        }

        if c.nonlinear {
            let peak = diffused[0].abs().max(diffused[1].abs());
            if self.nonlinear_remaining <= 0.0 && peak > 1e-4 {
                self.nonlinear_duration = c.duration.max(1.0);
                self.nonlinear_remaining = self.nonlinear_duration;
            }
            let gate = (self.nonlinear_remaining / self.nonlinear_duration).clamp(0.0, 1.0);
            let gate = gate * gate;
            wet[0] *= gate;
            wet[1] *= gate;
            self.nonlinear_remaining = (self.nonlinear_remaining - 1.0).max(0.0);
        } else {
            self.nonlinear_remaining = 0.0;
        }

        for (channel, wet) in wet.iter_mut().enumerate() {
            let low = self.output_low[channel].process(*wet, c.low_cut);
            *wet = self.output_high[channel].process(*wet - low, c.high_cut);
            if wet.abs() < 1e-25 {
                *wet = 0.0;
            }
        }
        wet
    }
}
