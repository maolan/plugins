use crate::common::bus;
use crate::eq::dsp::Biquad;
use crate::eq::params::{PARAMS, ParamId, ParamIdExt};
use crate::eq::plugin::{SPECTRUM_BINS, SharedState};
// menu widgets removed with sidechain source dropdown
use maolan_baseview::iced::{
    Alignment, Color, Element, Event, Length, Point, Rectangle, Renderer, Task, Theme,
    alignment::{Horizontal, Vertical},
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
    time::Duration,
};

pub const EDITOR_WIDTH: u32 = 800;
pub const EDITOR_HEIGHT: u32 = 680;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandType {
    LowPass,
    Bell,
    HighPass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slope {
    Db12,
    Db24,
    Db48,
    Db96,
}

impl std::fmt::Display for Slope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Slope::Db12 => write!(f, "12 dB/oct"),
            Slope::Db24 => write!(f, "24 dB/oct"),
            Slope::Db48 => write!(f, "48 dB/oct"),
            Slope::Db96 => write!(f, "96 dB/oct"),
        }
    }
}

impl From<u8> for Slope {
    fn from(v: u8) -> Self {
        match v {
            1 => Slope::Db24,
            2 => Slope::Db48,
            3 => Slope::Db96,
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
        }
    }
}

impl std::fmt::Display for BandType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BandType::LowPass => write!(f, "Low Pass"),
            BandType::Bell => write!(f, "Bell"),
            BandType::HighPass => write!(f, "High Pass"),
        }
    }
}

impl From<u8> for BandType {
    fn from(v: u8) -> Self {
        match v {
            0 => BandType::LowPass,
            2 => BandType::HighPass,
            _ => BandType::Bell,
        }
    }
}

