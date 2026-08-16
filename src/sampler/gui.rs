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
    Alignment, Background, Border, Color, Element, Length, Point, Rectangle, Size, Task, Theme,
    alignment::{Horizontal, Vertical},
    keyboard,
    widget::{button, canvas, checkbox, column, container, mouse_area, pick_list, row, scrollable, text, text_input},
};
use symphonia::core::{
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
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
        state::SampleZone,
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
    ZoneNoteHovered(usize, u8, f32),
    ZoneNoteReleased(usize, u8),
    StartZoneEdgeDrag(usize, ZoneEdge),
    StopZoneEdgeDrag,
    StartZoneBodyDrag(usize, f32, f32),
    StopZoneBodyDrag,
    StartRenameZone(usize),
    UpdateRenameText(String),
    FinishRenameZone,
    DeselectZone,
    DeleteSelectedZone,
    SelectZoneListItem(ZoneListSelection),
    Undo,
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
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneListSelection {
    Zone(usize),
    Group(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EgSelectorOption {
    Amp,
    Filter,
    Pitch,
    Eg1,
    Eg2,
    Eg3,
}

impl EgSelectorOption {
    fn index(self) -> usize {
        match self {
            EgSelectorOption::Amp => 0,
            EgSelectorOption::Filter => 1,
            EgSelectorOption::Pitch => 2,
            EgSelectorOption::Eg1 => 3,
            EgSelectorOption::Eg2 => 4,
            EgSelectorOption::Eg3 => 5,
        }
    }

    fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(EgSelectorOption::Amp),
            1 => Some(EgSelectorOption::Filter),
            2 => Some(EgSelectorOption::Pitch),
            3 => Some(EgSelectorOption::Eg1),
            4 => Some(EgSelectorOption::Eg2),
            5 => Some(EgSelectorOption::Eg3),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            EgSelectorOption::Amp => "Amp",
            EgSelectorOption::Filter => "Filter",
            EgSelectorOption::Pitch => "Pitch",
            EgSelectorOption::Eg1 => "EG 1",
            EgSelectorOption::Eg2 => "EG 2",
            EgSelectorOption::Eg3 => "EG 3",
        }
    }

    fn all() -> [Self; 6] {
        [
            EgSelectorOption::Amp,
            EgSelectorOption::Filter,
            EgSelectorOption::Pitch,
            EgSelectorOption::Eg1,
            EgSelectorOption::Eg2,
            EgSelectorOption::Eg3,
        ]
    }
}

impl std::fmt::Display for EgSelectorOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
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
    hovered_note: Option<usize>,
    hovered_velocity: Option<u8>,
    drag_y: Option<f32>,
    dragging_zone_edge: Option<(usize, ZoneEdge)>,
    dragging_zone_body: Option<(usize, f32, f32)>,
    editing_zone_name: Option<(usize, String)>,
    selected: Option<ZoneListSelection>,
    undo_stack: Vec<Vec<SampleZone>>,
}

#[derive(Debug, Clone)]
struct BrowserEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

fn unique_zone_name(zones: &[SampleZone], base: &str) -> String {
    let existing: std::collections::HashSet<&str> =
        zones.iter().map(|zone| zone.name.as_str()).collect();
    if !existing.contains(base) {
        return base.to_string();
    }

    let mut candidate = base.to_string();
    loop {
        let next = if let Some((prefix, number)) = candidate.rsplit_once(' ') {
            if let Ok(n) = number.parse::<u32>() {
                format!("{} {}", prefix, n + 1)
            } else {
                format!("{} 2", candidate)
            }
        } else {
            format!("{} 2", candidate)
        };
        if !existing.contains(next.as_str()) {
            return next;
        }
        candidate = next;
    }
}

fn find_vertical_slot(
    zones: &[SampleZone],
    start_note: usize,
    end_note: usize,
    velocity: u8,
) -> (u8, u8) {
    let mut overlapping: Vec<&SampleZone> = zones
        .iter()
        .filter(|zone| zone.start_note <= end_note && zone.end_note >= start_note)
        .collect();

    if overlapping.is_empty() {
        return (0, 127);
    }

    overlapping.sort_by_key(|zone| zone.vel_low);

    let mut gaps = Vec::new();
    let mut low = 0u8;
    for zone in &overlapping {
        if zone.vel_low > low {
            gaps.push((low, zone.vel_low.saturating_sub(1)));
        }
        low = zone.vel_high.saturating_add(1);
    }
    if low <= 127 {
        gaps.push((low, 127));
    }

    if gaps.is_empty() {
        return (velocity, velocity);
    }

    let mut best = gaps[0];
    let mut best_dist = u16::MAX;
    for (gap_low, gap_high) in gaps {
        if velocity >= gap_low && velocity <= gap_high {
            return (gap_low, gap_high);
        }
        let dist = if velocity < gap_low {
            (gap_low - velocity) as u16
        } else {
            (velocity - gap_high) as u16
        };
        if dist < best_dist {
            best_dist = dist;
            best = (gap_low, gap_high);
        }
    }
    best
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
            hovered_note: None,
            hovered_velocity: None,
            drag_y: None,
            dragging_zone_edge: None,
            dragging_zone_body: None,
            editing_zone_name: None,
            selected: None,
            undo_stack: Vec::new(),
        },
        Task::none(),
    )
}

