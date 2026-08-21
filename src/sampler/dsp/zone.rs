use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::sampler::dsp::mod_matrix::ModMatrix;
use crate::sampler::dsp::sample::Sample;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplePlayMode {
    Normal = 0,

    OneShot = 1,

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopMode {
    Off = 0,

    DuringVoice = 1,

    WhileGated = 2,

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopDirection {
    Forward = 0,

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CurveType {
    #[default]
    Linear = 0,
    Exponential = 1,
    Logarithmic = 2,
    SCurve = 3,
}

impl CurveType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => CurveType::Exponential,
            2 => CurveType::Logarithmic,
            3 => CurveType::SCurve,
            _ => CurveType::Linear,
        }
    }
}

pub fn apply_curve(x: f32, curve: CurveType) -> f32 {
    let x = x.clamp(0.0, 1.0);
    match curve {
        CurveType::Linear => x,
        CurveType::Exponential => x * x,
        CurveType::Logarithmic => x.sqrt(),
        CurveType::SCurve => x * x * (3.0 - 2.0 * x),
    }
}

pub fn curve_bipolar(x: f32, curve: CurveType) -> f32 {
    let sign = x.signum();
    let abs = x.abs().clamp(0.0, 1.0);
    sign * apply_curve(abs, curve)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariantMode {
    First = 0,

    RoundRobin = 1,

    Random = 2,

    RandomNoRepeat = 3,

    RandomCycle = 4,

    Unison = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CcCondition {
    pub cc: u8,
    pub low: u8,
    pub high: u8,
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

#[derive(Debug)]
pub struct Zone {
    pub sample: Arc<Sample>,
    pub name: String,

    pub root_key: u8,

    pub key_low: u8,
    pub key_high: u8,

    pub vel_low: u8,
    pub vel_high: u8,

    pub key_fade_low: u8,
    pub key_fade_high: u8,
    pub vel_fade_low: u8,
    pub vel_fade_high: u8,
    pub key_fade_in: Option<(u8, u8)>,
    pub key_fade_out: Option<(u8, u8)>,
    pub vel_fade_in: Option<(u8, u8)>,
    pub vel_fade_out: Option<(u8, u8)>,

    pub pitch_offset: f32,

    pub key_tracking: f32,

    pub velocity_curve: CurveType,

    pub key_tracking_curve: CurveType,

    pub gain_db: f32,

    pub pan: f32,

    pub width: f32,

    pub position: f32,

    pub amp_keytrack_db: f32,

    pub reverse: bool,

    pub play_mode: SamplePlayMode,

    pub loop_mode: LoopMode,

    pub loop_direction: LoopDirection,

    pub loop_start: usize,

    pub loop_end: usize,

    pub loop_count: u32,

    pub loop_crossfade: usize,

    pub start_offset: usize,

    pub offset_random: usize,

    pub end_offset: usize,

    pub delay: f32,

    pub delay_random: f32,

    pub pitch_bend_up: f32,

    pub pitch_bend_down: f32,

    pub variant_mode: VariantMode,

    pub channel_low: u8,

    pub channel_high: u8,

    pub pitch_bend_low: i16,

    pub pitch_bend_high: i16,

    pub cc_conditions: Vec<CcCondition>,

    pub random_low: f32,

    pub random_high: f32,

    pub seq_length: u32,

    pub seq_position: u32,

    seq_counter: AtomicUsize,

    pub off_by: u8,

    pub variants: Vec<Arc<Sample>>,

    current_variant: AtomicUsize,

    last_variant: AtomicUsize,

    variant_pool: Mutex<Vec<usize>>,

    pool_position: AtomicUsize,

    pub mod_matrix: ModMatrix,

    pub extra_sfz_opcodes: Vec<(String, String)>,
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
            key_fade_in: self.key_fade_in,
            key_fade_out: self.key_fade_out,
            vel_fade_in: self.vel_fade_in,
            vel_fade_out: self.vel_fade_out,
            pitch_offset: self.pitch_offset,
            key_tracking: self.key_tracking,
            velocity_curve: self.velocity_curve,
            key_tracking_curve: self.key_tracking_curve,
            gain_db: self.gain_db,
            pan: self.pan,
            width: self.width,
            position: self.position,
            amp_keytrack_db: self.amp_keytrack_db,
            reverse: self.reverse,
            play_mode: self.play_mode,
            loop_mode: self.loop_mode,
            loop_direction: self.loop_direction,
            loop_start: self.loop_start,
            loop_end: self.loop_end,
            loop_count: self.loop_count,
            loop_crossfade: self.loop_crossfade,
            start_offset: self.start_offset,
            offset_random: self.offset_random,
            end_offset: self.end_offset,
            delay: self.delay,
            delay_random: self.delay_random,
            pitch_bend_up: self.pitch_bend_up,
            pitch_bend_down: self.pitch_bend_down,
            variant_mode: self.variant_mode,
            channel_low: self.channel_low,
            channel_high: self.channel_high,
            pitch_bend_low: self.pitch_bend_low,
            pitch_bend_high: self.pitch_bend_high,
            cc_conditions: self.cc_conditions.clone(),
            random_low: self.random_low,
            random_high: self.random_high,
            seq_length: self.seq_length,
            seq_position: self.seq_position,
            seq_counter: AtomicUsize::new(self.seq_counter.load(Ordering::Relaxed)),
            off_by: self.off_by,
            variants: self.variants.clone(),
            current_variant: AtomicUsize::new(self.current_variant.load(Ordering::Relaxed)),
            last_variant: AtomicUsize::new(self.last_variant.load(Ordering::Relaxed)),
            variant_pool: Mutex::new(self.variant_pool.lock().unwrap().clone()),
            pool_position: AtomicUsize::new(self.pool_position.load(Ordering::Relaxed)),
            mod_matrix: self.mod_matrix.clone(),
            extra_sfz_opcodes: self.extra_sfz_opcodes.clone(),
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
            key_fade_in: None,
            key_fade_out: None,
            vel_fade_in: None,
            vel_fade_out: None,
            pitch_offset: 0.0,
            key_tracking: 1.0,
            velocity_curve: CurveType::Linear,
            key_tracking_curve: CurveType::Linear,
            gain_db: 0.0,
            pan: 0.0,
            width: 1.0,
            position: 0.0,
            amp_keytrack_db: 0.0,
            reverse: false,
            play_mode: SamplePlayMode::Normal,
            loop_mode: LoopMode::Off,
            loop_direction: LoopDirection::Forward,
            loop_start: 0,
            loop_end: 0,
            loop_count: 0,
            loop_crossfade: 0,
            start_offset: 0,
            offset_random: 0,
            end_offset: 0,
            delay: 0.0,
            delay_random: 0.0,
            pitch_bend_up: 2.0,
            pitch_bend_down: 2.0,
            variant_mode: VariantMode::First,
            channel_low: 1,
            channel_high: 16,
            pitch_bend_low: -8192,
            pitch_bend_high: 8192,
            cc_conditions: Vec::new(),
            random_low: 0.0,
            random_high: 1.0,
            seq_length: 0,
            seq_position: 0,
            seq_counter: AtomicUsize::new(0),
            off_by: 0,
            variants: Vec::new(),
            current_variant: AtomicUsize::new(0),
            last_variant: AtomicUsize::new(0),
            variant_pool: Mutex::new(Vec::new()),
            pool_position: AtomicUsize::new(0),
            mod_matrix: ModMatrix::default(),
            extra_sfz_opcodes: Vec::new(),
        }
    }
}

impl Zone {
    pub fn new_round_robin(
        name: String,
        sample: Arc<Sample>,
        root_key: u8,
        key_range: (u8, u8),
        vel_range: (u8, u8),
        variants: Vec<Arc<Sample>>,
    ) -> Self {
        Self {
            name,
            sample,
            root_key,
            key_low: key_range.0,
            key_high: key_range.1,
            vel_low: vel_range.0,
            vel_high: vel_range.1,
            variant_mode: VariantMode::RoundRobin,
            variants,
            ..Default::default()
        }
    }

    /// Return a `Zone` with SoundFont 2 defaults.
    ///
    /// SoundFont uses full key/velocity ranges, 100 % key tracking, and a
    /// linear velocity curve by default. This is currently the same as
    /// `Zone::default()`, but the constructor makes the intent explicit when
    /// translating SF2 instrument zones.
    pub fn sf2_default() -> Self {
        Self {
            key_low: 0,
            key_high: 127,
            vel_low: 0,
            vel_high: 127,
            key_tracking: 1.0,
            velocity_curve: CurveType::Linear,
            pitch_bend_up: 2.0,
            pitch_bend_down: 2.0,
            ..Default::default()
        }
    }
}

impl Zone {
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
    pub fn effective_end_frame(&self, sample_frames: usize) -> usize {
        if self.end_offset == 0 {
            sample_frames
        } else {
            self.end_offset.saturating_add(1).min(sample_frames)
        }
    }

    pub fn contains(&self, note: u8, velocity: u8) -> bool {
        self.contains_with_context(note, velocity, 1, &[0; 128], 0)
    }

    pub fn contains_with_context(
        &self,
        note: u8,
        velocity: u8,
        channel: u8,
        cc_values: &[u8; 128],
        pitch_bend_raw: i16,
    ) -> bool {
        if note < self.key_low
            || note > self.key_high
            || velocity < self.vel_low
            || velocity > self.vel_high
        {
            return false;
        }
        let sfz_channel = channel.saturating_add(1).clamp(1, 16);
        if sfz_channel < self.channel_low || sfz_channel > self.channel_high {
            return false;
        }
        if pitch_bend_raw < self.pitch_bend_low || pitch_bend_raw > self.pitch_bend_high {
            return false;
        }
        for condition in &self.cc_conditions {
            let value = cc_values[condition.cc as usize % 128];
            if value < condition.low || value > condition.high {
                return false;
            }
        }
        if self.random_low > 0.0 || self.random_high < 1.0 {
            let value: f32 = rand::random();
            if value < self.random_low || value > self.random_high {
                return false;
            }
        }
        if self.seq_length > 0 && self.seq_position > 0 {
            let length = self.seq_length.max(1) as usize;
            let position = self.seq_position.saturating_sub(1) as usize % length;
            let current = self.seq_counter.fetch_add(1, Ordering::Relaxed) % length;
            if current != position {
                return false;
            }
        }
        true
    }

    pub fn compute_amplitude(&self, note: u8, velocity: u8) -> f32 {
        let mut amp = 1.0_f32;

        if let Some((low, high)) = self.key_fade_in {
            amp *= fade_in_factor(note, low, high);
        } else if self.key_fade_low > 0 && note < self.key_low + self.key_fade_low {
            let fade = (note.saturating_sub(self.key_low)) as f32 / self.key_fade_low as f32;
            amp *= fade;
        }

        if let Some((low, high)) = self.key_fade_out {
            amp *= fade_out_factor(note, low, high);
        } else if self.key_fade_high > 0 && note > self.key_high.saturating_sub(self.key_fade_high)
        {
            let fade = (self.key_high.saturating_sub(note)) as f32 / self.key_fade_high as f32;
            amp *= fade;
        }

        if let Some((low, high)) = self.vel_fade_in {
            amp *= fade_in_factor(velocity, low, high);
        } else if self.vel_fade_low > 0 && velocity < self.vel_low + self.vel_fade_low {
            let fade = (velocity.saturating_sub(self.vel_low)) as f32 / self.vel_fade_low as f32;
            amp *= fade;
        }

        if let Some((low, high)) = self.vel_fade_out {
            amp *= fade_out_factor(velocity, low, high);
        } else if self.vel_fade_high > 0
            && velocity > self.vel_high.saturating_sub(self.vel_fade_high)
        {
            let fade = (self.vel_high.saturating_sub(velocity)) as f32 / self.vel_fade_high as f32;
            amp *= fade;
        }

        let vel_norm = apply_curve(velocity as f32 / 127.0, self.velocity_curve);
        amp *= vel_norm;

        let key_gain_db = (note as f32 - self.root_key as f32) * self.amp_keytrack_db;
        amp *= 10.0_f32.powf((self.gain_db + key_gain_db) / 20.0);

        amp
    }

    pub fn compute_increment(&self, note: u8, project_sample_rate: f32) -> f64 {
        self.compute_increment_with_bend(note, project_sample_rate, 0.0)
    }

    pub fn compute_increment_with_bend(
        &self,
        note: u8,
        project_sample_rate: f32,
        pitch_bend: f32,
    ) -> f64 {
        let key_delta = ((note as f64 - self.root_key as f64) / 60.0).clamp(-1.0, 1.0) as f32;
        let shaped_key = curve_bipolar(key_delta, self.key_tracking_curve);
        let semitones = shaped_key as f64 * 60.0 * self.key_tracking as f64
            + (self.pitch_offset as f64 / 100.0)
            + pitch_bend as f64;
        let pitch_ratio = 2.0_f64.powf(semitones / 12.0);
        let sr_ratio = project_sample_rate as f64 / self.sample.sample_rate as f64;
        pitch_ratio * sr_ratio
    }

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

fn fade_in_factor(value: u8, low: u8, high: u8) -> f32 {
    if high <= low {
        return if value >= high { 1.0 } else { 0.0 };
    }
    if value <= low {
        0.0
    } else if value >= high {
        1.0
    } else {
        (value - low) as f32 / (high - low) as f32
    }
}

fn fade_out_factor(value: u8, low: u8, high: u8) -> f32 {
    if high <= low {
        return if value <= low { 1.0 } else { 0.0 };
    }
    if value <= low {
        1.0
    } else if value >= high {
        0.0
    } else {
        1.0 - (value - low) as f32 / (high - low) as f32
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

        let mut counts = [0usize; 3];
        for _ in 0..6 {
            let v = zone.select_variant();

            let idx = zone
                .variants
                .iter()
                .position(|x| Arc::ptr_eq(x, &v))
                .unwrap();
            counts[idx] += 1;
        }

        assert_eq!(counts[0], 2);
        assert_eq!(counts[1], 2);
        assert_eq!(counts[2], 2);
    }

    #[test]
    fn test_sf2_default() {
        let zone = Zone::sf2_default();
        assert_eq!(zone.key_low, 0);
        assert_eq!(zone.key_high, 127);
        assert_eq!(zone.vel_low, 0);
        assert_eq!(zone.vel_high, 127);
        assert!((zone.key_tracking - 1.0).abs() < f32::EPSILON);
        assert_eq!(zone.velocity_curve, CurveType::Linear);
    }

    #[test]
    fn test_key_tracking() {
        let mut zone = Zone {
            root_key: 60,
            sample: Arc::new(Sample::silent(48000.0)),
            ..Default::default()
        };

        zone.key_tracking = 1.0;
        let inc_72_full = zone.compute_increment(72, 48000.0);
        let inc_60_full = zone.compute_increment(60, 48000.0);
        assert!((inc_72_full / inc_60_full - 2.0).abs() < 0.01);

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

        let amp_40 = zone.compute_amplitude(40, 127);
        let amp_45 = zone.compute_amplitude(45, 127);
        assert!(amp_40 < amp_45);
    }

    #[test]
    fn test_explicit_fade_endpoints_shape_amplitude() {
        let zone = Zone {
            key_low: 40,
            key_high: 60,
            key_fade_in: Some((45, 50)),
            vel_fade_out: Some((100, 120)),
            ..Default::default()
        };

        let amp_before_key_fade = zone.compute_amplitude(44, 110);
        let amp_mid_key_fade = zone.compute_amplitude(47, 110);
        let amp_after_key_fade = zone.compute_amplitude(50, 110);
        let amp_after_vel_fade = zone.compute_amplitude(50, 120);
        assert_eq!(amp_before_key_fade, 0.0);
        assert!(amp_mid_key_fade > amp_before_key_fade);
        assert!(amp_after_key_fade > amp_mid_key_fade);
        assert_eq!(amp_after_vel_fade, 0.0);
    }

    #[test]
    fn test_velocity_curve_shapes_amplitude() {
        let linear_zone = Zone {
            sample: Arc::new(Sample::silent(48000.0)),
            ..Default::default()
        };
        let exp_zone = Zone {
            sample: Arc::new(Sample::silent(48000.0)),
            velocity_curve: CurveType::Exponential,
            ..Default::default()
        };

        let amp_linear = linear_zone.compute_amplitude(60, 64);
        let amp_exp = exp_zone.compute_amplitude(60, 64);
        assert!(
            amp_exp < amp_linear,
            "exponential curve should reduce mid-velocity amplitude"
        );
    }

    #[test]
    fn test_amp_keytrack_shapes_amplitude_by_note() {
        let zone = Zone {
            root_key: 60,
            amp_keytrack_db: 1.0,
            ..Default::default()
        };

        let amp_root = zone.compute_amplitude(60, 127);
        let amp_above = zone.compute_amplitude(72, 127);
        assert!(amp_above > amp_root);
    }

    #[test]
    fn test_key_tracking_curve_shapes_pitch() {
        let linear_zone = Zone {
            root_key: 60,
            sample: Arc::new(Sample::silent(48000.0)),
            ..Default::default()
        };
        let exp_zone = Zone {
            root_key: 60,
            sample: Arc::new(Sample::silent(48000.0)),
            key_tracking_curve: CurveType::Exponential,
            ..Default::default()
        };

        let inc_linear = linear_zone.compute_increment(72, 48000.0);
        let inc_exp = exp_zone.compute_increment(72, 48000.0);
        assert!(
            inc_exp < inc_linear,
            "exponential key curve should reduce upper-key pitch rise"
        );
    }
}
