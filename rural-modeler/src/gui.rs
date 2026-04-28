use std::{
    ffi::CStr,
    path::Path,
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
use maolan_baseview::iced::widget::image::Image;
use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{button, checkbox, column, container, radio, row, scrollable, text, text_input},
};
use maolan_widgets::arch_slider::arch_slider;
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};

use crate::{
    params::{PARAMS, ParamId},
    plugin::SharedState,
    tone3000::{self, AssetKind, PaginatedSearchResults, SearchItem, SearchVariation},
};

pub const EDITOR_WIDTH: u32 = 720;
pub const EDITOR_HEIGHT: u32 = 560;

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
    LoadModel,
    LoadIr,
    ClearModel,
    ClearIr,
    SetParam(ParamId, f32),
    SetBoolParam(ParamId, bool),
    SetOutputMode(u8),
    ToneModelQueryChanged(String),
    ToneIrQueryChanged(String),
    ToneSearchModels,
    ToneSearchModelsComplete(PaginatedSearchResults),
    ToneSearchModelsFailed(String),
    ToneSearchIrs,
    ToneSearchIrsComplete(PaginatedSearchResults),
    ToneSearchIrsFailed(String),
    ToneModelPagePrev,
    ToneModelPageNext,
    ToneIrPagePrev,
    ToneIrPageNext,
    ToneModelGearAmp(bool),
    ToneModelGearFullRig(bool),
    ToneModelGearPedal(bool),
    ToneModelGearOutboard(bool),
    ToneModelVariationSelected(String),
    ToneModelDownloaded,
    ToneIrVariationSelected(String),
    ToneIrDownloaded,
    ToneOAuthClientIdChanged(String),
    ToneOAuthBrowserLogin,
    ToneOAuthCompleted,
    ToneOAuthClear,
}

struct State {
    shared: Arc<SharedState>,
    error: Option<String>,
    loading: Option<String>,
    tone_oauth_client_id: String,
    tone_model_query: String,
    tone_model_results: Vec<SearchItem>,
    tone_model_pictures: Vec<Option<maolan_baseview::iced::widget::image::Handle>>,
    tone_model_page: u32,
    tone_model_total_pages: u32,
    tone_model_gear_amp: bool,
    tone_model_gear_full_rig: bool,
    tone_model_gear_pedal: bool,
    tone_model_gear_outboard: bool,
    tone_model_selected_variation: Option<String>,
    tone_ir_query: String,
    tone_ir_results: Vec<SearchItem>,
    tone_ir_pictures: Vec<Option<maolan_baseview::iced::widget::image::Handle>>,
    tone_ir_page: u32,
    tone_ir_total_pages: u32,
    tone_ir_selected_variation: Option<String>,
    tone_oauth_authenticated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VariationOption {
    title: String,
    reference: String,
}

impl std::fmt::Display for VariationOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.title)
    }
}

fn set_error(state: &mut State, error: impl Into<String>) {
    let msg = error.into();
    state.error = Some(msg);
}

fn sync_error_from_shared(state: &mut State) {
    let shared_error = state.shared.last_error.read().clone();
    state.error = shared_error;
    state.loading = None;
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    let (tone_oauth_client_id, init_error) = match tone3000::load_saved_oauth_credentials() {
        Ok(Some(saved)) => (saved.client_id, None),
        Ok(None) => (String::new(), None),
        Err(err) => (String::new(), Some(err)),
    };
    let tone_oauth_authenticated = tone3000::has_valid_oauth_token();
    (
        State {
            shared,
            error: init_error,
            loading: None,
            tone_oauth_client_id,
            tone_model_query: String::new(),
            tone_model_results: Vec::new(),
            tone_model_pictures: Vec::new(),
            tone_model_page: 1,
            tone_model_total_pages: 0,
            tone_model_gear_amp: true,
            tone_model_gear_full_rig: true,
            tone_model_gear_pedal: false,
            tone_model_gear_outboard: false,
            tone_model_selected_variation: None,
            tone_ir_query: String::new(),
            tone_ir_results: Vec::new(),
            tone_ir_pictures: Vec::new(),
            tone_ir_page: 1,
            tone_ir_total_pages: 0,
            tone_ir_selected_variation: None,
            tone_oauth_authenticated,
        },
        Task::none(),
    )
}

const PAGE_SIZE: u32 = 20;