fn read_browser_entries(path: &PathBuf) -> Vec<BrowserEntry> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();

    dirs.push(BrowserEntry {
        name: String::from(".."),
        path: path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| path.clone()),
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
    if !matches!(
        extension.to_ascii_lowercase().as_str(),
        "wav" | "wave" | "aif" | "aiff" | "flac" | "ogg" | "mp3" | "m4a" | "aac"
    ) {
        return false;
    }
    is_mono_or_stereo(path)
}

fn is_mono_or_stereo(path: &PathBuf) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();
    let Ok(probed) =
        symphonia::default::get_probe().format(&hint, mss, &format_opts, &metadata_opts)
    else {
        return false;
    };
    let format = probed.format;
    format
        .tracks()
        .iter()
        .filter(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .next()
        .or_else(|| format.tracks().first())
        .and_then(|track| track.codec_params.channels)
        .map(|channels| {
            let count = channels.count();
            count == 1 || count == 2
        })
        .unwrap_or(false)
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
            state.hovered_note = None;
            state.hovered_velocity = None;
            state.drag_y = None;
        }
        Message::ZoneNoteHovered(note, velocity, y) => {
            state.hovered_note = Some(note);
            state.hovered_velocity = Some(velocity);
            state.drag_y = Some(y);
            let mut zones = state.shared.zones.load();
            let mut changed = false;
            if let Some((zone_index, edge)) = state.dragging_zone_edge {
                if let Some(zone) = Arc::make_mut(&mut zones).get_mut(zone_index) {
                    match edge {
                        ZoneEdge::Start => {
                            zone.start_note = note.min(zone.end_note);
                        }
                        ZoneEdge::End => {
                            zone.end_note = note.max(zone.start_note);
                        }
                        ZoneEdge::Top => {
                            zone.vel_high = velocity.max(zone.vel_low);
                        }
                        ZoneEdge::Bottom => {
                            zone.vel_low = velocity.min(zone.vel_high);
                        }
                    }
                    changed = true;
                }
            }
            if let Some((zone_index, note_offset, velocity_offset)) = state.dragging_zone_body {
                if let Some(zone) = Arc::make_mut(&mut zones).get_mut(zone_index) {
                    let width = zone.end_note - zone.start_note;
                    let height = zone.vel_high - zone.vel_low;
                    let mut new_start = (note as f32 - note_offset).round() as usize;
                    let mut new_end = new_start + width;
                    if new_end > SAMPLE_MAP_NOTES - 1 {
                        new_end = SAMPLE_MAP_NOTES - 1;
                        new_start = new_end.saturating_sub(width);
                    }
                    let mut new_vel_low = (velocity as f32 - velocity_offset).round() as u8;
                    let mut new_vel_high = new_vel_low.saturating_add(height);
                    if new_vel_high > 127 {
                        new_vel_high = 127;
                        new_vel_low = new_vel_high.saturating_sub(height);
                    }
                    zone.start_note = new_start;
                    zone.end_note = new_end;
                    zone.vel_low = new_vel_low;
                    zone.vel_high = new_vel_high;
                    changed = true;
                }
            }
            if changed {
                state.shared.zones.store(zones);
                state.shared.bump_zones_version();
                state.shared.mark_dirty();
            }
        }
        Message::ZoneNoteReleased(note, velocity) => {
            if let Some(file) = state.dragged_audio_file.take() {
                let mut zones = state.shared.zones.load();
                if zones.iter().any(|zone| {
                    zone.start_note <= note
                        && note <= zone.end_note
                        && zone.vel_low <= velocity
                        && velocity <= zone.vel_high
                }) {
                    let zones_arc = Arc::make_mut(&mut zones);
                    if let Some(zone) = zones_arc.iter_mut().find(|zone| {
                        zone.start_note <= note
                            && note <= zone.end_note
                            && zone.vel_low <= velocity
                            && velocity <= zone.vel_high
                    }) {
                        zone.files.push(file);
                    }
                } else {
                    let name = unique_zone_name(&zones, "New Zone");
                    let width_notes = state
                        .drag_y
                        .map(zone_width_from_drag_y)
                        .unwrap_or(1)
                        .clamp(1, SAMPLE_MAP_NOTES);
                    let half = width_notes / 2;
                    let start_note = note.saturating_sub(half);
                    let end_note = (start_note + width_notes - 1).min(SAMPLE_MAP_NOTES - 1);
                    let start_note = end_note.saturating_sub(width_notes - 1);
                    let (vel_low, vel_high) =
                        find_vertical_slot(&zones, start_note, end_note, velocity);
                    let group = zones
                        .last()
                        .map(|zone| zone.group.clone())
                        .filter(|group| !group.is_empty())
                        .unwrap_or_else(|| String::from("New Group"));
                    Arc::make_mut(&mut zones).push(SampleZone {
                        name,
                        files: vec![file],
                        start_note,
                        end_note,
                        vel_low,
                        vel_high,
                        group,
                    });
                }
                state.shared.zones.store(zones);
                state.shared.bump_zones_version();
                state.shared.mark_dirty();
            }
            state.hovered_note = None;
            state.hovered_velocity = None;
            state.drag_y = None;
            state.dragging_zone_edge = None;
            state.dragging_zone_body = None;
        }
        Message::StartZoneEdgeDrag(index, edge) => {
            state.selected = Some(ZoneListSelection::Zone(index));
            state.dragging_zone_edge = Some((index, edge));
        }
        Message::StopZoneEdgeDrag => {
            state.dragging_zone_edge = None;
        }
        Message::StartZoneBodyDrag(index, note_offset, velocity_offset) => {
            state.selected = Some(ZoneListSelection::Zone(index));
            state.dragging_zone_body = Some((index, note_offset, velocity_offset));
        }
        Message::StopZoneBodyDrag => {
            state.dragging_zone_body = None;
        }
        Message::DeselectZone => {
            state.selected = None;
        }
        Message::SelectZoneListItem(selection) => {
            state.selected = Some(selection);
        }
        Message::DeleteSelectedZone => {
            if let Some(selection) = state.selected.take() {
                let mut zones = state.shared.zones.load();
                state.undo_stack.push(zones.to_vec());
                let zones_arc = Arc::make_mut(&mut zones);
                match selection {
                    ZoneListSelection::Zone(index) => {
                        if index < zones_arc.len() {
                            zones_arc.remove(index);
                        }
                    }
                    ZoneListSelection::Group(group_name) => {
                        zones_arc.retain(|zone| zone.group != group_name);
                    }
                }
                state.shared.zones.store(zones);
                state.shared.bump_zones_version();
                state.shared.mark_dirty();
            }
        }
        Message::Undo => {
            if let Some(previous_zones) = state.undo_stack.pop() {
                state.shared.zones.store(Arc::new(previous_zones));
                state.shared.bump_zones_version();
                state.shared.mark_dirty();
                state.selected = None;
            }
        }
        Message::StartRenameZone(index) => {
            let zones = state.shared.zones.load();
            if let Some(zone) = zones.get(index) {
                state.editing_zone_name = Some((index, zone.name.clone()));
            }
        }
        Message::UpdateRenameText(text) => {
            if let Some((_, current)) = state.editing_zone_name.as_mut() {
                *current = text;
            }
        }
        Message::FinishRenameZone => {
            if let Some((index, name)) = state.editing_zone_name.take() {
                let mut zones = state.shared.zones.load();
                if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index) {
                    zone.name = name;
                    state.shared.zones.store(zones);
                    state.shared.bump_zones_version();
                    state.shared.mark_dirty();
                }
            }
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

