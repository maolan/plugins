use std::{
    ffi::CStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use maolan_baseview::iced::{
    Alignment, Color, Element, Length, Point, Rectangle, Size, Task, Theme,
    alignment::{Horizontal, Vertical},
    mouse::Cursor,
    widget::{canvas, checkbox, column, container, row, scrollable, text, toggler},
};
#[cfg(target_os = "macos")]
use maolan_clap::ffi::CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
use maolan_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use maolan_clap::ffi::CLAP_WINDOW_API_X11;
use maolan_widgets::multi_toggler::horizontal_multi_toggler;
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd"
))]
use raw_window_handle::RawWindowHandle;
use raw_window_handle::{HandleError, HasWindowHandle, WindowHandle};

use crate::{
    common::ui::{SmallKnob, small_knob},
    stereo::{
        params::{PARAMS, ParamId},
        plugin::{SharedState, VectorscopeData},
    },
};

pub const EDITOR_WIDTH: u32 = 910;
pub const EDITOR_HEIGHT: u32 = 760;
const SCOPE_WIDTH: f32 = 430.0;
const SCOPE_HEIGHT: f32 = SCOPE_WIDTH * 0.5;

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
    ReleaseParam(ParamId),
    ToggleGain(bool),
    ToggleDelay(bool),
    ToggleCharacter(bool),
    SetScopeMode(usize),
    Poll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeMode {
    PolarSample,
    PolarLevel,
    Lissajous,
}

impl ScopeMode {
    fn from_index(index: usize) -> Self {
        match index {
            1 => Self::PolarLevel,
            2 => Self::Lissajous,
            _ => Self::PolarSample,
        }
    }

    fn as_index(self) -> usize {
        match self {
            Self::PolarSample => 0,
            Self::PolarLevel => 1,
            Self::Lissajous => 2,
        }
    }
}

