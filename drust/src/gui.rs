use std::{
    ffi::CStr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
    time::Duration,
};

#[cfg(target_os = "macos")]
use clap_clap::ffi::CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
use clap_clap::ffi::CLAP_WINDOW_API_WIN32;
use clap_clap::ffi::CLAP_WINDOW_API_X11;
use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{button, column, container, progress_bar, text},
};
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};

use crate::{download, shared::SharedState};

pub const EDITOR_WIDTH: u32 = 300;
pub const EDITOR_HEIGHT: u32 = 280;

const KIT_NAMES: &[&str] = &["Crocell", "DRS", "Muldjord", "Aasimonster", "Shitty"];

fn variations_for_kit(kit_name: &str) -> &'static [&'static str] {
    match kit_name.to_lowercase().as_str() {
        "crocell" => &["full", "default", "small", "tiny"],
        "drs" => &["full", "basic", "minimal", "no_whiskers", "whiskers_only"],
        "aasimonster" => &["full", "minimal"],
        _ => &["Default", "Light", "Heavy", "Jazz", "Rock"],
    }
}

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
    VariationSelected(String),
    DownloadClicked,
    DownloadProgress(f32),
    DownloadFinished(Result<String, String>),
}

type DownloadResult = Option<Result<String, String>>;

struct State {
    shared: Arc<SharedState>,
    selected_kit: Option<String>,
    selected_variation: Option<String>,
    kit_cached: bool,
    download_progress: Option<f32>,
    download_progress_arc: Option<Arc<AtomicU32>>,
    download_result_arc: Option<Arc<std::sync::Mutex<DownloadResult>>>,
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    let raw_kit = shared.kit_path.read().clone();
    let selected_kit = if raw_kit.is_empty() {
        Some(KIT_NAMES[0].to_string())
    } else if raw_kit.contains('/') {
        download::kit_display_name_from_path(&raw_kit).or_else(|| {
            KIT_NAMES
                .iter()
                .find(|&&n| raw_kit.to_lowercase().contains(&n.to_lowercase()))
                .copied()
                .map(String::from)
        })
    } else {
        Some(raw_kit)
    };
    let mut selected_variation = shared.variation.read().clone();
    let variations = variations_for_kit(selected_kit.as_deref().unwrap_or(""));
    if selected_variation.is_empty() || !variations.contains(&selected_variation.as_str()) {
        selected_variation = variations.first().copied().unwrap_or("").to_string();
    }
    let selected_variation = if selected_variation.is_empty() {
        None
    } else {
        Some(selected_variation)
    };
    let kit_cached = download::is_kit_downloaded(selected_kit.as_deref().unwrap_or(""));
    (
        State {
            shared,
            selected_kit,
            selected_variation,
            kit_cached,
            download_progress: None,
            download_progress_arc: None,
            download_result_arc: None,
        },
        Task::none(),
    )
}

fn poll_download_task(progress: Arc<AtomicU32>) -> Task<Message> {
    Task::perform(
        async move {
            thread::sleep(Duration::from_millis(50));
            let prog = progress.load(Ordering::Acquire);
            if prog >= 200 {
                1.0
            } else {
                (prog as f32 / 100.0).min(1.0)
            }
        },
        Message::DownloadProgress,
    )
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::KitSelected(kit) => {
            state.selected_kit = Some(kit.clone());
            *state.shared.kit_path.write() = kit.clone();
            state.kit_cached = download::is_kit_downloaded(&kit);

            let variations = variations_for_kit(&kit);
            let current_var = state.selected_variation.as_deref().unwrap_or("");
            if !variations.contains(&current_var) {
                let new_variation = variations.first().copied().unwrap_or("").to_string();
                state.selected_variation = Some(new_variation.clone());
                *state.shared.variation.write() = new_variation;
            }

            state.shared.mark_dirty();
            Task::none()
        }
        Message::VariationSelected(variation) => {
            state.selected_variation = Some(variation.clone());
            *state.shared.variation.write() = variation;
            state.shared.mark_dirty();
            Task::none()
        }
        Message::DownloadClicked => {
            if let Some(kit) = state.selected_kit.as_ref() {
                let kit = kit.clone();
                let variation = state.selected_variation.clone().unwrap_or_else(|| {
                    variations_for_kit(&kit)
                        .first()
                        .copied()
                        .unwrap_or("")
                        .to_string()
                });
                let progress = Arc::new(AtomicU32::new(0));
                let result = Arc::new(std::sync::Mutex::new(None));
                state.download_progress = Some(0.0);
                state.download_progress_arc = Some(progress.clone());
                state.download_result_arc = Some(result.clone());
                let progress2 = progress.clone();
                thread::spawn(move || {
                    let res = download::download_kit_with_progress(&kit, &variation, |p| {
                        progress2.store((p * 100.0) as u32, Ordering::Release);
                    });
                    *result.lock().unwrap() = Some(res);
                    progress2.store(200, Ordering::Release);
                });
                return poll_download_task(progress);
            }
            Task::none()
        }
        Message::DownloadProgress(p) => {
            state.download_progress = Some(p);
            if let Some(progress_arc) = state.download_progress_arc.clone() {
                let prog = progress_arc.load(Ordering::Acquire);
                if prog >= 200 {
                    state.download_progress = None;
                    state.download_progress_arc = None;
                    if let Some(result) = state
                        .download_result_arc
                        .take()
                        .and_then(|arc| arc.lock().unwrap().take())
                    {
                        return Task::perform(async { result }, Message::DownloadFinished);
                    }
                } else {
                    return poll_download_task(progress_arc);
                }
            }
            Task::none()
        }
        Message::DownloadFinished(result) => {
            match result {
                Ok(xml_path) => {
                    *state.shared.pending_kit_path.write() = Some(xml_path);
                    state.kit_cached = true;
                }
                Err(e) => {
                    *state.shared.last_error.write() = Some(e);
                }
            }
            Task::none()
        }
    }
}

fn view(state: &State) -> Element<'_, Message> {
    let kit_options: Vec<String> = KIT_NAMES.iter().map(|s| s.to_string()).collect();
    let kit_selected = state.selected_kit.clone();

    let kit_dropdown: Element<'_, Message> = if kit_options.is_empty() {
        text("No kits available").into()
    } else {
        maolan_baseview::iced::widget::pick_list(kit_options, kit_selected, Message::KitSelected)
            .placeholder("Select kit")
            .into()
    };

    let variation_options: Vec<String> = if let Some(ref kit) = state.selected_kit {
        variations_for_kit(kit)
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        Vec::new()
    };
    let variation_selected = state.selected_variation.clone();

    let variation_dropdown: Element<'_, Message> = if variation_options.is_empty() {
        text("No variations available").into()
    } else {
        maolan_baseview::iced::widget::pick_list(
            variation_options,
            variation_selected,
            Message::VariationSelected,
        )
        .placeholder("Select variation")
        .into()
    };

    let download_button: Element<'_, Message> = if state.kit_cached {
        button("Load").into()
    } else {
        button("Load").on_press(Message::DownloadClicked).into()
    };

    let progress_widget: Element<'_, Message> = if let Some(progress) = state.download_progress {
        column![
            progress_bar(0.0..=1.0, progress),
            text(format!("{:.0}%", progress * 100.0)).size(12),
        ]
        .spacing(4)
        .align_x(Alignment::Center)
        .into()
    } else {
        text("").into()
    };

    let content = column![
        text("Drust").size(20),
        kit_dropdown,
        variation_dropdown,
        download_button,
        progress_widget,
    ]
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