fn zone_width_from_drag_y(y: f32) -> usize {
    let ratio = (y / EDITOR_HEIGHT as f32).clamp(0.0, 1.0);
    let width = 1.0 + (SAMPLE_MAP_NOTES as f32 - 1.0) * (1.0 - ratio);
    width.round() as usize
}

const PIANO_ROLL_HEIGHT: f32 = 48.0;
const VELOCITY_COUNT: f32 = 128.0;
const HANDLE_SIZE: f32 = 4.0;

fn note_at_x(x: f32, width: f32) -> usize {
    let note_width = width / SAMPLE_MAP_NOTES as f32;
    (x / note_width)
        .floor()
        .clamp(0.0, SAMPLE_MAP_NOTES as f32 - 1.0) as usize
}

fn velocity_at_y(y: f32, grid_bottom: f32, grid_height: f32) -> u8 {
    if grid_height <= 0.0 {
        return 0;
    }
    let velocity_height = grid_height / VELOCITY_COUNT;
    ((grid_bottom - y) / velocity_height)
        .floor()
        .clamp(0.0, VELOCITY_COUNT - 1.0) as u8
}

fn zone_rect(zone: &SampleZone, bounds: Rectangle) -> Rectangle {
    let width = bounds.width;
    let grid_height = bounds.height - PIANO_ROLL_HEIGHT;
    let note_width = width / SAMPLE_MAP_NOTES as f32;
    let velocity_height = grid_height / VELOCITY_COUNT;
    let x = zone.start_note as f32 * note_width;
    let zone_width = (zone.end_note - zone.start_note + 1) as f32 * note_width;
    let top = grid_height - (zone.vel_high + 1) as f32 * velocity_height;
    let bottom = grid_height - zone.vel_low as f32 * velocity_height;
    Rectangle {
        x,
        y: top,
        width: zone_width,
        height: bottom - top,
    }
}

