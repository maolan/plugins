use std::{
    ffi::CStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

#[cfg(target_os = "macos")]
use clap_clap::ffi::CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
use clap_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(all(unix, not(target_os = "macos")))]
use clap_clap::ffi::CLAP_WINDOW_API_X11;
use maolan_baseview::iced::widget::canvas::{Frame, Geometry, Path, Program, Stroke, Text};
use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{button, canvas, column, container, row, text},
};
use maolan_widgets::arch_slider::arch_slider;
use maolan_widgets::meters::meters;
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};

use crate::kick::{
    params::{PARAMS, ParamId},
    plugin::SharedState,
};

pub const EDITOR_WIDTH: u32 = 820;
pub const EDITOR_HEIGHT: u32 = 600;

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

// ---------------------------------------------------------------------------
// Waveform Canvas — draws the actual synthesized kick buffer
// ---------------------------------------------------------------------------

struct WaveformState {
    shared: Arc<SharedState>,
}

impl Program<Message> for WaveformState {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &maolan_baseview::iced::Renderer,
        _theme: &Theme,
        bounds: maolan_baseview::iced::Rectangle,
        _cursor: maolan_baseview::iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let width = bounds.width;
        let height = bounds.height;

        // Background
        frame.fill_rectangle(
            maolan_baseview::iced::Point::new(0.0, 0.0),
            maolan_baseview::iced::Size::new(width, height),
            maolan_baseview::iced::Color::from_rgb(0.08, 0.08, 0.10),
        );

        // Grid lines
        for i in 1..5 {
            let y = height * i as f32 / 5.0;
            let grid_path = Path::line(
                maolan_baseview::iced::Point::new(0.0, y),
                maolan_baseview::iced::Point::new(width, y),
            );
            frame.stroke(
                &grid_path,
                Stroke::default()
                    .with_color(maolan_baseview::iced::Color::from_rgb(0.15, 0.15, 0.18))
                    .with_width(0.5),
            );
        }

        // Draw actual waveform from shared buffer
        let waveform = self.shared.waveform_display.lock();
        if !waveform.is_empty() {
            let center_y = height / 2.0;
            let samples = waveform.len();
            let peak = waveform
                .iter()
                .fold(0.0f32, |a, &b| a.max(b.abs()))
                .max(1.0e-12);
            let scale_y = (height * 0.45) / peak;

            let path = Path::new(|builder| {
                let first_x = 0.0f32;
                let first_y = center_y - waveform[0] * scale_y;
                builder.move_to(maolan_baseview::iced::Point::new(first_x, first_y));

                let step = (samples as f32 / width).max(1.0);
                let mut x = 0.0f32;
                while x < width {
                    let idx = ((x / width) * samples as f32) as usize;
                    let idx = idx.min(samples - 1);
                    let y = center_y - waveform[idx] * scale_y;
                    builder.line_to(maolan_baseview::iced::Point::new(x, y));
                    x += step.max(1.0);
                }
            });
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(maolan_baseview::iced::Color::from_rgb(0.2, 0.85, 0.4))
                    .with_width(1.5),
            );
        } else {
            // No waveform yet — draw center line
            let line = Path::line(
                maolan_baseview::iced::Point::new(0.0, height / 2.0),
                maolan_baseview::iced::Point::new(width, height / 2.0),
            );
            frame.stroke(
                &line,
                Stroke::default()
                    .with_color(maolan_baseview::iced::Color::from_rgb(0.25, 0.25, 0.28))
                    .with_width(1.0),
            );
        }

        // Title text
        frame.fill_text(Text {
            content: "Maolan Kick".to_string(),
            position: maolan_baseview::iced::Point::new(8.0, 14.0),
            color: maolan_baseview::iced::Color::from_rgb(0.7, 0.7, 0.7),
            size: 14.0.into(),
            font: maolan_baseview::iced::Font::DEFAULT,
            ..Text::default()
        });

        vec![frame.into_geometry()]
    }
}

// ---------------------------------------------------------------------------
// Envelope Canvas — draws the amplitude ADSR shape
// ---------------------------------------------------------------------------

struct EnvelopeState {
    shared: Arc<SharedState>,
}

impl Program<Message> for EnvelopeState {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &maolan_baseview::iced::Renderer,
        _theme: &Theme,
        bounds: maolan_baseview::iced::Rectangle,
        _cursor: maolan_baseview::iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let width = bounds.width;
        let height = bounds.height;

        // Background
        frame.fill_rectangle(
            maolan_baseview::iced::Point::new(0.0, 0.0),
            maolan_baseview::iced::Size::new(width, height),
            maolan_baseview::iced::Color::from_rgb(0.10, 0.10, 0.12),
        );

        // Grid
        for i in 1..4 {
            let y = height * i as f32 / 4.0;
            let grid = Path::line(
                maolan_baseview::iced::Point::new(0.0, y),
                maolan_baseview::iced::Point::new(width, y),
            );
            frame.stroke(
                &grid,
                Stroke::default()
                    .with_color(maolan_baseview::iced::Color::from_rgb(0.18, 0.18, 0.20))
                    .with_width(0.5),
            );
        }

