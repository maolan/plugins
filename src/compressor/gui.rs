use std::{
    ffi::CStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use crate::common::{
    bus,
    spectrum::{
        DEFAULT_SPECTRUM_BINS, SPECTRUM_FLOOR_DB, SpectralAnalyzerWidget, SpectrumMarker,
        SpectrumThresholdCurve, display_range_bounds,
    },
    ui::{SmallKnob, VerticalSlider, small_knob, vertical_slider, vertical_ticks, vu_meter},
};

#[cfg(target_os = "windows")]
use clap_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(unix)]
use clap_clap::ffi::CLAP_WINDOW_API_X11;
use maolan_baseview::iced::{
    Alignment, Color, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{checkbox, column, container, row, text},
};
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};

use crate::compressor::{
    params::{PARAMS, ParamId},
    plugin::SharedState,
};

pub const EDITOR_WIDTH: u32 = 1024;
pub const EDITOR_HEIGHT: u32 = 720;
const MAX_COMPRESSOR_BANDS: usize = 6;

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

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum Message {
    SetParam(ParamId, f32),
    SetBoolParam(ParamId, bool),
    SetScMode(u8),
    SetMode(u8),
    SetScBoost(u8),
    SetTopology(u8),
    SetChannels(ChannelMode),
    SetDisplayRange(f32),
    SelectThresholdBand(usize),
    ResetGainBand(usize),
    StartThresholdBand(usize),
    ResetThresholdBand(usize),
    DragThresholdBand(usize, f32),
    ReleaseThresholdBand(usize),
    StartRangeBand(usize),
    DragRangeBand(usize, f32),
    ReleaseRangeBand(usize),
    StartGainBand(usize),
    DragGainBand(usize, f32),
    ReleaseGainBand(usize),
    CreateBand(f32),
    ReleaseParam(ParamId),
    UiTick,
}

struct State {
    shared: Arc<SharedState>,
    active_gestures: Vec<bool>,

    eq_peers: Vec<bus::PluginSharedData>,
    compressor_peer: Option<bus::PluginSharedData>,

    eq_band_freqs: Vec<f32>,
    spectrum_db: [[f32; DEFAULT_SPECTRUM_BINS]; 2],
    gain_reduction_db: Vec<f32>,
    selected_band: Option<usize>,
    display_range_db: f32,

    last_registry_version: u64,
}