struct State {
    shared: Arc<SharedState>,
    active_gestures: Vec<bool>,
    scope_mode: ScopeMode,
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    (
        State {
            shared,
            active_gestures: vec![false; ParamId::COUNT],
            scope_mode: ScopeMode::PolarSample,
        },
        Task::none(),
    )
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::SetParam(id, value) => {
            let discrete = matches!(
                id,
                ParamId::SoloLow | ParamId::SoloMid | ParamId::SoloHigh | ParamId::MonitorMode
            );
            if discrete {
                state.shared.mark_gesture_begin_pending(id);
                state.shared.set_param_outbound_only(id, value as f64);
                state.shared.mark_gesture_end_pending(id);
                return Task::none();
            }
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
        Message::ToggleGain(on) => {
            state.shared.set_gain_on(on);
        }
        Message::ToggleDelay(on) => {
            state.shared.set_delay_on(on);
        }
        Message::ToggleCharacter(on) => {
            state.shared.set_character_on(on);
        }
        Message::SetScopeMode(index) => {
            state.scope_mode = ScopeMode::from_index(index);
        }
        Message::Poll => {}
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;
    let b = |id: ParamId| state.shared.params.get(id) >= 0.5;

    let mut scope_data = VectorscopeData::default();
    let _ = state.shared.vectorscope.read(&mut scope_data);

    let mut controls = column![].spacing(16).align_x(Alignment::Start);

    controls = controls.push(
        row![
            text("Gain").size(14),
            text("Off").size(13),
            toggler(state.shared.gain_on()).on_toggle(Message::ToggleGain),
            text("On").size(13),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );
    controls = controls.push(
        row![
            container(
                column![
                    knob("Low", ParamId::LowGain, p(ParamId::LowGain), "", 1.0),
                    checkbox(b(ParamId::SoloLow)).label("Solo").on_toggle(|v| {
                        Message::SetParam(ParamId::SoloLow, if v { 1.0 } else { 0.0 })
                    })
                ]
                .spacing(6)
                .align_x(Alignment::Center),
            )
            .width(Length::Fixed(90.0)),
            container(
                column![
                    knob("Mid", ParamId::MidGain, p(ParamId::MidGain), "", 1.0),
                    checkbox(b(ParamId::SoloMid)).label("Solo").on_toggle(|v| {
                        Message::SetParam(ParamId::SoloMid, if v { 1.0 } else { 0.0 })
                    })
                ]
                .spacing(6)
                .align_x(Alignment::Center),
            )
            .width(Length::Fixed(90.0)),
            container(
                column![
                    knob("High", ParamId::HighGain, p(ParamId::HighGain), "", 1.0),
                    checkbox(b(ParamId::SoloHigh)).label("Solo").on_toggle(|v| {
                        Message::SetParam(ParamId::SoloHigh, if v { 1.0 } else { 0.0 })
                    })
                ]
                .spacing(6)
                .align_x(Alignment::Center),
            )
            .width(Length::Fixed(90.0)),
        ]
        .spacing(16),
    );
    controls = controls.push(
        row![
            knob("X1", ParamId::X1, p(ParamId::X1), "Hz", 1.0),
            knob("X2", ParamId::X2, p(ParamId::X2), "Hz", 1.0),
            knob("Boost", ParamId::Boost, p(ParamId::Boost), "x", 0.01),
        ]
        .spacing(16),
    );

    controls = controls.push(
        row![
            text("Delay").size(14),
            text("Off").size(13),
            toggler(state.shared.delay_on()).on_toggle(Message::ToggleDelay),
            text("On").size(13),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );
    controls = controls.push(
        row![
            knob("Strength", ParamId::Strength, p(ParamId::Strength), "", 0.1),
            knob("Low", ParamId::LowDelay, p(ParamId::LowDelay), "", 1.0),
            knob("Mid", ParamId::MidDelay, p(ParamId::MidDelay), "", 1.0),
            knob("High", ParamId::HighDelay, p(ParamId::HighDelay), "", 1.0),
        ]
        .spacing(16),
    );

    controls = controls.push(
        row![
            text("Character").size(14),
            text("Off").size(13),
            toggler(state.shared.character_on()).on_toggle(Message::ToggleCharacter),
            text("On").size(13),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );
    controls = controls.push(
        row![
            knob("Density", ParamId::Density, p(ParamId::Density), "", 0.01),
            knob("Focus", ParamId::Focus, p(ParamId::Focus), "", 0.01),
            knob("Amount", ParamId::Amount, p(ParamId::Amount), "", 0.01),
        ]
        .spacing(16),
    );

    controls = controls.push(text("Output").size(18));
    let monitor_selected = match p(ParamId::MonitorMode) as i32 {
        1 => 1,
        2 => 2,
        _ => 0,
    };
    controls = controls.push(
        row![
            knob(
                "Volume",
                ParamId::OutputGain,
                p(ParamId::OutputGain),
                "dB",
                0.1
            ),
            column![
                text("Mode"),
                horizontal_multi_toggler(["Stereo", "Mono", "Side"], monitor_selected, |index| {
                    Message::SetParam(ParamId::MonitorMode, index as f32)
                })
            ],
        ]
        .spacing(16),
    );

    let scope = column![
        container(
            canvas(Vectorscope {
                data: scope_data,
                mode: state.scope_mode,
            })
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fixed(SCOPE_WIDTH))
        .height(Length::Fixed(SCOPE_HEIGHT)),
        horizontal_multi_toggler(
            ["Polar Sample", "Polar Level", "Lissajous"],
            state.scope_mode.as_index(),
            Message::SetScopeMode,
        )
        .label_extent(96.0),
    ]
    .spacing(12)
    .align_x(Alignment::Center);

    let content = row![controls, scope].spacing(24).align_y(Alignment::Start);

    container(scrollable(content))
        .padding(24)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Left)
        .align_y(Vertical::Top)
        .into()
}

struct Vectorscope {
    data: VectorscopeData,
    mode: ScopeMode,
}

impl Vectorscope {
    fn lissajous_point(&self, index: usize, center: Point, scale: f32) -> Point {
        let left = self.data.left[index];
        let right = self.data.right[index];
        let side = (left - right) * 0.5;
        let mid = (left + right) * 0.5;
        Point::new(center.x + side * scale, center.y - mid * scale)
    }

    fn polar_sample_point(
        &self,
        index: usize,
        origin: Point,
        radius_x: f32,
        radius_y: f32,
        peak: f32,
    ) -> Point {
        let left = self.data.left[index];
        let right = self.data.right[index];
        let side = ((left - right) * 0.5 / peak).clamp(-1.0, 1.0);
        let level = ((left * left + right * right) * 0.5).sqrt() / peak;
        Self::polar_point_from_side_level(side, level, origin, radius_x, radius_y)
    }

    fn polar_point_from_side_level(
        side: f32,
        level: f32,
        origin: Point,
        radius_x: f32,
        radius_y: f32,
    ) -> Point {
        let angle =
            std::f32::consts::FRAC_PI_2 - side.clamp(-1.0, 1.0) * std::f32::consts::FRAC_PI_2;
        let level = level.clamp(0.0, 1.0);
        Point::new(
            origin.x + angle.cos() * level * radius_x,
            origin.y - angle.sin() * level * radius_y,
        )
    }

    fn draw_arc(
        frame: &mut canvas::Frame,
        center: Point,
        radius_x: f32,
        radius_y: f32,
        color: Color,
        width: f32,
    ) {
        let arc = canvas::Path::new(|builder| {
            for i in 0..=72 {
                let phase = std::f32::consts::PI - i as f32 / 72.0 * std::f32::consts::PI;
                let point = Point::new(
                    center.x + phase.cos() * radius_x,
                    center.y - phase.sin() * radius_y,
                );
                if i == 0 {
                    builder.move_to(point);
                } else {
                    builder.line_to(point);
                }
            }
        });
        frame.stroke(
            &arc,
            canvas::Stroke::default()
                .with_color(color)
                .with_width(width),
        );
    }

    fn draw_scope_background(&self, frame: &mut canvas::Frame, width: f32, height: f32) {
        let bg = canvas::Path::rectangle(Point::ORIGIN, Size::new(width, height));
        frame.fill(&bg, Color::from_rgb(0.055, 0.060, 0.075));
        frame.stroke(
            &bg,
            canvas::Stroke::default()
                .with_color(Color::from_rgba(0.80, 0.86, 0.95, 0.18))
                .with_width(1.0),
        );
    }

    fn draw_polar_grid(
        &self,
        frame: &mut canvas::Frame,
        width: f32,
        height: f32,
    ) -> (Point, f32, f32) {
        let pad = 20.0;
        let radius_x = ((width - pad * 2.0) * 0.5).max(16.0);
        let radius_y = (height - pad * 2.0).max(16.0);
        let center = Point::new(width * 0.5, height - pad);

        for factor in [0.25, 0.5, 0.75, 1.0] {
            Self::draw_arc(
                frame,
                center,
                radius_x * factor,
                radius_y * factor,
                Color::from_rgba(0.67, 0.72, 0.82, 0.09),
                if factor == 1.0 { 1.0 } else { 0.7 },
            );
        }

        for degrees in [30.0_f32, 60.0, 90.0, 120.0, 150.0] {
            let phase = degrees.to_radians();
            let end = Point::new(
                center.x + phase.cos() * radius_x,
                center.y - phase.sin() * radius_y,
            );
            frame.stroke(
                &canvas::Path::line(center, end),
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(0.68, 0.72, 0.80, 0.10))
                    .with_width(if degrees == 90.0 { 1.0 } else { 0.6 }),
            );
        }

        frame.stroke(
            &canvas::Path::line(
                Point::new(center.x - radius_x, center.y),
                Point::new(center.x + radius_x, center.y),
            ),
            canvas::Stroke::default()
                .with_color(Color::from_rgba(0.88, 0.92, 0.98, 0.24))
                .with_width(1.0),
        );
        frame.stroke(
            &canvas::Path::line(center, Point::new(center.x, center.y - radius_y)),
            canvas::Stroke::default()
                .with_color(Color::from_rgba(0.88, 0.92, 0.98, 0.30))
                .with_width(1.0),
        );

        (center, radius_x, radius_y)
    }

    fn draw_lissajous_grid(
        &self,
        frame: &mut canvas::Frame,
        width: f32,
        height: f32,
    ) -> (Point, f32) {
        let pad = 18.0;
        let side = (width.min(height) - pad * 2.0).max(16.0);
        let left = (width - side) * 0.5;
        let top = (height - side) * 0.5;
        let center = Point::new(width * 0.5, height * 0.5);
        let radius = side * 0.5;

        for offset in [-0.5_f32, 0.0, 0.5] {
            let x = center.x + offset * radius;
            let y = center.y + offset * radius;
            frame.stroke(
                &canvas::Path::line(Point::new(x, top), Point::new(x, top + side)),
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(0.68, 0.72, 0.80, 0.10))
                    .with_width(if offset == 0.0 { 1.0 } else { 0.6 }),
            );
            frame.stroke(
                &canvas::Path::line(Point::new(left, y), Point::new(left + side, y)),
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(0.68, 0.72, 0.80, 0.10))
                    .with_width(if offset == 0.0 { 1.0 } else { 0.6 }),
            );
        }

        let diamond = canvas::Path::new(|builder| {
            builder.move_to(Point::new(center.x, top));
            builder.line_to(Point::new(left + side, center.y));
            builder.line_to(Point::new(center.x, top + side));
            builder.line_to(Point::new(left, center.y));
            builder.close();
        });
        frame.stroke(
            &diamond,
            canvas::Stroke::default()
                .with_color(Color::from_rgba(0.75, 0.80, 0.90, 0.16))
                .with_width(1.0),
        );

        (center, radius)
    }