        // Read envelope params
        let a = self.shared.params.get(ParamId::OscAmpEnvAttack) as f32;
        let d = self.shared.params.get(ParamId::OscAmpEnvDecay) as f32;
        let s = self.shared.params.get(ParamId::OscAmpEnvSustain) as f32;
        let r = self.shared.params.get(ParamId::OscAmpEnvRelease) as f32;
        let total = (a + d + r).max(1.0);

        // Draw envelope path
        let env_path = Path::new(|builder| {
            let x_a = (a / total) * width;
            let x_d = ((a + d) / total) * width;
            let y_s = height - s * height;

            builder.move_to(maolan_baseview::iced::Point::new(0.0, height));
            builder.line_to(maolan_baseview::iced::Point::new(x_a, 0.0));
            builder.line_to(maolan_baseview::iced::Point::new(x_d, y_s));
            builder.line_to(maolan_baseview::iced::Point::new(width, height));
        });
        frame.stroke(
            &env_path,
            Stroke::default()
                .with_color(maolan_baseview::iced::Color::from_rgb(0.9, 0.5, 0.2))
                .with_width(1.5),
        );

        // Fill under envelope
        let fill_path = Path::new(|builder| {
            let x_a = (a / total) * width;
            let x_d = ((a + d) / total) * width;
            let y_s = height - s * height;

            builder.move_to(maolan_baseview::iced::Point::new(0.0, height));
            builder.line_to(maolan_baseview::iced::Point::new(x_a, 0.0));
            builder.line_to(maolan_baseview::iced::Point::new(x_d, y_s));
            builder.line_to(maolan_baseview::iced::Point::new(width, height));
            builder.close();
        });
        frame.fill(
            &fill_path,
            maolan_baseview::iced::Color::from_rgba(0.9, 0.5, 0.2, 0.15),
        );

        // Label
        frame.fill_text(Text {
            content: "Amp Envelope".to_string(),
            position: maolan_baseview::iced::Point::new(6.0, 12.0),
            color: maolan_baseview::iced::Color::from_rgb(0.6, 0.6, 0.6),
            size: 10.0.into(),
            font: maolan_baseview::iced::Font::DEFAULT,
            ..Text::default()
        });

        vec![frame.into_geometry()]
    }
}

// ---------------------------------------------------------------------------
// GUI Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum Message {
    SetParam(ParamId, f32),
    ReleaseParam(ParamId),
    SetNoiseFilterType(u8),
    SetMasterFilterType(u8),
}

struct State {
    shared: Arc<SharedState>,
    active_gestures: Vec<bool>,
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    (
        State {
            shared,
            active_gestures: vec![false; ParamId::COUNT],
        },
        Task::none(),
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
        Message::SetNoiseFilterType(v) => {
            state
                .shared
                .set_param_outbound_only(ParamId::NoiseFilterType, v as f64);
        }
        Message::SetMasterFilterType(v) => {
            state
                .shared
                .set_param_outbound_only(ParamId::MasterFilterType, v as f64);
        }
    }
    Task::none()
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

fn view(state: &State) -> Element<'_, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;
    let peak_db = state.shared.output_peak_db();

    // Waveform display canvas
    let waveform = canvas(WaveformState {
        shared: state.shared.clone(),
    })
    .width(Length::Fill)
    .height(Length::Fixed(140.0));

    // Envelope graph
    let envelope_graph = canvas(EnvelopeState {
        shared: state.shared.clone(),
    })
    .width(Length::Fixed(160.0))
    .height(Length::Fixed(100.0));

    // Output meter
    let meter = container(meters(1, &[peak_db], 100.0))
        .height(Length::Fixed(100.0))
        .width(Length::Fixed(32.0));

    // Oscillator section
    let osc_section = column![
        section_header("OSCILLATOR"),
        row![
            knob(
                "Wave",
                ParamId::OscWaveform,
                p(ParamId::OscWaveform),
                "",
                1.0
            ),
            knob("Freq", ParamId::OscFreq, p(ParamId::OscFreq), "Hz", 1.0),
            knob("Amp", ParamId::OscAmp, p(ParamId::OscAmp), "", 0.01),
        ]
        .spacing(6),
        row![
            knob(
                "PStart",
                ParamId::OscPitchEnvStart,
                p(ParamId::OscPitchEnvStart),
                "Hz",
                1.0
            ),
            knob(
                "PEnd",
                ParamId::OscPitchEnvEnd,
                p(ParamId::OscPitchEnvEnd),
                "Hz",
                1.0
            ),
            knob(
                "PTime",
                ParamId::OscPitchEnvTime,
                p(ParamId::OscPitchEnvTime),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "Attack",
                ParamId::OscAmpEnvAttack,
                p(ParamId::OscAmpEnvAttack),
                "ms",
                0.1
            ),
            knob(
                "Decay",
                ParamId::OscAmpEnvDecay,
                p(ParamId::OscAmpEnvDecay),
                "ms",
                1.0
            ),
            knob(
                "Sust",
                ParamId::OscAmpEnvSustain,
                p(ParamId::OscAmpEnvSustain),
                "",
                0.01
            ),
        ]
        .spacing(6),
    ]
    .spacing(6);

