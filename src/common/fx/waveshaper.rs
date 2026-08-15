#[derive(Debug, Clone, Copy, Default)]
pub struct Waveshaper {
    drive: f32,
}

impl Waveshaper {
    pub fn new() -> Self {
        Self { drive: 0.0 }
    }

    pub fn set_drive(&mut self, drive: f32) {
        self.drive = drive;
    }

    pub fn reset(&mut self) {
        self.drive = 0.0;
    }

    pub fn process(&self, input_l: f32, input_r: f32) -> (f32, f32) {
        if self.drive < 1.0e-6 {
            return (input_l, input_r);
        }
        let drive = 1.0 + self.drive * 10.0;
        ((input_l * drive).tanh(), (input_r * drive).tanh())
    }

    pub fn process_block(&self, buf_l: &mut [f32], buf_r: &mut [f32]) {
        if self.drive < 1.0e-6 {
            return;
        }
        for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
            (*l, *r) = self.process(*l, *r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveshaper_clips_high_drive() {
        let mut w = Waveshaper::new();
        w.set_drive(1.0);
        let mut l = vec![10.0f32; 4];
        let mut r = vec![10.0f32; 4];
        w.process_block(&mut l, &mut r);
        assert!(l[0] < 1.1);
        assert!(l[0] > 0.9);
    }
}
