#[derive(Debug, Clone)]
pub struct FloatyDelay {
    sample_rate: f32,
    line_l: Vec<f32>,
    line_r: Vec<f32>,
    write_pos: usize,
    phase: f32,
    time_sec: f32,
    feedback: f32,
    rate_hz: f32,
    depth: f32,
    mix: f32,
    z1_l: f32,
    z1_r: f32,
}

impl Default for FloatyDelay {
    fn default() -> Self {
        Self::new(48000.0)
    }
}

impl FloatyDelay {
    pub fn new(sample_rate: f32) -> Self {
        let max_delay_seconds = 2.0;
        let max_samples = (max_delay_seconds * sample_rate).ceil() as usize;
        Self {
            sample_rate,
            line_l: vec![0.0; max_samples.max(1)],
            line_r: vec![0.0; max_samples.max(1)],
            write_pos: 0,
            phase: 0.0,
            time_sec: 0.4,
            feedback: 0.4,
            rate_hz: 0.3,
            depth: 0.3,
            mix: 0.5,
            z1_l: 0.0,
            z1_r: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.line_l.fill(0.0);
        self.line_r.fill(0.0);
        self.write_pos = 0;
        self.phase = 0.0;
        self.z1_l = 0.0;
        self.z1_r = 0.0;
    }

    pub fn set_time(&mut self, time_sec: f32) {
        self.time_sec = time_sec.max(0.0);
    }

    pub fn set_feedback(&mut self, feedback: f32) {
        self.feedback = feedback.clamp(0.0, 1.0);
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
        self.phase += self.rate_hz / self.sample_rate;
        while self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let lfo = (self.phase * 2.0 * std::f32::consts::PI).sin();
        let base_samples = (self.time_sec * self.sample_rate).max(1.0);
        let mod_samples =
            (base_samples * (1.0 + lfo * self.depth * 0.2)).min(self.line_l.len() as f32) as usize;
        let len = self.line_l.len();
        let read_pos = (self.write_pos + len - mod_samples) % len;

        let delayed_l = self.line_l[read_pos];
        let delayed_r = self.line_r[read_pos];

        let alpha = 0.3;
        let fb_l = delayed_l * alpha + self.z1_l * (1.0 - alpha);
        let fb_r = delayed_r * alpha + self.z1_r * (1.0 - alpha);
        self.z1_l = fb_l;
        self.z1_r = fb_r;

        self.line_l[self.write_pos] = input_l + fb_l * self.feedback;
        self.line_r[self.write_pos] = input_r + fb_r * self.feedback;
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
