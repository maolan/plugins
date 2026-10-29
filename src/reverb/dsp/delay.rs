pub(super) struct DelayLine {
    samples: Box<[f32]>,
    write_pos: usize,
}

impl DelayLine {
    pub(super) fn new(length: usize) -> Self {
        Self {
            samples: vec![0.0; length.max(4)].into_boxed_slice(),
            write_pos: 0,
        }
    }

    pub(super) fn clear(&mut self) {
        self.samples.fill(0.0);
        self.write_pos = 0;
    }

    pub(super) fn read(&self, delay: f32) -> f32 {
        let delay = delay.clamp(1.0, (self.samples.len() - 2) as f32);
        let position = self.write_pos as f32 - delay;
        let lower = position.floor();
        let fraction = position - lower;
        let lower = lower as isize;
        let length = self.samples.len() as isize;
        let a = lower.rem_euclid(length) as usize;
        let b = (lower - 1).rem_euclid(length) as usize;
        self.samples[a] * (1.0 - fraction) + self.samples[b] * fraction
    }

    pub(super) fn write(&mut self, value: f32) {
        self.samples[self.write_pos] = value;
        self.write_pos = (self.write_pos + 1) % self.samples.len();
    }

    pub(super) fn allpass(&mut self, input: f32, delay: f32, gain: f32) -> f32 {
        let delayed = self.read(delay);
        let output = delayed - gain * input;
        self.write(input + gain * output);
        output
    }
}
