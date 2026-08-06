use crate::common::fft::SpectrumAnalyzer;

pub const FFT_SIZE: usize = 4096;
pub const SPECTRUM_FLOOR_DB: f32 = -90.0;

/// Real-time spectrum analyzer feeding the EQ display: single-channel ring
/// buffer, Hann-windowed 4096-point FFT, max-magnitude remap onto
/// log-spaced display bins, and peak-hold/decay smoothing per bin.
pub struct LogSpectrumAnalyzer {
    fft: SpectrumAnalyzer,
    ring: Vec<f32>,
    write_pos: usize,
    hann: Vec<f32>,
    windowed: Vec<f32>,
    mags: Vec<f32>,
    smoothed_db: Vec<f32>,
    /// Geometric edges of the display bins, as FFT bin indices at 48 kHz
    /// reference — recomputed per `compute` call for the actual sample rate.
    bins: usize,
}

impl LogSpectrumAnalyzer {
    pub fn new(bins: usize) -> Self {
        let hann: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos()
            })
            .collect();
        Self {
            fft: SpectrumAnalyzer::new(FFT_SIZE),
            ring: vec![0.0; FFT_SIZE],
            write_pos: 0,
            hann,
            windowed: vec![0.0; FFT_SIZE],
            mags: vec![0.0; FFT_SIZE / 2 + 1],
            smoothed_db: vec![SPECTRUM_FLOOR_DB; bins],
            bins,
        }
    }

    pub fn reset(&mut self) {
        self.ring.fill(0.0);
        self.write_pos = 0;
        self.smoothed_db.fill(SPECTRUM_FLOOR_DB);
    }

    pub fn push_block(&mut self, samples: &[f32]) {
        for &s in samples {
            self.ring[self.write_pos] = s;
            self.write_pos = (self.write_pos + 1) % FFT_SIZE;
        }
    }

    /// Computes the smoothed log-binned spectrum into `out` (dB, floored at
    /// -90). `out.len()` must equal the `bins` this analyzer was created
    /// with. Designed to be called at display rate (~10 Hz), not per block.
    pub fn compute(&mut self, sample_rate: f32, out: &mut [f32]) {
        if out.len() != self.bins || sample_rate <= 0.0 {
            return;
        }
        // Oldest-to-newest copy, Hann windowed.
        for i in 0..FFT_SIZE {
            let idx = (self.write_pos + i) % FFT_SIZE;
            self.windowed[i] = self.ring[idx] * self.hann[i];
        }
        self.fft.process(&self.windowed, &mut self.mags);

        // Hann coherent gain 0.5 and one-sided spectrum: a full-scale sine
        // peaks at its true amplitude with scale 4/N.
        let scale = 4.0 / FFT_SIZE as f32;
        let fft_bins = FFT_SIZE / 2 + 1;
        let bin_hz = sample_rate / FFT_SIZE as f32;

        let f_lo = 20.0_f32;
        let f_hi = (sample_rate * 0.45).min(20_000.0);
        let ratio = f_hi / f_lo;
        let n = self.bins as f32;

        // Per-tick decay so peaks fall smoothly between computes.
        const DECAY_DB_PER_TICK: f32 = 9.0;

        for (i, o) in out.iter_mut().enumerate() {
            let f_edge_lo = f_lo * ratio.powf(i as f32 / n);
            let f_edge_hi = f_lo * ratio.powf((i + 1) as f32 / n);
            let mut k_lo = (f_edge_lo / bin_hz).floor() as usize;
            let mut k_hi = (f_edge_hi / bin_hz).ceil() as usize;
            k_lo = k_lo.min(fft_bins - 1);
            k_hi = k_hi.clamp(k_lo + 1, fft_bins);

            let mut best_db = SPECTRUM_FLOOR_DB;
            for k in k_lo..k_hi {
                let mag = self.mags[k] * scale;
                if mag > 1.0e-7 {
                    let db = 20.0 * mag.log10();
                    if db > best_db {
                        best_db = db;
                    }
                }
            }

            let smoothed = if best_db >= self.smoothed_db[i] {
                best_db
            } else {
                (self.smoothed_db[i] - DECAY_DB_PER_TICK).max(best_db)
            };
            self.smoothed_db[i] = smoothed.clamp(SPECTRUM_FLOOR_DB, 20.0);
            *o = self.smoothed_db[i];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_scale_sine_peaks_near_zero_dbfs_at_its_bin() {
        let sr = 48_000.0;
        let bins = 192;
        let mut analyzer = LogSpectrumAnalyzer::new(bins);
        let mut pos = 0usize;
        // Feed enough blocks to fill the ring completely.
        while pos < FFT_SIZE * 2 {
            let block: Vec<f32> = (0..512)
                .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * (pos + i) as f32 / sr).sin())
                .collect();
            pos += 512;
            analyzer.push_block(&block);
        }
        let mut out = vec![0.0_f32; bins];
        analyzer.compute(sr, &mut out);
        let peak = out.iter().copied().fold(-200.0_f32, f32::max);
        assert!(peak > -3.5, "peak read {peak} dB");
        assert!(peak < 1.0, "peak read {peak} dB");
        // Bins far from 1 kHz stay well below the peak.
        let bin_hz = |f: f32| {
            let t = (f / 20.0).ln() / (20_000.0_f32 / 20.0).ln();
            (t * bins as f32) as usize
        };
        let at_100 = out[bin_hz(100.0)];
        assert!(at_100 < peak - 40.0, "100 Hz bin at {at_100} dB");
    }
}
