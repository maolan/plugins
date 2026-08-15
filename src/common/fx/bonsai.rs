#[derive(Debug, Clone)]
pub struct Bonsai {
    hp_z1_l: f32,
    hp_z1_r: f32,
    lp_z1_l: f32,
    lp_z1_r: f32,
    drive: f32,
    tone: f32,
    mix: f32,
}

impl Default for Bonsai {
    fn default() -> Self {
        Self::new()
    }
}

impl Bonsai {
    pub fn new() -> Self {
        Self {
            hp_z1_l: 0.0,
            hp_z1_r: 0.0,
            lp_z1_l: 0.0,
            lp_z1_r: 0.0,
            drive: 0.3,
            tone: 0.5,
            mix: 0.3,
        }
    }

    pub fn reset(&mut self) {
        self.hp_z1_l = 0.0;
        self.hp_z1_r = 0.0;
        self.lp_z1_l = 0.0;
        self.lp_z1_r = 0.0;
    }

    pub fn set_drive(&mut self, drive: f32) {
        self.drive = drive.clamp(0.0, 1.0);
    }

    pub fn set_tone(&mut self, tone: f32) {
        self.tone = tone.clamp(0.0, 1.0);
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    pub fn process(&mut self, input_l: f32, input_r: f32) -> (f32, f32) {
        let d = 1.0 + self.drive * 15.0;
        let mut dl = input_l * d;
        let mut dr = input_r * d;
        dl = if dl > 0.0 {
            dl.tanh()
        } else {
            (dl * 0.5).tanh() * 2.0
        };
        dr = if dr > 0.0 {
            dr.tanh()
        } else {
            (dr * 0.5).tanh() * 2.0
        };

        let hp_alpha = 0.02;
        dl -= self.hp_z1_l;
        dr -= self.hp_z1_r;
        self.hp_z1_l += hp_alpha * dl;
        self.hp_z1_r += hp_alpha * dr;

        let lp_alpha = self.tone.clamp(0.05, 0.95);
        dl = dl * lp_alpha + self.lp_z1_l * (1.0 - lp_alpha);
        dr = dr * lp_alpha + self.lp_z1_r * (1.0 - lp_alpha);
        self.lp_z1_l = dl;
        self.lp_z1_r = dr;

        (
            input_l * (1.0 - self.mix) + dl * self.mix,
            input_r * (1.0 - self.mix) + dr * self.mix,
        )
    }

    pub fn process_block(&mut self, buf_l: &mut [f32], buf_r: &mut [f32]) {
        for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
            (*l, *r) = self.process(*l, *r);
        }
    }
}
