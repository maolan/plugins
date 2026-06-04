//! Phasor — transport-synced ramp generator for modulation.
//!
//! Produces a 0..1 ramp that can be synced to musical divisions,
//! song position, or voice position.

/// How the phasor is synchronized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PhasorSync {
    /// Free-running at a given Hz rate.
    #[default]
    Free,
    /// Synced to a musical division of the transport tempo.
    Tempo,
    /// Synced to song position (looping 0..1 every N bars).
    Song,
    /// Synced to voice position (0 when note-on, advances while gated).
    Voice,
}

/// Musical divisions for tempo sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PhasorDivision {
    Whole = 4,
    Half = 2,
    #[default]
    Quarter = 1,
    Eighth = 0,
    Sixteenth = -1,
    ThirtySecond = -2,
}

/// A phasor produces a 0..1 ramp for modulation.
pub struct Phasor {
    sample_rate: f32,
    phase: f32,
    increment: f32,
    sync: PhasorSync,
    division: PhasorDivision,
    tempo_bpm: f32,
    /// Whether the phasor is gated (for voice sync).
    gated: bool,
}

impl Phasor {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            phase: 0.0,
            increment: 0.0,
            sync: PhasorSync::Free,
            division: PhasorDivision::Quarter,
            tempo_bpm: 120.0,
            gated: false,
        }
    }

    pub fn set_sync(&mut self, sync: PhasorSync) {
        self.sync = sync;
        self.recalc_increment();
    }

    pub fn set_rate_hz(&mut self, hz: f32) {
        self.increment = hz / self.sample_rate;
    }

    pub fn set_division(&mut self, div: PhasorDivision) {
        self.division = div;
        self.recalc_increment();
    }

    pub fn set_tempo(&mut self, bpm: f32) {
        self.tempo_bpm = bpm.max(1.0);
        self.recalc_increment();
    }

    fn recalc_increment(&mut self) {
        match self.sync {
            PhasorSync::Free => {}
            PhasorSync::Tempo => {
                let beats_per_sec = self.tempo_bpm / 60.0;
                let div_mult = 2.0f32.powi(self.division as i32);
                let hz = beats_per_sec * div_mult;
                self.increment = hz / self.sample_rate;
            }
            PhasorSync::Song | PhasorSync::Voice => {
                // These require external transport/voice position input.
                // For now, fall back to tempo sync.
                let beats_per_sec = self.tempo_bpm / 60.0;
                self.increment = beats_per_sec / self.sample_rate;
            }
        }
    }

    pub fn set_gated(&mut self, gated: bool) {
        self.gated = gated;
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Advance the phasor by one sample and return the current 0..1 value.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f32 {
        match self.sync {
            PhasorSync::Voice if !self.gated => {
                // Hold phase when not gated in voice sync mode.
                return self.phase;
            }
            _ => {}
        }
        let out = self.phase;
        self.phase += self.increment;
        while self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }

    /// Set phase directly from external transport (for Song sync).
    pub fn set_phase_from_song_position(&mut self, beats: f32) {
        let div_mult = 2.0f32.powi(self.division as i32);
        self.phase = (beats * div_mult).fract();
    }

    /// Get current phase (0..1).
    pub fn phase(&self) -> f32 {
        self.phase
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phasor_free() {
        let mut p = Phasor::new(48000.0);
        p.set_rate_hz(1.0); // 1 Hz
        // After 48000 samples, should have completed one cycle.
        for _ in 0..48000 {
            p.next();
        }
        assert!(p.phase() < 0.1);
    }

    #[test]
    fn test_phasor_tempo() {
        let mut p = Phasor::new(48000.0);
        p.set_sync(PhasorSync::Tempo);
        p.set_tempo(120.0);
        p.set_division(PhasorDivision::Quarter);
        // 120 BPM = 2 beats/sec = 2 Hz for quarter notes.
        // After 48000 samples (1 sec), should have completed 2 cycles.
        for _ in 0..48000 {
            p.next();
        }
        // Allow some floating point drift.
        assert!(p.phase() < 0.01 || p.phase() > 0.99);
    }

    #[test]
    fn test_phasor_voice_gate() {
        let mut p = Phasor::new(48000.0);
        p.set_sync(PhasorSync::Voice);
        p.set_rate_hz(1.0);
        p.set_gated(true);

        // Advance 100 samples while gated.
        for _ in 0..100 {
            p.next();
        }
        let phase_gated = p.phase();
        assert!(phase_gated > 0.0);

        // Stop gating — phase should freeze.
        p.set_gated(false);
        for _ in 0..100 {
            p.next();
        }
        assert_eq!(p.phase(), phase_gated);
    }
}
