use std::{
    ffi::CStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use maolan_widgets::multi_toggler::horizontal_multi_toggler;

use maolan_baseview::iced::{
    Alignment, Color, Element, Length, Point, Rectangle, Renderer, Task, Theme,
    alignment::{Horizontal, Vertical},
    mouse,
    widget::{
        canvas,
        canvas::{Frame, Geometry, Path, Program, Stroke},
        checkbox, column, container, row, text,
    },
};
#[cfg(target_os = "macos")]
use maolan_clap::ffi::CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
use maolan_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use maolan_clap::ffi::CLAP_WINDOW_API_X11;
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd"
))]
use raw_window_handle::RawWindowHandle;
use raw_window_handle::{HandleError, HasWindowHandle, WindowHandle};

use crate::{
    common::ui::{
        SmallKnob, VerticalSlider, small_knob, vertical_slider, vertical_ticks, vu_meter,
    },
    limiter::{
        params::{PARAMS, ParamId},
        plugin::{SharedState, WAVEFORM_POINTS},
    },
};

pub const EDITOR_WIDTH: u32 = 900;
pub const EDITOR_HEIGHT: u32 = 520;
const DISPLAY_LATENCY_POINTS: f32 = 8.0;
const GRAPH_EDGE_INSET: f32 = 8.0;

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
    #[cfg(target_os = "macos")]
    {
        CLAP_WINDOW_API_COCOA
    }
}

pub fn is_api_supported(api: &CStr, _is_floating: bool) -> bool {
    api == preferred_api()
}

pub enum ParentWindowHandle {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    X11(u64),
    #[cfg(target_os = "macos")]
    Cocoa(*mut std::ffi::c_void),
    #[cfg(target_os = "windows")]
    Win32(*mut std::ffi::c_void),
}

impl HasWindowHandle for ParentWindowHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        match self {
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
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
            #[cfg(target_os = "macos")]
            ParentWindowHandle::Cocoa(ns_view) => {
                let handle = raw_window_handle::AppKitWindowHandle::new(
                    std::ptr::NonNull::new(*ns_view).expect("null NSView"),
                );
                Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::AppKit(handle)) })
            }
        }
    }
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum Message {
    SetParam(ParamId, f32),
    SetChannels(ChannelMode),
    SetShowOriginal(bool),
    SetShowProcessed(bool),
    ReleaseParam(ParamId),
    UiTick,
}

struct State {
    shared: Arc<SharedState>,
    active_gestures: Vec<bool>,
    visual_write_position: f32,
    display_initialized: bool,
    last_ui_tick: Instant,
    show_original_signal: bool,
    show_processed_signal: bool,
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    (
        State {
            shared,
            active_gestures: vec![false; ParamId::COUNT],
            visual_write_position: 0.0,
            display_initialized: false,
            last_ui_tick: Instant::now(),
            show_original_signal: true,
            show_processed_signal: false,
        },
        next_ui_tick_task(),
    )
}

