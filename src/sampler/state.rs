use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, Ordering};

pub use crate::common::state::{
    PluginState, SamplerGroupState, SamplerModRouteState, SamplerZoneState,
};
use crate::sampler::dsp::mod_matrix::{ModCurve, ModMatrix, ModSource, ModTarget};
use crate::sampler::dsp::zone::{
    CcCondition, CurveType, LoopDirection, LoopMode, SamplePlayMode, VariantMode,
};

#[derive(Debug, Clone)]
pub struct SampleGroup {
    pub name: String,
    pub poly_limit: usize,
    pub exclusive_group: u8,
    pub gain_db: f32,
    pub pan: f32,
    pub extra_sfz_opcodes: Vec<(String, String)>,
}

impl SampleGroup {
    pub fn new(name: String) -> Self {
        Self {
            name,
            poly_limit: 0,
            exclusive_group: 0,
            gain_db: 0.0,
            pan: 0.0,
            extra_sfz_opcodes: Vec::new(),
        }
    }

    pub fn to_state(&self) -> SamplerGroupState {
        SamplerGroupState {
            name: self.name.clone(),
            poly_limit: Some(self.poly_limit),
            exclusive_group: Some(self.exclusive_group),
            gain_db: Some(self.gain_db),
            pan: Some(self.pan),
            extra_sfz_opcodes: self.extra_sfz_opcodes.clone(),
        }
    }

