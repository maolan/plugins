#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PhasorSync {
    #[default]
    Free,

    Tempo,

    Song,

    Voice,
}

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

pub struct Phasor {
    sample_rate: f32,
    phase: f32,
    increment: f32,
    sync: PhasorSync,
    division: PhasorDivision,
    tempo_bpm: f32,

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

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f32 {
        match self.sync {
            PhasorSync::Voice if !self.gated => {
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

    pub fn set_phase_from_song_position(&mut self, beats: f32) {
        let div_mult = 2.0f32.powi(self.division as i32);
        self.phase = (beats * div_mult).fract();
    }

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
        p.set_rate_hz(1.0);

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

        for _ in 0..48000 {
            p.next();
        }

        assert!(p.phase() < 0.01 || p.phase() > 0.99);
    }

    #[test]
    fn test_phasor_voice_gate() {
        let mut p = Phasor::new(48000.0);
        p.set_sync(PhasorSync::Voice);
        p.set_rate_hz(1.0);
        p.set_gated(true);

        for _ in 0..100 {
            p.next();
        }
        let phase_gated = p.phase();
        assert!(phase_gated > 0.0);

        p.set_gated(false);
        for _ in 0..100 {
            p.next();
        }
        assert_eq!(p.phase(), phase_gated);
    }
}