#[derive(Clone)]
struct ZoneEditorData {
    zones: Vec<SampleZone>,
    dragged_audio_file: Option<PathBuf>,
    hovered_note: Option<usize>,
    hovered_velocity: Option<u8>,
    drag_y: Option<f32>,
    dragging_zone_edge: Option<(usize, ZoneEdge)>,
    dragging_zone_body: Option<(usize, f32, f32)>,
    selected: Option<ZoneListSelection>,
}

struct ZoneEditor {
    data: ZoneEditorData,
}

impl ZoneEditor {
    fn edge_hit_test(&self, position: Point, bounds: Rectangle) -> Option<(usize, ZoneEdge)> {
        let grid_height = bounds.height - PIANO_ROLL_HEIGHT;
        if position.y < 0.0 || position.y > grid_height {
            return None;
        }
        for (index, zone) in self.data.zones.iter().enumerate() {
            let rect = zone_rect(zone, bounds);
            let near_left = (position.x - rect.x).abs() <= HANDLE_SIZE;
            let near_right = (position.x - (rect.x + rect.width)).abs() <= HANDLE_SIZE;
            let near_top = (position.y - rect.y).abs() <= HANDLE_SIZE;
            let near_bottom = (position.y - (rect.y + rect.height)).abs() <= HANDLE_SIZE;
            if !near_left && !near_right && !near_top && !near_bottom {
                continue;
            }
            let in_horizontal = position.x >= rect.x && position.x <= rect.x + rect.width;
            let in_vertical = position.y >= rect.y && position.y <= rect.y + rect.height;
            let edge = if near_left && in_vertical {
                ZoneEdge::Start
            } else if near_right && in_vertical {
                ZoneEdge::End
            } else if near_top && in_horizontal {
                ZoneEdge::Top
            } else if near_bottom && in_horizontal {
                ZoneEdge::Bottom
            } else {
                continue;
            };
            return Some((index, edge));
        }
        None
    }

    fn body_hit_test(&self, position: Point, bounds: Rectangle) -> Option<usize> {
        let grid_height = bounds.height - PIANO_ROLL_HEIGHT;
        if position.y < 0.0 || position.y > grid_height {
            return None;
        }
        for (index, zone) in self.data.zones.iter().enumerate() {
            let rect = zone_rect(zone, bounds);
            if position.x >= rect.x
                && position.x <= rect.x + rect.width
                && position.y >= rect.y
                && position.y <= rect.y + rect.height
            {
                return Some(index);
            }
        }
        None
    }
}

