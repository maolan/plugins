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
use clap_clap::ffi::CLAP_WINDOW_API_X11;
use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{column, container, text},
};
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};

use crate::shared::SharedState;

pub const EDITOR_WIDTH: u32 = 300;
pub const EDITOR_HEIGHT: u32 = 150;

const KIT_NAMES: &[&str] = &["Crocell", "DRS", "Muldjord", "Aasimonster", "Shitty"];

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

// ------------------------------------------------------------------
// Parent window handle wrapper for baseview
// ------------------------------------------------------------------

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

// ------------------------------------------------------------------
// Iced GUI
// ------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Message {
    KitSelected(String),
}

struct State {
    shared: Arc<SharedState>,
    selected_kit: Option<String>,
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    let selected_kit = shared.kit_path.read().clone();
    let selected_kit = if selected_kit.is_empty() {
        None
    } else {
        Some(selected_kit)
    };
    (
        State {
            shared,
            selected_kit,
        },
        Task::none(),
    )
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::KitSelected(kit) => {
            state.selected_kit = Some(kit.clone());
            *state.shared.kit_path.write() = kit;
            state.shared.mark_dirty();
            Task::none()
        }
    }
}

fn view(state: &State) -> Element<'_, Message> {
    let options: Vec<String> = KIT_NAMES.iter().map(|s| s.to_string()).collect();
    let selected = state.selected_kit.clone();

    let dropdown: Element<'_, Message> = if options.is_empty() {
        text("No kits available").into()
    } else {
        maolan_baseview::iced::widget::pick_list(options, selected, Message::KitSelected)
            .placeholder("Select kit")
            .into()
    };

    let content = column![text("Drust").size(20), dropdown,]
        .spacing(12)
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

// ------------------------------------------------------------------
// GuiBridge
// ------------------------------------------------------------------

struct AnyWindowHandle {
    _inner: Box<dyn std::any::Any>,
}

// baseview::WindowHandle is !Send because it contains raw pointers, but in practice
// the window runs on its own thread and the handle only sends signals to it.
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
                title: String::from("Drust"),
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
            if self.floating_open.load(Ordering::Acquire) {
                return true;
            }
            let Some(shared) = self.shared.clone() else {
                return false;
            };
            let floating_open = self.floating_open.clone();
            floating_open.store(true, Ordering::Release);
            let _ = thread::Builder::new()
                .name("drust-gui".to_string())
                .spawn(move || {
                    let settings = maolan_baseview::iced::IcedBaseviewSettings {
                        window: maolan_baseview::iced::baseview::WindowOpenOptions {
                            title: String::from("Drust"),
                            size: maolan_baseview::iced::baseview::Size::new(
                                EDITOR_WIDTH as f64,
                                EDITOR_HEIGHT as f64,
                            ),
                            scale: maolan_baseview::iced::baseview::WindowScalePolicy::SystemScaleFactor,
                        },
                        ignore_non_modifier_keys: false,
                        always_redraw: false,
                    };
                    maolan_baseview::iced::open_blocking(
                        settings,
                        maolan_baseview::iced::PollSubNotifier::new(),
                        move || build_app(shared),
                    );
                    floating_open.store(false, Ordering::Release);
                });
            return true;
        }
        true
    }

    pub fn hide(&mut self) -> bool {
        true
    }
}