impl From<BandType> for u8 {
    fn from(t: BandType) -> Self {
        match t {
            BandType::LowPass => 0,
            BandType::Bell => 1,
            BandType::HighPass => 2,
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
    SetBandFreqGain(usize, f32, f32),
    EndBandDrag(usize),
    SetBoolParam(ParamId, bool),
    ReleaseParam(ParamId),
    CreateBand(f32, f32),
    SelectBand(usize),
    DeselectBand,
    DeleteBand,
    SetChannels(ChannelMode),
    NoOp,
    UiTick,
}

struct State {
    shared: Arc<SharedState<ParamId>>,
    selected_band: Option<usize>,
    active_gestures: HashSet<ParamId>,
    /// Discovered non-EQ peers on the inter-plugin bus.
    bus_peers: Vec<Arc<bus::PluginSharedData>>,
    /// Per-band collision score (0.0 = none, 1.0 = heavy overlap with peer FFT).
    collision_scores: [f32; 32],
    /// Last seen registry version; re-discover only when this changes.
    last_registry_version: u64,
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
            active_gestures: HashSet::new(),
            bus_peers: Vec::new(),
            collision_scores: [0.0; 32],
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
            if state.active_gestures.insert(id) {
                state.shared.mark_gesture_begin_pending(id);
            }
            state.shared.set_param_outbound_only(id, value as f64);
        }
        Message::SetBandFreqGain(index, freq, gain) => {
            let freq = freq.clamp(20.0, 20_000.0);
            let gain = gain.clamp(-24.0, 24.0);
            let fid = ParamId::para_freq(index);
            let gid = ParamId::para_gain(index);
            if state.active_gestures.insert(fid) {
                state.shared.mark_gesture_begin_pending(fid);
            }
            if state.active_gestures.insert(gid) {
                state.shared.mark_gesture_begin_pending(gid);
            }
            state.shared.set_param_outbound_only(fid, freq as f64);
            state.shared.set_param_outbound_only(gid, gain as f64);
        }
        Message::EndBandDrag(index) => {
            let fid = ParamId::para_freq(index);
            let gid = ParamId::para_gain(index);
            if state.active_gestures.remove(&fid) {
                state.shared.mark_gesture_end_pending(fid);
            }
            if state.active_gestures.remove(&gid) {
                state.shared.mark_gesture_end_pending(gid);
            }
        }
        Message::SetBoolParam(id, value) => {
            if state.active_gestures.insert(id) {
                state.shared.mark_gesture_begin_pending(id);
            }
            state
                .shared
                .set_param_outbound_only(id, if value { 1.0 } else { 0.0 });
            if id == ParamId::SidechainEnable {
                // Sidechain changed → may affect collision-relevant frequencies.
                state.last_registry_version = 0;
            }
        }
        Message::ReleaseParam(id) => {
            if state.active_gestures.remove(&id) {
                state.shared.mark_gesture_end_pending(id);
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
                    let q = if gain >= 0.0 {
                        1.0 + (gain / 24.0) * 2.0
                    } else {
                        1.0 + (gain.abs() / 24.0) * 9.0
                    };
                    state.shared.set_param_outbound_only(oid, 1.0);
                    state.shared.set_param_outbound_only(fid, freq as f64);
                    state.shared.set_param_outbound_only(gid, gain as f64);
                    state.shared.set_param_outbound_only(qid, q as f64);
                    state.shared.set_param_outbound_only(tid, 1.0); // Bell default
                    state.selected_band = Some(i);
                    break;
                }
            }
            // Band count changed → may affect collision-relevant frequencies.
            state.last_registry_version = 0;
        }
        Message::SelectBand(index) => {
            state.selected_band = Some(index);
        }
        Message::DeselectBand => {
            state.selected_band = None;
        }
        Message::DeleteBand => {
            if let Some(sb) = state.selected_band {
                let oid = ParamId::para_on(sb);
                state.shared.set_param_outbound_only(oid, 0.0);
                state.selected_band = None;
            }
            // Band count changed → may affect collision-relevant frequencies.
            state.last_registry_version = 0;
        }
        Message::SetChannels(mode) => {
            state
                .shared
                .set_param_outbound_only(ParamId::Channels, u32::from(mode) as f64);
            state.shared.request_audio_ports_rescan();
        }
        Message::NoOp => {}
        Message::UiTick => {
            // Re-discover peers only when the registry has changed.
            let version = bus::registry_version();
            if version != state.last_registry_version {
                state.bus_peers = bus::discover(|p| p.plugin_type != bus::PluginType::Eq);
                state.last_registry_version = version;
            }

            // Read peer FFTs and compute collision scores for each EQ band.
            state.collision_scores.fill(0.0);
            let mut peer_fft = bus::FftData::default();
            for peer in &state.bus_peers {
                if let Some(ref slot) = peer.fft_slot {
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
                            // Cut bands don't cause audible collisions.
                            continue;
                        }
                        // Find the FFT bin closest to this band frequency.
                        let bin_idx = ((freq / nyquist) * peer_fft.valid_bins as f32)
                            .clamp(0.0, (peer_fft.valid_bins - 1) as f32)
                            as usize;
                        let db = peer_fft.bins[bin_idx];
                        if db > -60.0 {
                            // Normalize collision score: -60 dB -> 0.0, 0 dB -> 1.0.
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
        ));
    }

    let output_spectrum_db = state.shared.output_spectrum_db();
    let sample_rate = state.shared.sample_rate();
    let response = eq_response_graph(
        bands.clone(),
        output_spectrum_db,
        state.selected_band,
        sample_rate,
        is_listen_on,
        state.collision_scores,
    );

    let channels = p(ParamId::Channels).round() as u32;
    let channels_dropdown = maolan_baseview::iced::widget::pick_list(
        vec![ChannelMode::Mono, ChannelMode::Stereo],
        Some(ChannelMode::from(channels)),
        Message::SetChannels,
    )
    .placeholder("Channels");

    let knobs: Element<'_, Message> = if let Some(sb) = state.selected_band {
        let band_type = BandType::from(p(ParamId::para_type(sb)) as u8);
        let type_dropdown = maolan_baseview::iced::widget::pick_list(
            vec![BandType::LowPass, BandType::Bell, BandType::HighPass],
            Some(band_type),
            move |t| Message::SetParam(ParamId::para_type(sb), u8::from(t) as f32),
        )
        .placeholder("Type")
        .width(Length::Fixed(100.0));

