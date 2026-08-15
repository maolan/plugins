//! Pitch and tuning helpers shared across plugins.

/// Convert a MIDI note number to frequency in Hz using A4 = 440 Hz.
pub fn midi_note_to_frequency(note: u8) -> f32 {
    440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0)
}

/// Convert a pitch-bend value in semitones to a frequency ratio.
pub fn bend_semitones_to_ratio(semitones: f32) -> f32 {
    2.0f32.powf(semitones / 12.0)
}

/// Apply a pitch-bend offset in semitones to a base frequency.
pub fn apply_pitch_bend(base_hz: f32, bend_semitones: f32) -> f32 {
    base_hz * bend_semitones_to_ratio(bend_semitones)
}

/// Convert a cent offset to a frequency ratio.
pub fn cents_to_ratio(cents: f32) -> f32 {
    2.0f32.powf(cents / 1200.0)
}

/// Convert a frequency ratio to cents.
pub fn ratio_to_cents(ratio: f32) -> f32 {
    1200.0 * ratio.log2()
}

/// Convert a semitone offset to a frequency ratio.
pub fn semitones_to_ratio(semitones: f32) -> f32 {
    2.0f32.powf(semitones / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_a4_is_440() {
        assert!((midi_note_to_frequency(69) - 440.0).abs() < 0.001);
    }

    #[test]
    fn midi_a3_is_220() {
        assert!((midi_note_to_frequency(57) - 220.0).abs() < 0.001);
    }

    #[test]
    fn octave_bend_doubles_frequency() {
        assert!((apply_pitch_bend(440.0, 12.0) - 880.0).abs() < 0.001);
    }

    #[test]
    fn cents_near_unity() {
        assert!((cents_to_ratio(100.0) - 1.05946).abs() < 0.0001);
    }
}
