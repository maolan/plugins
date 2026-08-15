#[derive(Debug, Clone)]
pub struct TreeMonster {
    sample_rate: f32,
    phase: f32,
    z1_l: f32,
    z1_r: f32,
    drive: f32,
    tone: f32,
    mix: f32,
}

impl Default for TreeMonster {
    fn default() -> Self {
        Self::new(48000.0)
    }
}

impl TreeMonster {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            phase: 0.0,
            z1_l: 0.0,
            z1_r: 0.0,
            drive: 0.5,
            tone: 0.5,
            mix: 0.5,
        }
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.z1_l = 0.0;
        self.z1_r = 0.0;
    }

    pub fn set_drive(&mut self, drive: f32) {
        self.drive = drive.clamp(0.0, 1.0);
    }

    pub fn set_tone(&mut self, tone: f32) {
        self.tone = tone.clamp(0.0, 1.0);
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    pub fn process(&mut self, input_l: f32, input_r: f32) -> (f32, f32) {
        self.phase += 0.07 / self.sample_rate;
        while self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let lfo = (self.phase * 2.0 * std::f32::consts::PI).sin();

        let carrier = 1.0 + lfo * 0.3;
        let d = 1.0 + self.drive * 20.0;
        let mut dl = (input_l * d * carrier).tanh();
        let mut dr = (input_r * d * carrier).tanh();

        let alpha = self.tone.clamp(0.01, 0.99);
        dl = dl * alpha + self.z1_l * (1.0 - alpha);
        dr = dr * alpha + self.z1_r * (1.0 - alpha);
        self.z1_l = dl;
        self.z1_r = dr;

        (
            input_l * (1.0 - self.mix) + dl * self.mix,
            input_r * (1.0 - self.mix) + dr * self.mix,
        )
    }

    pub fn process_block(&mut self, buf_l: &mut [f32], buf_r: &mut [f32]) {
        for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
            (*l, *r) = self.process(*l, *r);
        }
    }
}