fn build_nam_gears_filter(state: &State) -> Option<String> {
    let mut parts = Vec::new();
    if state.tone_model_gear_amp {
        parts.push("amp");
    }
    if state.tone_model_gear_full_rig {
        parts.push("full-rig");
    }
    if state.tone_model_gear_pedal {
        parts.push("pedal");
    }
    if state.tone_model_gear_outboard {
        parts.push("outboard");
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("_"))
    }
}

fn start_search_irs(state: &mut State) -> Task<Message> {
    let query = state.tone_ir_query.trim().to_string();
    state.tone_ir_results.clear();
    state.tone_ir_pictures.clear();
    state.tone_ir_selected_variation = None;
    state.tone_ir_page = 1;
    state.tone_ir_total_pages = 0;
    state.tone_model_results.clear();
    state.tone_model_pictures.clear();
    state.tone_model_selected_variation = None;
    state.tone_model_page = 1;
    state.tone_model_total_pages = 0;
    if query.is_empty() {
        set_error(state, "Tone3000 IR search query is empty");
        return Task::none();
    }
    state.loading = Some("Searching IR...".to_string());
    state.error = None;
    let page = state.tone_ir_page;
    Task::perform(
        async move {
            std::thread::spawn(move || {
                tone3000::search(AssetKind::Ir, &query, page, PAGE_SIZE, None)
            })
            .join()
            .unwrap()
        },
        |result| match result {
            Ok(results) => Message::ToneSearchIrsComplete(results),
            Err(err) => Message::ToneSearchIrsFailed(err),
        },
    )
}