    fn draw_polar_sample(
        &self,
        frame: &mut canvas::Frame,
        origin: Point,
        radius_x: f32,
        radius_y: f32,
        peak: f32,
    ) {
        let step = (self.data.len / 320).max(1);
        for i in (0..self.data.len).step_by(step) {
            let point = self.polar_sample_point(i, origin, radius_x, radius_y, peak);
            let side = (self.data.left[i] - self.data.right[i]) * 0.5;
            let color = if side < 0.0 {
                Color::from_rgba(1.00, 0.42, 0.22, 0.50)
            } else {
                Color::from_rgba(0.05, 0.78, 0.95, 0.50)
            };
            frame.fill(&canvas::Path::circle(point, 1.15), color);
        }
    }

    fn draw_polar_level(
        &self,
        frame: &mut canvas::Frame,
        origin: Point,
        radius_x: f32,
        radius_y: f32,
        peak: f32,
    ) {
        const BINS: usize = 129;
        let mut levels = [0.0_f32; BINS];
        for i in 0..self.data.len {
            let left = self.data.left[i];
            let right = self.data.right[i];
            let side = ((left - right) * 0.5 / peak).clamp(-1.0, 1.0);
            let level = ((left * left + right * right) * 0.5).sqrt() / peak;
            let bin = (((side + 1.0) * 0.5) * (BINS - 1) as f32).round() as usize;
            levels[bin] = levels[bin].max(level);
        }

        let fill = canvas::Path::new(|builder| {
            builder.move_to(origin);
            for (bin, level) in levels.iter().enumerate() {
                let side = bin as f32 / (BINS - 1) as f32 * 2.0 - 1.0;
                builder.line_to(Self::polar_point_from_side_level(
                    side,
                    level.sqrt(),
                    origin,
                    radius_x,
                    radius_y,
                ));
            }
            builder.line_to(origin);
            builder.close();
        });
        frame.fill(&fill, Color::from_rgba(0.05, 0.78, 0.95, 0.24));
        frame.stroke(
            &fill,
            canvas::Stroke::default()
                .with_color(Color::from_rgba(0.05, 0.78, 0.95, 0.62))
                .with_width(1.6),
        );
    }

