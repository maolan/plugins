/// Modulated delay line for subtle chorus-style pitch shift.
/// Uses a sine LFO to sweep delay time, creating ~3-cent pitch modulation
/// at 0.5 Hz with 0.5 ms depth (default). Fully mono-compatible.
#[derive(Debug, Clone)]
pub struct ModulatedDelay {
    buffer: Vec<f64>,
    write_idx: usize,
    phase: f64,
    phase_increment: f64,
    depth_samples: f64,
    base_delay_samples: f64,
}

impl ModulatedDelay {
    pub fn new(sample_rate: f64) -> Self {
        let rate_hz = 0.5;
        let depth_seconds = 0.0005;
        let base_delay_seconds = 0.001;
        let depth_samples = depth_seconds * sample_rate;
        let base_delay_samples = base_delay_seconds * sample_rate;
        let max_delay = (base_delay_samples + depth_samples + 10.0).ceil() as usize;
        Self {
            buffer: vec![0.0; max_delay],
            write_idx: 0,
            phase: 0.0,
            phase_increment: std::f64::consts::TAU * rate_hz / sample_rate,
            depth_samples,
            base_delay_samples,
        }
    }

    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_idx = 0;
        self.phase = 0.0;
    }

    pub fn process(&mut self, input: f64) -> f64 {
        self.buffer[self.write_idx] = input;

        let delay = self.base_delay_samples + self.depth_samples * self.phase.sin();
        let read_idx = self.write_idx as f64 - delay;
        let read_idx_floor = read_idx.floor() as isize;
        let frac = read_idx - read_idx.floor();

        let buf_len = self.buffer.len();
        let i0 = read_idx_floor.rem_euclid(buf_len as isize) as usize;
        let i1 = (read_idx_floor + 1).rem_euclid(buf_len as isize) as usize;

        let output = self.buffer[i0] * (1.0 - frac) + self.buffer[i1] * frac;

        self.phase += self.phase_increment;
        if self.phase >= std::f64::consts::TAU {
            self.phase -= std::f64::consts::TAU;
        }
        self.write_idx = (self.write_idx + 1) % buf_len;

        output
    }
}
