use std::{ffi::CStr, sync::Arc};

use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{column, container, row},
};
#[cfg(target_os = "macos")]
use maolan_clap::ffi::CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
use maolan_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use maolan_clap::ffi::CLAP_WINDOW_API_X11;
use maolan_widgets::knob::Knob;
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd"
))]
use raw_window_handle::RawWindowHandle;
use raw_window_handle::{HandleError, HasWindowHandle, WindowHandle};

use crate::{
    bitcrusher::{
        params::{PARAMS, ParamId},
        plugin::SharedState,
    },
    common::preset::{self, PresetProvider, PresetUi},
};

pub const EDITOR_WIDTH: u32 = 640;
pub const EDITOR_HEIGHT: u32 = 210;

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
    X11(usize),
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
                let handle = raw_window_handle::XlibWindowHandle::new(*window as _);
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
pub enum Message {
    SetParam(ParamId, f32),
    ReleaseParam(ParamId),
    Preset(preset::Msg),
}

impl From<preset::Msg> for Message {
    fn from(msg: preset::Msg) -> Self {
        Message::Preset(msg)
    }
}

struct State {
    shared: Arc<SharedState>,
    active_gestures: Vec<bool>,
    preset_ui: PresetUi,
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    (
        State {
            shared,
            active_gestures: vec![false; ParamId::COUNT],
            preset_ui: PresetUi::new(SharedState::DIR_NAME),
        },
        Task::none(),
    )
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::SetParam(id, value) => {
            let index = id.as_index();
            if !state.active_gestures[index] {
                state.active_gestures[index] = true;
                state.shared.mark_gesture_begin_pending(id);
            }
            state.shared.set_param_outbound_only(id, value as f64);
        }
        Message::ReleaseParam(id) => {
            let index = id.as_index();
            if state.active_gestures[index] {
                state.active_gestures[index] = false;
                state.shared.mark_gesture_end_pending(id);
            }
        }
        Message::Preset(msg) => {
            preset::update(&mut state.preset_ui, msg, state.shared.as_ref());
        }
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;
    let content = column![
        preset::bar(&state.preset_ui),
        row![
            knob(
                "Bit Depth",
                ParamId::BitDepth,
                p(ParamId::BitDepth),
                "bit",
                1.0,
            ),
            knob(
                "Downsample",
                ParamId::Downsample,
                p(ParamId::Downsample),
                "x",
                1.0,
            ),
            knob("Drive", ParamId::Drive, p(ParamId::Drive), "dB", 0.1),
            knob("Mix", ParamId::DryWet, p(ParamId::DryWet), "%", 0.01),
            knob(
                "Output",
                ParamId::OutputGain,
                p(ParamId::OutputGain),
                "dB",
                0.1,
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    ]
    .spacing(24)
    .align_x(Alignment::Start);

    container(content)
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
    let value_text = match units {
        "dB" => format!("{value:.1} dB"),
        "%" => format!("{:.0}%", value * 100.0),
        "bit" => format!("{value:.0} bit"),
        "x" => format!("{value:.0}x"),
        _ => format!("{value:.2}"),
    };
    maolan_widgets::knob::knob(
        Knob {
            label: label.to_string(),
            value,
            range: def.min as f32..=def.max as f32,
            default: def.default as f32,
            step,
            value_text,
            assigned: false,
        },
        move |value| Message::SetParam(id, value),
        Message::ReleaseParam(id),
    )
}

fn build_app(shared: Arc<SharedState>) -> impl maolan_baseview::iced::Program {
    maolan_baseview::iced::application(move || init(shared.clone()), update, view)
        .font(maolan_widgets::iced_fonts::LUCIDE_FONT_BYTES)
        .theme(theme)
        .run()
}

pub struct App;

impl crate::common::gui_bridge::GuiApp<super::plugin::SharedState> for App {
    type Parent = ParentWindowHandle;

    fn title() -> String {
        String::from("Maolan Bitcrusher")
    }

    fn size() -> (u32, u32) {
        (EDITOR_WIDTH, EDITOR_HEIGHT)
    }

    fn always_redraw() -> bool {
        false
    }

    fn notify_closed_on_hide() -> bool {
        true
    }

    fn opens_parented_window() -> bool {
        true
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
        crate::bitcrusher::gui::parent_from_window(window, api)
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
            Some(ParentWindowHandle::X11(unsafe {
                window.clap_window__.x11 as usize
            }))
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
