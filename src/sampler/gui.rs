use std::{
    ffi::CStr,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

#[cfg(target_os = "windows")]
use clap_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(unix)]
use clap_clap::ffi::CLAP_WINDOW_API_X11;
use maolan_baseview::iced::{
    Alignment, Background, Border, Color, Element, Length, Point, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{button, checkbox, column, container, mouse_area, row, scrollable, text},
};
use maolan_widgets::arch_slider::arch_slider;
use maolan_widgets::slider::Slider;
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};

use crate::{
    common::{
        filter::FilterType,
        lfo::LfoShape,
        lfo_assignment::{LfoAssignmentConfig, LfoAssignmentState, ModRouteParamIds},
    },
    sampler::{
        dsp::mod_matrix::ModTarget,
        params::{PARAMS, ParamId},
        plugin::SharedState,
    },
};

pub const EDITOR_WIDTH: u32 = 1100;
pub const EDITOR_HEIGHT: u32 = 750;
const SAMPLE_MAP_NOTES: usize = 128;

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

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum Message {
    SetParam(ParamId, f32),
    ReleaseParam(ParamId),
    AssignLfoToParam(ParamId),
    ToggleLfoAssignment(usize),
    ToggleParam(ParamId, bool),
    SelectLfo(usize),
    SelectFilter(usize),
    SelectEg(usize),
    StartSideResize(SidePanel),
    ResizeSidePanel(f32),
    StopSideResize,
    OpenBrowserEntry(PathBuf),
    BeginAudioFileDrag(PathBuf),
    ZoneNoteHovered(usize),
    ZoneNoteReleased(usize),
    StartZoneEdgeDrag(usize, ZoneEdge),
    StopZoneEdgeDrag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidePanel {
    Zones,
    Browser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneEdge {
    Start,
    End,
}

struct State {
    shared: Arc<SharedState>,
    active_gestures: Vec<bool>,
    lfo_assignment: LfoAssignmentState,
    selected_lfo: usize,
    selected_filter: usize,
    selected_eg: usize,
    zones_width: f32,
    browser_width: f32,
    resizing_side: Option<SidePanel>,
    resize_last_x: Option<f32>,
    browser_path: PathBuf,
    browser_entries: Vec<BrowserEntry>,
    dragged_audio_file: Option<PathBuf>,
    zones: Vec<SampleZone>,
    dragging_zone_edge: Option<(usize, ZoneEdge)>,
}

#[derive(Debug, Clone)]
struct BrowserEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

#[derive(Debug, Clone)]
struct SampleZone {
    file: PathBuf,
    start_note: usize,
    end_note: usize,
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    let browser_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let browser_entries = read_browser_entries(&browser_path);
    (
        State {
            shared,
            active_gestures: vec![false; ParamId::COUNT],
            lfo_assignment: LfoAssignmentState::default(),
            selected_lfo: 0,
            selected_filter: 0,
            selected_eg: 0,
            zones_width: 138.0,
            browser_width: 138.0,
            resizing_side: None,
            resize_last_x: None,
            browser_path,
            browser_entries,
            dragged_audio_file: None,
            zones: Vec::new(),
            dragging_zone_edge: None,
        },
        Task::none(),
    )
}

fn read_browser_entries(path: &PathBuf) -> Vec<BrowserEntry> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();

    dirs.push(BrowserEntry {
        name: String::from(".."),
        path: path.parent().map(PathBuf::from).unwrap_or_else(|| path.clone()),
        is_dir: true,
    });

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().map(|ty| ty.is_dir()).unwrap_or(false);
            let is_audio = is_audio_file(&entry_path);
            let browser_entry = BrowserEntry {
                name,
                path: entry_path,
                is_dir,
            };
            if is_dir {
                dirs.push(browser_entry);
            } else if is_audio {
                files.push(browser_entry);
            }
        }
    }

    let sort_key = |entry: &BrowserEntry| entry.name.to_ascii_lowercase();
    dirs[1..].sort_by_key(sort_key);
    files.sort_by_key(sort_key);
    dirs.extend(files);
    dirs
}