    pub fn from_state(state: &SamplerGroupState) -> Self {
        Self {
            name: if state.name.is_empty() {
                String::from("New Group")
            } else {
                state.name.clone()
            },
            poly_limit: state.poly_limit.unwrap_or(0),
            exclusive_group: state.exclusive_group.unwrap_or(0),
            gain_db: state.gain_db.unwrap_or(0.0),
            pan: state.pan.unwrap_or(0.0),
            extra_sfz_opcodes: state.extra_sfz_opcodes.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SampleZone {
    pub name: String,
    pub files: Vec<PathBuf>,
    pub start_note: usize,
    pub end_note: usize,
    pub vel_low: u8,
    pub vel_high: u8,
    pub group: String,
    pub root_key: u8,
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
    pub off_by: u8,
    pub mod_matrix: ModMatrix,
    pub extra_sfz_opcodes: Vec<(String, String)>,
}

impl SampleZone {
    pub fn new_basic(
        name: String,
        files: Vec<PathBuf>,
        start_note: usize,
        end_note: usize,
        vel_low: u8,
        vel_high: u8,
        group: String,
    ) -> Self {
        Self {
            name,
            files,
            start_note,
            end_note,
            vel_low,
            vel_high,
            group,
            root_key: Self::default_root_key(start_note, end_note),
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
            off_by: 0,
            mod_matrix: ModMatrix::default(),
            extra_sfz_opcodes: Vec::new(),
        }
    }

    pub fn default_root_key(start_note: usize, end_note: usize) -> u8 {
        ((start_note + end_note) / 2).min(127) as u8
    }

    pub fn to_state(&self) -> SamplerZoneState {
        SamplerZoneState {
            name: self.name.clone(),
            files: self
                .files
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect(),
            start_note: self.start_note,
            end_note: self.end_note,
            vel_low: self.vel_low,
            vel_high: self.vel_high,
            group: self.group.clone(),
            root_key: Some(self.root_key),
            key_fade_low: Some(self.key_fade_low),
            key_fade_high: Some(self.key_fade_high),
            vel_fade_low: Some(self.vel_fade_low),
            vel_fade_high: Some(self.vel_fade_high),
            key_fade_in: self.key_fade_in,
            key_fade_out: self.key_fade_out,
            vel_fade_in: self.vel_fade_in,
            vel_fade_out: self.vel_fade_out,
            pitch_offset: Some(self.pitch_offset),
            key_tracking: Some(self.key_tracking),
            velocity_curve: Some(self.velocity_curve as u8),
            key_tracking_curve: Some(self.key_tracking_curve as u8),
            gain_db: Some(self.gain_db),
            pan: Some(self.pan),
            width: Some(self.width),
            position: Some(self.position),
            amp_keytrack_db: Some(self.amp_keytrack_db),
            reverse: Some(self.reverse),
            play_mode: Some(self.play_mode as u8),
            loop_mode: Some(self.loop_mode as u8),
            loop_direction: Some(self.loop_direction as u8),
            loop_start: Some(self.loop_start),
            loop_end: Some(self.loop_end),
            loop_count: Some(self.loop_count),
            loop_crossfade: Some(self.loop_crossfade),
            start_offset: Some(self.start_offset),
            offset_random: Some(self.offset_random),
            end_offset: Some(self.end_offset),
            delay: Some(self.delay),
            delay_random: Some(self.delay_random),
            pitch_bend_up: Some(self.pitch_bend_up),
            pitch_bend_down: Some(self.pitch_bend_down),
            variant_mode: Some(self.variant_mode as u8),
            channel_low: Some(self.channel_low),
            channel_high: Some(self.channel_high),
            pitch_bend_low: Some(self.pitch_bend_low),
            pitch_bend_high: Some(self.pitch_bend_high),
            cc_conditions: self
                .cc_conditions
                .iter()
                .map(|condition| (condition.cc, condition.low, condition.high))
                .collect(),
            random_low: Some(self.random_low),
            random_high: Some(self.random_high),
            seq_length: Some(self.seq_length),
            seq_position: Some(self.seq_position),
            off_by: Some(self.off_by),
            mod_routes: self
                .mod_matrix
                .routes
                .iter()
                .filter(|route| route.active)
                .map(|route| SamplerModRouteState {
                    source: route.source as u8,
                    source_cc: route.source_cc,
                    target: route.target as u8,
                    depth: route.depth,
                    active: route.active,
                    source_curve: route.source_curve.points.to_vec(),
                })
                .collect(),
            extra_sfz_opcodes: self.extra_sfz_opcodes.clone(),
        }
    }

    pub fn from_state(state: &SamplerZoneState) -> Self {
        let root_key = Self::default_root_key(state.start_note, state.end_note);
        let mut mod_matrix = ModMatrix::default();
        for (index, route) in state
            .mod_routes
            .iter()
            .take(mod_matrix.routes.len())
            .enumerate()
        {
            let mut curve = ModCurve::linear();
            for (point, value) in curve.points.iter_mut().zip(route.source_curve.iter()) {
                *point = value.clamp(0.0, 1.0);
            }
            mod_matrix.routes[index] = crate::sampler::dsp::mod_matrix::ModRoute {
                source: ModSource::from_u8(route.source),
                source_cc: route.source_cc.min(127),
                source_curve: curve,
                target: ModTarget::from_u8(route.target),
                depth: route.depth,
                active: route.active,
            };
        }
        Self {
            name: state.name.clone(),
            files: state.files.iter().map(PathBuf::from).collect(),
            start_note: state.start_note,
            end_note: state.end_note,
            vel_low: state.vel_low,
            vel_high: state.vel_high,
            group: if state.group.is_empty() {
                String::from("New Group")
            } else {
                state.group.clone()
            },
            root_key: state.root_key.unwrap_or(root_key),
            key_fade_low: state.key_fade_low.unwrap_or(0),
            key_fade_high: state.key_fade_high.unwrap_or(0),
            vel_fade_low: state.vel_fade_low.unwrap_or(0),
            vel_fade_high: state.vel_fade_high.unwrap_or(0),
            key_fade_in: state.key_fade_in,
            key_fade_out: state.key_fade_out,
            vel_fade_in: state.vel_fade_in,
            vel_fade_out: state.vel_fade_out,
            pitch_offset: state.pitch_offset.unwrap_or(0.0),
            key_tracking: state.key_tracking.unwrap_or(1.0),
            velocity_curve: state
                .velocity_curve
                .map(CurveType::from_u8)
                .unwrap_or(CurveType::Linear),
            key_tracking_curve: state
                .key_tracking_curve
                .map(CurveType::from_u8)
                .unwrap_or(CurveType::Linear),
            gain_db: state.gain_db.unwrap_or(0.0),
            pan: state.pan.unwrap_or(0.0),
            width: state.width.unwrap_or(1.0),
            position: state.position.unwrap_or(0.0),
            amp_keytrack_db: state.amp_keytrack_db.unwrap_or(0.0),
            reverse: state.reverse.unwrap_or(false),
            play_mode: state
                .play_mode
                .map(SamplePlayMode::from_u8)
                .unwrap_or(SamplePlayMode::Normal),
            loop_mode: state
                .loop_mode
                .map(LoopMode::from_u8)
                .unwrap_or(LoopMode::Off),
            loop_direction: state
                .loop_direction
                .map(LoopDirection::from_u8)
                .unwrap_or(LoopDirection::Forward),
            loop_start: state.loop_start.unwrap_or(0),
            loop_end: state.loop_end.unwrap_or(0),
            loop_count: state.loop_count.unwrap_or(0),
            loop_crossfade: state.loop_crossfade.unwrap_or(0),
            start_offset: state.start_offset.unwrap_or(0),
            offset_random: state.offset_random.unwrap_or(0),
            end_offset: state.end_offset.unwrap_or(0),
            delay: state.delay.unwrap_or(0.0),
            delay_random: state.delay_random.unwrap_or(0.0),
            pitch_bend_up: state.pitch_bend_up.unwrap_or(2.0),
            pitch_bend_down: state.pitch_bend_down.unwrap_or(2.0),
            variant_mode: state
                .variant_mode
                .map(VariantMode::from_u8)
                .unwrap_or(VariantMode::First),
            channel_low: state.channel_low.unwrap_or(1),
            channel_high: state.channel_high.unwrap_or(16),
            pitch_bend_low: state.pitch_bend_low.unwrap_or(-8192),
            pitch_bend_high: state.pitch_bend_high.unwrap_or(8192),
            cc_conditions: state
                .cc_conditions
                .iter()
                .map(|(cc, low, high)| CcCondition {
                    cc: *cc,
                    low: *low,
                    high: *high,
                })
                .collect(),
            random_low: state.random_low.unwrap_or(0.0),
            random_high: state.random_high.unwrap_or(1.0),
            seq_length: state.seq_length.unwrap_or(0),
            seq_position: state.seq_position.unwrap_or(0),
            off_by: state.off_by.unwrap_or(0),
            mod_matrix,
            extra_sfz_opcodes: state.extra_sfz_opcodes.clone(),
        }
    }
}

pub struct AtomicArc<T> {
    ptr: AtomicPtr<T>,
}

impl<T> std::fmt::Debug for AtomicArc<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AtomicArc").finish_non_exhaustive()
    }
}

impl<T> AtomicArc<T> {
    pub fn new(value: Arc<T>) -> Self {
        let ptr = Arc::into_raw(value) as *mut T;
        Self {
            ptr: AtomicPtr::new(ptr),
        }
    }

    pub fn load(&self) -> Arc<T> {
        let ptr = self.ptr.load(Ordering::Acquire);
        unsafe {
            Arc::increment_strong_count(ptr);
            Arc::from_raw(ptr)
        }
    }

    pub fn store(&self, value: Arc<T>) {
        let new_ptr = Arc::into_raw(value) as *mut T;
        let old_ptr = self.ptr.swap(new_ptr, Ordering::AcqRel);
        unsafe {
            drop(Arc::from_raw(old_ptr));
        }
    }
}

impl<T: Default> Default for AtomicArc<T> {
    fn default() -> Self {
        Self::new(Arc::new(T::default()))
    }
}

impl<T> Drop for AtomicArc<T> {
    fn drop(&mut self) {
        let ptr = self.ptr.load(Ordering::Acquire);
        unsafe {
            drop(Arc::from_raw(ptr));
        }
    }
}

unsafe impl<T: Send> Send for AtomicArc<T> {}
unsafe impl<T: Send + Sync> Sync for AtomicArc<T> {}
