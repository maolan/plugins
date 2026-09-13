//! ITU-R BS.1770 K-weighted loudness metering for a stereo channel pair:
//! momentary (400 ms), short-term (3 s), and integrated (gated) LUFS.
//!
//! The K-weighting filters are Brecht DeMan's revised BS.1770 stage 1 high
//! shelf (+4 dB) and stage 2 high pass, designed by bilinear transform so
//! the coefficients follow the sample rate (the RBJ cookbook shelf with the
//! same nominal parameters does not reproduce the ITU coefficients).

const MOMENTARY_MS: f64 = 400.0;
const SHORT_TERM_MS: f64 = 3000.0;
const INTEGRATED_HOP_MS: f64 = 100.0;
/// BS.1770-4 loudness offset applied to the channel-averaged mean square.
const LOUDNESS_OFFSET: f64 = -0.691;
const ABSOLUTE_GATE_LUFS: f64 = -70.0;
const RELATIVE_GATE_LU: f64 = -10.0;

#[derive(Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    /// BS.1770 stage 1: high-frequency shelf (+4 dB) using DeMan's revised
    /// design. The RBJ cookbook shelf with the same nominal parameters does
    /// not reproduce the ITU coefficients; this does (see BrechtDeMan's
    /// loudness.py and pyloudnorm's `high_shelf_DeMan`).
    fn bs1770_high_shelf(sample_rate: f64) -> Self {
        const GAIN_DB: f64 = 3.999843853973347;
        const Q: f64 = 0.7071752369554193;
        const CUTOFF: f64 = 1_681.974_450_955_532;
        let k = (std::f64::consts::PI * CUTOFF / sample_rate).tan();
        let vh = 10.0_f64.powf(GAIN_DB / 20.0);
        let vb = vh.powf(0.499666774155);
        let k_over_q = k / Q;
        let norm = 1.0 + k_over_q + k * k;
        Self {
            b0: (vh + vb * k_over_q + k * k) / norm,
            b1: 2.0 * (k * k - vh) / norm,
            b2: (vh - vb * k_over_q + k * k) / norm,
            a1: 2.0 * (k * k - 1.0) / norm,
            a2: (1.0 - k_over_q + k * k) / norm,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// BS.1770 stage 2: high pass at 38 Hz using DeMan's revised design.
    fn bs1770_high_pass(sample_rate: f64) -> Self {
        const Q: f64 = 0.5003270373253953;
        const CUTOFF: f64 = 38.13547087613982;
        let k = (std::f64::consts::PI * CUTOFF / sample_rate).tan();
        let k_over_q = k / Q;
        let norm = 1.0 + k_over_q + k * k;
        Self {
            b0: 1.0,
            b1: -2.0,
            b2: 1.0,
            a1: 2.0 * (k * k - 1.0) / norm,
            a2: (1.0 - k_over_q + k * k) / norm,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// Ring buffer of squared samples with an incremental running sum, so the
/// trailing-window mean square is available at any time in O(1).
struct SquaredWindow {
    buf: Vec<f64>,
    pos: usize,
    filled: usize,
    sum: f64,
}

impl SquaredWindow {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            pos: 0,
            filled: 0,
            sum: 0.0,
        }
    }

    fn push(&mut self, sq: f64) {
        let old = self.buf[self.pos];
        self.buf[self.pos] = sq;
        self.sum += sq - old;
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
        self.filled = (self.filled + 1).min(self.buf.len());
    }

    fn mean_square(&self) -> f64 {
        if self.filled == 0 {
            0.0
        } else {
            self.sum / self.filled as f64
        }
    }

    fn is_full(&self) -> bool {
        self.filled == self.buf.len()
    }

    fn reset(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
        self.filled = 0;
        self.sum = 0.0;
    }
}

struct ChannelMeter {
    shelf: Biquad,
    high_pass: Biquad,
    momentary: SquaredWindow,
    short_term: SquaredWindow,
}

impl ChannelMeter {
    fn new(sample_rate: f64) -> Self {
        Self {
            shelf: Biquad::bs1770_high_shelf(sample_rate),
            high_pass: Biquad::bs1770_high_pass(sample_rate),
            momentary: SquaredWindow::new(samples_for_ms(sample_rate, MOMENTARY_MS)),
            short_term: SquaredWindow::new(samples_for_ms(sample_rate, SHORT_TERM_MS)),
        }
    }

    fn step(&mut self, x: f64) {
        let y = self.high_pass.process(self.shelf.process(x));
        let sq = y * y;
        self.momentary.push(sq);
        self.short_term.push(sq);
    }

    fn reset(&mut self) {
        self.shelf.reset();
        self.high_pass.reset();
        self.momentary.reset();
        self.short_term.reset();
    }
}

fn samples_for_ms(sample_rate: f64, ms: f64) -> usize {
    (sample_rate * ms / 1000.0).round().max(1.0) as usize
}

fn loudness(energy: f64) -> f64 {
    if energy > 0.0 {
        LOUDNESS_OFFSET + 10.0 * energy.log10()
    } else {
        f64::NEG_INFINITY
    }
}

/// Stereo BS.1770 loudness meter.
pub struct LufsMeter {
    channels: [ChannelMeter; 2],
    hop_len: usize,
    hop_remaining: usize,
    /// Channel-averaged 400 ms energies, one per 100 ms hop, for the
    /// integrated measurement's absolute and relative gating.
    blocks: Vec<f64>,
}

impl LufsMeter {
    pub fn new(sample_rate: f64) -> Self {
        let hop_len = samples_for_ms(sample_rate, INTEGRATED_HOP_MS);
        Self {
            channels: [
                ChannelMeter::new(sample_rate),
                ChannelMeter::new(sample_rate),
            ],
            hop_len,
            hop_remaining: hop_len,
            blocks: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        for channel in &mut self.channels {
            channel.reset();
        }
        self.hop_remaining = self.hop_len;
        self.blocks.clear();
    }

    /// Feed one block of interleaved-by-slice stereo samples.
    pub fn process(&mut self, left: &[f32], right: &[f32]) {
        for (&l, &r) in left.iter().zip(right.iter()) {
            self.channels[0].step(l as f64);
            self.channels[1].step(r as f64);
            self.hop_remaining = self.hop_remaining.saturating_sub(1);
            if self.hop_remaining == 0 {
                self.hop_remaining = self.hop_len;
                if self.channels.iter().all(|c| c.momentary.is_full()) {
                    let energy = self
                        .channels
                        .iter()
                        .map(|c| c.momentary.mean_square())
                        .sum::<f64>()
                        / self.channels.len() as f64;
                    self.blocks.push(energy);
                }
            }
        }
    }

    pub fn momentary_lufs(&self) -> f64 {
        loudness(self.window_energy(|c| &c.momentary))
    }

    pub fn short_term_lufs(&self) -> f64 {
        loudness(self.window_energy(|c| &c.short_term))
    }

    fn window_energy(&self, window: impl Fn(&ChannelMeter) -> &SquaredWindow) -> f64 {
        self.channels
            .iter()
            .map(|c| window(c).mean_square())
            .sum::<f64>()
            / self.channels.len() as f64
    }

    pub fn integrated_lufs(&self) -> f64 {
        if self.blocks.is_empty() {
            return f64::NEG_INFINITY;
        }
        let absolute_gate = 10.0_f64.powf((ABSOLUTE_GATE_LUFS - LOUDNESS_OFFSET) / 10.0);
        let gated: Vec<f64> = self
            .blocks
            .iter()
            .copied()
            .filter(|&e| e >= absolute_gate)
            .collect();
        if gated.is_empty() {
            return f64::NEG_INFINITY;
        }
        let gated_mean = gated.iter().sum::<f64>() / gated.len() as f64;
        let relative_gate = gated_mean * 10.0_f64.powf(RELATIVE_GATE_LU / 10.0);
        let final_blocks: Vec<f64> = gated.into_iter().filter(|&e| e >= relative_gate).collect();
        if final_blocks.is_empty() {
            return f64::NEG_INFINITY;
        }
        let final_mean = final_blocks.iter().sum::<f64>() / final_blocks.len() as f64;
        LOUDNESS_OFFSET + 10.0 * final_mean.log10()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFERENCE_TOLERANCE: f64 = 1.0e-3;

    fn sine(sample_rate: f64, freq: f64, seconds: f64) -> Vec<f32> {
        let n = (sample_rate * seconds) as usize;
        (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / sample_rate).sin() as f32)
            .collect()
    }

    #[test]
    fn k_weighting_matches_bs1770_at_48k() {
        let shelf = Biquad::bs1770_high_shelf(48_000.0);
        assert!((shelf.b0 - 1.53512485958697).abs() < REFERENCE_TOLERANCE);
        assert!((shelf.b1 - -2.69169618940638).abs() < REFERENCE_TOLERANCE);
        assert!((shelf.b2 - 1.19839281085285).abs() < REFERENCE_TOLERANCE);
        assert!((shelf.a1 - -1.69065929318241).abs() < REFERENCE_TOLERANCE);
        assert!((shelf.a2 - 0.73248077421585).abs() < REFERENCE_TOLERANCE);

        let hp = Biquad::bs1770_high_pass(48_000.0);
        assert!((hp.b0 - 1.0).abs() < REFERENCE_TOLERANCE);
        assert!((hp.b1 - -2.0).abs() < REFERENCE_TOLERANCE);
        assert!((hp.b2 - 1.0).abs() < REFERENCE_TOLERANCE);
        assert!((hp.a1 - -1.99004745483398).abs() < REFERENCE_TOLERANCE);
        assert!((hp.a2 - 0.99007225036621).abs() < REFERENCE_TOLERANCE);
    }

    #[test]
    fn full_scale_1khz_sine_measures_expected_lufs() {
        let sample_rate = 48_000.0;
        let mut meter = LufsMeter::new(sample_rate);
        let signal = sine(sample_rate, 1000.0, 1.0);
        meter.process(&signal, &signal);

        // Mean square of a full-scale sine is 0.5, and BS.1770 K-weighting
        // gain at 1 kHz is +0.698 dB, so the expected loudness is
        // -0.691 + 10*log10(0.5) + 0.698 = -3.00 LUFS.
        let momentary = meter.momentary_lufs();
        assert!((momentary - -3.004).abs() < 0.1, "momentary {momentary}");
        let short_term = meter.short_term_lufs();
        assert!((short_term - -3.004).abs() < 0.1, "short term {short_term}");
        let integrated = meter.integrated_lufs();
        assert!((integrated - -3.004).abs() < 0.1, "integrated {integrated}");
    }

    #[test]
    fn integrated_gating_excludes_silence() {
        let sample_rate = 48_000.0;
        let mut meter = LufsMeter::new(sample_rate);
        // Silence first: its blocks fall below the -70 LUFS absolute gate and
        // must not drag the measurement down. The first three loud blocks
        // still ramp up through the trailing window (25%, 50%, 75% of the
        // steady energy); BS.1770 counts them, so the expectation below
        // includes them.
        let silence = vec![0.0_f32; (sample_rate * 3.0) as usize];
        let loud = sine(sample_rate, 1000.0, 3.0);
        meter.process(&silence, &silence);
        meter.process(&loud, &loud);
        let integrated = meter.integrated_lufs();
        let expected =
            -0.691 + 10.0 * (27.0_f64 * 0.5875 + 0.881_25).log10() - 10.0 * (30.0_f64).log10();
        assert!(
            (integrated - expected).abs() < 0.05,
            "integrated {integrated}"
        );
    }

    #[test]
    fn relative_gate_drops_outlier_blocks() {
        let sample_rate = 48_000.0;
        let mut meter = LufsMeter::new(sample_rate);
        // The -43 LUFS blocks (and their three ramp blocks) pass the absolute
        // gate but fall more than 10 LU below the loud blocks, so the
        // relative gate excludes them; the loud blocks' own ramp blocks are
        // counted, as in the silence test above.
        let quiet_scale = 0.01_f32;
        let quiet: Vec<f32> = sine(sample_rate, 1000.0, 4.0)
            .iter()
            .map(|&s| s * quiet_scale)
            .collect();
        let loud = sine(sample_rate, 1000.0, 4.0);
        meter.process(&quiet, &quiet);
        meter.process(&loud, &loud);
        let integrated = meter.integrated_lufs();
        let expected =
            -0.691 + 10.0 * (37.0_f64 * 0.5875 + 0.881_25).log10() - 10.0 * (40.0_f64).log10();
        assert!(
            (integrated - expected).abs() < 0.05,
            "integrated {integrated}"
        );
    }

    #[test]
    fn silence_and_fresh_meter_report_negative_infinity() {
        let sample_rate = 48_000.0;
        let mut meter = LufsMeter::new(sample_rate);
        assert_eq!(meter.momentary_lufs(), f64::NEG_INFINITY);
        assert_eq!(meter.short_term_lufs(), f64::NEG_INFINITY);
        assert_eq!(meter.integrated_lufs(), f64::NEG_INFINITY);

        let signal = sine(sample_rate, 1000.0, 1.0);
        meter.process(&signal, &signal);
        meter.reset();
        let silence = vec![0.0_f32; (sample_rate * 0.5) as usize];
        meter.process(&silence, &silence);
        assert_eq!(meter.momentary_lufs(), f64::NEG_INFINITY);
        assert_eq!(meter.integrated_lufs(), f64::NEG_INFINITY);
    }

    #[test]
    fn meter_is_finite_at_44k1() {
        let sample_rate = 44_100.0;
        let mut meter = LufsMeter::new(sample_rate);
        let signal = sine(sample_rate, 997.0, 1.0);
        meter.process(&signal, &signal);
        assert!(meter.momentary_lufs().is_finite());
        assert!(meter.short_term_lufs().is_finite());
        assert!(meter.integrated_lufs().is_finite());
    }
}