    fn draw_lissajous(&self, frame: &mut canvas::Frame, center: Point, radius: f32, peak: f32) {
        let scale = radius * 0.94 / peak;
        let trace = canvas::Path::new(|builder| {
            builder.move_to(self.lissajous_point(0, center, scale));
            for i in 1..self.data.len {
                builder.line_to(self.lissajous_point(i, center, scale));
            }
        });
        frame.stroke(
            &trace,
            canvas::Stroke::default()
                .with_color(Color::from_rgba(1.00, 0.42, 0.22, 0.20))
                .with_width(5.0),
        );
        frame.stroke(
            &trace,
            canvas::Stroke::default()
                .with_color(Color::from_rgba(0.24, 0.62, 1.00, 0.18))
                .with_width(3.0),
        );

        let step = (self.data.len / 220).max(1);
        for i in (0..self.data.len).step_by(step) {
            let point = self.lissajous_point(i, center, scale);
            let side_signal = (self.data.left[i] - self.data.right[i]) * 0.5;
            let color = if side_signal < 0.0 {
                Color::from_rgba(1.00, 0.42, 0.22, 0.58)
            } else {
                Color::from_rgba(0.24, 0.62, 1.00, 0.58)
            };
            frame.fill(&canvas::Path::circle(point, 1.35), color);
        }
    }
}

