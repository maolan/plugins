use crate::common::{
    envelope_follower::EnvelopeFollower,
    filter::{Filter, FilterType},
    lfo::{Lfo, LfoShape},
};

/// Wah operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WahMode {
    /// Pedal-controlled static position (map a MIDI CC to the Position param).
    #[default]
    Manual = 0,
    /// LFO sweeps the wah automatically.
    Lfo = 1,
    /// Envelope follower sweeps the wah automatically.
    Envelope = 2,
}

impl WahMode {
    pub fn from_value(value: f64) -> Self {
        match value.round() as i32 {
            1 => Self::Lfo,
            2 => Self::Envelope,
            _ => Self::Manual,
        }
    }

    pub fn to_value(self) -> f64 {
        self as i32 as f64
    }
}

/// LFO shape for the LFO wah mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LfoShapeParam {
    #[default]
    Sine = 0,
    Triangle = 1,
    Saw = 2,
    Square = 3,
}

impl LfoShapeParam {
    pub fn from_value(value: f64) -> Self {
        match value.round() as i32 {
            1 => Self::Triangle,
            2 => Self::Saw,
            3 => Self::Square,
            _ => Self::Sine,
        }
    }

    pub fn to_value(self) -> f64 {
        self as i32 as f64
    }

    pub fn to_lfo_shape(self) -> LfoShape {
        match self {
            Self::Sine => LfoShape::Sine,
            Self::Triangle => LfoShape::Triangle,
            Self::Saw => LfoShape::Saw,
            Self::Square => LfoShape::Square,
        }
    }
}

/// Runtime parameters for one process block.
#[derive(Debug, Clone, Copy)]
pub struct WahParams {
    pub mode: WahMode,
    pub min_cutoff_hz: f64,
    pub max_cutoff_hz: f64,
    pub resonance: f64,
    pub position: f64,
    pub lfo_rate_hz: f64,
    pub lfo_depth: f64,
    pub lfo_shape: LfoShapeParam,
    pub env_attack_ms: f64,
    pub env_release_ms: f64,
    pub env_depth: f64,
    pub dry_wet: f64,
}

impl Default for WahParams {
    fn default() -> Self {
        Self {
            mode: WahMode::Manual,
            min_cutoff_hz: 300.0,
            max_cutoff_hz: 3000.0,
            resonance: 4.0,
            position: 0.5,
            lfo_rate_hz: 2.0,
            lfo_depth: 0.5,
            lfo_shape: LfoShapeParam::Sine,
            env_attack_ms: 20.0,
            env_release_ms: 200.0,
            env_depth: 0.5,
            dry_wet: 1.0,
        }
    }
}

/// Stereo wah-wah effect.
pub struct Wah {
    sample_rate: f64,
    filter_l: Filter,
    filter_r: Filter,
    lfo: Lfo,
    follower: EnvelopeFollower,
    current_mode: WahMode,
    current_shape: LfoShapeParam,
}