fn start_search_models(state: &mut State) -> Task<Message> {
    let query = state.tone_model_query.trim().to_string();
    state.tone_model_results.clear();
    state.tone_model_pictures.clear();
    state.tone_model_selected_variation = None;
    state.tone_model_page = 1;
    state.tone_model_total_pages = 0;
    state.tone_ir_results.clear();
    state.tone_ir_pictures.clear();
    state.tone_ir_selected_variation = None;
    state.tone_ir_page = 1;
    state.tone_ir_total_pages = 0;
    if query.is_empty() {
        set_error(state, "Tone3000 NAM search query is empty");
        return Task::none();
    }
    state.loading = Some("Searching NAM...".to_string());
    state.error = None;
    let page = state.tone_model_page;
    let gears = build_nam_gears_filter(state);
    Task::perform(
        async move {
            std::thread::spawn(move || {
                tone3000::search(AssetKind::Nam, &query, page, PAGE_SIZE, gears.as_deref())
            })
            .join()
            .unwrap()
        },
        |result| match result {
            Ok(results) => Message::ToneSearchModelsComplete(results),
            Err(err) => Message::ToneSearchModelsFailed(err),
        },
    )
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::LoadModel => {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("NAM model", &["nam"])
                .pick_file()
            {
                state.shared.load_model(path.display().to_string());
                sync_error_from_shared(state);
            }
            Task::none()
        }
        Message::LoadIr => {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Impulse response", &["wav"])
                .pick_file()
            {
                state.shared.load_ir(path.display().to_string());
                sync_error_from_shared(state);
            }
            Task::none()
        }
        Message::ClearModel => {
            state.shared.clear_model();
            sync_error_from_shared(state);
            Task::none()
        }
        Message::ClearIr => {
            state.shared.clear_ir();
            sync_error_from_shared(state);
            Task::none()
        }
        Message::SetParam(id, value) => {
            state.shared.set_param(id, value as f64);
            Task::none()
        }
        Message::SetBoolParam(id, value) => {
            state.shared.set_param(id, if value { 1.0 } else { 0.0 });
            Task::none()
        }
        Message::SetOutputMode(mode) => {
            state.shared.set_param(ParamId::OutputMode, mode as f64);
            Task::none()
        }
        Message::ToneModelQueryChanged(value) => {
            state.tone_model_query = value;
            state.tone_model_page = 1;
            Task::none()
        }
        Message::ToneIrQueryChanged(value) => {
            state.tone_ir_query = value;
            state.tone_ir_page = 1;
            Task::none()
        }
        Message::ToneSearchModels => start_search_models(state),
        Message::ToneSearchModelsComplete(results) => {
            state.tone_model_pictures = results
                .items
                .iter()
                .map(|item| {
                    item.picture.as_ref().map(|bytes: &Vec<u8>| {
                        maolan_baseview::iced::widget::image::Handle::from_bytes(bytes.clone())
                    })
                })
                .collect();
            state.tone_model_results = results.items;
            state.tone_model_page = results.page;
            state.tone_model_total_pages = results.total_pages;
            state.loading = None;
            Task::none()
        }
        Message::ToneSearchModelsFailed(err) => {
            state.loading = None;
            set_error(state, err);
            Task::none()
        }
        Message::ToneSearchIrs => start_search_irs(state),
        Message::ToneSearchIrsComplete(results) => {
            state.tone_ir_pictures = results
                .items
                .iter()
                .map(|item| {
                    item.picture.as_ref().map(|bytes: &Vec<u8>| {
                        maolan_baseview::iced::widget::image::Handle::from_bytes(bytes.clone())
                    })
                })
                .collect();
            state.tone_ir_results = results.items;
            state.tone_ir_page = results.page;
            state.tone_ir_total_pages = results.total_pages;
            state.loading = None;
            Task::none()
        }
        Message::ToneSearchIrsFailed(err) => {
            state.loading = None;
            set_error(state, err);
            Task::none()
        }
        Message::ToneModelPagePrev => {
            if state.tone_model_page > 1 {
                state.tone_model_page -= 1;
                return start_search_models(state);
            }
            Task::none()
        }
        Message::ToneModelPageNext => {
            state.tone_model_page += 1;
            start_search_models(state)
        }
        Message::ToneIrPagePrev => {
            if state.tone_ir_page > 1 {
                state.tone_ir_page -= 1;
                return start_search_irs(state);
            }
            Task::none()
        }
        Message::ToneIrPageNext => {
            state.tone_ir_page += 1;
            start_search_irs(state)
        }
        Message::ToneModelGearAmp(value) => {
            state.tone_model_gear_amp = value;
            Task::none()
        }
        Message::ToneModelGearFullRig(value) => {
            state.tone_model_gear_full_rig = value;
            Task::none()
        }
        Message::ToneModelGearPedal(value) => {
            state.tone_model_gear_pedal = value;
            Task::none()
        }
        Message::ToneModelGearOutboard(value) => {
            state.tone_model_gear_outboard = value;
            Task::none()
        }
        Message::ToneModelVariationSelected(reference) => {
            let reference = reference.trim();
            if reference.is_empty() {
                set_error(state, "Tone3000 NAM variation reference is empty");
                return Task::none();
            }
            state.tone_model_selected_variation = Some(reference.to_string());
            let reference = reference.to_string();
            let shared = state.shared.clone();
            state.loading = Some("Downloading NAM...".to_string());
            state.error = None;
            Task::perform(
                async move {
                    std::thread::spawn(move || {
                        match tone3000::download_to_temp(AssetKind::Nam, &reference) {
                            Ok(path) => shared.load_model(path.display().to_string()),
                            Err(err) => {
                                *shared.last_error.write() = Some(err);
                            }
                        }
                    })
                    .join()
                    .unwrap()
                },
                |_| Message::ToneModelDownloaded,
            )
        }
        Message::ToneModelDownloaded => {
            state.loading = None;
            sync_error_from_shared(state);
            Task::none()
        }

        Message::ToneOAuthClientIdChanged(value) => {
            state.tone_oauth_client_id = value;
            Task::none()
        }
        Message::ToneOAuthBrowserLogin => {
            let client_id = state.tone_oauth_client_id.clone();
            let shared = state.shared.clone();
            state.loading = Some("OAuth Browser Login in progress...".to_string());
            state.error = None;
            Task::perform(
                async move {
                    std::thread::spawn(move || {
                        match tone3000::oauth_login_with_browser(&client_id) {
                            Ok(()) => *shared.last_error.write() = None,
                            Err(err) => *shared.last_error.write() = Some(err),
                        }
                    })
                    .join()
                    .unwrap()
                },
                |_| Message::ToneOAuthCompleted,
            )
        }
        Message::ToneOAuthCompleted => {
            state.loading = None;
            sync_error_from_shared(state);
            state.tone_oauth_authenticated = tone3000::has_valid_oauth_token();
            Task::none()
        }
        Message::ToneOAuthClear => {
            state.error = None;
            match tone3000::clear_oauth_credentials() {
                Ok(()) => {
                    state.tone_oauth_authenticated = false;
                    state.tone_oauth_client_id.clear();
                }
                Err(err) => set_error(state, err),
            }
            Task::none()
        }
        Message::ToneIrVariationSelected(reference) => {
            let reference = reference.trim();
            if reference.is_empty() {
                set_error(state, "Tone3000 IR variation reference is empty");
                return Task::none();
            }
            state.tone_ir_selected_variation = Some(reference.to_string());
            let reference = reference.to_string();
            let shared = state.shared.clone();
            state.loading = Some("Downloading IR...".to_string());
            state.error = None;
            Task::perform(
                async move {
                    std::thread::spawn(move || {
                        match tone3000::download_to_temp(AssetKind::Ir, &reference) {
                            Ok(path) => shared.load_ir(path.display().to_string()),
                            Err(err) => {
                                *shared.last_error.write() = Some(err);
                            }
                        }
                    })
                    .join()
                    .unwrap()
                },
                |_| Message::ToneIrDownloaded,
            )
        }
        Message::ToneIrDownloaded => {
            state.loading = None;
            sync_error_from_shared(state);
            Task::none()
        }
    }
}

