use std::{
    ffi::CStr,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use maolan_baseview::iced::widget::image::Image;
use maolan_baseview::iced::{
    Alignment, Background, Border, Color, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{
        button, button::Status, checkbox, column, container, mouse_area, radio, row, scrollable,
        text, text_input,
    },
    window,
};
#[cfg(target_os = "macos")]
use maolan_clap::ffi::CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
use maolan_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use maolan_clap::ffi::CLAP_WINDOW_API_X11;
use maolan_widgets::arch_slider::arch_slider;
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
    modeler::{
        params::{PARAMS, ParamId},
        plugin::SharedState,
        tone3000::{
            self, AssetKind, PaginatedSearchResults, SearchFilters, SearchItem, SearchVariation,
            Taxonomies, TaxonomyItem,
        },
    },
};

pub const EDITOR_WIDTH: u32 = 720;
pub const EDITOR_HEIGHT: u32 = 560;

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
    LoadModel,
    LoadIr,
    ClearModel,
    ClearIr,
    SetParam(ParamId, f32),
    SetBoolParam(ParamId, bool),
    SetOutputMode(u8),
    SetEnumParam(ParamId, f32),
    ReleaseParam(ParamId),
    ToneModelQueryChanged(String),
    ToneIrQueryChanged(String),
    ToneSearchModels,
    ToneSearchModelsComplete(PaginatedSearchResults),
    ToneSearchModelsFailed(String),
    ToneSearchIrs,
    ToneSearchIrsComplete(PaginatedSearchResults),
    ToneSearchIrsFailed(String),
    TonePictureLoaded(AssetKind, usize, String, Result<Vec<u8>, String>),
    ToneVariationsLoaded(AssetKind, usize, String, Vec<SearchVariation>),
    ToneTaxonomiesLoad,
    ToneTaxonomiesLoaded(Result<Taxonomies, String>),
    ToneBrowserModeSelected(ToneBrowserMode),
    ToggleToneTagsMenu,
    ToneTagToggled(String, bool),
    ToneTagsClear,
    ToggleToneModelsMenu,
    ToneModelFilterToggled(String, bool),
    ToneModelsClear,
    ToggleToneCreatorsMenu,
    ToneCreatorToggled(String, bool),
    ToneCreatorsClear,
    ToneCreatorQueryChanged(String),
    ToneCreatorsSearch,
    ToneCreatorsLoaded(Result<Vec<TaxonomyItem>, String>),
    ToneModelPagePrev,
    ToneModelPageNext,
    ToneIrPagePrev,
    ToneIrPageNext,
    ToneModelGearAmp(bool),
    ToneModelGearFullRig(bool),
    ToneModelGearPedal(bool),
    ToneModelGearOutboard(bool),
    ToneModelVariationSelected(String, String, Option<String>),
    ToneModelDownloaded,
    ToneIrVariationSelected(String, String, Option<String>),
    ToneIrDownloaded,
    ToneOAuthClientIdChanged(String),
    ToneOAuthBrowserLogin,
    ToneOAuthCompleted,
    ToneOAuthClear,
    ToggleToneSettings,
    ToggleBassModeMenu,
    ToggleTrebleModeMenu,
    WindowClosed,
}

struct State {
    shared: Arc<SharedState>,
    active_gestures: Vec<bool>,
    error: Option<String>,
    loading: Option<String>,
    tone_oauth_client_id: String,
    tone_browser_mode: ToneBrowserMode,
    tone_model_query: String,
    tone_model_results: Vec<SearchItem>,
    tone_model_pictures: Vec<Option<maolan_baseview::iced::widget::image::Handle>>,
    tone_model_picture_paths: Vec<Option<String>>,
    tone_model_variations_loading: Vec<bool>,
    tone_model_page: u32,
    tone_model_total_pages: u32,
    tone_model_gear_amp: bool,
    tone_model_gear_full_rig: bool,
    tone_model_gear_pedal: bool,
    tone_model_gear_outboard: bool,
    tone_model_selected_variation: Option<String>,
    tone_tags: Vec<TaxonomyItem>,
    tone_makes: Vec<TaxonomyItem>,
    tone_creators: Vec<TaxonomyItem>,
    tone_creator_query: String,
    tone_selected_tags: Vec<String>,
    tone_selected_makes: Vec<String>,
    tone_selected_creators: Vec<String>,
    tone_taxonomy_loading: bool,
    tone_taxonomy_error: Option<String>,
    tone_tags_menu_open: bool,
    tone_models_menu_open: bool,
    tone_creators_menu_open: bool,
    tone_ir_query: String,
    tone_ir_results: Vec<SearchItem>,
    tone_ir_pictures: Vec<Option<maolan_baseview::iced::widget::image::Handle>>,
    tone_ir_picture_paths: Vec<Option<String>>,
    tone_ir_variations_loading: Vec<bool>,
    tone_ir_page: u32,
    tone_ir_total_pages: u32,
    tone_ir_selected_variation: Option<String>,
    tone_oauth_authenticated: bool,
    tone_settings_open: bool,
    bass_mode_menu_open: bool,
    treble_mode_menu_open: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToneBrowserMode {
    Nam,
    Ir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToneFilterMode {
    Shelf,
    BandEq,
}

impl ToneFilterMode {
    fn from_value(value: f32) -> Self {
        if value >= 0.5 {
            Self::BandEq
        } else {
            Self::Shelf
        }
    }

    fn as_f32(self) -> f32 {
        match self {
            Self::Shelf => 0.0,
            Self::BandEq => 1.0,
        }
    }
}

impl std::fmt::Display for ToneFilterMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shelf => f.write_str("Shelf"),
            Self::BandEq => f.write_str("Band EQ"),
        }
    }
}

