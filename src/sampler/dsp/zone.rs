//! Zone data model — key/velocity mapping with sample reference.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::sampler::dsp::mod_matrix::ModMatrix;
use crate::sampler::dsp::sample::Sample;

/// How the zone responds to note-on/off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplePlayMode {
    /// Standard: note-on triggers sample, note-off enters release.
    Normal = 0,
    /// Sample plays to end regardless of key hold.
    OneShot = 1,
    /// Sample starts on note-off instead of note-on.
    OnRelease = 2,
}

impl SamplePlayMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => SamplePlayMode::OneShot,
            2 => SamplePlayMode::OnRelease,
            _ => SamplePlayMode::Normal,
        }
    }
}

/// Loop behaviour for the zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopMode {
    /// No looping.
    Off = 0,
    /// Loop persists for the voice lifetime.
    DuringVoice = 1,
    /// Loop only while key held; plays out after release.
    WhileGated = 2,
    /// Loop exactly N times, then continue.
    Count = 3,
}

impl LoopMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => LoopMode::DuringVoice,
            2 => LoopMode::WhileGated,
            3 => LoopMode::Count,
            _ => LoopMode::Off,
        }
    }
}

/// Loop direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopDirection {
    /// Standard forward loop.
    Forward = 0,
    /// Bidirectional / ping-pong loop.
    Alternate = 1,
}

impl LoopDirection {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => LoopDirection::Alternate,
            _ => LoopDirection::Forward,
        }
    }
}

/// How to select among multiple sample variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariantMode {
    /// Always use the first variant.
    First = 0,
    /// Cycle forward through variants.
    RoundRobin = 1,
    /// Uniform random selection.
    Random = 2,
    /// Random, never same twice in a row.
    RandomNoRepeat = 3,
    /// Random permutation that cycles through all variants before repeating.
    RandomCycle = 4,
    /// Play all variants simultaneously.
    Unison = 5,
}

impl VariantMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => VariantMode::RoundRobin,
            2 => VariantMode::Random,
            3 => VariantMode::RandomNoRepeat,
            _ => VariantMode::First,
        }
    }
}

/// A zone maps a sample to a key/velocity range with optional fade zones.
#[derive(Debug)]
pub struct Zone {
    pub sample: Arc<Sample>,
    pub name: String,
    /// Root MIDI note where the sample plays at its native pitch.
    pub root_key: u8,
    /// Key range (inclusive).
    pub key_low: u8,
    pub key_high: u8,
    /// Velocity range (inclusive, 0–127).
    pub vel_low: u8,
    pub vel_high: u8,
    /// Fade zones (number of keys/velocity steps to crossfade).
    pub key_fade_low: u8,
    pub key_fade_high: u8,
    pub vel_fade_low: u8,
    pub vel_fade_high: u8,
    /// Fine tuning in cents (-100..100).
    pub pitch_offset: f32,
    /// Key tracking amount (0.0 = no tracking, 1.0 = full tracking).
    pub key_tracking: f32,
    /// Gain in dB.
    pub gain_db: f32,
    /// Pan (-1..1).
    pub pan: f32,
    /// Playback direction.
    pub reverse: bool,
    /// Play mode (normal, one-shot, on-release).
    pub play_mode: SamplePlayMode,
    /// Loop mode.
    pub loop_mode: LoopMode,
    /// Loop direction.
    pub loop_direction: LoopDirection,
    /// Loop start sample index.
    pub loop_start: usize,
    /// Loop end sample index (exclusive).
    pub loop_end: usize,
    /// Number of times to loop (when LoopMode::Count).
    pub loop_count: u32,
    /// Sample start offset (skip this many samples at start).
    pub start_offset: usize,
    /// Pitch bend range up (semitones).
    pub pitch_bend_up: f32,
    /// Pitch bend range down (semitones).
    pub pitch_bend_down: f32,
    /// Variant selection mode.
    pub variant_mode: VariantMode,
    /// Sample variants (up to 16).
    pub variants: Vec<Arc<Sample>>,
    /// Current variant index (for round-robin).
    current_variant: AtomicUsize,
    /// Last variant index (for random no-repeat).
    last_variant: AtomicUsize,
    /// Shuffled pool for random-cycle mode.
    variant_pool: Mutex<Vec<usize>>,
    /// Position in the shuffled pool.
    pool_position: AtomicUsize,
    /// Zone-level modulation matrix.
    pub mod_matrix: ModMatrix,
}