impl canvas::Program<Message> for ZoneEditor {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &maolan_baseview::iced::Event,
        bounds: Rectangle,
        cursor: maolan_baseview::iced::mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            maolan_baseview::iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Delete),
                ..
            }) => Some(canvas::Action::publish(Message::DeleteSelectedZone).and_capture()),
            maolan_baseview::iced::Event::Mouse(mouse_event) => {
                let position = cursor.position_in(bounds)?;
                match mouse_event {
                    maolan_baseview::iced::mouse::Event::CursorMoved { .. } => {
                        let note = note_at_x(position.x, bounds.width);
                        let velocity = velocity_at_y(position.y, bounds.height - PIANO_ROLL_HEIGHT, bounds.height - PIANO_ROLL_HEIGHT);
                        Some(canvas::Action::publish(Message::ZoneNoteHovered(
                            note,
                            velocity,
                            position.y + bounds.y,
                        )))
                    }
                    maolan_baseview::iced::mouse::Event::ButtonPressed(
                        maolan_baseview::iced::mouse::Button::Left,
                    ) => {
                        if let Some((index, edge)) = self.edge_hit_test(position, bounds) {
                            Some(
                                canvas::Action::publish(Message::StartZoneEdgeDrag(index, edge))
                                    .and_capture(),
                            )
                        } else if let Some(index) = self.body_hit_test(position, bounds) {
                            let zone = &self.data.zones[index];
                            let note_width = bounds.width / SAMPLE_MAP_NOTES as f32;
                            let grid_height = bounds.height - PIANO_ROLL_HEIGHT;
                            let velocity_height = grid_height / VELOCITY_COUNT;
                            let note_offset = (position.x - zone.start_note as f32 * note_width)
                                / note_width;
                            let velocity_offset =
                                ((grid_height - position.y) / velocity_height) - zone.vel_low as f32;
                            Some(
                                canvas::Action::publish(Message::StartZoneBodyDrag(
                                    index,
                                    note_offset,
                                    velocity_offset,
                                ))
                                .and_capture(),
                            )
                        } else if self.data.dragged_audio_file.is_none() {
                            Some(canvas::Action::publish(Message::DeselectZone).and_capture())
                        } else {
                            None
                        }
                    }
                    maolan_baseview::iced::mouse::Event::ButtonReleased(
                        maolan_baseview::iced::mouse::Button::Left,
                    ) => {
                        let note = note_at_x(position.x, bounds.width);
                        let velocity = velocity_at_y(
                            position.y,
                            bounds.height - PIANO_ROLL_HEIGHT,
                            bounds.height - PIANO_ROLL_HEIGHT,
                        );
                        Some(
                            canvas::Action::publish(Message::ZoneNoteReleased(note, velocity))
                                .and_capture(),
                        )
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &maolan_baseview::iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: maolan_baseview::iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let width = bounds.width;
        let height = bounds.height;
        let grid_height = height - PIANO_ROLL_HEIGHT;
        let note_width = width / SAMPLE_MAP_NOTES as f32;
        let velocity_height = grid_height / VELOCITY_COUNT;

        let background = canvas::Path::rectangle(Point::ORIGIN, bounds.size());
        frame.fill(&background, Color::from_rgb(0.075, 0.078, 0.095));

        for note in 0..SAMPLE_MAP_NOTES {
            let x = note as f32 * note_width;
            let is_c = note % 12 == 0;
            let line = canvas::Path::line(
                Point::new(x, 0.0),
                Point::new(x, grid_height),
            );
            frame.stroke(
                &line,
                canvas::Stroke::default()
                    .with_color(if is_c {
                        Color::from_rgb(0.18, 0.18, 0.22)
                    } else {
                        Color::from_rgb(0.12, 0.12, 0.14)
                    })
                    .with_width(1.0),
            );
        }

        for vel in (0..=128).step_by(16) {
            let y = grid_height - vel as f32 * velocity_height;
            let line = canvas::Path::line(Point::new(0.0, y), Point::new(width, y));
            frame.stroke(
                &line,
                canvas::Stroke::default()
                    .with_color(Color::from_rgb(0.14, 0.14, 0.16))
                    .with_width(1.0),
            );
        }

        for (index, zone) in self.data.zones.iter().enumerate() {
            let rect = zone_rect(zone, bounds);
            let path = canvas::Path::rectangle(Point::new(rect.x, rect.y), Size::new(rect.width, rect.height));
            let selected = self
                .data
                .selected
                .as_ref()
                .is_some_and(|sel| matches!(sel, ZoneListSelection::Zone(i) if *i == index));
            let color = if self.data.dragging_zone_edge.is_some_and(|(i, _)| i == index) {
                Color::from_rgb(0.20, 0.28, 0.40)
            } else if selected {
                Color::from_rgb(0.24, 0.34, 0.50)
            } else {
                Color::from_rgb(0.16, 0.23, 0.34)
            };
            frame.fill(&path, color);
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_color(if selected {
                        Color::from_rgb(0.85, 0.90, 1.0)
                    } else {
                        Color::from_rgb(0.22, 0.48, 0.95)
                    })
                    .with_width(if selected { 2.0 } else { 1.0 }),
            );

            let text = canvas::Text {
                content: zone.name.clone(),
                position: Point::new(rect.x + 2.0, rect.y + 2.0),
                color: Color::from_rgb(0.72, 0.74, 0.82),
                size: maolan_baseview::iced::Pixels(9.0),
                ..canvas::Text::default()
            };
            frame.fill_text(text);
        }

        if let Some(position) = cursor.position_in(bounds) {
            if let Some((index, edge)) = self.edge_hit_test(position, bounds) {
                let zone = &self.data.zones[index];
                let rect = zone_rect(zone, bounds);
                let (start, end) = match edge {
                    ZoneEdge::Start => (
                        Point::new(rect.x, rect.y),
                        Point::new(rect.x, rect.y + rect.height),
                    ),
                    ZoneEdge::End => (
                        Point::new(rect.x + rect.width, rect.y),
                        Point::new(rect.x + rect.width, rect.y + rect.height),
                    ),
                    ZoneEdge::Top => (
                        Point::new(rect.x, rect.y),
                        Point::new(rect.x + rect.width, rect.y),
                    ),
                    ZoneEdge::Bottom => (
                        Point::new(rect.x, rect.y + rect.height),
                        Point::new(rect.x + rect.width, rect.y + rect.height),
                    ),
                };
                let line = canvas::Path::line(start, end);
                frame.stroke(
                    &line,
                    canvas::Stroke::default()
                        .with_color(Color::from_rgb(0.78, 0.88, 1.0))
                        .with_width(2.0),
                );
            }
        }

        if self.data.dragged_audio_file.is_some() {
            if let Some(hovered_note) = self.data.hovered_note {
                let preview_width = self
                    .data
                    .drag_y
                    .map(zone_width_from_drag_y)
                    .unwrap_or(1)
                    .clamp(1, SAMPLE_MAP_NOTES);
                let half = preview_width / 2;
                let start_note = hovered_note.saturating_sub(half);
                let end_note = (start_note + preview_width - 1).min(SAMPLE_MAP_NOTES - 1);
                let start_note = end_note.saturating_sub(preview_width - 1);
                let velocity = self.data.hovered_velocity.unwrap_or(0);
                let (vel_low, vel_high) =
                    find_vertical_slot(&self.data.zones, start_note, end_note, velocity);
                let preview = SampleZone {
                    name: String::new(),
                    files: Vec::new(),
                    start_note,
                    end_note,
                    vel_low,
                    vel_high,
                    group: String::new(),
                };
                let rect = zone_rect(&preview, bounds);
                let path = canvas::Path::rectangle(Point::new(rect.x, rect.y), Size::new(rect.width, rect.height));
                frame.fill(&path, Color::from_rgba(0.22, 0.34, 0.48, 0.6));
                frame.stroke(
                    &path,
                    canvas::Stroke::default()
                        .with_color(Color::from_rgb(0.42, 0.72, 1.0))
                        .with_width(1.0),
                );
            }
        }

        let piano_background = canvas::Path::rectangle(
            Point::new(0.0, grid_height),
            Size::new(width, PIANO_ROLL_HEIGHT),
        );
        frame.fill(&piano_background, Color::from_rgb(0.045, 0.047, 0.058));

        for note in 0..SAMPLE_MAP_NOTES {
            let x = note as f32 * note_width;
            let pitch = note % 12;
            let is_black = matches!(pitch, 1 | 3 | 6 | 8 | 10);
            let key_height = if is_black { PIANO_ROLL_HEIGHT * 0.65 } else { PIANO_ROLL_HEIGHT };
            let key_y = grid_height + PIANO_ROLL_HEIGHT - key_height;
            let key_rect = canvas::Path::rectangle(
                Point::new(x, key_y),
                Size::new(note_width, key_height),
            );
            frame.fill(
                &key_rect,
                if is_black {
                    Color::from_rgb(0.035, 0.037, 0.045)
                } else {
                    Color::from_rgb(0.78, 0.80, 0.84)
                },
            );
            frame.stroke(
                &key_rect,
                canvas::Stroke::default()
                    .with_color(if is_black {
                        Color::from_rgb(0.015, 0.016, 0.020)
                    } else {
                        Color::from_rgb(0.42, 0.43, 0.48)
                    })
                    .with_width(1.0),
            );
            if pitch == 0 {
                let label = canvas::Text {
                    content: format!("C{}", note as i32 / 12 - 1),
                    position: Point::new(x + 2.0, grid_height + PIANO_ROLL_HEIGHT - 12.0),
                    color: Color::from_rgb(0.16, 0.17, 0.20),
                    size: maolan_baseview::iced::Pixels(9.0),
                    ..canvas::Text::default()
                };
                frame.fill_text(label);
            }
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: maolan_baseview::iced::mouse::Cursor,
    ) -> maolan_baseview::iced::mouse::Interaction {
        let Some(position) = cursor.position_in(bounds) else {
            return maolan_baseview::iced::mouse::Interaction::default();
        };
        if self.data.dragging_zone_body.is_some() {
            return maolan_baseview::iced::mouse::Interaction::Grabbing;
        }
        match self.edge_hit_test(position, bounds) {
            Some((_, ZoneEdge::Start | ZoneEdge::End)) => {
                maolan_baseview::iced::mouse::Interaction::ResizingHorizontally
            }
            Some((_, ZoneEdge::Top | ZoneEdge::Bottom)) => {
                maolan_baseview::iced::mouse::Interaction::ResizingVertically
            }
            None => {
                if self.body_hit_test(position, bounds).is_some() {
                    maolan_baseview::iced::mouse::Interaction::Grab
                } else {
                    maolan_baseview::iced::mouse::Interaction::default()
                }
            }
        }
    }
}

fn sample_map<'a>(state: &'a State) -> Element<'a, Message> {
    let data = ZoneEditorData {
        zones: state.shared.zones.load().to_vec(),
        dragged_audio_file: state.dragged_audio_file.clone(),
        hovered_note: state.hovered_note,
        hovered_velocity: state.hovered_velocity,
        drag_y: state.drag_y,
        dragging_zone_edge: state.dragging_zone_edge,
        dragging_zone_body: state.dragging_zone_body,
        selected: state.selected.clone(),
    };
    canvas(ZoneEditor { data })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn resize_handle(side: SidePanel) -> Element<'static, Message> {
    mouse_area(
        container(text(""))
            .width(Length::Fixed(6.0))
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.18, 0.18, 0.22))),
                border: Border {
                    color: Color::from_rgb(0.27, 0.27, 0.32),
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..container::Style::default()
            }),
    )
    .on_press(Message::StartSideResize(side))
    .on_release(Message::StopSideResize)
    .into()
}