fn is_audio_file(path: &PathBuf) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "wav" | "wave" | "aif" | "aiff" | "flac" | "ogg" | "mp3" | "m4a" | "aac"
    )
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::SetParam(id, value) => {
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
        Message::AssignLfoToParam(id) => {
            if let Some(lfo_index) = state.lfo_assignment.armed_lfo() {
                assign_lfo_to_param(state, lfo_index, id);
            }
        }
        Message::ToggleLfoAssignment(index) => {
            state.lfo_assignment.toggle(index);
            state.selected_lfo = index;
        }
        Message::ToggleParam(id, checked) => {
            let idx = id.as_index();
            let value = if checked { 1.0f32 } else { 0.0f32 };
            if !state.active_gestures[idx] {
                state.active_gestures[idx] = true;
                state.shared.mark_gesture_begin_pending(id);
            }
            state.shared.set_param_outbound_only(id, value as f64);
            state.active_gestures[idx] = false;
            state.shared.mark_gesture_end_pending(id);
        }
        Message::SelectLfo(index) => {
            state.selected_lfo = index;
        }
        Message::SelectFilter(index) => {
            state.selected_filter = index;
        }
        Message::SelectEg(index) => {
            state.selected_eg = index;
        }
        Message::StartSideResize(side) => {
            state.resizing_side = Some(side);
            state.resize_last_x = None;
        }
        Message::ResizeSidePanel(x) => {
            if let Some(side) = state.resizing_side {
                if let Some(last_x) = state.resize_last_x {
                    let delta = x - last_x;
                    match side {
                        SidePanel::Zones => {
                            state.zones_width = (state.zones_width + delta).clamp(92.0, 260.0);
                        }
                        SidePanel::Browser => {
                            state.browser_width = (state.browser_width - delta).clamp(92.0, 260.0);
                        }
                    }
                }
                state.resize_last_x = Some(x);
            }
        }
        Message::StopSideResize => {
            state.resizing_side = None;
            state.resize_last_x = None;
        }
        Message::OpenBrowserEntry(path) => {
            if path.is_dir() {
                state.browser_path = path;
                state.browser_entries = read_browser_entries(&state.browser_path);
            }
        }
        Message::BeginAudioFileDrag(path) => {
            state.dragged_audio_file = Some(path);
        }
        Message::ZoneNoteHovered(note) => {
            if let Some((zone_index, edge)) = state.dragging_zone_edge {
                if let Some(zone) = state.zones.get_mut(zone_index) {
                    match edge {
                        ZoneEdge::Start => {
                            zone.start_note = note.min(zone.end_note);
                        }
                        ZoneEdge::End => {
                            zone.end_note = note.max(zone.start_note);
                        }
                    }
                }
            }
        }
        Message::ZoneNoteReleased(note) => {
            if let Some(file) = state.dragged_audio_file.take() {
                state.zones.push(SampleZone {
                    file,
                    start_note: note,
                    end_note: note,
                });
            }
            state.dragging_zone_edge = None;
        }
        Message::StartZoneEdgeDrag(index, edge) => {
            state.dragging_zone_edge = Some((index, edge));
        }
        Message::StopZoneEdgeDrag => {
            state.dragging_zone_edge = None;
        }
    }
    Task::none()
}

const MOD_ROUTES: [ModRouteParamIds<ParamId>; 6] = [
    ModRouteParamIds {
        source: ParamId::ModRoute1Source,
        target: ParamId::ModRoute1Target,
        depth: ParamId::ModRoute1Depth,
    },
    ModRouteParamIds {
        source: ParamId::ModRoute2Source,
        target: ParamId::ModRoute2Target,
        depth: ParamId::ModRoute2Depth,
    },
    ModRouteParamIds {
        source: ParamId::ModRoute3Source,
        target: ParamId::ModRoute3Target,
        depth: ParamId::ModRoute3Depth,
    },
    ModRouteParamIds {
        source: ParamId::ModRoute4Source,
        target: ParamId::ModRoute4Target,
        depth: ParamId::ModRoute4Depth,
    },
    ModRouteParamIds {
        source: ParamId::ModRoute5Source,
        target: ParamId::ModRoute5Target,
        depth: ParamId::ModRoute5Depth,
    },
    ModRouteParamIds {
        source: ParamId::ModRoute6Source,
        target: ParamId::ModRoute6Target,
        depth: ParamId::ModRoute6Depth,
    },
];

const LFO_ASSIGNMENT_CONFIG: LfoAssignmentConfig<'static, ParamId> = LfoAssignmentConfig {
    routes: &MOD_ROUTES,
    first_lfo_source: ModSourceValue::Lfo1 as u8,
    lfo_count: 6,
    default_depth: 0.5,
};

#[repr(u8)]
enum ModSourceValue {
    Lfo1 = 7,
}

fn set_param_once(state: &State, id: ParamId, value: f32) {
    state.shared.mark_gesture_begin_pending(id);
    state.shared.set_param_outbound_only(id, value as f64);
    state.shared.mark_gesture_end_pending(id);
}

