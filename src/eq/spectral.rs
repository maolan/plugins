use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

use crate::eq::dsp::{self, MAX_BANDS};

pub const SPECTRAL_FFT_SIZE: usize = 2048;
pub const SPECTRAL_HOP: usize = SPECTRAL_FFT_SIZE / 4;
/// One full analysis frame of delay while spectral dynamics is active; the
/// host is compensated through CLAP_EXT_LATENCY.
pub const SPECTRAL_LATENCY: u32 = SPECTRAL_FFT_SIZE as u32;

const NUM_BINS: usize = SPECTRAL_FFT_SIZE / 2 + 1;
/// Sense-filter region gate: bins whose sense response is within this many dB
/// of the +12 dB sense peak are inside the band's spectral region.
const REGION_WITHIN_PEAK_DB: f32 = 6.0;
/// Sense filters are built with +12 dB emphasis; the threshold contour is
/// referenced back to the band center by this offset.
const SENSE_REF_DB: f32 = 12.0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpectralBandConfig {
    pub on: bool,
    pub external: bool,
    pub freq: f32,
    pub q: f32,
    pub shape: u8,
    pub slope: u8,
    pub threshold_db: f32,
    pub ratio: f32,
    pub knee_db: f32,
    pub range_db: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
}

struct SpectralBand {
    config: SpectralBandConfig,
    contour: Vec<f32>,
    region: Vec<bool>,
    env_db: Vec<f32>,
    gain_db: Vec<f32>,
    attack_coef: f32,
    release_coef: f32,
    gain_smooth: f32,
    configured: bool,
}

impl SpectralBand {
    fn new() -> Self {
        Self {
            config: SpectralBandConfig::default(),
            contour: vec![0.0; NUM_BINS],
            region: vec![false; NUM_BINS],
            env_db: vec![-120.0; NUM_BINS],
            gain_db: vec![0.0; NUM_BINS],
            attack_coef: 0.0,
            release_coef: 0.0,
            gain_smooth: 0.0,
            configured: false,
        }
    }

    fn configure(&mut self, sample_rate: f32, config: SpectralBandConfig) {
        if self.configured && self.config == config {
            return;
        }
        self.config = config;
        self.configured = true;
        let hop_dt = SPECTRAL_HOP as f32 / sample_rate;
        let attack_s = (config.attack_ms * 0.001).max(1.0e-4);
        let release_s = (config.release_ms * 0.001).max(1.0e-3);
        self.attack_coef = (-hop_dt / attack_s).exp();
        self.release_coef = (-hop_dt / release_s).exp();
        self.gain_smooth = (-hop_dt / 0.010).exp();

        let sense = dsp::detector_biquad(config.shape, sample_rate, config.freq, config.q);
        let threshold_chain = dsp::build_chain(
            config.shape,
            config.slope,
            sample_rate,
            config.freq,
            config.q,
            config.threshold_db,
        );
        let bin_hz = sample_rate / SPECTRAL_FFT_SIZE as f32;
        for bin in 0..NUM_BINS {
            let freq = bin as f32 * bin_hz;
            if !(20.0..=20_000.0).contains(&freq) {
                self.region[bin] = false;
                self.contour[bin] = 0.0;
                continue;
            }
            let sense_db = sense.magnitude_db(freq, sample_rate);
            self.region[bin] = sense_db > SENSE_REF_DB - REGION_WITHIN_PEAK_DB;
            self.contour[bin] = threshold_chain
                .iter()
                .map(|bq| bq.magnitude_db(freq, sample_rate))
                .sum();
        }
    }

