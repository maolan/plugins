use crate::common::{
    bus,
    spectrum::{db_to_y, display_range_bounds, freq_to_x, x_to_freq, y_to_db},
    ui::{SmallKnob, VerticalSlider, small_knob, vertical_slider, vertical_ticks, vu_meter},
};
use crate::eq::dsp::{self, Biquad};
use crate::eq::params::{PARAMS, ParamId, ParamIdExt};
use crate::eq::plugin::{SPECTRUM_BINS, SharedState};

use maolan_baseview::iced::{
    Alignment, Color, Element, Event, Length, Point, Rectangle, Renderer, Task, Theme,
    alignment::{Horizontal, Vertical},
    core::keyboard,
    mouse,
    widget::{
        canvas,
        canvas::{Action as CanvasAction, Frame, Geometry, Path, Program, Text},
        column, container, row, text,
    },
};
use maolan_widgets::arch_slider::arch_slider;
use std::{
    collections::HashSet,
    ffi::CStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub const EDITOR_WIDTH: u32 = 1100;
pub const EDITOR_HEIGHT: u32 = 700;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandType {
    LowCut,
    Bell,
    HighCut,
    LowShelf,
    HighShelf,
    Notch,
    BandPass,
    TiltShelf,
}

impl std::fmt::Display for BandType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BandType::LowCut => write!(f, "Low Cut"),
            BandType::Bell => write!(f, "Bell"),
            BandType::HighCut => write!(f, "High Cut"),
            BandType::LowShelf => write!(f, "Low Shelf"),
            BandType::HighShelf => write!(f, "High Shelf"),
            BandType::Notch => write!(f, "Notch"),
            BandType::BandPass => write!(f, "Band Pass"),
            BandType::TiltShelf => write!(f, "Tilt Shelf"),
        }
    }
}

impl From<u8> for BandType {
    fn from(v: u8) -> Self {
        match v {
            dsp::SHAPE_HIGH_CUT => BandType::HighCut,
            dsp::SHAPE_LOW_CUT => BandType::LowCut,
            dsp::SHAPE_LOW_SHELF => BandType::LowShelf,
            dsp::SHAPE_HIGH_SHELF => BandType::HighShelf,
            dsp::SHAPE_NOTCH => BandType::Notch,
            dsp::SHAPE_BAND_PASS => BandType::BandPass,
            dsp::SHAPE_TILT_SHELF => BandType::TiltShelf,
            _ => BandType::Bell,
        }
    }
}

impl From<BandType> for u8 {
    fn from(t: BandType) -> Self {
        match t {
            BandType::LowCut => dsp::SHAPE_LOW_CUT,
            BandType::Bell => dsp::SHAPE_BELL,
            BandType::HighCut => dsp::SHAPE_HIGH_CUT,
            BandType::LowShelf => dsp::SHAPE_LOW_SHELF,
            BandType::HighShelf => dsp::SHAPE_HIGH_SHELF,
            BandType::Notch => dsp::SHAPE_NOTCH,
            BandType::BandPass => dsp::SHAPE_BAND_PASS,
            BandType::TiltShelf => dsp::SHAPE_TILT_SHELF,
        }
    }
}

impl BandType {
    pub const ALL: [BandType; 8] = [
        BandType::Bell,
        BandType::LowShelf,
        BandType::HighShelf,
        BandType::LowCut,
        BandType::HighCut,
        BandType::Notch,
        BandType::BandPass,
        BandType::TiltShelf,
    ];

    pub fn dyn_capable(self) -> bool {
        dsp::dyn_capable(u8::from(self))
    }

    pub fn supports_dynamic_target(self) -> bool {
        matches!(self, BandType::Bell)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Stereo,
    Left,
    Right,
    Mid,
    Side,
}

impl std::fmt::Display for Placement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Placement::Stereo => write!(f, "Stereo"),
            Placement::Left => write!(f, "Left"),
            Placement::Right => write!(f, "Right"),
            Placement::Mid => write!(f, "Mid"),
            Placement::Side => write!(f, "Side"),
        }
    }
}

impl From<u8> for Placement {
    fn from(v: u8) -> Self {
        match v {
            dsp::PLACEMENT_LEFT => Placement::Left,
            dsp::PLACEMENT_RIGHT => Placement::Right,
            dsp::PLACEMENT_MID => Placement::Mid,
            dsp::PLACEMENT_SIDE => Placement::Side,
            _ => Placement::Stereo,
        }
    }
}

impl From<Placement> for u8 {
    fn from(p: Placement) -> Self {
        match p {
            Placement::Stereo => dsp::PLACEMENT_STEREO,
            Placement::Left => dsp::PLACEMENT_LEFT,
            Placement::Right => dsp::PLACEMENT_RIGHT,
            Placement::Mid => dsp::PLACEMENT_MID,
            Placement::Side => dsp::PLACEMENT_SIDE,
        }
    }
}

impl Placement {
    pub const ALL: [Placement; 3] = [Placement::Stereo, Placement::Mid, Placement::Side];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynSource {
    Internal,
    External,
}

impl std::fmt::Display for DynSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DynSource::Internal => write!(f, "Internal"),
            DynSource::External => write!(f, "External"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slope {
    Db12,
    Db24,
    Db48,
    Db96,
    Brickwall,
}

impl std::fmt::Display for Slope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Slope::Db12 => write!(f, "12 dB/oct"),
            Slope::Db24 => write!(f, "24 dB/oct"),
            Slope::Db48 => write!(f, "48 dB/oct"),
            Slope::Db96 => write!(f, "96 dB/oct"),
            Slope::Brickwall => write!(f, "Brickwall"),
        }
    }
}

impl From<u8> for Slope {
    fn from(v: u8) -> Self {
        match v {
            1 => Slope::Db24,
            2 => Slope::Db48,
            3 => Slope::Db96,
            dsp::SLOPE_BRICKWALL => Slope::Brickwall,
            _ => Slope::Db12,
        }
    }
}

impl From<Slope> for u8 {
    fn from(s: Slope) -> Self {
        match s {
            Slope::Db12 => 0,
            Slope::Db24 => 1,
            Slope::Db48 => 2,
            Slope::Db96 => 3,
            Slope::Brickwall => dsp::SLOPE_BRICKWALL,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelMode {
    Mono,
    Stereo,
}

impl std::fmt::Display for ChannelMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChannelMode::Mono => write!(f, "Mono"),
            ChannelMode::Stereo => write!(f, "Stereo"),
        }
    }
}

impl From<u32> for ChannelMode {
    fn from(v: u32) -> Self {
        if v >= 2 {
            ChannelMode::Stereo
        } else {
            ChannelMode::Mono
        }
    }
}

