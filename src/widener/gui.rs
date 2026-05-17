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
use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{checkbox, column, container, radio, row, scrollable, text},
};
use maolan_widgets::arch_slider::arch_slider;
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};

use crate::widener::{
    params::{PARAMS, ParamId},
    plugin::SharedState,
};

pub const EDITOR_WIDTH: u32 = 600;
pub const EDITOR_HEIGHT: u32 = 500;

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

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum Message {
    SetParam(ParamId, f32),
    ReleaseParam(ParamId),
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
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;
    let b = |id: ParamId| state.shared.params.get(id) >= 0.5;

    let mut content = column![].spacing(16).align_x(Alignment::Start);

    content = content.push(
        row![
            container(
                column![
                    knob("Low", ParamId::Low, p(ParamId::Low), "", 1.0),
                    checkbox(b(ParamId::SoloLow)).label("Solo").on_toggle(|v| {
                        Message::SetParam(ParamId::SoloLow, if v { 1.0 } else { 0.0 })
                    })
                ]
                .spacing(6)
                .align_x(Alignment::Center),
            )
            .width(Length::Fixed(96.0)),
            container(
                column![
                    knob("Mid", ParamId::Mid, p(ParamId::Mid), "", 1.0),
                    checkbox(b(ParamId::SoloMid)).label("Solo").on_toggle(|v| {
                        Message::SetParam(ParamId::SoloMid, if v { 1.0 } else { 0.0 })
                    })
                ]
                .spacing(6)
                .align_x(Alignment::Center),
            )
            .width(Length::Fixed(96.0)),
            container(
                column![
                    knob("High", ParamId::High, p(ParamId::High), "", 1.0),
                    checkbox(b(ParamId::SoloHigh)).label("Solo").on_toggle(|v| {
                        Message::SetParam(ParamId::SoloHigh, if v { 1.0 } else { 0.0 })
                    })
                ]
                .spacing(6)
                .align_x(Alignment::Center),
            )
            .width(Length::Fixed(96.0)),
            knob(
                "Strength",
                ParamId::Strength,
                p(ParamId::Strength),
                "ms",
                0.1
            ),
        ]
        .spacing(16),
    );
    content = content.push(
        row![
            knob("X1", ParamId::X1, p(ParamId::X1), "Hz", 1.0),
            knob("X2", ParamId::X2, p(ParamId::X2), "Hz", 1.0),
            knob("Boost", ParamId::Boost, p(ParamId::Boost), "x", 0.01),
            knob(
                "Output",
                ParamId::OutputGain,
                p(ParamId::OutputGain),
                "dB",
                0.1
            ),
        ]
        .spacing(16),
    );
    let monitor_selected = match p(ParamId::MonitorMode) as i32 {
        1 => Some(1u8),
        2 => Some(2u8),
        _ => Some(0u8),
    };
    content = content.push(
        row![
            text("Monitor").size(14),
            radio("Stereo", 0u8, monitor_selected, |v| {
                Message::SetParam(ParamId::MonitorMode, v as f32)
            }),
            radio("Mono", 1u8, monitor_selected, |v| {
                Message::SetParam(ParamId::MonitorMode, v as f32)
            }),
            radio("Side", 2u8, monitor_selected, |v| {
                Message::SetParam(ParamId::MonitorMode, v as f32)
            }),
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    );

    container(scrollable(content))
        .padding(24)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Left)
        .align_y(Vertical::Top)
        .into()
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
    .width(Length::Fixed(86.0))
    .height(Length::Fixed(86.0));

    let value_text = pretty_value(id, value, units);

    container(
        column![text(label).size(14), slider, text(value_text).size(13)]
            .spacing(4)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(96.0))
    .into()
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
        ParamId::Low | ParamId::Mid | ParamId::High => format!("{value:.0} %"),
        ParamId::Strength => format!("{value:.1} ms"),
        ParamId::Boost => format!("{value:.2}x"),
        ParamId::OutputGain => format!("{value:.1} dB"),
        ParamId::X1 | ParamId::X2 => format!("{value:.0} Hz"),
    }
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
                title: String::from("Maolan Widener"),
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
                        title: String::from("Maolan Widener"),
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
