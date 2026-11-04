use pitch_detection::detector::{PitchDetector, yin::YINDetector};

/// Result of a single pitch-detection pass.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TuningResult {
    /// Detected fundamental frequency in Hz, or `0.0` if no pitch was found.
    pub frequency_hz: f32,
    /// Estimated clarity of the detection in `[0.0, 1.0]`.
    pub clarity: f32,
    /// MIDI note number corresponding to the detected frequency.
    pub note: f32,
    /// Deviation from the nearest equal-tempered note in cents.
    pub cents: f32,
    /// Whether a stable pitch was detected this pass.
    pub detected: bool,
}

/// Real-time monophonic pitch detector using the YIN algorithm.
pub struct Tuner {
    sample_rate: f64,
    window_size: usize,
    hop_size: usize,
    power_threshold: f64,
    clarity_threshold: f64,
    reference_hz: f32,
    detector: YINDetector<f64>,
    buffer: Vec<f64>,
    work: Vec<f64>,
    /// Number of samples collected since the last reported detection.
    samples_since_report: usize,
}

impl Tuner {
    /// Create a new tuner for the given sample rate.
    ///
    /// Uses a 4096-sample analysis window with 50 % overlap, which gives
    /// reliable detection down to roughly 40 Hz at 48 kHz.
    pub fn new(sample_rate: f64) -> Self {
        let window_size = 4096;
        let hop_size = window_size / 2;
        let padding = window_size / 2;
        let mut detector = YINDetector::new(window_size, padding);
        let work = vec![1.0; window_size];
        detector.get_pitch(&work, sample_rate as usize, 5.0, 0.7);
        Self {
            sample_rate,
            window_size,
            hop_size,
            power_threshold: 5.0,
            clarity_threshold: 0.7,
            reference_hz: 440.0,
            detector,
            buffer: Vec::with_capacity(window_size),
            work,
            samples_since_report: 0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
    }

    pub fn set_reference_hz(&mut self, hz: f32) {
        self.reference_hz = hz.max(1.0);
    }

    pub fn set_clarity_threshold(&mut self, threshold: f32) {
        self.clarity_threshold = threshold as f64;
    }

    /// Reset the internal buffer and detector state.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.samples_since_report = 0;
    }

    fn frequency_to_note(&self, frequency: f32) -> f32 {
        if frequency <= 0.0 || !frequency.is_finite() {
            return 0.0;
        }
        69.0 + 12.0 * (frequency / self.reference_hz).log2()
    }

    fn note_to_cents(note: f32) -> f32 {
        if !note.is_finite() {
            return 0.0;
        }
        let rounded = note.round();
        (note - rounded) * 100.0
    }

    fn analyze_window(&mut self) -> TuningResult {
        self.work.copy_from_slice(&self.buffer[..self.window_size]);

        let result = self
            .detector
            .get_pitch(
                &self.work,
                self.sample_rate as usize,
                self.power_threshold,
                self.clarity_threshold,
            )
            .and_then(|pitch| {
                let frequency = pitch.frequency as f32;
                if frequency.is_finite() && frequency > 0.0 {
                    Some((frequency, pitch.clarity as f32))
                } else {
                    None
                }
            });

        if let Some((frequency, clarity)) = result {
            let note = self.frequency_to_note(frequency);
            let cents = Self::note_to_cents(note);
            TuningResult {
                frequency_hz: frequency,
                clarity,
                note,
                cents,
                detected: true,
            }
        } else {
            TuningResult {
                frequency_hz: 0.0,
                clarity: 0.0,
                note: 0.0,
                cents: 0.0,
                detected: false,
            }
        }
    }

    /// Feed one sample into the tuner.
    ///
    /// Returns a new [`TuningResult`] whenever enough samples have been
    /// collected for the next analysis window.
    pub fn feed_sample(&mut self, sample: f32) -> Option<TuningResult> {
        self.buffer.push(sample as f64);
        self.samples_since_report += 1;

        if self.buffer.len() >= self.window_size && self.samples_since_report >= self.hop_size {
            let result = self.analyze_window();
            let drain = self
                .hop_size
                .min(self.buffer.len() - self.window_size + self.hop_size);
            self.buffer.drain(..drain);
            self.samples_since_report = 0;
            Some(result)
        } else {
            None
        }
    }

    /// Feed a mono buffer and return the latest result.
    pub fn feed_mono(&mut self, mono: &[f32]) -> Option<TuningResult> {
        let mut latest = None;
        for &sample in mono {
            if let Some(result) = self.feed_sample(sample) {
                latest = Some(result);
            }
        }
        latest
    }
}

/// Format a MIDI note number as a note name with optional cents deviation.
pub fn format_note(note: f32) -> String {
    if !note.is_finite() || note <= 0.0 {
        return "--".to_string();
    }
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let rounded = note.round() as i32;
    let name = NAMES[((rounded % 12 + 12) % 12) as usize];
    let octave = (rounded / 12) - 1;
    format!("{name}{octave}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn sine(freq: f32, n: usize, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / SR).sin())
            .collect()
    }

    #[test]
    fn detects_a4() {
        let input = sine(440.0, 8192, 0.5);
        let mut tuner = Tuner::new(SR as f64);
        let result = tuner.feed_mono(&input).unwrap();
        assert!(result.detected, "pitch not detected");
        assert!(
            (result.frequency_hz - 440.0).abs() < 2.0,
            "detected {} Hz",
            result.frequency_hz
        );
        assert!(
            (result.note - 69.0).abs() < 0.1,
            "detected note {}",
            result.note
        );
    }

    #[test]
    fn detects_low_e() {
        let input = sine(82.41, 16384, 0.5);
        let mut tuner = Tuner::new(SR as f64);
        let result = tuner.feed_mono(&input).unwrap();
        assert!(result.detected, "pitch not detected");
        assert!(
            (result.frequency_hz - 82.41).abs() < 2.0,
            "detected {} Hz",
            result.frequency_hz
        );
    }

    #[test]
    fn silence_is_not_detected() {
        let input = vec![0.0_f32; 8192];
        let mut tuner = Tuner::new(SR as f64);
        let result = tuner.feed_mono(&input);
        assert!(result.is_none() || !result.unwrap().detected);
    }

    #[test]
    fn note_formatting() {
        assert_eq!(format_note(69.0), "A4");
        assert_eq!(format_note(60.0), "C4");
        assert_eq!(format_note(61.0), "C#4");
        assert_eq!(format_note(0.0), "--");
    }
}
