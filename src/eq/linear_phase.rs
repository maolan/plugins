use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

use crate::eq::dsp;

/// Linear Phase processing mode: the aggregate static response of all active
/// bands (including their current dynamic gains, refreshed at block rate) is
/// turned into a symmetric 4096-tap FIR and applied by overlap-add block
/// convolution. Latency is `LP_LATENCY` samples and is reported to the host
/// through CLAP_EXT_LATENCY while the mode is active.
pub const LP_IR_LEN: usize = 4096;
pub const LP_BLOCK: usize = 1024;
pub const LP_FFT: usize = 8192;
/// FIR group delay (LP_IR_LEN / 2) + one block of convolution scheduling.
pub const LP_LATENCY: u32 = (LP_IR_LEN / 2 + LP_BLOCK - 1) as u32;

/// Static band description used for FIR design.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandDesign {
    pub on: bool,
    pub shape: u8,
    pub slope: u8,
    pub freq: f32,
    pub q: f32,
    pub gain_db: f32,
}

pub struct LinearPhaseEq {
    fft: Arc<dyn Fft<f32>>,
    ifft: Arc<dyn Fft<f32>>,
    ir_spec: Vec<Complex32>,
    in_hist: [Vec<f32>; 2],
    ola: [Vec<f32>; 2],
    frame: Vec<Complex32>,
    product: Vec<Complex32>,
    designs: Vec<BandDesign>,
    redesign_pending: bool,
    redesign_countdown: usize,
    consumed: usize,
    emitted: usize,
}

impl Default for LinearPhaseEq {
    fn default() -> Self {
        Self::new()
    }
}

impl LinearPhaseEq {
    pub fn new() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(LP_FFT);
        let ifft = planner.plan_fft_inverse(LP_FFT);
        Self {
            fft,
            ifft,
            ir_spec: vec![Complex32::ONE; LP_FFT],
            in_hist: [vec![0.0; LP_FFT], vec![0.0; LP_FFT]],
            ola: [vec![0.0; LP_FFT], vec![0.0; LP_FFT]],
            frame: vec![Complex32::ZERO; LP_FFT],
            product: vec![Complex32::ZERO; LP_FFT],
            designs: Vec::new(),
            redesign_pending: true,
            redesign_countdown: 0,
            consumed: 0,
            emitted: 0,
        }
    }

    pub fn reset(&mut self) {
        for buf in &mut self.in_hist {
            buf.fill(0.0);
        }
        for buf in &mut self.ola {
            buf.fill(0.0);
        }
        self.consumed = 0;
        self.emitted = 0;
    }

    /// Updates the band designs. The FIR is redesigned at most ~10 times per
    /// second; intermediate updates are coalesced.
    pub fn set_bands(&mut self, designs: &[BandDesign], sample_rate: f32) {
        if designs == self.designs.as_slice() && !self.redesign_pending {
            return;
        }
        if self.redesign_countdown == 0 {
            self.redesign(designs, sample_rate);
            // ~100 ms between redesigns at typical block rates.
            self.redesign_countdown = (sample_rate as usize / LP_BLOCK / 10).max(1);
        } else {
            self.redesign_pending = true;
            self.designs = designs.to_vec();
        }
    }

    fn redesign(&mut self, designs: &[BandDesign], sample_rate: f32) {
        self.designs = designs.to_vec();
        self.redesign_pending = false;

        let bin_hz = sample_rate / LP_FFT as f32;
        let mut spec = vec![Complex32::ZERO; LP_FFT];
        for (k, h) in spec.iter_mut().enumerate().take(LP_FFT / 2 + 1) {
            let freq = k as f32 * bin_hz;
            let mut re = 1.0_f32;
            let mut im = 0.0_f32;
            for design in designs {
                if !design.on || freq < 1.0 {
                    continue;
                }
                let chain = dsp::build_chain(
                    design.shape,
                    design.slope,
                    sample_rate,
                    design.freq,
                    design.q,
                    design.gain_db,
                );
                for bq in &chain {
                    let (br, bi) = bq.transfer(freq, sample_rate);
                    let tr = re * br - im * bi;
                    im = re * bi + im * br;
                    re = tr;
                }
            }
            *h = Complex32::new(re, im);
        }
        for k in LP_FFT / 2 + 1..LP_FFT {
            spec[k] = spec[LP_FFT - k].conj();
        }

        self.frame.copy_from_slice(&spec);
        self.ifft.process(&mut self.frame);

        // Extract the centered LP_IR_LEN tap window and Hann-window it.
        let mut ir = vec![0.0_f32; LP_FFT];
        for (i, tap) in ir.iter_mut().enumerate().take(LP_IR_LEN) {
            let src = (i + LP_FFT - LP_IR_LEN / 2) % LP_FFT;
            let w =
                0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (LP_IR_LEN - 1) as f32).cos();
            *tap = self.frame[src].re * w / LP_FFT as f32;
        }
        self.frame.iter_mut().for_each(|c| *c = Complex32::ZERO);
        for (i, c) in self.frame.iter_mut().enumerate().take(LP_IR_LEN) {
            *c = Complex32::new(ir[i], 0.0);
        }
        self.fft.process(&mut self.frame);
        self.ir_spec.copy_from_slice(&self.frame);
    }

    fn process_block(&mut self, channels: usize) {
        let start = self.consumed - LP_BLOCK;
        for ch in 0..channels {
            for (i, f) in self.frame.iter_mut().enumerate() {
                *f = if i < LP_BLOCK {
                    Complex32::new(self.in_hist[ch][(start + i) % LP_FFT], 0.0)
                } else {
                    Complex32::ZERO
                };
            }
            self.fft.process(&mut self.frame);
            for (i, p) in self.product.iter_mut().enumerate() {
                *p = self.frame[i] * self.ir_spec[i];
            }
            self.ifft.process(&mut self.product);
            let scale = 1.0 / LP_FFT as f32;
            for (i, p) in self
                .product
                .iter()
                .enumerate()
                .take(LP_BLOCK + LP_IR_LEN - 1)
            {
                self.ola[ch][(start + i) % LP_FFT] += p.re * scale;
            }
        }
        if self.redesign_countdown > 0 {
            self.redesign_countdown -= 1;
        }
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32]) {
        let frames = left.len().min(right.len());
        for i in 0..frames {
            let pos = self.consumed % LP_FFT;
            self.in_hist[0][pos] = left[i];
            self.in_hist[1][pos] = right[i];
            self.consumed += 1;

            if self.consumed.is_multiple_of(LP_BLOCK) {
                self.process_block(2);
            }

            if self.consumed >= LP_BLOCK {
                let epos = self.emitted % LP_FFT;
                left[i] = self.ola[0][epos];
                right[i] = self.ola[1][epos];
                self.ola[0][epos] = 0.0;
                self.ola[1][epos] = 0.0;
                self.emitted += 1;
            } else {
                left[i] = 0.0;
                right[i] = 0.0;
            }
        }
    }

    pub fn process_mono(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            let pos = self.consumed % LP_FFT;
            self.in_hist[0][pos] = *sample;
            self.consumed += 1;

            if self.consumed.is_multiple_of(LP_BLOCK) {
                self.process_block(1);
            }

            if self.consumed >= LP_BLOCK {
                let epos = self.emitted % LP_FFT;
                *sample = self.ola[0][epos];
                self.ola[0][epos] = 0.0;
                self.emitted += 1;
            } else {
                *sample = 0.0;
            }
        }
    }
}
