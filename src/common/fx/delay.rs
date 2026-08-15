#[derive(Debug, Clone)]
pub struct Delay {
    sample_rate: f32,
    max_delay_seconds: f32,
    line_l: Vec<f32>,
    line_r: Vec<f32>,
    write_pos: usize,
    time_sec: f32,
    feedback: f32,
    mix: f32,
}

impl Default for Delay {
    fn default() -> Self {
        Self::new(48000.0)
    }
}

impl Delay {
    pub fn new(sample_rate: f32) -> Self {
        let max_delay_seconds = 2.0;
        let max_samples = (max_delay_seconds * sample_rate).ceil() as usize;
        Self {
            sample_rate,
            max_delay_seconds,
            line_l: vec![0.0; max_samples.max(1)],
            line_r: vec![0.0; max_samples.max(1)],
            write_pos: 0,
            time_sec: 0.25,
            feedback: 0.3,
            mix: 0.5,
        }
    }

    pub fn new_with_max_delay(sample_rate: f32, max_delay_seconds: f32) -> Self {
        let max_samples = (max_delay_seconds * sample_rate).ceil() as usize;
        Self {
            sample_rate,
            max_delay_seconds,
            line_l: vec![0.0; max_samples.max(1)],
            line_r: vec![0.0; max_samples.max(1)],
            write_pos: 0,
            time_sec: 0.25,
            feedback: 0.3,
            mix: 0.5,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        let max_samples = (self.max_delay_seconds * sample_rate).ceil() as usize;
        self.line_l.resize(max_samples.max(1), 0.0);
        self.line_r.resize(max_samples.max(1), 0.0);
        self.write_pos = 0;
    }

    pub fn reset(&mut self) {
        self.line_l.fill(0.0);
        self.line_r.fill(0.0);
        self.write_pos = 0;
    }

    pub fn set_time(&mut self, time_sec: f32) {
        self.time_sec = time_sec.max(0.0);
    }

    pub fn set_feedback(&mut self, feedback: f32) {
        self.feedback = feedback.clamp(0.0, 1.0);
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    pub fn process(&mut self, input_l: f32, input_r: f32) -> (f32, f32) {
        let len = self.line_l.len();
        let delay_samples = (self.time_sec * self.sample_rate).max(1.0).min(len as f32) as usize;
        let read_pos = (self.write_pos + len - delay_samples) % len;

        let delayed_l = self.line_l[read_pos];
        let delayed_r = self.line_r[read_pos];

        self.line_l[self.write_pos] = input_l + delayed_l * self.feedback;
        self.line_r[self.write_pos] = input_r + delayed_r * self.feedback;
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
