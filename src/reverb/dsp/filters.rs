#[derive(Clone, Copy, Default)]
pub(super) struct OnePole {
    state: f32,
}

impl OnePole {
    pub(super) fn process(&mut self, input: f32, coefficient: f32) -> f32 {
        self.state += coefficient * (input - self.state);
        if self.state.abs() < 1e-25 {
            self.state = 0.0;
        }
        self.state
    }

    pub(super) fn clear(&mut self) {
        self.state = 0.0;
    }
}

pub(super) fn lowpass_coefficient(frequency: f32, sample_rate: f32) -> f32 {
    let frequency = frequency.clamp(1.0, sample_rate * 0.45);
    1.0 - (-std::f32::consts::TAU * frequency / sample_rate).exp()
}