impl Wah {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            filter_l: Filter::new(FilterType::Bandpass, sample_rate as f32),
            filter_r: Filter::new(FilterType::Bandpass, sample_rate as f32),
            lfo: Lfo::new(sample_rate as f32),
            follower: EnvelopeFollower::new(sample_rate as f32),
            current_mode: WahMode::Manual,
            current_shape: LfoShapeParam::Sine,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.filter_l = Filter::new(FilterType::Bandpass, sample_rate as f32);
        self.filter_r = Filter::new(FilterType::Bandpass, sample_rate as f32);
        self.lfo = Lfo::new(sample_rate as f32);
        self.follower = EnvelopeFollower::new(sample_rate as f32);
    }

    pub fn reset(&mut self) {
        self.filter_l.reset();
        self.filter_r.reset();
        self.lfo.reset();
        self.follower.reset();
    }

    fn map_position_to_cutoff(&self, position: f32, params: &WahParams) -> f32 {
        let min = params.min_cutoff_hz as f32;
        let max = params.max_cutoff_hz as f32;
        let pos = position.clamp(0.0, 1.0);
        min * (max / min).powf(pos)
    }

    fn update_lfo(&mut self, params: &WahParams) {
        if self.current_shape != params.lfo_shape {
            self.current_shape = params.lfo_shape;
            self.lfo.set_shape(params.lfo_shape.to_lfo_shape());
        }
        self.lfo.set_rate_hz(params.lfo_rate_hz as f32);
        self.lfo.set_amount(1.0);
        self.lfo.set_unipolar(false);
    }

    fn update_follower(&mut self, params: &WahParams) {
        self.follower
            .set_attack((params.env_attack_ms / 1000.0) as f32);
        self.follower
            .set_release((params.env_release_ms / 1000.0) as f32);
    }

    /// Process one stereo sample and return the filtered stereo sample.
    fn process_sample(&mut self, input_l: f32, input_r: f32, params: &WahParams) -> (f32, f32) {
        let base_position = params.position as f32;

        let modulation = match params.mode {
            WahMode::Manual => 0.0f32,
            WahMode::Lfo => {
                self.update_lfo(params);
                self.lfo.next() * params.lfo_depth as f32
            }
            WahMode::Envelope => {
                self.update_follower(params);
                self.follower.process(input_l, input_r) * params.env_depth as f32
            }
        };

        let position = (base_position + modulation).clamp(0.0, 1.0);
        let cutoff = self.map_position_to_cutoff(position, params);
        let resonance = params.resonance as f32;

        self.filter_l.prepare_block(cutoff, resonance, 1);
        self.filter_r.prepare_block(cutoff, resonance, 1);

        let wet_l = self.filter_l.process(input_l);
        let wet_r = self.filter_r.process(input_r);

        let wet = params.dry_wet.clamp(0.0, 1.0) as f32;
        let dry_l = input_l * (1.0 - wet);
        let dry_r = input_r * (1.0 - wet);

        (dry_l + wet_l * wet, dry_r + wet_r * wet)
    }

    /// Process stereo buffers in place.
    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32], params: &WahParams) {
        if params.mode != self.current_mode {
            self.current_mode = params.mode;
            self.reset();
        }

        // Pre-update modulators once per block so parameter changes take effect
        // even if the mode does not drive them sample-by-sample.
        match params.mode {
            WahMode::Lfo => self.update_lfo(params),
            WahMode::Envelope => self.update_follower(params),
            WahMode::Manual => {}
        }

        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            (*l, *r) = self.process_sample(*l, *r, params);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn sine(freq: f32, n: usize, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / SR).sin())
            .collect()
    }

    fn rms(buf: &[f32]) -> f32 {
        let sum: f32 = buf.iter().map(|s| s * s).sum();
        (sum / buf.len().max(1) as f32).sqrt()
    }

    #[test]
    fn manual_mode_filters_signal() {
        let input = sine(1000.0, 4800, 0.5);
        let mut left = input.clone();
        let mut right = input.clone();

        let mut wah = Wah::new(SR as f64);
        let params = WahParams {
            mode: WahMode::Manual,
            position: 0.5,
            dry_wet: 1.0,
            ..WahParams::default()
        };

        wah.process_stereo(&mut left, &mut right, &params);

        // The bandpass should change the signal level compared to bypass.
        assert!((rms(&left) - rms(&input)).abs() > 1e-6);
        assert!(left.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn lfo_mode_produces_modulation() {
        let input = sine(1000.0, 9600, 0.5);
        let mut left = input.clone();
        let mut right = input.clone();

        let mut wah = Wah::new(SR as f64);
        let params = WahParams {
            mode: WahMode::Lfo,
            lfo_rate_hz: 4.0,
            lfo_depth: 1.0,
            dry_wet: 1.0,
            ..WahParams::default()
        };

        wah.process_stereo(&mut left, &mut right, &params);

        assert!(left.iter().all(|s| s.is_finite()));
        assert!((rms(&left) - rms(&input)).abs() > 1e-6);
    }

    #[test]
    fn envelope_mode_responds_to_level() {
        // Build a block that starts silent and then becomes loud.
        let mut input = vec![0.0f32; 2400];
        input.extend(sine(1000.0, 2400, 0.5));
        let mut left = input.clone();
        let mut right = input.clone();

        let mut wah = Wah::new(SR as f64);
        let params = WahParams {
            mode: WahMode::Envelope,
            env_attack_ms: 1.0,
            env_release_ms: 10.0,
            env_depth: 1.0,
            dry_wet: 1.0,
            ..WahParams::default()
        };

        wah.process_stereo(&mut left, &mut right, &params);

        assert!(left.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn dry_wet_passthrough() {
        let input = sine(1000.0, 4800, 0.5);
        let mut left = input.clone();
        let mut right = input.clone();

        let mut wah = Wah::new(SR as f64);
        let params = WahParams {
            dry_wet: 0.0,
            ..WahParams::default()
        };

        wah.process_stereo(&mut left, &mut right, &params);

        for i in 0..left.len() {
            assert!((left[i] - input[i]).abs() < 1e-6);
            assert!((right[i] - input[i]).abs() < 1e-6);
        }
    }
}