impl From<ChannelMode> for u32 {
    fn from(mode: ChannelMode) -> Self {
        match mode {
            ChannelMode::Mono => 1,
            ChannelMode::Stereo => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    SetParam(ParamId, f32),
    SetParamImmediate(ParamId, f32),
    SetBandFreqGain(usize, f32, f32),
    StartBandDynamicTarget(usize, f32, f32),
    SetBandDynamicTarget(usize, f32),
    EndBandDrag(usize),
    SetBoolParam(ParamId, bool),
    ReleaseParam(ParamId),
    CreateBand(f32, f32),
    SelectBand(usize),
    ToggleBandSelection(usize),
    CycleBandShape(usize),
    ToggleListen(usize),
    DeselectBand,
    DeleteBand,
    SetChannels(ChannelMode),
    TogglePreSpectrum,
    TogglePostSpectrum,
    ToggleFreeze,
    SetDisplayRange(f32),
    SetTilt(f32),
    ToggleSketchMode,
    BeginSketch,
    ApplySketch(Vec<(f32, f32)>),
    CopyBands,
    PasteBands,
    SetMatchPeer(u32),
    ApplyEqMatch,
    ToggleInstanceList,
    SelectGhostPeer(u32),
    NoOp,
    UiTick,
}

struct State {
    shared: Arc<SharedState<ParamId>>,
    selected_band: Option<usize>,
    selection: HashSet<usize>,
    active_gestures: HashSet<ParamId>,

    bus_peers: Vec<bus::PluginSharedData>,

    collision_scores: [f32; 32],

    last_registry_version: u64,

    show_pre_spectrum: bool,
    show_post_spectrum: bool,
    spectrum_frozen: bool,
    tilt_db_per_oct: f32,
    display_range_db: f32,
    pre_spectrum_db: [[f32; SPECTRUM_BINS]; 2],
    post_spectrum_db: [[f32; SPECTRUM_BINS]; 2],
    band_gr_db: [f32; 32],
    sketch_mode: bool,
    last_sketch: Vec<(f32, f32)>,
    eq_peers: Vec<bus::PluginSharedData>,
    match_peer: Option<u32>,
    show_instances: bool,
    ghost_peer: Option<u32>,
    ghost_bands: Vec<bus::EqBand>,
    peer_band_counts: Vec<(u32, usize)>,
}

impl Drop for State {
    fn drop(&mut self) {
        bus::remove_needs(bus::NEED_FFT);
    }
}

fn init(shared: Arc<SharedState<ParamId>>) -> (State, Task<Message>) {
    bus::add_needs(bus::NEED_FFT);
    (
        State {
            shared,
            selected_band: None,
            selection: HashSet::new(),
            active_gestures: HashSet::new(),
            bus_peers: Vec::new(),
            collision_scores: [0.0; 32],
            last_registry_version: 0,
            show_pre_spectrum: false,
            show_post_spectrum: true,
            spectrum_frozen: false,
            tilt_db_per_oct: 0.0,
            display_range_db: 12.0,
            pre_spectrum_db: [[-90.0; SPECTRUM_BINS]; 2],
            post_spectrum_db: [[-90.0; SPECTRUM_BINS]; 2],
            band_gr_db: [0.0; 32],
            sketch_mode: false,
            last_sketch: Vec::new(),
            eq_peers: Vec::new(),
            match_peer: None,
            show_instances: false,
            ghost_peer: None,
            ghost_bands: Vec::new(),
            peer_band_counts: Vec::new(),
        },
        next_ui_tick_task(),
    )
}

fn next_ui_tick_task() -> Task<Message> {
    Task::perform(
        async move {
            thread::sleep(Duration::from_millis(33));
        },
        |_| Message::UiTick,
    )
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::SetParam(id, value) => {
            if state.active_gestures.insert(id) {
                state.shared.mark_gesture_begin_pending(id);
            }
            state.shared.set_param_outbound_only(id, value as f64);
            if let Some(band) = gain_param_band(id)
                && state.shared.params.get_bool(ParamId::para_dyn(band))
                && BandType::from(state.shared.params.get(ParamId::para_type(band)) as u8)
                    .supports_dynamic_target()
            {
                let range_id = ParamId::para_dyn_range(band);
                if state.active_gestures.insert(range_id) {
                    state.shared.mark_gesture_begin_pending(range_id);
                }
                state
                    .shared
                    .set_param_outbound_only(range_id, (-value.clamp(-24.0, 24.0)) as f64);
            }
        }
        Message::SetParamImmediate(id, value) => {
            state.shared.set_param_outbound_only(id, value as f64);
            state.shared.mark_gesture_begin_pending(id);
            state.shared.mark_gesture_end_pending(id);
        }
        Message::SetBandFreqGain(index, freq, gain) => {
            let freq = freq.clamp(20.0, 20_000.0);
            let shape = state.shared.params.get(ParamId::para_type(index)) as u8;
            let dynamic_target_mode = state.shared.params.get_bool(ParamId::para_dyn(index))
                && BandType::from(shape).supports_dynamic_target();
            let value = gain.clamp(-24.0, 24.0);
            let old_gain_for_shift = state.shared.params.get(ParamId::para_gain(index)) as f32;
            if state.selection.len() > 1 && state.selection.contains(&index) {
                let old_freq = state.shared.params.get(ParamId::para_freq(index)) as f32;
                let log_shift = (freq / old_freq.clamp(20.0, 20_000.0)).ln();
                let gain_shift = if dynamic_target_mode {
                    0.0
                } else {
                    value - old_gain_for_shift
                };
                for &other in &state.selection {
                    if other == index {
                        continue;
                    }
                    let ofid = ParamId::para_freq(other);
                    let ogid = ParamId::para_gain(other);
                    let of = state.shared.params.get(ofid) as f32;
                    let og = state.shared.params.get(ogid) as f32;
                    let nf = (of * log_shift.exp()).clamp(20.0, 20_000.0);
                    let ng = (og + gain_shift).clamp(-24.0, 24.0);
                    if state.active_gestures.insert(ofid) {
                        state.shared.mark_gesture_begin_pending(ofid);
                    }
                    state.shared.set_param_outbound_only(ofid, nf as f64);
                    if !dynamic_target_mode {
                        if state.active_gestures.insert(ogid) {
                            state.shared.mark_gesture_begin_pending(ogid);
                        }
                        state.shared.set_param_outbound_only(ogid, ng as f64);
                    }
                }
            }
            let fid = ParamId::para_freq(index);
            let gid = ParamId::para_gain(index);
            if state.active_gestures.insert(fid) {
                state.shared.mark_gesture_begin_pending(fid);
            }
            state.shared.set_param_outbound_only(fid, freq as f64);
            if dynamic_target_mode {
                let threshold_id = ParamId::para_dyn_threshold(index);
                if state.active_gestures.insert(threshold_id) {
                    state.shared.mark_gesture_begin_pending(threshold_id);
                }
                state
                    .shared
                    .set_param_outbound_only(threshold_id, value as f64);
            } else {
                if state.active_gestures.insert(gid) {
                    state.shared.mark_gesture_begin_pending(gid);
                }
                state.shared.set_param_outbound_only(gid, value as f64);
            }
        }
        Message::StartBandDynamicTarget(index, threshold, target_gain) => {
            let shape = state.shared.params.get(ParamId::para_type(index)) as u8;
            if !BandType::from(shape).supports_dynamic_target() {
                return Task::none();
            }
            let dyn_id = ParamId::para_dyn(index);
            let threshold_id = ParamId::para_dyn_threshold(index);
            if !state.shared.params.get_bool(dyn_id) {
                state.shared.set_param_outbound_only(dyn_id, 1.0);
                state.shared.mark_gesture_begin_pending(dyn_id);
                state.shared.mark_gesture_end_pending(dyn_id);
                state
                    .shared
                    .set_param_outbound_only(threshold_id, threshold.clamp(-24.0, 24.0) as f64);
                state.shared.mark_gesture_begin_pending(threshold_id);
                state.shared.mark_gesture_end_pending(threshold_id);
            }
            return update(state, Message::SetBandDynamicTarget(index, target_gain));
        }
        Message::SetBandDynamicTarget(index, target_gain) => {
            let shape = state.shared.params.get(ParamId::para_type(index)) as u8;
            if !BandType::from(shape).supports_dynamic_target() {
                return Task::none();
            }
            let gain_id = ParamId::para_gain(index);
            let dyn_id = ParamId::para_dyn(index);
            let range_id = ParamId::para_dyn_range(index);
            let target_gain = target_gain.clamp(-24.0, 24.0);
            if !state.shared.params.get_bool(dyn_id) {
                state.shared.set_param_outbound_only(dyn_id, 1.0);
                state.shared.mark_gesture_begin_pending(dyn_id);
                state.shared.mark_gesture_end_pending(dyn_id);
            }
            if state.active_gestures.insert(gain_id) {
                state.shared.mark_gesture_begin_pending(gain_id);
            }
            if state.active_gestures.insert(range_id) {
                state.shared.mark_gesture_begin_pending(range_id);
            }
            state
                .shared
                .set_param_outbound_only(gain_id, target_gain as f64);
            state
                .shared
                .set_param_outbound_only(range_id, (-target_gain).clamp(-24.0, 24.0) as f64);
        }
        Message::EndBandDrag(index) => {
            let end_gestures = |state: &mut State, band: usize| {
                let fid = ParamId::para_freq(band);
                let gid = ParamId::para_gain(band);
                let range_id = ParamId::para_dyn_range(band);
                let threshold_id = ParamId::para_dyn_threshold(band);
                if state.active_gestures.remove(&fid) {
                    state.shared.mark_gesture_end_pending(fid);
                }
                if state.active_gestures.remove(&gid) {
                    state.shared.mark_gesture_end_pending(gid);
                }
                if state.active_gestures.remove(&range_id) {
                    state.shared.mark_gesture_end_pending(range_id);
                }
                if state.active_gestures.remove(&threshold_id) {
                    state.shared.mark_gesture_end_pending(threshold_id);
                }
            };
            if state.selection.contains(&index) {
                let members: Vec<usize> = state.selection.iter().copied().collect();
                for band in members {
                    end_gestures(state, band);
                }
            } else {
                end_gestures(state, index);
            }
        }
        Message::SetBoolParam(id, value) => {
            state
                .shared
                .set_param_outbound_only(id, if value { 1.0 } else { 0.0 });
            state.shared.mark_gesture_begin_pending(id);
            state.shared.mark_gesture_end_pending(id);
            if value
                && let Some(band) = dyn_param_band(id)
                && BandType::from(state.shared.params.get(ParamId::para_type(band)) as u8)
                    .supports_dynamic_target()
            {
                let gain = state.shared.params.get(ParamId::para_gain(band)) as f32;
                let range_id = ParamId::para_dyn_range(band);
                state
                    .shared
                    .set_param_outbound_only(range_id, (-gain).clamp(-24.0, 24.0) as f64);
                state.shared.mark_gesture_begin_pending(range_id);
                state.shared.mark_gesture_end_pending(range_id);
            } else if !value
                && let Some(band) = dyn_param_band(id)
                && BandType::from(state.shared.params.get(ParamId::para_type(band)) as u8)
                    .supports_dynamic_target()
            {
                let threshold = state.shared.params.get(ParamId::para_dyn_threshold(band)) as f32;
                let gain_id = ParamId::para_gain(band);
                state
                    .shared
                    .set_param_outbound_only(gain_id, threshold.clamp(-24.0, 24.0) as f64);
                state.shared.mark_gesture_begin_pending(gain_id);
                state.shared.mark_gesture_end_pending(gain_id);
            }
        }
        Message::ReleaseParam(id) => {
            if state.active_gestures.remove(&id) {
                state.shared.mark_gesture_end_pending(id);
            }
            if let Some(band) = gain_param_band(id) {
                let range_id = ParamId::para_dyn_range(band);
                if state.active_gestures.remove(&range_id) {
                    state.shared.mark_gesture_end_pending(range_id);
                }
            }
        }
        Message::CreateBand(freq, gain) => {
            for i in 0..32 {
                if !state.shared.params.get_bool(ParamId::para_on(i)) {
                    let oid = ParamId::para_on(i);
                    let fid = ParamId::para_freq(i);
                    let gid = ParamId::para_gain(i);
                    let qid = ParamId::para_q(i);
                    let tid = ParamId::para_type(i);
                    let did = ParamId::para_dyn(i);
                    let q = if gain >= 0.0 {
                        1.0 + (gain / 24.0) * 2.0
                    } else {
                        1.0 + (gain.abs() / 24.0) * 9.0
                    };
                    state.shared.set_param_outbound_only(oid, 1.0);
                    state.shared.set_param_outbound_only(fid, freq as f64);
                    state.shared.set_param_outbound_only(gid, gain as f64);
                    state.shared.set_param_outbound_only(qid, q as f64);
                    state.shared.set_param_outbound_only(tid, 1.0);
                    state.shared.set_param_outbound_only(did, 0.0);
                    state.selected_band = Some(i);
                    state.selection.clear();
                    state.selection.insert(i);
                    break;
                }
            }

            state.last_registry_version = 0;
        }
        Message::SelectBand(index) => {
            state.selected_band = Some(index);
            state.selection.clear();
            state.selection.insert(index);
        }
        Message::ToggleBandSelection(index) => {
            if state.selection.contains(&index) {
                state.selection.remove(&index);
                if state.selected_band == Some(index) {
                    state.selected_band = state.selection.iter().next().copied();
                }
            } else {
                state.selection.insert(index);
                state.selected_band = Some(index);
            }
        }
        Message::CycleBandShape(index) => {
            let current = BandType::from(state.shared.params.get(ParamId::para_type(index)) as u8);
            let pos = BandType::ALL
                .iter()
                .position(|t| *t == current)
                .unwrap_or(0);
            let next = BandType::ALL[(pos + 1) % BandType::ALL.len()];
            let id = ParamId::para_type(index);
            state
                .shared
                .set_param_outbound_only(id, u8::from(next) as f64);
            state.shared.mark_gesture_begin_pending(id);
            state.shared.mark_gesture_end_pending(id);
        }
        Message::ToggleListen(index) => {
            if state.shared.get_listen_band() == index as u32 {
                state.shared.set_listen_band(32);
            } else {
                state.shared.set_listen_band(index as u32);
            }
        }
        Message::DeselectBand => {
            state.selected_band = None;
            state.selection.clear();
        }
        Message::DeleteBand => {
            if state.selection.is_empty() {
                if let Some(sb) = state.selected_band {
                    state
                        .shared
                        .set_param_outbound_only(ParamId::para_on(sb), 0.0);
                }
            } else {
                for &band in &state.selection {
                    state
                        .shared
                        .set_param_outbound_only(ParamId::para_on(band), 0.0);
                }
            }
            state.selected_band = None;
            state.selection.clear();

            state.last_registry_version = 0;
        }
        Message::SetChannels(mode) => {
            state
                .shared
                .set_param_outbound_only(ParamId::Channels, u32::from(mode) as f64);
            state.shared.sync_channels_from_params();
            state.shared.request_audio_ports_rescan();
            state.shared.mark_dirty();
        }
        Message::TogglePreSpectrum => {
            state.show_pre_spectrum = !state.show_pre_spectrum;
        }
        Message::TogglePostSpectrum => {
            state.show_post_spectrum = !state.show_post_spectrum;
        }
        Message::ToggleFreeze => {
            state.spectrum_frozen = !state.spectrum_frozen;
        }
        Message::SetDisplayRange(range) => {
            state.display_range_db = range;
        }
        Message::SetTilt(tilt) => {
            state.tilt_db_per_oct = tilt;
        }
        Message::ToggleSketchMode => {
            state.sketch_mode = !state.sketch_mode;
        }
        Message::BeginSketch => {
            state.last_sketch.clear();
        }
        Message::ApplySketch(points) => {
            state.sketch_mode = false;
            let fitted = fit_sketch_bands(&points, state.shared.sample_rate());
            state.last_sketch = points;
            create_fitted_bands(state, &fitted);
        }
        Message::SetMatchPeer(slot) => {
            state.match_peer = Some(slot);
        }
        Message::ToggleInstanceList => {
            state.show_instances = !state.show_instances;
            if !state.show_instances {
                state.ghost_peer = None;
                state.ghost_bands.clear();
            }
        }
        Message::SelectGhostPeer(slot) => {
            if state.ghost_peer == Some(slot) {
                state.ghost_peer = None;
                state.ghost_bands.clear();
            } else {
                state.ghost_peer = Some(slot);
            }
        }
        Message::ApplyEqMatch => {
            let Some(peer_slot) = state.match_peer else {
                return Task::none();
            };
            let Some(peer) = state.eq_peers.iter().find(|p| p.slot_index() == peer_slot) else {
                return Task::none();
            };
            let Some(slot) = peer.fft_slot() else {
                return Task::none();
            };
            let mut fft = bus::FftData::default();
            if !slot.read(&mut fft) || fft.valid_bins == 0 {
                return Task::none();
            }
            let mut diff = [0.0_f32; SPECTRUM_BINS];
            for (i, d) in diff.iter_mut().enumerate() {
                let peer_db = if i < fft.valid_bins {
                    fft.bins[i]
                } else {
                    -90.0
                };
                let our_db = state.pre_spectrum_db[0][i].max(state.pre_spectrum_db[1][i]);
                *d = if peer_db > -70.0 && our_db > -70.0 {
                    (peer_db - our_db).clamp(-30.0, 30.0)
                } else {
                    0.0
                };
            }
            let fitted = fit_match_bands(&diff, state.shared.sample_rate());
            create_fitted_bands(state, &fitted);
        }
        Message::CopyBands => {
            let mut clipboard = BAND_CLIPBOARD.lock();
            clipboard.clear();
            for i in 0..32 {
                if !state.shared.params.get_bool(ParamId::para_on(i)) {
                    continue;
                }
                clipboard.push(BandSnapshot {
                    freq: p_of(&state.shared.params, ParamId::para_freq(i)),
                    gain: p_of(&state.shared.params, ParamId::para_gain(i)),
                    q: p_of(&state.shared.params, ParamId::para_q(i)),
                    typ: p_of(&state.shared.params, ParamId::para_type(i)) as u8,
                    slope: p_of(&state.shared.params, ParamId::para_slope(i)) as u8,
                    placement: p_of(&state.shared.params, ParamId::para_placement(i)) as u8,
                    dyn_on: state.shared.params.get_bool(ParamId::para_dyn(i)),
                    dyn_threshold: p_of(&state.shared.params, ParamId::para_dyn_threshold(i)),
                    dyn_ratio: p_of(&state.shared.params, ParamId::para_dyn_ratio(i)),
                    dyn_knee: p_of(&state.shared.params, ParamId::para_dyn_knee(i)),
                    dyn_range: p_of(&state.shared.params, ParamId::para_dyn_range(i)),
                    dyn_attack: p_of(&state.shared.params, ParamId::para_dyn_attack(i)),
                    dyn_release: p_of(&state.shared.params, ParamId::para_dyn_release(i)),
                    dyn_source: p_of(&state.shared.params, ParamId::para_dyn_source(i)),
                    dyn_mode: p_of(&state.shared.params, ParamId::para_dyn_mode(i)),
                });
            }
        }
        Message::PasteBands => {
            let snapshots = BAND_CLIPBOARD.lock().clone();
            for snap in snapshots {
                for i in 0..32 {
                    if !state.shared.params.get_bool(ParamId::para_on(i)) {
                        for (id, value) in [
                            (ParamId::para_on(i), 1.0),
                            (ParamId::para_freq(i), snap.freq as f64),
                            (ParamId::para_gain(i), snap.gain as f64),
                            (ParamId::para_q(i), snap.q as f64),
                            (ParamId::para_type(i), snap.typ as f64),
                            (ParamId::para_slope(i), snap.slope as f64),
                            (ParamId::para_placement(i), snap.placement as f64),
                            (ParamId::para_dyn(i), if snap.dyn_on { 1.0 } else { 0.0 }),
                            (ParamId::para_dyn_threshold(i), snap.dyn_threshold as f64),
                            (ParamId::para_dyn_ratio(i), snap.dyn_ratio as f64),
                            (ParamId::para_dyn_knee(i), snap.dyn_knee as f64),
                            (ParamId::para_dyn_range(i), snap.dyn_range as f64),
                            (ParamId::para_dyn_attack(i), snap.dyn_attack as f64),
                            (ParamId::para_dyn_release(i), snap.dyn_release as f64),
                            (ParamId::para_dyn_source(i), snap.dyn_source as f64),
                            (ParamId::para_dyn_mode(i), snap.dyn_mode as f64),
                        ] {
                            state.shared.set_param_outbound_only(id, value);
                        }
                        break;
                    }
                }
            }
            state.shared.mark_dirty();
            state.last_registry_version = 0;
        }
        Message::NoOp => {}
        Message::UiTick => {
            if !state.spectrum_frozen {
                state.pre_spectrum_db = state.shared.input_spectrum_db();
                state.post_spectrum_db = state.shared.output_spectrum_db();
            }
            for (i, gr) in state.band_gr_db.iter_mut().enumerate() {
                *gr = state.shared.band_dyn_gain_db(i);
            }
            state.shared.set_dyn_visual_band(state.selected_band);

            let version = bus::registry_version();
            if version != state.last_registry_version {
                state.bus_peers = bus::discover(|p| p.plugin_type != bus::PluginType::Eq);
                let own_slot = state.shared.own_slot();
                state.eq_peers = bus::discover(|p| {
                    p.plugin_type == bus::PluginType::Eq && p.slot_index() != own_slot
                });
                state.peer_band_counts = state
                    .eq_peers
                    .iter()
                    .map(|p| {
                        let count = p
                            .bands_slot()
                            .and_then(|slot| {
                                let mut bands = bus::EqBands::default();
                                slot.read(&mut bands).then_some(bands.len)
                            })
                            .unwrap_or(0);
                        (p.slot_index(), count)
                    })
                    .collect();
                state.last_registry_version = version;
            }

            if let Some(ghost) = state.ghost_peer {
                let peer = state
                    .eq_peers
                    .iter()
                    .find(|p| p.slot_index() == ghost)
                    .copied();
                if let Some(peer) = peer {
                    if let Some(slot) = peer.bands_slot() {
                        let mut bands = bus::EqBands::default();
                        if slot.read(&mut bands) {
                            state.ghost_bands =
                                bands.bands[..bands.len.min(bands.bands.len())].to_vec();
                        }
                    }
                } else {
                    state.ghost_peer = None;
                    state.ghost_bands.clear();
                }
            }

            state.collision_scores.fill(0.0);
            let mut peer_fft = bus::FftData::default();
            for peer in &state.bus_peers {
                if let Some(slot) = peer.fft_slot() {
                    if !slot.read(&mut peer_fft) || peer_fft.valid_bins == 0 {
                        continue;
                    }
                    let nyquist = state.shared.sample_rate() / 2.0;
                    for band_idx in 0..32 {
                        if !state.shared.params.get_bool(ParamId::para_on(band_idx)) {
                            continue;
                        }
                        let freq = state.shared.params.get(ParamId::para_freq(band_idx)) as f32;
                        let gain = state.shared.params.get(ParamId::para_gain(band_idx)) as f32;
                        if gain <= -0.1 {
                            continue;
                        }

                        let bin_idx = ((freq / nyquist) * peer_fft.valid_bins as f32)
                            .clamp(0.0, (peer_fft.valid_bins - 1) as f32)
                            as usize;
                        let db = peer_fft.bins[bin_idx];
                        if db > -60.0 {
                            let score = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
                            state.collision_scores[band_idx] =
                                state.collision_scores[band_idx].max(score);
                        }
                    }
                }
            }

            return next_ui_tick_task();
        }
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;

    let listen_band = state.shared.get_listen_band();
    let is_listen_on = state
        .selected_band
        .map(|sb| listen_band == sb as u32)
        .unwrap_or(false);

    let mut bands = Vec::with_capacity(32);
    for i in 0..32 {
        bands.push((
            i,
            p(ParamId::para_freq(i)),
            p(ParamId::para_gain(i)),
            p(ParamId::para_q(i)),
            state.shared.params.get_bool(ParamId::para_on(i)),
            p(ParamId::para_type(i)) as u8,
            p(ParamId::para_slope(i)) as u8,
            p(ParamId::para_placement(i)) as u8,
        ));
    }

    let mut band_dyn_on = [false; 32];
    let mut band_dyn_threshold = [0.0; 32];
    for i in 0..32 {
        band_dyn_on[i] = state.shared.params.get_bool(ParamId::para_dyn(i));
        band_dyn_threshold[i] = p(ParamId::para_dyn_threshold(i));
    }
    let sample_rate = state.shared.sample_rate();
    let response = eq_response_graph(EqResponseCanvas {
        bands: bands.clone(),
        selection: state.selection.clone(),
        sketch_mode: state.sketch_mode,
        last_sketch: state.last_sketch.clone(),
        ghost_bands: state.ghost_bands.clone(),
        pre_spectrum_db: state.pre_spectrum_db,
        post_spectrum_db: state.post_spectrum_db,
        stereo: state.shared.channels.load(Ordering::Acquire) >= 2,
        show_pre: state.show_pre_spectrum,
        show_post: state.show_post_spectrum,
        tilt: state.tilt_db_per_oct,
        display_range_db: state.display_range_db,
        selected_band: state.selected_band,
        band_dyn_on,
        band_dyn_threshold,
        sample_rate,
        listen_mode: is_listen_on,
        collision_scores: state.collision_scores,
        band_gr_db: state.band_gr_db,
    });

    let channels = p(ParamId::Channels).round() as u32;
    let channels_dropdown = maolan_baseview::iced::widget::pick_list(
        vec![ChannelMode::Mono, ChannelMode::Stereo],
        Some(ChannelMode::from(channels)),
        Message::SetChannels,
    )
    .placeholder("Channels")
    .width(Length::Fixed(95.0));

    let peer_slots: Vec<u32> = state.eq_peers.iter().map(|p| p.slot_index()).collect();
    let analyzer_controls = row![
        channels_dropdown,
        maolan_baseview::iced::widget::checkbox(state.show_pre_spectrum)
            .label("Pre")
            .on_toggle(|_| Message::TogglePreSpectrum),
        maolan_baseview::iced::widget::checkbox(state.show_post_spectrum)
            .label("Post")
            .on_toggle(|_| Message::TogglePostSpectrum),
        maolan_baseview::iced::widget::checkbox(state.spectrum_frozen)
            .label("Freeze")
            .on_toggle(|_| Message::ToggleFreeze),
        text("Range").size(11),
        maolan_baseview::iced::widget::pick_list(
            vec![3.0_f32, 6.0, 12.0, 30.0],
            Some(state.display_range_db),
            Message::SetDisplayRange,
        )
        .width(Length::Fixed(70.0)),
        text("Tilt").size(11),
        maolan_baseview::iced::widget::pick_list(
            vec![0.0_f32, 4.5],
            Some(state.tilt_db_per_oct),
            Message::SetTilt,
        )
        .width(Length::Fixed(70.0)),
        text("Match").size(11),
        maolan_baseview::iced::widget::pick_list(
            peer_slots,
            state.match_peer,
            Message::SetMatchPeer,
        )
        .placeholder("EQ instance")
        .width(Length::Fixed(110.0)),
        maolan_baseview::iced::widget::button("Apply").on_press(Message::ApplyEqMatch),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    const PROCESSING_MODES: [&str; 3] = ["Zero Latency", "Natural Phase", "Linear Phase"];
    const CHARACTERS: [&str; 3] = ["Clean", "Gentle", "Warm"];
    let mode_idx = (p(ParamId::ProcessingMode).round() as usize).min(2);
    let char_idx = (p(ParamId::Character).round() as usize).min(2);
    let processing_controls = row![
        text("Mode").size(11),
        maolan_baseview::iced::widget::pick_list(
            PROCESSING_MODES.to_vec(),
            Some(PROCESSING_MODES[mode_idx]),
            |m| {
                let v = PROCESSING_MODES.iter().position(|x| *x == m).unwrap_or(0) as f32;
                Message::SetParamImmediate(ParamId::ProcessingMode, v)
            },
        )
        .width(Length::Fixed(120.0)),
        text("Character").size(11),
        maolan_baseview::iced::widget::pick_list(
            CHARACTERS.to_vec(),
            Some(CHARACTERS[char_idx]),
            |c| {
                let v = CHARACTERS.iter().position(|x| *x == c).unwrap_or(0) as f32;
                Message::SetParamImmediate(ParamId::Character, v)
            },
        )
        .width(Length::Fixed(95.0)),
        maolan_baseview::iced::widget::checkbox(p(ParamId::AutoGain) >= 0.5)
            .label("Auto Gain")
            .on_toggle(|v| Message::SetBoolParam(ParamId::AutoGain, v)),
        knob(
            "Scale".to_string(),
            ParamId::GainScale,
            p(ParamId::GainScale),
            "",
            0.01,
        ),
        maolan_baseview::iced::widget::checkbox(p(ParamId::PhaseInvert) >= 0.5)
            .label("Phase")
            .on_toggle(|v| Message::SetBoolParam(ParamId::PhaseInvert, v)),
        maolan_baseview::iced::widget::checkbox(state.sketch_mode)
            .label("Sketch")
            .on_toggle(|_| Message::ToggleSketchMode),
        maolan_baseview::iced::widget::checkbox(state.show_instances)
            .label("Instances")
            .on_toggle(|_| Message::ToggleInstanceList),
        maolan_baseview::iced::widget::button("Copy Bands").on_press(Message::CopyBands),
        maolan_baseview::iced::widget::button("Paste Bands").on_press(Message::PasteBands),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let instance_row: Element<'_, Message> = if state.show_instances {
        let mut strip = row![text("Instances").size(11)]
            .spacing(8)
            .align_y(Alignment::Center);
        if state.peer_band_counts.is_empty() {
            strip = strip.push(text("no other EQ instances in session").size(11));
        }
        for (slot, count) in &state.peer_band_counts {
            let active = state.ghost_peer == Some(*slot);
            let label = format!("{} EQ #{slot} ({count})", if active { "●" } else { "○" });
            strip = strip.push(
                maolan_baseview::iced::widget::button(text(label).size(11))
                    .on_press(Message::SelectGhostPeer(*slot)),
            );
        }
        strip.into()
    } else {
        row![].spacing(0).into()
    };

    let knobs: Element<'_, Message> = if let Some(sb) = state.selected_band {
        let band_type = BandType::from(p(ParamId::para_type(sb)) as u8);
        let type_dropdown = maolan_baseview::iced::widget::pick_list(
            BandType::ALL.to_vec(),
            Some(band_type),
            move |t| Message::SetParam(ParamId::para_type(sb), u8::from(t) as f32),
        )
        .placeholder("Shape")
        .width(Length::Fixed(100.0));

        let placement = match Placement::from(p(ParamId::para_placement(sb)) as u8) {
            Placement::Left | Placement::Right => Placement::Stereo,
            placement => placement,
        };
        let placement_dropdown = maolan_baseview::iced::widget::pick_list(
            Placement::ALL.to_vec(),
            Some(placement),
            move |pl| Message::SetParam(ParamId::para_placement(sb), u8::from(pl) as f32),
        )
        .placeholder("Placement")
        .width(Length::Fixed(85.0));

        let slope = Slope::from(p(ParamId::para_slope(sb)) as u8);
        let slope_dropdown = maolan_baseview::iced::widget::pick_list(
            vec![
                Slope::Db12,
                Slope::Db24,
                Slope::Db48,
                Slope::Db96,
                Slope::Brickwall,
            ],
            Some(slope),
            move |s| Message::SetParam(ParamId::para_slope(sb), u8::from(s) as f32),
        )
        .placeholder("Slope")
        .width(Length::Fixed(100.0));

        let listen_checkbox = maolan_baseview::iced::widget::checkbox(is_listen_on)
            .label("Listen")
            .on_toggle(move |v| {
                if v {
                    state.shared.set_listen_band(sb as u32);
                } else {
                    state.shared.set_listen_band(32);
                }
                Message::UiTick
            });

        let dyn_on = state.shared.params.get_bool(ParamId::para_dyn(sb));
        let band_label = text(format!("Band {}", sb + 1))
            .size(13)
            .color(placement_color(p(ParamId::para_placement(sb)) as u8));
        let mut controls = row![band_label, type_dropdown]
            .spacing(12)
            .align_y(Alignment::Center);
        if channels > 1 {
            controls = controls.push(placement_dropdown);
        }
        controls = controls.push(freq_knob(ParamId::para_freq(sb), p(ParamId::para_freq(sb))));

        if band_type.dyn_capable() {
            controls = controls.push(knob(
                "Gain".to_string(),
                ParamId::para_gain(sb),
                p(ParamId::para_gain(sb)),
                "dB",
                0.1,
            ));
        }

        controls = controls.push(knob(
            "Q".to_string(),
            ParamId::para_q(sb),
            p(ParamId::para_q(sb)),
            "",
            0.01,
        ));

        if matches!(band_type, BandType::LowCut | BandType::HighCut) {
            controls = controls.push(slope_dropdown);
        }

        if band_type.dyn_capable() {
            let dyn_checkbox = maolan_baseview::iced::widget::checkbox(dyn_on)
                .label("Dyn")
                .on_toggle(move |v| Message::SetBoolParam(ParamId::para_dyn(sb), v));
            controls = controls.push(dyn_checkbox);
            if dyn_on {
                let source = if p(ParamId::para_dyn_source(sb)) >= 0.5 {
                    DynSource::External
                } else {
                    DynSource::Internal
                };
                let source_dropdown = maolan_baseview::iced::widget::pick_list(
                    vec![DynSource::Internal, DynSource::External],
                    Some(source),
                    move |s| {
                        Message::SetBoolParam(
                            ParamId::para_dyn_source(sb),
                            matches!(s, DynSource::External),
                        )
                    },
                )
                .placeholder("SC")
                .width(Length::Fixed(95.0));
                let spectral_on = p(ParamId::para_dyn_mode(sb)) >= 0.5;
                let mode_dropdown = maolan_baseview::iced::widget::pick_list(
                    vec!["Band", "Spectral"],
                    Some(if spectral_on { "Spectral" } else { "Band" }),
                    move |m| Message::SetBoolParam(ParamId::para_dyn_mode(sb), m == "Spectral"),
                )
                .placeholder("Mode")
                .width(Length::Fixed(95.0));
                controls = controls
                    .push(mode_dropdown)
                    .push(knob(
                        "Ratio".to_string(),
                        ParamId::para_dyn_ratio(sb),
                        p(ParamId::para_dyn_ratio(sb)),
                        ":1",
                        0.1,
                    ))
                    .push(knob(
                        "Knee".to_string(),
                        ParamId::para_dyn_knee(sb),
                        p(ParamId::para_dyn_knee(sb)),
                        "dB",
                        0.1,
                    ));
                if !band_type.supports_dynamic_target() {
                    controls = controls.push(knob(
                        "Range".to_string(),
                        ParamId::para_dyn_range(sb),
                        p(ParamId::para_dyn_range(sb)),
                        "dB",
                        0.1,
                    ));
                }
                controls = controls
                    .push(knob(
                        "Attack".to_string(),
                        ParamId::para_dyn_attack(sb),
                        p(ParamId::para_dyn_attack(sb)),
                        "ms",
                        0.1,
                    ))
                    .push(knob(
                        "Release".to_string(),
                        ParamId::para_dyn_release(sb),
                        p(ParamId::para_dyn_release(sb)),
                        "ms",
                        1.0,
                    ))
                    .push(source_dropdown);
            }
        }

        controls = controls
            .push(listen_checkbox)
            .push(maolan_baseview::iced::widget::button("Delete").on_press(Message::DeleteBand));
        controls.into()
    } else {
        let hint = if state.sketch_mode {
            "Sketch ON: drag on the display to draw a curve, release to create bands"
        } else {
            "Double-click: new band · tick Sketch to draw a curve · wheel on node: Q"
        };
        row![text(hint).size(12)]
            .spacing(20)
            .align_y(Alignment::Center)
            .into()
    };
    let meter_channels = if state.shared.channels.load(Ordering::Acquire) >= 2 {
        2
    } else {
        1
    };

    let display_row = row![
        gain_slider(ParamId::InputGain, p(ParamId::InputGain), "dB", 0.1,),
        vertical_ticks(),
        vu_meter(meter_channels, state.shared.input_levels_db()),
        response,
        vu_meter(meter_channels, state.shared.output_levels_db()),
        vertical_ticks(),
        gain_slider(ParamId::OutputGain, p(ParamId::OutputGain), "dB", 0.1,),
    ]
    .spacing(8)
    .height(Length::Fill)
    .align_y(Alignment::Center);

    let content = column![
        display_row,
        analyzer_controls,
        instance_row,
        processing_controls,
        knobs
    ]
    .spacing(14)
    .align_x(Alignment::Center);

    container(content)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center)
        .into()
}

#[derive(Debug, Clone, Copy)]
enum DragTarget {
    Band(usize),
    DynamicTarget(usize),
}

#[derive(Default, Debug)]
struct EqResponseState {
    dragging: Option<DragTarget>,
    hover_pos: Option<Point>,
    drag_last_pos: Option<Point>,
    last_press: Option<(Instant, Point)>,
    modifiers: keyboard::Modifiers,
    sketching: bool,
    sketch_points: Vec<(f32, f32)>,
}

/// (idx, freq, gain, q, on, typ, slope, placement)
type BandView = (usize, f32, f32, f32, bool, u8, u8, u8);

#[derive(Clone)]
struct EqResponseCanvas {
    bands: Vec<BandView>,
    selection: HashSet<usize>,
    sketch_mode: bool,
    last_sketch: Vec<(f32, f32)>,
    ghost_bands: Vec<bus::EqBand>,
    pre_spectrum_db: [[f32; SPECTRUM_BINS]; 2],
    post_spectrum_db: [[f32; SPECTRUM_BINS]; 2],
    stereo: bool,
    show_pre: bool,
    show_post: bool,
    tilt: f32,
    display_range_db: f32,
    selected_band: Option<usize>,
    band_dyn_on: [bool; 32],
    band_dyn_threshold: [f32; 32],
    sample_rate: f32,
    listen_mode: bool,
    collision_scores: [f32; 32],
    band_gr_db: [f32; 32],
}

pub fn placement_color(placement: u8) -> Color {
    match placement {
        dsp::PLACEMENT_LEFT => Color::from_rgb(0.25, 0.60, 1.00), // blue
        dsp::PLACEMENT_RIGHT => Color::from_rgb(1.00, 0.30, 0.55), // pink/red
        dsp::PLACEMENT_MID => Color::from_rgb(0.30, 0.90, 0.40),  // green
        dsp::PLACEMENT_SIDE => Color::from_rgb(0.55, 0.45, 1.00), // purple
        _ => Color::from_rgb(1.00, 0.83, 0.10),                   // yellow (stereo)
    }
}

impl EqResponseCanvas {
    const F_MIN: f32 = 20.0;
    const F_MAX: f32 = 20_000.0;
    const SPECTRUM_MIN_DB: f32 = -60.0;
    const SPECTRUM_MAX_DB: f32 = 0.0;