impl canvas::Program<Message> for Vectorscope {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &maolan_baseview::iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let width = bounds.width;
        let height = bounds.height;
        self.draw_scope_background(&mut frame, width, height);

        let peak = self.data.peak.max(0.08);
        if self.data.len >= 2 && self.data.peak > 0.000_01 {
            match self.mode {
                ScopeMode::PolarSample => {
                    let (center, radius_x, radius_y) =
                        self.draw_polar_grid(&mut frame, width, height);
                    self.draw_polar_sample(&mut frame, center, radius_x, radius_y, peak);
                }
                ScopeMode::PolarLevel => {
                    let (center, radius_x, radius_y) =
                        self.draw_polar_grid(&mut frame, width, height);
                    self.draw_polar_level(&mut frame, center, radius_x, radius_y, peak);
                }
                ScopeMode::Lissajous => {
                    let (center, radius) = self.draw_lissajous_grid(&mut frame, width, height);
                    self.draw_lissajous(&mut frame, center, radius, peak);
                }
            }
        } else {
            match self.mode {
                ScopeMode::PolarSample | ScopeMode::PolarLevel => {
                    let _ = self.draw_polar_grid(&mut frame, width, height);
                }
                ScopeMode::Lissajous => {
                    let _ = self.draw_lissajous_grid(&mut frame, width, height);
                }
            }
        }

        vec![frame.into_geometry()]
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
    let value_text = pretty_value(id, value, units);

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

fn pretty_value(id: ParamId, value: f32, _units: &'static str) -> String {
    match id {
        ParamId::SoloLow | ParamId::SoloMid | ParamId::SoloHigh => {
            if value >= 0.5 {
                "On".to_string()
            } else {
                "Off".to_string()
            }
        }
        ParamId::MonitorMode => match value as i32 {
            1 => "Mono".to_string(),
            2 => "Side".to_string(),
            _ => "Stereo".to_string(),
        },
        ParamId::LowGain
        | ParamId::MidGain
        | ParamId::HighGain
        | ParamId::LowDelay
        | ParamId::MidDelay
        | ParamId::HighDelay => format!("{value:.0} %"),
        ParamId::Strength => format!("{value:.1}"),
        ParamId::Boost => format!("{value:.2}x"),
        ParamId::OutputGain => format!("{value:.1} dB"),
        ParamId::X1 | ParamId::X2 => format!("{value:.0} Hz"),
        ParamId::Density | ParamId::Focus | ParamId::Amount => format!("{value:.2}"),
    }
}

fn build_app(shared: Arc<SharedState>) -> impl maolan_baseview::iced::Program {
    maolan_baseview::iced::application(move || init(shared.clone()), update, view)
        .subscription(|_state| maolan_baseview::iced::poll_events().map(|_| Message::Poll))
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
        if let Some(shared) = &self.shared {
            *shared.poll_notifier.lock() = None;
        }
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
                title: String::from("Maolan Stereo"),
                size: maolan_baseview::iced::baseview::Size::new(
                    EDITOR_WIDTH as f64,
                    EDITOR_HEIGHT as f64,
                ),
                scale: maolan_baseview::iced::baseview::WindowScalePolicy::SystemScaleFactor,
            },
            ignore_non_modifier_keys: false,
            always_redraw: true,
        };
        let notifier = maolan_baseview::iced::PollSubNotifier::new();
        *shared.poll_notifier.lock() = Some(notifier.clone());

        let handle =
            maolan_baseview::iced::shell::open_parented(&parent, settings, notifier, move || {
                build_app(shared)
            });

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
                        title: String::from("Maolan Stereo"),
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
                let notifier = maolan_baseview::iced::PollSubNotifier::new();
                *shared.poll_notifier.lock() = Some(notifier.clone());
                maolan_baseview::iced::shell::open_blocking(settings, notifier, move || {
                    build_app(shared)
                });
                open_flag.store(false, Ordering::Release);
            });
        }
        true
    }

    pub fn hide(&mut self, shared: Arc<SharedState>) -> bool {
        *shared.poll_notifier.lock() = None;
        if self.floating {
            self.floating_open.store(false, Ordering::Release);
            shared.request_gui_closed();
            return true;
        }
        self.window_handle = None;
        true
    }
}