fn view(state: &State) -> Element<'_, Message> {
    let model_path = state.shared.model_path.read().clone();
    let model_label = if model_path.is_empty() {
        "No model selected".to_string()
    } else {
        Path::new(&model_path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|s| s.to_string())
            .unwrap_or(model_path)
    };

    let ir_path = state.shared.ir_path.read().clone();
    let ir_label = if ir_path.is_empty() {
        "No IR selected".to_string()
    } else {
        Path::new(&ir_path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|s| s.to_string())
            .unwrap_or(ir_path)
    };

    let p = |id: ParamId| state.shared.params.get(id) as f32;
    let b = |id: ParamId| state.shared.params.get_bool(id);

    let mut content = column![text("Rural Modeler").size(24)]
        .spacing(12)
        .align_x(Alignment::Start);

    content = content.push(
        row![
            knob(
                "Input",
                ParamId::InputLevel,
                p(ParamId::InputLevel),
                "dB",
                0.1
            ),
            knob(
                "Threshold",
                ParamId::NoiseGateThreshold,
                p(ParamId::NoiseGateThreshold),
                "dB",
                0.1
            ),
            knob("Bass", ParamId::ToneBass, p(ParamId::ToneBass), "", 0.1),
            knob("Middle", ParamId::ToneMid, p(ParamId::ToneMid), "", 0.1),
            knob(
                "Treble",
                ParamId::ToneTreble,
                p(ParamId::ToneTreble),
                "",
                0.1
            ),
            knob(
                "Output",
                ParamId::OutputLevel,
                p(ParamId::OutputLevel),
                "dB",
                0.1
            ),
        ]
        .spacing(12),
    );

    content = content.push(
        row![
            checkbox(b(ParamId::NoiseGateActive))
                .label("Noise Gate")
                .on_toggle(|v| Message::SetBoolParam(ParamId::NoiseGateActive, v)),
            checkbox(b(ParamId::EqActive))
                .label("EQ")
                .on_toggle(|v| Message::SetBoolParam(ParamId::EqActive, v)),
            checkbox(b(ParamId::IrToggle))
                .label("IR")
                .on_toggle(|v| Message::SetBoolParam(ParamId::IrToggle, v)),
        ]
        .spacing(16),
    );

    content = content.push(
        row![
            checkbox(b(ParamId::CalibrateInput))
                .label("Calibrate Input")
                .on_toggle(|v| Message::SetBoolParam(ParamId::CalibrateInput, v)),
            knob(
                "Input Cal",
                ParamId::InputCalibrationLevel,
                p(ParamId::InputCalibrationLevel),
                "dBu",
                0.1
            ),
        ]
        .spacing(24)
        .align_y(Alignment::Center),
    );

    let output_mode = state.shared.params.get_enum(ParamId::OutputMode).min(2);
    content = content.push(
        row![
            text("Output Mode").size(16),
            radio("Raw", 0u8, Some(output_mode as u8), Message::SetOutputMode),
            radio(
                "Normalized",
                1u8,
                Some(output_mode as u8),
                Message::SetOutputMode
            ),
            radio(
                "Calibrated",
                2u8,
                Some(output_mode as u8),
                Message::SetOutputMode
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    );

    content = content.push(text("Model"));
    content = content.push(
        row![
            button(text("Load")).on_press(Message::LoadModel),
            button(text("Clear")).on_press(Message::ClearModel),
        ]
        .spacing(8),
    );
    content = content.push(text(model_label).size(14));

    content = content.push(text("IR"));
    content = content.push(
        row![
            button(text("Load")).on_press(Message::LoadIr),
            button(text("Clear")).on_press(Message::ClearIr),
        ]
        .spacing(8),
    );
    content = content.push(text(ir_label).size(14));

    content = content.push(text("Tone3000").size(16));
    content = content.push(
        row![
            text_input("Tone3000 client_id", &state.tone_oauth_client_id)
                .on_input(Message::ToneOAuthClientIdChanged)
                .width(Length::Fill),
            button(text("OAuth Login")).on_press(Message::ToneOAuthBrowserLogin),
            button(text("Clear OAuth")).on_press(Message::ToneOAuthClear),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );

    if state.tone_oauth_authenticated {
        content = content.push(
            row![
                text_input("NAM id/url or search query", &state.tone_model_query)
                    .on_input(Message::ToneModelQueryChanged)
                    .on_submit(Message::ToneSearchModels)
                    .width(Length::Fill),
                button(text("Search")).on_press(Message::ToneSearchModels),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
        content = content.push(
            row![
                checkbox(state.tone_model_gear_amp)
                    .label("Amp Head")
                    .on_toggle(Message::ToneModelGearAmp),
                checkbox(state.tone_model_gear_full_rig)
                    .label("Full Rig")
                    .on_toggle(Message::ToneModelGearFullRig),
                checkbox(state.tone_model_gear_pedal)
                    .label("Pedal")
                    .on_toggle(Message::ToneModelGearPedal),
                checkbox(state.tone_model_gear_outboard)
                    .label("Outboard")
                    .on_toggle(Message::ToneModelGearOutboard),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        );
        content = content.push(
            row![
                text_input("IR id/url or search query", &state.tone_ir_query)
                    .on_input(Message::ToneIrQueryChanged)
                    .on_submit(Message::ToneSearchIrs)
                    .width(Length::Fill),
                button(text("Search")).on_press(Message::ToneSearchIrs),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );

        if let Some(loading) = &state.loading {
            content = content.push(text(format!("⏳ {loading}")).size(14));
        }

        if let Some(error) = &state.error {
            content = content.push(text(error));
        }

        for (idx, item) in state.tone_ir_results.iter().enumerate() {
            let options = variation_options(&item.variations);
            let selected = options
                .iter()
                .find(|choice| {
                    state
                        .tone_ir_selected_variation
                        .as_ref()
                        .is_some_and(|sel| sel == &choice.reference)
                })
                .cloned();
            let variation_widget: Element<'_, Message> = if options.is_empty() {
                text("No variations").into()
            } else {
                maolan_baseview::iced::widget::pick_list(options, selected, |choice| {
                    Message::ToneIrVariationSelected(choice.reference)
                })
                .placeholder("Variation")
                .into()
            };
            let image_widget: Element<'_, Message> =
                if let Some(handle) = state.tone_ir_pictures.get(idx).and_then(|h| h.clone()) {
                    Image::new(handle)
                        .width(Length::Fixed(40.0))
                        .height(Length::Fixed(40.0))
                        .into()
                } else {
                    container(text(""))
                        .width(Length::Fixed(40.0))
                        .height(Length::Fixed(40.0))
                        .into()
                };
            let result_row = row![
                image_widget,
                text(item.name.clone())
                    .size(13)
                    .width(Length::FillPortion(2)),
                variation_widget
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            content = content.push(result_row);
        }

        if state.tone_ir_total_pages > 0 {
            let page_label = format!(
                "Page {} / {}",
                state.tone_ir_page, state.tone_ir_total_pages
            );
            content = content.push(
                row![
                    button(text("< Prev")).on_press_maybe(
                        (state.tone_ir_page > 1).then_some(Message::ToneIrPagePrev)
                    ),
                    text(page_label).size(13),
                    button(text("Next >")).on_press_maybe(
                        (state.tone_ir_page < state.tone_ir_total_pages)
                            .then_some(Message::ToneIrPageNext)
                    ),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }

        for (idx, item) in state.tone_model_results.iter().enumerate() {
            let options = variation_options(&item.variations);
            let selected = options
                .iter()
                .find(|choice| {
                    state
                        .tone_model_selected_variation
                        .as_ref()
                        .is_some_and(|sel| sel == &choice.reference)
                })
                .cloned();
            let variation_widget: Element<'_, Message> = if options.is_empty() {
                text("No variations").into()
            } else {
                maolan_baseview::iced::widget::pick_list(options, selected, |choice| {
                    Message::ToneModelVariationSelected(choice.reference)
                })
                .placeholder("Variation")
                .into()
            };
            let image_widget: Element<'_, Message> =
                if let Some(handle) = state.tone_model_pictures.get(idx).and_then(|h| h.clone()) {
                    Image::new(handle)
                        .width(Length::Fixed(40.0))
                        .height(Length::Fixed(40.0))
                        .into()
                } else {
                    container(text(""))
                        .width(Length::Fixed(40.0))
                        .height(Length::Fixed(40.0))
                        .into()
                };
            let result_row = row![
                image_widget,
                text(item.name.clone())
                    .size(13)
                    .width(Length::FillPortion(2)),
                variation_widget
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            content = content.push(result_row);
        }

        if state.tone_model_total_pages > 0 {
            let page_label = format!(
                "Page {} / {}",
                state.tone_model_page, state.tone_model_total_pages
            );
            content = content.push(
                row![
                    button(text("< Prev")).on_press_maybe(
                        (state.tone_model_page > 1).then_some(Message::ToneModelPagePrev)
                    ),
                    text(page_label).size(13),
                    button(text("Next >")).on_press_maybe(
                        (state.tone_model_page < state.tone_model_total_pages)
                            .then_some(Message::ToneModelPageNext)
                    ),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
    } else {
        if let Some(loading) = &state.loading {
            content = content.push(text(format!("⏳ {loading}")).size(14));
        }

        if let Some(error) = &state.error {
            content = content.push(text(error));
        }
    }

    container(scrollable(content))
        .padding(16)
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
    .fill_from_start()
    .width(Length::Fixed(86.0))
    .height(Length::Fixed(86.0));

    let value_text = if units.is_empty() {
        format!("{value:.1}")
    } else {
        format!("{value:.1} {units}")
    };

    container(
        column![text(label).size(14), slider, text(value_text).size(13)]
            .spacing(4)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(96.0))
    .into()
}

fn variation_options(variations: &[SearchVariation]) -> Vec<VariationOption> {
    variations
        .iter()
        .map(|variation| VariationOption {
            title: variation.title.clone(),
            reference: variation.reference.clone(),
        })
        .collect()
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
                title: String::from("Rural Modeler"),
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
                .name("rural-modeler-gui".to_string())
                .spawn(move || {
                    let settings = maolan_baseview::iced::IcedBaseviewSettings {
                        window: maolan_baseview::iced::baseview::WindowOpenOptions {
                            title: String::from("Rural Modeler"),
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

#[cfg(test)]
mod tests {
    use super::GuiBridge;

    #[test]
    fn create_succeeds_for_supported_api() {
        let mut bridge = GuiBridge::default();
        assert!(bridge.create(
            std::sync::Arc::new(crate::plugin::SharedState::default()),
            super::preferred_api(),
            false
        ));
    }

    #[test]
    fn create_fails_for_unsupported_api() {
        let mut bridge = GuiBridge::default();
        assert!(!bridge.create(
            std::sync::Arc::new(crate::plugin::SharedState::default()),
            c"unsupported",
            false
        ));
    }

    #[test]
    fn create_succeeds_for_floating() {
        let mut bridge = GuiBridge::default();
        assert!(bridge.create(
            std::sync::Arc::new(crate::plugin::SharedState::default()),
            super::preferred_api(),
            true
        ));
    }

    #[test]
    fn destroy_resets_created() {
        let mut bridge = GuiBridge::default();
        bridge.create(
            std::sync::Arc::new(crate::plugin::SharedState::default()),
            super::preferred_api(),
            false,
        );
        bridge.destroy();
        assert!(!bridge.set_parent(
            std::sync::Arc::new(crate::plugin::SharedState::default()),
            #[cfg(all(unix, not(target_os = "macos")))]
            super::ParentWindowHandle::X11(0),
            #[cfg(target_os = "macos")]
            super::ParentWindowHandle::Cocoa(std::ptr::null_mut()),
            #[cfg(target_os = "windows")]
            super::ParentWindowHandle::Win32(std::ptr::null_mut()),
        ));
    }
}
