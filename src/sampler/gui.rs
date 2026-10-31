use std::{
    ffi::CStr,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use maolan_baseview::iced::{
    Alignment, Background, Border, Color, Element, Length, Point, Rectangle, Size, Task, Theme,
    alignment::{Horizontal, Vertical},
    keyboard,
    widget::{
        button, canvas, checkbox, column, container, mouse_area, pick_list, row, scrollable, text,
        text_input,
    },
    window,
};
#[cfg(target_os = "macos")]
use maolan_clap::ffi::CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
use maolan_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use maolan_clap::ffi::CLAP_WINDOW_API_X11;
use maolan_widgets::adsr::{AdsrMessage, adsr_envelope};
use maolan_widgets::arch_slider::ArchSlider;
use maolan_widgets::iced_aw::Wrap;
use maolan_widgets::knob::{Knob, RangeKnob, range_knob};
use maolan_widgets::meters;
use maolan_widgets::multi_toggler::{LabelPosition, horizontal_multi_toggler};
use maolan_widgets::piano::{
    Orientation, draw_octave_into, draw_partial_octave_into, note_at_in_range, octave_note_count,
};
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd"
))]
use raw_window_handle::RawWindowHandle;
use raw_window_handle::{HandleError, HasWindowHandle, WindowHandle};
use symphonia::core::{
    codecs::audio::CODEC_ID_NULL_AUDIO,
    formats::{FormatOptions, probe::Hint},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

use crate::{
    common::{
        envelope::AdsrParams,
        filter::{FilterParams, FilterSubtype, FilterType},
        lfo::LfoShape,
        lfo_assignment::{LfoAssignmentConfig, LfoAssignmentState, ModRouteParamIds},
    },
    sampler::{
        dsp::{
            mod_matrix::{ModCurve, ModRoute, ModSource, ModTarget},
            patch::Patch,
            sfz::{export_patch_to_sfz, is_non_vendor_sfz_opcode},
            voice::LfoParams,
            zone::{CcCondition, CurveType, LoopMode, OffMode, SamplePlayMode},
        },
        load_status::SamplerLoadStatus,
        loader::{PresetInfo, detect_format},
        params::{PARAMS, ParamId},
        plugin::{MAX_SAMPLER_BUSES, SharedState, build_export_patch, output_bus_label},
        state::{SampleGroup, SampleZone},
    },
};

pub const EDITOR_WIDTH: u32 = 1100;
pub const EDITOR_HEIGHT: u32 = 790;
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
#[allow(clippy::enum_variant_names)]
pub enum Message {
    None,
    SetParam(ParamId, f32),
    SetPitchBendRange(f32, f32),
    ReleaseParam(ParamId),
    ResetParam(ParamId),
    AssignLfoToParam(ParamId),
    ToggleLfoAssignment(usize),
    ToggleParam(ParamId, bool),
    SelectLfo(usize),
    SelectFilter(usize),
    SelectEg(usize),
    ToggleZonesPanel,
    ToggleBrowserPanel,
    StartSideResize(SidePanel),
    ResizeSidePanel(f32),
    StopSideResize,
    OpenBrowserEntry(PathBuf),
    BeginAudioFileDrag(PathBuf),
    LoadInstrument(PathBuf),
    ImportInstrument(PathBuf),
    PickInstrumentFile,
    PickImportFile,
    ReloadInstrument,
    ExportSfz,
    SelectSf2Preset(PresetInfo),
    PollLoadStatus,
    ZoneNoteHovered(usize, u8, f32),
    ZoneNoteReleased(usize, u8),
    StartZoneEdgeDrag(usize, ZoneEdge),
    StopZoneEdgeDrag,
    StartZoneBodyDrag(usize, f32, f32),
    StopZoneBodyDrag,
    CreateZoneListItem(ZoneCreateKind),
    BeginZoneListDrag(usize),
    HoverZoneDropGroup(Option<String>),
    FinishZoneListDrag,
    StartRenameZone(usize),
    UpdateRenameText(String),
    FinishRenameZone,
    DeselectZone,
    DeleteSelectedZone,
    SelectZoneListItem(ZoneListSelection),
    Undo,
    OpenSamplerEditor(usize),
    CloseSamplerEditor,
    OpenGroupEditor(String),
    CloseGroupEditor,
    StartRenameGroup,
    UpdateRenameGroupText(String),
    FinishRenameGroup,
    SelectGroupEg(usize),
    SelectGroupLfo(usize),
    SetSelectedGroupSwLastRange(f32, f32),
    AudioEditor(Box<maolan_editor::app::Message>),
    SetEditingZoneValue(ZoneEditField, f32),
    SetEditingZoneRange(ZoneRangeField, f32, f32),
    ToggleEditingZoneReverse(bool),
    SetEditingZonePlayMode(ZonePlayModeOption),
    SetEditingZoneVelocityCurve(ZoneCurveOption),
    SetEditingZoneOffMode(ZoneOffModeOption),
    AddEditingZoneCcRoute,
    RemoveEditingZoneCcRoute(usize),
    SetEditingZoneCcRouteSource(usize, ModSource),
    SetEditingZoneCcRouteCc(usize, f32),
    SetEditingZoneCcRouteTarget(usize, CcModTargetOption),
    SetEditingZoneCcRouteDepth(usize, f32),
    SetEditingZoneCcRouteCurve(usize, CcCurveOption),
    AddEditingZoneCcCondition,
    RemoveEditingZoneCcCondition(usize),
    SetEditingZoneCcConditionCc(usize, f32),
    SetEditingZoneCcConditionRange(usize, f32, f32),
    SetSelectedGroupValue(GroupEditField, f32),
    SetSelectedGroupParam(GroupParamField, f32),
    SetSelectedGroupLfoShape(usize, LfoShape),
    SetSelectedGroupFilterType(FilterType),
    SetSelectedGroupFilterSubtype(FilterSubtype),
    SetSelectedGroupSwLabel(String),
    SetEditingZoneExtraSfz(String),
    SetSelectedGroupExtraSfz(String),
    PianoKeyPressed(u8, u8),
    PianoKeyReleased(u8),
    SamplerAuditionFinished(u64, u8),
    AddOutput,
    CleanUpOutputs,
    RemoveOutput(usize),
    RenameOutput(usize, String),
    PointerReleased,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneEditField {
    RootKey,
    GainDb,
    Pan,
    Width,
    Position,
    AmpKeytrack,
    PitchOffset,
    KeyTracking,
    StartOffset,
    OffsetRandom,
    EndOffset,
    SeqLength,
    SeqPosition,
    OffBy,
    Output,
    AmpVeltrack,
    Count,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneRangeField {
    Velocity,
    KeyFadeIn,
    KeyFadeOut,
    VelFadeIn,
    VelFadeOut,
    PitchBend,
    Channel,
    PitchBendRange,
    Random,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupEditField {
    PolyLimit,
    ExclusiveGroup,
    GainDb,
    Pan,
    Output,
    SwLast,
    SwDown,
    SwUp,
    SwPrevious,
    SwDefault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupParamField {
    Eg1Attack,
    Eg1Decay,
    Eg1Sustain,
    Eg1Release,
    Eg2Attack,
    Eg2Decay,
    Eg2Sustain,
    Eg2Release,
    Lfo1Rate,
    Lfo1Amount,
    Lfo2Rate,
    Lfo2Amount,
    Lfo3Rate,
    Lfo3Amount,
    Lfo4Rate,
    Lfo4Amount,
    FilterCutoff,
    FilterResonance,
    FilterKeyTrack,
    FilterVelTrack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcModTargetOption {
    Volume,
    Pan,
    Tune,
    Cutoff,
    Resonance,
    Offset,
}

impl CcModTargetOption {
    fn all() -> [Self; 6] {
        [
            Self::Volume,
            Self::Pan,
            Self::Tune,
            Self::Cutoff,
            Self::Resonance,
            Self::Offset,
        ]
    }

    fn target(self) -> ModTarget {
        match self {
            Self::Volume => ModTarget::Amplitude,
            Self::Pan => ModTarget::Pan,
            Self::Tune => ModTarget::Pitch,
            Self::Cutoff => ModTarget::FilterCutoff,
            Self::Resonance => ModTarget::FilterResonance,
            Self::Offset => ModTarget::SampleOffset,
        }
    }

    fn from_target(target: ModTarget) -> Option<Self> {
        match target {
            ModTarget::Amplitude => Some(Self::Volume),
            ModTarget::Pan => Some(Self::Pan),
            ModTarget::Pitch => Some(Self::Tune),
            ModTarget::FilterCutoff => Some(Self::Cutoff),
            ModTarget::FilterResonance => Some(Self::Resonance),
            ModTarget::SampleOffset => Some(Self::Offset),
            ModTarget::None | ModTarget::SampleStart | ModTarget::Delay => None,
        }
    }
}

impl std::fmt::Display for CcModTargetOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Volume => write!(f, "Volume"),
            Self::Pan => write!(f, "Pan"),
            Self::Tune => write!(f, "Tune"),
            Self::Cutoff => write!(f, "Cutoff"),
            Self::Resonance => write!(f, "Resonance"),
            Self::Offset => write!(f, "Offset"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcCurveOption {
    Linear,
    Exponential,
    Logarithmic,
    SCurve,
    Custom,
}

impl CcCurveOption {
    fn all() -> [Self; 5] {
        [
            Self::Linear,
            Self::Exponential,
            Self::Logarithmic,
            Self::SCurve,
            Self::Custom,
        ]
    }

    fn curve(self) -> ModCurve {
        match self {
            Self::Linear | Self::Custom => ModCurve::linear(),
            Self::Exponential => mod_curve_from_fn(|x| x * x),
            Self::Logarithmic => mod_curve_from_fn(f32::sqrt),
            Self::SCurve => mod_curve_from_fn(|x| x * x * (3.0 - 2.0 * x)),
        }
    }

    fn from_curve(curve: &ModCurve) -> Self {
        const EPSILON: f32 = 0.001;
        for option in [
            Self::Linear,
            Self::Exponential,
            Self::Logarithmic,
            Self::SCurve,
        ] {
            let expected = option.curve();
            if curve
                .points
                .iter()
                .zip(expected.points.iter())
                .all(|(a, b)| (*a - *b).abs() <= EPSILON)
            {
                return option;
            }
        }
        Self::Custom
    }
}

impl std::fmt::Display for CcCurveOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Linear => write!(f, "Linear"),
            Self::Exponential => write!(f, "Exp"),
            Self::Logarithmic => write!(f, "Log"),
            Self::SCurve => write!(f, "S"),
            Self::Custom => write!(f, "Custom"),
        }
    }
}

fn mod_curve_from_fn(f: impl Fn(f32) -> f32) -> ModCurve {
    let mut points = [0.0; 128];
    for (index, point) in points.iter_mut().enumerate() {
        *point = f(index as f32 / 127.0).clamp(0.0, 1.0);
    }
    ModCurve { points }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZonePlayModeOption {
    Attack,
    OneShot,
    Release,
    First,
    Legato,
}

impl ZonePlayModeOption {
    fn all() -> [Self; 5] {
        [
            Self::Attack,
            Self::OneShot,
            Self::Release,
            Self::First,
            Self::Legato,
        ]
    }

    fn from_mode(mode: SamplePlayMode) -> Self {
        match mode {
            SamplePlayMode::OneShot => Self::OneShot,
            SamplePlayMode::OnRelease => Self::Release,
            SamplePlayMode::First => Self::First,
            SamplePlayMode::Legato => Self::Legato,
            SamplePlayMode::Normal => Self::Attack,
        }
    }

    fn mode(self) -> SamplePlayMode {
        match self {
            Self::Attack => SamplePlayMode::Normal,
            Self::OneShot => SamplePlayMode::OneShot,
            Self::Release => SamplePlayMode::OnRelease,
            Self::First => SamplePlayMode::First,
            Self::Legato => SamplePlayMode::Legato,
        }
    }
}

impl std::fmt::Display for ZonePlayModeOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Attack => write!(f, "Attack"),
            Self::OneShot => write!(f, "One Shot"),
            Self::Release => write!(f, "Release"),
            Self::First => write!(f, "First"),
            Self::Legato => write!(f, "Legato"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneCurveOption {
    Linear,
    Exponential,
    Logarithmic,
    SCurve,
}

impl ZoneCurveOption {
    fn all() -> [Self; 4] {
        [
            Self::Linear,
            Self::Exponential,
            Self::Logarithmic,
            Self::SCurve,
        ]
    }

    fn from_curve(curve: CurveType) -> Self {
        match curve {
            CurveType::Exponential => Self::Exponential,
            CurveType::Logarithmic => Self::Logarithmic,
            CurveType::SCurve => Self::SCurve,
            CurveType::Linear => Self::Linear,
        }
    }

    fn curve(self) -> CurveType {
        match self {
            Self::Linear => CurveType::Linear,
            Self::Exponential => CurveType::Exponential,
            Self::Logarithmic => CurveType::Logarithmic,
            Self::SCurve => CurveType::SCurve,
        }
    }
}

impl std::fmt::Display for ZoneCurveOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Linear => write!(f, "Linear"),
            Self::Exponential => write!(f, "Exponential"),
            Self::Logarithmic => write!(f, "Logarithmic"),
            Self::SCurve => write!(f, "S-Curve"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneOffModeOption {
    Fast,
    Normal,
}

impl ZoneOffModeOption {
    fn all() -> [Self; 2] {
        [Self::Fast, Self::Normal]
    }

    fn from_mode(mode: OffMode) -> Self {
        match mode {
            OffMode::Normal => Self::Normal,
            OffMode::Fast => Self::Fast,
        }
    }

    fn mode(self) -> OffMode {
        match self {
            Self::Fast => OffMode::Fast,
            Self::Normal => OffMode::Normal,
        }
    }
}

impl std::fmt::Display for ZoneOffModeOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fast => write!(f, "Fast"),
            Self::Normal => write!(f, "Normal"),
        }
    }
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
pub enum ZoneCreateKind {
    Group,
    Zone,
}

impl std::fmt::Display for ZoneCreateKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZoneCreateKind::Group => write!(f, "Group"),
            ZoneCreateKind::Zone => write!(f, "Zone"),
        }
    }
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
    zones_visible: bool,
    browser_visible: bool,
    zones_width: f32,
    browser_width: f32,
    resizing_side: Option<SidePanel>,
    /// (mouse x, panel width) captured when a side-panel resize drag starts.
    resize_start: Option<(f32, f32)>,
    browser_path: PathBuf,
    browser_entries: Vec<BrowserEntry>,
    dragged_audio_file: Option<PathBuf>,
    status_revision: u64,
    hovered_note: Option<usize>,
    hovered_velocity: Option<u8>,
    drag_y: Option<f32>,
    dragging_zone_edge: Option<(usize, ZoneEdge)>,
    dragging_zone_body: Option<(usize, f32, f32)>,
    dragging_zone_list_item: Option<usize>,
    hovered_zone_drop_group: Option<String>,
    editing_zone_name: Option<(usize, String)>,
    selected: Option<ZoneListSelection>,
    undo_stack: Vec<Vec<SampleZone>>,
    editing_zone_index: Option<usize>,
    editing_group_name: Option<String>,
    renaming_group: Option<String>,
    selected_group_eg: usize,
    selected_group_lfo: usize,
    audio_editor: Option<maolan_editor::app::EditApp>,
    audio_editor_path: Option<PathBuf>,
    extra_sfz_opcode_text: String,
    piano_active_note: Option<u8>,
    sampler_audition: Option<(u64, u8)>,
    next_sampler_audition_id: u64,
}

#[derive(Debug, Clone)]
struct BrowserEntry {
    name: String,
    path: PathBuf,
    kind: BrowserEntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserEntryKind {
    Directory,
    Audio,
    Instrument,
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

fn unique_group_name(groups: &[SampleGroup], zones: &[SampleZone], base: &str) -> String {
    let exists = |candidate: &str| {
        groups.iter().any(|group| group.name == candidate)
            || zones.iter().any(|zone| zone.group == candidate)
    };
    if !exists(base) {
        return base.to_string();
    }

    let mut index = 2;
    loop {
        let candidate = format!("{base} {index}");
        if !exists(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

fn group_for_new_zone(state: &State, groups: &[SampleGroup], zones: &[SampleZone]) -> String {
    match &state.selected {
        Some(ZoneListSelection::Group(group)) => group.clone(),
        Some(ZoneListSelection::Zone(index)) => zones
            .get(*index)
            .map(|zone| zone.group.clone())
            .filter(|group| !group.is_empty())
            .unwrap_or_else(|| String::from("New Group")),
        None => groups
            .last()
            .map(|group| group.name.clone())
            .or_else(|| zones.last().map(|zone| zone.group.clone()))
            .filter(|group| !group.is_empty())
            .unwrap_or_else(|| String::from("New Group")),
    }
}

fn default_new_zone(zones: &[SampleZone], group: String) -> SampleZone {
    SampleZone::new_basic(
        unique_zone_name(zones, "New Zone"),
        Vec::new(),
        60,
        60,
        0,
        127,
        group,
    )
}

fn default_export_path(state: &State) -> PathBuf {
    state
        .shared
        .instrument_path
        .lock()
        .as_ref()
        .map(|path| path.with_extension("sfz"))
        .unwrap_or_else(|| PathBuf::from("sampler.sfz"))
}

fn export_patch_from_state(state: &State) -> Patch {
    build_export_patch(&state.shared)
}

fn patch_zone_sample(
    state: &State,
    zone_index: usize,
) -> Option<Arc<crate::sampler::dsp::sample::Sample>> {
    let patch = state.shared.patch.load();
    patch
        .parts
        .iter()
        .flat_map(|part| part.groups.iter())
        .flat_map(|group| group.zones.iter())
        .nth(zone_index)
        .map(|zone| zone.sample.clone())
}

fn sampler_vu_meter<'a>(state: &State) -> Element<'a, Message> {
    let levels = state.shared.output_levels_db();
    meters::meters(levels.len(), &levels, 1.0)
}

fn zone_audition_note(zone: &SampleZone) -> (u8, u8) {
    let note = usize::from(zone.root_key)
        .clamp(zone.start_note, zone.end_note)
        .min(127) as u8;
    let velocity = zone
        .vel_low
        .saturating_add((zone.vel_high - zone.vel_low) / 2);
    (note, velocity.max(1))
}

fn zone_audition_duration(state: &State, zone_index: usize) -> Option<Duration> {
    let sample = patch_zone_sample(state, zone_index)?;
    if sample.frames == 0 || sample.sample_rate <= 0.0 {
        return None;
    }
    Some(Duration::from_secs_f32(
        sample.frames as f32 / sample.sample_rate,
    ))
}

async fn finish_sampler_audition_after(duration: Duration, id: u64, note: u8) -> (u64, u8) {
    std::thread::sleep(duration);
    (id, note)
}

fn editor_audio_buffer_for_zone(
    state: &State,
    zone_index: usize,
) -> Option<maolan_editor::app::AudioBuffer> {
    let zones = state.shared.zones.load();
    let zone = zones.get(zone_index);
    let sample = patch_zone_sample(state, zone_index)?;
    let name = zone
        .and_then(|zone| {
            zone.files
                .first()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .map(ToOwned::to_owned)
                .or_else(|| (!zone.name.is_empty()).then(|| zone.name.clone()))
        })
        .unwrap_or_else(|| format!("Zone {zone_index}"));

    let frames = sample
        .frames
        .min(sample.data_l.len())
        .min(sample.data_r.len());
    if frames == 0 {
        return None;
    }
    let mut samples = Vec::with_capacity(frames * 2);
    for frame in 0..frames {
        samples.push(sample.data_l[frame]);
        samples.push(sample.data_r[frame]);
    }

    Some(maolan_editor::app::AudioBuffer::new(
        name,
        Arc::new(samples),
        2,
        sample.sample_rate.max(1.0).round() as u32,
    ))
}

fn reload_zone_sample(state: &mut State, path: &Path) {
    let Some(index) = state.editing_zone_index else {
        return;
    };
    let Ok(sample) = crate::sampler::dsp::sample::load_audio(path) else {
        return;
    };
    state.shared.clear_edited_zone_edits(index);
    let mut patch = (*state.shared.patch.load()).clone();
    let mut zone_iter = patch
        .parts
        .iter_mut()
        .flat_map(|part| part.groups.iter_mut())
        .flat_map(|group| group.zones.iter_mut());
    let Some(zone) = zone_iter.nth(index) else {
        return;
    };
    zone.sample = sample.clone();
    zone.files = vec![path.to_path_buf()];
    let zones = crate::sampler::plugin::build_zones_from_patch(&patch);
    let groups = crate::sampler::plugin::build_groups_from_patch(&patch);
    state.shared.patch.store(Arc::new(patch));
    state.shared.zones.store(Arc::new(zones));
    state.shared.groups.store(Arc::new(groups));
    state.shared.bump_patch_version();
    state.shared.bump_zones_version();
    state.shared.request_audio_ports_rescan();
    state.shared.mark_dirty();
}

fn dispatch_editor_action_to_zone(
    state: &mut State,
    action: maolan_editor::app::AudioEditAction,
) -> bool {
    let Some(index) = state.editing_zone_index else {
        return false;
    };
    let mut edits = state
        .shared
        .edited_zone_edits
        .lock()
        .get(&index)
        .cloned()
        .unwrap_or_default();
    edits.actions.push(action);
    state.shared.set_edited_zone_edits(index, edits);
    true
}

fn sync_editing_zone_loop_from_editor(state: &mut State) {
    let (Some(editor), Some(index)) = (state.audio_editor.as_ref(), state.editing_zone_index)
    else {
        return;
    };
    let enabled = maolan_editor::app::loop_enabled(editor);
    let xfade = maolan_editor::app::xfade_samples(editor);
    let frames = patch_zone_sample(state, index)
        .map(|sample| sample.frames)
        .unwrap_or(0);
    let region = if enabled {
        maolan_editor::app::loop_region(editor)
            .map(|(start, end)| (start, end.min(frames)))
            .filter(|(start, end)| end > start)
    } else {
        None
    };
    let mut zones = state.shared.zones.load();
    let Some(zone) = Arc::make_mut(&mut zones).get_mut(index) else {
        return;
    };
    let loop_mode = if enabled {
        LoopMode::DuringVoice
    } else {
        LoopMode::Off
    };
    let changed = zone.loop_mode != loop_mode
        || zone.loop_crossfade != xfade
        || (enabled
            && region.is_some_and(|(start, end)| zone.loop_start != start || zone.loop_end != end));
    if !changed {
        return;
    }
    zone.loop_mode = loop_mode;
    zone.loop_crossfade = xfade;
    if let Some((start, end)) = region {
        zone.loop_start = start;
        zone.loop_end = end;
    }
    state.shared.zones.store(zones);
    state.shared.bump_zones_version();
    state.shared.mark_dirty();
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

fn clamp_u7(value: f32) -> u8 {
    value.round().clamp(0.0, 127.0) as u8
}

fn note_option_from_slider(value: f32) -> Option<u8> {
    let rounded = value.round() as i32;
    if (0..=127).contains(&rounded) {
        Some(rounded as u8)
    } else {
        None
    }
}

fn slider_value_from_note_option(note: Option<u8>) -> f32 {
    note.map(f32::from).unwrap_or(-1.0)
}

fn note_option_text(note: Option<u8>) -> String {
    match note {
        Some(n) => n.to_string(),
        None => String::from("off"),
    }
}

fn group_eg_params_mut(group: &mut SampleGroup, index: usize) -> &mut AdsrParams {
    let params = match index {
        2 => &mut group.eg2_params,
        _ => &mut group.eg1_params,
    };
    params.get_or_insert(AdsrParams::default())
}

fn group_lfo_params_mut(group: &mut SampleGroup, index: usize) -> &mut LfoParams {
    let params = match index {
        1 => &mut group.lfo1_params,
        2 => &mut group.lfo2_params,
        3 => &mut group.lfo3_params,
        _ => &mut group.lfo4_params,
    };
    params.get_or_insert(LfoParams::default())
}

fn group_filter_params_mut(group: &mut SampleGroup) -> &mut FilterParams {
    group.filter_params.get_or_insert(FilterParams::default())
}

fn apply_group_param_field(group: &mut SampleGroup, field: GroupParamField, value: f32) {
    match field {
        GroupParamField::Eg1Attack => {
            group_eg_params_mut(group, 1).attack = value.max(0.0);
        }
        GroupParamField::Eg1Decay => {
            group_eg_params_mut(group, 1).decay = value.max(0.0);
        }
        GroupParamField::Eg1Sustain => {
            group_eg_params_mut(group, 1).sustain = value.clamp(0.0, 1.0);
        }
        GroupParamField::Eg1Release => {
            group_eg_params_mut(group, 1).release = value.max(0.0);
        }
        GroupParamField::Eg2Attack => {
            group_eg_params_mut(group, 2).attack = value.max(0.0);
        }
        GroupParamField::Eg2Decay => {
            group_eg_params_mut(group, 2).decay = value.max(0.0);
        }
        GroupParamField::Eg2Sustain => {
            group_eg_params_mut(group, 2).sustain = value.clamp(0.0, 1.0);
        }
        GroupParamField::Eg2Release => {
            group_eg_params_mut(group, 2).release = value.max(0.0);
        }
        GroupParamField::Lfo1Rate => {
            let params = group_lfo_params_mut(group, 1);
            params.rate = value.max(0.0);
            params.enabled = true;
        }
        GroupParamField::Lfo1Amount => {
            let params = group_lfo_params_mut(group, 1);
            params.amount = value;
            params.enabled = true;
        }
        GroupParamField::Lfo2Rate => {
            let params = group_lfo_params_mut(group, 2);
            params.rate = value.max(0.0);
            params.enabled = true;
        }
        GroupParamField::Lfo2Amount => {
            let params = group_lfo_params_mut(group, 2);
            params.amount = value;
            params.enabled = true;
        }
        GroupParamField::Lfo3Rate => {
            let params = group_lfo_params_mut(group, 3);
            params.rate = value.max(0.0);
            params.enabled = true;
        }
        GroupParamField::Lfo3Amount => {
            let params = group_lfo_params_mut(group, 3);
            params.amount = value;
            params.enabled = true;
        }
        GroupParamField::Lfo4Rate => {
            let params = group_lfo_params_mut(group, 4);
            params.rate = value.max(0.0);
            params.enabled = true;
        }
        GroupParamField::Lfo4Amount => {
            let params = group_lfo_params_mut(group, 4);
            params.amount = value;
            params.enabled = true;
        }
        GroupParamField::FilterCutoff => {
            group_filter_params_mut(group).cutoff = value.max(20.0);
        }
        GroupParamField::FilterResonance => {
            group_filter_params_mut(group).resonance = value.clamp(0.0, 1.0);
        }
        GroupParamField::FilterKeyTrack => {
            group_filter_params_mut(group).key_tracking = (value / 100.0).clamp(0.0, 1.0);
        }
        GroupParamField::FilterVelTrack => {
            group_filter_params_mut(group).vel_tracking = value.clamp(-9600.0, 9600.0);
        }
    }
}

fn set_zone_fade_pair(pair: &mut Option<(u8, u8)>, width: &mut u8, low: f32, high: f32) {
    let low = clamp_u7(low);
    let high = clamp_u7(high.max(f32::from(low)));
    *pair = (high > low).then_some((low, high));
    *width = pair.map(|(low, high)| high - low).unwrap_or(0);
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
            zones_visible: true,
            browser_visible: true,
            zones_width: 220.0,
            browser_width: 138.0,
            resizing_side: None,
            resize_start: None,
            browser_path,
            browser_entries,
            dragged_audio_file: None,
            status_revision: 0,
            hovered_note: None,
            hovered_velocity: None,
            drag_y: None,
            dragging_zone_edge: None,
            dragging_zone_body: None,
            dragging_zone_list_item: None,
            hovered_zone_drop_group: None,
            editing_zone_name: None,
            selected: None,
            undo_stack: Vec::new(),
            editing_zone_index: None,
            editing_group_name: None,
            renaming_group: None,
            selected_group_eg: 0,
            selected_group_lfo: 0,
            audio_editor: None,
            audio_editor_path: None,
            extra_sfz_opcode_text: String::new(),
            piano_active_note: None,
            sampler_audition: None,
            next_sampler_audition_id: 1,
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
        kind: BrowserEntryKind::Directory,
    });

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().map(|ty| ty.is_dir()).unwrap_or(false);
            let is_audio = is_audio_file(&entry_path);
            let is_instrument = is_instrument_file(&entry_path);
            let browser_entry = BrowserEntry {
                name,
                path: entry_path,
                kind: if is_dir {
                    BrowserEntryKind::Directory
                } else if is_instrument {
                    BrowserEntryKind::Instrument
                } else {
                    BrowserEntryKind::Audio
                },
            };
            if is_dir {
                dirs.push(browser_entry);
            } else if is_audio || is_instrument {
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

fn is_instrument_file(path: &Path) -> bool {
    detect_format(path).is_some()
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
    let Ok(format) = symphonia::default::get_probe().probe(&hint, mss, format_opts, metadata_opts)
    else {
        return false;
    };
    format
        .tracks()
        .iter()
        .find(|t| {
            t.codec_params
                .as_ref()
                .and_then(|p| p.audio())
                .is_some_and(|a| a.codec != CODEC_ID_NULL_AUDIO)
        })
        .or_else(|| format.tracks().first())
        .and_then(|track| track.codec_params.as_ref())
        .and_then(|params| params.audio())
        .and_then(|audio| audio.channels.clone())
        .map(|channels| {
            let count = channels.count();
            count == 1 || count == 2
        })
        .unwrap_or(false)
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    'task: {
        match message {
            Message::None => {}
            Message::SetParam(id, value) => {
                let idx = id.as_index();
                if !state.active_gestures[idx] {
                    state.active_gestures[idx] = true;
                    state.shared.mark_gesture_begin_pending(id);
                }
                state.shared.set_param_outbound_only(id, value as f64);
            }
            Message::SetPitchBendRange(low, high) => {
                let low = low.clamp(0.0, 12000.0);
                let high = high.clamp(0.0, 12000.0);
                for (id, value) in [(ParamId::PitchBendDown, low), (ParamId::PitchBendUp, high)] {
                    let idx = id.as_index();
                    if !state.active_gestures[idx] {
                        state.active_gestures[idx] = true;
                        state.shared.mark_gesture_begin_pending(id);
                    }
                    state.shared.set_param_outbound_only(id, value as f64);
                }
                for id in [ParamId::PitchBendDown, ParamId::PitchBendUp] {
                    let idx = id.as_index();
                    if state.active_gestures[idx] {
                        state.active_gestures[idx] = false;
                        state.shared.mark_gesture_end_pending(id);
                    }
                }
            }
            Message::ReleaseParam(id) => {
                let idx = id.as_index();
                if state.active_gestures[idx] {
                    state.active_gestures[idx] = false;
                    state.shared.mark_gesture_end_pending(id);
                }
            }
            Message::ResetParam(id) => {
                set_param_once(state, id, PARAMS[id.as_index()].default as f32);
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
            Message::ToggleZonesPanel => {
                state.zones_visible = !state.zones_visible;
                if !state.zones_visible && state.resizing_side == Some(SidePanel::Zones) {
                    state.resizing_side = None;
                    state.resize_start = None;
                }
            }
            Message::ToggleBrowserPanel => {
                state.browser_visible = !state.browser_visible;
                if !state.browser_visible && state.resizing_side == Some(SidePanel::Browser) {
                    state.resizing_side = None;
                    state.resize_start = None;
                }
            }
            Message::StartSideResize(side) => {
                state.resizing_side = Some(side);
                state.resize_start = None;
            }
            Message::ResizeSidePanel(x) => {
                if let Some(side) = state.resizing_side {
                    let (start_x, start_width) = match state.resize_start {
                        Some(start) => start,
                        None => {
                            let width = match side {
                                SidePanel::Zones => state.zones_width,
                                SidePanel::Browser => state.browser_width,
                            };
                            state.resize_start = Some((x, width));
                            (x, width)
                        }
                    };
                    let offset = x - start_x;
                    match side {
                        SidePanel::Zones => {
                            state.zones_width = (start_width + offset).max(220.0);
                        }
                        SidePanel::Browser => {
                            state.browser_width = (start_width - offset).max(92.0);
                        }
                    }
                }
            }
            Message::StopSideResize => {
                state.resizing_side = None;
                state.resize_start = None;
            }
            Message::OpenBrowserEntry(path) => {
                if path.is_dir() {
                    state.browser_path = path;
                    state.browser_entries = read_browser_entries(&state.browser_path);
                }
            }
            Message::LoadInstrument(path) => {
                Arc::clone(&state.shared).load_file(path);
            }
            Message::ImportInstrument(path) => {
                Arc::clone(&state.shared).import_file(path);
            }
            Message::PickInstrumentFile => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Sampler instruments", &["sfz", "sf2", "ariax"])
                    .pick_file()
                {
                    Arc::clone(&state.shared).load_file(path);
                }
            }
            Message::PickImportFile => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Sampler instruments", &["sfz", "sf2", "ariax"])
                    .pick_file()
                {
                    Arc::clone(&state.shared).import_file(path);
                }
            }
            Message::ReloadInstrument => {
                Arc::clone(&state.shared).reload_file();
            }
            Message::ExportSfz => {
                let default_path = default_export_path(state);
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("SFZ instrument", &["sfz"])
                    .set_file_name(
                        default_path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("sampler.sfz"),
                    )
                    .save_file()
                {
                    let export_patch = export_patch_from_state(state);
                    let result = export_patch_to_sfz(&path, &export_patch);
                    let mut log = state.shared.load_log.lock();
                    match result {
                        Ok(()) => log.push(format!("Exported {}", path.display())),
                        Err(error) => log.push(format!("Export failed: {error}")),
                    }
                }
            }
            Message::SelectSf2Preset(preset) => {
                let presets = state.shared.sf2_presets.lock();
                if let Some(index) = presets.iter().position(|candidate| candidate == &preset)
                    && let Some(path) = state.shared.instrument_path.lock().clone()
                {
                    drop(presets);
                    Arc::clone(&state.shared).load_file_with_preset(path, Some(index));
                }
            }
            Message::PollLoadStatus => {
                state.status_revision = state.status_revision.wrapping_add(1);
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
                if let Some((zone_index, edge)) = state.dragging_zone_edge
                    && let Some(zone) = Arc::make_mut(&mut zones).get_mut(zone_index)
                {
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
                if let Some((zone_index, note_offset, velocity_offset)) = state.dragging_zone_body
                    && let Some(zone) = Arc::make_mut(&mut zones).get_mut(zone_index)
                {
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
                if changed {
                    state.shared.zones.store(zones);
                    state.shared.bump_zones_version();
                    state.shared.note_names_changed();
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
                        let mut groups = state.shared.groups.load();
                        let group = group_for_new_zone(state, &groups, &zones);
                        if !groups.iter().any(|candidate| candidate.name == group) {
                            Arc::make_mut(&mut groups).push(SampleGroup::new(group.clone()));
                            state.shared.groups.store(groups);
                            state.shared.request_audio_ports_rescan();
                        }
                        Arc::make_mut(&mut zones).push(SampleZone::new_basic(
                            name,
                            vec![file],
                            start_note,
                            end_note,
                            vel_low,
                            vel_high,
                            group,
                        ));
                    }
                    state.shared.zones.store(zones);
                    state.shared.bump_zones_version();
                    state.shared.note_names_changed();
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
            Message::CreateZoneListItem(kind) => {
                let mut zones = state.shared.zones.load();
                state.undo_stack.push(zones.to_vec());
                let mut groups = state.shared.groups.load();
                match kind {
                    ZoneCreateKind::Group => {
                        let group = unique_group_name(&groups, &zones, "New Group");
                        Arc::make_mut(&mut groups).push(SampleGroup::new(group.clone()));
                        state.selected = Some(ZoneListSelection::Group(group));
                        state.shared.groups.store(groups);
                        state.shared.request_audio_ports_rescan();
                    }
                    ZoneCreateKind::Zone => {
                        let group = group_for_new_zone(state, &groups, &zones);
                        let new_zone = default_new_zone(&zones, group.clone());
                        if !groups.iter().any(|candidate| candidate.name == group) {
                            Arc::make_mut(&mut groups).push(SampleGroup::new(group));
                            state.shared.groups.store(groups);
                            state.shared.request_audio_ports_rescan();
                        }
                        let zones_arc = Arc::make_mut(&mut zones);
                        zones_arc.push(new_zone);
                        let new_index = zones_arc.len() - 1;
                        state.selected = Some(ZoneListSelection::Zone(new_index));
                        state.shared.zones.store(zones);
                    }
                }
                state.shared.bump_zones_version();
                state.shared.note_names_changed();
                state.shared.mark_dirty();
            }
            Message::BeginZoneListDrag(index) => {
                state.selected = Some(ZoneListSelection::Zone(index));
                state.dragging_zone_list_item = Some(index);
                state.hovered_zone_drop_group = None;
            }
            Message::HoverZoneDropGroup(group) => {
                if state.dragging_zone_list_item.is_some() {
                    state.hovered_zone_drop_group = group;
                }
            }
            Message::FinishZoneListDrag => {
                if let Some(index) = state.dragging_zone_list_item.take()
                    && let Some(group) = state.hovered_zone_drop_group.take()
                {
                    let mut zones = state.shared.zones.load();
                    if zones.get(index).is_some_and(|zone| zone.group != group) {
                        state.undo_stack.push(zones.to_vec());
                        if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index) {
                            zone.group = group;
                        }
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.note_names_changed();
                        state.shared.mark_dirty();
                    }
                }
                state.hovered_zone_drop_group = None;
            }
            Message::DeselectZone => {
                state.selected = None;
            }
            Message::SelectZoneListItem(selection) => {
                match &selection {
                    ZoneListSelection::Group(group_name) => {
                        let groups = state.shared.groups.load();
                        state.extra_sfz_opcode_text = groups
                            .iter()
                            .find(|group| group.name == *group_name)
                            .map(|group| format_sfz_opcode_text(&group.extra_sfz_opcodes))
                            .unwrap_or_default();
                    }
                    ZoneListSelection::Zone(index) => {
                        let zones = state.shared.zones.load();
                        state.extra_sfz_opcode_text = zones
                            .get(*index)
                            .map(|zone| format_sfz_opcode_text(&zone.extra_sfz_opcodes))
                            .unwrap_or_default();
                    }
                }
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
                            let mut groups = state.shared.groups.load();
                            Arc::make_mut(&mut groups).retain(|group| group.name != group_name);
                            state.shared.groups.store(groups);
                            state.shared.request_audio_ports_rescan();
                            zones_arc.retain(|zone| zone.group != group_name);
                        }
                    }
                    state.shared.zones.store(zones);
                    state.shared.bump_zones_version();
                    state.shared.note_names_changed();
                    state.shared.mark_dirty();
                }
            }
            Message::Undo => {
                if let Some(previous_zones) = state.undo_stack.pop() {
                    state.shared.zones.store(Arc::new(previous_zones));
                    state.shared.bump_zones_version();
                    state.shared.note_names_changed();
                    state.shared.mark_dirty();
                    state.selected = None;
                }
            }
            Message::OpenSamplerEditor(index) => {
                state.dragging_zone_edge = None;
                state.dragging_zone_body = None;
                state.editing_zone_index = Some(index);
                state.selected = Some(ZoneListSelection::Zone(index));
                let zones = state.shared.zones.load();
                state.extra_sfz_opcode_text = zones
                    .get(index)
                    .map(|zone| format_sfz_opcode_text(&zone.extra_sfz_opcodes))
                    .unwrap_or_default();
                let mut editor = maolan_editor::app::EditApp::default();
                let audio = editor_audio_buffer_for_zone(state, index);
                let open_task = if let Some(audio) = audio {
                    maolan_editor::app::open_audio(&mut editor, audio)
                } else {
                    Task::none()
                };
                let zone_loop = zones.get(index).and_then(|zone| {
                    (zone.loop_mode != LoopMode::Off && zone.loop_end > zone.loop_start)
                        .then_some((zone.loop_start, zone.loop_end))
                });
                maolan_editor::app::set_embedded_loop(&mut editor, zone_loop.is_some(), zone_loop);
                maolan_editor::app::set_embedded_xfade(
                    &mut editor,
                    zones
                        .get(index)
                        .map(|zone| zone.loop_crossfade)
                        .unwrap_or(0),
                );
                state.audio_editor = Some(editor);
                state.audio_editor_path = zones
                    .get(index)
                    .and_then(|zone| zone.files.first().cloned());
                return open_task.map(|msg| Message::AudioEditor(Box::new(msg)));
            }
            Message::CloseSamplerEditor => {
                state.sampler_audition = None;
                state.next_sampler_audition_id =
                    state.next_sampler_audition_id.wrapping_add(1).max(1);
                if let Some(note) = state.piano_active_note.take() {
                    state.shared.send_note_off(note);
                }
                state.editing_zone_index = None;
                state.audio_editor = None;
                state.audio_editor_path = None;
                state.extra_sfz_opcode_text.clear();
            }
            Message::OpenGroupEditor(name) => {
                state.dragging_zone_list_item = None;
                state.hovered_zone_drop_group = None;
                let groups = state.shared.groups.load();
                state.extra_sfz_opcode_text = groups
                    .iter()
                    .find(|group| group.name == name)
                    .map(|group| format_sfz_opcode_text(&group.extra_sfz_opcodes))
                    .unwrap_or_default();
                state.selected = Some(ZoneListSelection::Group(name.clone()));
                state.editing_group_name = Some(name);
            }
            Message::CloseGroupEditor => {
                state.editing_group_name = None;
                state.renaming_group = None;
                state.extra_sfz_opcode_text.clear();
            }
            Message::SelectGroupEg(index) => {
                state.selected_group_eg = index;
            }
            Message::SelectGroupLfo(index) => {
                state.selected_group_lfo = index;
            }
            Message::SetSelectedGroupSwLastRange(low, high) => {
                if let Some(ZoneListSelection::Group(group_name)) = state.selected.as_ref() {
                    let mut groups = state.shared.groups.load();
                    if let Some(group) = Arc::make_mut(&mut groups)
                        .iter_mut()
                        .find(|group| group.name == *group_name)
                    {
                        let low = low.round().clamp(0.0, 127.0) as u8;
                        let high = high.round().clamp(0.0, 127.0) as u8;
                        if low == high {
                            group.sw_lolast = None;
                            group.sw_hilast = None;
                        } else {
                            group.sw_lolast = Some(low.min(high));
                            group.sw_hilast = Some(low.max(high));
                        }
                        state.shared.groups.store(groups);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::StartRenameGroup => {
                if let Some(name) = state.editing_group_name.clone() {
                    state.renaming_group = Some(name);
                }
            }
            Message::UpdateRenameGroupText(text) => {
                if let Some(current) = state.renaming_group.as_mut() {
                    *current = text;
                }
            }
            Message::FinishRenameGroup => {
                let Some(new_name) = state
                    .renaming_group
                    .take()
                    .map(|text| text.trim().to_string())
                else {
                    return Task::none();
                };
                let Some(old_name) = state.editing_group_name.clone() else {
                    return Task::none();
                };
                if new_name.is_empty() || new_name == old_name {
                    return Task::none();
                }
                let mut groups = state.shared.groups.load();
                if groups.iter().any(|group| group.name == new_name) {
                    return Task::none();
                }
                if let Some(group) = Arc::make_mut(&mut groups)
                    .iter_mut()
                    .find(|group| group.name == old_name)
                {
                    group.name = new_name.clone();
                }
                state.shared.groups.store(groups);
                let mut zones = state.shared.zones.load();
                for zone in Arc::make_mut(&mut zones).iter_mut() {
                    if zone.group == old_name {
                        zone.group = new_name.clone();
                    }
                }
                state.shared.zones.store(zones);
                state.shared.bump_zones_version();
                state.shared.note_names_changed();
                state.shared.request_audio_ports_rescan();
                state.shared.mark_dirty();
                state.editing_group_name = Some(new_name);
            }
            Message::AudioEditor(msg) => {
                let msg = *msg;
                if matches!(
                    msg,
                    maolan_editor::app::Message::Save | maolan_editor::app::Message::SaveAs
                ) {
                    break 'task Task::none();
                }
                if let Some(editor) = state.audio_editor.as_ref()
                    && let Some(action) =
                        maolan_editor::app::audio_edit_action_for_message(editor, &msg)
                {
                    state.shared.mark_dirty();
                    dispatch_editor_action_to_zone(state, action);
                    break 'task Task::none();
                }
                let edits_document = maolan_editor::app::message_edits_document(&msg);
                if edits_document {
                    state.shared.mark_dirty();
                }
                match msg {
                    maolan_editor::app::Message::Play => {
                        let loop_enabled = state
                            .audio_editor
                            .as_ref()
                            .is_some_and(maolan_editor::app::loop_enabled);
                        let editor_task = state
                            .audio_editor
                            .as_mut()
                            .map(|editor| {
                                maolan_editor::app::update(
                                    editor,
                                    maolan_editor::app::Message::Play,
                                )
                                .map(|editor_msg| Message::AudioEditor(Box::new(editor_msg)))
                            })
                            .unwrap_or_else(Task::none);
                        if let Some(index) = state.editing_zone_index {
                            let zones = state.shared.zones.load();
                            if let Some(zone) = zones.get(index) {
                                let (note, velocity) = zone_audition_note(zone);
                                let duration = zone_audition_duration(state, index);
                                let audition_id = state.next_sampler_audition_id;
                                state.next_sampler_audition_id =
                                    state.next_sampler_audition_id.wrapping_add(1).max(1);
                                if let Some(prev) = state.piano_active_note.replace(note) {
                                    state.shared.send_note_off(prev);
                                }
                                state.sampler_audition = Some((audition_id, note));
                                state.shared.send_note_on(note, velocity);
                                if let Some(duration) = duration.filter(|_| !loop_enabled) {
                                    break 'task Task::perform(
                                        finish_sampler_audition_after(duration, audition_id, note),
                                        |(id, note)| Message::SamplerAuditionFinished(id, note),
                                    )
                                    .chain(editor_task);
                                }
                            }
                        }
                        break 'task editor_task;
                    }
                    maolan_editor::app::Message::Stop => {
                        state.sampler_audition = None;
                        state.next_sampler_audition_id =
                            state.next_sampler_audition_id.wrapping_add(1).max(1);
                        if let Some(note) = state.piano_active_note.take() {
                            state.shared.send_note_off(note);
                        }
                        let editor_task = state
                            .audio_editor
                            .as_mut()
                            .map(|editor| {
                                maolan_editor::app::update(
                                    editor,
                                    maolan_editor::app::Message::Stop,
                                )
                                .map(|editor_msg| Message::AudioEditor(Box::new(editor_msg)))
                            })
                            .unwrap_or_else(Task::none);
                        break 'task editor_task;
                    }
                    _ => {}
                }
                let sync_loop_after = matches!(
                    msg,
                    maolan_editor::app::Message::ToggleLoop
                        | maolan_editor::app::Message::SelectionFinish(_)
                        | maolan_editor::app::Message::SelectionResize(_)
                        | maolan_editor::app::Message::SelectionXfade(..)
                        | maolan_editor::app::Message::SelectMarkerRegion(_)
                );
                let Some(editor) = state.audio_editor.as_mut() else {
                    break 'task Task::none();
                };
                let task = match msg {
                    maolan_editor::app::Message::DocumentSaved(Ok(path)) => {
                        let path = path.clone();
                        let task = maolan_editor::app::update(
                            editor,
                            maolan_editor::app::Message::DocumentSaved(Ok(path.clone())),
                        )
                        .map(|editor_msg| Message::AudioEditor(Box::new(editor_msg)));
                        reload_zone_sample(state, &path);
                        task
                    }
                    _ => maolan_editor::app::update(editor, msg)
                        .map(|editor_msg| Message::AudioEditor(Box::new(editor_msg))),
                };
                if sync_loop_after {
                    sync_editing_zone_loop_from_editor(state);
                }
                break 'task task;
            }
            Message::SetEditingZoneValue(field, value) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index) {
                        match field {
                            ZoneEditField::RootKey => {
                                zone.root_key = (value.round() as u8).min(127);
                            }
                            ZoneEditField::GainDb => {
                                zone.gain_db = value.clamp(-96.0, 24.0);
                            }
                            ZoneEditField::Pan => {
                                zone.pan = (value / 100.0).clamp(-1.0, 1.0);
                            }
                            ZoneEditField::Width => {
                                zone.width = (value / 100.0).clamp(0.0, 2.0);
                            }
                            ZoneEditField::Position => {
                                zone.position = (value / 100.0).clamp(-1.0, 1.0);
                            }
                            ZoneEditField::AmpKeytrack => {
                                zone.amp_keytrack_db = value.clamp(-12.0, 12.0);
                            }
                            ZoneEditField::PitchOffset => {
                                zone.pitch_offset = value.clamp(-1200.0, 1200.0);
                            }
                            ZoneEditField::KeyTracking => {
                                zone.key_tracking = (value / 100.0).clamp(0.0, 2.0);
                            }
                            ZoneEditField::StartOffset => {
                                zone.start_offset = value.max(0.0).round() as usize;
                            }
                            ZoneEditField::OffsetRandom => {
                                zone.offset_random = value.max(0.0).round() as usize;
                            }
                            ZoneEditField::EndOffset => {
                                zone.end_offset = value.max(0.0).round() as usize;
                            }
                            ZoneEditField::SeqLength => {
                                zone.seq_length = value.max(0.0).round() as u32;
                            }
                            ZoneEditField::SeqPosition => {
                                zone.seq_position = value.max(0.0).round() as u32;
                            }
                            ZoneEditField::OffBy => {
                                zone.off_by = clamp_u7(value);
                            }
                            ZoneEditField::Output => {
                                zone.output = value.round().clamp(
                                    0.0,
                                    (crate::sampler::plugin::MAX_SAMPLER_BUSES - 1) as f32,
                                ) as u16;
                                state.shared.request_audio_ports_rescan();
                            }
                            ZoneEditField::AmpVeltrack => {
                                zone.amp_veltrack = value.clamp(-100.0, 100.0);
                            }
                            ZoneEditField::Count => {
                                zone.count = value.max(0.0).round() as u32;
                            }
                        }
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetEditingZoneRange(field, low, high) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index) {
                        match field {
                            ZoneRangeField::Velocity => {
                                zone.vel_low = low.round().clamp(0.0, 127.0) as u8;
                                zone.vel_high =
                                    high.round().clamp(f32::from(zone.vel_low), 127.0) as u8;
                            }
                            ZoneRangeField::KeyFadeIn => {
                                set_zone_fade_pair(
                                    &mut zone.key_fade_in,
                                    &mut zone.key_fade_low,
                                    low,
                                    high,
                                );
                            }
                            ZoneRangeField::KeyFadeOut => {
                                set_zone_fade_pair(
                                    &mut zone.key_fade_out,
                                    &mut zone.key_fade_high,
                                    low,
                                    high,
                                );
                            }
                            ZoneRangeField::VelFadeIn => {
                                set_zone_fade_pair(
                                    &mut zone.vel_fade_in,
                                    &mut zone.vel_fade_low,
                                    low,
                                    high,
                                );
                            }
                            ZoneRangeField::VelFadeOut => {
                                set_zone_fade_pair(
                                    &mut zone.vel_fade_out,
                                    &mut zone.vel_fade_high,
                                    low,
                                    high,
                                );
                            }
                            ZoneRangeField::PitchBend => {
                                zone.pitch_bend_down = low.clamp(0.0, 12000.0);
                                zone.pitch_bend_up = high.clamp(0.0, 12000.0);
                            }
                            ZoneRangeField::Channel => {
                                zone.channel_low = low.round().clamp(1.0, 16.0) as u8;
                                zone.channel_high =
                                    high.round().clamp(f32::from(zone.channel_low), 16.0) as u8;
                            }
                            ZoneRangeField::PitchBendRange => {
                                zone.pitch_bend_low = low.round().clamp(-8192.0, 8192.0) as i16;
                                zone.pitch_bend_high =
                                    high.round().clamp(f32::from(zone.pitch_bend_low), 8192.0)
                                        as i16;
                            }
                            ZoneRangeField::Random => {
                                zone.random_low = low.clamp(0.0, 1.0);
                                zone.random_high = high.clamp(zone.random_low, 1.0);
                            }
                        }
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.note_names_changed();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::ToggleEditingZoneReverse(reverse) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index) {
                        zone.reverse = reverse;
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetEditingZonePlayMode(mode) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index) {
                        zone.play_mode = mode.mode();
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetEditingZoneVelocityCurve(curve) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index) {
                        zone.velocity_curve = curve.curve();
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetEditingZoneOffMode(mode) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index) {
                        zone.off_mode = mode.mode();
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::AddEditingZoneCcRoute => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index)
                        && let Some(route) = zone
                            .mod_matrix
                            .routes
                            .iter_mut()
                            .find(|route| !route.active)
                    {
                        *route = ModRoute {
                            source: ModSource::MidiCc,
                            source_cc: 1,
                            target: ModTarget::Amplitude,
                            depth: 0.0,
                            active: true,
                            ..ModRoute::default()
                        };
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::RemoveEditingZoneCcRoute(route_index) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index)
                        && let Some(route) = zone.mod_matrix.routes.get_mut(route_index)
                    {
                        *route = ModRoute::default();
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetEditingZoneCcRouteSource(route_index, source) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index)
                        && let Some(route) = zone.mod_matrix.routes.get_mut(route_index)
                    {
                        route.source = source;
                        route.active = route.target != ModTarget::None;
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetEditingZoneCcRouteCc(route_index, value) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index)
                        && let Some(route) = zone.mod_matrix.routes.get_mut(route_index)
                    {
                        route.source = ModSource::MidiCc;
                        route.source_cc = clamp_u7(value);
                        route.active = route.target != ModTarget::None;
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetEditingZoneCcRouteTarget(route_index, target) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index)
                        && let Some(route) = zone.mod_matrix.routes.get_mut(route_index)
                    {
                        route.source = ModSource::MidiCc;
                        route.target = target.target();
                        route.active = true;
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetEditingZoneCcRouteDepth(route_index, value) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index)
                        && let Some(route) = zone.mod_matrix.routes.get_mut(route_index)
                    {
                        route.source = ModSource::MidiCc;
                        route.depth = cc_mod_depth_from_amount(route.target, value);
                        route.active = route.target != ModTarget::None;
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetEditingZoneCcRouteCurve(route_index, curve) => {
                if curve != CcCurveOption::Custom
                    && let Some(index) = state.editing_zone_index
                {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index)
                        && let Some(route) = zone.mod_matrix.routes.get_mut(route_index)
                    {
                        route.source_curve = curve.curve();
                        route.source = ModSource::MidiCc;
                        route.active = route.target != ModTarget::None;
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::AddEditingZoneCcCondition => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index) {
                        let cc = (0..=127u8)
                            .find(|cc| {
                                !zone
                                    .cc_conditions
                                    .iter()
                                    .any(|condition| condition.cc == *cc)
                            })
                            .unwrap_or(0);
                        zone.cc_conditions.push(CcCondition {
                            cc,
                            low: 1,
                            high: 127,
                        });
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::RemoveEditingZoneCcCondition(condition_index) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index)
                        && condition_index < zone.cc_conditions.len()
                    {
                        zone.cc_conditions.remove(condition_index);
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetEditingZoneCcConditionCc(condition_index, value) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index)
                        && let Some(condition) = zone.cc_conditions.get_mut(condition_index)
                    {
                        condition.cc = clamp_u7(value);
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetEditingZoneCcConditionRange(condition_index, low, high) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index)
                        && let Some(condition) = zone.cc_conditions.get_mut(condition_index)
                    {
                        condition.low = clamp_u7(low);
                        condition.high = clamp_u7(high.max(f32::from(condition.low)));
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetSelectedGroupValue(field, value) => {
                if let Some(ZoneListSelection::Group(group_name)) = state.selected.as_ref() {
                    let mut groups = state.shared.groups.load();
                    if let Some(group) = Arc::make_mut(&mut groups)
                        .iter_mut()
                        .find(|group| group.name == *group_name)
                    {
                        match field {
                            GroupEditField::PolyLimit => {
                                group.poly_limit = value.max(0.0).round() as usize;
                            }
                            GroupEditField::ExclusiveGroup => {
                                group.exclusive_group = value.round() as u8;
                            }
                            GroupEditField::GainDb => {
                                group.gain_db = value.clamp(-96.0, 24.0);
                            }
                            GroupEditField::Pan => {
                                group.pan = (value / 100.0).clamp(-1.0, 1.0);
                            }
                            GroupEditField::Output => {
                                group.output = value.round().clamp(
                                    0.0,
                                    (crate::sampler::plugin::MAX_SAMPLER_BUSES - 1) as f32,
                                ) as u16;
                                state.shared.request_audio_ports_rescan();
                            }
                            GroupEditField::SwLast => {
                                group.sw_last = note_option_from_slider(value);
                            }
                            GroupEditField::SwDown => {
                                group.sw_down = note_option_from_slider(value);
                            }
                            GroupEditField::SwUp => {
                                group.sw_up = note_option_from_slider(value);
                            }
                            GroupEditField::SwPrevious => {
                                group.sw_previous = note_option_from_slider(value);
                            }
                            GroupEditField::SwDefault => {
                                group.sw_default = note_option_from_slider(value);
                            }
                        }
                        state.shared.groups.store(groups);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetSelectedGroupParam(field, value) => {
                if let Some(ZoneListSelection::Group(group_name)) = state.selected.as_ref() {
                    let mut groups = state.shared.groups.load();
                    if let Some(group) = Arc::make_mut(&mut groups)
                        .iter_mut()
                        .find(|group| group.name == *group_name)
                    {
                        apply_group_param_field(group, field, value);
                        state.shared.groups.store(groups);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetSelectedGroupLfoShape(index, shape) => {
                if let Some(ZoneListSelection::Group(group_name)) = state.selected.as_ref() {
                    let mut groups = state.shared.groups.load();
                    if let Some(group) = Arc::make_mut(&mut groups)
                        .iter_mut()
                        .find(|group| group.name == *group_name)
                    {
                        let params = group_lfo_params_mut(group, index);
                        params.shape = shape;
                        params.enabled = true;
                        state.shared.groups.store(groups);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetSelectedGroupFilterType(filter_type) => {
                if let Some(ZoneListSelection::Group(group_name)) = state.selected.as_ref() {
                    let mut groups = state.shared.groups.load();
                    if let Some(group) = Arc::make_mut(&mut groups)
                        .iter_mut()
                        .find(|group| group.name == *group_name)
                    {
                        let params = group.filter_params.get_or_insert(FilterParams::default());
                        params.filter_type = filter_type;
                        params.enabled = true;
                        state.shared.groups.store(groups);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetSelectedGroupFilterSubtype(subtype) => {
                if let Some(ZoneListSelection::Group(group_name)) = state.selected.as_ref() {
                    let mut groups = state.shared.groups.load();
                    if let Some(group) = Arc::make_mut(&mut groups)
                        .iter_mut()
                        .find(|group| group.name == *group_name)
                    {
                        let params = group.filter_params.get_or_insert(FilterParams::default());
                        params.subtype = subtype;
                        params.enabled = true;
                        state.shared.groups.store(groups);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetSelectedGroupSwLabel(text) => {
                state.extra_sfz_opcode_text = text.clone();
                if let Some(ZoneListSelection::Group(group_name)) = state.selected.as_ref() {
                    let mut groups = state.shared.groups.load();
                    if let Some(group) = Arc::make_mut(&mut groups)
                        .iter_mut()
                        .find(|group| group.name == *group_name)
                    {
                        group.sw_label = if text.is_empty() { None } else { Some(text) };
                        state.shared.groups.store(groups);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetEditingZoneExtraSfz(text) => {
                state.extra_sfz_opcode_text = text;
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index) {
                        zone.extra_sfz_opcodes =
                            parse_sfz_opcode_text(&state.extra_sfz_opcode_text);
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetSelectedGroupExtraSfz(text) => {
                state.extra_sfz_opcode_text = text;
                if let Some(ZoneListSelection::Group(group_name)) = state.selected.as_ref() {
                    let mut groups = state.shared.groups.load();
                    if let Some(group) = Arc::make_mut(&mut groups)
                        .iter_mut()
                        .find(|group| group.name == *group_name)
                    {
                        group.extra_sfz_opcodes =
                            parse_sfz_opcode_text(&state.extra_sfz_opcode_text);
                        state.shared.groups.store(groups);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::PianoKeyPressed(note, velocity) => {
                state.sampler_audition = None;
                state.next_sampler_audition_id =
                    state.next_sampler_audition_id.wrapping_add(1).max(1);
                if let Some(prev) = state.piano_active_note.replace(note) {
                    state.shared.send_note_off(prev);
                }
                state.shared.send_note_on(note, velocity);
            }
            Message::PianoKeyReleased(note) => {
                state.sampler_audition = None;
                state.piano_active_note = None;
                state.shared.send_note_off(note);
            }
            Message::SamplerAuditionFinished(id, note) => {
                if state.sampler_audition == Some((id, note)) {
                    state.sampler_audition = None;
                    if state.piano_active_note == Some(note) {
                        state.piano_active_note = None;
                        state.shared.send_note_off(note);
                    }
                }
            }
            Message::PointerReleased => {
                state.resizing_side = None;
                state.resize_start = None;
                return update(state, Message::FinishZoneListDrag);
            }
            Message::AddOutput => {
                state.shared.add_output();
            }
            Message::CleanUpOutputs => {
                state.shared.clean_up_outputs();
            }
            Message::RemoveOutput(index) => {
                state.shared.remove_output(index);
            }
            Message::RenameOutput(index, name) => {
                state.shared.rename_output(index, name);
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

fn small_knob<'a>(
    id: ParamId,
    label: &'a str,
    state: &'a State,
    step: f32,
) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let def = &PARAMS[id.as_index()];
    let assigned = param_has_lfo_assignment(state, id);
    let value_text = if def.step >= 1.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    };
    let content = maolan_widgets::knob::knob(
        Knob {
            label: label.to_string(),
            value,
            range: def.min as f32..=def.max as f32,
            default: def.default as f32,
            step,
            value_text,
            assigned,
        },
        move |value| Message::SetParam(id, value),
        Message::ReleaseParam(id),
    );

    if mod_target_for_param(id).is_some() {
        mouse_area(content)
            .on_press(Message::AssignLfoToParam(id))
            .into()
    } else {
        content
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
    .width(Length::Shrink)
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

fn adsr_panel<'a>(state: &'a State, ids: [ParamId; 4]) -> Element<'a, Message> {
    let values = ids.map(|id| state.shared.params.get(id) as f32);
    let ranges = ids.map(|id| {
        let def = &PARAMS[id.as_index()];
        (def.min as f32, def.max as f32)
    });
    let steps = ids.map(|id| PARAMS[id.as_index()].step as f32);
    adsr_envelope(
        values,
        ranges,
        steps,
        ids.map(|id| param_has_lfo_assignment(state, id)),
    )
    .map(move |message| {
        let index = match &message {
            AdsrMessage::Change(i, _)
            | AdsrMessage::Release(i)
            | AdsrMessage::Reset(i)
            | AdsrMessage::Assign(i)
            | AdsrMessage::Edit(i)
            | AdsrMessage::EditChanged(i, _)
            | AdsrMessage::EditSubmit(i) => *i,
        };
        let id = ids[index];
        match message {
            AdsrMessage::Change(_, value) => Message::SetParam(id, value),
            AdsrMessage::Release(_) => Message::ReleaseParam(id),
            AdsrMessage::Reset(_) => Message::ResetParam(id),
            AdsrMessage::Assign(_) => Message::AssignLfoToParam(id),
            AdsrMessage::Edit(_) | AdsrMessage::EditChanged(_, _) | AdsrMessage::EditSubmit(_) => {
                Message::None
            }
        }
    })
}

/// A horizontal multi-toggler over a fixed option list. `selected` is the
/// index of the current option; picking a segment publishes `on_pick(option)`.
fn option_toggler<'a, T: Copy + 'a>(
    options: Vec<T>,
    selected: usize,
    labels: Vec<String>,
    on_pick: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    horizontal_multi_toggler(labels, selected, move |index| on_pick(options[index])).into()
}

/// A multi-toggler with a caption above it and the selected-option text
/// below the track, matching the knob field layout.
fn labeled_toggler<'a, T: Copy + PartialEq + std::fmt::Display + 'a>(
    label: &'static str,
    options: Vec<T>,
    selected: T,
    on_pick: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    let index = options
        .iter()
        .position(|option| *option == selected)
        .unwrap_or(0);
    let labels: Vec<String> = options.iter().map(|option| format!("{option}")).collect();
    let toggler = horizontal_multi_toggler(labels, index, move |index| on_pick(options[index]))
        .label_position(LabelPosition::Below);
    column![text(label).size(11), toggler].spacing(2).into()
}

fn filter_selector_dropdown<'a>(state: &'a State) -> Element<'a, Message> {
    container(
        pick_list(
            ["Filter 1", "Filter 2"].map(String::from).to_vec(),
            Some(
                if state.selected_filter == 0 {
                    "Filter 1"
                } else {
                    "Filter 2"
                }
                .to_string(),
            ),
            |name| Message::SelectFilter(if name == "Filter 2" { 1 } else { 0 }),
        )
        .width(Length::Shrink),
    )
    .align_y(Alignment::Center)
    .into()
}

fn filter_type_dropdown<'a>(id: ParamId, state: &'a State) -> Element<'a, Message> {
    let filter_type = FilterType::from_u8(state.shared.params.get(id) as u8);
    // The sampler's voice filter implements this SFZ-compatible set; offering
    // the full shared 48-type enum would list types that pass through unfiltered.
    let options: Vec<FilterType> = [
        FilterType::Lowpass,
        FilterType::Highpass,
        FilterType::Bandpass,
        FilterType::Notch,
        FilterType::Peak,
        FilterType::Allpass,
        FilterType::LowShelf,
        FilterType::HighShelf,
        FilterType::Bell,
    ]
    .to_vec();
    let selected = options
        .iter()
        .position(|option| *option == filter_type)
        .unwrap_or(0);
    let labels: Vec<String> = options
        .iter()
        .map(|option| option.name().to_string())
        .collect();
    let toggler = horizontal_multi_toggler(labels, selected, move |index| {
        Message::SetParam(id, options[index] as u8 as f32)
    })
    .size(10.0)
    .spacing(2.0)
    .text_size(10.0)
    .label_extent(90.0);

    container(toggler).into()
}

fn lfo_shape_dropdown<'a>(id: ParamId, state: &'a State) -> Element<'a, Message> {
    let shape = LfoShape::from_u8(state.shared.params.get(id) as u8);
    let shapes = LfoShape::all().to_vec();
    let names: Vec<String> = shapes
        .iter()
        .map(|option| option.name().to_string())
        .collect();
    container(
        pick_list(names, Some(shape.name().to_string()), move |name| {
            let index = shapes
                .iter()
                .position(|option| option.name() == name)
                .unwrap_or(0);
            Message::SetParam(id, shapes[index] as u8 as f32)
        })
        .width(Length::Shrink),
    )
    .align_y(Alignment::Center)
    .into()
}

fn lfo_selector_dropdown<'a>(state: &'a State) -> Element<'a, Message> {
    const NAMES: [&str; 6] = ["LFO 1", "LFO 2", "LFO 3", "LFO 4", "LFO 5", "LFO 6"];
    container(
        pick_list(
            NAMES.map(String::from).to_vec(),
            Some(NAMES[state.selected_lfo.min(NAMES.len() - 1)].to_string()),
            |name| {
                let index = NAMES.iter().position(|option| *option == name).unwrap_or(0);
                Message::SelectLfo(index)
            },
        )
        .width(Length::Shrink),
    )
    .align_y(Alignment::Center)
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
        .height(Length::Shrink)
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

/// A titled panel that stretches to the full height of its parent row. Only
/// works as the single fill-height child alongside natural-height siblings:
/// with no non-fluid sibling the row has no height to distribute.
fn panel_fill<'a>(title: &'static str, content: Element<'a, Message>) -> Element<'a, Message> {
    container(
        column![section_title(title), content]
            .spacing(6)
            .align_x(Alignment::Start),
    )
    .padding(8)
    .height(Length::Fill)
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
const ZONE_CLICK_DRAG_THRESHOLD: f32 = 4.0;

fn note_at_x(x: f32, width: f32) -> usize {
    let note_width = width / SAMPLE_MAP_NOTES as f32;
    (x / note_width)
        .floor()
        .clamp(0.0, SAMPLE_MAP_NOTES as f32 - 1.0) as usize
}

fn piano_note_at(position: Point, bounds: Rectangle) -> Option<(u8, u8)> {
    let grid_height = bounds.height - PIANO_ROLL_HEIGHT;
    if position.y < grid_height || position.y > bounds.height {
        return None;
    }
    let note_width = bounds.width / SAMPLE_MAP_NOTES as f32;
    for octave in 0..10_u8 {
        let octave_x = f32::from(octave) * 12.0 * note_width;
        let octave_width = 12.0 * note_width;
        if position.x < octave_x || position.x > octave_x + octave_width {
            continue;
        }
        let local_position = Point::new(position.x - octave_x, position.y - grid_height);
        let local_bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: octave_width,
            height: PIANO_ROLL_HEIGHT,
        };
        if let Some((note_class, velocity)) =
            note_at_in_range(local_position, local_bounds, Orientation::Degree0, 12)
        {
            return Some((octave * 12 + note_class, velocity));
        }
    }
    let partial_x = 10.0 * 12.0 * note_width;
    let partial_width = 8.0 * note_width;
    if position.x >= partial_x && position.x <= partial_x + partial_width {
        let local_position = Point::new(position.x - partial_x, position.y - grid_height);
        let local_bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: partial_width,
            height: PIANO_ROLL_HEIGHT,
        };
        let note_count = octave_note_count(10);
        if let Some((note_class, velocity)) = note_at_in_range(
            local_position,
            local_bounds,
            Orientation::Degree0,
            note_count,
        ) {
            return Some((10 * 12 + note_class, velocity));
        }
    }
    None
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
    piano_active_note: Option<u8>,
}

#[derive(Default, Debug)]
struct ZoneEditorState {
    last_click_at: Option<Instant>,
    body_press: Option<ZoneBodyPress>,
}

#[derive(Debug)]
struct ZoneBodyPress {
    index: usize,
    position: Point,
    note_offset: f32,
    velocity_offset: f32,
    moved: bool,
}

struct ZoneEditor {
    data: ZoneEditorData,
}

impl ZoneEditor {
    fn edge_hit_test(&self, position: Point, bounds: Rectangle) -> Option<(usize, ZoneEdge)> {
        let grid_height = bounds.height - PIANO_ROLL_HEIGHT;
        if position.y < 0.0 || position.y >= grid_height {
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
        if position.y < 0.0 || position.y >= grid_height {
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
    type State = ZoneEditorState;

    fn update(
        &self,
        state: &mut Self::State,
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
                        if let Some(press) = state.body_press.as_mut()
                            && !press.moved
                        {
                            let dx = position.x - press.position.x;
                            let dy = position.y - press.position.y;
                            if dx.hypot(dy) >= ZONE_CLICK_DRAG_THRESHOLD {
                                press.moved = true;
                                return Some(
                                    canvas::Action::publish(Message::StartZoneBodyDrag(
                                        press.index,
                                        press.note_offset,
                                        press.velocity_offset,
                                    ))
                                    .and_capture(),
                                );
                            }
                        }

                        let note = note_at_x(position.x, bounds.width);
                        let velocity = velocity_at_y(
                            position.y,
                            bounds.height - PIANO_ROLL_HEIGHT,
                            bounds.height - PIANO_ROLL_HEIGHT,
                        );
                        Some(canvas::Action::publish(Message::ZoneNoteHovered(
                            note,
                            velocity,
                            position.y + bounds.y,
                        )))
                    }
                    maolan_baseview::iced::mouse::Event::ButtonPressed(
                        maolan_baseview::iced::mouse::Button::Left,
                    ) => {
                        let now = Instant::now();
                        let is_double_click = state.last_click_at.is_some_and(|last| {
                            now.duration_since(last) <= Duration::from_millis(300)
                        });
                        state.last_click_at = Some(now);

                        if is_double_click {
                            if let Some((index, _edge)) = self.edge_hit_test(position, bounds) {
                                state.body_press = None;
                                return Some(
                                    canvas::Action::publish(Message::OpenSamplerEditor(index))
                                        .and_capture(),
                                );
                            }
                            if let Some(index) = self.body_hit_test(position, bounds) {
                                state.body_press = None;
                                return Some(
                                    canvas::Action::publish(Message::OpenSamplerEditor(index))
                                        .and_capture(),
                                );
                            }
                        }

                        if let Some((index, edge)) = self.edge_hit_test(position, bounds) {
                            state.body_press = None;
                            Some(
                                canvas::Action::publish(Message::StartZoneEdgeDrag(index, edge))
                                    .and_capture(),
                            )
                        } else if let Some(index) = self.body_hit_test(position, bounds) {
                            let zone = &self.data.zones[index];
                            let note_width = bounds.width / SAMPLE_MAP_NOTES as f32;
                            let grid_height = bounds.height - PIANO_ROLL_HEIGHT;
                            let velocity_height = grid_height / VELOCITY_COUNT;
                            let note_offset =
                                (position.x - zone.start_note as f32 * note_width) / note_width;
                            let velocity_offset = ((grid_height - position.y) / velocity_height)
                                - zone.vel_low as f32;
                            state.body_press = Some(ZoneBodyPress {
                                index,
                                position,
                                note_offset,
                                velocity_offset,
                                moved: false,
                            });
                            Some(
                                canvas::Action::publish(Message::SelectZoneListItem(
                                    ZoneListSelection::Zone(index),
                                ))
                                .and_capture(),
                            )
                        } else if let Some((note, velocity)) = piano_note_at(position, bounds) {
                            state.body_press = None;
                            Some(
                                canvas::Action::publish(Message::PianoKeyPressed(note, velocity))
                                    .and_capture(),
                            )
                        } else if self.data.dragged_audio_file.is_none() {
                            state.body_press = None;
                            Some(canvas::Action::publish(Message::DeselectZone).and_capture())
                        } else {
                            state.body_press = None;
                            None
                        }
                    }
                    maolan_baseview::iced::mouse::Event::ButtonReleased(
                        maolan_baseview::iced::mouse::Button::Left,
                    ) => {
                        if let Some(note) = self.data.piano_active_note {
                            Some(
                                canvas::Action::publish(Message::PianoKeyReleased(note))
                                    .and_capture(),
                            )
                        } else if let Some(press) = state.body_press.take()
                            && !press.moved
                            && self.data.dragged_audio_file.is_none()
                        {
                            Some(canvas::Action::capture())
                        } else {
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
                    }
                    maolan_baseview::iced::mouse::Event::CursorLeft => {
                        self.data.piano_active_note.map(|note| {
                            canvas::Action::publish(Message::PianoKeyReleased(note)).and_capture()
                        })
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
        let active_note = self
            .data
            .piano_active_note
            .map(|note| (note / 12, note % 12));

        let background = canvas::Path::rectangle(Point::ORIGIN, bounds.size());
        frame.fill(&background, Color::from_rgb(0.075, 0.078, 0.095));

        for note in 0..SAMPLE_MAP_NOTES {
            let x = note as f32 * note_width;
            let is_c = note % 12 == 0;
            let line = canvas::Path::line(Point::new(x, 0.0), Point::new(x, grid_height));
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
            let path = canvas::Path::rectangle(
                Point::new(rect.x, rect.y),
                Size::new(rect.width, rect.height),
            );
            let selected = self
                .data
                .selected
                .as_ref()
                .is_some_and(|sel| matches!(sel, ZoneListSelection::Zone(i) if *i == index));
            let color = if self
                .data
                .dragging_zone_edge
                .is_some_and(|(i, _)| i == index)
            {
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
        }

        if let Some(position) = cursor.position_in(bounds)
            && let Some((index, edge)) = self.edge_hit_test(position, bounds)
        {
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

        if self.data.dragged_audio_file.is_some()
            && let Some(hovered_note) = self.data.hovered_note
        {
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
            let preview = SampleZone::new_basic(
                String::new(),
                Vec::new(),
                start_note,
                end_note,
                vel_low,
                vel_high,
                String::new(),
            );
            let rect = zone_rect(&preview, bounds);
            let path = canvas::Path::rectangle(
                Point::new(rect.x, rect.y),
                Size::new(rect.width, rect.height),
            );
            frame.fill(&path, Color::from_rgba(0.22, 0.34, 0.48, 0.6));
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_color(Color::from_rgb(0.42, 0.72, 1.0))
                    .with_width(1.0),
            );
        }

        let piano_background = canvas::Path::rectangle(
            Point::new(0.0, grid_height),
            Size::new(width, PIANO_ROLL_HEIGHT),
        );
        frame.fill(&piano_background, Color::from_rgb(0.045, 0.047, 0.058));

        let names = std::collections::HashMap::new();

        for octave in 0..10 {
            let octave_x = octave as f32 * 12.0 * note_width;
            let octave_width = 12.0 * note_width;
            let octave_bounds = Rectangle {
                x: octave_x,
                y: grid_height,
                width: octave_width,
                height: PIANO_ROLL_HEIGHT,
            };
            let pressed: std::collections::HashSet<u8> = active_note
                .filter(|(o, _)| *o == octave as u8)
                .map(|(_, c)| c)
                .into_iter()
                .collect();
            draw_octave_into(
                &mut frame,
                octave_bounds,
                &pressed,
                octave as u8,
                &names,
                Orientation::Degree0,
            );
        }

        let partial_x = 10.0 * 12.0 * note_width;
        let partial_width = 8.0 * note_width;
        let partial_bounds = Rectangle {
            x: partial_x,
            y: grid_height,
            width: partial_width,
            height: PIANO_ROLL_HEIGHT,
        };
        let pressed: std::collections::HashSet<u8> = active_note
            .filter(|(o, _)| *o == 10)
            .map(|(_, c)| c)
            .into_iter()
            .collect();
        draw_partial_octave_into(
            &mut frame,
            partial_bounds,
            &pressed,
            10,
            &names,
            Orientation::Degree0,
        );

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
                } else if piano_note_at(position, bounds).is_some() {
                    maolan_baseview::iced::mouse::Interaction::Pointer
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
        piano_active_note: state.piano_active_note,
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
    .on_release(Message::PointerReleased)
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
        text(zone.name)
            .size(11)
            .wrapping(text::Wrapping::None)
            .into()
    };

    let content = column![
        name_element,
        text(file_name)
            .size(9)
            .wrapping(text::Wrapping::None)
            .color(Color::from_rgb(0.48, 0.50, 0.58)),
        text(range_text)
            .size(9)
            .wrapping(text::Wrapping::None)
            .color(Color::from_rgb(0.48, 0.50, 0.58)),
    ]
    .spacing(1)
    .width(Length::Fill);
    let content = container(content).width(Length::Fill).clip(true);

    let selected = state
        .selected
        .as_ref()
        .is_some_and(|sel| matches!(sel, ZoneListSelection::Zone(i) if *i == index));
    let dragging = state.dragging_zone_list_item == Some(index);

    let wrapped = if is_editing {
        container(content)
    } else {
        let mouse_area = if state.editing_zone_index.is_some() {
            mouse_area(content).on_press(Message::OpenSamplerEditor(index))
        } else {
            mouse_area(content)
                .on_press(Message::BeginZoneListDrag(index))
                .on_release(Message::FinishZoneListDrag)
                .on_double_click(Message::OpenSamplerEditor(index))
        };
        let mut inner = row![mouse_area].spacing(4).align_y(Alignment::Center);
        inner = inner.push(
            button(text("✎").size(9))
                .on_press(Message::StartRenameZone(index))
                .padding([2, 4]),
        );
        container(inner)
    }
    .padding([4, 6])
    .width(Length::Fill)
    .clip(true)
    .style(move |_theme: &Theme| container::Style {
        background: dragging.then(|| Background::Color(Color::from_rgb(0.12, 0.15, 0.20))),
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

fn knob_field<'a>(
    label: &'static str,
    knob: impl Into<Element<'a, Message>>,
    value_text: String,
) -> Element<'a, Message> {
    container(
        column![text(label).size(11), knob.into(), text(value_text).size(10)]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(50.0))
    .padding(2)
    .into()
}

/// Labeled dropdown for assigning a zone/group to one of the defined output
/// buses.
fn output_bus_picker<'a>(
    label: &'static str,
    options: Vec<OutputBusOption>,
    selected: Option<OutputBusOption>,
    on_select: impl Fn(OutputBusOption) -> Message + 'a,
) -> Element<'a, Message> {
    container(
        column![
            text(label).size(11),
            pick_list(options, selected, on_select).width(Length::Fixed(120.0)),
        ]
        .spacing(2)
        .align_x(Alignment::Center),
    )
    .padding(2)
    .into()
}

fn group_knob<'a>(
    label: &'static str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    field: GroupEditField,
    value_text: String,
) -> Element<'a, Message> {
    let knob = ArchSlider::new(range, value, move |v| {
        Message::SetSelectedGroupValue(field, v)
    })
    .step(step)
    .width(Length::Fixed(41.0))
    .height(Length::Fixed(41.0))
    .fill_from_start();

    knob_field(label, knob, value_text)
}

fn group_param_knob<'a>(
    label: &'static str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    field: GroupParamField,
    value_text: String,
) -> Element<'a, Message> {
    let knob = ArchSlider::new(range, value, move |v| {
        Message::SetSelectedGroupParam(field, v)
    })
    .step(step)
    .width(Length::Fixed(41.0))
    .height(Length::Fixed(41.0))
    .fill_from_start();

    knob_field(label, knob, value_text)
}

fn keyswitch_knob<'a>(
    label: &'static str,
    value: Option<u8>,
    field: GroupEditField,
) -> Element<'a, Message> {
    let knob = ArchSlider::new(
        -1.0..=127.0,
        slider_value_from_note_option(value),
        move |v| Message::SetSelectedGroupValue(field, v),
    )
    .step(1.0)
    .width(Length::Fixed(41.0))
    .height(Length::Fixed(41.0))
    .fill_from_start();

    knob_field(label, knob, note_option_text(value))
}

fn group_lfo_selector_dropdown<'a>(state: &'a State) -> Element<'a, Message> {
    const NAMES: [&str; 4] = ["LFO 1", "LFO 2", "LFO 3", "LFO 4"];
    container(
        pick_list(
            NAMES.map(String::from).to_vec(),
            Some(NAMES[state.selected_group_lfo.min(3)].to_string()),
            |name| {
                let index = NAMES.iter().position(|option| *option == name).unwrap_or(0);
                Message::SelectGroupLfo(index)
            },
        )
        .width(Length::Shrink),
    )
    .align_y(Alignment::Center)
    .into()
}

fn group_lfo_shape_dropdown<'a>(lfo_index: usize, shape: LfoShape) -> Element<'a, Message> {
    let shapes = LfoShape::all().to_vec();
    let names: Vec<String> = shapes
        .iter()
        .map(|option| option.name().to_string())
        .collect();
    container(
        pick_list(names, Some(shape.name().to_string()), move |name| {
            let index = shapes
                .iter()
                .position(|option| option.name() == name)
                .unwrap_or(0);
            Message::SetSelectedGroupLfoShape(lfo_index, shapes[index])
        })
        .width(Length::Shrink),
    )
    .align_y(Alignment::Center)
    .into()
}

fn group_filter_type_dropdown<'a>(filter_type: FilterType) -> Element<'a, Message> {
    let options = filter_type_options().to_vec();
    let names: Vec<String> = options
        .iter()
        .map(|option| option.name().to_string())
        .collect();
    container(
        pick_list(names, Some(filter_type.name().to_string()), move |name| {
            let index = options
                .iter()
                .position(|option| option.name() == name)
                .unwrap_or(0);
            Message::SetSelectedGroupFilterType(options[index])
        })
        .width(Length::Shrink),
    )
    .align_y(Alignment::Center)
    .into()
}

fn group_filter_subtype_dropdown<'a>(subtype: FilterSubtype) -> Element<'a, Message> {
    let options = filter_subtype_options().to_vec();
    let labels: Vec<String> = options.iter().map(|option| format!("{option}")).collect();
    container(
        pick_list(labels, Some(format!("{subtype}")), move |name| {
            let index = options
                .iter()
                .position(|option| format!("{option}") == name)
                .unwrap_or(0);
            Message::SetSelectedGroupFilterSubtype(options[index])
        })
        .width(Length::Shrink),
    )
    .align_y(Alignment::Center)
    .into()
}

fn filter_type_options() -> [FilterType; 9] {
    [
        FilterType::Lowpass,
        FilterType::Highpass,
        FilterType::Bandpass,
        FilterType::Notch,
        FilterType::Peak,
        FilterType::Allpass,
        FilterType::LowShelf,
        FilterType::HighShelf,
        FilterType::Bell,
    ]
}

fn filter_subtype_options() -> [FilterSubtype; 7] {
    [
        FilterSubtype::Clean,
        FilterSubtype::MildDrive,
        FilterSubtype::HeavyDrive,
        FilterSubtype::Asymmetric,
        FilterSubtype::SoftClip,
        FilterSubtype::SineSat,
        FilterSubtype::Ojd,
    ]
}

fn sanitize_sfz_opcode_key(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        .collect::<String>()
        .to_ascii_lowercase()
}

fn format_sfz_opcode_text(opcodes: &[(String, String)]) -> String {
    opcodes
        .iter()
        .filter(|(key, _)| is_non_vendor_sfz_opcode(key))
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_sfz_opcode_text(text: &str) -> Vec<(String, String)> {
    text.split_whitespace()
        .filter_map(|item| {
            let (key, value) = item.split_once('=')?;
            let key = sanitize_sfz_opcode_key(key);
            if key.is_empty() || !is_non_vendor_sfz_opcode(&key) {
                return None;
            }
            Some((key, value.to_string()))
        })
        .collect()
}

fn group_eg_widget<'a>(params: Option<AdsrParams>, eg: usize) -> Element<'a, Message> {
    let params = params.unwrap_or(AdsrParams {
        attack: 0.01,
        decay: 0.0,
        sustain: 1.0,
        release: 0.05,
        ..AdsrParams::default()
    });
    adsr_envelope(
        [params.attack, params.decay, params.sustain, params.release],
        [(0.0, 10.0), (0.0, 10.0), (0.0, 1.0), (0.0, 10.0)],
        [0.001, 0.001, 0.01, 0.001],
        [false; 4],
    )
    .map(move |message| match message {
        AdsrMessage::Change(index, value) => {
            let field = match (eg, index) {
                (0, 0) => GroupParamField::Eg1Attack,
                (0, 1) => GroupParamField::Eg1Decay,
                (0, 2) => GroupParamField::Eg1Sustain,
                (0, _) => GroupParamField::Eg1Release,
                (_, 0) => GroupParamField::Eg2Attack,
                (_, 1) => GroupParamField::Eg2Decay,
                (_, 2) => GroupParamField::Eg2Sustain,
                _ => GroupParamField::Eg2Release,
            };
            Message::SetSelectedGroupParam(field, value)
        }
        AdsrMessage::Release(_)
        | AdsrMessage::Reset(_)
        | AdsrMessage::Assign(_)
        | AdsrMessage::Edit(_)
        | AdsrMessage::EditChanged(_, _)
        | AdsrMessage::EditSubmit(_) => Message::None,
    })
}

fn group_controls_panel<'a>(state: &'a State, group: &SampleGroup) -> Element<'a, Message> {
    let selected_eg_widget = if state.selected_group_eg == 0 {
        group_eg_widget(group.eg1_params, 0)
    } else {
        group_eg_widget(group.eg2_params, 1)
    };
    let lfo_index = state.selected_group_lfo.min(3);
    let (lfo_rate, lfo_amount, lfo_shape) = match lfo_index {
        0 => (
            group.lfo1_params.map_or(0.0, |p| p.rate),
            group.lfo1_params.map_or(0.0, |p| p.amount),
            group.lfo1_params.map_or(LfoShape::Sine, |p| p.shape),
        ),
        1 => (
            group.lfo2_params.map_or(0.0, |p| p.rate),
            group.lfo2_params.map_or(0.0, |p| p.amount),
            group.lfo2_params.map_or(LfoShape::Sine, |p| p.shape),
        ),
        2 => (
            group.lfo3_params.map_or(0.0, |p| p.rate),
            group.lfo3_params.map_or(0.0, |p| p.amount),
            group.lfo3_params.map_or(LfoShape::Sine, |p| p.shape),
        ),
        _ => (
            group.lfo4_params.map_or(0.0, |p| p.rate),
            group.lfo4_params.map_or(0.0, |p| p.amount),
            group.lfo4_params.map_or(LfoShape::Sine, |p| p.shape),
        ),
    };
    let lfo_rate_field = [
        GroupParamField::Lfo1Rate,
        GroupParamField::Lfo2Rate,
        GroupParamField::Lfo3Rate,
        GroupParamField::Lfo4Rate,
    ][lfo_index];
    let lfo_amount_field = [
        GroupParamField::Lfo1Amount,
        GroupParamField::Lfo2Amount,
        GroupParamField::Lfo3Amount,
        GroupParamField::Lfo4Amount,
    ][lfo_index];
    column![
        row![
            panel_no_title(
                column![
                    knob_row(vec![
                        group_knob(
                            "Poly",
                            group.poly_limit as f32,
                            0.0..=128.0,
                            1.0,
                            GroupEditField::PolyLimit,
                            group.poly_limit.to_string(),
                        ),
                        group_knob(
                            "Excl",
                            group.exclusive_group as f32,
                            0.0..=255.0,
                            1.0,
                            GroupEditField::ExclusiveGroup,
                            group.exclusive_group.to_string(),
                        ),
                        group_knob(
                            "Gain",
                            group.gain_db,
                            -96.0..=24.0,
                            0.1,
                            GroupEditField::GainDb,
                            format!("{:.1} dB", group.gain_db),
                        ),
                        group_knob(
                            "Pan",
                            group.pan * 100.0,
                            -100.0..=100.0,
                            1.0,
                            GroupEditField::Pan,
                            format!("{:.0}", group.pan * 100.0),
                        ),
                    ]),
                    text("Keyswitches")
                        .size(10)
                        .color(Color::from_rgb(0.68, 0.70, 0.76)),
                    knob_row(vec![
                        keyswitch_knob("Last", group.sw_last, GroupEditField::SwLast),
                        keyswitch_knob("Down", group.sw_down, GroupEditField::SwDown),
                        keyswitch_knob("Up", group.sw_up, GroupEditField::SwUp),
                    ]),
                    knob_row(vec![
                        keyswitch_knob("Previous", group.sw_previous, GroupEditField::SwPrevious),
                        range_knob(
                            RangeKnob {
                                label: "Lo - Hi".into(),
                                low: f32::from(group.sw_lolast.or(group.sw_hilast).unwrap_or(0)),
                                high: f32::from(group.sw_hilast.or(group.sw_lolast).unwrap_or(0)),
                                range: 0.0..=127.0,
                                step: 1.0,
                                value_text: if group.sw_lolast.is_none()
                                    && group.sw_hilast.is_none()
                                {
                                    String::from("off")
                                } else {
                                    format!(
                                        "{} - {}",
                                        group.sw_lolast.unwrap_or(0),
                                        group.sw_hilast.unwrap_or(0)
                                    )
                                },
                                assigned: false,
                            },
                            Message::SetSelectedGroupSwLastRange,
                            Message::None,
                        ),
                        keyswitch_knob("Default", group.sw_default, GroupEditField::SwDefault),
                    ]),
                    row![{
                        let names = state.shared.output_names.load();
                        let options = output_bus_options(&names, group.output);
                        let selected = options
                            .iter()
                            .find(|option| option.index == group.output as usize)
                            .cloned();
                        output_bus_picker("Output", options, selected, |option| {
                            Message::SetSelectedGroupValue(
                                GroupEditField::Output,
                                option.index as f32,
                            )
                        })
                    }]
                    .spacing(8)
                    .align_y(Alignment::Start),
                ]
                .spacing(4)
                .into(),
            ),
            panel_no_title(
                column![
                    horizontal_multi_toggler(
                        ["Amp", "Filter"],
                        state.selected_group_eg.min(1),
                        Message::SelectGroupEg,
                    ),
                    selected_eg_widget,
                    text("LFO")
                        .size(10)
                        .color(Color::from_rgb(0.68, 0.70, 0.76)),
                    knob_row(vec![
                        group_lfo_selector_dropdown(state),
                        group_lfo_shape_dropdown(lfo_index + 1, lfo_shape),
                        group_param_knob(
                            "Rate",
                            lfo_rate,
                            0.0..=20.0,
                            0.1,
                            lfo_rate_field,
                            format!("{lfo_rate:.1}"),
                        ),
                        group_param_knob(
                            "Amt",
                            lfo_amount,
                            0.0..=100.0,
                            1.0,
                            lfo_amount_field,
                            format!("{lfo_amount:.1}"),
                        ),
                    ]),
                    knob_row(vec![
                        group_filter_type_dropdown(
                            group
                                .filter_params
                                .map_or(FilterType::Lowpass, |p| p.filter_type),
                        ),
                        group_filter_subtype_dropdown(
                            group
                                .filter_params
                                .map_or(FilterSubtype::Clean, |p| p.subtype),
                        ),
                        group_param_knob(
                            "Cut",
                            group
                                .filter_params
                                .map_or(FilterParams::default().cutoff, |p| p.cutoff),
                            20.0..=20000.0,
                            1.0,
                            GroupParamField::FilterCutoff,
                            format!(
                                "{:.0}",
                                group
                                    .filter_params
                                    .map_or(FilterParams::default().cutoff, |p| p.cutoff)
                            ),
                        ),
                        group_param_knob(
                            "Res",
                            group
                                .filter_params
                                .map_or(FilterParams::default().resonance, |p| p.resonance),
                            0.0..=1.0,
                            0.01,
                            GroupParamField::FilterResonance,
                            format!(
                                "{:.2}",
                                group
                                    .filter_params
                                    .map_or(FilterParams::default().resonance, |p| p.resonance)
                            ),
                        ),
                        group_param_knob(
                            "Key",
                            group
                                .filter_params
                                .map_or(FilterParams::default().key_tracking * 100.0, |p| p
                                    .key_tracking
                                    * 100.0),
                            0.0..=100.0,
                            1.0,
                            GroupParamField::FilterKeyTrack,
                            format!(
                                "{:.0}",
                                group
                                    .filter_params
                                    .map_or(FilterParams::default().key_tracking * 100.0, |p| p
                                        .key_tracking
                                        * 100.0)
                            ),
                        ),
                        group_param_knob(
                            "Vel",
                            group
                                .filter_params
                                .map_or(FilterParams::default().vel_tracking, |p| p.vel_tracking),
                            -9600.0..=9600.0,
                            1.0,
                            GroupParamField::FilterVelTrack,
                            format!(
                                "{:.0}",
                                group
                                    .filter_params
                                    .map_or(FilterParams::default().vel_tracking, |p| p
                                        .vel_tracking)
                            ),
                        ),
                    ]),
                ]
                .spacing(4)
                .into(),
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Start),
        text_input("sw_label ...", group.sw_label.as_deref().unwrap_or(""))
            .on_input(Message::SetSelectedGroupSwLabel)
            .size(10)
            .width(Length::Fill),
        text_input("opcode=value ...", &state.extra_sfz_opcode_text)
            .on_input(Message::SetSelectedGroupExtraSfz)
            .size(10)
            .width(Length::Fill),
    ]
    .spacing(6)
    .into()
}

fn zones_panel<'a>(state: &'a State) -> Element<'a, Message> {
    let shared_zones = state.shared.zones.load();
    let shared_groups = state.shared.groups.load();

    let mut groups: Vec<(String, Vec<(usize, SampleZone)>)> = Vec::new();
    for group in shared_groups.iter() {
        groups.push((group.name.clone(), Vec::new()));
    }
    for (index, zone) in shared_zones.iter().cloned().enumerate() {
        match groups.iter_mut().find(|(name, _)| name == &zone.group) {
            Some((_, entries)) => entries.push((index, zone)),
            None => groups.push((zone.group.clone(), vec![(index, zone)])),
        }
    }

    let mut panel = column![].spacing(6).width(Length::Fill);
    for (group_name, mut entries) in groups {
        entries.sort_by_key(|(index, zone)| {
            (
                zone.start_note,
                zone.end_note,
                zone.vel_low,
                zone.vel_high,
                *index,
            )
        });
        let group_selected = state.selected.as_ref().is_some_and(
            |sel| matches!(sel, ZoneListSelection::Group(name) if name == &group_name),
        );
        let group_drop_target = state.dragging_zone_list_item.is_some()
            && state
                .hovered_zone_drop_group
                .as_ref()
                .is_some_and(|name| name == &group_name);
        let mut zones = column![].spacing(3).width(Length::Fill);
        for (index, zone) in entries {
            zones = zones.push(zone_row(state, index, zone));
        }
        let header = text(group_name.clone())
            .size(10)
            .wrapping(text::Wrapping::None)
            .color(Color::from_rgb(0.90, 0.91, 0.94));
        let group_box_content = container(column![header, zones].spacing(4).width(Length::Fill))
            .padding([6, 6])
            .width(Length::Fill)
            .clip(true)
            .style(move |_theme: &Theme| container::Style {
                background: group_drop_target
                    .then(|| Background::Color(Color::from_rgb(0.11, 0.17, 0.24))),
                border: Border {
                    color: if group_drop_target {
                        Color::from_rgb(0.70, 0.82, 1.0)
                    } else if group_selected {
                        Color::from_rgb(0.45, 0.65, 0.95)
                    } else {
                        Color::from_rgb(0.22, 0.26, 0.34)
                    },
                    width: if group_selected || group_drop_target {
                        2.0
                    } else {
                        1.0
                    },
                    radius: 4.0.into(),
                },
                ..container::Style::default()
            });
        let group_name_for_enter = group_name.clone();
        let group_name_for_open = group_name.clone();
        let group_box = mouse_area(group_box_content)
            .on_enter(Message::HoverZoneDropGroup(Some(group_name_for_enter)))
            .on_exit(Message::HoverZoneDropGroup(None))
            .on_release(Message::FinishZoneListDrag)
            .on_double_click(Message::OpenGroupEditor(group_name_for_open));
        panel = panel.push(group_box);
    }

    let header = row![
        section_title("Zones"),
        button(text("+ Group").size(10))
            .padding([3, 8])
            .on_press(Message::CreateZoneListItem(ZoneCreateKind::Group)),
        button(text("+ Zone").size(10))
            .padding([3, 8])
            .on_press(Message::CreateZoneListItem(ZoneCreateKind::Zone)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let mut content = column![header]
        .spacing(8)
        .height(Length::Fill)
        .align_x(Alignment::Start);
    content = content.push(scrollable(panel).height(Length::Fill));

    container(content)
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
    let is_dir = entry.kind == BrowserEntryKind::Directory;
    let is_instrument = entry.kind == BrowserEntryKind::Instrument;
    let label = if is_dir {
        format!("{}/", entry.name)
    } else if is_instrument {
        format!("{} *", entry.name)
    } else {
        entry.name.clone()
    };
    let content = container(text(label).size(11))
        .padding([3, 6])
        .style(move |_theme: &Theme| container::Style {
            background: (is_dir || is_instrument)
                .then(|| Background::Color(Color::from_rgb(0.105, 0.108, 0.128))),
            border: Border {
                color: if is_dir {
                    Color::from_rgb(0.22, 0.48, 0.95)
                } else if is_instrument {
                    Color::from_rgb(0.24, 0.62, 0.46)
                } else {
                    Color::from_rgb(0.18, 0.18, 0.22)
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            text_color: Some(if is_dir {
                Color::from_rgb(0.82, 0.84, 0.92)
            } else if is_instrument {
                Color::from_rgb(0.72, 0.86, 0.78)
            } else {
                Color::from_rgb(0.62, 0.64, 0.70)
            }),
            ..container::Style::default()
        });

    if is_dir {
        button(content)
            .padding(1)
            .on_press(Message::OpenBrowserEntry(entry.path.clone()))
            .into()
    } else if is_instrument {
        button(content)
            .padding(1)
            .on_press(Message::LoadInstrument(entry.path.clone()))
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
                text(
                    state
                        .browser_path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| state.browser_path.display().to_string())
                )
                .size(10),
            )
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct OutputBusOption {
    index: usize,

    label: String,
}

impl std::fmt::Display for OutputBusOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

/// One `pick_list` entry per defined output bus. A zone/group whose assigned
/// bus is beyond the defined list (e.g. from an SFZ loaded before names were
/// extended) keeps its entry so the current value stays visible.
fn output_bus_options(names: &[String], current: u16) -> Vec<OutputBusOption> {
    let mut options: Vec<OutputBusOption> = (0..names.len())
        .map(|index| OutputBusOption {
            index,
            label: output_bus_label(names, index),
        })
        .collect();
    let current = current as usize;
    if current >= names.len() && current < MAX_SAMPLER_BUSES {
        options.push(OutputBusOption {
            index: current,
            label: format!("{current}: {}", output_bus_label(names, current)),
        });
    }
    options
}

fn outputs_panel<'a>(state: &'a State) -> Element<'a, Message> {
    let names = state.shared.output_names.load();

    let cards: Vec<Element<'_, Message>> = names
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != 0)
        .map(|(index, name)| {
            let name_field: Element<'_, Message> = if index == 0 {
                text(name.clone()).size(11).into()
            } else {
                text_input("Bus name", name)
                    .on_input(move |text| Message::RenameOutput(index, text))
                    .size(11)
                    .width(Length::Fixed(100.0))
                    .into()
            };
            let mut row_content = row![
                text(format!("{index:02}"))
                    .size(10)
                    .color(Color::from_rgb(0.68, 0.70, 0.76)),
                name_field,
            ]
            .spacing(6)
            .align_y(Alignment::Center);
            if index > 0 {
                row_content = row_content.push(
                    button(text("x").size(10))
                        .padding([2, 6])
                        .on_press(Message::RemoveOutput(index)),
                );
            }
            panel_no_title(row_content.into())
        })
        .collect();

    let cleanup_button = button(text("Clean Up").size(10))
        .padding([3, 8])
        .on_press(Message::CleanUpOutputs);
    let mut add_button = button(text("+ Output").size(10)).padding([3, 8]);
    if names.len() < MAX_SAMPLER_BUSES {
        add_button = add_button.on_press(Message::AddOutput);
    }

    let header = row![section_title("Outputs"), cleanup_button, add_button]
        .spacing(6)
        .align_y(Alignment::Center);

    container(
        column![
            header,
            scrollable(
                container(Wrap::with_elements(cards).spacing(6).line_spacing(6))
                    .width(Length::Fill)
                    .padding([4, 8]),
            )
            .height(Length::Fill)
        ]
        .spacing(6)
        .width(Length::Fill),
    )
    .padding(8)
    .height(Length::Fill)
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

fn instrument_panel<'a>(state: &'a State) -> Element<'a, Message> {
    let status = state.shared.load_status.lock().clone();
    let path = state.shared.instrument_path.lock().clone();
    let presets = state.shared.sf2_presets.lock().clone();
    let selected_preset = *state.shared.selected_sf2_preset.lock();
    let log = state.shared.load_log.lock().clone();

    let (title, detail, loading, is_error) = match &status {
        SamplerLoadStatus::Empty => (
            String::from("No instrument"),
            String::from("Load an SFZ or SF2 file"),
            false,
            false,
        ),
        SamplerLoadStatus::Parsing => (
            String::from("Parsing"),
            path_display(path.as_ref()),
            true,
            false,
        ),
        SamplerLoadStatus::LoadingSamples { loaded, total } => (
            String::from("Loading samples"),
            format!("{loaded}/{total} samples"),
            true,
            false,
        ),
        SamplerLoadStatus::Resampling => (
            String::from("Resampling"),
            path_display(path.as_ref()),
            true,
            false,
        ),
        SamplerLoadStatus::Ready {
            name,
            sample_count,
            zone_count,
        } => (
            name.clone(),
            format!("{sample_count} samples / {zone_count} zones"),
            false,
            false,
        ),
        SamplerLoadStatus::Error(message) => {
            (String::from("Load error"), message.clone(), false, true)
        }
    };

    let mut controls = row![
        button(text("Load").size(11))
            .on_press(Message::PickInstrumentFile)
            .padding([4, 8]),
        button(text("Import").size(11))
            .on_press(Message::PickImportFile)
            .padding([4, 8]),
        button(text("Reload").size(11))
            .on_press(Message::ReloadInstrument)
            .padding([4, 8]),
        button(text("Export").size(11))
            .on_press(Message::ExportSfz)
            .padding([4, 8]),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    if presets.len() > 1 {
        let selected = selected_preset.and_then(|index| presets.get(index).cloned());
        controls = controls.push(
            pick_list(presets, selected, Message::SelectSf2Preset)
                .placeholder("Preset")
                .width(Length::Fixed(220.0)),
        );
    }

    let progress: Element<'_, Message> = if loading {
        container(text("Loading...").size(10))
            .padding([2, 6])
            .style(|_theme: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.18, 0.20, 0.25))),
                border: Border {
                    color: Color::from_rgb(0.36, 0.44, 0.58),
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..container::Style::default()
            })
            .into()
    } else {
        container(text("").size(10))
            .height(Length::Fixed(18.0))
            .into()
    };

    let mut log_column = column![].spacing(2).width(Length::Fill);
    for line in log.iter().rev().take(8).rev() {
        log_column = log_column.push(text(line.clone()).size(10));
    }

    panel_no_title(
        column![
            row![
                column![
                    text(title).size(14),
                    text(detail).size(10).style(move |_theme: &Theme| {
                        if is_error {
                            text::Style {
                                color: Some(Color::from_rgb(1.0, 0.38, 0.34)),
                            }
                        } else {
                            text::Style {
                                color: Some(Color::from_rgb(0.66, 0.68, 0.74)),
                            }
                        }
                    }),
                ]
                .spacing(2)
                .width(Length::Fill),
                progress,
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            controls,
            scrollable(log_column).height(Length::Fixed(58.0)),
        ]
        .spacing(8)
        .into(),
    )
}

fn path_display(path: Option<&PathBuf>) -> String {
    path.and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| String::from("Instrument"))
}

fn knob_row<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut r = row![]
        .spacing(6)
        .align_y(Alignment::Center)
        .width(Length::Shrink);
    for item in items {
        r = r.push(item);
    }
    r.into()
}

fn knob_column<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut c = column![].spacing(4).align_x(Alignment::Start);
    for item in items {
        c = c.push(item);
    }
    c.into()
}

fn zone_knob<'a>(
    label: &'static str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    field: ZoneEditField,
    value_text: String,
) -> Element<'a, Message> {
    let knob = ArchSlider::new(range, value, move |v| {
        Message::SetEditingZoneValue(field, v)
    })
    .step(step)
    .width(Length::Fixed(41.0))
    .height(Length::Fixed(41.0))
    .fill_from_start();

    knob_field(label, knob, value_text)
}

fn zone_range_knob<'a>(
    label: &'static str,
    low: f32,
    high: f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    field: ZoneRangeField,
    value_text: String,
) -> Element<'a, Message> {
    range_knob(
        RangeKnob {
            label: label.into(),
            low,
            high,
            range,
            step,
            value_text,
            assigned: false,
        },
        move |low, high| Message::SetEditingZoneRange(field, low, high),
        Message::None,
    )
}

fn reverse_toggler(reverse: bool) -> Element<'static, Message> {
    let toggler = horizontal_multi_toggler(["Normal", "Reverse"], usize::from(reverse), |index| {
        Message::ToggleEditingZoneReverse(index == 1)
    })
    .label_position(LabelPosition::Below);
    column![text("Direction").size(11), toggler]
        .spacing(2)
        .into()
}

fn cc_mod_amount(route: &ModRoute) -> f32 {
    match route.target {
        ModTarget::Amplitude | ModTarget::Pan | ModTarget::Pitch | ModTarget::FilterResonance => {
            route.depth * 100.0
        }
        ModTarget::FilterCutoff => route.depth * 1200.0,
        ModTarget::SampleOffset => route.depth,
        ModTarget::None | ModTarget::SampleStart | ModTarget::Delay => route.depth,
    }
}

fn cc_mod_depth_from_amount(target: ModTarget, amount: f32) -> f32 {
    match target {
        ModTarget::Amplitude | ModTarget::Pan | ModTarget::Pitch | ModTarget::FilterResonance => {
            amount / 100.0
        }
        ModTarget::FilterCutoff => amount / 1200.0,
        ModTarget::SampleOffset => amount,
        ModTarget::None | ModTarget::SampleStart | ModTarget::Delay => amount,
    }
}

fn cc_mod_amount_range(target: ModTarget) -> std::ops::RangeInclusive<f32> {
    match target {
        ModTarget::Pitch | ModTarget::FilterCutoff => -1200.0..=1200.0,
        ModTarget::SampleOffset => -48_000.0..=48_000.0,
        _ => -100.0..=100.0,
    }
}

fn cc_mod_amount_text(target: ModTarget, amount: f32) -> String {
    match target {
        ModTarget::Pitch | ModTarget::FilterCutoff => format!("{amount:.0} c"),
        ModTarget::SampleOffset => format!("{amount:.0} fr"),
        _ => format!("{amount:.1}"),
    }
}

fn fade_pair_value(pair: Option<(u8, u8)>, fallback: (u8, u8)) -> (u8, u8) {
    pair.unwrap_or(fallback)
}

fn fade_pair_text(pair: Option<(u8, u8)>) -> String {
    match pair {
        Some((low, high)) => format!("{low} - {high}"),
        None => String::from("off"),
    }
}

fn cc_mod_route_row<'a>(index: usize, route: &ModRoute) -> Element<'a, Message> {
    let target = CcModTargetOption::from_target(route.target);
    let curve = CcCurveOption::from_curve(&route.source_curve);
    let amount = cc_mod_amount(route);
    let amount_range = cc_mod_amount_range(route.target);
    let amount_text = cc_mod_amount_text(route.target, amount);
    let show_cc = route.source == ModSource::MidiCc;
    let cc_knob: Element<'a, Message> = if show_cc {
        knob_field(
            "CC",
            ArchSlider::new(0.0..=127.0, route.source_cc as f32, move |value| {
                Message::SetEditingZoneCcRouteCc(index, value)
            })
            .step(1.0)
            .width(Length::Fixed(41.0))
            .height(Length::Fixed(41.0))
            .fill_from_start(),
            route.source_cc.to_string(),
        )
    } else {
        container(text("").size(10))
            .width(Length::Fixed(50.0))
            .into()
    };
    let amount_knob = knob_field(
        "Depth",
        ArchSlider::new(amount_range, amount, move |value| {
            Message::SetEditingZoneCcRouteDepth(index, value)
        })
        .step(1.0)
        .width(Length::Fixed(41.0))
        .height(Length::Fixed(41.0))
        .fill_from_start(),
        amount_text,
    );

    row![
        pick_list(
            ModSource::all_routable(),
            Some(route.source),
            move |source| { Message::SetEditingZoneCcRouteSource(index, source) },
        )
        .width(Length::Fixed(110.0)),
        cc_knob,
        labeled_toggler(
            "Target",
            CcModTargetOption::all().to_vec(),
            target.unwrap_or(CcModTargetOption::Volume),
            move |target| Message::SetEditingZoneCcRouteTarget(index, target),
        ),
        labeled_toggler(
            "Curve",
            CcCurveOption::all().to_vec(),
            curve,
            move |curve| Message::SetEditingZoneCcRouteCurve(index, curve),
        ),
        amount_knob,
        button(text("Del").size(10))
            .padding([3, 6])
            .on_press(Message::RemoveEditingZoneCcRoute(index)),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

fn cc_mod_routes_controls<'a>(zone: &SampleZone) -> Element<'a, Message> {
    let mut route_elements: Vec<Element<'a, Message>> = Vec::new();
    for (index, route) in zone.mod_matrix.routes.iter().enumerate() {
        if route.active && CcModTargetOption::from_target(route.target).is_some() {
            route_elements.push(panel_no_title(cc_mod_route_row(index, route)));
        }
    }
    let routes: Element<'a, Message> = if route_elements.is_empty() {
        text("No mod routes")
            .size(10)
            .color(Color::from_rgb(0.58, 0.60, 0.66))
            .into()
    } else {
        Wrap::with_elements(route_elements)
            .spacing(6)
            .line_spacing(6)
            .into()
    };

    column![
        row![
            text("Mod Matrix")
                .size(10)
                .color(Color::from_rgb(0.68, 0.70, 0.76)),
            button(text("Add").size(10))
                .padding([3, 8])
                .on_press(Message::AddEditingZoneCcRoute),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        routes,
    ]
    .spacing(4)
    .into()
}

fn cc_condition_row<'a>(index: usize, condition: &CcCondition) -> Element<'a, Message> {
    let cc_knob = knob_field(
        "CC",
        ArchSlider::new(0.0..=127.0, condition.cc as f32, move |value| {
            Message::SetEditingZoneCcConditionCc(index, value)
        })
        .step(1.0)
        .width(Length::Fixed(41.0))
        .height(Length::Fixed(41.0))
        .fill_from_start(),
        condition.cc.to_string(),
    );
    let range_knob = range_knob(
        RangeKnob {
            label: "Lo - Hi".into(),
            low: condition.low as f32,
            high: condition.high as f32,
            range: 0.0..=127.0,
            step: 1.0,
            value_text: format!("{} - {}", condition.low, condition.high),
            assigned: false,
        },
        move |low, high| Message::SetEditingZoneCcConditionRange(index, low, high),
        Message::None,
    );

    row![
        cc_knob,
        range_knob,
        button(text("Del").size(10))
            .padding([3, 6])
            .on_press(Message::RemoveEditingZoneCcCondition(index)),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

fn cc_conditions_controls<'a>(zone: &SampleZone) -> Element<'a, Message> {
    let conditions: Element<'a, Message> = if zone.cc_conditions.is_empty() {
        text("No CC conditions")
            .size(10)
            .color(Color::from_rgb(0.58, 0.60, 0.66))
            .into()
    } else {
        Wrap::with_elements(
            zone.cc_conditions
                .iter()
                .enumerate()
                .map(|(index, condition)| panel_no_title(cc_condition_row(index, condition)))
                .collect(),
        )
        .spacing(6)
        .line_spacing(6)
        .into()
    };

    column![
        row![
            text("CC Conditions")
                .size(10)
                .color(Color::from_rgb(0.68, 0.70, 0.76)),
            button(text("Add").size(10))
                .padding([3, 8])
                .on_press(Message::AddEditingZoneCcCondition),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        conditions,
    ]
    .spacing(4)
    .into()
}

fn editing_zone_controls<'a>(
    state: &'a State,
    zone: &SampleZone,
    sample_frames: usize,
) -> Element<'a, Message> {
    let frame_max = sample_frames
        .max(zone.loop_end)
        .max(zone.start_offset)
        .max(zone.offset_random)
        .max(zone.end_offset)
        .max(zone.loop_crossfade)
        .max(1) as f32;
    let play_mode = ZonePlayModeOption::from_mode(zone.play_mode);
    let key_fade_in = fade_pair_value(
        zone.key_fade_in,
        (
            zone.start_note.min(127) as u8,
            zone.start_note.min(127) as u8,
        ),
    );
    let key_fade_out = fade_pair_value(
        zone.key_fade_out,
        (zone.end_note.min(127) as u8, zone.end_note.min(127) as u8),
    );
    let vel_fade_in = fade_pair_value(zone.vel_fade_in, (zone.vel_low, zone.vel_low));
    let vel_fade_out = fade_pair_value(zone.vel_fade_out, (zone.vel_high, zone.vel_high));

    let velocity = zone_range_knob(
        "Velocity",
        f32::from(zone.vel_low),
        f32::from(zone.vel_high),
        0.0..=127.0,
        1.0,
        ZoneRangeField::Velocity,
        format!("{} - {}", zone.vel_low, zone.vel_high),
    );
    let root = zone_knob(
        "Root",
        zone.root_key as f32,
        0.0..=127.0,
        1.0,
        ZoneEditField::RootKey,
        zone.root_key.to_string(),
    );
    let key_tracking = zone_knob(
        "Key Trk",
        zone.key_tracking * 100.0,
        0.0..=200.0,
        1.0,
        ZoneEditField::KeyTracking,
        format!("{:.0}%", zone.key_tracking * 100.0),
    );
    let bend = zone_range_knob(
        "Bend",
        zone.pitch_bend_down.min(zone.pitch_bend_up),
        zone.pitch_bend_down.max(zone.pitch_bend_up),
        0.0..=12000.0,
        1.0,
        ZoneRangeField::PitchBend,
        format!("{:.0} - {:.0} c", zone.pitch_bend_down, zone.pitch_bend_up),
    );

    let gain = zone_knob(
        "Gain",
        zone.gain_db,
        -96.0..=24.0,
        0.1,
        ZoneEditField::GainDb,
        format!("{:.1} dB", zone.gain_db),
    );
    let pan = zone_knob(
        "Pan",
        zone.pan * 100.0,
        -100.0..=100.0,
        1.0,
        ZoneEditField::Pan,
        format!("{:.0}", zone.pan * 100.0),
    );
    let width = zone_knob(
        "Width",
        zone.width * 100.0,
        0.0..=200.0,
        1.0,
        ZoneEditField::Width,
        format!("{:.0}", zone.width * 100.0),
    );
    let position = zone_knob(
        "Position",
        zone.position * 100.0,
        -100.0..=100.0,
        1.0,
        ZoneEditField::Position,
        format!("{:.0}", zone.position * 100.0),
    );
    let amp_key = zone_knob(
        "Amp Key",
        zone.amp_keytrack_db,
        -12.0..=12.0,
        0.1,
        ZoneEditField::AmpKeytrack,
        format!("{:.1}", zone.amp_keytrack_db),
    );
    let vel_track = zone_knob(
        "Vel Track",
        zone.amp_veltrack,
        -100.0..=100.0,
        1.0,
        ZoneEditField::AmpVeltrack,
        format!("{:.0}", zone.amp_veltrack),
    );
    let tune = zone_knob(
        "Tune",
        zone.pitch_offset,
        -1200.0..=1200.0,
        1.0,
        ZoneEditField::PitchOffset,
        format!("{:.0} c", zone.pitch_offset),
    );
    let offset = zone_knob(
        "Offset",
        zone.start_offset as f32,
        0.0..=frame_max,
        1.0,
        ZoneEditField::StartOffset,
        zone.start_offset.to_string(),
    );
    let offset_r = zone_knob(
        "Offset R",
        zone.offset_random as f32,
        0.0..=frame_max,
        1.0,
        ZoneEditField::OffsetRandom,
        zone.offset_random.to_string(),
    );
    let end = zone_knob(
        "End",
        zone.end_offset as f32,
        0.0..=frame_max,
        1.0,
        ZoneEditField::EndOffset,
        if zone.end_offset == 0 {
            String::from("full")
        } else {
            zone.end_offset.to_string()
        },
    );
    let trigger = row![
        labeled_toggler(
            "Trigger",
            ZonePlayModeOption::all().to_vec(),
            play_mode,
            Message::SetEditingZonePlayMode,
        ),
        reverse_toggler(zone.reverse),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let fades = row![
        zone_range_knob(
            "Key In",
            key_fade_in.0 as f32,
            key_fade_in.1 as f32,
            0.0..=127.0,
            1.0,
            ZoneRangeField::KeyFadeIn,
            fade_pair_text(zone.key_fade_in),
        ),
        zone_range_knob(
            "Key Out",
            key_fade_out.0 as f32,
            key_fade_out.1 as f32,
            0.0..=127.0,
            1.0,
            ZoneRangeField::KeyFadeOut,
            fade_pair_text(zone.key_fade_out),
        ),
        zone_range_knob(
            "Vel In",
            vel_fade_in.0 as f32,
            vel_fade_in.1 as f32,
            0.0..=127.0,
            1.0,
            ZoneRangeField::VelFadeIn,
            fade_pair_text(zone.vel_fade_in),
        ),
        zone_range_knob(
            "Vel Out",
            vel_fade_out.0 as f32,
            vel_fade_out.1 as f32,
            0.0..=127.0,
            1.0,
            ZoneRangeField::VelFadeOut,
            fade_pair_text(zone.vel_fade_out),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Start);

    let conditions_section = panel_fill(
        "Conditions",
        column![
            row![
                zone_range_knob(
                    "Channel",
                    f32::from(zone.channel_low),
                    f32::from(zone.channel_high),
                    1.0..=16.0,
                    1.0,
                    ZoneRangeField::Channel,
                    format!("{} - {}", zone.channel_low, zone.channel_high),
                ),
                zone_range_knob(
                    "Bend In",
                    f32::from(zone.pitch_bend_low),
                    f32::from(zone.pitch_bend_high),
                    -8192.0..=8192.0,
                    1.0,
                    ZoneRangeField::PitchBendRange,
                    format!("{} - {}", zone.pitch_bend_low, zone.pitch_bend_high),
                ),
                zone_range_knob(
                    "Random",
                    zone.random_low,
                    zone.random_high,
                    0.0..=1.0,
                    0.01,
                    ZoneRangeField::Random,
                    format!("{:.2} - {:.2}", zone.random_low, zone.random_high),
                ),
                zone_knob(
                    "Seq Len",
                    zone.seq_length as f32,
                    0.0..=128.0,
                    1.0,
                    ZoneEditField::SeqLength,
                    zone.seq_length.to_string(),
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Start),
            row![
                zone_knob(
                    "Seq Pos",
                    zone.seq_position as f32,
                    0.0..=128.0,
                    1.0,
                    ZoneEditField::SeqPosition,
                    zone.seq_position.to_string(),
                ),
                zone_knob(
                    "Off By",
                    zone.off_by as f32,
                    0.0..=127.0,
                    1.0,
                    ZoneEditField::OffBy,
                    zone.off_by.to_string(),
                ),
                zone_knob(
                    "Count",
                    zone.count as f32,
                    0.0..=128.0,
                    1.0,
                    ZoneEditField::Count,
                    zone.count.to_string(),
                ),
                labeled_toggler(
                    "Off Mode",
                    ZoneOffModeOption::all().to_vec(),
                    ZoneOffModeOption::from_mode(zone.off_mode),
                    Message::SetEditingZoneOffMode,
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Start),
            row![{
                let names = state.shared.output_names.load();
                let options = output_bus_options(&names, zone.output);
                let selected = options
                    .iter()
                    .find(|option| option.index == zone.output as usize)
                    .cloned();
                output_bus_picker("Output", options, selected, |option| {
                    Message::SetEditingZoneValue(ZoneEditField::Output, option.index as f32)
                })
            }]
            .spacing(8)
            .align_y(Alignment::Start),
        ]
        .spacing(8)
        .into(),
    );

    let advanced_section = panel_fill(
        "Advanced",
        scrollable(
            column![
                cc_conditions_controls(zone),
                cc_mod_routes_controls(zone),
                text("SFZ")
                    .size(10)
                    .color(Color::from_rgb(0.68, 0.70, 0.76)),
                text_input("opcode=value ...", &state.extra_sfz_opcode_text)
                    .on_input(Message::SetEditingZoneExtraSfz)
                    .size(10)
                    .width(Length::Fill),
            ]
            .spacing(6)
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .into(),
    );

    let range_section = panel_fill(
        "Range",
        column![
            row![velocity, root, key_tracking, bend]
                .spacing(8)
                .align_y(Alignment::Start),
            fades,
            labeled_toggler(
                "Vel Curve",
                ZoneCurveOption::all().to_vec(),
                ZoneCurveOption::from_curve(zone.velocity_curve),
                Message::SetEditingZoneVelocityCurve,
            ),
        ]
        .spacing(8)
        .into(),
    );
    let playback_section = panel(
        "Playback",
        column![
            row![gain, pan, width, position]
                .spacing(8)
                .align_y(Alignment::Start),
            row![amp_key, vel_track, tune, offset]
                .spacing(8)
                .align_y(Alignment::Start),
            row![offset_r, end, trigger]
                .spacing(8)
                .align_y(Alignment::Start),
        ]
        .spacing(8)
        .into(),
    );

    panel_no_title(
        column![
            row![
                range_section,
                playback_section,
                conditions_section,
                advanced_section
            ]
            .spacing(12)
            .align_y(Alignment::Start),
        ]
        .spacing(6)
        .into(),
    )
}

fn sampler_editor_view<'a>(state: &'a State, index: usize) -> Element<'a, Message> {
    let zones = state.shared.zones.load();
    let zone = zones.get(index).cloned().unwrap_or_else(|| {
        SampleZone::new_basic(String::new(), Vec::new(), 0, 0, 0, 0, String::new())
    });

    let header = row![
        button(text("← Back").size(11))
            .on_press(Message::CloseSamplerEditor)
            .padding([4, 8]),
        text(zone.name.clone()).size(14),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let editor_element = state
        .audio_editor
        .as_ref()
        .map(|editor| {
            maolan_editor::app::embedded_view_without_vu_meter(editor)
                .map(|editor_msg| Message::AudioEditor(Box::new(editor_msg)))
        })
        .unwrap_or_else(|| {
            container(text("No sample loaded"))
                .height(Length::Fill)
                .into()
        });
    let sample_frames = patch_zone_sample(state, index)
        .map(|sample| sample.frames)
        .unwrap_or(0);
    let controls = editing_zone_controls(state, &zone, sample_frames);

    let main_content = column![header, controls, editor_element]
        .spacing(10)
        .height(Length::Fill)
        .align_x(Alignment::Start);

    let mut content_row = row![]
        .spacing(8)
        .height(Length::Fill)
        .align_y(Alignment::Start);
    if state.zones_visible {
        content_row = content_row
            .push(zones_panel(state))
            .push(resize_handle(SidePanel::Zones));
    }
    content_row = content_row.push(main_content).push(sampler_vu_meter(state));
    if state.browser_visible {
        content_row = content_row
            .push(resize_handle(SidePanel::Browser))
            .push(browser_panel(state));
    }

    let content = mouse_area(container(content_row).padding(16).height(Length::Fill))
        .on_move(|Point { x, .. }| Message::ResizeSidePanel(x))
        .on_release(Message::PointerReleased);

    container(content)
        .height(Length::Fill)
        .align_x(Horizontal::Left)
        .align_y(Vertical::Top)
        .into()
}

fn group_editor_view<'a>(state: &'a State, group_name: &str) -> Element<'a, Message> {
    let groups = state.shared.groups.load();

    let title: Element<'a, Message> = if let Some(rename_text) = state.renaming_group.clone() {
        text_input("Group name", &rename_text)
            .on_input(Message::UpdateRenameGroupText)
            .on_submit(Message::FinishRenameGroup)
            .size(13)
            .width(Length::Fixed(180.0))
            .into()
    } else {
        row![
            text(group_name.to_string()).size(14),
            button(text("✎").size(9))
                .on_press(Message::StartRenameGroup)
                .padding([2, 4]),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    };

    let header = row![
        button(text("← Back").size(11))
            .on_press(Message::CloseGroupEditor)
            .padding([4, 8]),
        title,
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let controls: Element<'a, Message> = groups
        .iter()
        .find(|group| group.name == group_name)
        .map(|group| group_controls_panel(state, group))
        .unwrap_or_else(|| text("Group not found").size(12).into());

    let main_content = column![header, controls]
        .spacing(10)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Start);

    let mut content_row = row![]
        .spacing(8)
        .height(Length::Fill)
        .align_y(Alignment::Start);
    if state.zones_visible {
        content_row = content_row
            .push(zones_panel(state))
            .push(resize_handle(SidePanel::Zones));
    }
    content_row = content_row.push(main_content).push(sampler_vu_meter(state));
    if state.browser_visible {
        content_row = content_row
            .push(resize_handle(SidePanel::Browser))
            .push(browser_panel(state));
    }

    let content = mouse_area(container(content_row).padding(16).height(Length::Fill))
        .on_move(|Point { x, .. }| Message::ResizeSidePanel(x))
        .on_release(Message::PointerReleased);

    container(content)
        .height(Length::Fill)
        .align_x(Horizontal::Left)
        .align_y(Vertical::Top)
        .into()
}

fn view(state: &State) -> Element<'_, Message> {
    if let Some(group_name) = state.editing_group_name.clone() {
        return group_editor_view(state, &group_name);
    }
    if let Some(index) = state.editing_zone_index {
        return sampler_editor_view(state, index);
    }

    let sample_map = fill_panel_no_title(sample_map(state));

    let bend_down = state.shared.params.get(ParamId::PitchBendDown) as f32;
    let bend_up = state.shared.params.get(ParamId::PitchBendUp) as f32;
    let bend_range = range_knob(
        RangeKnob {
            label: "Bend".into(),
            low: bend_down.min(bend_up),
            high: bend_down.max(bend_up),
            range: 0.0..=12000.0,
            step: 1.0,
            value_text: format!("{bend_down:.0} - {bend_up:.0} c"),
            assigned: false,
        },
        Message::SetPitchBendRange,
        Message::None,
    );

    let filter1: Element<'_, Message> = column![
        filter_type_dropdown(ParamId::FilterType, state),
        knob_row(vec![
            filter_selector_dropdown(state),
            param_control(ParamId::FilterSubtype, "Sub", state),
            param_control(ParamId::FilterCutoff, "Cut", state),
            param_control(ParamId::FilterResonance, "Res", state),
            param_control(ParamId::FilterEgAmount, "EG", state),
            param_control(ParamId::FilterKeyTrack, "Key", state),
            param_control(ParamId::FilterDrive, "Drive", state),
            param_control(ParamId::FilterEnabled, "On", state),
        ]),
    ]
    .spacing(6)
    .into();

    let filter2: Element<'_, Message> = column![
        filter_type_dropdown(ParamId::Filter2Type, state),
        knob_row(vec![
            filter_selector_dropdown(state),
            param_control(ParamId::Filter2Subtype, "Sub", state),
            param_control(ParamId::Filter2Cutoff, "Cut", state),
            param_control(ParamId::Filter2Resonance, "Res", state),
            param_control(ParamId::Filter2EgAmount, "EG", state),
            param_control(ParamId::Filter2KeyTrack, "Key", state),
            param_control(ParamId::Filter2Drive, "Drive", state),
            param_control(ParamId::Filter2Enabled, "On", state),
        ]),
    ]
    .spacing(6)
    .into();

    let selected_filter_content = match state.selected_filter {
        0 => filter1,
        _ => filter2,
    };

    let amp_eg = adsr_panel(
        state,
        [
            ParamId::AmpAttack,
            ParamId::AmpDecay,
            ParamId::AmpSustain,
            ParamId::AmpRelease,
        ],
    );

    let filter_eg = adsr_panel(
        state,
        [
            ParamId::FilterAttack,
            ParamId::FilterDecay,
            ParamId::FilterSustain,
            ParamId::FilterRelease,
        ],
    );

    let pitch_eg = adsr_panel(
        state,
        [
            ParamId::Eg2Attack,
            ParamId::Eg2Decay,
            ParamId::Eg2Sustain,
            ParamId::Eg2Release,
        ],
    );

    let selected_eg =
        EgSelectorOption::from_index(state.selected_eg).unwrap_or(EgSelectorOption::Amp);
    let eg_options = EgSelectorOption::all().to_vec();
    let eg_labels = eg_options
        .iter()
        .map(|option| option.label().to_string())
        .collect();
    let eg_selector = option_toggler(eg_options, selected_eg.index(), eg_labels, |option| {
        Message::SelectEg(option.index())
    });

    let eg3 = adsr_panel(
        state,
        [
            ParamId::Eg3Attack,
            ParamId::Eg3Decay,
            ParamId::Eg3Sustain,
            ParamId::Eg3Release,
        ],
    );

    let eg4 = adsr_panel(
        state,
        [
            ParamId::Eg4Attack,
            ParamId::Eg4Decay,
            ParamId::Eg4Sustain,
            ParamId::Eg4Release,
        ],
    );

    let eg5 = adsr_panel(
        state,
        [
            ParamId::Eg5Attack,
            ParamId::Eg5Decay,
            ParamId::Eg5Sustain,
            ParamId::Eg5Release,
        ],
    );

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

    let lfo_panel = knob_row(vec![
        lfo_selector_dropdown(state),
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
    ]);

    let master_panel = panel_no_title(
        column![
            knob_row(vec![
                param_control(ParamId::MasterGain, "Gain", state),
                param_control(ParamId::MasterPan, "Pan", state),
                bend_range,
            ]),
            eg_selector,
            selected_eg_panel,
        ]
        .spacing(10)
        .into(),
    );

    let filter_section = panel_no_title(
        column![selected_filter_content, lfo_panel]
            .spacing(10)
            .width(Length::Shrink)
            .into(),
    );

    let top_row = row![master_panel, filter_section, outputs_panel(state)]
        .spacing(10)
        .align_y(Alignment::Start);

    let main_content = column![instrument_panel(state), sample_map,]
        .spacing(12)
        .height(Length::Fill)
        .align_x(Alignment::Start);
    let main_content = main_content
        .push(top_row)
        .spacing(12)
        .height(Length::Fill)
        .align_x(Alignment::Start);

    let mut content_row = row![]
        .spacing(8)
        .height(Length::Fill)
        .align_y(Alignment::Start);
    if state.zones_visible {
        content_row = content_row
            .push(zones_panel(state))
            .push(resize_handle(SidePanel::Zones));
    }
    content_row = content_row.push(main_content).push(sampler_vu_meter(state));
    if state.browser_visible {
        content_row = content_row
            .push(resize_handle(SidePanel::Browser))
            .push(browser_panel(state));
    }

    let content = column![content_row]
        .spacing(4)
        .height(Length::Fill)
        .align_x(Alignment::Start);
    let content = mouse_area(container(content).padding(16).height(Length::Fill))
        .on_move(|Point { x, .. }| Message::ResizeSidePanel(x))
        .on_release(Message::PointerReleased);

    container(content)
        .height(Length::Fill)
        .align_x(Horizontal::Left)
        .align_y(Vertical::Top)
        .into()
}

fn theme(_state: &State) -> Theme {
    Theme::TokyoNight
}

fn panel_shortcuts(
    event: maolan_baseview::iced::Event,
    status: maolan_baseview::iced::event::Status,
    _id: maolan_baseview::iced::window::Id,
) -> Option<Message> {
    if status == maolan_baseview::iced::event::Status::Captured {
        return None;
    }
    if let maolan_baseview::iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key,
        modifiers,
        ..
    }) = event
        && !modifiers.command()
        && !modifiers.control()
        && !modifiers.alt()
    {
        if matches!(key, keyboard::Key::Character(ref c) if c.eq_ignore_ascii_case("z")) {
            return Some(Message::ToggleZonesPanel);
        }
        if matches!(key, keyboard::Key::Character(ref c) if c.eq_ignore_ascii_case("b")) {
            return Some(Message::ToggleBrowserPanel);
        }
    }
    None
}

fn sampler_shortcuts(
    event: maolan_baseview::iced::Event,
    status: maolan_baseview::iced::event::Status,
    _id: maolan_baseview::iced::window::Id,
) -> Option<Message> {
    if status == maolan_baseview::iced::event::Status::Captured {
        return None;
    }
    if let maolan_baseview::iced::Event::Window(window::Event::FileDropped(path)) = &event
        && is_instrument_file(path)
    {
        return Some(Message::LoadInstrument(path.clone()));
    }
    if let maolan_baseview::iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key,
        modifiers,
        ..
    }) = event
    {
        let is_undo = matches!(
            key,
            keyboard::Key::Character(ref c)
                if c.eq_ignore_ascii_case("z") && modifiers.command()
        );
        let is_delete = matches!(key, keyboard::Key::Named(keyboard::key::Named::Delete));
        let is_esc = matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape));
        if is_undo {
            return Some(Message::Undo);
        }
        if is_delete {
            return Some(Message::DeleteSelectedZone);
        }
        if is_esc {
            return Some(Message::CloseSamplerEditor);
        }
    }
    None
}

fn subscription(state: &State) -> maolan_baseview::iced::Subscription<Message> {
    let editor_sub = state.audio_editor.as_ref().map(|editor| {
        maolan_editor::app::subscription(editor)
            .map(|editor_msg| Message::AudioEditor(Box::new(editor_msg)))
    });
    let panel_shortcuts_sub = (state.editing_zone_name.is_none())
        .then(|| maolan_baseview::iced::event::listen_with(panel_shortcuts));
    let full_shortcuts_sub = (state.editing_zone_name.is_none() && state.audio_editor.is_none())
        .then(|| maolan_baseview::iced::event::listen_with(sampler_shortcuts));
    let status_poll = maolan_baseview::iced::time::every(Duration::from_millis(120))
        .map(|_| Message::PollLoadStatus);
    let mut subscriptions = vec![status_poll];
    if let Some(sub) = panel_shortcuts_sub {
        subscriptions.push(sub);
    }
    if let Some(sub) = full_shortcuts_sub {
        subscriptions.push(sub);
    }
    if let Some(editor_sub) = editor_sub {
        subscriptions.push(editor_sub);
    }
    maolan_baseview::iced::Subscription::batch(subscriptions)
}

fn build_app(shared: Arc<SharedState>) -> impl maolan_baseview::iced::Program {
    maolan_baseview::iced::application(move || init(shared.clone()), update, view)
        .font(maolan_widgets::iced_fonts::LUCIDE_FONT_BYTES)
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