    fn range_max(&self) -> f32 {
        display_range_bounds(self.display_range_db).1
    }

    fn range_min(&self) -> f32 {
        display_range_bounds(self.display_range_db).0
    }

    fn freq_to_x(freq: f32, bounds: Rectangle) -> f32 {
        freq_to_x(freq, bounds)
    }

    fn x_to_freq(x: f32, bounds: Rectangle) -> f32 {
        x_to_freq(x, bounds)
    }

    fn bin_freq(&self, bin: usize) -> f32 {
        let t = bin as f32 / (SPECTRUM_BINS.saturating_sub(1).max(1) as f32);
        Self::F_MIN * (Self::F_MAX / Self::F_MIN).powf(t)
    }

    fn apply_tilt(&self, db: f32, freq: f32) -> f32 {
        if self.tilt == 0.0 || db <= Self::SPECTRUM_MIN_DB + 1.0 {
            return db;
        }
        db + self.tilt * (freq / 1000.0).log2()
    }

    fn gain_to_y(&self, gain: f32, bounds: Rectangle) -> f32 {
        let min = self.range_min();
        let max = self.range_max();
        db_to_y(gain, bounds, min, max)
    }

    fn y_to_gain(&self, y: f32, bounds: Rectangle) -> f32 {
        let min = self.range_min();
        let max = self.range_max();
        y_to_db(y, bounds, min, max)
    }

