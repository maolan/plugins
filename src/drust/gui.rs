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

use crate::drust::{download, params::ParamId, shared::SharedState};
use maolan_widgets::horizontal_slider::horizontal_slider;

pub const EDITOR_WIDTH: u32 = 400;
pub const EDITOR_HEIGHT: u32 = 720;

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
    LoadClicked,
    LoadProgress(f32),
    LoadFinished(Result<String, String>),
    BalanceChanged(usize, f32),
    ParamChanged(ParamId, f64),
}

type LoadResult = Option<Result<String, String>>;

struct State {
    shared: Arc<SharedState>,
    selected_kit: Option<String>,
    selected_variation: Option<String>,
    loaded_kit: Option<String>,
    loaded_variation: Option<String>,
    load_progress: Option<f32>,
    load_progress_arc: Option<Arc<AtomicU32>>,
    load_result_arc: Option<Arc<std::sync::Mutex<LoadResult>>>,
    load_poll_count: u32,
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    let raw_kit = shared.kit_path.read().clone();
    let has_kit = !raw_kit.is_empty();
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

    // Determine actual load state from shared progress.
    let progress = shared.loading_progress.load(Ordering::Acquire);
    let active_channels = shared.active_channels.load(Ordering::Acquire);
    let kit_already_known = progress >= 100 || active_channels > 0;
    let (loaded_kit, loaded_variation, initial_task) = if kit_already_known {
        // Previous load completed before GUI opened or active_channels was
        // restored from state (UI-only instance doesn't load the kit itself).
        (
            selected_kit.clone(),
            selected_variation.clone(),
            Task::none(),
        )
    } else if has_kit {
        // Kit path is set but load may be in progress or not yet reflected.
        // Start polling so the UI discovers the real state.
        (None, None, poll_engine_load_task(Arc::clone(&shared)))
    } else {
        // No kit selected.
        (None, None, Task::none())
    };

    (
        State {
            shared,
            selected_kit: selected_kit.clone(),
            selected_variation: selected_variation.clone(),
            loaded_kit,
            loaded_variation,
            load_progress: if has_kit && !kit_already_known {
                Some(progress as f32 / 100.0)
            } else {
                None
            },
            load_progress_arc: None,
            load_result_arc: None,
            load_poll_count: 0,
        },
        initial_task,
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
        Message::LoadProgress,
    )
}

const MAX_ENGINE_LOAD_POLLS: u32 = 600; // ~60s at 100ms intervals