fn zone_row<'a>(state: &'a State, index: usize, zone: SampleZone) -> Element<'a, Message> {
    let file_name = if zone.files.is_empty() {
        String::new()
    } else if zone.files.len() == 1 {
        zone.files[0]
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default()
    } else {
        format!("{} files", zone.files.len())
    };
    let range_text = format!(
        "{}-{} | Vel {}-{}",
        zone.start_note, zone.end_note, zone.vel_low, zone.vel_high
    );

    let is_editing = state
        .editing_zone_name
        .as_ref()
        .is_some_and(|(edit_index, _)| *edit_index == index);
    let name_element: Element<'a, Message> = if is_editing {
        let edit_text = state
            .editing_zone_name
            .as_ref()
            .map(|(_, text)| text.as_str())
            .unwrap_or("");
        text_input("Zone name", edit_text)
            .on_input(Message::UpdateRenameText)
            .on_submit(Message::FinishRenameZone)
            .size(11)
            .into()
    } else {
        text(zone.name).size(11).into()
    };

    let content = column![
        name_element,
        text(file_name).size(9).color(Color::from_rgb(0.48, 0.50, 0.58)),
        text(range_text).size(9).color(Color::from_rgb(0.48, 0.50, 0.58)),
    ]
    .spacing(1)
    .width(Length::Fill);

    let selected = state
        .selected
        .as_ref()
        .is_some_and(|sel| matches!(sel, ZoneListSelection::Zone(i) if *i == index));

    let wrapped = if is_editing {
        container(content)
    } else {
        container(
            mouse_area(content)
                .on_press(Message::SelectZoneListItem(ZoneListSelection::Zone(index)))
                .on_double_click(Message::StartRenameZone(index)),
        )
    }
    .width(Length::Fill)
    .padding([4, 6])
    .style(move |_theme: &Theme| container::Style {
        border: Border {
            color: if selected {
                Color::from_rgb(0.42, 0.60, 0.90)
            } else {
                Color::from_rgb(0.18, 0.18, 0.22)
            },
            width: if selected { 2.0 } else { 1.0 },
            radius: 3.0.into(),
        },
        ..container::Style::default()
    });

    wrapped.into()
}

