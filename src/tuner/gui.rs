use std::{
    ffi::CStr,
    sync::{Arc, atomic::Ordering},
};

#[cfg(target_os = "macos")]
use maolan_clap::ffi::CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
use maolan_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use maolan_clap::ffi::CLAP_WINDOW_API_X11;

use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{column, container, row, text},
};
use maolan_widgets::horizontal_slider::horizontal_slider;
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd"
))]
use raw_window_handle::RawWindowHandle;
use raw_window_handle::{HandleError, HasWindowHandle, WindowHandle};

use crate::tuner::{dsp::format_note, plugin::SharedState};

pub const EDITOR_WIDTH: u32 = 280;
pub const EDITOR_HEIGHT: u32 = 200;

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

struct State {
    shared: Arc<SharedState>,
}

fn init(shared: Arc<SharedState>) -> (State, Task<()>) {
    (State { shared }, Task::none())
}

fn update(_state: &mut State, _message: ()) -> Task<()> {
    Task::none()
}

fn view(state: &State) -> Element<'_, ()> {
    let detected = state.shared.detected.load(Ordering::Relaxed);
    let frequency = state.shared.frequency_hz.load(Ordering::Relaxed) as f32;
    let note = state.shared.note.load(Ordering::Relaxed) as f32;
    let cents = state.shared.cents.load(Ordering::Relaxed) as f32;
    let clarity = state.shared.clarity.load(Ordering::Relaxed) as f32;

    let note_text = if detected && note > 0.0 {
        format_note(note)
    } else {
        "--".to_string()
    };
    let freq_text = if detected && frequency > 0.0 {
        format!("{frequency:.1} Hz")
    } else {
        "-- Hz".to_string()
    };
    let cents_text = if detected {
        format!("{:+.0} cents", cents)
    } else {
        "--".to_string()
    };
    let clarity_text = format!("clarity: {:.0}%", clarity * 100.0);
    let cents_value = if detected {
        cents.clamp(-50.0, 50.0)
    } else {
        0.0
    };
    let cents_slider = horizontal_slider(-50.0..=50.0, cents_value, |_| ())
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(16.0));

    let content = column![
        text(note_text).size(64),
        row![text(freq_text).size(13), text(cents_text).size(13),].spacing(16),
        text(clarity_text).size(11),
        cents_slider,
    ]
    .spacing(8)
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
        .subscription(|_state| maolan_baseview::iced::poll_events())
        .theme(theme)
        .run()
}

pub struct App;

impl crate::common::gui_bridge::GuiApp<super::plugin::SharedState> for App {
    type Parent = ParentWindowHandle;

    fn title() -> String {
        String::from("Maolan Tuner")
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
        crate::tuner::gui::parent_from_window(window, api)
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