fn assign_lfo_to_param(state: &mut State, lfo_index: usize, id: ParamId) {
    let Some(target) = mod_target_for_param(id) else {
        return;
    };
    LFO_ASSIGNMENT_CONFIG.assign(
        &state.shared.params,
        lfo_index,
        target as u8,
        |id, value| {
            set_param_once(state, id, value);
        },
    );

    let (amount, enabled) = match lfo_index {
        0 => (ParamId::Lfo1Amount, ParamId::Lfo1Enabled),
        1 => (ParamId::Lfo2Amount, ParamId::Lfo2Enabled),
        2 => (ParamId::Lfo3Amount, ParamId::Lfo3Enabled),
        3 => (ParamId::Lfo4Amount, ParamId::Lfo4Enabled),
        4 => (ParamId::Lfo5Amount, ParamId::Lfo5Enabled),
        _ => (ParamId::Lfo6Amount, ParamId::Lfo6Enabled),
    };
    if state.shared.params.get(amount).abs() <= 0.001 {
        set_param_once(state, amount, 1.0);
    }
    if state.shared.params.get(enabled) < 0.5 {
        set_param_once(state, enabled, 1.0);
    }
}

fn param_has_lfo_assignment(state: &State, id: ParamId) -> bool {
    let Some(target) = mod_target_for_param(id) else {
        return false;
    };
    let Some(lfo_index) = state.lfo_assignment.armed_lfo() else {
        return false;
    };
    LFO_ASSIGNMENT_CONFIG.has_lfo_assignment(&state.shared.params, lfo_index, target as u8)
}

fn mod_target_for_param(id: ParamId) -> Option<ModTarget> {
    match id {
        ParamId::MasterGain => Some(ModTarget::Amplitude),
        ParamId::MasterPan => Some(ModTarget::Pan),
        ParamId::FilterCutoff | ParamId::Filter2Cutoff => Some(ModTarget::FilterCutoff),
        ParamId::FilterResonance | ParamId::Filter2Resonance => Some(ModTarget::FilterResonance),
        _ => None,
    }
}

fn assignment_color() -> Color {
    Color::from_rgb(1.0, 0.83, 0.10)
}

fn small_knob<'a>(
    id: ParamId,
    label: &'a str,
    state: &'a State,
    step: f32,
) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let def = &PARAMS[id.as_index()];
    let assigned = param_has_lfo_assignment(state, id);
    let mut slider = arch_slider(def.min as f32..=def.max as f32, value, move |v| {
        Message::SetParam(id, v)
    })
    .step(step)
    .double_click_reset(def.default as f32)
    .on_release(Message::ReleaseParam(id))
    .fill_from_start()
    .width(Length::Fixed(48.0))
    .height(Length::Fixed(48.0));
    if assigned {
        slider = slider
            .filled_color(Color::from_rgb(0.82, 0.58, 0.08))
            .handle_color(assignment_color());
    }

    let value_text = if def.step >= 1.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    };

    let content = container(
        column![text(label).size(11), slider, text(value_text).size(10)]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(56.0))
    .padding(2)
    .style(move |_theme: &Theme| container::Style {
        background: assigned.then(|| Background::Color(Color::from_rgba(1.0, 0.83, 0.10, 0.10))),
        border: Border {
            color: if assigned {
                assignment_color()
            } else {
                Color::TRANSPARENT
            },
            width: if assigned { 1.0 } else { 0.0 },
            radius: 3.0.into(),
        },
        ..container::Style::default()
    });

    if mod_target_for_param(id).is_some() {
        mouse_area(content)
            .on_press(Message::AssignLfoToParam(id))
            .into()
    } else {
        content.into()
    }
}

fn small_checkbox<'a>(id: ParamId, label: &'a str, state: &'a State) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let checkbox_widget = checkbox(value > 0.5)
        .label(label)
        .on_toggle(move |checked| Message::ToggleParam(id, checked));
    container(
        column![checkbox_widget]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(56.0))
    .into()
}

fn param_control<'a>(id: ParamId, label: &'a str, state: &'a State) -> Element<'a, Message> {
    let def = &PARAMS[id.as_index()];
    if def.min == 0.0 && def.max == 1.0 && def.step >= 1.0 {
        small_checkbox(id, label, state)
    } else {
        small_knob(id, label, state, def.step as f32)
    }
}

fn vslider<'a>(id: ParamId, label: &'a str, state: &'a State) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let def = &PARAMS[id.as_index()];
    let slider = Slider::new(def.min as f32..=def.max as f32, value, move |v| {
        Message::SetParam(id, v)
    })
    .step(def.step as f32)
    .double_click_reset(def.default as f32)
    .on_release(Message::ReleaseParam(id))
    .width(Length::Fixed(20.0))
    .height(Length::Fixed(80.0));

    container(
        column![
            text(label).size(11),
            slider,
            text(format!("{value:.2}")).size(10)
        ]
        .spacing(2)
        .align_x(Alignment::Center),
    )
    .width(Length::Fixed(32.0))
    .padding(2)
    .into()
}