fn zones_panel<'a>(state: &'a State) -> Element<'a, Message> {
    let shared_zones = state.shared.zones.load();

    let mut groups: Vec<(String, Vec<(usize, SampleZone)>)> = Vec::new();
    for (index, zone) in shared_zones.iter().cloned().enumerate() {
        match groups.iter_mut().find(|(name, _)| name == &zone.group) {
            Some((_, entries)) => entries.push((index, zone)),
            None => groups.push((zone.group.clone(), vec![(index, zone)])),
        }
    }

    let mut panel = column![].spacing(6).width(Length::Fill);
    for (group_name, entries) in groups {
        let group_selected = state
            .selected
            .as_ref()
            .is_some_and(|sel| matches!(sel, ZoneListSelection::Group(name) if name == &group_name));
        let mut zones = column![].spacing(3).width(Length::Fill);
        for (index, zone) in entries {
            zones = zones.push(zone_row(state, index, zone));
        }
        let group_name_for_press = group_name.clone();
        let header = mouse_area(
            container(
                text(group_name)
                    .size(10)
                    .color(Color::from_rgb(0.90, 0.91, 0.94)),
            )
            .width(Length::Fill)
            .padding([5, 6])
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(if group_selected {
                    Color::from_rgb(0.18, 0.28, 0.45)
                } else {
                    Color::from_rgb(0.10, 0.12, 0.16)
                })),
                border: Border {
                    color: if group_selected {
                        Color::from_rgb(0.45, 0.65, 0.95)
                    } else {
                        Color::from_rgb(0.18, 0.20, 0.26)
                    },
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..container::Style::default()
            }),
        )
        .on_press(Message::SelectZoneListItem(ZoneListSelection::Group(
            group_name_for_press,
        )));
        let group_box = container(
            column![header, zones]
                .spacing(4)
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .padding([6, 6])
        .style(move |_theme: &Theme| container::Style {
            border: Border {
                color: if group_selected {
                    Color::from_rgb(0.45, 0.65, 0.95)
                } else {
                    Color::from_rgb(0.22, 0.26, 0.34)
                },
                width: if group_selected { 2.0 } else { 1.0 },
                radius: 4.0.into(),
            },
            ..container::Style::default()
        });
        panel = panel.push(group_box);
    }

    container(
        column![
            section_title("Zones"),
            scrollable(panel).height(Length::Fill),
        ]
        .spacing(8)
        .height(Length::Fill)
        .align_x(Alignment::Start),
    )
    .width(Length::Fixed(state.zones_width))
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
            container(
                text(state
                    .browser_path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| state.browser_path.display().to_string()))
                .size(10),
            )
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
    let sample_map = fill_panel_no_title(sample_map(state));

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

    let selected_eg =
        EgSelectorOption::from_index(state.selected_eg).unwrap_or(EgSelectorOption::Amp);
    let eg_selector = pick_list(
        EgSelectorOption::all().to_vec(),
        Some(selected_eg),
        |option| Message::SelectEg(option.index()),
    )
    .width(Length::Fixed(100.0));

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

    let top_row = row![
        top_bar,
        column![filter_selector, selected_filter_panel].spacing(10),
    ]
    .spacing(10)
    .align_y(Alignment::Start);

    let bottom_row = row![
        column![lfo_selector, lfo_panel].spacing(10),
        column![eg_selector, selected_eg_panel].spacing(10),
    ]
    .spacing(10)
    .align_y(Alignment::Start);

    let main_content = column![
        sample_map,
        top_row,
        bottom_row,
    ]
    .spacing(12)
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Alignment::Start);

    let content_row = row![
        zones_panel(state),
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

fn subscription(state: &State) -> maolan_baseview::iced::Subscription<Message> {
    if state.editing_zone_name.is_some() {
        return maolan_baseview::iced::Subscription::none();
    }
    maolan_baseview::iced::event::listen_with(|event, status, _ids| {
        if status == maolan_baseview::iced::event::Status::Captured {
            return None;
        }
        if let maolan_baseview::iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            ..
        }) = event
        {
            let is_undo = matches!(
                key,
                keyboard::Key::Character(ref c) if c == "z" && modifiers.command()
            );
            let is_delete =
                matches!(key, keyboard::Key::Named(keyboard::key::Named::Delete));
            if is_undo {
                return Some(Message::Undo);
            }
            if is_delete {
                return Some(Message::DeleteSelectedZone);
            }
        }
        None
    })
}

fn build_app(shared: Arc<SharedState>) -> impl maolan_baseview::iced::Program {
    maolan_baseview::iced::application(move || init(shared.clone()), update, view)
        .font(iced_fonts::LUCIDE_FONT_BYTES)
        .theme(theme)
        .subscription(subscription)
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