impl Clone for Zone {
    fn clone(&self) -> Self {
        Self {
            sample: self.sample.clone(),
            name: self.name.clone(),
            root_key: self.root_key,
            key_low: self.key_low,
            key_high: self.key_high,
            vel_low: self.vel_low,
            vel_high: self.vel_high,
            key_fade_low: self.key_fade_low,
            key_fade_high: self.key_fade_high,
            vel_fade_low: self.vel_fade_low,
            vel_fade_high: self.vel_fade_high,
            pitch_offset: self.pitch_offset,
            key_tracking: self.key_tracking,
            gain_db: self.gain_db,
            pan: self.pan,
            reverse: self.reverse,
            play_mode: self.play_mode,
            loop_mode: self.loop_mode,
            loop_direction: self.loop_direction,
            loop_start: self.loop_start,
            loop_end: self.loop_end,
            loop_count: self.loop_count,
            start_offset: self.start_offset,
            pitch_bend_up: self.pitch_bend_up,
            pitch_bend_down: self.pitch_bend_down,
            variant_mode: self.variant_mode,
            variants: self.variants.clone(),
            current_variant: AtomicUsize::new(self.current_variant.load(Ordering::Relaxed)),
            last_variant: AtomicUsize::new(self.last_variant.load(Ordering::Relaxed)),
            variant_pool: Mutex::new(self.variant_pool.lock().unwrap().clone()),
            pool_position: AtomicUsize::new(self.pool_position.load(Ordering::Relaxed)),
            mod_matrix: self.mod_matrix.clone(),
        }
    }
}

impl Default for Zone {
    fn default() -> Self {
        Self {
            sample: Arc::new(Sample::silent(48000.0)),
            name: String::new(),
            root_key: 60,
            key_low: 0,
            key_high: 127,
            vel_low: 0,
            vel_high: 127,
            key_fade_low: 0,
            key_fade_high: 0,
            vel_fade_low: 0,
            vel_fade_high: 0,
            pitch_offset: 0.0,
            key_tracking: 1.0,
            gain_db: 0.0,
            pan: 0.0,
            reverse: false,
            play_mode: SamplePlayMode::Normal,
            loop_mode: LoopMode::Off,
            loop_direction: LoopDirection::Forward,
            loop_start: 0,
            loop_end: 0,
            loop_count: 0,
            start_offset: 0,
            pitch_bend_up: 2.0,
            pitch_bend_down: 2.0,
            variant_mode: VariantMode::First,
            variants: Vec::new(),
            current_variant: AtomicUsize::new(0),
            last_variant: AtomicUsize::new(0),
            variant_pool: Mutex::new(Vec::new()),
            pool_position: AtomicUsize::new(0),
            mod_matrix: ModMatrix::default(),
        }
    }
}

impl Zone {
    /// Select a variant based on the zone's variant mode.
    pub fn select_variant(&self) -> Arc<Sample> {
        if self.variants.is_empty() {
            return self.sample.clone();
        }
        let count = self.variants.len();
        let idx = match self.variant_mode {
            VariantMode::First => 0,
            VariantMode::RoundRobin => {
                let idx = self.current_variant.load(Ordering::Relaxed) % count;
                self.current_variant
                    .store((idx + 1) % count, Ordering::Relaxed);
                idx
            }
            VariantMode::Random => (rand::random::<u32>() as usize) % count,
            VariantMode::RandomNoRepeat => {
                if count <= 1 {
                    0
                } else {
                    let mut idx;
                    let last = self.last_variant.load(Ordering::Relaxed);
                    loop {
                        idx = (rand::random::<u32>() as usize) % count;
                        if idx != last {
                            break;
                        }
                    }
                    self.last_variant.store(idx, Ordering::Relaxed);
                    idx
                }
            }
            VariantMode::RandomCycle => {
                let mut pos = self.pool_position.load(Ordering::Relaxed);
                let mut pool = self.variant_pool.lock().unwrap();
                if pos >= count || pool.len() != count {
                    // Reshuffle pool.
                    *pool = (0..count).collect();
                    for i in (1..pool.len()).rev() {
                        let j = rand::random_range(0..=i);
                        pool.swap(i, j);
                    }
                    pos = 0;
                }
                let idx = pool[pos];
                pos += 1;
                drop(pool);
                self.pool_position.store(pos, Ordering::Relaxed);
                idx
            }
            VariantMode::Unison => 0,
        };
        self.variants[idx].clone()
    }
}

impl Zone {
    /// Check if a note/velocity falls within this zone's mapping.
    pub fn contains(&self, note: u8, velocity: u8) -> bool {
        note >= self.key_low
            && note <= self.key_high
            && velocity >= self.vel_low
            && velocity <= self.vel_high
    }

    /// Compute the amplitude scaling for a note/velocity hit, including fade zones.
    pub fn compute_amplitude(&self, note: u8, velocity: u8) -> f32 {
        let mut amp = 1.0_f32;

        // Key fade low.
        if self.key_fade_low > 0 && note < self.key_low + self.key_fade_low {
            let fade = (note.saturating_sub(self.key_low)) as f32 / self.key_fade_low as f32;
            amp *= fade;
        }
        // Key fade high.
        if self.key_fade_high > 0 && note > self.key_high.saturating_sub(self.key_fade_high) {
            let fade = (self.key_high.saturating_sub(note)) as f32 / self.key_fade_high as f32;
            amp *= fade;
        }
        // Velocity fade low.
        if self.vel_fade_low > 0 && velocity < self.vel_low + self.vel_fade_low {
            let fade = (velocity.saturating_sub(self.vel_low)) as f32 / self.vel_fade_low as f32;
            amp *= fade;
        }
        // Velocity fade high.
        if self.vel_fade_high > 0 && velocity > self.vel_high.saturating_sub(self.vel_fade_high) {
            let fade = (self.vel_high.saturating_sub(velocity)) as f32 / self.vel_fade_high as f32;
            amp *= fade;
        }

        // Velocity curve (linear for now).
        amp *= velocity as f32 / 127.0;

        // Gain in dB to linear.
        amp *= 10.0_f32.powf(self.gain_db / 20.0);

        amp
    }