impl Drop for State {
    fn drop(&mut self) {
        bus::remove_needs(bus::NEED_BANDS | bus::NEED_FFT | bus::NEED_GR);
    }
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    bus::add_needs(bus::NEED_BANDS | bus::NEED_FFT | bus::NEED_GR);
    let band_count = active_band_count(&shared);
    (
        State {
            shared,
            active_gestures: vec![false; ParamId::COUNT],
            eq_peers: Vec::new(),
            compressor_peer: None,
            eq_band_freqs: Vec::new(),
            spectrum_db: [[SPECTRUM_FLOOR_DB; DEFAULT_SPECTRUM_BINS]; 2],
            gain_reduction_db: vec![0.0; band_count],
            selected_band: Some(0),
            display_range_db: 12.0,
            last_registry_version: 0,
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
            let idx = id.as_index();
            if !state.active_gestures[idx] {
                state.active_gestures[idx] = true;
                state.shared.mark_gesture_begin_pending(id);
            }
            state.shared.set_param_outbound_only(id, value as f64);
        }
        Message::ReleaseParam(id) => {
            let idx = id.as_index();
            if state.active_gestures[idx] {
                state.active_gestures[idx] = false;
                state.shared.mark_gesture_end_pending(id);
            }
        }
        Message::SetBoolParam(id, value) => {
            state.shared.mark_gesture_begin_pending(id);
            state
                .shared
                .set_param_outbound_only(id, if value { 1.0 } else { 0.0 });
            state.shared.mark_gesture_end_pending(id);
        }
        Message::SetScMode(mode) => {
            state.shared.mark_gesture_begin_pending(ParamId::ScMode);
            state
                .shared
                .set_param_outbound_only(ParamId::ScMode, mode as f64);
            state.shared.mark_gesture_end_pending(ParamId::ScMode);
        }
        Message::SetMode(mode) => {
            state.shared.mark_gesture_begin_pending(ParamId::Mode);
            state
                .shared
                .set_param_outbound_only(ParamId::Mode, mode as f64);
            state.shared.mark_gesture_end_pending(ParamId::Mode);
        }
        Message::SetScBoost(mode) => {
            state.shared.mark_gesture_begin_pending(ParamId::ScBoost);
            state
                .shared
                .set_param_outbound_only(ParamId::ScBoost, mode as f64);
            state.shared.mark_gesture_end_pending(ParamId::ScBoost);
        }
        Message::SetTopology(mode) => {
            state.shared.mark_gesture_begin_pending(ParamId::Topology);
            state
                .shared
                .set_param_outbound_only(ParamId::Topology, mode as f64);
            state.shared.mark_gesture_end_pending(ParamId::Topology);
        }
        Message::SetChannels(mode) => {
            state
                .shared
                .set_param_outbound_only(ParamId::Channels, u32::from(mode) as f64);
            state.shared.request_audio_ports_rescan();
        }
        Message::SetDisplayRange(range) => {
            state.display_range_db = range;
        }
        Message::SelectThresholdBand(band) => {
            state.selected_band =
                Some(band.min(active_band_count(&state.shared).saturating_sub(1)));
        }
        Message::ResetGainBand(band) => {
            let band = band.min(active_band_count(&state.shared).saturating_sub(1));
            state.selected_band = Some(band);
            let id = makeup_param_for_band(band);
            state.shared.mark_gesture_begin_pending(id);
            state.shared.set_param_outbound_only(id, 0.0);
            state.shared.mark_gesture_end_pending(id);
        }
        Message::StartThresholdBand(band) => {
            state.selected_band =
                Some(band.min(active_band_count(&state.shared).saturating_sub(1)));
            let id = threshold_param_for_band(band);
            let idx = id.as_index();
            if !state.active_gestures[idx] {
                state.active_gestures[idx] = true;
                state.shared.mark_gesture_begin_pending(id);
            }
            state
                .shared
                .set_param_outbound_only(id, PARAMS[idx].default);
        }
        Message::ResetThresholdBand(band) => {
            let band = band.min(active_band_count(&state.shared).saturating_sub(1));
            state.selected_band = Some(band);
            let id = threshold_param_for_band(band);
            state.shared.mark_gesture_begin_pending(id);
            state
                .shared
                .set_param_outbound_only(id, PARAMS[id.as_index()].default);
            state.shared.mark_gesture_end_pending(id);
        }
        Message::DragThresholdBand(band, delta_db) => {
            let id = threshold_param_for_band(band);
            let idx = id.as_index();
            if !state.active_gestures[idx] {
                state.active_gestures[idx] = true;
                state.shared.mark_gesture_begin_pending(id);
            }
            let (min, max) = display_range_bounds(state.display_range_db);
            let value = (state.shared.params.get(id) + f64::from(delta_db))
                .clamp(f64::from(min), f64::from(max));
            state.shared.set_param_outbound_only(id, value);
        }
        Message::ReleaseThresholdBand(band) => {
            let id = threshold_param_for_band(band);
            let idx = id.as_index();
            if state.active_gestures[idx] {
                state.active_gestures[idx] = false;
                state.shared.mark_gesture_end_pending(id);
            }
        }
        Message::StartRangeBand(band) => {
            state.selected_band =
                Some(band.min(active_band_count(&state.shared).saturating_sub(1)));
            let id = range_param_for_band(band);
            let idx = id.as_index();
            if !state.active_gestures[idx] {
                state.active_gestures[idx] = true;
                state.shared.mark_gesture_begin_pending(id);
            }
            state.shared.set_param_outbound_only(id, 0.0);
        }
        Message::DragRangeBand(band, delta_db) => {
            let id = range_param_for_band(band);
            let idx = id.as_index();
            if !state.active_gestures[idx] {
                state.active_gestures[idx] = true;
                state.shared.mark_gesture_begin_pending(id);
            }
            let def = PARAMS[idx];
            let value = (state.shared.params.get(id) + f64::from(delta_db)).clamp(def.min, def.max);
            state.shared.set_param_outbound_only(id, value);
        }
        Message::ReleaseRangeBand(band) => {
            let id = range_param_for_band(band);
            let idx = id.as_index();
            if state.active_gestures[idx] {
                state.active_gestures[idx] = false;
                state.shared.mark_gesture_end_pending(id);
            }
        }
        Message::StartGainBand(band) => {
            state.selected_band =
                Some(band.min(active_band_count(&state.shared).saturating_sub(1)));
            let id = makeup_param_for_band(band);
            let idx = id.as_index();
            if !state.active_gestures[idx] {
                state.active_gestures[idx] = true;
                state.shared.mark_gesture_begin_pending(id);
            }
            state.shared.set_param_outbound_only(id, 0.0);
        }
        Message::DragGainBand(band, delta_db) => {
            let id = makeup_param_for_band(band);
            let idx = id.as_index();
            if !state.active_gestures[idx] {
                state.active_gestures[idx] = true;
                state.shared.mark_gesture_begin_pending(id);
            }
            let def = PARAMS[idx];
            let value = (state.shared.params.get(id) + f64::from(delta_db)).clamp(def.min, def.max);
            state.shared.set_param_outbound_only(id, value);
        }
        Message::ReleaseGainBand(band) => {
            let id = makeup_param_for_band(band);
            let idx = id.as_index();
            if state.active_gestures[idx] {
                state.active_gestures[idx] = false;
                state.shared.mark_gesture_end_pending(id);
            }
        }
        Message::CreateBand(freq) => {
            let band_count = active_band_count(&state.shared);
            if band_count < MAX_COMPRESSOR_BANDS {
                let mut splits = active_splits(&state.shared);
                splits.push(freq.clamp(20.0, 20_000.0));
                splits.sort_by(|a, b| a.total_cmp(b));
                for (index, split) in splits.iter().copied().enumerate() {
                    let id = split_param_for_index(index);
                    state.shared.set_param_outbound_only(id, f64::from(split));
                    state.shared.mark_gesture_begin_pending(id);
                    state.shared.mark_gesture_end_pending(id);
                }
                state
                    .shared
                    .set_param_outbound_only(ParamId::BandCount, (band_count + 1) as f64);
                state.shared.mark_gesture_begin_pending(ParamId::BandCount);
                state.shared.mark_gesture_end_pending(ParamId::BandCount);
                if band_count + 1 > 4 {
                    state.shared.set_param_outbound_only(ParamId::Topology, 1.0);
                    state.shared.mark_gesture_begin_pending(ParamId::Topology);
                    state.shared.mark_gesture_end_pending(ParamId::Topology);
                }
                state.selected_band = Some(
                    splits
                        .iter()
                        .position(|split| (*split - freq).abs() < 1.0)
                        .map(|index| index + 1)
                        .unwrap_or(band_count)
                        .min(MAX_COMPRESSOR_BANDS - 1),
                );
            }
        }
        Message::UiTick => {
            state.spectrum_db = state.shared.spectrum_db();
            let version = bus::registry_version();
            if version != state.last_registry_version {
                state.eq_peers = bus::discover(|p| p.plugin_type == bus::PluginType::Eq);
                let own_slot = state.shared.own_slot();
                state.compressor_peer = bus::discover(|p| {
                    p.plugin_type == bus::PluginType::Compressor && p.slot_index() == own_slot
                })
                .into_iter()
                .next();
                state.last_registry_version = version;
            }

            let mut freqs = Vec::new();
            let mut bands = bus::EqBands::default();
            for peer in &state.eq_peers {
                if let Some(slot) = peer.bands_slot()
                    && slot.read(&mut bands)
                {
                    for i in 0..bands.len.min(bands.bands.len()) {
                        if bands.bands[i].on {
                            freqs.push(bands.bands[i].freq);
                        }
                    }
                }
            }
            freqs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            freqs.dedup_by(|a, b| (*a - *b).abs() < 5.0);
            state.eq_band_freqs = freqs;

            if let Some(peer) = &state.compressor_peer
                && let Some(slot) = peer.gr_slot()
            {
                let mut gr = bus::CompressorGrData::default();
                if slot.read(&mut gr) {
                    state.gain_reduction_db.clear();
                    for i in 0..gr.valid_bands.min(gr.gr_db.len()) {
                        state.gain_reduction_db.push(gr.gr_db[i]);
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
    let b = |id: ParamId| state.shared.params.get_bool(id);
    let band_count = active_band_count(&state.shared);
    let splits = active_splits(&state.shared);

    let analyzer = SpectralAnalyzerWidget::new(
        state.spectrum_db,
        state.shared.channels.load(Ordering::Acquire) >= 2,
    )
    .with_spectrum_markers(
        split_param_ids()[..band_count.saturating_sub(1)]
            .iter()
            .enumerate()
            .map(|(index, &id)| {
                SpectrumMarker::draggable(
                    splits[index],
                    move |freq| Message::SetParam(id, freq),
                    Message::ReleaseParam(id),
                )
            })
            .collect(),
    )
    .with_threshold_curves(threshold_curves(ThresholdCurveParams {
        splits,
        band_count,
        input_gain_db: p(ParamId::InputGain),
        sc_boost: state.shared.params.get_enum(ParamId::ScBoost),
        sample_rate: state.shared.sample_rate(),
        thresholds: threshold_param_ids().map(&p),
        ranges: range_param_ids().map(&p),
        gains: makeup_param_ids().map(&p),
        gain_reduction_db: std::array::from_fn(|i| {
            state.gain_reduction_db.get(i).copied().unwrap_or(0.0)
        }),
        selected_band: state.selected_band,
    }))
    .with_display_range(state.display_range_db)
    .with_gain_reduction(state.gain_reduction_db.clone())
    .on_double_click(|freq, _db| Message::CreateBand(freq))
    .view_fill();

    let channels = p(ParamId::Channels).round() as u32;
    let channels_dropdown = maolan_baseview::iced::widget::pick_list(
        vec![ChannelMode::Mono, ChannelMode::Stereo],
        Some(ChannelMode::from(channels)),
        Message::SetChannels,
    )
    .placeholder("Channels");

    if !state.eq_band_freqs.is_empty() {
        let freq_text = state
            .eq_band_freqs
            .iter()
            .map(|f| format!("{f:.0} Hz"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = freq_text;
    }

    let sc_mode = state.shared.params.get_enum(ParamId::ScMode).min(1);
    let mode = state.shared.params.get_enum(ParamId::Mode).min(1);
    let sc_boost = state.shared.params.get_enum(ParamId::ScBoost).min(4);
    let topology = state.shared.params.get_enum(ParamId::Topology).min(1);

    let sidechain_controls = row![
        text("Sidechain").size(16),
        maolan_baseview::iced::widget::radio("Peak", 0u8, Some(sc_mode as u8), Message::SetScMode),
        maolan_baseview::iced::widget::radio("RMS", 1u8, Some(sc_mode as u8), Message::SetScMode),
        checkbox(b(ParamId::Bypass))
            .label("Bypass")
            .on_toggle(|v| Message::SetBoolParam(ParamId::Bypass, v)),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let mode_controls = row![
        text("Mode").size(16),
        maolan_baseview::iced::widget::radio("Compress", 0u8, Some(mode as u8), Message::SetMode),
        maolan_baseview::iced::widget::radio("Expand", 1u8, Some(mode as u8), Message::SetMode),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let boost_controls = row![
        text("SC Boost").size(16),
        maolan_baseview::iced::widget::radio("Off", 0u8, Some(sc_boost as u8), Message::SetScBoost),
        maolan_baseview::iced::widget::radio(
            "BT+3",
            1u8,
            Some(sc_boost as u8),
            Message::SetScBoost
        ),
        maolan_baseview::iced::widget::radio(
            "MT+3",
            2u8,
            Some(sc_boost as u8),
            Message::SetScBoost
        ),
        maolan_baseview::iced::widget::radio(
            "BT+6",
            3u8,
            Some(sc_boost as u8),
            Message::SetScBoost
        ),
        maolan_baseview::iced::widget::radio(
            "MT+6",
            4u8,
            Some(sc_boost as u8),
            Message::SetScBoost
        ),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let topology_controls = row![
        text("Topology").size(16),
        maolan_baseview::iced::widget::radio(
            "Classic",
            0u8,
            Some(topology as u8),
            Message::SetTopology
        ),
        maolan_baseview::iced::widget::radio(
            "Modern",
            1u8,
            Some(topology as u8),
            Message::SetTopology
        ),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let bottom_controls = row![
        channels_dropdown,
        text("Range").size(11),
        maolan_baseview::iced::widget::pick_list(
            vec![3.0_f32, 6.0, 12.0, 30.0],
            Some(state.display_range_db),
            Message::SetDisplayRange,
        )
        .width(Length::Fixed(70.0)),
        knob("Dry", ParamId::DryGain, p(ParamId::DryGain), "", 0.01),
        knob("Wet", ParamId::WetGain, p(ParamId::WetGain), "", 0.01),
        knob(
            "Lookahead",
            ParamId::Lookahead,
            p(ParamId::Lookahead),
            "ms",
            0.01
        ),
    ]
    .spacing(12)
    .align_y(Alignment::Center);
    let meter_channels = if state.shared.channels.load(Ordering::Acquire) >= 2 {
        2
    } else {
        1
    };

    let display_row = row![
        gain_slider(ParamId::InputGain, p(ParamId::InputGain), "dB", 0.1),
        vertical_ticks(),
        vu_meter(meter_channels, state.shared.input_levels_db()),
        analyzer,
        vu_meter(meter_channels, state.shared.output_levels_db()),
        vertical_ticks(),
        gain_slider(ParamId::OutputGain, p(ParamId::OutputGain), "dB", 0.1),
    ]
    .spacing(8)
    .height(Length::Fill)
    .align_y(Alignment::Center);

    let content = column![
        display_row,
        band_strip(state),
        sidechain_controls,
        mode_controls,
        boost_controls,
        topology_controls,
        bottom_controls,
    ]
    .spacing(12)
    .align_x(Alignment::Start);

    container(content)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Left)
        .align_y(Vertical::Top)
        .into()
}

#[derive(Debug, Clone, Copy)]
struct BandIds {
    th: ParamId,
    range: ParamId,
    ratio: ParamId,
    att: ParamId,
    rel: ParamId,
    knee: ParamId,
    makeup: ParamId,
}

fn band_strip(state: &State) -> Element<'_, Message> {
    let band_count = active_band_count(&state.shared);
    let selected = state
        .selected_band
        .unwrap_or(0)
        .min(band_count.saturating_sub(1));
    let ids = band_ids();
    band_section(selected, state, ids[selected])
}

fn band_section<'a>(_band: usize, state: &'a State, ids: BandIds) -> Element<'a, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;
    let threshold_range = display_range_bounds(state.display_range_db);
    container(
        row![
            knob_with_range("Th", ids.th, p(ids.th), "dB", 0.1, Some(threshold_range)),
            knob("Range", ids.range, p(ids.range), "dB", 0.1),
            knob("Ratio", ids.ratio, p(ids.ratio), "", 0.1),
            knob("Knee", ids.knee, p(ids.knee), "dB", 0.1),
            knob("Atk", ids.att, p(ids.att), "ms", 0.1),
            knob("Rel", ids.rel, p(ids.rel), "ms", 0.1),
            knob("Makeup", ids.makeup, p(ids.makeup), "dB", 0.1),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding(4)
    .width(Length::Fill)
    .into()
}

fn active_band_count(shared: &SharedState) -> usize {
    shared
        .params
        .get(ParamId::BandCount)
        .round()
        .clamp(1.0, MAX_COMPRESSOR_BANDS as f64) as usize
}

fn split_param_ids() -> [ParamId; MAX_COMPRESSOR_BANDS - 1] {
    [
        ParamId::Split1,
        ParamId::Split2,
        ParamId::Split3,
        ParamId::Split4,
        ParamId::Split5,
    ]
}

fn threshold_param_ids() -> [ParamId; MAX_COMPRESSOR_BANDS] {
    [
        ParamId::B1Threshold,
        ParamId::B2Threshold,
        ParamId::B3Threshold,
        ParamId::B4Threshold,
        ParamId::B5Threshold,
        ParamId::B6Threshold,
    ]
}

fn range_param_ids() -> [ParamId; MAX_COMPRESSOR_BANDS] {
    [
        ParamId::B1Range,
        ParamId::B2Range,
        ParamId::B3Range,
        ParamId::B4Range,
        ParamId::B5Range,
        ParamId::B6Range,
    ]
}

fn makeup_param_ids() -> [ParamId; MAX_COMPRESSOR_BANDS] {
    [
        ParamId::B1Makeup,
        ParamId::B2Makeup,
        ParamId::B3Makeup,
        ParamId::B4Makeup,
        ParamId::B5Makeup,
        ParamId::B6Makeup,
    ]
}

fn split_param_for_index(index: usize) -> ParamId {
    split_param_ids()[index.min(MAX_COMPRESSOR_BANDS - 2)]
}

fn active_splits(shared: &SharedState) -> Vec<f32> {
    let count = active_band_count(shared).saturating_sub(1);
    let mut splits: Vec<f32> = split_param_ids()[..count]
        .iter()
        .map(|&id| shared.params.get(id) as f32)
        .collect();
    splits.sort_by(|a, b| a.total_cmp(b));
    splits
}

fn band_ids() -> [BandIds; MAX_COMPRESSOR_BANDS] {
    [
        BandIds {
            th: ParamId::B1Threshold,
            range: ParamId::B1Range,
            ratio: ParamId::B1Ratio,
            att: ParamId::B1Attack,
            rel: ParamId::B1Release,
            knee: ParamId::B1Knee,
            makeup: ParamId::B1Makeup,
        },
        BandIds {
            th: ParamId::B2Threshold,
            range: ParamId::B2Range,
            ratio: ParamId::B2Ratio,
            att: ParamId::B2Attack,
            rel: ParamId::B2Release,
            knee: ParamId::B2Knee,
            makeup: ParamId::B2Makeup,
        },
        BandIds {
            th: ParamId::B3Threshold,
            range: ParamId::B3Range,
            ratio: ParamId::B3Ratio,
            att: ParamId::B3Attack,
            rel: ParamId::B3Release,
            knee: ParamId::B3Knee,
            makeup: ParamId::B3Makeup,
        },
        BandIds {
            th: ParamId::B4Threshold,
            range: ParamId::B4Range,
            ratio: ParamId::B4Ratio,
            att: ParamId::B4Attack,
            rel: ParamId::B4Release,
            knee: ParamId::B4Knee,
            makeup: ParamId::B4Makeup,
        },
        BandIds {
            th: ParamId::B5Threshold,
            range: ParamId::B5Range,
            ratio: ParamId::B5Ratio,
            att: ParamId::B5Attack,
            rel: ParamId::B5Release,
            knee: ParamId::B5Knee,
            makeup: ParamId::B5Makeup,
        },
        BandIds {
            th: ParamId::B6Threshold,
            range: ParamId::B6Range,
            ratio: ParamId::B6Ratio,
            att: ParamId::B6Attack,
            rel: ParamId::B6Release,
            knee: ParamId::B6Knee,
            makeup: ParamId::B6Makeup,
        },
    ]
}

fn threshold_param_for_band(band: usize) -> ParamId {
    threshold_param_ids()[band.min(MAX_COMPRESSOR_BANDS - 1)]
}

fn range_param_for_band(band: usize) -> ParamId {
    range_param_ids()[band.min(MAX_COMPRESSOR_BANDS - 1)]
}

fn makeup_param_for_band(band: usize) -> ParamId {
    makeup_param_ids()[band.min(MAX_COMPRESSOR_BANDS - 1)]
}

struct ThresholdCurveParams {
    splits: Vec<f32>,
    band_count: usize,
    input_gain_db: f32,
    sc_boost: u32,
    sample_rate: f32,
    thresholds: [f32; MAX_COMPRESSOR_BANDS],
    ranges: [f32; MAX_COMPRESSOR_BANDS],
    gains: [f32; MAX_COMPRESSOR_BANDS],
    gain_reduction_db: [f32; MAX_COMPRESSOR_BANDS],
    selected_band: Option<usize>,
}

fn threshold_curves(params: ThresholdCurveParams) -> Vec<SpectrumThresholdCurve<Message>> {
    const F_MIN: f32 = 20.0;
    const F_MAX: f32 = 20_000.0;
    const POINTS: usize = 192;

    let band_count = params.band_count.clamp(1, MAX_COMPRESSOR_BANDS);
    let splits = sorted_splits(&params.splits, band_count);
    let threshold_values: [f32; MAX_COMPRESSOR_BANDS] = std::array::from_fn(|band| {
        params.thresholds[band] - params.input_gain_db - sidechain_boost_db(params.sc_boost, band)
    });
    let range_values: [f32; MAX_COMPRESSOR_BANDS] =
        std::array::from_fn(|band| params.gains[band] + params.ranges[band]);
    let live_gain_values: [f32; MAX_COMPRESSOR_BANDS] = std::array::from_fn(|band| {
        params.gains[band]
            + live_range_offset_db(params.ranges[band], params.gain_reduction_db[band])
    });
    let mut gain_points = Vec::with_capacity(POINTS);
    let mut range_points = Vec::with_capacity(POINTS);
    let mut threshold_points = Vec::with_capacity(POINTS);
    for point in 0..POINTS {
        let t = point as f32 / (POINTS - 1) as f32;
        let freq = F_MIN * (F_MAX / F_MIN).powf(t);
        gain_points.push((
            freq,
            blended_threshold_db(
                freq,
                &splits,
                band_count,
                params.sample_rate,
                &live_gain_values,
            ),
        ));
        range_points.push((
            freq,
            blended_threshold_db(freq, &splits, band_count, params.sample_rate, &range_values),
        ));
        threshold_points.push((
            freq,
            blended_threshold_db(
                freq,
                &splits,
                band_count,
                params.sample_rate,
                &threshold_values,
            ),
        ));
    }

    let select_splits = splits.clone();
    let drag_splits = splits.clone();
    let reset_splits = splits.clone();
    let release_splits = splits.clone();
    let threshold_middle_select_splits = splits.clone();
    let threshold_middle_drag_splits = splits.clone();
    let threshold_middle_release_splits = splits.clone();
    let threshold_right_select_splits = splits.clone();
    let threshold_right_drag_splits = splits.clone();
    let threshold_right_release_splits = splits.clone();
    let range_select_splits = splits.clone();
    let range_drag_splits = splits.clone();
    let range_release_splits = splits.clone();
    let gain_select_splits = splits.clone();
    let gain_drag_splits = splits.clone();
    let gain_release_splits = splits;
    let selected = params.selected_band.is_some();
    vec![
        SpectrumThresholdCurve {
            points: gain_points,
            selected,
            color: Some(Color::from_rgba(0.90, 0.93, 0.96, 0.90)),
            selected_color: Some(Color::from_rgba(0.90, 0.93, 0.96, 0.96)),
            width: 1.5,
            selected_width: 1.5,
            on_select: None,
            on_select_at: Some(Arc::new(move |freq| {
                Message::SelectThresholdBand(
                    band_for_freq(freq, &select_splits).min(band_count - 1),
                )
            })),
            on_drag_db: None,
            on_drag_db_at: Some(Arc::new(move |freq, delta_db| {
                Message::DragGainBand(
                    band_for_freq(freq, &drag_splits).min(band_count - 1),
                    delta_db,
                )
            })),
            on_double_click_at: Some(Arc::new(move |freq| {
                Message::ResetGainBand(band_for_freq(freq, &reset_splits).min(band_count - 1))
            })),
            on_release: None,
            on_release_at: Some(Arc::new(move |freq| {
                Message::ReleaseGainBand(band_for_freq(freq, &release_splits).min(band_count - 1))
            })),
            on_middle_select_at: Some(Arc::new(move |freq| {
                Message::StartRangeBand(
                    band_for_freq(freq, &threshold_middle_select_splits).min(band_count - 1),
                )
            })),
            on_middle_drag_db_at: Some(Arc::new(move |freq, delta_db| {
                Message::DragRangeBand(
                    band_for_freq(freq, &threshold_middle_drag_splits).min(band_count - 1),
                    delta_db,
                )
            })),
            on_middle_release_at: Some(Arc::new(move |freq| {
                Message::ReleaseRangeBand(
                    band_for_freq(freq, &threshold_middle_release_splits).min(band_count - 1),
                )
            })),
            on_right_select_at: Some(Arc::new(move |freq| {
                Message::StartThresholdBand(
                    band_for_freq(freq, &threshold_right_select_splits).min(band_count - 1),
                )
            })),
            on_right_drag_db_at: Some(Arc::new(move |freq, delta_db| {
                Message::DragThresholdBand(
                    band_for_freq(freq, &threshold_right_drag_splits).min(band_count - 1),
                    delta_db,
                )
            })),
            on_right_release_at: Some(Arc::new(move |freq| {
                Message::ReleaseThresholdBand(
                    band_for_freq(freq, &threshold_right_release_splits).min(band_count - 1),
                )
            })),
        },
        SpectrumThresholdCurve {
            points: range_points,
            selected,
            color: Some(Color::from_rgba(1.0, 0.83, 0.10, 0.30)),
            selected_color: Some(Color::from_rgba(1.0, 0.83, 0.10, 0.44)),
            width: 1.0,
            selected_width: 1.5,
            on_select: None,
            on_select_at: Some(Arc::new(move |freq| {
                Message::SelectThresholdBand(
                    band_for_freq(freq, &range_select_splits).min(band_count - 1),
                )
            })),
            on_drag_db: None,
            on_drag_db_at: Some(Arc::new(move |freq, delta_db| {
                Message::DragRangeBand(
                    band_for_freq(freq, &range_drag_splits).min(band_count - 1),
                    delta_db,
                )
            })),
            on_double_click_at: None,
            on_release: None,
            on_release_at: Some(Arc::new(move |freq| {
                Message::ReleaseRangeBand(
                    band_for_freq(freq, &range_release_splits).min(band_count - 1),
                )
            })),
            on_middle_select_at: None,
            on_middle_drag_db_at: None,
            on_middle_release_at: None,
            on_right_select_at: None,
            on_right_drag_db_at: None,
            on_right_release_at: None,
        },
        SpectrumThresholdCurve {
            points: threshold_points,
            selected,
            color: Some(Color::from_rgba(0.18, 0.76, 0.95, 0.34)),
            selected_color: Some(Color::from_rgba(0.18, 0.76, 0.95, 0.52)),
            width: 1.0,
            selected_width: 1.5,
            on_select: None,
            on_select_at: Some(Arc::new(move |freq| {
                Message::SelectThresholdBand(
                    band_for_freq(freq, &gain_select_splits).min(band_count - 1),
                )
            })),
            on_drag_db: None,
            on_drag_db_at: Some(Arc::new(move |freq, delta_db| {
                Message::DragThresholdBand(
                    band_for_freq(freq, &gain_drag_splits).min(band_count - 1),
                    delta_db,
                )
            })),
            on_double_click_at: None,
            on_release: None,
            on_release_at: Some(Arc::new(move |freq| {
                Message::ReleaseThresholdBand(
                    band_for_freq(freq, &gain_release_splits).min(band_count - 1),
                )
            })),
            on_middle_select_at: None,
            on_middle_drag_db_at: None,
            on_middle_release_at: None,
            on_right_select_at: None,
            on_right_drag_db_at: None,
            on_right_release_at: None,
        },
    ]
}

fn sorted_splits(splits: &[f32], band_count: usize) -> Vec<f32> {
    const F_MIN: f32 = 20.0;
    const F_MAX: f32 = 20_000.0;
    let mut splits: Vec<f32> = splits
        .iter()
        .take(band_count.saturating_sub(1))
        .map(|split| split.clamp(F_MIN, F_MAX))
        .collect();
    splits.sort_by(|a, b| a.total_cmp(b));
    splits
}

fn band_for_freq(freq: f32, splits: &[f32]) -> usize {
    splits.iter().take_while(|split| freq >= **split).count()
}

fn blended_threshold_db(
    freq: f32,
    splits: &[f32],
    band_count: usize,
    sample_rate: f32,
    thresholds: &[f32; MAX_COMPRESSOR_BANDS],
) -> f32 {
    let mut weighted_sum = 0.0;
    let mut total_weight = 0.0;
    for (band, threshold) in thresholds.iter().copied().enumerate().take(band_count) {
        let weight = detector_band_magnitude(band, freq, splits, sample_rate).powi(2);
        weighted_sum += threshold * weight;
        total_weight += weight;
    }
    if total_weight > 1.0e-9 {
        weighted_sum / total_weight
    } else {
        thresholds[band_for_freq(freq, splits).min(band_count.saturating_sub(1))]
    }
}

fn live_range_offset_db(range_db: f32, gain_reduction_db: f32) -> f32 {
    let live_gain_db = -gain_reduction_db;
    if range_db > 0.0 {
        live_gain_db.clamp(0.0, range_db)
    } else if range_db < 0.0 {
        live_gain_db.clamp(range_db, 0.0)
    } else {
        0.0
    }
}

fn sidechain_boost_db(sc_boost: u32, band: usize) -> f32 {
    match sc_boost {
        1 if band == 0 => 3.0,
        2 if band <= 1 => 3.0,
        3 if band == 0 => 6.0,
        4 if band <= 1 => 6.0,
        _ => 0.0,
    }
}

fn detector_band_magnitude(band: usize, freq: f32, splits: &[f32], sample_rate: f32) -> f32 {
    let mut response = 1.0;
    for split in splits.iter().take(band) {
        response *= lr4_highpass_magnitude(freq, *split, sample_rate);
    }
    if let Some(split) = splits.get(band) {
        response *= lr4_lowpass_magnitude(freq, *split, sample_rate);
    }
    response
}

fn lr4_lowpass_magnitude(freq: f32, cutoff: f32, sample_rate: f32) -> f32 {
    let biquad = BiquadCoeffs::lowpass(cutoff, sample_rate);
    let magnitude = biquad.magnitude(freq, sample_rate);
    magnitude * magnitude
}

fn lr4_highpass_magnitude(freq: f32, cutoff: f32, sample_rate: f32) -> f32 {
    let biquad = BiquadCoeffs::highpass(cutoff, sample_rate);
    let magnitude = biquad.magnitude(freq, sample_rate);
    magnitude * magnitude
}

struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl BiquadCoeffs {
    fn lowpass(cutoff_hz: f32, sample_rate: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * (1.0 / 2.0_f32.sqrt()));

        let b0 = (1.0 - cw) * 0.5;
        let b1 = 1.0 - cw;
        let b2 = (1.0 - cw) * 0.5;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cw;
        let a2 = 1.0 - alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    fn highpass(cutoff_hz: f32, sample_rate: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * (1.0 / 2.0_f32.sqrt()));

        let b0 = (1.0 + cw) * 0.5;
        let b1 = -(1.0 + cw);
        let b2 = (1.0 + cw) * 0.5;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cw;
        let a2 = 1.0 - alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    fn magnitude(&self, freq: f32, sample_rate: f32) -> f32 {
        let w = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let cos_w = w.cos();
        let sin_w = w.sin();
        let cos_2w = (2.0 * w).cos();
        let sin_2w = (2.0 * w).sin();
        let num_re = self.b0 + self.b1 * cos_w + self.b2 * cos_2w;
        let num_im = -self.b1 * sin_w - self.b2 * sin_2w;
        let den_re = 1.0 + self.a1 * cos_w + self.a2 * cos_2w;
        let den_im = -self.a1 * sin_w - self.a2 * sin_2w;
        let num_mag = (num_re * num_re + num_im * num_im).sqrt();
        let den_mag = (den_re * den_re + den_im * den_im).sqrt().max(1.0e-12);
        num_mag / den_mag
    }
}

fn theme(_state: &State) -> Theme {
    Theme::TokyoNight
}

fn knob(
    label: &'static str,
    id: ParamId,
    value: f32,
    units: &'static str,
    step: f32,
) -> Element<'static, Message> {
    knob_with_range(label, id, value, units, step, None)
}

fn knob_with_range(
    label: &'static str,
    id: ParamId,
    value: f32,
    units: &'static str,
    step: f32,
    range: Option<(f32, f32)>,
) -> Element<'static, Message> {
    let def = PARAMS[id.as_index()];
    let min = range.map(|(min, _)| min).unwrap_or(def.min as f32);
    let max = range.map(|(_, max)| max).unwrap_or(def.max as f32);
    let value_text = if units.is_empty() {
        format!("{value:.2}")
    } else if units == "Hz" {
        format!("{value:.0} {units}")
    } else {
        format!("{value:.1} {units}")
    };

    small_knob(
        SmallKnob {
            label: label.to_string(),
            value,
            range: min..=max,
            default: def.default as f32,
            step,
            value_text,
        },
        move |v| Message::SetParam(id, v.clamp(min, max)),
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

fn build_app(shared: Arc<SharedState>) -> impl maolan_baseview::iced::Program {
    maolan_baseview::iced::application(move || init(shared.clone()), update, view)
        .font(maolan_widgets::iced_fonts::LUCIDE_FONT_BYTES)
        .theme(theme)
        .run()
}

struct AnyWindowHandle {
    _inner: Box<dyn std::any::Any>,
}

unsafe impl Send for AnyWindowHandle {}

pub struct GuiBridge {
    created: bool,
    floating: bool,
    shared: Option<Arc<SharedState>>,
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
    pub fn create(&mut self, shared: Arc<SharedState>, api: &CStr, is_floating: bool) -> bool {
        if !is_api_supported(api, is_floating) {
            return false;
        }
        self.created = true;
        self.floating = is_floating;
        self.shared = Some(shared);
        true
    }

    pub fn destroy(&mut self) {
        self.window_handle = None;
        self.shared = None;
        self.floating = false;
        self.created = false;
    }

    pub fn set_parent(&mut self, shared: Arc<SharedState>, parent: ParentWindowHandle) -> bool {
        if !self.created {
            return false;
        }
        if self.floating {
            self.shared = Some(shared);
            return true;
        }

        let settings = maolan_baseview::iced::IcedBaseviewSettings {
            window: maolan_baseview::iced::baseview::WindowOpenOptions {
                title: String::from("Maolan MB Compressor"),
                size: maolan_baseview::iced::baseview::Size::new(
                    EDITOR_WIDTH as f64,
                    EDITOR_HEIGHT as f64,
                ),
                scale: maolan_baseview::iced::baseview::WindowScalePolicy::SystemScaleFactor,
            },
            ignore_non_modifier_keys: false,
            always_redraw: false,
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
            let open_flag = self.floating_open.clone();
            thread::spawn(move || {
                let settings = maolan_baseview::iced::IcedBaseviewSettings {
                    window: maolan_baseview::iced::baseview::WindowOpenOptions {
                        title: String::from("Maolan MB Compressor"),
                        size: maolan_baseview::iced::baseview::Size::new(
                            EDITOR_WIDTH as f64,
                            EDITOR_HEIGHT as f64,
                        ),
                        scale:
                            maolan_baseview::iced::baseview::WindowScalePolicy::SystemScaleFactor,
                    },
                    ignore_non_modifier_keys: false,
                    always_redraw: false,
                };
                maolan_baseview::iced::shell::open_blocking(
                    settings,
                    maolan_baseview::iced::PollSubNotifier::new(),
                    move || build_app(shared),
                );
                open_flag.store(false, Ordering::Release);
            });
        }
        true
    }

    pub fn hide(&mut self) -> (bool, bool) {
        if self.floating {
            self.floating_open.store(false, Ordering::Release);
            return (true, true);
        }
        self.window_handle = None;
        (true, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve_for_thresholds(thresholds: [f32; MAX_COMPRESSOR_BANDS]) -> Vec<(f32, f32)> {
        let curves = threshold_curves(ThresholdCurveParams {
            splits: vec![200.0, 2000.0, 8000.0],
            band_count: 4,
            input_gain_db: 0.0,
            sc_boost: 0,
            sample_rate: 48_000.0,
            thresholds,
            ranges: [-6.0; MAX_COMPRESSOR_BANDS],
            gains: [0.0; MAX_COMPRESSOR_BANDS],
            gain_reduction_db: [0.0; MAX_COMPRESSOR_BANDS],
            selected_band: Some(0),
        });
        assert_eq!(curves.len(), 3);
        curves[2].points.clone()
    }

    fn range_curve_for_values(
        thresholds: [f32; MAX_COMPRESSOR_BANDS],
        ranges: [f32; MAX_COMPRESSOR_BANDS],
    ) -> Vec<(f32, f32)> {
        let curves = threshold_curves(ThresholdCurveParams {
            splits: vec![200.0, 2000.0, 8000.0],
            band_count: 4,
            input_gain_db: 0.0,
            sc_boost: 0,
            sample_rate: 48_000.0,
            thresholds,
            ranges,
            gains: [0.0; MAX_COMPRESSOR_BANDS],
            gain_reduction_db: [0.0; MAX_COMPRESSOR_BANDS],
            selected_band: Some(0),
        });
        assert_eq!(curves.len(), 3);
        curves[1].points.clone()
    }

    fn gain_curve_for_values(gains: [f32; MAX_COMPRESSOR_BANDS]) -> Vec<(f32, f32)> {
        let curves = threshold_curves(ThresholdCurveParams {
            splits: vec![200.0, 2000.0, 8000.0],
            band_count: 4,
            input_gain_db: 0.0,
            sc_boost: 0,
            sample_rate: 48_000.0,
            thresholds: [-10.0; MAX_COMPRESSOR_BANDS],
            ranges: [-6.0; MAX_COMPRESSOR_BANDS],
            gains,
            gain_reduction_db: [0.0; MAX_COMPRESSOR_BANDS],
            selected_band: Some(0),
        });
        assert_eq!(curves.len(), 3);
        curves[0].points.clone()
    }

    #[test]
    fn equal_thresholds_draw_flat_contour() {
        let points = curve_for_thresholds([-10.5; MAX_COMPRESSOR_BANDS]);
        for &(_, db) in &points {
            assert!(
                (db + 10.5).abs() < 1.0e-4,
                "expected flat -10.5 dB contour, got {db}"
            );
        }
    }

    #[test]
    fn threshold_ui_range_matches_analyzer_range() {
        assert_eq!(display_range_bounds(12.0), (-21.0, 12.0));
        assert_eq!(display_range_bounds(30.0), (-52.5, 30.0));
    }

    #[test]
    fn compressor_defaults_are_gain_threshold_range() {
        assert_eq!(PARAMS[ParamId::BandCount.as_index()].default, 1.0);
        for id in makeup_param_ids() {
            assert_eq!(PARAMS[id.as_index()].default, 0.0);
        }
        for id in threshold_param_ids() {
            assert_eq!(PARAMS[id.as_index()].default, -3.0);
        }
        for id in range_param_ids() {
            assert_eq!(PARAMS[id.as_index()].default, -12.0);
        }
    }

    #[test]
    fn middle_drag_range_starts_from_threshold() {
        let shared = Arc::new(SharedState::default());
        shared.set_param_outbound_only(ParamId::B1Range, -30.0);
        let (mut state, _task) = init(shared.clone());

        let _ = update(&mut state, Message::StartRangeBand(0));
        assert_eq!(state.selected_band, Some(0));
        assert_eq!(shared.params.get(ParamId::B1Range), 0.0);

        let _ = update(&mut state, Message::DragRangeBand(0, 4.0));
        assert_eq!(shared.params.get(ParamId::B1Range), 4.0);
    }

    #[test]
    fn right_drag_threshold_starts_from_default() {
        let shared = Arc::new(SharedState::default());
        shared.set_param_outbound_only(ParamId::B1Threshold, -18.0);
        let (mut state, _task) = init(shared.clone());

        let _ = update(&mut state, Message::StartThresholdBand(0));
        assert_eq!(state.selected_band, Some(0));
        assert_eq!(shared.params.get(ParamId::B1Threshold), -3.0);

        let _ = update(&mut state, Message::DragThresholdBand(0, -3.5));
        assert_eq!(shared.params.get(ParamId::B1Threshold), -6.5);
    }

    #[test]
    fn reset_gain_band_sets_makeup_to_zero() {
        let shared = Arc::new(SharedState::default());
        shared.set_param_outbound_only(ParamId::BandCount, 2.0);
        shared.set_param_outbound_only(ParamId::B2Makeup, 7.0);
        let (mut state, _task) = init(shared.clone());

        let _ = update(&mut state, Message::ResetGainBand(1));

        assert_eq!(state.selected_band, Some(1));
        assert_eq!(shared.params.get(ParamId::B2Makeup), 0.0);
    }

    #[test]
    fn reset_threshold_band_sets_threshold_to_default() {
        let shared = Arc::new(SharedState::default());
        shared.set_param_outbound_only(ParamId::BandCount, 2.0);
        shared.set_param_outbound_only(ParamId::B2Threshold, -18.0);
        let (mut state, _task) = init(shared.clone());

        let _ = update(&mut state, Message::ResetThresholdBand(1));

        assert_eq!(state.selected_band, Some(1));
        assert_eq!(shared.params.get(ParamId::B2Threshold), -3.0);
    }

    #[test]
    fn gain_curve_double_click_resets_clicked_band() {
        let curves = threshold_curves(ThresholdCurveParams {
            splits: vec![200.0, 2000.0, 8000.0],
            band_count: 4,
            input_gain_db: 0.0,
            sc_boost: 0,
            sample_rate: 48_000.0,
            thresholds: [-10.0; MAX_COMPRESSOR_BANDS],
            ranges: [-6.0; MAX_COMPRESSOR_BANDS],
            gains: [0.0; MAX_COMPRESSOR_BANDS],
            gain_reduction_db: [0.0; MAX_COMPRESSOR_BANDS],
            selected_band: Some(0),
        });

        let reset = curves[0].on_double_click_at.as_ref().unwrap()(500.0);
        match reset {
            Message::ResetGainBand(band) => assert_eq!(band, 1),
            other => panic!("expected reset message, got {other:?}"),
        }
        assert!(curves[1].on_double_click_at.is_none());
        assert!(curves[2].on_double_click_at.is_none());
        let start_threshold = curves[0].on_right_select_at.as_ref().unwrap()(500.0);
        match start_threshold {
            Message::StartThresholdBand(band) => assert_eq!(band, 1),
            other => panic!("expected start threshold message, got {other:?}"),
        }
    }

    #[test]
    fn live_gain_contour_ducks_whole_band_toward_negative_range() {
        let curves = threshold_curves(ThresholdCurveParams {
            splits: vec![200.0, 2000.0, 8000.0],
            band_count: 4,
            input_gain_db: 0.0,
            sc_boost: 0,
            sample_rate: 48_000.0,
            thresholds: [-10.0; MAX_COMPRESSOR_BANDS],
            ranges: [-6.0; MAX_COMPRESSOR_BANDS],
            gains: [0.0; MAX_COMPRESSOR_BANDS],
            gain_reduction_db: [3.0; MAX_COMPRESSOR_BANDS],
            selected_band: Some(0),
        });

        for &(_, db) in &curves[0].points {
            assert!(
                (db + 3.0).abs() < 1.0e-4,
                "expected live contour to duck to -3 dB, got {db}"
            );
        }
    }

    #[test]
    fn live_gain_contour_expands_whole_band_toward_positive_range() {
        let curves = threshold_curves(ThresholdCurveParams {
            splits: vec![200.0, 2000.0, 8000.0],
            band_count: 4,
            input_gain_db: 0.0,
            sc_boost: 0,
            sample_rate: 48_000.0,
            thresholds: [-10.0; MAX_COMPRESSOR_BANDS],
            ranges: [6.0; MAX_COMPRESSOR_BANDS],
            gains: [0.0; MAX_COMPRESSOR_BANDS],
            gain_reduction_db: [-4.0; MAX_COMPRESSOR_BANDS],
            selected_band: Some(0),
        });

        for &(_, db) in &curves[0].points {
            assert!(
                (db - 4.0).abs() < 1.0e-4,
                "expected live contour to expand to +4 dB, got {db}"
            );
        }
    }

    #[test]
    fn live_gain_contour_clamps_to_configured_range() {
        let curves = threshold_curves(ThresholdCurveParams {
            splits: vec![200.0, 2000.0, 8000.0],
            band_count: 4,
            input_gain_db: 0.0,
            sc_boost: 0,
            sample_rate: 48_000.0,
            thresholds: [-10.0; MAX_COMPRESSOR_BANDS],
            ranges: [-6.0; MAX_COMPRESSOR_BANDS],
            gains: [0.0; MAX_COMPRESSOR_BANDS],
            gain_reduction_db: [24.0; MAX_COMPRESSOR_BANDS],
            selected_band: Some(0),
        });

        for &(_, db) in &curves[0].points {
            assert!(
                (db + 6.0).abs() < 1.0e-4,
                "expected live contour to clamp at -6 dB, got {db}"
            );
        }
    }

    #[test]
    fn range_contour_draws_gain_plus_range() {
        let points =
            range_curve_for_values([-10.0; MAX_COMPRESSOR_BANDS], [-6.0; MAX_COMPRESSOR_BANDS]);
        for &(_, db) in &points {
            assert!(
                (db + 6.0).abs() < 1.0e-4,
                "expected flat -6 dB range contour, got {db}"
            );
        }

        let points =
            range_curve_for_values([-10.0; MAX_COMPRESSOR_BANDS], [4.0; MAX_COMPRESSOR_BANDS]);
        for &(_, db) in &points {
            assert!(
                (db - 4.0).abs() < 1.0e-4,
                "expected flat +4 dB range contour, got {db}"
            );
        }
    }

    #[test]
    fn gain_contour_draws_makeup_gain() {
        let points = gain_curve_for_values([3.0; MAX_COMPRESSOR_BANDS]);
        for &(_, db) in &points {
            assert!(
                (db - 3.0).abs() < 1.0e-4,
                "expected flat +3 dB gain contour, got {db}"
            );
        }

        let points = gain_curve_for_values([-5.5; MAX_COMPRESSOR_BANDS]);
        for &(_, db) in &points {
            assert!(
                (db + 5.5).abs() < 1.0e-4,
                "expected flat -5.5 dB gain contour, got {db}"
            );
        }
    }

    #[test]
    fn different_thresholds_ease_across_split() {
        let points = curve_for_thresholds([-20.0, -8.0, -8.0, -8.0, 0.0, 0.0]);
        let low = points
            .iter()
            .find(|(freq, _)| *freq > 90.0)
            .map(|(_, db)| *db)
            .unwrap();
        let split = points
            .iter()
            .find(|(freq, _)| *freq > 200.0)
            .map(|(_, db)| *db)
            .unwrap();
        let high = points
            .iter()
            .find(|(freq, _)| *freq > 500.0)
            .map(|(_, db)| *db)
            .unwrap();

        assert!(low < split && split < high);
        assert!(
            low < -14.0,
            "low band should stay near its threshold: {low}"
        );
        assert!(
            (-18.0..=-10.0).contains(&split),
            "split should be an eased blend, got {split}"
        );
        assert!(
            high > -10.0,
            "next band should ease toward its threshold: {high}"
        );
    }
}