    /// Updates per-bin envelopes from the source magnitude spectrum and
    /// accumulates this band's gain (dB) into `total_gain_db`.
    fn apply(&mut self, mags_db: &[f32; NUM_BINS], total_gain_db: &mut [f32; NUM_BINS]) {
        let cfg = &self.config;
        let slope = 1.0 - 1.0 / cfg.ratio.max(1.0);
        let half_knee = cfg.knee_db * 0.5;
        for bin in 0..NUM_BINS {
            let m = mags_db[bin];
            let env = &mut self.env_db[bin];
            let coef = if m > *env {
                self.attack_coef
            } else {
                self.release_coef
            };
            *env = coef * *env + (1.0 - coef) * m;

            let target = if self.region[bin] {
                let over = *env - self.contour[bin];
                let gr = if cfg.knee_db > 0.0 && over.abs() <= half_knee {
                    slope * (over + half_knee) * (over + half_knee) / (2.0 * cfg.knee_db)
                } else if over > half_knee {
                    over * slope
                } else {
                    0.0
                };
                let gr = gr.clamp(0.0, cfg.range_db.abs());
                if cfg.range_db > 0.0 { -gr } else { gr }
            } else {
                0.0
            };
            let gain = &mut self.gain_db[bin];
            *gain = self.gain_smooth * *gain + (1.0 - self.gain_smooth) * target;
            total_gain_db[bin] += *gain;
        }
    }

    fn reset(&mut self) {
        self.env_db.fill(-120.0);
        self.gain_db.fill(0.0);
    }
}

/// Soothe/Pro-Q 4-style spectral dynamics: STFT analysis of the signal (and
/// optionally the external side chain), per-band per-bin dynamic gains that
/// only duck the frequencies inside a band's region that exceed its
/// threshold contour, and overlap-add resynthesis. Introduces
/// `SPECTRAL_LATENCY` samples of latency while active.
pub struct SpectralDynamics {
    fft: Arc<dyn Fft<f32>>,
    ifft: Arc<dyn Fft<f32>>,
    in_buf: [Vec<f32>; 2],
    sc_buf: [Vec<f32>; 2],
    ola: [Vec<f32>; 2],
    norm: Vec<f32>,
    window: Vec<f32>,
    frame: Vec<Complex32>,
    sc_frame: Vec<Complex32>,
    specs: [Vec<Complex32>; 2],
    sc_spec: Vec<Complex32>,
    bands: Vec<SpectralBand>,
    consumed: usize,
    emitted: usize,
}

impl Default for SpectralDynamics {
    fn default() -> Self {
        Self::new()
    }
}