    fn threshold_to_y(&self, threshold: f32, bounds: Rectangle) -> f32 {
        self.gain_to_y(threshold, bounds)
    }

    fn y_to_threshold(&self, y: f32, bounds: Rectangle) -> f32 {
        self.y_to_gain(y, bounds)
    }

    fn band_uses_threshold_dot(&self, global_idx: usize, typ: u8) -> bool {
        BandType::from(typ).supports_dynamic_target()
            && self.band_dyn_on.get(global_idx).copied().unwrap_or(false)
    }

    fn spectrum_to_y(&self, db: f32, bounds: Rectangle) -> f32 {
        let db = db.clamp(Self::SPECTRUM_MIN_DB, Self::SPECTRUM_MAX_DB);
        let t = (db - Self::SPECTRUM_MIN_DB) / (Self::SPECTRUM_MAX_DB - Self::SPECTRUM_MIN_DB);
        bounds.y + (1.0 - t) * bounds.height
    }

    fn smoothed_spectrum_points(
        &self,
        bins_db: &[f32; SPECTRUM_BINS],
        bounds: Rectangle,
    ) -> Vec<Point> {
        bins_db
            .iter()
            .enumerate()
            .map(|(i, &db)| {
                let t = i as f32 / (SPECTRUM_BINS.saturating_sub(1).max(1) as f32);
                Point::new(t * bounds.width, self.spectrum_to_y(db, bounds))
            })
            .collect()
    }

    fn draw_smooth_points(points: &[Point], b: &mut canvas::path::Builder) {
        let Some(&first) = points.first() else {
            return;
        };
        b.move_to(first);
        if points.len() == 1 {
            return;
        }
        if points.len() == 2 {
            b.line_to(points[1]);
            return;
        }

        for i in 0..points.len() - 1 {
            let p0 = if i == 0 { points[i] } else { points[i - 1] };
            let p1 = points[i];
            let p2 = points[i + 1];
            let p3 = if i + 2 < points.len() {
                points[i + 2]
            } else {
                points[i + 1]
            };
            let c1 = Point::new(p1.x + (p2.x - p0.x) / 6.0, p1.y + (p2.y - p0.y) / 6.0);
            let c2 = Point::new(p2.x - (p3.x - p1.x) / 6.0, p2.y - (p3.y - p1.y) / 6.0);
            b.bezier_curve_to(c1, c2, p2);
        }
    }

    /// Spectrum Grab: snaps a newly created band's frequency to the strongest
    /// nearby peak of the post-EQ spectrum (within ±15%, above -50 dB).
    fn snap_to_spectrum_peak(&self, freq: f32) -> f32 {
        let mut best: Option<(f32, f32)> = None;
        for bin in 0..SPECTRUM_BINS {
            let f = self.bin_freq(bin);
            if f < freq * 0.85 || f > freq * 1.18 {
                continue;
            }
            let db = self.post_spectrum_db[0][bin].max(self.post_spectrum_db[1][bin]);
            if best.map(|(best_db, _)| db > best_db).unwrap_or(true) {
                best = Some((db, f));
            }
        }
        match best {
            Some((db, peak_freq)) if db > -50.0 => peak_freq,
            _ => freq,
        }
    }

    fn spectrum_path(&self, bins_db: &[f32; SPECTRUM_BINS], bounds: Rectangle) -> Path {
        let points = self.smoothed_spectrum_points(bins_db, bounds);
        Path::new(|b| {
            Self::draw_smooth_points(&points, b);
        })
    }

