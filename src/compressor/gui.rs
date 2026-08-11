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
        SpectrumThresholdCurve,
    },
};

#[cfg(target_os = "windows")]
use clap_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(unix)]
use clap_clap::ffi::CLAP_WINDOW_API_X11;
use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{checkbox, column, container, row, scrollable, text},
};
use maolan_widgets::arch_slider::arch_slider;
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
    SelectThresholdBand(usize),
    DragThresholdBand(usize, f32),
    ReleaseThresholdBand(usize),
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
            last_registry_version: 0,
        },
        next_ui_tick_task(),
    )
}

fn next_ui_tick_task() -> Task<Message> {
    Task::perform(
        async move {
            thread::sleep(Duration::from_millis(500));
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
        Message::SelectThresholdBand(band) => {
            state.selected_band =
                Some(band.min(active_band_count(&state.shared).saturating_sub(1)));
        }
        Message::DragThresholdBand(band, delta_db) => {
            let id = threshold_param_for_band(band);
            let idx = id.as_index();
            if !state.active_gestures[idx] {
                state.active_gestures[idx] = true;
                state.shared.mark_gesture_begin_pending(id);
            }
            let value = state.shared.params.get(id) + f64::from(delta_db);
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

            if let Some(peer) = &state.compressor_peer {
                if let Some(slot) = peer.fft_slot() {
                    let mut fft = bus::FftData::default();
                    if slot.read(&mut fft) && fft.valid_bins > 0 {
                        let n = fft.valid_bins.min(DEFAULT_SPECTRUM_BINS);
                        state.spectrum_db[0][..n].copy_from_slice(&fft.bins[..n]);
                        state.spectrum_db[1][..n].copy_from_slice(&fft.bins[..n]);
                    }
                }
                if let Some(slot) = peer.gr_slot() {
                    let mut gr = bus::CompressorGrData::default();
                    if slot.read(&mut gr) {
                        state.gain_reduction_db.clear();
                        for i in 0..gr.valid_bands.min(gr.gr_db.len()) {
                            state.gain_reduction_db.push(gr.gr_db[i]);
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
    let b = |id: ParamId| state.shared.params.get_bool(id);
    let band_count = active_band_count(&state.shared);
    let splits = active_splits(&state.shared);

    let mut content = column![].spacing(12).align_x(Alignment::Start);
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
        selected_band: state.selected_band,
    }))
    .with_gain_reduction(state.gain_reduction_db.clone())
    .on_double_click(|freq, _db| Message::CreateBand(freq))
    .view();

    content = content.push(analyzer);

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
        content = content.push(row![text(format!("EQ bands: {freq_text}")).size(12)].spacing(4));
    }

    content = content.push(band_strip(state));

    let sc_mode = state.shared.params.get_enum(ParamId::ScMode).min(1);
    let mode = state.shared.params.get_enum(ParamId::Mode).min(1);
    let sc_boost = state.shared.params.get_enum(ParamId::ScBoost).min(4);
    let topology = state.shared.params.get_enum(ParamId::Topology).min(1);
    content = content.push(
        row![
            text("Sidechain").size(16),
            maolan_baseview::iced::widget::radio(
                "Peak",
                0u8,
                Some(sc_mode as u8),
                Message::SetScMode
            ),
            maolan_baseview::iced::widget::radio(
                "RMS",
                1u8,
                Some(sc_mode as u8),
                Message::SetScMode
            ),
            checkbox(b(ParamId::Bypass))
                .label("Bypass")
                .on_toggle(|v| Message::SetBoolParam(ParamId::Bypass, v)),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    );
    content = content.push(
        row![
            text("Mode").size(16),
            maolan_baseview::iced::widget::radio(
                "Compress",
                0u8,
                Some(mode as u8),
                Message::SetMode
            ),
            maolan_baseview::iced::widget::radio("Expand", 1u8, Some(mode as u8), Message::SetMode),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    );
    content = content.push(
        row![
            text("SC Boost").size(16),
            maolan_baseview::iced::widget::radio(
                "Off",
                0u8,
                Some(sc_boost as u8),
                Message::SetScBoost
            ),
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
        .align_y(Alignment::Center),
    );
    content = content.push(
        row![
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
        .align_y(Alignment::Center),
    );

    content = content.push(
        row![
            channels_dropdown,
            knob(
                "Input",
                ParamId::InputGain,
                p(ParamId::InputGain),
                "dB",
                0.1
            ),
            knob(
                "Output",
                ParamId::OutputGain,
                p(ParamId::OutputGain),
                "dB",
                0.1
            ),
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
        .align_y(Alignment::Center),
    );

    container(scrollable(content))
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

fn band_section<'a>(band: usize, state: &'a State, ids: BandIds) -> Element<'a, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;
    container(
        column![
            text(format!("Band {}", band + 1)).size(16),
            row![
                knob("Th", ids.th, p(ids.th), "dB", 0.1),
                knob("Range", ids.range, p(ids.range), "dB", 0.1),
            ]
            .spacing(4),
            row![
                knob("Ratio", ids.ratio, p(ids.ratio), "", 0.1),
                knob("Knee", ids.knee, p(ids.knee), "dB", 0.1),
            ]
            .spacing(4),
            row![
                knob("Atk", ids.att, p(ids.att), "ms", 0.1),
                knob("Rel", ids.rel, p(ids.rel), "ms", 0.1),
            ]
            .spacing(4),
            knob("Makeup", ids.makeup, p(ids.makeup), "dB", 0.1),
        ]
        .spacing(6)
        .align_x(Alignment::Center),
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

struct ThresholdCurveParams {
    splits: Vec<f32>,
    band_count: usize,
    input_gain_db: f32,
    sc_boost: u32,
    sample_rate: f32,
    thresholds: [f32; MAX_COMPRESSOR_BANDS],
    selected_band: Option<usize>,
}

fn threshold_curves(params: ThresholdCurveParams) -> Vec<SpectrumThresholdCurve<Message>> {
    const F_MIN: f32 = 20.0;
    const F_MAX: f32 = 20_000.0;
    const POINTS: usize = 192;

    let band_count = params.band_count.clamp(1, MAX_COMPRESSOR_BANDS);
    let splits = sorted_splits(&params.splits, band_count);
    (0..band_count)
        .map(|band| {
            let mut points = Vec::with_capacity(POINTS);
            let boost_db = sidechain_boost_db(params.sc_boost, band);
            for point in 0..POINTS {
                let t = point as f32 / (POINTS - 1) as f32;
                let freq = F_MIN * (F_MAX / F_MIN).powf(t);
                let response_db =
                    detector_band_response_db(band, freq, &splits, params.sample_rate);
                points.push((
                    freq,
                    params.thresholds[band] - params.input_gain_db - boost_db - response_db,
                ));
            }
            SpectrumThresholdCurve {
                points,
                selected: params.selected_band == Some(band),
                on_select: Some(Message::SelectThresholdBand(band)),
                on_drag_db: Some(Arc::new(move |delta_db| {
                    Message::DragThresholdBand(band, delta_db)
                })),
                on_release: Some(Message::ReleaseThresholdBand(band)),
            }
        })
        .collect()
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

fn sidechain_boost_db(sc_boost: u32, band: usize) -> f32 {
    match sc_boost {
        1 if band == 0 => 3.0,
        2 if band <= 1 => 3.0,
        3 if band == 0 => 6.0,
        4 if band <= 1 => 6.0,
        _ => 0.0,
    }
}

fn detector_band_response_db(band: usize, freq: f32, splits: &[f32], sample_rate: f32) -> f32 {
    let mut response = 1.0;
    for split in splits.iter().take(band) {
        response *= lr4_highpass_magnitude(freq, *split, sample_rate);
    }
    if let Some(split) = splits.get(band) {
        response *= lr4_lowpass_magnitude(freq, *split, sample_rate);
    }
    20.0 * response.max(1.0e-6).log10()
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
    let def = PARAMS[id.as_index()];
    let slider = arch_slider(def.min as f32..=def.max as f32, value, move |v| {
        Message::SetParam(id, v)
    })
    .step(step)
    .double_click_reset(def.default as f32)
    .on_release(Message::ReleaseParam(id))
    .fill_from_start()
    .width(Length::Fixed(41.0))
    .height(Length::Fixed(41.0));

    let value_text = if units.is_empty() {
        format!("{value:.2}")
    } else if units == "Hz" {
        format!("{value:.0} {units}")
    } else {
        format!("{value:.1} {units}")
    };

    container(
        column![text(label).size(11), slider, text(value_text).size(10)]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(50.0))
    .into()
}

fn build_app(shared: Arc<SharedState>) -> impl maolan_baseview::iced::Program {
    maolan_baseview::iced::application(move || init(shared.clone()), update, view)
        .font(iced_fonts::LUCIDE_FONT_BYTES)
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

    pub fn hide(&mut self, shared: Arc<SharedState>) -> bool {
        if self.floating {
            self.floating_open.store(false, Ordering::Release);
            shared.request_gui_closed();
            return true;
        }
        self.window_handle = None;
        true
    }
}
