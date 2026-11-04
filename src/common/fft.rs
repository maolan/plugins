use std::sync::Arc;

use rustfft::{Fft, FftPlanner, num_complex::Complex};

pub struct SpectrumAnalyzer {
    fft: Arc<dyn Fft<f32>>,
    complex_buf: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
}

impl SpectrumAnalyzer {
    pub fn new(max_size: usize) -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(max_size);
        let scratch = vec![Complex { re: 0.0, im: 0.0 }; fft.get_inplace_scratch_len()];
        Self {
            fft,
            complex_buf: vec![Complex { re: 0.0, im: 0.0 }; max_size],
            scratch,
        }
    }

    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        if input.is_empty() {
            output.fill(0.0);
            return;
        }

        for (complex, &sample) in self.complex_buf.iter_mut().zip(input) {
            *complex = Complex {
                re: sample,
                im: 0.0,
            };
        }
        self.complex_buf[input.len()..].fill(Complex { re: 0.0, im: 0.0 });
        self.fft
            .process_with_scratch(&mut self.complex_buf, &mut self.scratch);

        let out_len = output.len().min(self.complex_buf.len());
        for (c, o) in self.complex_buf[..out_len]
            .iter()
            .zip(output[..out_len].iter_mut())
        {
            *o = (c.re * c.re + c.im * c.im).sqrt();
        }
    }
}

pub fn magnitude_to_db(mag: &[f32], db_out: &mut [f32], floor_db: f32) {
    for (m, d) in mag.iter().zip(db_out.iter_mut()) {
        *d = if *m > 1.0e-8 {
            (20.0 * m.log10()).max(floor_db)
        } else {
            floor_db
        };
    }
}