fn next_ui_tick_task() -> Task<Message> {
    Task::perform(
        async move {
            thread::sleep(Duration::from_millis(16));
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
        Message::SetChannels(mode) => {
            state
                .shared
                .set_param_outbound_only(ParamId::Channels, u32::from(mode) as f64);
            state.shared.request_audio_ports_rescan();
        }
        Message::SetShowOriginal(show) => {
            state.show_original_signal = show;
        }
        Message::SetShowProcessed(show) => {
            state.show_processed_signal = show;
        }
        Message::UiTick => {
            let now = Instant::now();
            let elapsed = now.duration_since(state.last_ui_tick).as_secs_f32();
            state.last_ui_tick = now;
            let (write_position, bins_per_second) = state.shared.display_timing();
            let target_position = (write_position - DISPLAY_LATENCY_POINTS).max(0.0);
            if state.display_initialized {
                let next_position = state.visual_write_position + bins_per_second * elapsed;
                state.visual_write_position = if state.visual_write_position <= target_position {
                    next_position.min(target_position)
                } else {
                    target_position
                };
            } else {
                state.visual_write_position = target_position;
                state.display_initialized = true;
            }
            return next_ui_tick_task();
        }
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;
    let channels = state.shared.params.get_enum(ParamId::Channels).clamp(1, 2);
    let peaks = PeakHistoryWidget::<WAVEFORM_POINTS>::new(
        state.shared.clone(),
        state.visual_write_position,
        state.show_original_signal,
        state.show_processed_signal,
        channels >= 2,
    )
    .view();

    let mut controls = column![].spacing(10).align_x(Alignment::Start);
    let channels_selected = if channels >= 2 { 1 } else { 0 };
    let channels_toggle =
        horizontal_multi_toggler(["Mono", "Stereo"], channels_selected, |index| {
            Message::SetChannels(if index >= 1 {
                ChannelMode::Stereo
            } else {
                ChannelMode::Mono
            })
        });
    let original_checkbox = checkbox(state.show_original_signal)
        .label("Original")
        .on_toggle(Message::SetShowOriginal);
    let processed_checkbox = checkbox(state.show_processed_signal)
        .label("Processed")
        .on_toggle(Message::SetShowProcessed);

    controls = controls.push(
        row![
            channels_toggle,
            text("Display").size(16),
            original_checkbox,
            processed_checkbox,
            knob("Ceiling", ParamId::Ceiling, p(ParamId::Ceiling), "dB", 0.1),
            knob(
                "Lookahead",
                ParamId::Lookahead,
                p(ParamId::Lookahead),
                "ms",
                0.1
            ),
            knob("Attack", ParamId::Attack, p(ParamId::Attack), "ms", 0.1),
            knob("Release", ParamId::Release, p(ParamId::Release), "ms", 1.0),
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    );

    controls = controls.push(
        row![
            text("Channel Linking").size(16),
            knob(
                "Transients",
                ParamId::LinkTransients,
                p(ParamId::LinkTransients),
                "%",
                1.0
            ),
            knob(
                "Release",
                ParamId::LinkRelease,
                p(ParamId::LinkRelease),
                "%",
                1.0
            ),
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    );

    let output_control: Element<'_, Message> = row![
        vertical_ticks(),
        gain_slider(ParamId::OutputGain, p(ParamId::OutputGain), "dB", 0.1),
    ]
    .spacing(8)
    .height(Length::Fill)
    .align_y(Alignment::Center)
    .into();

    let display_row = row![
        gain_slider(ParamId::Boost, p(ParamId::Boost), "dB", 0.1),
        vertical_ticks(),
        vu_meter(channels as usize, state.shared.input_levels_db()),
        peaks,
        vu_meter(channels as usize, state.shared.output_levels_db()),
        output_control,
    ]
    .spacing(8)
    .height(Length::Fill)
    .align_y(Alignment::Center);

    container(column![display_row, controls].spacing(14))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Left)
        .align_y(Vertical::Center)
        .into()
}

#[derive(Clone)]
struct PeakHistoryWidget<const POINTS: usize> {
    shared: Arc<SharedState>,
    visual_write_position: f32,
    show_original: bool,
    show_processed: bool,
    stereo: bool,
}

impl<const POINTS: usize> PeakHistoryWidget<POINTS> {
    const MIN_DB: f32 = -60.0;
    const MAX_DB: f32 = 0.0;

    fn new(
        shared: Arc<SharedState>,
        visual_write_position: f32,
        show_original: bool,
        show_processed: bool,
        stereo: bool,
    ) -> Self {
        Self {
            shared,
            visual_write_position,
            show_original,
            show_processed,
            stereo,
        }
    }

    fn view<Message: 'static>(self) -> Element<'static, Message> {
        canvas(self).width(Length::Fill).height(Length::Fill).into()
    }

    fn peak_to_y(peak: f32, bounds: Rectangle) -> f32 {
        let db = if peak > 0.0 {
            20.0 * peak.log10()
        } else {
            Self::MIN_DB
        }
        .clamp(Self::MIN_DB, Self::MAX_DB);
        let t = (db - Self::MIN_DB) / (Self::MAX_DB - Self::MIN_DB);
        bounds.y + (1.0 - t) * bounds.height
    }

    fn reduction_to_y(reduction_db: f32, bounds: Rectangle) -> f32 {
        let t = (reduction_db / -Self::MIN_DB).clamp(0.0, 1.0);
        bounds.y + t * bounds.height
    }

    fn ring_index(sample_index: i64) -> usize {
        sample_index.rem_euclid(POINTS as i64) as usize
    }

    fn point_spacing(bounds: Rectangle) -> f32 {
        bounds.width / POINTS.saturating_sub(1).max(1) as f32
    }

    fn background_color() -> Color {
        Color::from_rgb(0.098, 0.098, 0.106)
    }

    fn bucket_to_x(bucket_index: f32, visual_write_position: f32, bounds: Rectangle) -> f32 {
        let spacing = Self::point_spacing(bounds);
        let distance_from_playhead = visual_write_position - bucket_index;
        bounds.x + bounds.width - distance_from_playhead * spacing
    }

    fn bucket_range(visual_write_position: f32) -> (i64, i64) {
        let newest_bucket = visual_write_position.floor() as i64 + 1;
        let oldest_bucket = (newest_bucket - POINTS as i64 - 1).max(0);
        (oldest_bucket, newest_bucket)
    }

    fn bucket_stride(visual_write_position: f32, bounds: Rectangle) -> i64 {
        let (oldest_bucket, newest_bucket) = Self::bucket_range(visual_write_position);
        let bucket_count = (newest_bucket - oldest_bucket + 1).max(1) as usize;
        let point_budget = (bounds.width.ceil() as usize).max(2);
        bucket_count.div_ceil(point_budget).max(1) as i64
    }

    fn peak_points(
        load_peak: &impl Fn(usize) -> f32,
        visual_write_position: f32,
        bounds: Rectangle,
    ) -> Vec<Point> {
        let (oldest_bucket, newest_bucket) = Self::bucket_range(visual_write_position);
        let stride = Self::bucket_stride(visual_write_position, bounds);
        let capacity = ((newest_bucket - oldest_bucket + stride) / stride).max(1) as usize;
        let mut points = Vec::with_capacity(capacity);
        let mut bucket = oldest_bucket;
        while bucket <= newest_bucket {
            let end_bucket = (bucket + stride - 1).min(newest_bucket);
            let mut peak = 0.0_f32;
            for i in bucket..=end_bucket {
                peak = peak.max(load_peak(Self::ring_index(i)));
            }
            let center_bucket = (bucket + end_bucket) as f32 * 0.5;
            points.push(Point::new(
                Self::bucket_to_x(center_bucket, visual_write_position, bounds),
                Self::peak_to_y(peak, bounds),
            ));
            bucket = end_bucket + 1;
        }
        points
    }

    fn draw_peak_points(points: &[Point], builder: &mut canvas::path::Builder) {
        let Some(&first) = points.first() else {
            return;
        };
        builder.move_to(first);
        for &point in &points[1..] {
            builder.line_to(point);
        }
    }

    fn draw_step_points(points: &[Point], builder: &mut canvas::path::Builder) {
        let Some(&first) = points.first() else {
            return;
        };
        builder.move_to(first);
        for pair in points.windows(2) {
            let previous = pair[0];
            let next = pair[1];
            builder.line_to(Point::new(next.x, previous.y));
            builder.line_to(next);
        }
    }

    fn reduction_points(
        load_reduction: &impl Fn(usize) -> f32,
        visual_write_position: f32,
        bounds: Rectangle,
    ) -> Vec<Point> {
        let (oldest_bucket, newest_bucket) = Self::bucket_range(visual_write_position);
        let stride = Self::bucket_stride(visual_write_position, bounds);
        let capacity = ((newest_bucket - oldest_bucket + stride) / stride).max(1) as usize;
        let mut points = Vec::with_capacity(capacity);
        let mut bucket = oldest_bucket;
        while bucket <= newest_bucket {
            let end_bucket = (bucket + stride - 1).min(newest_bucket);
            let mut reduction_db = 0.0_f32;
            for i in bucket..=end_bucket {
                reduction_db = reduction_db.max(load_reduction(Self::ring_index(i)));
            }
            let center_bucket = (bucket + end_bucket) as f32 * 0.5;
            points.push(Point::new(
                Self::bucket_to_x(center_bucket, visual_write_position, bounds),
                Self::reduction_to_y(reduction_db, bounds),
            ));
            bucket = end_bucket + 1;
        }
        points
    }

    fn peak_path_from_points(points: &[Point]) -> Path {
        Path::new(|builder| {
            Self::draw_peak_points(points, builder);
        })
    }

    fn peak_fill_path_from_points(points: &[Point], bounds: Rectangle) -> Path {
        Path::new(|builder| {
            Self::draw_peak_points(points, builder);
            let Some(first) = points.first() else {
                return;
            };
            let Some(last) = points.last() else {
                return;
            };
            let baseline = bounds.y + bounds.height;
            builder.line_to(Point::new(last.x, baseline));
            builder.line_to(Point::new(first.x, baseline));
            builder.close();
        })
    }

    fn reduction_path_from_points(points: &[Point]) -> Path {
        Path::new(|builder| {
            Self::draw_step_points(points, builder);
        })
    }

    fn reduction_fill_path_from_points(points: &[Point], bounds: Rectangle) -> Path {
        Path::new(|builder| {
            Self::draw_step_points(points, builder);
            let Some(first) = points.first() else {
                return;
            };
            let Some(last) = points.last() else {
                return;
            };
            builder.line_to(Point::new(last.x, bounds.y));
            builder.line_to(Point::new(first.x, bounds.y));
            builder.close();
        })
    }

    fn draw_background(frame: &mut Frame, bounds: Rectangle) {
        frame.fill(
            &Path::rectangle(Point::new(0.0, 0.0), bounds.size()),
            Self::background_color(),
        );
    }

    fn draw_overlay(frame: &mut Frame, bounds: Rectangle, graph_inset: f32) {
        let right_mask_x = (bounds.width - graph_inset).max(0.0);
        for mask in [
            Path::rectangle(
                Point::new(0.0, 0.0),
                maolan_baseview::iced::Size::new(graph_inset, bounds.height),
            ),
            Path::rectangle(
                Point::new(right_mask_x, 0.0),
                maolan_baseview::iced::Size::new(graph_inset, bounds.height),
            ),
        ] {
            frame.fill(&mask, Self::background_color());
        }

        for db in [Self::MIN_DB, -45.0, -30.0, -15.0, Self::MAX_DB] {
            let y = Self::peak_to_y(10.0_f32.powf(db / 20.0), bounds);
            let line = Path::line(Point::new(0.0, y), Point::new(bounds.width, y));
            let color = if db == Self::MAX_DB {
                Color::from_rgba(0.85, 0.87, 0.90, 0.28)
            } else {
                Color::from_rgba(0.72, 0.76, 0.82, 0.12)
            };
            frame.stroke(&line, Stroke::default().with_color(color).with_width(1.0));
        }

        for i in 1..8 {
            let x = bounds.width * i as f32 / 8.0;
            let line = Path::line(Point::new(x, 0.0), Point::new(x, bounds.height));
            frame.stroke(
                &line,
                Stroke::default()
                    .with_color(Color::from_rgba(0.72, 0.76, 0.82, 0.10))
                    .with_width(1.0),
            );
        }
    }
}

#[derive(Default)]
struct PeakHistoryCanvasState {
    background: canvas::Cache<Renderer>,
    overlay: canvas::Cache<Renderer>,
}

impl<Message, const POINTS: usize> Program<Message> for PeakHistoryWidget<POINTS> {
    type State = PeakHistoryCanvasState;

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
        let graph_inset = GRAPH_EDGE_INSET.min(bounds.width * 0.25);
        let graph_bounds = Rectangle {
            x: graph_inset,
            y: 0.0,
            width: (bounds.width - graph_inset * 2.0).max(1.0),
            height: bounds.height,
        };
        let background = state.background.draw(renderer, bounds.size(), |frame| {
            PeakHistoryWidget::<POINTS>::draw_background(frame, local_bounds);
        });
        let mut frame = Frame::new(renderer, bounds.size());

        let channels = if self.stereo { 2 } else { 1 };
        for channel in 0..channels {
            let load_reduction = |index| self.shared.reduction_sample(channel, index);
            let points = PeakHistoryWidget::<POINTS>::reduction_points(
                &load_reduction,
                self.visual_write_position,
                graph_bounds,
            );
            let fill =
                PeakHistoryWidget::<POINTS>::reduction_fill_path_from_points(&points, graph_bounds);
            frame.fill(&fill, Color::from_rgba(1.0, 0.16, 0.12, 0.08));
            let line = PeakHistoryWidget::<POINTS>::reduction_path_from_points(&points);
            frame.stroke(
                &line,
                Stroke::default()
                    .with_color(Color::from_rgba(1.0, 0.18, 0.14, 0.46))
                    .with_width(1.2),
            );
        }

        if self.show_original {
            for channel in 0..channels {
                let load_peak = |index| self.shared.input_peak_sample(channel, index);
                let points = PeakHistoryWidget::<POINTS>::peak_points(
                    &load_peak,
                    self.visual_write_position,
                    graph_bounds,
                );
                let fill =
                    PeakHistoryWidget::<POINTS>::peak_fill_path_from_points(&points, graph_bounds);
                frame.fill(&fill, Color::from_rgba(0.72, 0.74, 0.78, 0.05));
                let line = PeakHistoryWidget::<POINTS>::peak_path_from_points(&points);
                frame.stroke(
                    &line,
                    Stroke::default()
                        .with_color(Color::from_rgba(0.78, 0.80, 0.84, 0.34))
                        .with_width(1.2),
                );
            }
        }

        if self.show_processed {
            for channel in 0..channels {
                let load_peak = |index| self.shared.output_peak_sample(channel, index);
                let points = PeakHistoryWidget::<POINTS>::peak_points(
                    &load_peak,
                    self.visual_write_position,
                    graph_bounds,
                );
                let fill =
                    PeakHistoryWidget::<POINTS>::peak_fill_path_from_points(&points, graph_bounds);
                frame.fill(&fill, Color::from_rgba(0.48, 0.50, 0.55, 0.05));
                let line = PeakHistoryWidget::<POINTS>::peak_path_from_points(&points);
                frame.stroke(
                    &line,
                    Stroke::default()
                        .with_color(Color::from_rgba(0.56, 0.58, 0.64, 0.30))
                        .with_width(1.2),
                );
            }
        }

        let overlay = state.overlay.draw(renderer, bounds.size(), |frame| {
            PeakHistoryWidget::<POINTS>::draw_overlay(frame, local_bounds, graph_inset);
        });

        vec![background, frame.into_geometry(), overlay]
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
    let value_text = match id {
        ParamId::Boost | ParamId::Ceiling | ParamId::OutputGain if units == "dB" => {
            format!("{value:.1} {units}")
        }
        ParamId::Lookahead | ParamId::Attack | ParamId::Release if units == "ms" => {
            format!("{value:.1} {units}")
        }
        ParamId::LinkTransients | ParamId::LinkRelease if units == "%" => {
            format!("{value:.0}{units}")
        }
        _ => {
            if units.is_empty() {
                format!("{value:.2}")
            } else {
                format!("{value:.1} {units}")
            }
        }
    };

    small_knob(
        SmallKnob {
            label: label.to_string(),
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
    let value_text = match id {
        ParamId::Boost | ParamId::Ceiling | ParamId::OutputGain if units == "dB" => {
            format!("{value:.1} {units}")
        }
        ParamId::Lookahead | ParamId::Attack | ParamId::Release if units == "ms" => {
            format!("{value:.1} {units}")
        }
        ParamId::LinkTransients | ParamId::LinkRelease if units == "%" => {
            format!("{value:.0}{units}")
        }
        _ => {
            if units.is_empty() {
                format!("{value:.2}")
            } else {
                format!("{value:.1} {units}")
            }
        }
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
                title: String::from("Maolan Limiter"),
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
                        title: String::from("Maolan Limiter"),
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
