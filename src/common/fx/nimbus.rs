#[derive(Debug, Clone)]
pub struct Nimbus {
    sample_rate: f32,
    lines_l: [Vec<f32>; 4],
    lines_r: [Vec<f32>; 4],
    pos: [usize; 4],
    phase: f32,
    size: f32,
    density: f32,
    mix: f32,
}

const BASE_DELAYS: [f32; 4] = [0.013, 0.019, 0.023, 0.029];

impl Default for Nimbus {
    fn default() -> Self {
        Self::new(48000.0)
    }
}

impl Nimbus {
    pub fn new(sample_rate: f32) -> Self {
        let max_len = (sample_rate * 0.05) as usize + 1;
        Self {
            sample_rate,
            lines_l: [
                vec![0.0; max_len],
                vec![0.0; max_len],
                vec![0.0; max_len],
                vec![0.0; max_len],
            ],
            lines_r: [
                vec![0.0; max_len],
                vec![0.0; max_len],
                vec![0.0; max_len],
                vec![0.0; max_len],
            ],
            pos: [0; 4],
            phase: 0.0,
            size: 0.5,
            density: 0.5,
            mix: 0.3,
        }
    }

    pub fn reset(&mut self) {
        for buf in &mut self.lines_l {
            buf.fill(0.0);
        }
        for buf in &mut self.lines_r {
            buf.fill(0.0);
        }
        self.pos = [0; 4];
        self.phase = 0.0;
    }

    pub fn set_size(&mut self, size: f32) {
        self.size = size.clamp(0.0, 1.0);
    }

    pub fn set_density(&mut self, density: f32) {
        self.density = density.clamp(0.0, 1.0);
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    pub fn process(&mut self, input_l: f32, input_r: f32) -> (f32, f32) {
        self.phase += 0.2 / self.sample_rate;
        while self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let lfo = (self.phase * 2.0 * std::f32::consts::PI).sin();
        let mut out_l = 0.0f32;
        let mut out_r = 0.0f32;
        let fb = self.size * 0.7;

        for (i, base_delay) in BASE_DELAYS.iter().enumerate() {
            let buf_l = &mut self.lines_l[i];
            let buf_r = &mut self.lines_r[i];
            let pos = self.pos[i];
            let ds = (*base_delay
                * self.sample_rate
                * (1.0 + lfo * self.density * 0.1 * (i as f32 + 1.0)))
                as usize;
            let ds = ds.max(1).min(buf_l.len());
            let read_pos = (pos + buf_l.len() - ds) % buf_l.len();
            let delayed_l = buf_l[read_pos];
            let delayed_r = buf_r[read_pos];

            buf_l[pos] = input_l + delayed_l * fb;
            buf_r[pos] = input_r + delayed_r * fb;
            self.pos[i] = (pos + 1) % buf_l.len();

            let amp = 0.25 * (1.0 + self.density * (i as f32 * 0.1).sin());
            out_l += delayed_l * amp;
            out_r += delayed_r * amp;
        }

        (
            input_l * (1.0 - self.mix) + out_l * self.mix,
            input_r * (1.0 - self.mix) + out_r * self.mix,
        )
    }

    pub fn process_block(&mut self, buf_l: &mut [f32], buf_r: &mut [f32]) {
        for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
            (*l, *r) = self.process(*l, *r);
        }
    }
}