fn set_error(state: &mut State, error: impl Into<String>) {
    let msg = error.into();
    state.error = Some(msg);
}

fn clear_tone_oauth(state: &mut State) -> Task<Message> {
    state.error = None;
    match tone3000::clear_oauth_credentials() {
        Ok(()) => {
            state.tone_oauth_authenticated = false;
            state.tone_oauth_client_id.clear();
            state.tone_tags.clear();
            state.tone_makes.clear();
            state.tone_creators.clear();
            state.tone_creator_query.clear();
            state.tone_selected_tags.clear();
            state.tone_selected_makes.clear();
            state.tone_selected_creators.clear();
            state.tone_taxonomy_loading = false;
            state.tone_taxonomy_error = None;
        }
        Err(err) => set_error(state, err),
    }
    Task::none()
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
            active_gestures: vec![false; ParamId::COUNT],
            error: init_error,
            loading: None,
            tone_oauth_client_id,
            tone_browser_mode: ToneBrowserMode::Nam,
            tone_model_query: String::new(),
            tone_model_results: Vec::new(),
            tone_model_pictures: Vec::new(),
            tone_model_picture_paths: Vec::new(),
            tone_model_variations_loading: Vec::new(),
            tone_model_page: 1,
            tone_model_total_pages: 0,
            tone_model_gear_amp: true,
            tone_model_gear_full_rig: true,
            tone_model_gear_pedal: false,
            tone_model_gear_outboard: false,
            tone_model_selected_variation: None,
            tone_tags: Vec::new(),
            tone_makes: Vec::new(),
            tone_creators: Vec::new(),
            tone_creator_query: String::new(),
            tone_selected_tags: Vec::new(),
            tone_selected_makes: Vec::new(),
            tone_selected_creators: Vec::new(),
            tone_taxonomy_loading: tone_oauth_authenticated,
            tone_taxonomy_error: None,
            tone_tags_menu_open: false,
            tone_models_menu_open: false,
            tone_creators_menu_open: false,
            tone_ir_query: String::new(),
            tone_ir_results: Vec::new(),
            tone_ir_pictures: Vec::new(),
            tone_ir_picture_paths: Vec::new(),
            tone_ir_variations_loading: Vec::new(),
            tone_ir_page: 1,
            tone_ir_total_pages: 0,
            tone_ir_selected_variation: None,
            tone_oauth_authenticated,
            tone_settings_open: false,
            bass_mode_menu_open: false,
            treble_mode_menu_open: false,
        },
        if tone_oauth_authenticated {
            start_load_taxonomies()
        } else {
            Task::none()
        },
    )
}

const PAGE_SIZE: u32 = 20;

fn build_nam_gears_filter(state: &State) -> Option<String> {
    let mut parts = Vec::new();
    if state.tone_model_gear_amp {
        parts.push("amp");
    }
    if state.tone_model_gear_full_rig {
        parts.push("amp-cab");
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

fn search_filters(state: &State) -> SearchFilters {
    SearchFilters {
        tags: state.tone_selected_tags.clone(),
        makes: state.tone_selected_makes.clone(),
        creators: state.tone_selected_creators.clone(),
    }
}

fn file_stem_label(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string())
}

fn write_tone_picture_to_temp(id: &str, picture: &[u8]) -> Option<String> {
    if picture.is_empty() {
        return None;
    }
    let safe_id: String = id
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    let name = if safe_id.is_empty() {
        "tone3000-picture".to_string()
    } else {
        format!("tone3000-picture-{safe_id}")
    };
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, picture)
        .ok()
        .map(|()| path.to_string_lossy().into_owned())
}

fn start_load_taxonomies() -> Task<Message> {
    Task::perform(
        async move {
            std::thread::spawn(tone3000::load_taxonomies)
                .join()
                .unwrap()
        },
        Message::ToneTaxonomiesLoaded,
    )
}

fn start_search_creators(state: &mut State) -> Task<Message> {
    let query = state.tone_creator_query.trim().to_string();
    state.tone_taxonomy_loading = true;
    state.tone_taxonomy_error = None;
    Task::perform(
        async move {
            std::thread::spawn(move || {
                if query.is_empty() {
                    tone3000::load_taxonomies().map(|taxonomies| taxonomies.creators)
                } else {
                    tone3000::search_creators(&query)
                }
            })
            .join()
            .unwrap()
        },
        Message::ToneCreatorsLoaded,
    )
}