fn filter_type_dropdown<'a>(id: ParamId, state: &'a State) -> Element<'a, Message> {
    let filter_type = FilterType::from_u8(state.shared.params.get(id) as u8);
    let options: Vec<FilterType> = (1..=48).map(FilterType::from_u8).collect();
    let dropdown = maolan_baseview::iced::widget::pick_list(options, Some(filter_type), move |t| {
        Message::SetParam(id, t as u8 as f32)
    })
    .placeholder("Type")
    .width(Length::Fixed(84.0));

    container(
        column![text("Type").size(11), dropdown]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(90.0))
    .into()
}

fn lfo_shape_dropdown<'a>(id: ParamId, state: &'a State) -> Element<'a, Message> {
    let shape = LfoShape::from_u8(state.shared.params.get(id) as u8);
    let options = vec![
        LfoShape::Sine,
        LfoShape::Triangle,
        LfoShape::Saw,
        LfoShape::Ramp,
        LfoShape::Square,
        LfoShape::SampleHold,
        LfoShape::Noise,
        LfoShape::Envelope,
        LfoShape::StepSeq,
        LfoShape::Mseg,
    ];
    let dropdown = maolan_baseview::iced::widget::pick_list(options, Some(shape), move |shape| {
        Message::SetParam(id, shape as u8 as f32)
    })
    .placeholder("Shape")
    .width(Length::Fixed(84.0));

    container(
        column![text("Shape").size(11), dropdown]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(90.0))
    .into()
}

fn section_title(title: &'static str) -> Element<'static, Message> {
    container(text(title).size(13))
        .padding([3, 6])
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.15, 0.15, 0.18))),
            border: Border {
                color: Color::from_rgb(0.28, 0.28, 0.32),
                width: 1.0,
                radius: 3.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn panel<'a>(title: &'static str, content: Element<'a, Message>) -> Element<'a, Message> {
    container(
        column![section_title(title), content]
            .spacing(6)
            .align_x(Alignment::Start),
    )
    .padding(8)
    .style(|_theme: &Theme| container::Style {
        background: Some(Background::Color(Color::from_rgb(0.08, 0.08, 0.10))),
        border: Border {
            color: Color::from_rgb(0.20, 0.20, 0.24),
            width: 1.0,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    })
    .into()
}

fn panel_no_title<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    container(content)
        .padding(8)
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.08, 0.08, 0.10))),
            border: Border {
                color: Color::from_rgb(0.20, 0.20, 0.24),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn fill_panel_no_title<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(8)
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.08, 0.08, 0.10))),
            border: Border {
                color: Color::from_rgb(0.20, 0.20, 0.24),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn zone_for_note(state: &State, note: usize) -> Option<(usize, &SampleZone)> {
    state
        .zones
        .iter()
        .enumerate()
        .find(|(_, zone)| zone.start_note <= note && note <= zone.end_note)
}

fn zone_edge_handle(zone_index: usize, edge: ZoneEdge) -> Element<'static, Message> {
    mouse_area(
        container(text(""))
            .width(Length::Fixed(3.0))
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.78, 0.88, 1.0))),
                border: Border {
                    color: Color::from_rgb(0.22, 0.48, 0.95),
                    width: 1.0,
                    radius: 1.0.into(),
                },
                ..container::Style::default()
            }),
    )
    .on_press(Message::StartZoneEdgeDrag(zone_index, edge))
    .on_release(Message::StopZoneEdgeDrag)
    .into()
}

fn zone_note_cell<'a>(state: &'a State, note: usize) -> Element<'a, Message> {
    let pitch = note % 12;
    let is_c = pitch == 0;
    let label = if is_c {
        format!("C{}", note as i32 / 12 - 1)
    } else {
        String::new()
    };
    let zone = zone_for_note(state, note);
    let in_zone = zone.is_some();
    let zone_index = zone.map(|(index, _)| index);
    let start_edge = zone
        .map(|(_, zone)| zone.start_note == note)
        .unwrap_or(false);
    let end_edge = zone.map(|(_, zone)| zone.end_note == note).unwrap_or(false);
    let zone_label = zone
        .and_then(|(_, zone)| zone.file.file_name())
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or(label);

    let mut content = row![].spacing(0).height(Length::Fill);
    if let Some(index) = zone_index {
        if start_edge {
            content = content.push(zone_edge_handle(index, ZoneEdge::Start));
        }
    }
    content = content.push(
        container(text(zone_label).size(9))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Top)
            .padding(2),
    );
    if let Some(index) = zone_index {
        if end_edge {
            content = content.push(zone_edge_handle(index, ZoneEdge::End));
        }
    }

    mouse_area(
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(if in_zone {
                    Color::from_rgb(0.16, 0.23, 0.34)
                } else if is_c {
                    Color::from_rgb(0.105, 0.108, 0.128)
                } else {
                    Color::from_rgb(0.085, 0.088, 0.108)
                })),
                border: Border {
                    color: if in_zone {
                        Color::from_rgb(0.22, 0.48, 0.95)
                    } else {
                        Color::from_rgb(0.18, 0.18, 0.22)
                    },
                    width: 1.0,
                    radius: 0.0.into(),
                },
                text_color: Some(Color::from_rgb(0.48, 0.50, 0.58)),
                ..container::Style::default()
            }),
    )
    .on_move(move |_| Message::ZoneNoteHovered(note))
    .on_release(Message::ZoneNoteReleased(note))
    .into()
}

