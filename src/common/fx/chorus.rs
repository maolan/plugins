#[derive(Debug, Clone)]
pub struct Chorus {
    sample_rate: f32,
    line_l: Vec<f32>,
    line_r: Vec<f32>,
    write_pos: usize,
    phase: f32,
    rate_hz: f32,
    depth: f32,
    mix: f32,
}

impl Default for Chorus {
    fn default() -> Self {
        Self::new(48000.0)
    }
}

impl Chorus {
    pub fn new(sample_rate: f32) -> Self {
        let max_delay_ms = 50.0;
        let max_samples = (max_delay_ms * sample_rate / 1000.0).ceil() as usize;
        Self {
            sample_rate,
            line_l: vec![0.0; max_samples.max(1)],
            line_r: vec![0.0; max_samples.max(1)],
            write_pos: 0,
            phase: 0.0,
            rate_hz: 0.5,
            depth: 0.5,
            mix: 0.5,
        }
    }

    pub fn reset(&mut self) {
        self.line_l.fill(0.0);
        self.line_r.fill(0.0);
        self.write_pos = 0;
        self.phase = 0.0;
    }

    pub fn set_rate_hz(&mut self, rate_hz: f32) {
        self.rate_hz = rate_hz;
    }

    pub fn set_depth(&mut self, depth: f32) {
        self.depth = depth.clamp(0.0, 1.0);
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    pub fn process(&mut self, input_l: f32, input_r: f32) -> (f32, f32) {
        let max_delay_ms = 20.0;
        let max_delay_samples = (max_delay_ms * self.sample_rate / 1000.0) as usize;
        self.phase += self.rate_hz / self.sample_rate;
        while self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let lfo = (self.phase * 2.0 * std::f32::consts::PI).sin();
        let mod_delay = ((1.0 + lfo * self.depth) * max_delay_samples as f32) as usize;
        let delay_samples = mod_delay.max(1).min(self.line_l.len());
        let len = self.line_l.len();
        let read_pos = (self.write_pos + len - delay_samples) % len;

        let delayed_l = self.line_l[read_pos];
        let delayed_r = self.line_r[read_pos];

        self.line_l[self.write_pos] = input_l;
        self.line_r[self.write_pos] = input_r;
        self.write_pos = (self.write_pos + 1) % len;

        (
            input_l * (1.0 - self.mix) + delayed_l * self.mix,
            input_r * (1.0 - self.mix) + delayed_r * self.mix,
        )
    }

    pub fn process_block(&mut self, buf_l: &mut [f32], buf_r: &mut [f32]) {
        for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
            (*l, *r) = self.process(*l, *r);
        }
    }
}