    fn spectrum_fill_path(&self, bins_db: &[f32; SPECTRUM_BINS], bounds: Rectangle) -> Path {
        let points = self.smoothed_spectrum_points(bins_db, bounds);
        Path::new(|b| {
            Self::draw_smooth_points(&points, b);
            b.line_to(Point::new(bounds.width, bounds.height));
            b.line_to(Point::new(0.0, bounds.height));
            b.close();
        })
    }
}

impl Program<Message> for EqResponseCanvas {
    type State = EqResponseState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<CanvasAction<Message>> {
        let local_bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: bounds.width,
            height: bounds.height,
        };
        let hit_dot = |p: Point| -> Option<(usize, bool)> {
            let mut closest = None;
            let mut best_d2 = 12.0_f32 * 12.0_f32;
            for (local_idx, (global_idx, freq, gain, _q, on, typ, _slope, _placement)) in
                self.bands.iter().enumerate()
            {
                if !*on {
                    continue;
                }
                let x = Self::freq_to_x(*freq, local_bounds);
                let y = if self.band_uses_threshold_dot(*global_idx, *typ) {
                    self.threshold_to_y(
                        self.band_dyn_threshold
                            .get(*global_idx)
                            .copied()
                            .unwrap_or(-24.0),
                        local_bounds,
                    )
                } else {
                    self.gain_to_y(*gain, local_bounds)
                };
                let dx = p.x - x;
                let dy = p.y - y;
                let d2 = dx * dx + dy * dy;
                if d2 <= best_d2 {
                    best_d2 = d2;
                    closest = Some((local_idx, false));
                }

                if self.band_uses_threshold_dot(*global_idx, *typ) {
                    let target_y = self.gain_to_y(*gain, local_bounds);
                    let dx = p.x - x;
                    let dy = p.y - target_y;
                    let d2 = dx * dx + dy * dy;
                    if d2 <= best_d2 {
                        best_d2 = d2;
                        closest = Some((local_idx, true));
                    }
                }
            }
            closest
        };
        let hit_band = |p: Point| -> Option<usize> {
            hit_dot(p).map(|(local_idx, _dynamic_target)| local_idx)
        };
        match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Delete),
                ..
            }) => {
                return Some(CanvasAction::publish(Message::DeleteBand).and_capture());
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if self.sketch_mode => {
                if let Some(p) = cursor.position_in(bounds) {
                    state.sketching = true;
                    state.sketch_points = vec![(
                        Self::x_to_freq(p.x, local_bounds),
                        self.y_to_gain(p.y, local_bounds),
                    )];
                    return Some(CanvasAction::publish(Message::BeginSketch).and_capture());
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(p) = cursor.position_in(bounds) {
                    if let Some((local_idx, dynamic_target)) = hit_dot(p) {
                        let global_idx = self.bands[local_idx].0;
                        if state.modifiers.alt() {
                            return Some(
                                CanvasAction::publish(Message::ToggleListen(global_idx))
                                    .and_capture(),
                            );
                        }
                        if state.modifiers.shift() {
                            return Some(
                                CanvasAction::publish(Message::ToggleBandSelection(global_idx))
                                    .and_capture(),
                            );
                        }
                        state.dragging = Some(if dynamic_target {
                            DragTarget::DynamicTarget(local_idx)
                        } else {
                            DragTarget::Band(local_idx)
                        });
                        state.drag_last_pos = Some(p);
                        if !self.selection.contains(&global_idx) {
                            return Some(
                                CanvasAction::publish(Message::SelectBand(global_idx))
                                    .and_capture(),
                            );
                        }
                        return Some(CanvasAction::capture());
                    } else {
                        let now = Instant::now();
                        let is_double = state
                            .last_press
                            .map(|(t, lp)| {
                                now.duration_since(t) < Duration::from_millis(400)
                                    && (p.x - lp.x).powi(2) + (p.y - lp.y).powi(2) < 64.0
                            })
                            .unwrap_or(false);
                        state.last_press = Some((now, p));
                        if is_double {
                            for (
                                local_idx,
                                (_global_idx, _freq, _gain, _q, on, _typ, _slope, _placement),
                            ) in self.bands.iter().enumerate()
                            {
                                if !*on {
                                    let raw_freq = Self::x_to_freq(p.x, local_bounds);
                                    let freq = self.snap_to_spectrum_peak(raw_freq);
                                    let gain = self.y_to_gain(p.y, local_bounds);
                                    state.dragging = Some(DragTarget::Band(local_idx));
                                    state.drag_last_pos = Some(p);
                                    state.last_press = None;
                                    return Some(
                                        CanvasAction::publish(Message::CreateBand(freq, gain))
                                            .and_capture(),
                                    );
                                }
                            }
                        }
                        return Some(CanvasAction::publish(Message::DeselectBand).and_capture());
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                if let Some(p) = cursor.position_in(bounds)
                    && let Some(local_idx) = hit_band(p)
                {
                    let global_idx = self.bands[local_idx].0;
                    return Some(
                        CanvasAction::publish(Message::CycleBandShape(global_idx)).and_capture(),
                    );
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle)) => {
                if let Some(p) = cursor.position_in(bounds)
                    && let Some(local_idx) = hit_band(p)
                {
                    let (global_idx, _freq, gain, _q, on, typ, _slope, _placement) =
                        self.bands[local_idx];
                    if on
                        && Some(global_idx) == self.selected_band
                        && BandType::from(typ).supports_dynamic_target()
                    {
                        let threshold =
                            self.y_to_threshold(self.gain_to_y(gain, local_bounds), local_bounds);
                        state.dragging = Some(DragTarget::DynamicTarget(local_idx));
                        state.drag_last_pos = Some(p);
                        return Some(
                            CanvasAction::publish(Message::StartBandDynamicTarget(
                                global_idx,
                                threshold,
                                self.y_to_gain(p.y, local_bounds),
                            ))
                            .and_capture(),
                        );
                    }
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if let Some(p) = cursor.position_in(bounds) {
                    let step = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => *y,
                        mouse::ScrollDelta::Pixels { y, .. } => *y / 50.0,
                    };
                    let target = hit_band(p).or_else(|| {
                        self.selected_band.and_then(|sel| {
                            self.bands
                                .iter()
                                .position(|(idx, _, _, _, on, _, _, _)| *idx == sel && *on)
                        })
                    });
                    if let Some(local_idx) = target {
                        let (global_idx, _freq, _gain, q, on, _typ, _slope, _placement) =
                            self.bands[local_idx];
                        if on {
                            let new_q = (q * (1.0 + step * 0.15)).clamp(0.1, 24.0);
                            return Some(
                                CanvasAction::publish(Message::SetParamImmediate(
                                    ParamId::para_q(global_idx),
                                    new_q,
                                ))
                                .and_capture(),
                            );
                        }
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left | mouse::Button::Middle,
            )) => {
                if state.sketching {
                    state.sketching = false;
                    let points = std::mem::take(&mut state.sketch_points);
                    if points.len() >= 4 {
                        return Some(
                            CanvasAction::publish(Message::ApplySketch(points)).and_capture(),
                        );
                    }
                    return Some(CanvasAction::capture());
                }
                if let Some(dragging) = state.dragging.take() {
                    state.drag_last_pos = None;
                    let local_idx = match dragging {
                        DragTarget::Band(local_idx) | DragTarget::DynamicTarget(local_idx) => {
                            local_idx
                        }
                    };
                    if let Some((global_idx, _freq, _gain, _q, _on, _typ, _slope, _placement)) =
                        self.bands.get(local_idx).copied()
                    {
                        return Some(CanvasAction::publish(Message::EndBandDrag(global_idx)));
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(p) = cursor.position_in(bounds) {
                    state.hover_pos = Some(p);
                    if state.sketching {
                        state.sketch_points.push((
                            Self::x_to_freq(p.x, local_bounds),
                            self.y_to_gain(p.y, local_bounds),
                        ));
                        return Some(CanvasAction::capture());
                    }
                    if let Some(dragging) = state.dragging {
                        match dragging {
                            DragTarget::Band(local_idx) => {
                                if let Some((
                                    global_idx,
                                    _freq,
                                    _gain,
                                    q,
                                    _on,
                                    _typ,
                                    _slope,
                                    _placement,
                                )) = self.bands.get(local_idx).copied()
                                {
                                    if state.modifiers.shift() {
                                        let dy = state
                                            .drag_last_pos
                                            .map(|last| p.y - last.y)
                                            .unwrap_or(0.0);
                                        state.drag_last_pos = Some(p);
                                        let new_q = (q * (1.0 - dy * 0.02)).clamp(0.1, 24.0);
                                        return Some(
                                            CanvasAction::publish(Message::SetParamImmediate(
                                                ParamId::para_q(global_idx),
                                                new_q,
                                            ))
                                            .and_capture(),
                                        );
                                    }
                                    state.drag_last_pos = Some(p);
                                    let freq = Self::x_to_freq(p.x, local_bounds);
                                    let gain = if self.band_uses_threshold_dot(global_idx, _typ) {
                                        self.y_to_threshold(p.y, local_bounds)
                                    } else {
                                        self.y_to_gain(p.y, local_bounds)
                                    };
                                    return Some(
                                        CanvasAction::publish(Message::SetBandFreqGain(
                                            global_idx, freq, gain,
                                        ))
                                        .and_capture(),
                                    );
                                }
                            }
                            DragTarget::DynamicTarget(local_idx) => {
                                if let Some((
                                    global_idx,
                                    _freq,
                                    _gain,
                                    _q,
                                    on,
                                    typ,
                                    _slope,
                                    _placement,
                                )) = self.bands.get(local_idx).copied()
                                    && on
                                    && BandType::from(typ).supports_dynamic_target()
                                {
                                    state.drag_last_pos = Some(Point::new(p.x, p.y));
                                    return Some(
                                        CanvasAction::publish(Message::SetBandDynamicTarget(
                                            global_idx,
                                            self.y_to_gain(p.y, local_bounds),
                                        ))
                                        .and_capture(),
                                    );
                                }
                            }
                        }
                    }
                } else {
                    state.hover_pos = None;
                }
            }
            _ => {}
        }
        None
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let local_bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: bounds.width,
            height: bounds.height,
        };
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill(
            &Path::rectangle(Point::new(0.0, 0.0), bounds.size()),
            Color::from_rgb(0.098, 0.098, 0.106),
        );

        let min = self.range_min();
        let max = self.range_max();
        let h_grid_db = [min, min * 0.5, 0.0, max * 0.5, max];
        for db in h_grid_db {
            let y = self.gain_to_y(
                db,
                Rectangle {
                    x: 0.0,
                    y: 0.0,
                    ..bounds
                },
            );
            let path = Path::line(Point::new(0.0, y), Point::new(bounds.width, y));
            let c = if db == 0.0 {
                Color::from_rgba(0.85, 0.87, 0.90, 0.28)
            } else {
                Color::from_rgba(0.72, 0.76, 0.82, 0.12)
            };
            frame.stroke(
                &path,
                canvas::Stroke::default().with_color(c).with_width(1.0),
            );
            let label = if db == 0.0 {
                "0".to_string()
            } else if db.fract().abs() < 0.01 {
                format!("{db:+.0}")
            } else {
                format!("{db:+.1}")
            };
            frame.fill_text(Text {
                content: label,
                position: Point::new(4.0, y + 2.0),
                color: Color::from_rgba(0.72, 0.76, 0.82, 0.45),
                size: 9.0.into(),
                ..Text::default()
            });
        }

        let v_grid_hz = [
            20.0_f32, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0,
        ];
        for hz in v_grid_hz {
            let x = Self::freq_to_x(
                hz,
                Rectangle {
                    x: 0.0,
                    y: 0.0,
                    ..bounds
                },
            );
            let path = Path::line(Point::new(x, 0.0), Point::new(x, bounds.height));
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(0.72, 0.76, 0.82, 0.10))
                    .with_width(1.0),
            );
            frame.fill_text(Text {
                content: format_freq(hz),
                position: Point::new(x + 3.0, bounds.height - 14.0),
                color: Color::from_rgba(0.72, 0.76, 0.82, 0.45),
                size: 9.0.into(),
                ..Text::default()
            });
        }

        // Piano key markers along the top (Q4-style piano display).
        for midi in 21..=108_i32 {
            let freq = 440.0 * ((midi - 69) as f32 / 12.0).exp2();
            if !(Self::F_MIN..=Self::F_MAX).contains(&freq) {
                continue;
            }
            let x = Self::freq_to_x(
                freq,
                Rectangle {
                    x: 0.0,
                    y: 0.0,
                    ..bounds
                },
            );
            let is_c = midi % 12 == 0;
            let key = Path::line(
                Point::new(x, 0.0),
                Point::new(x, if is_c { 8.0 } else { 5.0 }),
            );
            frame.stroke(
                &key,
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(0.72, 0.76, 0.82, 0.30))
                    .with_width(1.0),
            );
            if is_c {
                frame.fill_text(Text {
                    content: format!("C{}", midi / 12 - 1),
                    position: Point::new(x + 2.0, 9.0),
                    color: Color::from_rgba(0.72, 0.76, 0.82, 0.40),
                    size: 8.0.into(),
                    ..Text::default()
                });
            }
        }

        let mut pre_tilted = self.pre_spectrum_db;
        let mut post_tilted = self.post_spectrum_db;
        if self.tilt != 0.0 {
            for channel in 0..2 {
                for i in 0..SPECTRUM_BINS {
                    let f = self.bin_freq(i);
                    pre_tilted[channel][i] = self.apply_tilt(pre_tilted[channel][i], f);
                    post_tilted[channel][i] = self.apply_tilt(post_tilted[channel][i], f);
                }
            }
        }

        let spectrum_channels = if self.stereo { 2 } else { 1 };
        if self.show_pre {
            for bins in pre_tilted.iter().take(spectrum_channels) {
                let pre_line = self.spectrum_path(bins, local_bounds);
                frame.stroke(
                    &pre_line,
                    canvas::Stroke::default()
                        .with_color(Color::from_rgba(0.72, 0.74, 0.78, 0.24))
                        .with_width(1.0),
                );
            }
        }

        if self.show_post {
            for bins in post_tilted.iter().take(spectrum_channels) {
                let post_fill = self.spectrum_fill_path(bins, local_bounds);
                frame.fill(&post_fill, Color::from_rgba(0.72, 0.74, 0.78, 0.06));
                let post_line = self.spectrum_path(bins, local_bounds);
                frame.stroke(
                    &post_line,
                    canvas::Stroke::default()
                        .with_color(Color::from_rgba(0.72, 0.74, 0.78, 0.32))
                        .with_width(1.2),
                );
            }
        }

        let visible_spectrum_y = |ui_bin: usize| -> Option<f32> {
            let bin = ui_bin.min(SPECTRUM_BINS - 1);
            let mut y: Option<f32> = None;
            if self.show_pre {
                for bins in pre_tilted.iter().take(spectrum_channels) {
                    let next = self.spectrum_to_y(bins[bin], local_bounds);
                    y = Some(y.map_or(next, |current| current.min(next)));
                }
            }
            if self.show_post {
                for bins in post_tilted.iter().take(spectrum_channels) {
                    let next = self.spectrum_to_y(bins[bin], local_bounds);
                    y = Some(y.map_or(next, |current| current.min(next)));
                }
            }
            y
        };

        let band_biquads: Vec<(usize, u8, bool, Vec<Biquad>)> = self
            .bands
            .iter()
            .filter_map(|(idx, f0, gain, q, on, typ, slope, placement)| {
                if !on {
                    return None;
                }
                Some((
                    *idx,
                    *placement,
                    self.band_uses_threshold_dot(*idx, *typ),
                    dsp::build_chain(*typ, *slope, self.sample_rate, *f0, *q, *gain),
                ))
            })
            .collect();

        for (idx, placement, threshold_dot, chain) in &band_biquads {
            if self.listen_mode && Some(*idx) != self.selected_band {
                continue;
            }
            if *threshold_dot {
                continue;
            }
            let is_selected = self.selection.contains(idx);
            let color = placement_color(*placement);
            let band_path = Path::new(|b| {
                let mut first = true;
                for xi in 0..(bounds.width as usize).max(2) {
                    let x = xi as f32;
                    let freq = Self::x_to_freq(
                        x,
                        Rectangle {
                            x: 0.0,
                            y: 0.0,
                            ..bounds
                        },
                    );
                    let band_db: f32 = chain
                        .iter()
                        .map(|bq| bq.magnitude_db(freq, self.sample_rate))
                        .sum();
                    let y = self.gain_to_y(
                        band_db,
                        Rectangle {
                            x: 0.0,
                            y: 0.0,
                            ..bounds
                        },
                    );
                    if first {
                        b.move_to(Point::new(x, y));
                        first = false;
                    } else {
                        b.line_to(Point::new(x, y));
                    }
                }
            });
            frame.stroke(
                &band_path,
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(
                        color.r,
                        color.g,
                        color.b,
                        if is_selected { 0.85 } else { 0.35 },
                    ))
                    .with_width(if is_selected { 1.5 } else { 1.0 }),
            );
        }

        // Ghost curves: the selected peer instance's bands (Instance List).
        for gb in &self.ghost_bands {
            if !gb.on {
                continue;
            }
            let chain =
                dsp::build_chain(gb.typ, gb.slope, self.sample_rate, gb.freq, gb.q, gb.gain);
            let ghost_path = Path::new(|b| {
                let mut first = true;
                for xi in 0..(bounds.width as usize).max(2) {
                    let x = xi as f32;
                    let freq = Self::x_to_freq(
                        x,
                        Rectangle {
                            x: 0.0,
                            y: 0.0,
                            ..bounds
                        },
                    );
                    let band_db: f32 = chain
                        .iter()
                        .map(|bq| bq.magnitude_db(freq, self.sample_rate))
                        .sum();
                    let y = self.gain_to_y(
                        band_db,
                        Rectangle {
                            x: 0.0,
                            y: 0.0,
                            ..bounds
                        },
                    );
                    if first {
                        b.move_to(Point::new(x, y));
                        first = false;
                    } else {
                        b.line_to(Point::new(x, y));
                    }
                }
            });
            frame.stroke(
                &ghost_path,
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(0.75, 0.78, 0.85, 0.30))
                    .with_width(1.0),
            );
        }

        let response = Path::new(|b| {
            let mut first = true;
            for xi in 0..(bounds.width as usize).max(2) {
                let x = xi as f32;
                let freq = Self::x_to_freq(
                    x,
                    Rectangle {
                        x: 0.0,
                        y: 0.0,
                        ..bounds
                    },
                );
                let mut total_lin = 1.0_f32;
                for (idx, _placement, threshold_dot, chain) in &band_biquads {
                    if self.listen_mode && Some(*idx) != self.selected_band {
                        continue;
                    }
                    if *threshold_dot {
                        continue;
                    }
                    for bq in chain {
                        total_lin *= 10.0_f32.powf(bq.magnitude_db(freq, self.sample_rate) * 0.05);
                    }
                }
                let total_db = 20.0 * total_lin.max(1.0e-12).log10();
                let y = self.gain_to_y(
                    total_db,
                    Rectangle {
                        x: 0.0,
                        y: 0.0,
                        ..bounds
                    },
                );
                if first {
                    b.move_to(Point::new(x, y));
                    first = false;
                } else {
                    b.line_to(Point::new(x, y));
                }
            }
        });
        frame.stroke(
            &response,
            canvas::Stroke::default()
                .with_color(Color::from_rgb(0.90, 0.93, 0.96))
                .with_width(2.0),
        );

        if self.last_sketch.len() > 1 {
            let reference = Path::new(|b| {
                let mut first = true;
                for &(f, g) in &self.last_sketch {
                    let x = Self::freq_to_x(
                        f,
                        Rectangle {
                            x: 0.0,
                            y: 0.0,
                            ..bounds
                        },
                    );
                    let y = self.gain_to_y(
                        g,
                        Rectangle {
                            x: 0.0,
                            y: 0.0,
                            ..bounds
                        },
                    );
                    if first {
                        b.move_to(Point::new(x, y));
                        first = false;
                    } else {
                        b.line_to(Point::new(x, y));
                    }
                }
            });
            frame.stroke(
                &reference,
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(1.0, 0.85, 0.3, 0.35))
                    .with_width(1.5),
            );
        }

        if state.sketching && state.sketch_points.len() > 1 {
            let sketch = Path::new(|b| {
                let mut first = true;
                for &(f, g) in &state.sketch_points {
                    let x = Self::freq_to_x(
                        f,
                        Rectangle {
                            x: 0.0,
                            y: 0.0,
                            ..bounds
                        },
                    );
                    let y = self.gain_to_y(
                        g,
                        Rectangle {
                            x: 0.0,
                            y: 0.0,
                            ..bounds
                        },
                    );
                    if first {
                        b.move_to(Point::new(x, y));
                        first = false;
                    } else {
                        b.line_to(Point::new(x, y));
                    }
                }
            });
            frame.stroke(
                &sketch,
                canvas::Stroke::default()
                    .with_color(Color::from_rgb(1.0, 0.85, 0.3))
                    .with_width(2.0),
            );
        }
        if state.dragging.is_none()
            && self.bands.iter().any(|(_, _, _, _, on, _, _, _)| !*on)
            && let Some(hover) = state.hover_pos
        {
            let hover_freq = Self::x_to_freq(hover.x, local_bounds);
            let hover_gain = self.y_to_gain(hover.y, local_bounds);
            let hover_q = if hover_gain >= 0.0 {
                1.0 + (hover_gain / 24.0) * 2.0
            } else {
                1.0 + (hover_gain.abs() / 24.0) * 9.0
            };

            let preview = Path::new(|b| {
                let mut first = true;
                for xi in 0..(bounds.width as usize).max(2) {
                    let x = xi as f32;
                    let freq = Self::x_to_freq(
                        x,
                        Rectangle {
                            x: 0.0,
                            y: 0.0,
                            ..bounds
                        },
                    );
                    let mut total_lin = 1.0_f32;
                    for (_idx, _placement, threshold_dot, chain) in &band_biquads {
                        if *threshold_dot {
                            continue;
                        }
                        for bq in chain {
                            total_lin *=
                                10.0_f32.powf(bq.magnitude_db(freq, self.sample_rate) * 0.05);
                        }
                    }

                    let mut hover_bq = Biquad::default();
                    hover_bq.set_peaking(self.sample_rate, hover_freq, hover_q, hover_gain);
                    total_lin *=
                        10.0_f32.powf(hover_bq.magnitude_db(freq, self.sample_rate) * 0.05);

                    let total_db = 20.0 * total_lin.max(1.0e-12).log10();
                    let y = self.gain_to_y(
                        total_db,
                        Rectangle {
                            x: 0.0,
                            y: 0.0,
                            ..bounds
                        },
                    );
                    if first {
                        b.move_to(Point::new(x, y));
                        first = false;
                    } else {
                        b.line_to(Point::new(x, y));
                    }
                }
            });
            frame.stroke(
                &preview,
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(0.95, 0.95, 0.98, 0.55))
                    .with_width(1.0),
            );
        }

        for (global_idx, freq, gain, q, on, typ, slope, placement) in self.bands.iter() {
            if !*on {
                continue;
            }
            let x = Self::freq_to_x(
                *freq,
                Rectangle {
                    x: 0.0,
                    y: 0.0,
                    ..bounds
                },
            );
            let y = self.gain_to_y(
                *gain,
                Rectangle {
                    x: 0.0,
                    y: 0.0,
                    ..bounds
                },
            );
            let threshold_dot = self.band_uses_threshold_dot(*global_idx, *typ);
            let y = if threshold_dot {
                self.threshold_to_y(
                    self.band_dyn_threshold
                        .get(*global_idx)
                        .copied()
                        .unwrap_or(-24.0),
                    Rectangle {
                        x: 0.0,
                        y: 0.0,
                        ..bounds
                    },
                )
            } else {
                y
            };
            let is_primary = Some(*global_idx) == self.selected_band;
            let is_selected = self.selection.contains(global_idx);
            let hovered = state
                .hover_pos
                .map(|hp| (hp.x - x).powi(2) + (hp.y - y).powi(2) < 100.0)
                .unwrap_or(false);
            let collision = self
                .collision_scores
                .get(*global_idx)
                .copied()
                .unwrap_or(0.0);
            let color = placement_color(*placement);
            let center = Point::new(x, y);
            let radius = if is_primary {
                7.0
            } else if is_selected {
                5.5
            } else {
                4.5
            };
            let node = Path::circle(center, radius);

            if threshold_dot {
                let threshold_gain = self.y_to_gain(y, local_bounds).clamp(-24.0, 24.0);
                let threshold_chain = dsp::build_chain(
                    dsp::SHAPE_BELL,
                    *slope,
                    self.sample_rate,
                    *freq,
                    *q,
                    threshold_gain,
                );
                let target_gain = *gain;
                let target_chain = dsp::build_chain(
                    dsp::SHAPE_BELL,
                    *slope,
                    self.sample_rate,
                    *freq,
                    *q,
                    target_gain,
                );
                let threshold_path = Path::new(|b| {
                    let mut first = true;
                    for xi in 0..(bounds.width as usize).max(2) {
                        let curve_x = xi as f32;
                        let ui_bin = ((curve_x / bounds.width.max(1.0))
                            * (SPECTRUM_BINS.saturating_sub(1) as f32))
                            .round()
                            .clamp(0.0, SPECTRUM_BINS.saturating_sub(1) as f32)
                            as usize;
                        let curve_freq = Self::x_to_freq(
                            curve_x,
                            Rectangle {
                                x: 0.0,
                                y: 0.0,
                                ..bounds
                            },
                        );
                        let band_db: f32 = threshold_chain
                            .iter()
                            .map(|bq| bq.magnitude_db(curve_freq, self.sample_rate))
                            .sum();
                        let target_db: f32 = target_chain
                            .iter()
                            .map(|bq| bq.magnitude_db(curve_freq, self.sample_rate))
                            .sum();
                        let threshold_y = self.gain_to_y(
                            band_db,
                            Rectangle {
                                x: 0.0,
                                y: 0.0,
                                ..bounds
                            },
                        );
                        let curve_y =
                            visible_spectrum_y(ui_bin).map_or(threshold_y, |spectrum_y| {
                                if spectrum_y >= threshold_y {
                                    return threshold_y;
                                }
                                self.gain_to_y(
                                    band_db + target_db,
                                    Rectangle {
                                        x: 0.0,
                                        y: 0.0,
                                        ..bounds
                                    },
                                )
                            });
                        if first {
                            b.move_to(Point::new(curve_x, curve_y));
                            first = false;
                        } else {
                            b.line_to(Point::new(curve_x, curve_y));
                        }
                    }
                });
                frame.stroke(
                    &threshold_path,
                    canvas::Stroke::default()
                        .with_color(Color::from_rgba(0.90, 0.93, 0.96, 0.90))
                        .with_width(1.5),
                );
            }

            frame.fill(&node, color);
            frame.stroke(
                &node,
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(0.95, 0.95, 0.98, 0.85))
                    .with_width(1.5),
            );

            let dynamic_dragging_this = matches!(
                state.dragging,
                Some(DragTarget::DynamicTarget(local_idx))
                    if self
                        .bands
                        .get(local_idx)
                        .map(|(idx, _, _, _, _, _, _, _)| idx == global_idx)
                        .unwrap_or(false)
            );
            if threshold_dot || dynamic_dragging_this {
                let target_y = if dynamic_dragging_this {
                    state.drag_last_pos.map(|p| p.y).unwrap_or_else(|| {
                        self.gain_to_y(
                            *gain,
                            Rectangle {
                                x: 0.0,
                                y: 0.0,
                                ..bounds
                            },
                        )
                    })
                } else {
                    self.gain_to_y(
                        *gain,
                        Rectangle {
                            x: 0.0,
                            y: 0.0,
                            ..bounds
                        },
                    )
                };
                let target_gain = self.y_to_gain(target_y, local_bounds).clamp(-24.0, 24.0);
                let dynamic_chain = dsp::build_chain(
                    dsp::SHAPE_BELL,
                    *slope,
                    self.sample_rate,
                    *freq,
                    *q,
                    target_gain,
                );
                let dynamic_path = Path::new(|b| {
                    let mut first = true;
                    for xi in 0..(bounds.width as usize).max(2) {
                        let curve_x = xi as f32;
                        let curve_freq = Self::x_to_freq(
                            curve_x,
                            Rectangle {
                                x: 0.0,
                                y: 0.0,
                                ..bounds
                            },
                        );
                        let band_db: f32 = dynamic_chain
                            .iter()
                            .map(|bq| bq.magnitude_db(curve_freq, self.sample_rate))
                            .sum();
                        let curve_y = self.gain_to_y(
                            band_db,
                            Rectangle {
                                x: 0.0,
                                y: 0.0,
                                ..bounds
                            },
                        );
                        if first {
                            b.move_to(Point::new(curve_x, curve_y));
                            first = false;
                        } else {
                            b.line_to(Point::new(curve_x, curve_y));
                        }
                    }
                });
                frame.stroke(
                    &dynamic_path,
                    canvas::Stroke::default()
                        .with_color(Color::from_rgba(
                            color.r,
                            color.g,
                            color.b,
                            if is_selected { 0.42 } else { 0.20 },
                        ))
                        .with_width(if is_selected { 1.5 } else { 1.0 }),
                );
                let dynamic_center = Point::new(x, target_y);
                let dynamic_node = Path::circle(dynamic_center, radius);
                frame.fill(
                    &dynamic_node,
                    Color::from_rgba(color.r, color.g, color.b, 0.42),
                );
                frame.stroke(
                    &dynamic_node,
                    canvas::Stroke::default()
                        .with_color(Color::from_rgba(color.r, color.g, color.b, 0.18))
                        .with_width(1.0),
                );
            }

            let gr = self.band_gr_db.get(*global_idx).copied().unwrap_or(0.0);
            if gr.abs() > 0.1 {
                let frac = (gr.abs() / 24.0).min(1.0);
                let start = -std::f32::consts::FRAC_PI_2;
                let end = start + frac * std::f32::consts::TAU;
                let ring = Path::new(|b| {
                    const SEGMENTS: usize = 32;
                    for i in 0..=SEGMENTS {
                        let a = start + (end - start) * i as f32 / SEGMENTS as f32;
                        let p = Point::new(x + 9.5 * a.cos(), y + 9.5 * a.sin());
                        if i == 0 {
                            b.move_to(p);
                        } else {
                            b.line_to(p);
                        }
                    }
                });
                let ring_color = if gr < 0.0 {
                    Color::from_rgb(1.0, 0.35, 0.30)
                } else {
                    Color::from_rgb(0.35, 1.0, 0.45)
                };
                frame.stroke(
                    &ring,
                    canvas::Stroke::default()
                        .with_color(ring_color)
                        .with_width(2.0),
                );
            }

            if collision > 0.05 {
                frame.stroke(
                    &node,
                    canvas::Stroke::default()
                        .with_color(Color::from_rgb(
                            1.0,
                            0.2 * (1.0 - collision),
                            0.2 * (1.0 - collision),
                        ))
                        .with_width(1.0 + collision * 2.0),
                );
            } else if is_primary {
                frame.stroke(
                    &node,
                    canvas::Stroke::default()
                        .with_color(Color::from_rgb(0.95, 0.95, 0.98))
                        .with_width(1.5),
                );
            } else if is_selected {
                frame.stroke(
                    &node,
                    canvas::Stroke::default()
                        .with_color(Color::from_rgba(0.95, 0.95, 0.98, 0.6))
                        .with_width(1.0),
                );
            } else {
                frame.stroke(
                    &node,
                    canvas::Stroke::default()
                        .with_color(Color::from_rgb(0.16, 0.16, 0.18))
                        .with_width(1.0),
                );
            }

            if is_primary || hovered {
                let note = freq_to_note(*freq);
                let label = if note.is_empty() {
                    format!("{} {:+.1} dB", format_freq(*freq), *gain)
                } else {
                    format!("{} ({}) {:+.1} dB", format_freq(*freq), note, *gain)
                };
                let label_dot_y = if threshold_dot {
                    self.gain_to_y(
                        *gain,
                        Rectangle {
                            x: 0.0,
                            y: 0.0,
                            ..bounds
                        },
                    )
                } else {
                    y
                };
                let label_x = (x - 22.0).clamp(0.0, (bounds.width - 64.0).max(0.0));
                let label_y = (label_dot_y - 14.0).max(10.0);
                frame.fill_text(Text {
                    content: label,
                    position: Point::new(label_x, label_y),
                    color: Color::from_rgb(0.95, 0.95, 0.98),
                    size: 10.0.into(),
                    ..Text::default()
                });
                if threshold_dot {
                    let threshold = self
                        .band_dyn_threshold
                        .get(*global_idx)
                        .copied()
                        .unwrap_or(-24.0);
                    let threshold_label = if note.is_empty() {
                        format!("{} {:+.1} dB", format_freq(*freq), threshold)
                    } else {
                        format!("{} ({}) {:+.1} dB", format_freq(*freq), note, threshold)
                    };
                    let threshold_label_x = (x + 10.0).clamp(0.0, (bounds.width - 64.0).max(0.0));
                    let threshold_label_y = (y - 14.0).max(10.0);
                    frame.fill_text(Text {
                        content: threshold_label,
                        position: Point::new(threshold_label_x, threshold_label_y),
                        color: Color::from_rgba(0.95, 0.95, 0.98, 0.78),
                        size: 10.0.into(),
                        ..Text::default()
                    });
                }
            }
        }

        vec![frame.into_geometry()]
    }
}

