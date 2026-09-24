use std::{ffi::CStr, sync::Arc};

use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{column, container, pick_list, row, text},
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

use crate::random::{
    params::{LENGTHS, Note, ParamId},
    plugin::SharedState,
};

pub const EDITOR_WIDTH: u32 = 440;
pub const EDITOR_HEIGHT: u32 = 240;

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
pub enum Message {
    Length(ParamId, &'static str),
    Pitch(ParamId, Note),
}
struct State {
    shared: Arc<SharedState>,
}
fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    (State { shared }, Task::none())
}
fn update(state: &mut State, message: Message) -> Task<Message> {
    let (id, value) = match message {
        Message::Length(id, label) => {
            let Some(index) = LENGTHS.iter().position(|s| *s == label) else {
                return Task::none();
            };
            (id, index as f64)
        }
        Message::Pitch(id, note) => (id, f64::from(note.0)),
    };
    state.shared.mark_gesture_begin_pending(id);
    state.shared.set_param_outbound_only(id, value);
    state.shared.mark_gesture_end_pending(id);
    Task::none()
}
fn view(state: &State) -> Element<'_, Message> {
    let control = |id, label| {
        column![
            text(label),
            pick_list(
                LENGTHS.to_vec(),
                Some(LENGTHS[state.shared.params.get(id) as usize]),
                move |value| Message::Length(id, value)
            )
        ]
        .spacing(8)
    };
    let lowest = state.shared.params.get(ParamId::LowestNote) as u8;
    let highest = state.shared.params.get(ParamId::HighestNote) as u8;
    let pitch_control = |id, label, selected, options: Vec<Note>| {
        column![
            text(label),
            pick_list(options, Some(Note(selected)), move |note| Message::Pitch(
                id, note
            ))
        ]
        .spacing(8)
    };
    container(
        column![
            row![
                control(ParamId::NoteLength, "Note length"),
                control(ParamId::PauseLength, "Pause length")
            ]
            .spacing(24)
            .align_y(Alignment::Center),
            row![
                pitch_control(
                    ParamId::LowestNote,
                    "Lowest note",
                    lowest,
                    (0..=highest).map(Note).collect()
                ),
                pitch_control(
                    ParamId::HighestNote,
                    "Highest note",
                    highest,
                    (lowest..=127).map(Note).collect()
                ),
            ]
            .spacing(24)
            .align_y(Alignment::Center),
        ]
        .spacing(24),
    )
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
        String::from("Maolan Random")
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
        crate::random::gui::parent_from_window(window, api)
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