        let slope = Slope::from(p(ParamId::para_slope(sb)) as u8);
        let slope_dropdown = maolan_baseview::iced::widget::pick_list(
            vec![Slope::Db12, Slope::Db24, Slope::Db48, Slope::Db96],
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
                Message::UiTick // dummy message to force refresh
            });

        if matches!(band_type, BandType::LowPass | BandType::HighPass) {
            row![
                channels_dropdown,
                type_dropdown,
                slope_dropdown,
                freq_knob(ParamId::para_freq(sb), p(ParamId::para_freq(sb))),
                knob(
                    "Q".to_string(),
                    ParamId::para_q(sb),
                    p(ParamId::para_q(sb)),
                    "",
                    0.01
                ),
                listen_checkbox,
                maolan_baseview::iced::widget::button("Delete").on_press(Message::DeleteBand),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into()
        } else {
            let dyn_knob = knob(
                "Dyn".to_string(),
                ParamId::para_dyn(sb),
                p(ParamId::para_dyn(sb)),
                "",
                0.01,
            );
            row![
                channels_dropdown,
                type_dropdown,
                freq_knob(ParamId::para_freq(sb), p(ParamId::para_freq(sb))),
                knob(
                    "Gain".to_string(),
                    ParamId::para_gain(sb),
                    p(ParamId::para_gain(sb)),
                    "dB",
                    0.1
                ),
                knob(
                    "Q".to_string(),
                    ParamId::para_q(sb),
                    p(ParamId::para_q(sb)),
                    "",
                    0.01
                ),
                dyn_knob,
                listen_checkbox,
                maolan_baseview::iced::widget::button("Delete").on_press(Message::DeleteBand),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into()
        }
    } else {
        row![
            channels_dropdown,
            text("Click a band or empty space to create one").size(12)
        ]
        .spacing(20)
        .align_y(Alignment::Center)
        .into()
    };

    let sidechain_enabled = p(ParamId::SidechainEnable) >= 0.5;

    let sidechain_enable_toggle = maolan_baseview::iced::widget::checkbox(sidechain_enabled)
        .label("Sidechain")
        .on_toggle(|v| Message::SetBoolParam(ParamId::SidechainEnable, v));

    let sidechain_row = row![
        sidechain_enable_toggle,
        knob(
            "Threshold".to_string(),
            ParamId::SidechainThreshold,
            p(ParamId::SidechainThreshold),
            "dB",
            0.1
        ),
        knob(
            "Ratio".to_string(),
            ParamId::SidechainRatio,
            p(ParamId::SidechainRatio),
            ":1",
            0.1
        ),
        knob(
            "Attack".to_string(),
            ParamId::SidechainAttackMs,
            p(ParamId::SidechainAttackMs),
            "ms",
            0.1
        ),
        knob(
            "Release".to_string(),
            ParamId::SidechainReleaseMs,
            p(ParamId::SidechainReleaseMs),
            "ms",
            1.0
        ),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let content = column![response, knobs, sidechain_row]
        .spacing(20)
        .align_x(Alignment::Center);

    container(content)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center)
        .into()
}

#[derive(Default, Debug)]
struct EqResponseState {
    dragging: Option<usize>,
    hover_pos: Option<Point>,
}

#[derive(Clone)]
struct EqResponseCanvas {
    bands: Vec<(usize, f32, f32, f32, bool, u8, u8)>,
    output_spectrum_db: [f32; SPECTRUM_BINS],
    selected_band: Option<usize>,
    sample_rate: f32,
    listen_mode: bool,
    collision_scores: [f32; 32],
}

impl EqResponseCanvas {
    const F_MIN: f32 = 20.0;
    const F_MAX: f32 = 20_000.0;
    const G_MIN: f32 = -24.0;
    const G_MAX: f32 = 24.0;
    const S_MIN: f32 = -90.0;
    const S_MAX: f32 = 0.0;

