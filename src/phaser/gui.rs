use std::{
    ffi::CStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

#[cfg(target_os = "windows")]
use maolan_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(unix)]
use maolan_clap::ffi::CLAP_WINDOW_API_X11;

use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{column, container, row, text, toggler},
};
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};

use crate::{
    common::ui::{SmallKnob, small_knob},
    phaser::{
        params::{PARAMS, ParamId},
        plugin::SharedState,
    },
};

pub const EDITOR_WIDTH: u32 = 360;
pub const EDITOR_HEIGHT: u32 = 280;

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

#[derive(Debug, Clone, Copy)]
enum Message {
    ParamChanged(ParamId, f32),
    ParamReleased(ParamId),
    BoolParamChanged(ParamId, bool),
    Poll,
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
        Message::ParamChanged(id, value) => {
            let idx = id.as_index();
            if !state.active_gestures[idx] {
                state.active_gestures[idx] = true;
                state.shared.mark_gesture_begin_pending(id);
            }
            state.shared.set_param_outbound_only(id, value as f64);
        }
        Message::ParamReleased(id) => {
            let idx = id.as_index();
            if state.active_gestures[idx] {
                state.active_gestures[idx] = false;
                state.shared.mark_gesture_end_pending(id);
            }
        }
        Message::BoolParamChanged(id, value) => {
            state.shared.mark_gesture_begin_pending(id);
            state
                .shared
                .set_param_outbound_only(id, if value { 1.0 } else { 0.0 });
            state.shared.mark_gesture_end_pending(id);
            state.shared.mark_dirty();
        }
        Message::Poll => {}
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    let title = text("Maolan Phaser").size(18);

    let content = column![
        title,
        row![
            knob(ParamId::LfoRate, "Rate", state),
            knob(ParamId::LfoDepth, "Depth", state),
            knob(ParamId::Manual, "Manual", state),
            knob(ParamId::Feedback, "Feedback", state),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        row![
            delay_switch(state),
            knob(ParamId::DelayTime, "Time", state),
            knob(ParamId::Stages, "Stages", state),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(16)
    .align_x(Alignment::Center);

    container(content)
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center)
        .into()
}

fn delay_switch(state: &State) -> Element<'_, Message> {
    let enabled = state.shared.params.get(ParamId::FeedbackDelayOn) >= 0.5;
    let value_text = if enabled { "On" } else { "Off" };

    container(
        column![
            text("Delay").size(11),
            toggler(enabled).on_toggle(|v| Message::BoolParamChanged(ParamId::FeedbackDelayOn, v)),
            text(value_text).size(10),
        ]
        .spacing(7)
        .align_x(Alignment::Center),
    )
    .width(Length::Fixed(70.0))
    .into()
}

fn knob<'a>(id: ParamId, label: &'a str, state: &'a State) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let def = PARAMS[id.as_index()];
    let value_text = match id {
        ParamId::LfoRate => format!("{value:.2} Hz"),
        ParamId::LfoDepth | ParamId::Manual | ParamId::Feedback => format!("{value:.2}"),
        ParamId::FeedbackDelayOn => {
            if value >= 0.5 {
                "On".to_string()
            } else {
                "Off".to_string()
            }
        }
        ParamId::DelayTime => format!("{value:.1} ms"),
        ParamId::Stages => format!("{value:.0}"),
    };

    small_knob(
        SmallKnob {
            label: label.to_string(),
            value,
            range: def.min as f32..=def.max as f32,
            default: def.default as f32,
            step: def.step as f32,
            value_text,
        },
        move |v| Message::ParamChanged(id, v),
        Message::ParamReleased(id),
    )
}

fn theme(_state: &State) -> Theme {
    Theme::TokyoNight
}

fn build_app(shared: Arc<SharedState>) -> impl maolan_baseview::iced::Program {
    maolan_baseview::iced::application(move || init(shared.clone()), update, view)
        .subscription(|_state| maolan_baseview::iced::poll_events().map(|_| Message::Poll))
        .theme(theme)
        .run()
}

#[derive(Default)]
pub struct GuiBridge {
    created: bool,
    floating: bool,
    shared: Option<Arc<SharedState>>,
    floating_open: Arc<AtomicBool>,
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
        self.created = false;
        self.floating = false;
        self.shared = None;
    }

    pub fn set_parent(&mut self, shared: Arc<SharedState>, _parent: ParentWindowHandle) -> bool {
        if !self.created {
            return false;
        }
        if !self.floating {
            self.shared = Some(shared);
        }
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
            let notifier = maolan_baseview::iced::PollSubNotifier::new();
            *shared.poll_notifier.lock() = Some(notifier.clone());
            let open_flag = self.floating_open.clone();
            thread::spawn(move || {
                let settings = maolan_baseview::iced::IcedBaseviewSettings {
                    window: maolan_baseview::iced::baseview::WindowOpenOptions {
                        title: String::from("Maolan Phaser"),
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
                maolan_baseview::iced::shell::open_blocking(settings, notifier, move || {
                    build_app(shared)
                });
                open_flag.store(false, Ordering::Release);
            });
        }
        true
    }

    pub fn hide(&mut self, _shared: Arc<SharedState>) -> bool {
        if self.floating {
            self.floating_open.store(false, Ordering::Release);
        }
        true
    }
}
