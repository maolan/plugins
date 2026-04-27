use crate::dsp::filters::{Biquad, high_shelf, low_shelf, peaking};

#[derive(Debug, Clone)]
pub struct ToneStack {
    sample_rate: f32,
    bass: f32,
    middle: f32,
    treble: f32,
    bass_filter: Biquad,
    mid_filter: Biquad,
    treble_filter: Biquad,
}

impl Default for ToneStack {
    fn default() -> Self {
        let mut stack = Self {
            sample_rate: 48_000.0,
            bass: 5.0,
            middle: 5.0,
            treble: 5.0,
            bass_filter: Biquad::default(),
            mid_filter: Biquad::default(),
            treble_filter: Biquad::default(),
        };
        stack.refresh();
        stack
    }
}

impl ToneStack {
    pub fn reset(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.refresh();
    }

    pub fn set_bass(&mut self, value: f32) {
        self.bass = value;
        self.refresh();
    }

    pub fn set_middle(&mut self, value: f32) {
        self.middle = value;
        self.refresh();
    }

    pub fn set_treble(&mut self, value: f32) {
        self.treble = value;
        self.refresh();
    }

    fn refresh(&mut self) {
        let bass_gain_db = 4.0 * (self.bass - 5.0);
        self.bass_filter
            .set_coeffs(low_shelf(self.sample_rate, 150.0, 0.707, bass_gain_db));

        let mid_gain_db = 3.0 * (self.middle - 5.0);
        let mid_q = if mid_gain_db < 0.0 { 1.5 } else { 0.7 };
        self.mid_filter
            .set_coeffs(peaking(self.sample_rate, 425.0, mid_q, mid_gain_db));

        let treble_gain_db = 2.0 * (self.treble - 5.0);
        self.treble_filter
            .set_coeffs(high_shelf(self.sample_rate, 1800.0, 0.707, treble_gain_db));
    }

    pub fn process_block(&mut self, block: &mut [f32]) {
        self.bass_filter.process_block(block);
        self.mid_filter.process_block(block);
        self.treble_filter.process_block(block);
    }
}
