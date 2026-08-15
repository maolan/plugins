#[derive(Debug, Clone)]
pub struct Bitcrusher {
    bits: f32,
    rate_div: usize,
    counter: usize,
    hold_l: f32,
    hold_r: f32,
}

impl Default for Bitcrusher {
    fn default() -> Self {
        Self::new()
    }
}

impl Bitcrusher {
    pub fn new() -> Self {
        Self {
            bits: 8.0,
            rate_div: 1,
            counter: 0,
            hold_l: 0.0,
            hold_r: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.counter = 0;
        self.hold_l = 0.0;
        self.hold_r = 0.0;
    }

    pub fn set_bits(&mut self, bits: f32) {
        self.bits = bits.clamp(1.0, 16.0);
    }

    pub fn set_rate_div(&mut self, rate_div: f32) {
        self.rate_div = rate_div.max(1.0).round() as usize;
    }

    pub fn process(&mut self, input_l: f32, input_r: f32) -> (f32, f32) {
        if self.counter == 0 {
            let levels = (2.0f32.powf(self.bits) - 1.0).max(1.0);
            self.hold_l = (input_l * levels).round() / levels;
            self.hold_r = (input_r * levels).round() / levels;
        }
        self.counter += 1;
        if self.counter >= self.rate_div {
            self.counter = 0;
        }
        (self.hold_l, self.hold_r)
    }

    pub fn process_block(&mut self, buf_l: &mut [f32], buf_r: &mut [f32]) {
        for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
            (*l, *r) = self.process(*l, *r);
        }
    }
}