fn eq_response_graph(graph: EqResponseCanvas) -> Element<'static, Message> {
    canvas(graph)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn theme(_state: &State) -> Theme {
    Theme::TokyoNight
}

fn knob(
    label: String,
    id: ParamId,
    value: f32,
    units: &'static str,
    step: f32,
) -> Element<'static, Message> {
    let def = PARAMS[id.as_index()];
    let value_text = if units.is_empty() {
        format!("{value:.2}")
    } else if units == "Hz" {
        format!("{value:.0} {units}")
    } else {
        format!("{value:.1} {units}")
    };

    small_knob(
        SmallKnob {
            label,
            value,
            range: def.min as f32..=def.max as f32,
            default: def.default as f32,
            step,
            value_text,
        },
        move |v| Message::SetParam(id, v),
        Message::ReleaseParam(id),
    )
}

fn gain_slider(
    id: ParamId,
    value: f32,
    units: &'static str,
    step: f32,
) -> Element<'static, Message> {
    let def = PARAMS[id.as_index()];
    let value_text = if units.is_empty() {
        format!("{value:.2}")
    } else {
        format!("{value:.1} {units}")
    };

    vertical_slider(
        VerticalSlider {
            value,
            range: def.min as f32..=def.max as f32,
            default: def.default as f32,
            step,
            value_text,
        },
        move |v| Message::SetParam(id, v),
        Message::ReleaseParam(id),
    )
}

fn freq_to_norm(freq_hz: f32) -> f32 {
    let f_min = 20.0_f32;
    let f_mid = 1000.0_f32;
    let f_max = 20_000.0_f32;
    let f = freq_hz.max(f_min).min(f_max);
    if f <= f_mid {
        0.5 * ((f / f_min).ln() / (f_mid / f_min).ln())
    } else {
        0.5 + 0.5 * ((f / f_mid).ln() / (f_max / f_mid).ln())
    }
}

fn norm_to_freq(norm: f32) -> f32 {
    let f_min = 20.0_f32;
    let f_mid = 1000.0_f32;
    let f_max = 20_000.0_f32;
    let t = norm.clamp(0.0, 1.0);
    if t <= 0.5 {
        f_min * (f_mid / f_min).powf(t / 0.5)
    } else {
        f_mid * (f_max / f_mid).powf((t - 0.5) / 0.5)
    }
}

fn format_freq(freq_hz: f32) -> String {
    if freq_hz >= 1000.0 {
        format!("{:.2}k", freq_hz / 1000.0)
    } else {
        format!("{freq_hz:.0}")
    }
}

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

#[derive(Debug, Clone, Copy, Default)]
struct BandSnapshot {
    freq: f32,
    gain: f32,
    q: f32,
    typ: u8,
    slope: u8,
    placement: u8,
    dyn_on: bool,
    dyn_threshold: f32,
    dyn_ratio: f32,
    dyn_knee: f32,
    dyn_range: f32,
    dyn_attack: f32,
    dyn_release: f32,
    dyn_source: f32,
    dyn_mode: f32,
}

/// Process-local band clipboard for copying bands between EQ instances
/// (covers the common single-process DAW case; cross-process copy would
/// need a billboard-bus slot).
static BAND_CLIPBOARD: std::sync::LazyLock<parking_lot::Mutex<Vec<BandSnapshot>>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(Vec::new()));

fn p_of(params: &crate::eq::params::ParamStore<ParamId>, id: ParamId) -> f32 {
    params.get(id) as f32
}

fn band_param_index(id: ParamId, first: usize, stride: usize, count: usize) -> Option<usize> {
    let idx = id.as_index();
    if idx < first || idx >= first + stride * count {
        return None;
    }
    let offset = idx - first;
    offset.is_multiple_of(stride).then_some(offset / stride)
}

fn gain_param_band(id: ParamId) -> Option<usize> {
    band_param_index(id, ParamId::para_gain(0).as_index(), 3, 32)
}

fn dyn_param_band(id: ParamId) -> Option<usize> {
    band_param_index(id, ParamId::para_dyn(0).as_index(), 1, 32)
}

/// Creates bands from a fitted shape list (EQ Sketch and EQ Match share
/// this), filling free band slots. Uses the same direct-store mechanism as
/// double-click creation (proven against host echo behavior) and marks the
/// session dirty so the change is picked up by state save.
fn create_fitted_bands(state: &mut State, fitted: &[(u8, f32, f32, f32, u8)]) {
    for &(shape, freq, gain, q, slope) in fitted {
        for i in 0..32 {
            if !state.shared.params.get_bool(ParamId::para_on(i)) {
                state
                    .shared
                    .set_param_outbound_only(ParamId::para_on(i), 1.0);
                state
                    .shared
                    .set_param_outbound_only(ParamId::para_freq(i), freq as f64);
                state
                    .shared
                    .set_param_outbound_only(ParamId::para_gain(i), gain as f64);
                state
                    .shared
                    .set_param_outbound_only(ParamId::para_q(i), q as f64);
                state
                    .shared
                    .set_param_outbound_only(ParamId::para_type(i), shape as f64);
                state
                    .shared
                    .set_param_outbound_only(ParamId::para_slope(i), slope as f64);
                state.selected_band = Some(i);
                state.selection.clear();
                state.selection.insert(i);
                break;
            }
        }
    }
    state.shared.mark_dirty();
    state.last_registry_version = 0;
}

const FIT_N: usize = 64;

/// How to treat sustained edge levels when fitting a curve.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EdgeFit {
    /// Hand-drawn sketches: deep falloffs become cuts, plateaus shelves.
    Sketch,
    /// EQ Match targets: sustained edge deviations become shelves.
    Match,
}

fn fit_freq_of(i: usize) -> f32 {
    20.0_f32 * (1_000.0_f32).powf(i as f32 / (FIT_N - 1) as f32)
}

/// Response of a fitted band at `freq` in dB (uses the real DSP chains).
fn band_response_db(
    sample_rate: f32,
    shape: u8,
    slope: u8,
    freq0: f32,
    q: f32,
    gain: f32,
    freq: f32,
) -> f32 {
    dsp::build_chain(shape, slope, sample_rate, freq0, q, gain)
        .iter()
        .map(|b| b.magnitude_db(freq, sample_rate))
        .sum()
}