    // Noise section
    let noise_section = column![
        section_header("NOISE"),
        row![
            knob("Amp", ParamId::NoiseAmp, p(ParamId::NoiseAmp), "", 0.01),
            knob(
                "Density",
                ParamId::NoiseDensity,
                p(ParamId::NoiseDensity),
                "",
                0.01
            ),
            knob("Type", ParamId::NoiseType, p(ParamId::NoiseType), "", 1.0),
        ]
        .spacing(6),
        row![
            knob(
                "Cutoff",
                ParamId::NoiseFilterCutoff,
                p(ParamId::NoiseFilterCutoff),
                "Hz",
                1.0
            ),
            knob(
                "Q",
                ParamId::NoiseFilterQ,
                p(ParamId::NoiseFilterQ),
                "",
                0.01
            ),
        ]
        .spacing(6),
        row![filter_type_buttons(
            p(ParamId::NoiseFilterType) as u8,
            Message::SetNoiseFilterType
        ),]
        .spacing(4),
        row![
            knob(
                "Attack",
                ParamId::NoiseAmpEnvAttack,
                p(ParamId::NoiseAmpEnvAttack),
                "ms",
                0.1
            ),
            knob(
                "Decay",
                ParamId::NoiseAmpEnvDecay,
                p(ParamId::NoiseAmpEnvDecay),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
    ]
    .spacing(6);

    // Master section
    let master_section = column![
        section_header("MASTER"),
        row![
            knob(
                "Dist",
                ParamId::Distortion,
                p(ParamId::Distortion),
                "",
                0.01
            ),
            knob(
                "Gain",
                ParamId::OutputGain,
                p(ParamId::OutputGain),
                "dB",
                0.1
            ),
            knob(
                "Length",
                ParamId::KickLength,
                p(ParamId::KickLength),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "MFCut",
                ParamId::MasterFilterCutoff,
                p(ParamId::MasterFilterCutoff),
                "Hz",
                1.0
            ),
            knob(
                "MFQ",
                ParamId::MasterFilterQ,
                p(ParamId::MasterFilterQ),
                "",
                0.01
            ),
        ]
        .spacing(6),
        row![filter_type_buttons(
            p(ParamId::MasterFilterType) as u8,
            Message::SetMasterFilterType
        ),]
        .spacing(4),
    ]
    .spacing(6);

    let top_row = row![
        waveform,
        column![envelope_graph, meter]
            .spacing(8)
            .align_x(Alignment::Center),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let controls = row![osc_section, noise_section, master_section,]
        .spacing(10)
        .align_y(Alignment::Start);

    let content = column![top_row, controls,]
        .spacing(10)
        .align_x(Alignment::Start);

    container(content)
        .padding(10)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Left)
        .align_y(Vertical::Top)
        .into()
}

fn theme(_state: &State) -> Theme {
    Theme::TokyoNight
}

fn section_header(label: &'static str) -> Element<'static, Message> {
    text(label).size(11).into()
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
    .width(Length::Fixed(56.0))
    .height(Length::Fixed(56.0));

    let value_text = if units.is_empty() {
        format!("{value:.2}")
    } else {
        format!("{value:.1} {units}")
    };

    container(
        column![text(label).size(10), slider, text(value_text).size(9)]
            .spacing(1)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(64.0))
    .into()
}

fn filter_type_buttons(
    current: u8,
    on_press: impl Fn(u8) -> Message + Clone + 'static,
) -> Element<'static, Message> {
    let lp_style = if current == 0 {
        button::primary
    } else {
        button::secondary
    };
    let bp_style = if current == 2 {
        button::primary
    } else {
        button::secondary
    };
    let hp_style = if current == 1 {
        button::primary
    } else {
        button::secondary
    };

    row![
        button(text("LP").size(10))
            .style(lp_style)
            .on_press(on_press(0)),
        button(text("BP").size(10))
            .style(bp_style)
            .on_press(on_press(2)),
        button(text("HP").size(10))
            .style(hp_style)
            .on_press(on_press(1)),
    ]
    .spacing(4)
    .into()
}

fn build_app(shared: Arc<SharedState>) -> impl maolan_baseview::iced::Program {
    maolan_baseview::iced::application(move || init(shared.clone()), update, view)
        .theme(theme)
        .run()
}

struct AnyWindowHandle {
    _inner: Box<dyn std::any::Any>,
}

unsafe impl Send for AnyWindowHandle {}

#[derive(Default)]
pub struct GuiBridge {
    created: bool,
    floating: bool,
    shared: Option<Arc<SharedState>>,
    floating_open: Arc<AtomicBool>,
    window_handle: Option<AnyWindowHandle>,
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
                title: String::from("Maolan Kick"),
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
                        title: String::from("Maolan Kick"),
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

    pub fn size(&self) -> (u32, u32) {
        (EDITOR_WIDTH, EDITOR_HEIGHT)
    }
}
