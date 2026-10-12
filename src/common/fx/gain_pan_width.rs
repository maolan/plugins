#[derive(Debug, Clone, Copy, Default)]
pub struct Gain {
    gain_linear: f32,
}

impl Gain {
    pub fn new() -> Self {
        Self { gain_linear: 1.0 }
    }

    pub fn set_db(&mut self, gain_db: f32) {
        self.gain_linear = 10.0f32.powf(gain_db / 20.0);
    }

    pub fn set_linear(&mut self, gain_linear: f32) {
        self.gain_linear = gain_linear;
    }

    pub fn process(&self, input_l: f32, input_r: f32) -> (f32, f32) {
        (input_l * self.gain_linear, input_r * self.gain_linear)
    }

    pub fn process_block(&self, buf_l: &mut [f32], buf_r: &mut [f32]) {
        crate::simd::mul_inplace(buf_l, self.gain_linear);
        crate::simd::mul_inplace(buf_r, self.gain_linear);
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Pan {
    gain_l: f32,
    gain_r: f32,
}

impl Pan {
    pub fn new() -> Self {
        Self::from_pan(0.0)
    }

    pub fn from_pan(pan: f32) -> Self {
        let pan = pan.clamp(-1.0, 1.0);
        let angle = (pan + 1.0) * std::f32::consts::PI / 4.0;
        Self {
            gain_l: angle.cos(),
            gain_r: angle.sin(),
        }
    }

    pub fn set_pan(&mut self, pan: f32) {
        *self = Self::from_pan(pan);
    }

    pub fn process(&self, input_l: f32, input_r: f32) -> (f32, f32) {
        (input_l * self.gain_l, input_r * self.gain_r)
    }

    pub fn process_block(&self, buf_l: &mut [f32], buf_r: &mut [f32]) {
        crate::simd::mul_inplace(buf_l, self.gain_l);
        crate::simd::mul_inplace(buf_r, self.gain_r);
    }
}

#[derive(Debug, Default)]
pub struct Width {
    width: f32,
    /// Reusable mid buffer; only grows, never allocated per block.
    scratch: std::cell::RefCell<Vec<f32>>,
}

impl Width {
    pub fn new() -> Self {
        Self {
            width: 1.0,
            scratch: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub fn set_width(&mut self, width: f32) {
        self.width = width.clamp(0.0, 2.0);
    }

    pub fn process(&self, input_l: f32, input_r: f32) -> (f32, f32) {
        let mid = (input_l + input_r) * 0.5;
        let side = (input_l - input_r) * 0.5 * self.width;
        (mid + side, mid - side)
    }

    pub fn process_block(&self, buf_l: &mut [f32], buf_r: &mut [f32]) {
        let frames = buf_l.len().min(buf_r.len());
        if frames == 0 {
            return;
        }
        let mut scratch = self.scratch.borrow_mut();
        if scratch.len() < frames {
            scratch.resize(frames, 0.0);
        }
        // mid = 0.5 * (l + r) into the scratch, then side = 0.5 * (l - r) * w
        // in place of l; r is rebuilt as mid - side before l becomes mid + side.
        let mid = &mut scratch[..frames];
        crate::simd::copy_scaled_inplace(mid, &buf_l[..frames], 0.5);
        crate::simd::add_scaled_inplace(mid, &buf_r[..frames], 0.5);
        crate::simd::add_scaled_inplace(&mut buf_l[..frames], &buf_r[..frames], -1.0);
        crate::simd::mul_inplace(&mut buf_l[..frames], 0.5);
        crate::simd::mul_inplace(&mut buf_l[..frames], self.width);
        crate::simd::copy_scaled_inplace(&mut buf_r[..frames], mid, 1.0);
        crate::simd::add_scaled_inplace(&mut buf_r[..frames], &buf_l[..frames], -1.0);
        crate::simd::add_scaled_inplace(&mut buf_l[..frames], mid, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_boosts_by_db() {
        let mut g = Gain::new();
        g.set_db(6.0);
        let (l, r) = g.process(0.5, 0.5);
        assert!((l - 1.0).abs() < 0.01);
        assert!((r - 1.0).abs() < 0.01);
    }

    #[test]
    fn pan_right_mutes_left() {
        let p = Pan::from_pan(1.0);
        let (l, r) = p.process(1.0, 1.0);
        assert!(l.abs() < 0.1);
        assert!(r > 0.9);
    }

    #[test]
    fn width_zero_mutes_side() {
        let mut w = Width::new();
        w.set_width(0.0);
        let (l, r) = w.process(1.0, -1.0);
        assert!(l.abs() < 0.01);
        assert!(r.abs() < 0.01);
    }
}