/// Matching-pursuit curve fitter: repeatedly places a band at the point of
/// largest remaining deviation and subtracts its response, so the aggregate
/// result actually tracks the target curve instead of a trend-reduced
/// residual. Handles edge cuts/shelves first, then up to six bells whose Q is
/// estimated from the feature width.
fn fit_curve_bands(
    curve_db: &[f32; FIT_N],
    sample_rate: f32,
    edges: EdgeFit,
) -> Vec<(u8, f32, f32, f32, u8)> {
    let mut remaining = *curve_db;
    let mut out: Vec<(u8, f32, f32, f32, u8)> = Vec::new();

    let lo_edge: f32 = remaining[..5].iter().sum::<f32>() / 5.0;
    let hi_edge: f32 = remaining[FIT_N - 5..].iter().sum::<f32>() / 5.0;

    let subtract = |remaining: &mut [f32; FIT_N], band: (u8, f32, f32, f32, u8)| {
        let (shape, freq0, gain, q, slope) = band;
        for (i, r) in remaining.iter_mut().enumerate() {
            *r -= band_response_db(sample_rate, shape, slope, freq0, q, gain, fit_freq_of(i));
        }
    };

    // --- Low edge ---
    if edges == EdgeFit::Sketch && lo_edge < -3.5 {
        let mut cut_i = 5;
        for (i, r) in remaining.iter().enumerate().take(10) {
            if *r > -1.75 {
                cut_i = i.max(1);
                break;
            }
        }
        let band = (dsp::SHAPE_LOW_CUT, fit_freq_of(cut_i), 0.0, 0.707, 1);
        out.push(band);
        subtract(&mut remaining, band);
    } else if lo_edge.abs() > 2.5 {
        let band = (
            dsp::SHAPE_LOW_SHELF,
            150.0,
            lo_edge.clamp(-15.0, 15.0),
            0.707,
            0,
        );
        out.push(band);
        subtract(&mut remaining, band);
    }

    // --- High edge ---
    if edges == EdgeFit::Sketch && hi_edge < -3.5 {
        let mut cut_i = FIT_N - 6;
        for i in (FIT_N - 10..FIT_N).rev() {
            if remaining[i] > -1.75 {
                cut_i = i.min(FIT_N - 2);
                break;
            }
        }
        let band = (dsp::SHAPE_HIGH_CUT, fit_freq_of(cut_i), 0.0, 0.707, 1);
        out.push(band);
        subtract(&mut remaining, band);
    } else if hi_edge.abs() > 2.5 {
        let band = (
            dsp::SHAPE_HIGH_SHELF,
            8_000.0,
            hi_edge.clamp(-15.0, 15.0),
            0.707,
            0,
        );
        out.push(band);
        subtract(&mut remaining, band);
    }

    // --- Bell pursuit: 0.4 dB floor; spikes narrower than 3 bins at half
    // prominence are treated as mouse jitter and skipped, so deliberate
    // small ripples are still detected.
    for _ in 0..8 {
        let (mut best_i, mut best_v) = (2usize, 0.0_f32);
        for (i, r) in remaining.iter().enumerate().take(FIT_N - 2).skip(2) {
            if r.abs() > best_v.abs() {
                best_v = *r;
                best_i = i;
            }
        }
        if best_v.abs() < 0.4 {
            break;
        }
        // Feature width at half prominence, in octaves → bell Q.
        let half = best_v.abs() * 0.5;
        let mut left = best_i;
        while left > 0 && remaining[left].abs() > half {
            left -= 1;
        }
        let mut right = best_i;
        while right < FIT_N - 1 && remaining[right].abs() > half {
            right += 1;
        }
        if right - left > FIT_N / 2 {
            // Dominant deviation spans most of the spectrum: it is a
            // broadband offset/tilt, not a bell feature. Stop here — the
            // edge shelves/cuts already cover that case.
            break;
        }
        if right - left < 3 {
            // Jitter spike: erase it locally and keep looking.
            for r in &mut remaining[left..=right] {
                *r = 0.0;
            }
            continue;
        }
        let width_oct = (right - left) as f32 / (FIT_N - 1) as f32 * (1_000.0_f32).log2();
        let q = (1.44 / width_oct.max(0.2)).clamp(0.6, 8.0);
        let gain = best_v.clamp(-18.0, 18.0);
        let band = (dsp::SHAPE_BELL, fit_freq_of(best_i), gain, q, 0);
        out.push(band);
        subtract(&mut remaining, band);
    }

    // Zero-crossing anchors: EVERY zero crossing next to a peak over 2 dB
    // gets its own band as an editable handle, with gain set to the
    // corrective amount at that point (zero if the fit already crosses).
    let anchors_start = out.len();
    let mut anchors = 0;
    let mut last_anchor = usize::MAX;
    for i in 3..FIT_N - 3 {
        if anchors >= 4 {
            break;
        }
        let a = curve_db[i];
        let b = curve_db[i + 1];
        let sign_change = (a > 0.0) != (b > 0.0);
        let near_zero = a.abs() < 1.0 || b.abs() < 1.0;
        if !sign_change || !near_zero {
            continue;
        }
        // Only anchor crossings between meaningful features (>= 2 dB nearby).
        let context = curve_db[i.saturating_sub(8)..(i + 8).min(FIT_N)]
            .iter()
            .fold(0.0_f32, |m, v| m.max(v.abs()));
        if context < 2.0 {
            continue;
        }
        if last_anchor != usize::MAX && i.abs_diff(last_anchor) < 4 {
            continue;
        }
        let total: f32 = out
            .iter()
            .map(|&(s2, f2, g2, q2, sl2)| {
                band_response_db(sample_rate, s2, sl2, f2, q2, g2, fit_freq_of(i))
            })
            .sum();
        let band = (
            dsp::SHAPE_BELL,
            fit_freq_of(i),
            (-total).clamp(-6.0, 6.0),
            1.2,
            0,
        );
        out.push(band);
        subtract(&mut remaining, band);
        last_anchor = i;
        anchors += 1;
    }

    // Refinement: two Gauss-Seidel sweeps re-adjusting each band's gain by
    // least-squares projection against the ORIGINAL curve (bell skirts
    // overlap, so the one-pass pursuit lands slightly shallow).
    let target = curve_db;
    for _ in 0..2 {
        for band_idx in 0..out.len() {
            let (shape, f0, gain, q, slope) = out[band_idx];
            let f_lo_reg = f0 / 1.5;
            let f_hi_reg = f0 * 1.5;
            let mut num = 0.0_f32;
            let mut den = 0.0_f32;
            for (i, &t) in target.iter().enumerate() {
                let freq = fit_freq_of(i);
                if !(f_lo_reg..=f_hi_reg).contains(&freq) {
                    continue;
                }
                let total: f32 = out
                    .iter()
                    .map(|&(s2, f2, g2, q2, sl2)| {
                        band_response_db(sample_rate, s2, sl2, f2, q2, g2, freq)
                    })
                    .sum();
                let own = band_response_db(sample_rate, shape, slope, f0, q, 1.0, freq);
                let err = t - total;
                num += own * err;
                den += own * own;
            }
            if den > 1.0e-6 {
                let adjust = (num / den).clamp(-6.0, 6.0);
                let new_gain = (gain + adjust).clamp(-24.0, 24.0);
                if (new_gain - gain).abs() > 0.05 {
                    out[band_idx] = (shape, f0, new_gain, q, slope);
                }
            }
        }
    }
    // Bands that refined to nothing only clutter the list — except
    // zero-crossing anchors, which stay as editable handles by design.
    let mut idx = 0;
    out.retain(|band| {
        let keep = idx >= anchors_start
            || band.2.abs() > 0.3
            || matches!(band.0, dsp::SHAPE_LOW_CUT | dsp::SHAPE_HIGH_CUT);
        idx += 1;
        keep
    });
    out
}

/// EQ Match: resamples the peer/own spectrum difference onto the fit grid
/// and fits shelves at the edges plus pursuit bells.
fn fit_match_bands(diff: &[f32; SPECTRUM_BINS], sample_rate: f32) -> Vec<(u8, f32, f32, f32, u8)> {
    let mut curve = [0.0_f32; FIT_N];
    for (i, c) in curve.iter_mut().enumerate() {
        let src = i as f32 / (FIT_N - 1) as f32 * (SPECTRUM_BINS - 1) as f32;
        let i0 = src.floor() as usize;
        let i1 = (i0 + 1).min(SPECTRUM_BINS - 1);
        let u = src - i0 as f32;
        *c = diff[i0] * (1.0 - u) + diff[i1] * u;
    }
    fit_curve_bands(&curve, sample_rate, EdgeFit::Match)
}

/// EQ Sketch: resamples the hand-drawn polyline onto the fit grid, smooths
/// out mouse jitter, and fits edge cuts/shelves plus pursuit bells.
fn fit_sketch_bands(points: &[(f32, f32)], sample_rate: f32) -> Vec<(u8, f32, f32, f32, u8)> {
    if points.len() < 4 {
        return Vec::new();
    }
    let mut pts: Vec<(f32, f32)> = points.to_vec();
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let _f_lo = 20.0_f32;
    let _f_hi = 20_000.0_f32;
    let interp = |target: f32| -> f32 {
        let mut idx = 0;
        while idx + 1 < pts.len() && pts[idx + 1].0 < target {
            idx += 1;
        }
        if idx + 1 >= pts.len() {
            return pts[pts.len() - 1].1;
        }
        let (f0, g0) = pts[idx];
        let (f1, g1) = pts[idx + 1];
        if f1 <= f0 {
            return g0;
        }
        let u = ((target / f0).ln() / (f1 / f0).ln()).clamp(0.0, 1.0);
        g0 + u * (g1 - g0)
    };

    let mut curve = [0.0_f32; FIT_N];
    for (i, c) in curve.iter_mut().enumerate() {
        *c = interp(fit_freq_of(i));
    }
    let mut smooth = curve;
    for i in 1..FIT_N - 1 {
        smooth[i] = (curve[i - 1] + 2.0 * curve[i] + curve[i + 1]) * 0.25;
    }
    fit_curve_bands(&smooth, sample_rate, EdgeFit::Sketch)
}

fn freq_to_note(freq: f32) -> String {
    if freq < 8.0 {
        return String::new();
    }
    let midi = (69.0 + 12.0 * (freq / 440.0).log2()).round() as i32;
    if !(0..=127).contains(&midi) {
        return String::new();
    }
    format!("{}{}", NOTE_NAMES[(midi % 12) as usize], midi / 12 - 1)
}

fn freq_knob(id: ParamId, value_hz: f32) -> Element<'static, Message> {
    let def = PARAMS[id.as_index()];
    let value_norm = freq_to_norm(value_hz);
    let default_norm = freq_to_norm(def.default as f32);
    let slider = arch_slider(0.0_f32..=1.0_f32, value_norm, move |n| {
        Message::SetParam(id, norm_to_freq(n))
    })
    .double_click_reset(default_norm)
    .on_release(Message::ReleaseParam(id))
    .fill_from_start()
    .width(Length::Fixed(41.0))
    .height(Length::Fixed(41.0));

    container(
        column![text("Freq").size(11), slider, text("").size(10)]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(50.0))
    .into()
}

fn build_app(shared: Arc<SharedState<ParamId>>) -> impl maolan_baseview::iced::Program {
    maolan_baseview::iced::application(move || init(shared.clone()), update, view)
        .font(iced_fonts::LUCIDE_FONT_BYTES)
        .theme(theme)
        .run()
}

pub struct GuiBridge {
    created: bool,
    floating: bool,
    shared: Option<Arc<SharedState<ParamId>>>,
    floating_open: Arc<AtomicBool>,
    window_handle: Option<AnyWindowHandle>,
}

impl Default for GuiBridge {
    fn default() -> Self {
        Self {
            created: false,
            floating: false,
            shared: None,
            floating_open: Arc::new(AtomicBool::new(false)),
            window_handle: None,
        }
    }
}

impl GuiBridge {
    pub fn create(
        &mut self,
        shared: Arc<SharedState<ParamId>>,
        api: &CStr,
        is_floating: bool,
    ) -> bool {
        if !is_api_supported(api, is_floating) {
            return false;
        }
        self.created = true;
        self.floating = is_floating;
        self.shared = Some(shared);
        true
    }

    pub fn destroy(&mut self) {
        if let Some(shared) = &self.shared {
            shared.set_ui_visible(false);
        }
        self.window_handle = None;
        self.shared = None;
        self.floating = false;
        self.created = false;
    }

    pub fn set_parent(
        &mut self,
        shared: Arc<SharedState<ParamId>>,
        parent: ParentWindowHandle,
    ) -> bool {
        if !self.created {
            return false;
        }
        if self.floating {
            self.shared = Some(shared);
            return true;
        }
        shared.set_ui_visible(true);

        let settings = maolan_baseview::iced::IcedBaseviewSettings {
            window: maolan_baseview::iced::baseview::WindowOpenOptions {
                title: String::from("Maolan EQ"),
                size: maolan_baseview::iced::baseview::Size::new(
                    EDITOR_WIDTH as f64,
                    EDITOR_HEIGHT as f64,
                ),
                scale: maolan_baseview::iced::baseview::WindowScalePolicy::SystemScaleFactor,
            },
            ignore_non_modifier_keys: false,
            always_redraw: true,
        };

        let handle = maolan_baseview::iced::shell::open_parented(
            &parent,
            settings,
            maolan_baseview::iced::PollSubNotifier::new(),
            move || build_app(shared),
        );

        self.window_handle = Some(AnyWindowHandle {
            _inner: Box::new(handle),
        });
        true
    }

    pub fn show(&mut self) -> bool {
        if !self.created {
            return false;
        }
        if self.floating {
            if self.floating_open.swap(true, Ordering::AcqRel) {
                return true;
            }
            let Some(shared) = self.shared.clone() else {
                self.floating_open.store(false, Ordering::Release);
                return false;
            };
            shared.set_ui_visible(true);
            let open_flag = self.floating_open.clone();
            thread::spawn(move || {
                let shared_for_close = shared.clone();
                let settings = maolan_baseview::iced::IcedBaseviewSettings {
                    window: maolan_baseview::iced::baseview::WindowOpenOptions {
                        title: String::from("Maolan EQ"),
                        size: maolan_baseview::iced::baseview::Size::new(
                            EDITOR_WIDTH as f64,
                            EDITOR_HEIGHT as f64,
                        ),
                        scale:
                            maolan_baseview::iced::baseview::WindowScalePolicy::SystemScaleFactor,
                    },
                    ignore_non_modifier_keys: false,
                    always_redraw: true,
                };
                maolan_baseview::iced::shell::open_blocking(
                    settings,
                    maolan_baseview::iced::PollSubNotifier::new(),
                    move || build_app(shared),
                );
                open_flag.store(false, Ordering::Release);
                shared_for_close.set_ui_visible(false);
            });
        }
        true
    }

    pub fn hide(&mut self, shared: Arc<SharedState<ParamId>>) -> (bool, bool) {
        shared.set_ui_visible(false);
        if self.floating {
            self.floating_open.store(false, Ordering::Release);
            return (true, true);
        }
        self.window_handle = None;
        (true, false)
    }
}
#[cfg(target_os = "windows")]
use clap_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(unix)]
use clap_clap::ffi::CLAP_WINDOW_API_X11;
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};

pub fn preferred_api() -> &'static CStr {
    #[cfg(target_os = "windows")]
    {
        CLAP_WINDOW_API_WIN32
    }
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        CLAP_WINDOW_API_X11
    }
}

pub fn is_api_supported(api: &CStr, _is_floating: bool) -> bool {
    api == preferred_api()
}

pub enum ParentWindowHandle {
    #[cfg(unix)]
    X11(u64),
    #[cfg(target_os = "windows")]
    Win32(*mut std::ffi::c_void),
}