    fn freq_to_x(freq: f32, bounds: Rectangle) -> f32 {
        let f = freq.clamp(Self::F_MIN, Self::F_MAX);
        let t = (f / Self::F_MIN).ln() / (Self::F_MAX / Self::F_MIN).ln();
        bounds.x + t * bounds.width
    }

    fn x_to_freq(x: f32, bounds: Rectangle) -> f32 {
        let t = ((x - bounds.x) / bounds.width).clamp(0.0, 1.0);
        Self::F_MIN * (Self::F_MAX / Self::F_MIN).powf(t)
    }

    fn gain_to_y(gain: f32, bounds: Rectangle) -> f32 {
        let g = gain.clamp(Self::G_MIN, Self::G_MAX);
        let t = (g - Self::G_MIN) / (Self::G_MAX - Self::G_MIN);
        bounds.y + (1.0 - t) * bounds.height
    }

    fn y_to_gain(y: f32, bounds: Rectangle) -> f32 {
        let t = (1.0 - ((y - bounds.y) / bounds.height)).clamp(0.0, 1.0);
        Self::G_MIN + t * (Self::G_MAX - Self::G_MIN)
    }

    fn spectrum_to_y(db: f32, bounds: Rectangle) -> f32 {
        let s = db.clamp(Self::S_MIN, Self::S_MAX);
        let t = (s - Self::S_MIN) / (Self::S_MAX - Self::S_MIN);
        bounds.y + (1.0 - t) * bounds.height
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
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(p) = cursor.position_in(bounds) {
                    let mut closest = None;
                    let mut best_d2 = 12.0_f32 * 12.0_f32;
                    for (local_idx, (_global_idx, freq, gain, _q, on, _typ, _slope)) in
                        self.bands.iter().enumerate()
                    {
                        if !*on {
                            continue;
                        }
                        let x = Self::freq_to_x(*freq, local_bounds);
                        let y = Self::gain_to_y(*gain, local_bounds);
                        let dx = p.x - x;
                        let dy = p.y - y;
                        let d2 = dx * dx + dy * dy;
                        if d2 <= best_d2 {
                            best_d2 = d2;
                            closest = Some(local_idx);
                        }
                    }
                    if let Some(local_idx) = closest {
                        state.dragging = Some(local_idx);
                        let global_idx = self.bands[local_idx].0;
                        if Some(global_idx) != self.selected_band {
                            return Some(
                                CanvasAction::publish(Message::SelectBand(global_idx))
                                    .and_capture(),
                            );
                        }
                        return Some(CanvasAction::capture());
                    } else {
                        for (local_idx, (_global_idx, _freq, _gain, _q, on, _typ, _slope)) in
                            self.bands.iter().enumerate()
                        {
                            if !*on {
                                let freq = Self::x_to_freq(p.x, local_bounds);
                                let gain = Self::y_to_gain(p.y, local_bounds);
                                state.dragging = Some(local_idx);
                                return Some(
                                    CanvasAction::publish(Message::CreateBand(freq, gain))
                                        .and_capture(),
                                );
                            }
                        }
                        return Some(CanvasAction::publish(Message::DeselectBand).and_capture());
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(local_idx) = state.dragging.take()
                    && let Some((global_idx, _freq, _gain, _q, _on, _typ, _slope)) =
                        self.bands.get(local_idx).copied()
                {
                    return Some(CanvasAction::publish(Message::EndBandDrag(global_idx)));
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(p) = cursor.position_in(bounds) {
                    state.hover_pos = Some(p);
                    if let Some(local_idx) = state.dragging
                        && let Some((global_idx, _freq, _gain, _q, _on, _typ, _slope)) =
                            self.bands.get(local_idx).copied()
                    {
                        let freq = Self::x_to_freq(p.x, local_bounds);
                        let gain = Self::y_to_gain(p.y, local_bounds);
                        return Some(
                            CanvasAction::publish(Message::SetBandFreqGain(global_idx, freq, gain))
                                .and_capture(),
                        );
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
            Color::from_rgb(0.10, 0.11, 0.14),
        );

        let h_grid_db = [-24.0_f32, -18.0, -12.0, -6.0, 0.0, 6.0, 12.0, 18.0, 24.0];
        for db in h_grid_db {
            let y = Self::gain_to_y(
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
        }

        let spectrum_fill = Path::new(|b| {
            let mut first = true;
            let last = SPECTRUM_BINS.saturating_sub(1);
            for i in (0..SPECTRUM_BINS).step_by(2) {
                let db = self.output_spectrum_db[i];
                let t = i as f32 / (SPECTRUM_BINS.saturating_sub(1).max(1) as f32);
                let x = t * bounds.width;
                let y = Self::spectrum_to_y(
                    db,
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
            if last % 2 == 1 {
                let t = last as f32 / (SPECTRUM_BINS.saturating_sub(1).max(1) as f32);
                let x = t * bounds.width;
                let y = Self::spectrum_to_y(
                    self.output_spectrum_db[last],
                    Rectangle {
                        x: 0.0,
                        y: 0.0,
                        ..bounds
                    },
                );
                if first {
                    b.move_to(Point::new(x, y));
                } else {
                    b.line_to(Point::new(x, y));
                }
            }
            b.line_to(Point::new(bounds.width, bounds.height));
            b.line_to(Point::new(0.0, bounds.height));
            b.close();
        });
        frame.fill(&spectrum_fill, Color::from_rgba(0.0, 0.85, 0.3, 0.15));

        // Pre-build biquad chains for active bands
        let band_biquads: Vec<(usize, Vec<Biquad>)> = self
            .bands
            .iter()
            .filter_map(|(idx, f0, gain, q, on, typ, slope)| {
                if !on {
                    return None;
                }
                let n = match *slope {
                    1 => 2,
                    2 => 4,
                    3 => 8,
                    _ => 1,
                };
                let mut chain = Vec::with_capacity(n);
                for _ in 0..n {
                    let mut b = Biquad::default();
                    match *typ {
                        0 => b.set_lowpass(self.sample_rate, *f0, *q),
                        2 => b.set_highpass(self.sample_rate, *f0, *q),
                        _ => b.set_peaking(self.sample_rate, *f0, *q, *gain),
                    }
                    chain.push(b);
                }
                Some((*idx, chain))
            })
            .collect();

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
                for (idx, chain) in &band_biquads {
                    if self.listen_mode && Some(*idx) != self.selected_band {
                        continue;
                    }
                    for bq in chain {
                        total_lin *= 10.0_f32.powf(bq.magnitude_db(freq, self.sample_rate) * 0.05);
                    }
                }
                let total_db = 20.0 * total_lin.max(1.0e-12).log10();
                let y = Self::gain_to_y(
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
                .with_color(Color::from_rgb(0.53, 0.88, 0.98))
                .with_width(2.0),
        );

        if state.dragging.is_none()
            && self.bands.iter().any(|(_, _, _, _, on, _, _)| !*on)
            && let Some(hover) = state.hover_pos
        {
            let hover_freq = Self::x_to_freq(hover.x, local_bounds);
            let hover_gain = Self::y_to_gain(hover.y, local_bounds);
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
                    for (_idx, chain) in &band_biquads {
                        for bq in chain {
                            total_lin *=
                                10.0_f32.powf(bq.magnitude_db(freq, self.sample_rate) * 0.05);
                        }
                    }
                    // Add hover preview (single peaking biquad)
                    let mut hover_bq = Biquad::default();
                    hover_bq.set_peaking(self.sample_rate, hover_freq, hover_q, hover_gain);
                    total_lin *=
                        10.0_f32.powf(hover_bq.magnitude_db(freq, self.sample_rate) * 0.05);

                    let total_db = 20.0 * total_lin.max(1.0e-12).log10();
                    let y = Self::gain_to_y(
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

        for (global_idx, freq, gain, _q, on, _typ, _slope) in self.bands.iter() {
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
            let y = Self::gain_to_y(
                *gain,
                Rectangle {
                    x: 0.0,
                    y: 0.0,
                    ..bounds
                },
            );
            let is_selected = Some(*global_idx) == self.selected_band;
            let collision = self
                .collision_scores
                .get(*global_idx)
                .copied()
                .unwrap_or(0.0);
            let node = Path::circle(Point::new(x, y), if is_selected { 6.0 } else { 4.5 });
            frame.fill(
                &node,
                if is_selected {
                    Color::from_rgb(1.0, 0.85, 0.3)
                } else {
                    Color::from_rgb(0.95, 0.64, 0.18)
                },
            );
            // Collision indicator: red outline proportional to collision score.
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
            } else {
                frame.stroke(
                    &node,
                    canvas::Stroke::default()
                        .with_color(Color::from_rgb(0.16, 0.16, 0.18))
                        .with_width(1.0),
                );
            }

            let label = format_freq(*freq);
            let label_x = (x - 22.0).clamp(0.0, (bounds.width - 48.0).max(0.0));
            let label_y = (y - 12.0).max(10.0);
            frame.fill_text(Text {
                content: label,
                position: Point::new(label_x, label_y),
                color: Color::from_rgb(0.95, 0.95, 0.98),
                size: 10.0.into(),
                ..Text::default()
            });
        }

        vec![frame.into_geometry()]
    }
}

fn eq_response_graph(
    bands: Vec<(usize, f32, f32, f32, bool, u8, u8)>,
    output_spectrum_db: [f32; SPECTRUM_BINS],
    selected_band: Option<usize>,
    sample_rate: f32,
    listen_mode: bool,
    collision_scores: [f32; 32],
) -> Element<'static, Message> {
    canvas(EqResponseCanvas {
        bands,
        output_spectrum_db,
        selected_band,
        sample_rate,
        listen_mode,
        collision_scores,
    })
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

    pub fn hide(&mut self, shared: Arc<SharedState<ParamId>>) -> bool {
        shared.set_ui_visible(false);
        if self.floating {
            self.floating_open.store(false, Ordering::Release);
            shared.request_gui_closed();
            return true;
        }
        self.window_handle = None;
        true
    }
}
#[cfg(target_os = "macos")]
use clap_clap::ffi::CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
use clap_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(all(unix, not(target_os = "macos")))]
use clap_clap::ffi::CLAP_WINDOW_API_X11;
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};

pub fn preferred_api() -> &'static CStr {
    #[cfg(target_os = "windows")]
    {
        CLAP_WINDOW_API_WIN32
    }
    #[cfg(target_os = "macos")]
    {
        CLAP_WINDOW_API_COCOA
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
    #[cfg(all(unix, not(target_os = "macos")))]
    X11(u64),
    #[cfg(target_os = "macos")]
    Cocoa(*mut std::ffi::c_void),
    #[cfg(target_os = "windows")]
    Win32(*mut std::ffi::c_void),
}

unsafe impl HasRawWindowHandle for ParentWindowHandle {
    fn raw_window_handle(&self) -> RawWindowHandle {
        match self {
            #[cfg(all(unix, not(target_os = "macos")))]
            ParentWindowHandle::X11(window) => {
                let mut handle = raw_window_handle::XlibWindowHandle::empty();
                handle.window = *window;
                RawWindowHandle::Xlib(handle)
            }
            #[cfg(target_os = "macos")]
            ParentWindowHandle::Cocoa(ns_view) => {
                let mut handle = raw_window_handle::AppKitWindowHandle::empty();
                handle.ns_view = *ns_view;
                RawWindowHandle::AppKit(handle)
            }
            #[cfg(target_os = "windows")]
            ParentWindowHandle::Win32(hwnd) => {
                let mut handle = raw_window_handle::Win32WindowHandle::empty();
                handle.hwnd = *hwnd;
                RawWindowHandle::Win32(handle)
            }
        }
    }
}

pub struct AnyWindowHandle {
    pub _inner: Box<dyn std::any::Any>,
}

unsafe impl Send for AnyWindowHandle {}