fn poll_engine_load_task(shared: Arc<SharedState>) -> Task<Message> {
    Task::perform(
        async move {
            thread::sleep(Duration::from_millis(100));
            let prog = shared.loading_progress.load(Ordering::Acquire);
            if prog >= 100 {
                1.0
            } else {
                (prog as f32 / 100.0).min(0.99)
            }
        },
        Message::LoadProgress,
    )
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::KitSelected(kit) => {
            state.selected_kit = Some(kit.clone());
            *state.shared.kit_path.write() = kit.clone();

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
        Message::LoadClicked => {
            if state.load_progress.is_some() {
                return Task::none();
            }
            if let Some(kit) = state.selected_kit.as_ref() {
                let kit = kit.clone();
                let variation = state.selected_variation.clone().unwrap_or_else(|| {
                    variations_for_kit(&kit)
                        .first()
                        .copied()
                        .unwrap_or("")
                        .to_string()
                });
                // If already cached, load immediately without spawning a thread.
                if let Some(xml_path) = download::resolve_kit_xml(&kit, &variation) {
                    *state.shared.pending_kit_path.write() =
                        Some(xml_path.to_string_lossy().into_owned());
                    state.load_progress = Some(0.0);
                    state.shared.loading_progress.store(0, Ordering::Release);
                    return poll_engine_load_task(Arc::clone(&state.shared));
                }
                let progress = Arc::new(AtomicU32::new(0));
                let result = Arc::new(std::sync::Mutex::new(None));
                state.load_progress = Some(0.0);
                state.load_progress_arc = Some(progress.clone());
                state.load_result_arc = Some(result.clone());
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
        Message::LoadProgress(p) => {
            state.load_progress = Some(p);
            if let Some(progress_arc) = state.load_progress_arc.clone() {
                let prog = progress_arc.load(Ordering::Acquire);
                if prog >= 200 {
                    // Download done; transition to engine load.
                    state.load_progress_arc = None;
                    state.load_progress = Some(0.0);
                    state.load_poll_count = 0;
                    if let Some(result) = state
                        .load_result_arc
                        .take()
                        .and_then(|arc| arc.lock().unwrap().take())
                    {
                        match result {
                            Ok(xml_path) => {
                                *state.shared.pending_kit_path.write() = Some(xml_path);
                            }
                            Err(e) => {
                                *state.shared.last_error.write() = Some(e);
                            }
                        }
                    }
                    return poll_engine_load_task(Arc::clone(&state.shared));
                } else {
                    return poll_download_task(progress_arc);
                }
            } else if p < 1.0 {
                // Engine still loading.
                state.load_poll_count += 1;
                if state.load_poll_count > MAX_ENGINE_LOAD_POLLS {
                    state.load_progress = None;
                    state.load_result_arc = None;
                    state.load_poll_count = 0;
                    *state.shared.last_error.write() = Some("Engine load timed out.".to_string());
                    return Task::none();
                }
                return poll_engine_load_task(Arc::clone(&state.shared));
            } else {
                // Engine load done.
                state.load_progress = None;
                state.load_result_arc = None;
                state.load_poll_count = 0;
                state.loaded_kit = state.selected_kit.clone();
                state.loaded_variation = state.selected_variation.clone();
            }
            Task::none()
        }
        Message::LoadFinished(result) => {
            if let Err(e) = result {
                *state.shared.last_error.write() = Some(e);
            }
            Task::none()
        }
        Message::BalanceChanged(pair, value) => {
            let id = balance_param_id(pair);
            state.shared.set_param(id, value as f64);
            Task::none()
        }
        Message::ParamChanged(id, value) => {
            state.shared.set_param(id, value);
            Task::none()
        }
    }
}

fn balance_param_id(pair: usize) -> ParamId {
    match pair {
        0 => ParamId::Balance1,
        1 => ParamId::Balance2,
        2 => ParamId::Balance3,
        3 => ParamId::Balance4,
        4 => ParamId::Balance5,
        5 => ParamId::Balance6,
        6 => ParamId::Balance7,
        7 => ParamId::Balance8,
        _ => ParamId::Balance1,
    }
}

fn balance_label(pair: usize) -> &'static str {
    match pair {
        0 => "Kick",
        1 => "Snare",
        2 => "HiHat",
        3 => "Toms",
        4 => "Ride",
        5 => "Crash",
        6 => "China/Splash",
        7 => "Ambience",
        _ => "Out",
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

    let load_button: Element<'_, Message> = button("Load").on_press(Message::LoadClicked).into();

    let progress_widget: Element<'_, Message> = if let Some(progress) = state.load_progress {
        let label = if state.load_progress_arc.is_some() {
            format!("Downloading... {:.0}%", progress * 100.0)
        } else {
            format!("Loading... {:.0}%", progress * 100.0)
        };
        column![progress_bar(0.0..=1.0, progress), text(label).size(12),]
            .spacing(4)
            .align_x(Alignment::Center)
            .into()
    } else {
        text("").into()
    };

    let error_widget: Element<'_, Message> = if let Some(ref err) = *state.shared.last_error.read()
    {
        text(format!("Error: {err}"))
            .size(11)
            .color(maolan_baseview::iced::Color::from_rgb(1.0, 0.4, 0.4))
            .into()
    } else {
        text("").into()
    };

    let mut balance_rows = Vec::new();
    for row in 0..4 {
        let left_pair = row * 2;
        let right_pair = left_pair + 1;
        let left_label = balance_label(left_pair);
        let right_label = balance_label(right_pair);
        let left_value = state.shared.params.get(balance_param_id(left_pair)) as f32;
        let right_value = state.shared.params.get(balance_param_id(right_pair)) as f32;

        let left_slider = horizontal_slider(-1.0..=1.0, left_value, move |v| {
            Message::BalanceChanged(left_pair, v)
        })
        .step(0.01)
        .double_click_reset(0.0)
        .width(Length::Fixed(140.0))
        .height(Length::Fixed(12.0));

        let right_slider = horizontal_slider(-1.0..=1.0, right_value, move |v| {
            Message::BalanceChanged(right_pair, v)
        })
        .step(0.01)
        .double_click_reset(0.0)
        .width(Length::Fixed(140.0))
        .height(Length::Fixed(12.0));

        let left_col = column![text(left_label).size(11), left_slider]
            .spacing(2)
            .align_x(Alignment::Center)
            .width(Length::Fixed(160.0));
        let right_col = column![text(right_label).size(11), right_slider]
            .spacing(2)
            .align_x(Alignment::Center)
            .width(Length::Fixed(160.0));

        balance_rows.push(
            maolan_baseview::iced::widget::row![left_col, right_col]
                .spacing(8)
                .align_y(Alignment::Center)
                .into(),
        );
    }

    let balance_section = maolan_baseview::iced::widget::column(balance_rows)
        .spacing(6)
        .align_x(Alignment::Center);

    fn param_slider<'a>(
        label: &'static str,
        id: ParamId,
        range: std::ops::RangeInclusive<f32>,
        step: f32,
        reset: f32,
        state: &State,
    ) -> Element<'a, Message> {
        let value = state.shared.params.get(id) as f32;
        column![
            text(label).size(11),
            horizontal_slider(range, value, move |v| Message::ParamChanged(id, v as f64))
                .fill_from_start()
                .step(step)
                .double_click_reset(reset)
                .width(Length::Fixed(170.0))
                .height(Length::Fixed(12.0)),
        ]
        .spacing(2)
        .align_x(Alignment::Center)
        .width(Length::Fixed(180.0))
        .into()
    }

    let params_left = column![
        param_slider(
            "Master Gain",
            ParamId::MasterGain,
            -60.0..=12.0,
            1.0,
            0.0,
            state
        ),
        param_slider(
            "Humanize",
            ParamId::HumanizeAmount,
            0.0..=100.0,
            1.0,
            8.0,
            state
        ),
        param_slider(
            "Round Robin",
            ParamId::RoundRobinMix,
            0.0..=1.0,
            0.01,
            0.7,
            state
        ),
        param_slider(
            "Bleed",
            ParamId::BleedAmount,
            0.0..=100.0,
            1.0,
            100.0,
            state
        ),
    ]
    .spacing(6)
    .align_x(Alignment::Center);

    let params_right = column![
        param_slider(
            "Limiter Thr",
            ParamId::LimiterThreshold,
            -48.0..=0.0,
            1.0,
            -3.0,
            state
        ),
        param_slider(
            "Voice Limit",
            ParamId::VoiceLimitMax,
            1.0..=128.0,
            1.0,
            128.0,
            state
        ),
        param_slider(
            "Rampdown",
            ParamId::VoiceLimitRampdown,
            0.01..=2.0,
            0.01,
            0.5,
            state
        ),
    ]
    .spacing(6)
    .align_x(Alignment::Center);

    let params_section = column![
        text("Parameters").size(14),
        maolan_baseview::iced::widget::row![params_left, params_right].spacing(8),
    ]
    .spacing(8)
    .align_x(Alignment::Center);

    let loaded_label = {
        if let Some(ref kit) = state.loaded_kit {
            let var = state.loaded_variation.as_deref().unwrap_or("-");
            text(format!("Loaded: {kit}/{var}")).size(12)
        } else {
            // Load hasn't completed yet; show what is being loaded (if known).
            let kit_path = state.shared.kit_path.read();
            let variation = state.shared.variation.read();
            let kit_name = if kit_path.is_empty() {
                "None".to_string()
            } else {
                download::kit_display_name_from_path(&kit_path)
                    .unwrap_or_else(|| "Unknown".to_string())
            };
            let var_str = if variation.is_empty() {
                "-"
            } else {
                variation.as_str()
            };
            if state.load_progress.is_some() {
                text(format!("Loading: {kit_name}/{var_str}")).size(12)
            } else {
                text(format!("Kit: {kit_name}/{var_str}")).size(12)
            }
        }
    };

    let content = column![
        text("Drust").size(20),
        loaded_label,
        kit_dropdown,
        variation_dropdown,
        load_button,
        progress_widget,
        error_widget,
        balance_section,
        params_section,
    ]
    .spacing(10)
    .align_x(Alignment::Center);

    container(content)
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Center)
        .align_y(Vertical::Top)
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