fn start_search_irs(state: &mut State, reset_page: bool) -> Task<Message> {
    let query = state.tone_ir_query.trim().to_string();
    state.tone_ir_results.clear();
    state.tone_ir_pictures.clear();
    state.tone_ir_picture_paths.clear();
    state.tone_ir_variations_loading.clear();
    state.tone_ir_selected_variation = None;
    if reset_page {
        state.tone_ir_page = 1;
    }
    state.tone_ir_total_pages = 0;
    state.tone_model_results.clear();
    state.tone_model_pictures.clear();
    state.tone_model_picture_paths.clear();
    state.tone_model_variations_loading.clear();
    state.tone_model_selected_variation = None;
    state.tone_model_page = 1;
    state.tone_model_total_pages = 0;
    state.loading = Some("Searching IR...".to_string());
    state.error = None;
    let page = state.tone_ir_page;
    let filters = search_filters(state);
    Task::perform(
        async move {
            std::thread::spawn(move || {
                tone3000::search(AssetKind::Ir, &query, page, PAGE_SIZE, None, &filters)
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

fn start_fetch_pictures(kind: AssetKind, items: &[SearchItem]) -> Task<Message> {
    let tasks = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let url = item.picture_url.clone()?;
            let id = item.id.clone();
            Some(Task::perform(
                async move {
                    std::thread::spawn(move || tone3000::fetch_picture(&url))
                        .join()
                        .unwrap()
                },
                move |result| Message::TonePictureLoaded(kind, index, id.clone(), result),
            ))
        })
        .collect::<Vec<_>>();
    Task::batch(tasks)
}

fn start_fetch_variations(kind: AssetKind, items: &[SearchItem]) -> Task<Message> {
    let tasks = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let id = item.id.clone();
            let message_id = id.clone();
            Task::perform(
                async move {
                    std::thread::spawn(move || tone3000::fetch_variations(kind, &id))
                        .join()
                        .unwrap()
                },
                move |variations| {
                    Message::ToneVariationsLoaded(kind, index, message_id.clone(), variations)
                },
            )
        })
        .collect::<Vec<_>>();
    Task::batch(tasks)
}

fn start_search_models(state: &mut State, reset_page: bool) -> Task<Message> {
    let query = state.tone_model_query.trim().to_string();
    state.tone_model_results.clear();
    state.tone_model_pictures.clear();
    state.tone_model_picture_paths.clear();
    state.tone_model_variations_loading.clear();
    state.tone_model_selected_variation = None;
    if reset_page {
        state.tone_model_page = 1;
    }
    state.tone_model_total_pages = 0;
    state.tone_ir_results.clear();
    state.tone_ir_pictures.clear();
    state.tone_ir_picture_paths.clear();
    state.tone_ir_variations_loading.clear();
    state.tone_ir_selected_variation = None;
    state.tone_ir_page = 1;
    state.tone_ir_total_pages = 0;
    state.loading = Some("Searching NAM...".to_string());
    state.error = None;
    let page = state.tone_model_page;
    let gears = build_nam_gears_filter(state);
    let filters = search_filters(state);
    Task::perform(
        async move {
            std::thread::spawn(move || {
                tone3000::search(
                    AssetKind::Nam,
                    &query,
                    page,
                    PAGE_SIZE,
                    gears.as_deref(),
                    &filters,
                )
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
                state.shared.load_model(path.display().to_string(), true);
                sync_error_from_shared(state);
            }
            Task::none()
        }
        Message::LoadIr => {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Impulse response", &["wav"])
                .pick_file()
            {
                state.shared.load_ir(path.display().to_string(), true);
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
            let idx = id.as_index();
            if !state.active_gestures[idx] {
                state.active_gestures[idx] = true;
                state.shared.mark_gesture_begin_pending(id);
            }
            state.shared.set_param_outbound_only(id, value as f64);
            Task::none()
        }
        Message::ReleaseParam(id) => {
            let idx = id.as_index();
            if state.active_gestures[idx] {
                state.active_gestures[idx] = false;
                state.shared.mark_gesture_end_pending(id);
            }
            Task::none()
        }
        Message::SetBoolParam(id, value) => {
            state.shared.mark_gesture_begin_pending(id);
            state
                .shared
                .set_param_outbound_only(id, if value { 1.0 } else { 0.0 });
            state.shared.mark_gesture_end_pending(id);
            Task::none()
        }
        Message::SetOutputMode(mode) => {
            state.shared.mark_gesture_begin_pending(ParamId::OutputMode);
            state
                .shared
                .set_param_outbound_only(ParamId::OutputMode, mode as f64);
            state.shared.mark_gesture_end_pending(ParamId::OutputMode);
            Task::none()
        }
        Message::SetEnumParam(id, value) => {
            state.shared.mark_gesture_begin_pending(id);
            state.shared.set_param_outbound_only(id, value as f64);
            state.shared.mark_gesture_end_pending(id);
            match id {
                ParamId::ToneBassMode => state.bass_mode_menu_open = false,
                ParamId::ToneTrebleMode => state.treble_mode_menu_open = false,
                _ => {}
            }
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
        Message::ToneSearchModels => start_search_models(state, true),
        Message::ToneSearchModelsComplete(results) => {
            let picture_task = start_fetch_pictures(AssetKind::Nam, &results.items);
            let variations_task = start_fetch_variations(AssetKind::Nam, &results.items);
            state.tone_model_pictures = vec![None; results.items.len()];
            state.tone_model_picture_paths = vec![None; results.items.len()];
            state.tone_model_variations_loading = vec![true; results.items.len()];
            state.tone_model_results = results.items;
            state.tone_model_page = results.page;
            state.tone_model_total_pages = results.total_pages;
            state.loading = None;
            Task::batch([picture_task, variations_task])
        }
        Message::ToneSearchModelsFailed(err) => {
            state.loading = None;
            set_error(state, err);
            Task::none()
        }
        Message::ToneSearchIrs => start_search_irs(state, true),
        Message::ToneSearchIrsComplete(results) => {
            let picture_task = start_fetch_pictures(AssetKind::Ir, &results.items);
            let variations_task = start_fetch_variations(AssetKind::Ir, &results.items);
            state.tone_ir_pictures = vec![None; results.items.len()];
            state.tone_ir_picture_paths = vec![None; results.items.len()];
            state.tone_ir_variations_loading = vec![true; results.items.len()];
            state.tone_ir_results = results.items;
            state.tone_ir_page = results.page;
            state.tone_ir_total_pages = results.total_pages;
            state.loading = None;
            Task::batch([picture_task, variations_task])
        }
        Message::ToneSearchIrsFailed(err) => {
            state.loading = None;
            set_error(state, err);
            Task::none()
        }
        Message::TonePictureLoaded(kind, index, id, result) => {
            let (items, pictures, picture_paths) = match kind {
                AssetKind::Nam => (
                    &state.tone_model_results,
                    &mut state.tone_model_pictures,
                    &mut state.tone_model_picture_paths,
                ),
                AssetKind::Ir => (
                    &state.tone_ir_results,
                    &mut state.tone_ir_pictures,
                    &mut state.tone_ir_picture_paths,
                ),
            };
            let Some(item) = items.get(index) else {
                return Task::none();
            };
            if item.id != id {
                return Task::none();
            }
            let Ok(bytes) = result else {
                return Task::none();
            };
            if let Some(slot) = pictures.get_mut(index) {
                *slot = Some(maolan_baseview::iced::widget::image::Handle::from_bytes(
                    bytes.clone(),
                ));
            }
            if let Some(slot) = picture_paths.get_mut(index) {
                *slot = write_tone_picture_to_temp(&id, &bytes);
            }
            Task::none()
        }
        Message::ToneVariationsLoaded(kind, index, id, variations) => {
            let (items, loading) = match kind {
                AssetKind::Nam => (
                    &mut state.tone_model_results,
                    &mut state.tone_model_variations_loading,
                ),
                AssetKind::Ir => (
                    &mut state.tone_ir_results,
                    &mut state.tone_ir_variations_loading,
                ),
            };
            let Some(item) = items.get_mut(index) else {
                return Task::none();
            };
            if item.id != id {
                return Task::none();
            }
            item.variations = variations;
            if let Some(slot) = loading.get_mut(index) {
                *slot = false;
            }
            Task::none()
        }
        Message::ToneTaxonomiesLoad => {
            state.tone_taxonomy_loading = true;
            state.tone_taxonomy_error = None;
            start_load_taxonomies()
        }
        Message::ToneTaxonomiesLoaded(result) => {
            state.tone_taxonomy_loading = false;
            match result {
                Ok(taxonomies) => {
                    state.tone_tags = taxonomies.tags;
                    state.tone_makes = taxonomies.makes;
                    state.tone_creators = taxonomies.creators;
                    state.tone_taxonomy_error = None;
                }
                Err(err) => state.tone_taxonomy_error = Some(err),
            }
            Task::none()
        }
        Message::ToneBrowserModeSelected(mode) => {
            state.tone_browser_mode = mode;
            Task::none()
        }
        Message::ToggleToneTagsMenu => {
            state.tone_tags_menu_open = !state.tone_tags_menu_open;
            Task::none()
        }
        Message::ToneTagToggled(value, selected) => {
            if selected {
                if !state.tone_selected_tags.iter().any(|tag| tag == &value) {
                    state.tone_selected_tags.push(value);
                }
            } else {
                state.tone_selected_tags.retain(|tag| tag != &value);
            }
            Task::none()
        }
        Message::ToneTagsClear => {
            state.tone_selected_tags.clear();
            state.tone_tags_menu_open = false;
            Task::none()
        }
        Message::ToggleToneModelsMenu => {
            state.tone_models_menu_open = !state.tone_models_menu_open;
            Task::none()
        }
        Message::ToneModelFilterToggled(value, selected) => {
            if selected {
                if !state.tone_selected_makes.iter().any(|make| make == &value) {
                    state.tone_selected_makes.push(value);
                }
            } else {
                state.tone_selected_makes.retain(|make| make != &value);
            }
            Task::none()
        }
        Message::ToneModelsClear => {
            state.tone_selected_makes.clear();
            state.tone_models_menu_open = false;
            Task::none()
        }
        Message::ToggleToneCreatorsMenu => {
            state.tone_creators_menu_open = !state.tone_creators_menu_open;
            Task::none()
        }
        Message::ToneCreatorToggled(value, selected) => {
            if selected {
                if !state
                    .tone_selected_creators
                    .iter()
                    .any(|creator| creator == &value)
                {
                    state.tone_selected_creators.push(value);
                }
            } else {
                state
                    .tone_selected_creators
                    .retain(|creator| creator != &value);
            }
            Task::none()
        }
        Message::ToneCreatorsClear => {
            state.tone_selected_creators.clear();
            state.tone_creators_menu_open = false;
            Task::none()
        }
        Message::ToneCreatorQueryChanged(value) => {
            state.tone_creator_query = value;
            Task::none()
        }
        Message::ToneCreatorsSearch => start_search_creators(state),
        Message::ToneCreatorsLoaded(result) => {
            state.tone_taxonomy_loading = false;
            match result {
                Ok(creators) => {
                    state.tone_creators = creators;
                    state.tone_taxonomy_error = None;
                }
                Err(err) => state.tone_taxonomy_error = Some(err),
            }
            Task::none()
        }
        Message::ToneModelPagePrev => {
            if state.tone_model_page > 1 {
                state.tone_model_page -= 1;
                return start_search_models(state, false);
            }
            Task::none()
        }
        Message::ToneModelPageNext => {
            state.tone_model_page += 1;
            start_search_models(state, false)
        }
        Message::ToneIrPagePrev => {
            if state.tone_ir_page > 1 {
                state.tone_ir_page -= 1;
                return start_search_irs(state, false);
            }
            Task::none()
        }
        Message::ToneIrPageNext => {
            state.tone_ir_page += 1;
            start_search_irs(state, false)
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
        Message::ToneModelVariationSelected(reference, display_name, picture_path) => {
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
                            Ok(path) => shared.load_model_with_preview(
                                path.display().to_string(),
                                true,
                                Some(display_name),
                                picture_path,
                            ),
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
            if value.trim().is_empty() {
                return clear_tone_oauth(state);
            }
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
            if state.tone_oauth_authenticated {
                state.tone_taxonomy_loading = true;
                state.tone_taxonomy_error = None;
                start_load_taxonomies()
            } else {
                Task::none()
            }
        }
        Message::ToneOAuthClear => clear_tone_oauth(state),
        Message::ToggleToneSettings => {
            state.tone_settings_open = !state.tone_settings_open;
            Task::none()
        }
        Message::ToggleBassModeMenu => {
            state.bass_mode_menu_open = !state.bass_mode_menu_open;
            Task::none()
        }
        Message::ToggleTrebleModeMenu => {
            state.treble_mode_menu_open = !state.treble_mode_menu_open;
            Task::none()
        }
        Message::ToneIrVariationSelected(reference, display_name, picture_path) => {
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
                            Ok(path) => shared.load_ir_with_preview(
                                path.display().to_string(),
                                true,
                                Some(display_name),
                                picture_path,
                            ),
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
        Message::WindowClosed => {
            state.shared.request_gui_closed();
            Task::none()
        }
    }
}

fn view(state: &State) -> Element<'_, Message> {
    let model_path = state.shared.model_path.read().clone();
    let model_display_name = state.shared.model_display_name.read().clone();
    let model_label = if model_path.is_empty() {
        String::new()
    } else if model_display_name.trim().is_empty() {
        file_stem_label(&model_path)
    } else {
        model_display_name
    };
    let model_picture_path = state.shared.model_picture_path.read().clone();

    let ir_path = state.shared.ir_path.read().clone();
    let ir_display_name = state.shared.ir_display_name.read().clone();
    let ir_label = if ir_path.is_empty() {
        String::new()
    } else if ir_display_name.trim().is_empty() {
        file_stem_label(&ir_path)
    } else {
        ir_display_name
    };
    let ir_picture_path = state.shared.ir_picture_path.read().clone();

    let p = |id: ParamId| state.shared.params.get(id) as f32;
    let b = |id: ParamId| state.shared.params.get_bool(id);

    let mut content = column![]
        .spacing(12)
        .align_x(Alignment::Start)
        .width(Length::Fill);

    if state.tone_settings_open {
        content = content.push(
            column![
                row![
                    text_input(
                        "Tone3000 publishable key (client_id)",
                        &state.tone_oauth_client_id
                    )
                    .on_input(Message::ToneOAuthClientIdChanged)
                    .width(Length::Fixed(320.0)),
                    button(text("OAuth Login")).on_press(Message::ToneOAuthBrowserLogin),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .align_x(Alignment::End)
            .width(Length::Fill),
        );
    }

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
            tone_knob(
                "Bass",
                ParamId::ToneBass,
                p(ParamId::ToneBass),
                0.1,
                Some(ToneKnobMode {
                    id: ParamId::ToneBassMode,
                    value: p(ParamId::ToneBassMode),
                    menu_open: state.bass_mode_menu_open,
                    toggle_menu: Message::ToggleBassModeMenu,
                }),
            ),
            knob("Middle", ParamId::ToneMid, p(ParamId::ToneMid), "", 0.1),
            tone_knob(
                "Treble",
                ParamId::ToneTreble,
                p(ParamId::ToneTreble),
                0.1,
                Some(ToneKnobMode {
                    id: ParamId::ToneTrebleMode,
                    value: p(ParamId::ToneTrebleMode),
                    menu_open: state.treble_mode_menu_open,
                    toggle_menu: Message::ToggleTrebleModeMenu,
                }),
            ),
            knob(
                "Output",
                ParamId::OutputLevel,
                p(ParamId::OutputLevel),
                "dB",
                0.1
            ),
            knob(
                "Input Cal",
                ParamId::InputCalibrationLevel,
                p(ParamId::InputCalibrationLevel),
                "dBu",
                0.1
            ),
            container(text("")).width(Length::Fill),
            button(text("⚙").size(18))
                .on_press(Message::ToggleToneSettings)
                .style(|theme: &Theme, status: Status| {
                    let mut style = button::secondary(theme, status);
                    style.text_color = style
                        .background
                        .and_then(|background| match background {
                            Background::Color(color) => Some(color),
                            _ => None,
                        })
                        .unwrap_or(style.text_color);
                    style.background = None;
                    style.border.width = 0.0;
                    style
                }),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .width(Length::Fill),
    );

    content = content.push(
        row![
            checkbox(b(ParamId::NoiseGateActive))
                .label("Noise Gate")
                .on_toggle(|v| Message::SetBoolParam(ParamId::NoiseGateActive, v)),
            checkbox(b(ParamId::EqActive))
                .label("EQ")
                .on_toggle(|v| Message::SetBoolParam(ParamId::EqActive, v)),
            checkbox(b(ParamId::CalibrateInput))
                .label("Calibrate")
                .on_toggle(|v| Message::SetBoolParam(ParamId::CalibrateInput, v)),
        ]
        .spacing(16),
    );

    let output_mode = state.shared.params.get_enum(ParamId::OutputMode).min(2);
    content = content.push(
        row![
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

    content = content.push(
        row![
            asset_preview(
                &model_path,
                &model_label,
                &model_picture_path,
                "NAM",
                Message::LoadModel,
                Message::ClearModel,
            ),
            asset_preview(
                &ir_path,
                &ir_label,
                &ir_picture_path,
                "IR",
                Message::LoadIr,
                Message::ClearIr,
            ),
        ]
        .spacing(16),
    );

    if state.tone_oauth_authenticated {
        content = content.push(
            row![
                text("Tone3000").size(16),
                radio(
                    "NAM",
                    ToneBrowserMode::Nam,
                    Some(state.tone_browser_mode),
                    Message::ToneBrowserModeSelected,
                ),
                radio(
                    "IR",
                    ToneBrowserMode::Ir,
                    Some(state.tone_browser_mode),
                    Message::ToneBrowserModeSelected,
                ),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        );

        let filters = row![
            taxonomy_checkbox_dropdown(TaxonomyDropdownConfig {
                all_label: "All tags",
                items: &state.tone_tags,
                selected_values: &state.tone_selected_tags,
                menu_open: state.tone_tags_menu_open,
                messages: TaxonomyDropdownMessages {
                    toggle: Message::ToggleToneTagsMenu,
                    clear: Message::ToneTagsClear,
                    toggle_item: Message::ToneTagToggled,
                },
                search: None,
            }),
            taxonomy_checkbox_dropdown(TaxonomyDropdownConfig {
                all_label: "All models",
                items: &state.tone_makes,
                selected_values: &state.tone_selected_makes,
                menu_open: state.tone_models_menu_open,
                messages: TaxonomyDropdownMessages {
                    toggle: Message::ToggleToneModelsMenu,
                    clear: Message::ToneModelsClear,
                    toggle_item: Message::ToneModelFilterToggled,
                },
                search: None,
            }),
            taxonomy_checkbox_dropdown(TaxonomyDropdownConfig {
                all_label: "All creators",
                items: &state.tone_creators,
                selected_values: &state.tone_selected_creators,
                menu_open: state.tone_creators_menu_open,
                messages: TaxonomyDropdownMessages {
                    toggle: Message::ToggleToneCreatorsMenu,
                    clear: Message::ToneCreatorsClear,
                    toggle_item: Message::ToneCreatorToggled,
                },
                search: Some(TaxonomyDropdownSearch {
                    value: &state.tone_creator_query,
                    on_input: Message::ToneCreatorQueryChanged,
                    on_submit: Message::ToneCreatorsSearch,
                }),
            }),
        ]
        .spacing(10)
        .width(Length::Fill);

        let mut browser = column![].spacing(12).width(Length::Fill);
        match state.tone_browser_mode {
            ToneBrowserMode::Nam => {
                browser = browser
                    .push(
                        row![
                            text_input("NAM id/url or search query", &state.tone_model_query)
                                .on_input(Message::ToneModelQueryChanged)
                                .on_submit(Message::ToneSearchModels)
                                .width(Length::Fill),
                            button(text("Search")).on_press(Message::ToneSearchModels),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    )
                    .push(
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
            }
            ToneBrowserMode::Ir => {
                browser = browser.push(
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
            }
        }

        browser = browser.push(filters);

        if state.tone_taxonomy_loading {
            browser = browser.push(text("Loading filters...").size(12));
        }

        if let Some(error) = &state.tone_taxonomy_error {
            browser = browser.push(text(error).size(12));
        }

        if let Some(loading) = &state.loading {
            browser = browser.push(text(format!("Loading: {loading}")).size(14));
        }

        if let Some(error) = &state.error {
            browser = browser.push(text(error));
        }

        if state.tone_browser_mode == ToneBrowserMode::Ir {
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
                let variations_loading = state
                    .tone_ir_variations_loading
                    .get(idx)
                    .copied()
                    .unwrap_or(false);
                let variation_widget: Element<'_, Message> = if options.is_empty() {
                    if variations_loading {
                        text("Loading variations...").into()
                    } else {
                        text("No variations").into()
                    }
                } else {
                    let ir_name = item.name.clone();
                    let picture_path = state
                        .tone_ir_picture_paths
                        .get(idx)
                        .and_then(|path| path.clone());
                    maolan_baseview::iced::widget::pick_list(options, selected, move |choice| {
                        Message::ToneIrVariationSelected(
                            choice.reference,
                            ir_name.clone(),
                            picture_path.clone(),
                        )
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
                browser = browser.push(result_row);
            }

            if state.tone_ir_total_pages > 0 {
                let page_label = format!(
                    "Page {} / {}",
                    state.tone_ir_page, state.tone_ir_total_pages
                );
                browser = browser.push(
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
        }

        if state.tone_browser_mode == ToneBrowserMode::Nam {
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
                let model_name = item.name.clone();
                let picture_path = state
                    .tone_model_picture_paths
                    .get(idx)
                    .and_then(|path| path.clone());
                let variations_loading = state
                    .tone_model_variations_loading
                    .get(idx)
                    .copied()
                    .unwrap_or(false);
                let variation_widget: Element<'_, Message> = if options.is_empty() {
                    if variations_loading {
                        text("Loading variations...").into()
                    } else {
                        text("No variations").into()
                    }
                } else {
                    maolan_baseview::iced::widget::pick_list(options, selected, move |choice| {
                        Message::ToneModelVariationSelected(
                            choice.reference,
                            model_name.clone(),
                            picture_path.clone(),
                        )
                    })
                    .placeholder("Variation")
                    .into()
                };
                let image_widget: Element<'_, Message> = if let Some(handle) =
                    state.tone_model_pictures.get(idx).and_then(|h| h.clone())
                {
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
                browser = browser.push(result_row);
            }

            if state.tone_model_total_pages > 0 {
                let page_label = format!(
                    "Page {} / {}",
                    state.tone_model_page, state.tone_model_total_pages
                );
                browser = browser.push(
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
        }

        content = content.push(browser);
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

fn asset_preview<'a>(
    path: &str,
    label: &str,
    picture_path: &str,
    empty_label: &'static str,
    load_message: Message,
    clear_message: Message,
) -> Element<'a, Message> {
    let content: Element<'a, Message> = if path.is_empty() {
        text(empty_label).size(14).into()
    } else if !picture_path.is_empty() {
        match std::fs::read(picture_path) {
            Ok(bytes) => Image::new(maolan_baseview::iced::widget::image::Handle::from_bytes(
                bytes,
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            Err(_) => text(label.to_string()).size(14).into(),
        }
    } else {
        text(label.to_string()).size(14).into()
    };

    let preview = container(content)
        .width(Length::Fixed(160.0))
        .height(Length::Fixed(72.0))
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center)
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.08, 0.08, 0.10))),
            border: Border {
                color: Color::from_rgb(0.20, 0.20, 0.24),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..container::Style::default()
        });

    let button =
        button(preview)
            .on_press(load_message)
            .padding(0)
            .style(|theme: &Theme, status: Status| {
                let mut style = button::secondary(theme, status);
                style.background = None;
                style.border.width = 0.0;
                style
            });

    mouse_area(button).on_right_press(clear_message).into()
}

fn knob(
    label: &'static str,
    id: ParamId,
    value: f32,
    units: &'static str,
    step: f32,
) -> Element<'static, Message> {
    let def = PARAMS[id.as_index()];
    let value_text = if units.is_empty() {
        format!("{value:.1}")
    } else {
        format!("{value:.1} {units}")
    };

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

struct ToneKnobMode {
    id: ParamId,
    value: f32,
    menu_open: bool,
    toggle_menu: Message,
}

fn tone_knob(
    label: &'static str,
    id: ParamId,
    value: f32,
    step: f32,
    mode: Option<ToneKnobMode>,
) -> Element<'static, Message> {
    let def = PARAMS[id.as_index()];
    let slider = arch_slider(def.min as f32..=def.max as f32, value, move |v| {
        Message::SetParam(id, v)
    })
    .step(step)
    .double_click_reset(def.default as f32)
    .on_release(Message::ReleaseParam(id))
    .fill_from_start()
    .width(Length::Fixed(41.0))
    .height(Length::Fixed(41.0));

    let value_text = format!("{value:.1}");
    let (slider, mode_dropdown) = if let Some(mode) = mode {
        let dropdown = if mode.menu_open {
            let current_mode = ToneFilterMode::from_value(mode.value);
            let mode_id = mode.id;
            Some(
                maolan_baseview::iced::widget::pick_list(
                    vec![ToneFilterMode::Shelf, ToneFilterMode::BandEq],
                    Some(current_mode),
                    move |mode| Message::SetEnumParam(mode_id, mode.as_f32()),
                )
                .placeholder("Mode")
                .width(Length::Fixed(84.0)),
            )
        } else {
            None
        };
        (slider.on_right_click(mode.toggle_menu), dropdown)
    } else {
        (slider, None)
    };

    let mut column = column![text(label).size(11), slider, text(value_text).size(10)]
        .spacing(2)
        .align_x(Alignment::Center);
    if let Some(dropdown) = mode_dropdown {
        column = column.push(dropdown);
    }

    container(column).width(Length::Fixed(50.0)).into()
}

fn selected_taxonomies_label(
    all_label: &'static str,
    items: &[TaxonomyItem],
    selected_values: &[String],
) -> String {
    match selected_values {
        [] => all_label.to_string(),
        [value] => items
            .iter()
            .find(|item| &item.value == value)
            .map(|item| item.name.clone())
            .unwrap_or_else(|| value.clone()),
        values => format!("{} selected", values.len()),
    }
}

struct TaxonomyDropdownMessages {
    toggle: Message,
    clear: Message,
    toggle_item: fn(String, bool) -> Message,
}

struct TaxonomyDropdownSearch<'a> {
    value: &'a str,
    on_input: fn(String) -> Message,
    on_submit: Message,
}

struct TaxonomyDropdownConfig<'a> {
    all_label: &'static str,
    items: &'a [TaxonomyItem],
    selected_values: &'a [String],
    menu_open: bool,
    messages: TaxonomyDropdownMessages,
    search: Option<TaxonomyDropdownSearch<'a>>,
}

fn taxonomy_checkbox_dropdown<'a>(config: TaxonomyDropdownConfig<'a>) -> Element<'a, Message> {
    let mut dropdown = column![
        button(text(selected_taxonomies_label(
            config.all_label,
            config.items,
            config.selected_values,
        )))
        .on_press(config.messages.toggle.clone())
        .width(Length::Fill),
    ]
    .spacing(4);

    if config.menu_open {
        let mut menu = column![].spacing(4);

        if let Some(search) = config.search {
            menu = menu.push(
                row![
                    text_input("Search creators", search.value)
                        .on_input(search.on_input)
                        .on_submit(search.on_submit.clone())
                        .width(Length::Fill),
                    button(text("Search")).on_press(search.on_submit),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            );
        }

        menu = menu.push(
            checkbox(config.selected_values.is_empty())
                .label(config.all_label)
                .on_toggle(move |_| config.messages.clear.clone()),
        );

        for item in config.items {
            let checked = config
                .selected_values
                .iter()
                .any(|value| value == &item.value);
            let value = item.value.clone();
            let label = if item.count > 0 {
                format!("{} ({})", item.name, item.count)
            } else {
                item.name.clone()
            };
            menu =
                menu.push(checkbox(checked).label(label).on_toggle(move |enabled| {
                    (config.messages.toggle_item)(value.clone(), enabled)
                }));
        }

        dropdown = dropdown.push(container(menu).padding(4).width(Length::Fill));
    }

    dropdown.into()
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
        .font(maolan_widgets::iced_fonts::LUCIDE_FONT_BYTES)
        .subscription(|_| window::close_events().map(|_| Message::WindowClosed))
        .theme(theme)
        .run()
}

trait PluginWindowHandle {
    fn close_window(&mut self);
}

impl<Message: 'static + Send> PluginWindowHandle
    for maolan_baseview::iced::shell::window::WindowHandle<Message>
{
    fn close_window(&mut self) {
        maolan_baseview::iced::shell::window::WindowHandle::close_window(self);
    }
}

struct StoredWindowHandle {
    inner: Box<dyn PluginWindowHandle>,
}

unsafe impl Send for StoredWindowHandle {}

pub struct GuiBridge {
    created: bool,
    floating: bool,
    shared: Option<Arc<SharedState>>,
    floating_open: Arc<AtomicBool>,
    window_handle: Option<StoredWindowHandle>,
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
                title: String::from("Maolan Modeler"),
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

        self.window_handle = Some(StoredWindowHandle {
            inner: Box::new(handle),
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
                .name("maolan-modeler-gui".to_string())
                .spawn(move || {
                    let settings = maolan_baseview::iced::IcedBaseviewSettings {
                        window: maolan_baseview::iced::baseview::WindowOpenOptions {
                            title: String::from("Maolan Modeler"),
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
        if let Some(mut handle) = self.window_handle.take() {
            handle.inner.close_window();
        }
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
            std::sync::Arc::new(crate::modeler::plugin::SharedState::default()),
            super::preferred_api(),
            false
        ));
    }

    #[test]
    fn create_fails_for_unsupported_api() {
        let mut bridge = GuiBridge::default();
        assert!(!bridge.create(
            std::sync::Arc::new(crate::modeler::plugin::SharedState::default()),
            c"unsupported",
            false
        ));
    }

    #[test]
    fn create_succeeds_for_floating() {
        let mut bridge = GuiBridge::default();
        assert!(bridge.create(
            std::sync::Arc::new(crate::modeler::plugin::SharedState::default()),
            super::preferred_api(),
            true
        ));
    }

    #[test]
    fn destroy_resets_created() {
        let mut bridge = GuiBridge::default();
        bridge.create(
            std::sync::Arc::new(crate::modeler::plugin::SharedState::default()),
            super::preferred_api(),
            false,
        );
        bridge.destroy();
        assert!(!bridge.set_parent(
            std::sync::Arc::new(crate::modeler::plugin::SharedState::default()),
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
            super::ParentWindowHandle::X11(0),
            #[cfg(target_os = "macos")]
            super::ParentWindowHandle::Cocoa(std::ptr::null_mut()),
            #[cfg(target_os = "windows")]
            super::ParentWindowHandle::Win32(std::ptr::null_mut()),
        ));
    }
}
