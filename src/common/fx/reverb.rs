#[derive(Debug, Clone)]
pub struct Reverb {
    combs_l: [Vec<f32>; 4],
    combs_r: [Vec<f32>; 4],
    combs_pos: [usize; 4],
    allpass_l: Vec<f32>,
    allpass_r: Vec<f32>,
    allpass_pos: usize,
    size: f32,
    damp: f32,
    mix: f32,
}

impl Default for Reverb {
    fn default() -> Self {
        Self::new(48000.0)
    }
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            combs_l: [
                vec![0.0; ((sample_rate * 0.0297) as usize).max(1)],
                vec![0.0; ((sample_rate * 0.0371) as usize).max(1)],
                vec![0.0; ((sample_rate * 0.0411) as usize).max(1)],
                vec![0.0; ((sample_rate * 0.0437) as usize).max(1)],
            ],
            combs_r: [
                vec![0.0; ((sample_rate * 0.0297) as usize).max(1)],
                vec![0.0; ((sample_rate * 0.0371) as usize).max(1)],
                vec![0.0; ((sample_rate * 0.0411) as usize).max(1)],
                vec![0.0; ((sample_rate * 0.0437) as usize).max(1)],
            ],
            combs_pos: [0; 4],
            allpass_l: vec![0.0; ((sample_rate * 0.005) as usize).max(1)],
            allpass_r: vec![0.0; ((sample_rate * 0.005) as usize).max(1)],
            allpass_pos: 0,
            size: 0.5,
            damp: 0.5,
            mix: 0.3,
        }
    }

    pub fn reset(&mut self) {
        for buf in &mut self.combs_l {
            buf.fill(0.0);
        }
        for buf in &mut self.combs_r {
            buf.fill(0.0);
        }
        self.combs_pos = [0; 4];
        self.allpass_l.fill(0.0);
        self.allpass_r.fill(0.0);
        self.allpass_pos = 0;
    }

    pub fn set_size(&mut self, size: f32) {
        self.size = size.clamp(0.0, 1.0);
    }

    pub fn set_damp(&mut self, damp: f32) {
        self.damp = damp.clamp(0.0, 1.0);
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    pub fn process(&mut self, input_l: f32, input_r: f32) -> (f32, f32) {
        let feedback = self.size * 0.84 + 0.1;
        let mut out_l = 0.0f32;
        let mut out_r = 0.0f32;

        for i in 0..4 {
            let buf_l = &mut self.combs_l[i];
            let buf_r = &mut self.combs_r[i];
            let pos = self.combs_pos[i];
            let delayed_l = buf_l[pos];
            let delayed_r = buf_r[pos];
            buf_l[pos] = input_l + delayed_l * feedback;
            buf_r[pos] = input_r + delayed_r * feedback;
            out_l += delayed_l;
            out_r += delayed_r;
            self.combs_pos[i] = (pos + 1) % buf_l.len();
        }
        out_l *= 0.25;
        out_r *= 0.25;

        let ap_pos = self.allpass_pos;
        let delayed_l = self.allpass_l[ap_pos];
        let delayed_r = self.allpass_r[ap_pos];
        self.allpass_l[ap_pos] = out_l + delayed_l * self.damp;
        self.allpass_r[ap_pos] = out_r + delayed_r * self.damp;
        self.allpass_pos = (ap_pos + 1) % self.allpass_l.len();

        let rev_l = delayed_l - out_l * self.damp;
        let rev_r = delayed_r - out_r * self.damp;

        (
            input_l * (1.0 - self.mix) + rev_l * self.mix,
            input_r * (1.0 - self.mix) + rev_r * self.mix,
        )
    }

    pub fn process_block(&mut self, buf_l: &mut [f32], buf_r: &mut [f32]) {
        for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
            (*l, *r) = self.process(*l, *r);
        }
    }
}