fn zone_grid<'a>(state: &'a State) -> Element<'a, Message> {
    let mut lanes = row![].spacing(1).height(Length::Fill);
    for note in 0..SAMPLE_MAP_NOTES {
        lanes = lanes.push(zone_note_cell(state, note));
    }

    container(lanes)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.075, 0.078, 0.095))),
            border: Border {
                color: Color::from_rgb(0.18, 0.18, 0.22),
                width: 1.0,
                radius: 3.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn piano_key(note: usize) -> Element<'static, Message> {
    let pitch = note % 12;
    let is_black = matches!(pitch, 1 | 3 | 6 | 8 | 10);
    let label = if pitch == 0 {
        format!("C{}", note / 12 + 1)
    } else {
        String::new()
    };

    container(text(label).size(9))
        .width(Length::Fill)
        .height(Length::Fixed(48.0))
        .align_x(Horizontal::Center)
        .align_y(Vertical::Bottom)
        .padding(3)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(if is_black {
                Color::from_rgb(0.035, 0.037, 0.045)
            } else {
                Color::from_rgb(0.78, 0.80, 0.84)
            })),
            border: Border {
                color: if is_black {
                    Color::from_rgb(0.015, 0.016, 0.020)
                } else {
                    Color::from_rgb(0.42, 0.43, 0.48)
                },
                width: 1.0,
                radius: 2.0.into(),
            },
            text_color: Some(if is_black {
                Color::from_rgb(0.72, 0.72, 0.76)
            } else {
                Color::from_rgb(0.16, 0.17, 0.20)
            }),
            ..container::Style::default()
        })
        .into()
}

fn piano_roll<'a>() -> Element<'a, Message> {
    let mut keys = row![].spacing(1).align_y(Alignment::End);
    for note in 0..SAMPLE_MAP_NOTES {
        keys = keys.push(piano_key(note));
    }

    container(keys)
        .width(Length::Fill)
        .height(Length::Fixed(48.0))
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.045, 0.047, 0.058))),
            border: Border {
                color: Color::from_rgb(0.18, 0.18, 0.22),
                width: 1.0,
                radius: 3.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn resize_handle(side: SidePanel) -> Element<'static, Message> {
    mouse_area(
        container(text("")).width(Length::Fixed(6.0)).height(Length::Fill).style(
            |_theme: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.18, 0.18, 0.22))),
                border: Border {
                    color: Color::from_rgb(0.27, 0.27, 0.32),
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..container::Style::default()
            },
        ),
    )
    .on_press(Message::StartSideResize(side))
    .on_release(Message::StopSideResize)
    .into()
}

fn side_panel<'a>(title: &'static str, width: f32) -> Element<'a, Message> {
    container(
        column![section_title(title)]
            .spacing(8)
            .align_x(Alignment::Start),
    )
    .width(Length::Fixed(width))
    .height(Length::Fill)
    .padding(8)
    .style(|_theme: &Theme| container::Style {
        background: Some(Background::Color(Color::from_rgb(0.08, 0.08, 0.10))),
        border: Border {
            color: Color::from_rgb(0.20, 0.20, 0.24),
            width: 1.0,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    })
    .into()
}