    /// Compute the playback increment factor for a given MIDI note.
    /// Accounts for root key, pitch offset, and project/sample rate ratio.
    pub fn compute_increment(&self, note: u8, project_sample_rate: f32) -> f64 {
        self.compute_increment_with_bend(note, project_sample_rate, 0.0)
    }

    /// Compute the playback increment with additional pitch bend in semitones.
    pub fn compute_increment_with_bend(
        &self,
        note: u8,
        project_sample_rate: f32,
        pitch_bend: f32,
    ) -> f64 {
        let semitones = (note as f64 - self.root_key as f64) * self.key_tracking as f64
            + (self.pitch_offset as f64 / 100.0)
            + pitch_bend as f64;
        let pitch_ratio = 2.0_f64.powf(semitones / 12.0);
        let sr_ratio = project_sample_rate as f64 / self.sample.sample_rate as f64;
        pitch_ratio * sr_ratio
    }

    /// Compute the playback increment using a custom tuning table.
    pub fn compute_increment_with_tuning(
        &self,
        note: u8,
        project_sample_rate: f32,
        pitch_bend: f32,
        tuning: &crate::common::tuning::Tuning,
    ) -> f64 {
        let base_freq = tuning.note_to_freq(self.root_key) as f64;
        let note_freq = tuning.note_to_freq(note) as f64;
        let semitones = 12.0 * (note_freq / base_freq).log2() * self.key_tracking as f64
            + (self.pitch_offset as f64 / 100.0)
            + pitch_bend as f64;
        let pitch_ratio = 2.0_f64.powf(semitones / 12.0);
        let sr_ratio = project_sample_rate as f64 / self.sample.sample_rate as f64;
        pitch_ratio * sr_ratio
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zone_contains() {
        let zone = Zone {
            key_low: 40,
            key_high: 60,
            vel_low: 20,
            vel_high: 100,
            ..Default::default()
        };

        assert!(zone.contains(50, 50));
        assert!(!zone.contains(30, 50));
        assert!(!zone.contains(50, 10));
    }

    #[test]
    fn test_random_cycle_variant() {
        let zone = Zone {
            variant_mode: VariantMode::RandomCycle,
            variants: vec![
                Arc::new(Sample::silent(48000.0)),
                Arc::new(Sample::silent(48000.0)),
                Arc::new(Sample::silent(48000.0)),
            ],
            ..Default::default()
        };

        // Collect 6 selections — should cover all 3 variants twice (2 full cycles).
        let mut counts = [0usize; 3];
        for _ in 0..6 {
            let v = zone.select_variant();
            // Find which variant index this is by pointer comparison.
            let idx = zone
                .variants
                .iter()
                .position(|x| Arc::ptr_eq(x, &v))
                .unwrap();
            counts[idx] += 1;
        }
        // Each variant should appear exactly twice.
        assert_eq!(counts[0], 2);
        assert_eq!(counts[1], 2);
        assert_eq!(counts[2], 2);
    }

    #[test]
    fn test_key_tracking() {
        let mut zone = Zone {
            root_key: 60,
            sample: Arc::new(Sample::silent(48000.0)),
            ..Default::default()
        };

        // Full tracking: note 72 (one octave up) should be 2× speed.
        zone.key_tracking = 1.0;
        let inc_72_full = zone.compute_increment(72, 48000.0);
        let inc_60_full = zone.compute_increment(60, 48000.0);
        assert!((inc_72_full / inc_60_full - 2.0).abs() < 0.01);

        // No tracking: all notes play at same speed.
        zone.key_tracking = 0.0;
        let inc_72_none = zone.compute_increment(72, 48000.0);
        let inc_60_none = zone.compute_increment(60, 48000.0);
        assert!((inc_72_none - inc_60_none).abs() < 0.0001);
    }

    #[test]
    fn test_start_offset() {
        let zone = Zone {
            sample: Arc::new(Sample::silent(48000.0)),
            start_offset: 100,
            ..Default::default()
        };
        // start_offset field exists and defaults work.
        assert_eq!(zone.start_offset, 100);
    }

    #[test]
    fn test_key_fade() {
        let zone = Zone {
            key_low: 40,
            key_high: 60,
            key_fade_low: 5,
            ..Default::default()
        };

        // At the edge of fade zone, amplitude should be low.
        let amp_40 = zone.compute_amplitude(40, 127);
        let amp_45 = zone.compute_amplitude(45, 127);
        assert!(amp_40 < amp_45);
    }
}
