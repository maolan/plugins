const MAX_DELAY_SAMPLES: usize = 9600;
const MAX_VOICES: usize = 16;
const BASE_DELAY_SECONDS: f32 = 0.015;
const TWO_PI: f32 = 2.0 * std::f32::consts::PI;

pub struct ChorusParams {
    pub depth_ms: f64,
    pub rate_hz: f64,
    pub dry_wet: f64,
    pub voices: f64,
}

pub struct Chorus {
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write_pos: usize,
    phases: [f32; MAX_VOICES],
    sample_rate: f32,
}

impl Default for Chorus {
    fn default() -> Self {
        Self {
            buf_l: vec![0.0; MAX_DELAY_SAMPLES],
            buf_r: vec![0.0; MAX_DELAY_SAMPLES],
            write_pos: 0,
            phases: [0.0; MAX_VOICES],
            sample_rate: 48_000.0,
        }
    }
}

impl Chorus {
    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate as f32;
    }

    pub fn reset(&mut self) {
        self.buf_l.fill(0.0);
        self.buf_r.fill(0.0);
        self.write_pos = 0;
        self.phases = [0.0; MAX_VOICES];
        for i in 0..MAX_VOICES {
            self.phases[i] = TWO_PI * i as f32 / MAX_VOICES as f32;
        }
    }

    #[inline]
    fn read_interp(buf: &[f32], write_pos: usize, delay_samples: f32) -> f32 {
        let size = buf.len();
        let read_pos = write_pos as f32 - delay_samples;
        let idx = read_pos.floor();
        let frac = read_pos - idx;
        let i0 = idx.rem_euclid(size as f32) as usize;
        let i1 = (i0 + 1).min(size - 1);
        buf[i0] * (1.0 - frac) + buf[i1] * frac
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32], params: &ChorusParams) {
        let frames = left.len().min(right.len());
        let depth_samples = (params.depth_ms.clamp(0.0, 10.0) as f32 * 0.001) * self.sample_rate;
        let rate = params.rate_hz.clamp(0.1, 5.0) as f32;
        let mix = params.dry_wet.clamp(0.0, 1.0) as f32;
        let mut num_voices = params.voices.round() as i32;
        num_voices = num_voices.clamp(2, MAX_VOICES as i32);

        let base_delay = BASE_DELAY_SECONDS * self.sample_rate;
        let phase_inc = TWO_PI * rate / self.sample_rate;

        for i in 0..frames {
            self.buf_l[self.write_pos] = left[i];
            self.buf_r[self.write_pos] = right[i];

            let mut sum_l = 0.0f32;
            let mut sum_r = 0.0f32;

            for v in 0..num_voices as usize {
                let modulation = self.phases[v].sin();
                let delay = base_delay + modulation * depth_samples;
                let voice_out = if v % 2 == 0 {
                    Self::read_interp(&self.buf_l, self.write_pos, delay)
                } else {
                    Self::read_interp(&self.buf_r, self.write_pos, delay)
                };
                if v % 2 == 0 {
                    sum_l += voice_out;
                } else {
                    sum_r += voice_out;
                }

                self.phases[v] += phase_inc;
                if self.phases[v] >= TWO_PI {
                    self.phases[v] -= TWO_PI;
                }
            }

            let wet_l = sum_l / (num_voices as f32 / 2.0);
            let wet_r = sum_r / (num_voices as f32 / 2.0);

            left[i] = left[i] * (1.0 - mix) + wet_l * mix;
            right[i] = right[i] * (1.0 - mix) + wet_r * mix;

            self.write_pos = (self.write_pos + 1) % MAX_DELAY_SAMPLES;
        }
    }
}
