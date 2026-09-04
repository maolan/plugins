const MAX_DELAY_SAMPLES: usize = 48_000;
const MAX_STAGES: usize = 12;
const TWO_PI: f32 = 2.0 * std::f32::consts::PI;

pub struct PhaserParams {
    pub lfo_rate_hz: f64,
    pub lfo_depth: f64,
    pub manual: f64,
    pub feedback: f64,
    pub feedback_delay_on: f64,
    pub delay_time_ms: f64,
    pub stages: f64,
}

struct AllPass {
    x1: f32,
    y1: f32,
}

impl Default for AllPass {
    fn default() -> Self {
        Self { x1: 0.0, y1: 0.0 }
    }
}

impl AllPass {
    #[inline]
    fn process(&mut self, input: f32, g: f32) -> f32 {
        let out = -g * input + self.x1 + g * self.y1;
        let out = if out.abs() < 1e-15 { 0.0 } else { out };
        self.x1 = input;
        self.y1 = out;
        out
    }
}

pub struct Phaser {
    filters_l: [AllPass; MAX_STAGES],
    filters_r: [AllPass; MAX_STAGES],
    delay_buf_l: Vec<f32>,
    delay_buf_r: Vec<f32>,
    write_idx: usize,
    lfo_phase: f32,
    last_out_l: f32,
    last_out_r: f32,
    sample_rate: f32,
}

impl Default for Phaser {
    fn default() -> Self {
        Self {
            filters_l: std::array::from_fn(|_| AllPass::default()),
            filters_r: std::array::from_fn(|_| AllPass::default()),
            delay_buf_l: vec![0.0; MAX_DELAY_SAMPLES],
            delay_buf_r: vec![0.0; MAX_DELAY_SAMPLES],
            write_idx: 0,
            lfo_phase: 0.0,
            last_out_l: 0.0,
            last_out_r: 0.0,
            sample_rate: 48_000.0,
        }
    }
}

impl Phaser {
    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate as f32;
    }

    pub fn reset(&mut self) {
        for f in self.filters_l.iter_mut() {
            *f = AllPass::default();
        }
        for f in self.filters_r.iter_mut() {
            *f = AllPass::default();
        }
        self.delay_buf_l.fill(0.0);
        self.delay_buf_r.fill(0.0);
        self.write_idx = 0;
        self.lfo_phase = 0.0;
        self.last_out_l = 0.0;
        self.last_out_r = 0.0;
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32], params: &PhaserParams) {
        let frames = left.len().min(right.len());
        let rate = params.lfo_rate_hz.clamp(0.01, 2.0) as f32;
        let depth = params.lfo_depth.clamp(0.0, 1.0) as f32;
        let manual = params.manual.clamp(0.0, 1.0) as f32;
        let feedback = params.feedback.clamp(-0.98, 0.98) as f32;
        let delay_on = params.feedback_delay_on > 0.5;
        let delay_ms = params.delay_time_ms.clamp(0.0, 20.0) as f32;
        let mut stages = params.stages.round() as i32;
        stages = stages.clamp(1, MAX_STAGES as i32);

        let delay_samples = (delay_ms * 0.001 * self.sample_rate) as usize;
        let delay_samples = delay_samples.min(MAX_DELAY_SAMPLES - 1);

        let lfo_val = (self.lfo_phase).sin() * depth;
        let combined = (manual + lfo_val).clamp(0.0, 1.0);
        let freq = 50.0 * (300.0f32).powf(combined);
        let tan_val = (std::f32::consts::PI * freq / self.sample_rate).tan();
        let g = (tan_val - 1.0) / (tan_val + 1.0);

        let lfo_inc = TWO_PI * rate / self.sample_rate;

        for i in 0..frames {
            self.lfo_phase += lfo_inc;
            if self.lfo_phase > TWO_PI {
                self.lfo_phase -= TWO_PI;
            }

            let fb_sig_l = if delay_on {
                let read_idx =
                    (self.write_idx + MAX_DELAY_SAMPLES - delay_samples) % MAX_DELAY_SAMPLES;
                self.delay_buf_l[read_idx]
            } else {
                self.last_out_l
            };
            let fb_sig_r = if delay_on {
                let read_idx =
                    (self.write_idx + MAX_DELAY_SAMPLES - delay_samples) % MAX_DELAY_SAMPLES;
                self.delay_buf_r[read_idx]
            } else {
                self.last_out_r
            };

            let in_l = left[i] + fb_sig_l * feedback;
            let in_r = right[i] + fb_sig_r * feedback;

            let mut stage_l = in_l;
            let mut stage_r = in_r;
            for s in 0..stages as usize {
                stage_l = self.filters_l[s].process(stage_l, g);
                stage_r = self.filters_r[s].process(stage_r, g);
            }

            self.last_out_l = stage_l;
            self.last_out_r = stage_r;
            self.delay_buf_l[self.write_idx] = stage_l;
            self.delay_buf_r[self.write_idx] = stage_r;
            self.write_idx = (self.write_idx + 1) % MAX_DELAY_SAMPLES;

            left[i] = (left[i] + stage_l) * 0.5;
            right[i] = (right[i] + stage_r) * 0.5;
        }
    }
}
