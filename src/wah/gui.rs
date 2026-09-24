use std::{ffi::CStr, sync::Arc};

#[cfg(target_os = "macos")]
use maolan_clap::ffi::CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
use maolan_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use maolan_clap::ffi::CLAP_WINDOW_API_X11;

use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{column, container, row},
};
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd"
))]
use raw_window_handle::RawWindowHandle;
use raw_window_handle::{HandleError, HasWindowHandle, WindowHandle};

use crate::common::ui::{SmallKnob, small_knob};
use crate::wah::{
    params::{MODE_LABELS, PARAMS, ParamId, SHAPE_LABELS},
    plugin::SharedState,
};

pub const EDITOR_WIDTH: u32 = 360;
pub const EDITOR_HEIGHT: u32 = 360;

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
            // Stepped parameters emit a complete gesture in one shot.
            let discrete = matches!(id, ParamId::Mode | ParamId::LfoShape);
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

fn format_param(shared: &SharedState, id: ParamId) -> String {
    let value = shared.params.get(id);
    match id {
        ParamId::Mode => {
            let idx = (value.round() as usize).clamp(0, MODE_LABELS.len() - 1);
            MODE_LABELS[idx].to_string()
        }
        ParamId::LfoShape => {
            let idx = (value.round() as usize).clamp(0, SHAPE_LABELS.len() - 1);
            SHAPE_LABELS[idx].to_string()
        }
        ParamId::MinCutoff | ParamId::MaxCutoff => format!("{value:.0} Hz"),
        ParamId::Resonance => format!("{value:.1}"),
        ParamId::Position | ParamId::LfoDepth | ParamId::EnvDepth | ParamId::DryWet => {
            format!("{:.0}%", value * 100.0)
        }
        ParamId::LfoRate => format!("{value:.1} Hz"),
        ParamId::EnvAttack | ParamId::EnvRelease => format!("{value:.0} ms"),
    }
}

fn knob<'a>(id: ParamId, label: &'static str, state: &'a State) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let def = &PARAMS[id.as_index()];
    let value_text = format_param(&state.shared, id);

    small_knob(
        SmallKnob {
            label: label.to_string(),
            value,
            range: def.min as f32..=def.max as f32,
            default: def.default as f32,
            step: def.step as f32,
            value_text,
        },
        move |v| Message::SetParam(id, v),
        Message::ReleaseParam(id),
    )
}

fn view(state: &State) -> Element<'_, Message> {
    let content = column![
        row![
            knob(ParamId::Mode, "Mode", state),
            knob(ParamId::Position, "Position", state),
            knob(ParamId::DryWet, "Dry/Wet", state),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
        row![
            knob(ParamId::MinCutoff, "Min Cutoff", state),
            knob(ParamId::MaxCutoff, "Max Cutoff", state),
            knob(ParamId::Resonance, "Resonance", state),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
        row![
            knob(ParamId::LfoRate, "LFO Rate", state),
            knob(ParamId::LfoDepth, "LFO Depth", state),
            knob(ParamId::LfoShape, "LFO Shape", state),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
        row![
            knob(ParamId::EnvAttack, "Env Attack", state),
            knob(ParamId::EnvRelease, "Env Release", state),
            knob(ParamId::EnvDepth, "Env Depth", state),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    ]
    .spacing(16)
    .align_x(Alignment::Center);

    container(content)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center)
        .into()
}

fn theme(_state: &State) -> Theme {
    Theme::TokyoNight
}

fn build_app(shared: Arc<SharedState>) -> impl maolan_baseview::iced::Program {
    maolan_baseview::iced::application(move || init(shared.clone()), update, view)
        .theme(theme)
        .run()
}

pub struct App;

impl crate::common::gui_bridge::GuiApp<super::plugin::SharedState> for App {
    type Parent = ParentWindowHandle;

    fn title() -> String {
        String::from("Maolan Wah")
    }

    fn size() -> (u32, u32) {
        (EDITOR_WIDTH, EDITOR_HEIGHT)
    }

    fn always_redraw() -> bool {
        true
    }

    fn notify_closed_on_hide() -> bool {
        false
    }

    fn opens_parented_window() -> bool {
        false
    }

    fn preferred_api() -> &'static CStr {
        preferred_api()
    }

    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "freebsd"
    ))]
    fn parent_from_window(
        window: &maolan_clap::ffi::clap_window,
        api: &CStr,
    ) -> Option<Self::Parent> {
        crate::wah::gui::parent_from_window(window, api)
    }

    fn build_app(
        shared: std::sync::Arc<super::plugin::SharedState>,
    ) -> impl maolan_baseview::iced::Program {
        build_app(shared)
    }
}

pub type GuiBridge = crate::common::gui_bridge::GuiBridge<super::plugin::SharedState, App>;
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd"
))]
pub fn parent_from_window(
    window: &maolan_clap::ffi::clap_window,
    api: &CStr,
) -> Option<ParentWindowHandle> {
    use maolan_clap::ffi::{CLAP_WINDOW_API_COCOA, CLAP_WINDOW_API_WIN32, CLAP_WINDOW_API_X11};
    if api == CLAP_WINDOW_API_X11 {
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        {
            Some(ParentWindowHandle::X11(unsafe { window.clap_window__.x11 }))
        }
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        {
            None
        }
    } else if api == CLAP_WINDOW_API_WIN32 {
        #[cfg(target_os = "windows")]
        {
            Some(ParentWindowHandle::Win32(unsafe {
                window.clap_window__.win32
            }))
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    } else if api == CLAP_WINDOW_API_COCOA {
        #[cfg(target_os = "macos")]
        {
            Some(ParentWindowHandle::Cocoa(unsafe {
                window.clap_window__.cocoa
            }))
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    } else {
        None
    }
}
