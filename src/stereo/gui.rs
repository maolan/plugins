use std::{
    ffi::CStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{checkbox, column, container, radio, row, scrollable, text, toggler},
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
    common::ui::{SmallKnob, small_knob},
    stereo::{
        params::{PARAMS, ParamId},
        plugin::SharedState,
    },
};

pub const EDITOR_WIDTH: u32 = 550;
pub const EDITOR_HEIGHT: u32 = 830;

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
        Message::ToggleGain(on) => {
            state.shared.set_gain_on(on);
        }
        Message::ToggleDelay(on) => {
            state.shared.set_delay_on(on);
        }
        Message::ToggleCharacter(on) => {
            state.shared.set_character_on(on);
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
            text("Gain").size(14),
            text("Off").size(13),
            toggler(state.shared.gain_on()).on_toggle(Message::ToggleGain),
            text("On").size(13),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );
    content = content.push(
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
    content = content.push(
        row![
            knob("X1", ParamId::X1, p(ParamId::X1), "Hz", 1.0),
            knob("X2", ParamId::X2, p(ParamId::X2), "Hz", 1.0),
            knob("Boost", ParamId::Boost, p(ParamId::Boost), "x", 0.01),
        ]
        .spacing(16),
    );

    content = content.push(
        row![
            text("Delay").size(14),
            text("Off").size(13),
            toggler(state.shared.delay_on()).on_toggle(Message::ToggleDelay),
            text("On").size(13),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );
    content = content.push(
        row![
            knob("Strength", ParamId::Strength, p(ParamId::Strength), "", 0.1),
            knob("Low", ParamId::LowDelay, p(ParamId::LowDelay), "", 1.0),
            knob("Mid", ParamId::MidDelay, p(ParamId::MidDelay), "", 1.0),
            knob("High", ParamId::HighDelay, p(ParamId::HighDelay), "", 1.0),
        ]
        .spacing(16),
    );

    content = content.push(
        row![
            text("Character").size(14),
            text("Off").size(13),
            toggler(state.shared.character_on()).on_toggle(Message::ToggleCharacter),
            text("On").size(13),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );
    content = content.push(
        row![
            knob("Density", ParamId::Density, p(ParamId::Density), "", 0.01),
            knob("Focus", ParamId::Focus, p(ParamId::Focus), "", 0.01),
            knob("Amount", ParamId::Amount, p(ParamId::Amount), "", 0.01),
        ]
        .spacing(16),
    );

    content = content.push(text("Output").size(18));
    content = content.push(
        row![knob(
            "Volume",
            ParamId::OutputGain,
            p(ParamId::OutputGain),
            "dB",
            0.1
        ),]
        .spacing(16),
    );
    let monitor_selected = match p(ParamId::MonitorMode) as i32 {
        1 => Some(1u8),
        2 => Some(2u8),
        _ => Some(0u8),
    };
    content = content.push(
        row![
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
                title: String::from("Maolan Stereo"),
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
                        title: String::from("Maolan Stereo"),
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