impl SpectralDynamics {
    pub fn new() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(SPECTRAL_FFT_SIZE);
        let ifft = planner.plan_fft_inverse(SPECTRAL_FFT_SIZE);
        let window = (0..SPECTRAL_FFT_SIZE)
            .map(|i| {
                0.5 - 0.5
                    * (2.0 * std::f32::consts::PI * i as f32 / (SPECTRAL_FFT_SIZE - 1) as f32).cos()
            })
            .collect();
        Self {
            fft,
            ifft,
            in_buf: [vec![0.0; SPECTRAL_FFT_SIZE], vec![0.0; SPECTRAL_FFT_SIZE]],
            sc_buf: [vec![0.0; SPECTRAL_FFT_SIZE], vec![0.0; SPECTRAL_FFT_SIZE]],
            ola: [vec![0.0; SPECTRAL_FFT_SIZE], vec![0.0; SPECTRAL_FFT_SIZE]],
            norm: vec![0.0; SPECTRAL_FFT_SIZE],
            window,
            frame: vec![Complex32::ZERO; SPECTRAL_FFT_SIZE],
            sc_frame: vec![Complex32::ZERO; SPECTRAL_FFT_SIZE],
            specs: [
                vec![Complex32::ZERO; SPECTRAL_FFT_SIZE],
                vec![Complex32::ZERO; SPECTRAL_FFT_SIZE],
            ],
            sc_spec: vec![Complex32::ZERO; SPECTRAL_FFT_SIZE],
            bands: (0..MAX_BANDS).map(|_| SpectralBand::new()).collect(),
            consumed: 0,
            emitted: 0,
        }
    }

    pub fn reset(&mut self) {
        for buf in &mut self.in_buf {
            buf.fill(0.0);
        }
        for buf in &mut self.sc_buf {
            buf.fill(0.0);
        }
        for buf in &mut self.ola {
            buf.fill(0.0);
        }
        self.norm.fill(0.0);
        for band in &mut self.bands {
            band.reset();
        }
        self.consumed = 0;
        self.emitted = 0;
    }

    pub fn configure(&mut self, sample_rate: f32, configs: &[SpectralBandConfig]) {
        for (band, config) in self.bands.iter_mut().zip(configs.iter()) {
            band.configure(sample_rate, *config);
        }
    }

    pub fn any_external(&self) -> bool {
        self.bands.iter().any(|b| b.config.on && b.config.external)
    }

    pub fn band_gain_db<const N: usize>(&self, band: usize, sample_rate: f32) -> [f32; N] {
        let Some(band) = self.bands.get(band) else {
            return [0.0; N];
        };
        let bin_hz = sample_rate / SPECTRAL_FFT_SIZE as f32;
        std::array::from_fn(|i| {
            let t = i as f32 / (N.saturating_sub(1).max(1) as f32);
            let freq = 20.0_f32 * (20_000.0_f32 / 20.0_f32).powf(t);
            let bin = (freq / bin_hz).round() as usize;
            band.gain_db
                .get(bin.min(NUM_BINS - 1))
                .copied()
                .unwrap_or(0.0)
        })
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        sidechain: Option<(&[f32], &[f32])>,
    ) {
        let frames = left.len().min(right.len());
        for i in 0..frames {
            let pos = self.consumed % SPECTRAL_FFT_SIZE;
            self.in_buf[0][pos] = left[i];
            self.in_buf[1][pos] = right[i];
            if let Some((sc_l, sc_r)) = sidechain {
                self.sc_buf[0][pos] = sc_l[i];
                self.sc_buf[1][pos] = sc_r[i];
            }
            self.consumed += 1;

            if self.consumed >= SPECTRAL_FFT_SIZE
                && (self.consumed - SPECTRAL_FFT_SIZE).is_multiple_of(SPECTRAL_HOP)
            {
                self.process_frame(2);
            }

            if self.consumed > SPECTRAL_FFT_SIZE {
                let epos = self.emitted % SPECTRAL_FFT_SIZE;
                let n = self.norm[epos];
                left[i] = if n > 1.0e-6 {
                    self.ola[0][epos] / n
                } else {
                    0.0
                };
                right[i] = if n > 1.0e-6 {
                    self.ola[1][epos] / n
                } else {
                    0.0
                };
                self.ola[0][epos] = 0.0;
                self.ola[1][epos] = 0.0;
                self.norm[epos] = 0.0;
                self.emitted += 1;
            } else {
                left[i] = 0.0;
                right[i] = 0.0;
            }
        }
    }

    pub fn process_mono(&mut self, buffer: &mut [f32], sidechain: Option<&[f32]>) {
        for i in 0..buffer.len() {
            let pos = self.consumed % SPECTRAL_FFT_SIZE;
            self.in_buf[0][pos] = buffer[i];
            if let Some(sc) = sidechain {
                self.sc_buf[0][pos] = sc[i];
            }
            self.consumed += 1;

            if self.consumed >= SPECTRAL_FFT_SIZE
                && (self.consumed - SPECTRAL_FFT_SIZE).is_multiple_of(SPECTRAL_HOP)
            {
                self.process_frame(1);
            }

            if self.consumed > SPECTRAL_FFT_SIZE {
                let epos = self.emitted % SPECTRAL_FFT_SIZE;
                let n = self.norm[epos];
                buffer[i] = if n > 1.0e-6 {
                    self.ola[0][epos] / n
                } else {
                    0.0
                };
                self.ola[0][epos] = 0.0;
                self.norm[epos] = 0.0;
                self.emitted += 1;
            } else {
                buffer[i] = 0.0;
            }
        }
    }

    fn analyze(&mut self, channel: usize, sidechain: bool) {
        let (src, frame) = if sidechain {
            (&self.sc_buf[channel], &mut self.sc_frame)
        } else {
            (&self.in_buf[channel], &mut self.frame)
        };
        let start = self.consumed % SPECTRAL_FFT_SIZE;
        for (i, (f, w)) in frame.iter_mut().zip(self.window.iter()).enumerate() {
            let idx = (start + i) % SPECTRAL_FFT_SIZE;
            *f = Complex32::new(src[idx] * w, 0.0);
        }
        self.fft.process(frame);
        if sidechain {
            self.sc_spec.copy_from_slice(frame);
        } else {
            self.specs[channel].copy_from_slice(frame);
        }
    }

    fn process_frame(&mut self, channels: usize) {
        for ch in 0..channels {
            self.analyze(ch, false);
        }
        let need_sc = self.any_external();
        if need_sc {
            for ch in 0..channels {
                self.analyze(ch, true);
            }
        }

        // Amplitude scale for a complex FFT of a real, Hann-windowed signal.
        let scale = 4.0 / SPECTRAL_FFT_SIZE as f32;
        let mut mags_db = [0.0_f32; NUM_BINS];
        let mut total_gain_db = [0.0_f32; NUM_BINS];

        for band in &mut self.bands {
            if !band.config.on {
                continue;
            }
            for (bin, m_db) in mags_db.iter_mut().enumerate() {
                let mut mag = self.specs[0][bin].norm();
                if band.config.external {
                    mag = self.sc_spec[bin].norm();
                }
                if channels > 1 {
                    let other = if band.config.external {
                        self.sc_spec[bin].norm()
                    } else {
                        self.specs[1][bin].norm()
                    };
                    mag = mag.max(other);
                }
                let mag = mag * scale;
                *m_db = if mag > 1.0e-7 {
                    20.0 * mag.log10()
                } else {
                    -140.0
                };
            }
            band.apply(&mags_db, &mut total_gain_db);
        }

        // Frequency smoothing of the gain mask (3-tap) to avoid musical noise.
        let mut smoothed = total_gain_db;
        for bin in 1..NUM_BINS - 1 {
            smoothed[bin] = 0.25 * total_gain_db[bin - 1]
                + 0.5 * total_gain_db[bin]
                + 0.25 * total_gain_db[bin + 1];
        }

        for ch in 0..channels {
            for (bin, spec) in self.specs[ch].iter_mut().enumerate().take(NUM_BINS) {
                let g = dsp::db_to_gain(smoothed[bin].clamp(-60.0, 36.0));
                *spec *= g;
            }
            for bin in NUM_BINS..SPECTRAL_FFT_SIZE {
                let mirrored = SPECTRAL_FFT_SIZE - bin;
                let g = dsp::db_to_gain(smoothed[mirrored].clamp(-60.0, 36.0));
                self.specs[ch][bin] *= g;
            }
        }

        let start = self.consumed - SPECTRAL_FFT_SIZE;
        for ch in 0..channels {
            self.frame.copy_from_slice(&self.specs[ch]);
            self.ifft.process(&mut self.frame);
            for i in 0..SPECTRAL_FFT_SIZE {
                let slot = (start + i) % SPECTRAL_FFT_SIZE;
                self.ola[ch][slot] += self.frame[i].re * self.window[i] / SPECTRAL_FFT_SIZE as f32;
                if ch == 0 {
                    self.norm[slot] += self.window[i] * self.window[i];
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bell_threshold_contour_follows_displayed_curve() {
        let sample_rate = 48_000.0;
        let mut band = SpectralBand::new();
        band.configure(
            sample_rate,
            SpectralBandConfig {
                on: true,
                external: false,
                freq: 1000.0,
                q: 1.0,
                shape: dsp::SHAPE_BELL,
                slope: 0,
                threshold_db: 12.0,
                ratio: 2.0,
                knee_db: 0.0,
                range_db: 6.0,
                attack_ms: 10.0,
                release_ms: 100.0,
            },
        );

        let bin_hz = sample_rate / SPECTRAL_FFT_SIZE as f32;
        let center = (1000.0 / bin_hz).round() as usize;
        let edge = (2000.0 / bin_hz).round() as usize;

        assert!(band.contour[center] > 10.0);
        assert!(band.contour[center] > band.contour[edge] + 3.0);
    }
}