fn browser_row<'a>(entry: &'a BrowserEntry) -> Element<'a, Message> {
    let label = if entry.is_dir {
        format!("{}/", entry.name)
    } else {
        entry.name.clone()
    };
    let content = container(text(label).size(11))
        .width(Length::Fill)
        .padding([3, 6])
        .style(move |_theme: &Theme| container::Style {
            background: entry
                .is_dir
                .then(|| Background::Color(Color::from_rgb(0.105, 0.108, 0.128))),
            border: Border {
                color: if entry.is_dir {
                    Color::from_rgb(0.22, 0.48, 0.95)
                } else {
                    Color::from_rgb(0.18, 0.18, 0.22)
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            text_color: Some(if entry.is_dir {
                Color::from_rgb(0.82, 0.84, 0.92)
            } else {
                Color::from_rgb(0.62, 0.64, 0.70)
            }),
            ..container::Style::default()
        });

    if entry.is_dir {
        button(content)
            .padding(1)
            .on_press(Message::OpenBrowserEntry(entry.path.clone()))
            .width(Length::Fill)
            .into()
    } else {
        mouse_area(content)
            .on_press(Message::BeginAudioFileDrag(entry.path.clone()))
            .into()
    }
}

fn browser_panel<'a>(state: &'a State) -> Element<'a, Message> {
    let mut entries = column![].spacing(3).width(Length::Fill);
    for entry in &state.browser_entries {
        entries = entries.push(browser_row(entry));
    }

    container(
        column![
            section_title("Browser"),
            container(text(state.browser_path.display().to_string()).size(10))
                .width(Length::Fill)
                .padding([3, 6]),
            scrollable(entries).height(Length::Fill),
        ]
        .spacing(8)
        .height(Length::Fill)
        .align_x(Alignment::Start),
    )
    .width(Length::Fixed(state.browser_width))
    .height(Length::Fill)
    .padding(8)
    .style(|_theme: &Theme| container::Style {
        background: Some(Background::Color(Color::from_rgb(0.08, 0.08, 0.10))),
        border: Border {
            color: Color::from_rgb(0.20, 0.20, 0.24),
            width: 1.0,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    })
    .into()
}

fn knob_row<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut r = row![].spacing(6).align_y(Alignment::Center);
    for item in items {
        r = r.push(item);
    }
    r.into()
}

fn knob_column<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut c = column![].spacing(4).align_x(Alignment::Center);
    for item in items {
        c = c.push(item);
    }
    c.into()
}

