use rustfft::{FftPlanner, num_complex::Complex};

/// Reusable FFT-based spectrum analyzer.  Pre-allocates all working memory.
pub struct SpectrumAnalyzer {
    planner: FftPlanner<f32>,
    complex_buf: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
}

impl SpectrumAnalyzer {
    pub fn new(max_size: usize) -> Self {
        Self {
            planner: FftPlanner::new(),
            complex_buf: vec![Complex { re: 0.0, im: 0.0 }; max_size],
            scratch: vec![Complex { re: 0.0, im: 0.0 }; max_size],
        }
    }

    /// Compute magnitude spectrum of real mono audio.
    ///
    /// `input` is the time-domain audio buffer.
    /// `output` receives the magnitude per bin.
    ///
    /// This is real-time safe: no heap allocations after construction.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let n = input.len();
        if n == 0 {
            output.fill(0.0);
            return;
        }

        let fft = self.planner.plan_fft_forward(n);

        // Copy input into complex buffer (reuses allocation, no realloc).
        for (i, &s) in input.iter().enumerate() {
            self.complex_buf[i].re = s;
            self.complex_buf[i].im = 0.0;
        }
        for i in input.len()..n {
            self.complex_buf[i].re = 0.0;
            self.complex_buf[i].im = 0.0;
        }

        let scratch_len = fft.get_inplace_scratch_len();
        if self.scratch.len() < scratch_len {
            self.scratch.resize(scratch_len, Complex { re: 0.0, im: 0.0 });
        }
        fft.process_with_scratch(
            &mut self.complex_buf[..n],
            &mut self.scratch[..scratch_len],
        );

        let out_len = output.len().min(self.complex_buf.len());
        for i in 0..out_len {
            let c = self.complex_buf[i];
            output[i] = (c.re * c.re + c.im * c.im).sqrt();
        }
    }
}

/// Convert linear FFT magnitudes to dBFS, clamped to a floor.
pub fn magnitude_to_db(mag: &[f32], db_out: &mut [f32], floor_db: f32) {
    for (m, d) in mag.iter().zip(db_out.iter_mut()) {
        *d = if *m > 1.0e-8 {
            (20.0 * m.log10()).max(floor_db)
        } else {
            floor_db
        };
    }
}