impl HasWindowHandle for ParentWindowHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        match self {
            #[cfg(unix)]
            ParentWindowHandle::X11(window) => {
                let handle = raw_window_handle::XlibWindowHandle::new(*window);
                Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Xlib(handle)) })
            }
            #[cfg(target_os = "windows")]
            ParentWindowHandle::Win32(hwnd) => {
                let handle = raw_window_handle::Win32WindowHandle::new(
                    std::num::NonZeroIsize::new(*hwnd as isize).unwrap(),
                );
                Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
            }
        }
    }
}

pub struct AnyWindowHandle {
    pub _inner: Box<dyn std::any::Any>,
}

unsafe impl Send for AnyWindowHandle {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eq::params::ParamStore;

    fn smooth_curve(center_freq: f32, depth: f32) -> Vec<(f32, f32)> {
        // A smooth, broad dip (Gaussian, ~1 octave wide) like a hand-drawn one.
        (0..80)
            .map(|i| {
                let t = i as f32 / 79.0;
                let freq = 20.0 * (20_000.0_f32 / 20.0).powf(t);
                let x = (freq / center_freq).log2();
                let gain = depth * (-x * x * 4.0_f32).exp();
                (freq, gain)
            })
            .collect()
    }

    #[test]
    fn sketch_smooth_broad_dip_fits_band() {
        let points = smooth_curve(1_000.0, -8.0);
        let fitted = fit_sketch_bands(&points, 48_000.0);
        assert!(
            fitted
                .iter()
                .any(|(shape, _, gain, _, _)| *shape == dsp::SHAPE_BELL && *gain < -2.0),
            "smooth broad dip produced no bell: {fitted:?}"
        );
    }

    #[test]
    fn sketch_smooth_broad_boost_fits_band() {
        let points = smooth_curve(2_000.0, 9.0);
        let fitted = fit_sketch_bands(&points, 48_000.0);
        assert!(
            fitted
                .iter()
                .any(|(shape, _, gain, _, _)| *shape == dsp::SHAPE_BELL && *gain > 2.0),
            "smooth broad boost produced no bell: {fitted:?}"
        );
    }

    #[test]
    fn sketch_high_shelf_shape_fits_shelf() {
        // Flat then rising to a sustained high plateau: should become a shelf,
        // not silently nothing.
        let points: Vec<(f32, f32)> = (0..80)
            .map(|i| {
                let t = i as f32 / 79.0;
                let freq = 20.0 * (20_000.0_f32 / 20.0).powf(t);
                let gain = if freq > 3_000.0 { 8.0 } else { 0.0 };
                (freq, gain)
            })
            .collect();
        let fitted = fit_sketch_bands(&points, 48_000.0);
        assert!(!fitted.is_empty(), "high shelf shape produced nothing");
    }

    #[test]
    fn apply_sketch_creates_bands_in_params() {
        let shared = Arc::new(SharedState::new(
            ParamStore::new(&PARAMS),
            std::ptr::null(),
            2,
        ));
        let (mut state, _task) = init(shared.clone());
        let points = smooth_curve(1_000.0, -8.0);
        let _ = update(&mut state, Message::ApplySketch(points));
        let created: Vec<usize> = (0..32)
            .filter(|&i| shared.params.get_bool(ParamId::para_on(i)))
            .collect();
        assert!(!created.is_empty(), "ApplySketch created no bands");
        let gain = shared.params.get(ParamId::para_gain(created[0]));
        assert!(gain < -1.0, "created band should be a cut, got {gain} dB");
    }

    #[test]
    fn sketch_jittery_shallow_dip_still_fits() {
        // Realistic mouse scribble: shallow -4 dB dip with +-1 dB jitter.
        let mut points = Vec::new();
        for i in 0..200 {
            let t = i as f32 / 199.0;
            let freq = 20.0 * (20_000.0_f32 / 20.0).powf(t);
            let x = (freq / 800.0).log2();
            let dip = -4.0 * (-x * x * 4.0_f32).exp();
            let jitter = ((i * 37) % 11) as f32 / 11.0 * 2.0 - 1.0;
            points.push((freq, dip + jitter));
        }
        let fitted = fit_sketch_bands(&points, 48_000.0);
        assert!(
            fitted
                .iter()
                .any(|(shape, _, gain, _, _)| *shape == dsp::SHAPE_BELL && *gain < -1.0),
            "jittery shallow dip produced no bell: {fitted:?}"
        );
    }

    #[test]
    fn sketch_flat_curve_yields_nothing() {
        // A flat line at ~0 dB means "no EQ" and must stay band-free.
        let points: Vec<(f32, f32)> = (0..50)
            .map(|i| {
                let t = i as f32 / 49.0;
                (20.0 * (20_000.0_f32 / 20.0).powf(t), 0.2)
            })
            .collect();
        assert!(fit_sketch_bands(&points, 48_000.0).is_empty());
    }

    #[test]
    fn sketch_shelf_up_produces_band() {
        // Sustained high plateau: must produce *something* (shelf or bell).
        let points: Vec<(f32, f32)> = (0..80)
            .map(|i| {
                let t = i as f32 / 79.0;
                let freq = 20.0 * (20_000.0_f32 / 20.0).powf(t);
                (freq, if freq > 2_500.0 { 7.0 } else { 0.0 })
            })
            .collect();
        assert!(
            !fit_sketch_bands(&points, 48_000.0).is_empty(),
            "shelf-up sketch produced nothing"
        );
    }

    #[test]
    fn sketch_with_dip_fits_bell_band() {
        // Hand-draw a curve that is flat except for a dip around 1 kHz.
        let mut points = Vec::new();
        for i in 0..40 {
            let t = i as f32 / 39.0;
            let freq = 20.0 * (20_000.0_f32 / 20.0).powf(t);
            let center = (freq / 1_000.0).log2();
            let gain = if center.abs() < 0.35 { -10.0 } else { 0.0 };
            points.push((freq, gain));
        }
        let fitted = fit_sketch_bands(&points, 48_000.0);
        let bells: Vec<_> = fitted
            .iter()
            .filter(|(shape, _, gain, _, _)| *shape == dsp::SHAPE_BELL && *gain < -4.0)
            .collect();
        assert!(
            !bells.is_empty(),
            "expected a cut bell near 1 kHz, got {fitted:?}"
        );
        let &(_, freq, _, _, _) = bells[0];
        assert!(
            (500.0..2_000.0).contains(&freq),
            "bell landed at {freq} Hz instead of ~1 kHz"
        );
    }

    #[test]
    fn sketch_low_edge_fits_low_cut() {
        let mut points = Vec::new();
        for i in 0..40 {
            let t = i as f32 / 39.0;
            let freq = 20.0 * (20_000.0_f32 / 20.0).powf(t);
            let gain = if freq < 150.0 { -14.0 } else { 0.0 };
            points.push((freq, gain));
        }
        let fitted = fit_sketch_bands(&points, 48_000.0);
        assert!(
            fitted
                .iter()
                .any(|(shape, _, _, _, _)| *shape == dsp::SHAPE_LOW_CUT),
            "expected a low cut, got {fitted:?}"
        );
    }

    fn lobe(freq: f32, center: f32, gain: f32, width: f32) -> (f32, f32) {
        let x = (freq / center).log2() / width;
        (freq, gain * (-x * x).exp())
    }

    fn aggregate_response(fitted: &[(u8, f32, f32, f32, u8)], sample_rate: f32, freq: f32) -> f32 {
        fitted
            .iter()
            .map(|&(shape, f0, gain, q, slope)| {
                band_response_db(sample_rate, shape, slope, f0, q, gain, freq)
            })
            .sum()
    }

    #[test]
    fn sketch_fit_tracks_broad_multi_lobe_curve() {
        // The reported case: +-6 dB swoops spanning multiple octaves.
        let mut points = Vec::new();
        for i in 0..120 {
            let t = i as f32 / 119.0;
            let freq = 20.0 * (20_000.0_f32 / 20.0).powf(t);
            let mut g = 0.0;
            for (_, part) in [
                lobe(freq, 65.0, 6.0, 0.8),
                lobe(freq, 450.0, -5.5, 0.9),
                lobe(freq, 6_500.0, 6.0, 0.8),
            ] {
                g += part;
            }
            points.push((freq, g));
        }
        let sr = 48_000.0;
        let fitted = fit_sketch_bands(&points, sr);
        assert!(!fitted.is_empty());
        // The aggregate response must track the sketch everywhere, not only
        // at the extrema.
        let mut worst = 0.0_f32;
        for i in 0..FIT_N {
            let freq = fit_freq_of(i);
            let mut target = 0.0;
            for (_, part) in [
                lobe(freq, 65.0, 6.0, 0.8),
                lobe(freq, 450.0, -5.5, 0.9),
                lobe(freq, 6_500.0, 6.0, 0.8),
            ] {
                target += part;
            }
            let err = (aggregate_response(&fitted, sr, freq) - target).abs();
            worst = worst.max(err);
        }
        assert!(
            worst < 1.5,
            "fit deviates from the sketch by up to {worst:.1} dB: {fitted:?}"
        );
    }

    #[test]
    fn sketch_fit_detects_small_features() {
        // Big dip plus deliberate small +-1.5 dB ripples elsewhere.
        let mut points = Vec::new();
        for i in 0..120 {
            let t = i as f32 / 119.0;
            let freq = 20.0 * (20_000.0_f32 / 20.0).powf(t);
            let x1 = (freq / 500.0).log2() / 0.5;
            let x2 = (freq / 250.0).log2() / 0.25;
            let x3 = (freq / 2_500.0).log2() / 0.25;
            let g = -8.0 * (-x1 * x1).exp() + 1.5 * (-x2 * x2).exp() + 1.2 * (-x3 * x3).exp();
            points.push((freq, g));
        }
        let fitted = fit_sketch_bands(&points, 48_000.0);
        let has_big = fitted
            .iter()
            .any(|(shape, _, gain, _, _)| *shape == dsp::SHAPE_BELL && *gain < -4.0);
        let has_small = fitted.iter().any(|(shape, _, gain, _, _)| {
            *shape == dsp::SHAPE_BELL && (0.5..2.5).contains(&gain.abs())
        });
        assert!(has_big, "big dip not fitted: {fitted:?}");
        assert!(has_small, "small ripples not fitted: {fitted:?}");
    }

    #[test]
    fn sketch_fit_anchors_zero_crossings() {
        // Peak then dip: the curve crosses 0 dB between them, and the fitted
        // response must cross there too instead of sitting offset.
        let mut points = Vec::new();
        for i in 0..120 {
            let t = i as f32 / 119.0;
            let freq = 20.0 * (20_000.0_f32 / 20.0).powf(t);
            let x1 = (freq / 65.0).log2() / 0.7;
            let x2 = (freq / 450.0).log2() / 0.7;
            let g = 6.0 * (-x1 * x1).exp() - 5.5 * (-x2 * x2).exp();
            points.push((freq, g));
        }
        let sr = 48_000.0;
        let fitted = fit_sketch_bands(&points, sr);
        // Find the sketch's zero crossings and check the aggregate there.
        let mut checked = 0;
        for i in 3..FIT_N - 3 {
            let f0 = fit_freq_of(i);
            let f1 = fit_freq_of(i + 1);
            let g0 = {
                let x1 = (f0 / 65.0_f32).log2() / 0.7;
                let x2 = (f0 / 450.0_f32).log2() / 0.7;
                6.0 * (-x1 * x1).exp() - 5.5 * (-x2 * x2).exp()
            };
            let g1 = {
                let x1 = (f1 / 65.0_f32).log2() / 0.7;
                let x2 = (f1 / 450.0_f32).log2() / 0.7;
                6.0 * (-x1 * x1).exp() - 5.5 * (-x2 * x2).exp()
            };
            if (g0 > 0.0) != (g1 > 0.0) {
                let total = aggregate_response(&fitted, sr, f0);
                assert!(
                    total.abs() < 1.0,
                    "zero crossing at {f0:.0} Hz is off by {total:.2} dB"
                );
                checked += 1;
            }
        }
        assert!(checked > 0, "test found no crossings to check");
    }

    #[test]
    fn every_zero_crossing_gets_a_band() {
        // Two lobes crossing zero between them: a band must exist near the
        // crossing frequency regardless of how small the local miss is.
        let mut points = Vec::new();
        for i in 0..120 {
            let t = i as f32 / 119.0;
            let freq = 20.0 * (20_000.0_f32 / 20.0).powf(t);
            let x1 = (freq / 65.0).log2() / 0.7;
            let x2 = (freq / 450.0).log2() / 0.7;
            let g = 6.0 * (-x1 * x1).exp() - 5.5 * (-x2 * x2).exp();
            points.push((freq, g));
        }
        let fitted = fit_sketch_bands(&points, 48_000.0);
        // Crossing of this curve is around 150-200 Hz.
        let has_anchor = fitted
            .iter()
            .any(|&(_, f0, _, _, _)| (100.0..300.0).contains(&f0));
        assert!(has_anchor, "no band near the zero crossing: {fitted:?}");
    }

    #[test]
    fn sketch_too_few_points_yields_nothing() {
        assert!(fit_sketch_bands(&[(100.0, -6.0), (1_000.0, -6.0)], 48_000.0).is_empty());
    }
}

#[cfg(test)]
mod match_tests {
    use super::*;

    fn bin_of_freq(freq: f32) -> usize {
        ((SPECTRUM_BINS - 1) as f32 * (freq / 20.0).ln() / 1_000.0_f32.ln()) as usize
    }

    #[test]
    fn match_fitter_finds_bell() {
        let mut diff = [0.0_f32; SPECTRUM_BINS];
        let center = bin_of_freq(1_000.0);
        for (i, d) in diff.iter_mut().enumerate() {
            let dist = (i as i32 - center as i32).unsigned_abs() as f32;
            if dist < 6.0 {
                *d = 8.0 * (1.0 - dist / 6.0);
            }
        }
        let fitted = fit_match_bands(&diff, 48_000.0);
        let bells: Vec<_> = fitted
            .iter()
            .filter(|(shape, _, gain, _, _)| *shape == dsp::SHAPE_BELL && *gain > 3.0)
            .collect();
        assert!(!bells.is_empty(), "expected a boost bell, got {fitted:?}");
        let &(_, freq, _, _, _) = bells[0];
        assert!((600.0..1_700.0).contains(&freq), "bell landed at {freq} Hz");
    }

    #[test]
    fn match_fitter_finds_shelf() {
        let mut diff = [0.0_f32; SPECTRUM_BINS];
        for (i, d) in diff.iter_mut().enumerate() {
            if bin_of_freq(20.0) + i < bin_of_freq(200.0) {
                *d = -9.0;
            }
        }
        let fitted = fit_match_bands(&diff, 48_000.0);
        assert!(
            fitted
                .iter()
                .any(|(shape, _, gain, _, _)| *shape == dsp::SHAPE_LOW_SHELF && *gain < -3.0),
            "expected a low shelf cut, got {fitted:?}"
        );
    }

    #[test]
    fn match_fitter_ignores_flat_curve() {
        let diff = [0.5_f32; SPECTRUM_BINS];
        assert!(fit_match_bands(&diff, 48_000.0).is_empty());
    }
}