fn tab_button(label: &'static str, active: bool, msg: Message) -> Element<'static, Message> {
    button(
        container(text(label).size(11))
            .width(Length::Fixed(48.0))
            .align_x(Horizontal::Center),
    )
    .on_press(msg)
    .style(move |theme: &Theme, status| {
        let mut base = if active {
            button::primary(theme, status)
        } else {
            button::secondary(theme, status)
        };
        base.border.radius = 4.0.into();
        base
    })
    .into()
}

fn lfo_tab_button(label: &'static str, index: usize, state: &State) -> Element<'static, Message> {
    let active = state.selected_lfo == index;
    let armed = state.lfo_assignment.armed_lfo() == Some(index);
    let button = button(
        container(text(label).size(11))
            .width(Length::Fixed(40.0))
            .align_x(Horizontal::Center),
    )
    .on_press(Message::SelectLfo(index))
    .style(move |theme: &Theme, status| {
        let mut base = if armed || active {
            button::primary(theme, status)
        } else {
            button::secondary(theme, status)
        };
        if armed {
            base.background = Some(Background::Color(Color::from_rgb(0.72, 0.49, 0.06)));
            base.text_color = Color::from_rgb(1.0, 0.96, 0.78);
            base.border.color = assignment_color();
            base.border.width = 1.0;
        }
        base.border.radius = 4.0.into();
        base
    });

    mouse_area(button)
        .on_right_press(Message::ToggleLfoAssignment(index))
        .into()
}

fn view(state: &State) -> Element<'_, Message> {
    let sample_map = fill_panel_no_title(
        column![zone_grid(state), piano_roll()]
            .spacing(8)
            .height(Length::Fill)
            .into(),
    );

    let top_bar = row![
        panel(
            "Master",
            knob_row(vec![
                param_control(ParamId::MasterGain, "Gain", state),
                param_control(ParamId::MasterPan, "Pan", state),
            ])
        ),
        panel(
            "Pitch",
            knob_row(vec![
                param_control(ParamId::PitchBendUp, "Bend Up", state),
                param_control(ParamId::PitchBendDown, "Bend Down", state),
            ])
        ),
    ]
    .spacing(10)
    .align_y(Alignment::Start);

    let filter1 = panel_no_title(
        column![
            knob_row(vec![
                filter_type_dropdown(ParamId::FilterType, state),
                param_control(ParamId::FilterSubtype, "Sub", state),
                param_control(ParamId::FilterCutoff, "Cut", state),
                param_control(ParamId::FilterResonance, "Res", state),
                param_control(ParamId::FilterEgAmount, "EG", state),
                param_control(ParamId::FilterKeyTrack, "Key", state),
                param_control(ParamId::FilterDrive, "Drive", state),
            ]),
            knob_row(vec![param_control(ParamId::FilterEnabled, "On", state),]),
        ]
        .spacing(6)
        .into(),
    );

    let filter2 = panel_no_title(
        column![
            knob_row(vec![
                filter_type_dropdown(ParamId::Filter2Type, state),
                param_control(ParamId::Filter2Subtype, "Sub", state),
                param_control(ParamId::Filter2Cutoff, "Cut", state),
                param_control(ParamId::Filter2Resonance, "Res", state),
                param_control(ParamId::Filter2EgAmount, "EG", state),
                param_control(ParamId::Filter2KeyTrack, "Key", state),
                param_control(ParamId::Filter2Drive, "Drive", state),
            ]),
            knob_row(vec![param_control(ParamId::Filter2Enabled, "On", state),]),
        ]
        .spacing(6)
        .into(),
    );

    let filter_selector = row![
        tab_button(
            "Filter 1",
            state.selected_filter == 0,
            Message::SelectFilter(0)
        ),
        tab_button(
            "Filter 2",
            state.selected_filter == 1,
            Message::SelectFilter(1)
        ),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let selected_filter_panel = match state.selected_filter {
        0 => filter1,
        _ => filter2,
    };

    let amp_eg = panel_no_title(knob_row(vec![
        vslider(ParamId::AmpAttack, "A", state),
        vslider(ParamId::AmpDecay, "D", state),
        vslider(ParamId::AmpSustain, "S", state),
        vslider(ParamId::AmpRelease, "R", state),
    ]));

    let filter_eg = panel_no_title(knob_row(vec![
        vslider(ParamId::FilterAttack, "A", state),
        vslider(ParamId::FilterDecay, "D", state),
        vslider(ParamId::FilterSustain, "S", state),
        vslider(ParamId::FilterRelease, "R", state),
    ]));

    let pitch_eg = panel_no_title(knob_row(vec![
        vslider(ParamId::Eg2Attack, "A", state),
        vslider(ParamId::Eg2Decay, "D", state),
        vslider(ParamId::Eg2Sustain, "S", state),
        vslider(ParamId::Eg2Release, "R", state),
    ]));

    let eg_selector = row![
        tab_button("Amp", state.selected_eg == 0, Message::SelectEg(0)),
        tab_button("Filter", state.selected_eg == 1, Message::SelectEg(1)),
        tab_button("Pitch", state.selected_eg == 2, Message::SelectEg(2)),
        tab_button("EG 3", state.selected_eg == 3, Message::SelectEg(3)),
        tab_button("EG 4", state.selected_eg == 4, Message::SelectEg(4)),
        tab_button("EG 5", state.selected_eg == 5, Message::SelectEg(5)),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let eg3 = panel_no_title(knob_row(vec![
        vslider(ParamId::Eg3Attack, "A", state),
        vslider(ParamId::Eg3Decay, "D", state),
        vslider(ParamId::Eg3Sustain, "S", state),
        vslider(ParamId::Eg3Release, "R", state),
    ]));

    let eg4 = panel_no_title(knob_row(vec![
        vslider(ParamId::Eg4Attack, "A", state),
        vslider(ParamId::Eg4Decay, "D", state),
        vslider(ParamId::Eg4Sustain, "S", state),
        vslider(ParamId::Eg4Release, "R", state),
    ]));

    let eg5 = panel_no_title(knob_row(vec![
        vslider(ParamId::Eg5Attack, "A", state),
        vslider(ParamId::Eg5Decay, "D", state),
        vslider(ParamId::Eg5Sustain, "S", state),
        vslider(ParamId::Eg5Release, "R", state),
    ]));

    let selected_eg_panel = match state.selected_eg {
        0 => amp_eg,
        1 => filter_eg,
        2 => pitch_eg,
        3 => eg3,
        4 => eg4,
        _ => eg5,
    };

    let lfo_ids = match state.selected_lfo {
        0 => (
            ParamId::Lfo1Shape,
            ParamId::Lfo1Rate,
            ParamId::Lfo1Amount,
            ParamId::Lfo1Deform,
            ParamId::Lfo1Phase,
            ParamId::Lfo1Trigger,
            ParamId::Lfo1Unipolar,
            ParamId::Lfo1SyncMode,
        ),
        1 => (
            ParamId::Lfo2Shape,
            ParamId::Lfo2Rate,
            ParamId::Lfo2Amount,
            ParamId::Lfo2Deform,
            ParamId::Lfo2Phase,
            ParamId::Lfo2Trigger,
            ParamId::Lfo2Unipolar,
            ParamId::Lfo2SyncMode,
        ),
        2 => (
            ParamId::Lfo3Shape,
            ParamId::Lfo3Rate,
            ParamId::Lfo3Amount,
            ParamId::Lfo3Deform,
            ParamId::Lfo3Phase,
            ParamId::Lfo3Trigger,
            ParamId::Lfo3Unipolar,
            ParamId::Lfo3SyncMode,
        ),
        3 => (
            ParamId::Lfo4Shape,
            ParamId::Lfo4Rate,
            ParamId::Lfo4Amount,
            ParamId::Lfo4Deform,
            ParamId::Lfo4Phase,
            ParamId::Lfo4Trigger,
            ParamId::Lfo4Unipolar,
            ParamId::Lfo4SyncMode,
        ),
        4 => (
            ParamId::Lfo5Shape,
            ParamId::Lfo5Rate,
            ParamId::Lfo5Amount,
            ParamId::Lfo5Deform,
            ParamId::Lfo5Phase,
            ParamId::Lfo5Trigger,
            ParamId::Lfo5Unipolar,
            ParamId::Lfo5SyncMode,
        ),
        _ => (
            ParamId::Lfo6Shape,
            ParamId::Lfo6Rate,
            ParamId::Lfo6Amount,
            ParamId::Lfo6Deform,
            ParamId::Lfo6Phase,
            ParamId::Lfo6Trigger,
            ParamId::Lfo6Unipolar,
            ParamId::Lfo6SyncMode,
        ),
    };

    let lfo_selector = row![
        lfo_tab_button("LFO 1", 0, state),
        lfo_tab_button("LFO 2", 1, state),
        lfo_tab_button("LFO 3", 2, state),
        lfo_tab_button("LFO 4", 3, state),
        lfo_tab_button("LFO 5", 4, state),
        lfo_tab_button("LFO 6", 5, state),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let lfo_panel = panel_no_title(knob_row(vec![
        lfo_shape_dropdown(lfo_ids.0, state),
        param_control(lfo_ids.1, "Rate", state),
        param_control(lfo_ids.2, "Amt", state),
        param_control(lfo_ids.3, "Deform", state),
        param_control(lfo_ids.4, "Phase", state),
        param_control(lfo_ids.5, "Trig", state),
        knob_column(vec![
            param_control(lfo_ids.6, "Uni", state),
            param_control(lfo_ids.7, "Sync", state),
        ]),
    ]));

    let main_content = column![
        sample_map,
        top_bar,
        row![
            column![filter_selector, selected_filter_panel].spacing(10),
            column![eg_selector, selected_eg_panel].spacing(10),
        ]
        .spacing(10)
        .align_y(Alignment::Start),
        column![lfo_selector, lfo_panel].spacing(10),
    ]
    .spacing(12)
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Alignment::Start);

    let content_row = row![
        side_panel("Zones", state.zones_width),
        resize_handle(SidePanel::Zones),
        main_content,
        resize_handle(SidePanel::Browser),
        browser_panel(state)
    ]
    .spacing(8)
    .height(Length::Fill)
    .align_y(Alignment::Start);

    let content = mouse_area(container(content_row).padding(16).height(Length::Fill))
        .on_move(|Point { x, .. }| Message::ResizeSidePanel(x))
        .on_release(Message::StopSideResize);

    container(content)
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
        .font(iced_fonts::LUCIDE_FONT_BYTES)
        .theme(theme)
        .run()
}

struct AnyWindowHandle {
    _inner: Box<dyn std::any::Any>,
}

unsafe impl Send for AnyWindowHandle {}

#[derive(Default)]
pub struct GuiBridge {
    created: bool,
    floating: bool,
    shared: Option<Arc<SharedState>>,
    floating_open: Arc<AtomicBool>,
    window_handle: Option<AnyWindowHandle>,
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
        if self.window_handle.is_some() {
            return true;
        }

        let settings = maolan_baseview::iced::IcedBaseviewSettings {
            window: maolan_baseview::iced::baseview::WindowOpenOptions {
                title: String::from("Maolan Sampler"),
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
        if !self.floating {
            return self.window_handle.is_some();
        }
        if self.window_handle.is_some() {
            return true;
        }
        let shared = self.shared.clone().unwrap();
        let open_flag = self.floating_open.clone();
        open_flag.store(true, Ordering::Release);
        thread::spawn(move || {
            let settings = maolan_baseview::iced::IcedBaseviewSettings {
                window: maolan_baseview::iced::baseview::WindowOpenOptions {
                    title: String::from("Maolan Sampler"),
                    size: maolan_baseview::iced::baseview::Size::new(
                        EDITOR_WIDTH as f64,
                        EDITOR_HEIGHT as f64,
                    ),
                    scale: maolan_baseview::iced::baseview::WindowScalePolicy::SystemScaleFactor,
                },
                ignore_non_modifier_keys: false,
                always_redraw: false,
            };
            maolan_baseview::iced::shell::open_blocking(
                settings,
                maolan_baseview::iced::PollSubNotifier::new(),
                move || build_app(shared),
            );
            open_flag.store(false, Ordering::Release);
        });
        true
    }

    pub fn hide(&mut self, shared: Arc<SharedState>) -> bool {
        if self.floating {
            self.floating_open.store(false, Ordering::Release);
            shared.request_gui_closed();
            return true;
        }
        self.window_handle = None;
        true
    }
}
