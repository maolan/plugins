#[derive(Debug, Clone)]
pub struct RotarySpeaker {
    sample_rate: f32,
    horn_phase: f32,
    woofer_phase: f32,
    crossover_z1_l: f32,
    crossover_z1_r: f32,
    speed: f32,
    mix: f32,
}

impl Default for RotarySpeaker {
    fn default() -> Self {
        Self::new(48000.0)
    }
}

impl RotarySpeaker {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            horn_phase: 0.0,
            woofer_phase: 0.0,
            crossover_z1_l: 0.0,
            crossover_z1_r: 0.0,
            speed: 0.5,
            mix: 0.3,
        }
    }

    pub fn reset(&mut self) {
        self.horn_phase = 0.0;
        self.woofer_phase = 0.0;
        self.crossover_z1_l = 0.0;
        self.crossover_z1_r = 0.0;
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed.clamp(0.0, 1.0);
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    pub fn process(&mut self, input_l: f32, input_r: f32) -> (f32, f32) {
        let alpha = 0.15;
        let woofer_l = input_l * alpha + self.crossover_z1_l * (1.0 - alpha);
        let woofer_r = input_r * alpha + self.crossover_z1_r * (1.0 - alpha);
        let horn_l = input_l - woofer_l;
        let horn_r = input_r - woofer_r;
        self.crossover_z1_l = woofer_l;
        self.crossover_z1_r = woofer_r;

        self.horn_phase += (self.speed * 4.0 + 2.0) / self.sample_rate;
        while self.horn_phase >= 1.0 {
            self.horn_phase -= 1.0;
        }
        self.woofer_phase += (self.speed * 1.0 + 0.5) / self.sample_rate;
        while self.woofer_phase >= 1.0 {
            self.woofer_phase -= 1.0;
        }

        let horn_lfo = (self.horn_phase * 2.0 * std::f32::consts::PI).sin();
        let woofer_lfo = (self.woofer_phase * 2.0 * std::f32::consts::PI).sin();

        let horn_amp = 0.5 + horn_lfo * 0.3;
        let horn_pan_l = 0.7 + horn_lfo * 0.3;
        let horn_pan_r = 0.7 - horn_lfo * 0.3;
        let woofer_amp = 0.5 + woofer_lfo * 0.2;

        let wet_l = horn_l * horn_amp * horn_pan_l + woofer_l * woofer_amp;
        let wet_r = horn_r * horn_amp * horn_pan_r + woofer_r * woofer_amp;

        (
            input_l * (1.0 - self.mix) + wet_l * self.mix,
            input_r * (1.0 - self.mix) + wet_r * self.mix,
        )
    }

    pub fn process_block(&mut self, buf_l: &mut [f32], buf_r: &mut [f32]) {
        for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
            (*l, *r) = self.process(*l, *r);
        }
    }
}
