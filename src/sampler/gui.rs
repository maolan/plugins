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
#[cfg(target_os = "windows")]
use maolan_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(unix)]
use maolan_clap::ffi::CLAP_WINDOW_API_X11;
use maolan_widgets::arch_slider::arch_slider;
use maolan_widgets::meters;
use maolan_widgets::piano::{
    Orientation, draw_octave_into, draw_partial_octave_into, note_at_in_range, octave_note_count,
};
use maolan_widgets::slider::Slider;
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
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
        plugin::{SharedState, build_export_patch},
        state::{SampleGroup, SampleZone},
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
    AudioEditor(maolan_editor::app::Message),
    SetEditingZoneValue(ZoneEditField, f32),
    ToggleEditingZoneReverse(bool),
    SetEditingZonePlayMode(ZonePlayModeOption),
    SetEditingZoneLoopMode(ZoneLoopModeOption),
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
    SetEditingZoneCcConditionLow(usize, f32),
    SetEditingZoneCcConditionHigh(usize, f32),
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
    PointerReleased,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneEditField {
    StartNote,
    EndNote,
    VelLow,
    VelHigh,
    RootKey,
    KeyFadeInLow,
    KeyFadeInHigh,
    KeyFadeOutLow,
    KeyFadeOutHigh,
    VelFadeInLow,
    VelFadeInHigh,
    VelFadeOutLow,
    VelFadeOutHigh,
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
    LoopStart,
    LoopEnd,
    LoopCrossfade,
    LoopCount,
    PitchBendUp,
    PitchBendDown,
    ChannelLow,
    ChannelHigh,
    PitchBendLow,
    PitchBendHigh,
    RandomLow,
    RandomHigh,
    SeqLength,
    SeqPosition,
    OffBy,
    Output,
    AmpVeltrack,
    Count,
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
    SwLoLast,
    SwHiLast,
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
pub enum ZoneLoopModeOption {
    Off,
    Continuous,
    Sustain,
}

impl ZoneLoopModeOption {
    fn all() -> [Self; 3] {
        [Self::Off, Self::Continuous, Self::Sustain]
    }

    fn from_mode(mode: LoopMode) -> Self {
        match mode {
            LoopMode::DuringVoice | LoopMode::Count => Self::Continuous,
            LoopMode::WhileGated => Self::Sustain,
            LoopMode::Off => Self::Off,
        }
    }

    fn mode(self) -> LoopMode {
        match self {
            Self::Off => LoopMode::Off,
            Self::Continuous => LoopMode::DuringVoice,
            Self::Sustain => LoopMode::WhileGated,
        }
    }
}

impl std::fmt::Display for ZoneLoopModeOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Off => write!(f, "Off"),
            Self::Continuous => write!(f, "Continuous"),
            Self::Sustain => write!(f, "Sustain"),
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
    resize_last_x: Option<f32>,
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

fn set_pair_low(pair: &mut Option<(u8, u8)>, low: u8, default: (u8, u8)) {
    let (_, high) = pair.unwrap_or(default);
    *pair = Some((low.min(high), high.max(low)));
}

fn set_pair_high(pair: &mut Option<(u8, u8)>, high: u8, default: (u8, u8)) {
    let (low, _) = pair.unwrap_or(default);
    *pair = Some((low.min(high), high.max(low)));
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
            zones_width: 138.0,
            browser_width: 138.0,
            resizing_side: None,
            resize_last_x: None,
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
            Message::ToggleZonesPanel => {
                state.zones_visible = !state.zones_visible;
                if !state.zones_visible && state.resizing_side == Some(SidePanel::Zones) {
                    state.resizing_side = None;
                    state.resize_last_x = None;
                }
            }
            Message::ToggleBrowserPanel => {
                state.browser_visible = !state.browser_visible;
                if !state.browser_visible && state.resizing_side == Some(SidePanel::Browser) {
                    state.resizing_side = None;
                    state.resize_last_x = None;
                }
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
                                state.browser_width =
                                    (state.browser_width - delta).clamp(92.0, 260.0);
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
                state.audio_editor = Some(editor);
                state.audio_editor_path = zones
                    .get(index)
                    .and_then(|zone| zone.files.first().cloned());
                return open_task.map(Message::AudioEditor);
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
            Message::AudioEditor(msg) => {
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
                        let editor_task = state
                            .audio_editor
                            .as_mut()
                            .map(|editor| {
                                maolan_editor::app::update(
                                    editor,
                                    maolan_editor::app::Message::Play,
                                )
                                .map(Message::AudioEditor)
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
                                if let Some(duration) = duration {
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
                                .map(Message::AudioEditor)
                            })
                            .unwrap_or_else(Task::none);
                        break 'task editor_task;
                    }
                    _ => {}
                }
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
                        .map(Message::AudioEditor);
                        reload_zone_sample(state, &path);
                        task
                    }
                    _ => maolan_editor::app::update(editor, msg).map(Message::AudioEditor),
                };
                break 'task task;
            }
            Message::SetEditingZoneValue(field, value) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index) {
                        match field {
                            ZoneEditField::StartNote => {
                                zone.start_note = (value.round() as usize).clamp(0, zone.end_note);
                            }
                            ZoneEditField::EndNote => {
                                zone.end_note =
                                    (value.round() as usize).clamp(zone.start_note, 127);
                            }
                            ZoneEditField::VelLow => {
                                zone.vel_low = (value.round() as u8).min(zone.vel_high);
                            }
                            ZoneEditField::VelHigh => {
                                zone.vel_high = (value.round() as u8).max(zone.vel_low).min(127);
                            }
                            ZoneEditField::RootKey => {
                                zone.root_key = (value.round() as u8).min(127);
                            }
                            ZoneEditField::KeyFadeInLow => {
                                set_pair_low(
                                    &mut zone.key_fade_in,
                                    clamp_u7(value),
                                    (
                                        zone.start_note.min(127) as u8,
                                        zone.start_note.min(127) as u8,
                                    ),
                                );
                                if let Some((low, high)) = zone.key_fade_in {
                                    zone.key_fade_low = high.saturating_sub(low);
                                }
                            }
                            ZoneEditField::KeyFadeInHigh => {
                                set_pair_high(
                                    &mut zone.key_fade_in,
                                    clamp_u7(value),
                                    (
                                        zone.start_note.min(127) as u8,
                                        zone.start_note.min(127) as u8,
                                    ),
                                );
                                if let Some((low, high)) = zone.key_fade_in {
                                    zone.key_fade_low = high.saturating_sub(low);
                                }
                            }
                            ZoneEditField::KeyFadeOutLow => {
                                set_pair_low(
                                    &mut zone.key_fade_out,
                                    clamp_u7(value),
                                    (zone.end_note.min(127) as u8, zone.end_note.min(127) as u8),
                                );
                                if let Some((low, high)) = zone.key_fade_out {
                                    zone.key_fade_high = high.saturating_sub(low);
                                }
                            }
                            ZoneEditField::KeyFadeOutHigh => {
                                set_pair_high(
                                    &mut zone.key_fade_out,
                                    clamp_u7(value),
                                    (zone.end_note.min(127) as u8, zone.end_note.min(127) as u8),
                                );
                                if let Some((low, high)) = zone.key_fade_out {
                                    zone.key_fade_high = high.saturating_sub(low);
                                }
                            }
                            ZoneEditField::VelFadeInLow => {
                                set_pair_low(
                                    &mut zone.vel_fade_in,
                                    clamp_u7(value),
                                    (zone.vel_low, zone.vel_low),
                                );
                                if let Some((low, high)) = zone.vel_fade_in {
                                    zone.vel_fade_low = high.saturating_sub(low);
                                }
                            }
                            ZoneEditField::VelFadeInHigh => {
                                set_pair_high(
                                    &mut zone.vel_fade_in,
                                    clamp_u7(value),
                                    (zone.vel_low, zone.vel_low),
                                );
                                if let Some((low, high)) = zone.vel_fade_in {
                                    zone.vel_fade_low = high.saturating_sub(low);
                                }
                            }
                            ZoneEditField::VelFadeOutLow => {
                                set_pair_low(
                                    &mut zone.vel_fade_out,
                                    clamp_u7(value),
                                    (zone.vel_high, zone.vel_high),
                                );
                                if let Some((low, high)) = zone.vel_fade_out {
                                    zone.vel_fade_high = high.saturating_sub(low);
                                }
                            }
                            ZoneEditField::VelFadeOutHigh => {
                                set_pair_high(
                                    &mut zone.vel_fade_out,
                                    clamp_u7(value),
                                    (zone.vel_high, zone.vel_high),
                                );
                                if let Some((low, high)) = zone.vel_fade_out {
                                    zone.vel_fade_high = high.saturating_sub(low);
                                }
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
                            ZoneEditField::LoopStart => {
                                zone.loop_start = value.max(0.0).round() as usize;
                            }
                            ZoneEditField::LoopEnd => {
                                zone.loop_end = value.max(0.0).round() as usize;
                            }
                            ZoneEditField::LoopCrossfade => {
                                zone.loop_crossfade = value.max(0.0).round() as usize;
                            }
                            ZoneEditField::LoopCount => {
                                zone.loop_count = value.max(0.0).round() as u32;
                            }
                            ZoneEditField::PitchBendUp => {
                                zone.pitch_bend_up = value.clamp(0.0, 12000.0);
                            }
                            ZoneEditField::PitchBendDown => {
                                zone.pitch_bend_down = value.clamp(0.0, 12000.0);
                            }
                            ZoneEditField::ChannelLow => {
                                zone.channel_low =
                                    value.round().clamp(1.0, f32::from(zone.channel_high)) as u8;
                            }
                            ZoneEditField::ChannelHigh => {
                                zone.channel_high =
                                    value.round().clamp(f32::from(zone.channel_low), 16.0) as u8;
                            }
                            ZoneEditField::PitchBendLow => {
                                zone.pitch_bend_low = value
                                    .round()
                                    .clamp(-8192.0, f32::from(zone.pitch_bend_high))
                                    as i16;
                            }
                            ZoneEditField::PitchBendHigh => {
                                zone.pitch_bend_high =
                                    value.round().clamp(f32::from(zone.pitch_bend_low), 8192.0)
                                        as i16;
                            }
                            ZoneEditField::RandomLow => {
                                zone.random_low = value.clamp(0.0, zone.random_high);
                            }
                            ZoneEditField::RandomHigh => {
                                zone.random_high = value.clamp(zone.random_low, 1.0);
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
                                zone.output = value.round().clamp(0.0, 31.0) as u8;
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
                        if matches!(
                            field,
                            ZoneEditField::StartNote
                                | ZoneEditField::EndNote
                                | ZoneEditField::VelLow
                                | ZoneEditField::VelHigh
                        ) {
                            state.shared.note_names_changed();
                        }
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
            Message::SetEditingZoneLoopMode(mode) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index) {
                        zone.loop_mode = mode.mode();
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
            Message::SetEditingZoneCcConditionLow(condition_index, value) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index)
                        && let Some(condition) = zone.cc_conditions.get_mut(condition_index)
                    {
                        condition.low = clamp_u7(value).min(condition.high);
                        state.shared.zones.store(zones);
                        state.shared.bump_zones_version();
                        state.shared.mark_dirty();
                    }
                }
            }
            Message::SetEditingZoneCcConditionHigh(condition_index, value) => {
                if let Some(index) = state.editing_zone_index {
                    let mut zones = state.shared.zones.load();
                    if let Some(zone) = Arc::make_mut(&mut zones).get_mut(index)
                        && let Some(condition) = zone.cc_conditions.get_mut(condition_index)
                    {
                        condition.high = clamp_u7(value).max(condition.low);
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
                                group.output = value.round().clamp(0.0, 31.0) as u8;
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
                            GroupEditField::SwLoLast => {
                                group.sw_lolast = note_option_from_slider(value);
                            }
                            GroupEditField::SwHiLast => {
                                group.sw_hilast = note_option_from_slider(value);
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
                state.resize_last_x = None;
                return update(state, Message::FinishZoneListDrag);
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
    .width(Length::Fixed(41.0))
    .height(Length::Fixed(41.0));
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
    .width(Length::Fixed(50.0))
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
    .width(Length::Fixed(50.0))
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

fn hslider<'a>(id: ParamId, label: &'a str, state: &'a State) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let def = &PARAMS[id.as_index()];
    let slider = Slider::new(def.min as f32..=def.max as f32, value, move |v| {
        Message::SetParam(id, v)
    })
    .step(def.step as f32)
    .double_click_reset(def.default as f32)
    .on_release(Message::ReleaseParam(id))
    .horizontal()
    .width(Length::Fixed(50.0))
    .height(Length::Fixed(14.0));

    container(
        column![
            text(label).size(11),
            row![slider, text(format!("{value:.2}")).size(10)]
                .spacing(4)
                .align_y(Alignment::Center),
        ]
        .spacing(2)
        .align_x(Alignment::Center),
    )
    .width(Length::Fixed(70.0))
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
                        {
                            Some(
                                canvas::Action::publish(Message::OpenSamplerEditor(press.index))
                                    .and_capture(),
                            )
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
        text(zone.name).size(11).into()
    };

    let content = column![
        name_element,
        text(file_name)
            .size(9)
            .color(Color::from_rgb(0.48, 0.50, 0.58)),
        text(range_text)
            .size(9)
            .color(Color::from_rgb(0.48, 0.50, 0.58)),
    ]
    .spacing(1)
    .width(Length::Fill);

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
    .width(Length::Fill)
    .padding([4, 6])
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

fn group_slider<'a>(
    label: &'static str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    field: GroupEditField,
    value_text: String,
) -> Element<'a, Message> {
    let slider = Slider::new(range, value, move |v| {
        Message::SetSelectedGroupValue(field, v)
    })
    .step(step)
    .width(Length::Fixed(120.0))
    .height(Length::Fixed(20.0))
    .horizontal();

    row![text(label).size(10), slider, text(value_text).size(9)]
        .spacing(5)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
}

fn group_param_slider<'a>(
    label: &'static str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    field: GroupParamField,
    value_text: String,
) -> Element<'a, Message> {
    let slider = Slider::new(range, value, move |v| {
        Message::SetSelectedGroupParam(field, v)
    })
    .step(step)
    .width(Length::Fixed(120.0))
    .height(Length::Fixed(20.0))
    .horizontal();

    row![text(label).size(10), slider, text(value_text).size(9)]
        .spacing(5)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
}

fn keyswitch_slider<'a>(
    label: &'static str,
    value: Option<u8>,
    field: GroupEditField,
) -> Element<'a, Message> {
    let slider = Slider::new(
        -1.0..=127.0,
        slider_value_from_note_option(value),
        move |v| Message::SetSelectedGroupValue(field, v),
    )
    .step(1.0)
    .width(Length::Fixed(120.0))
    .height(Length::Fixed(20.0))
    .horizontal();

    row![
        text(label).size(10),
        slider,
        text(note_option_text(value)).size(9)
    ]
    .spacing(5)
    .align_y(Alignment::Center)
    .width(Length::Fill)
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

fn selected_group_controls<'a>(
    state: &'a State,
    groups: &[SampleGroup],
) -> Option<Element<'a, Message>> {
    let group_name = match state.selected.as_ref()? {
        ZoneListSelection::Group(name) => name,
        ZoneListSelection::Zone(_) => return None,
    };
    let group = groups.iter().find(|group| group.name == *group_name)?;

    Some(panel_no_title(
        column![
            text(group.name.clone())
                .size(11)
                .color(Color::from_rgb(0.90, 0.91, 0.94)),
            group_slider(
                "Poly",
                group.poly_limit as f32,
                0.0..=128.0,
                1.0,
                GroupEditField::PolyLimit,
                group.poly_limit.to_string(),
            ),
            group_slider(
                "Excl",
                group.exclusive_group as f32,
                0.0..=255.0,
                1.0,
                GroupEditField::ExclusiveGroup,
                group.exclusive_group.to_string(),
            ),
            group_slider(
                "Gain",
                group.gain_db,
                -96.0..=24.0,
                0.1,
                GroupEditField::GainDb,
                format!("{:.1} dB", group.gain_db),
            ),
            group_slider(
                "Pan",
                group.pan * 100.0,
                -100.0..=100.0,
                1.0,
                GroupEditField::Pan,
                format!("{:.0}", group.pan * 100.0),
            ),
            group_slider(
                "Output",
                group.output as f32,
                0.0..=31.0,
                1.0,
                GroupEditField::Output,
                group.output.to_string(),
            ),
            text("Keyswitches")
                .size(10)
                .color(Color::from_rgb(0.68, 0.70, 0.76)),
            keyswitch_slider("Last", group.sw_last, GroupEditField::SwLast),
            keyswitch_slider("Down", group.sw_down, GroupEditField::SwDown),
            keyswitch_slider("Up", group.sw_up, GroupEditField::SwUp),
            keyswitch_slider("Previous", group.sw_previous, GroupEditField::SwPrevious),
            keyswitch_slider("Lo Last", group.sw_lolast, GroupEditField::SwLoLast),
            keyswitch_slider("Hi Last", group.sw_hilast, GroupEditField::SwHiLast),
            keyswitch_slider("Default", group.sw_default, GroupEditField::SwDefault),
            text_input("sw_label ...", group.sw_label.as_deref().unwrap_or(""))
                .on_input(Message::SetSelectedGroupSwLabel)
                .size(10)
                .width(Length::Fill),
            text("Amp EG")
                .size(10)
                .color(Color::from_rgb(0.68, 0.70, 0.76)),
            group_param_slider(
                "A",
                group.eg1_params.map_or(0.01, |p| p.attack),
                0.0..=10.0,
                0.001,
                GroupParamField::Eg1Attack,
                format!("{:.3}", group.eg1_params.map_or(0.01, |p| p.attack)),
            ),
            group_param_slider(
                "D",
                group.eg1_params.map_or(0.0, |p| p.decay),
                0.0..=10.0,
                0.001,
                GroupParamField::Eg1Decay,
                format!("{:.3}", group.eg1_params.map_or(0.0, |p| p.decay)),
            ),
            group_param_slider(
                "S",
                group.eg1_params.map_or(1.0, |p| p.sustain) * 100.0,
                0.0..=100.0,
                1.0,
                GroupParamField::Eg1Sustain,
                format!("{:.0}", group.eg1_params.map_or(1.0, |p| p.sustain) * 100.0),
            ),
            group_param_slider(
                "R",
                group.eg1_params.map_or(0.05, |p| p.release),
                0.0..=10.0,
                0.001,
                GroupParamField::Eg1Release,
                format!("{:.3}", group.eg1_params.map_or(0.05, |p| p.release)),
            ),
            text("Filter EG")
                .size(10)
                .color(Color::from_rgb(0.68, 0.70, 0.76)),
            group_param_slider(
                "A",
                group.eg2_params.map_or(0.01, |p| p.attack),
                0.0..=10.0,
                0.001,
                GroupParamField::Eg2Attack,
                format!("{:.3}", group.eg2_params.map_or(0.01, |p| p.attack)),
            ),
            group_param_slider(
                "D",
                group.eg2_params.map_or(0.0, |p| p.decay),
                0.0..=10.0,
                0.001,
                GroupParamField::Eg2Decay,
                format!("{:.3}", group.eg2_params.map_or(0.0, |p| p.decay)),
            ),
            group_param_slider(
                "S",
                group.eg2_params.map_or(1.0, |p| p.sustain) * 100.0,
                0.0..=100.0,
                1.0,
                GroupParamField::Eg2Sustain,
                format!("{:.0}", group.eg2_params.map_or(1.0, |p| p.sustain) * 100.0),
            ),
            group_param_slider(
                "R",
                group.eg2_params.map_or(0.05, |p| p.release),
                0.0..=10.0,
                0.001,
                GroupParamField::Eg2Release,
                format!("{:.3}", group.eg2_params.map_or(0.05, |p| p.release)),
            ),
            text("LFOs")
                .size(10)
                .color(Color::from_rgb(0.68, 0.70, 0.76)),
            group_param_slider(
                "Lfo1 Rate",
                group.lfo1_params.map_or(0.0, |p| p.rate),
                0.0..=20.0,
                0.1,
                GroupParamField::Lfo1Rate,
                format!("{:.1}", group.lfo1_params.map_or(0.0, |p| p.rate)),
            ),
            group_param_slider(
                "Lfo1 Amt",
                group.lfo1_params.map_or(0.0, |p| p.amount),
                0.0..=100.0,
                1.0,
                GroupParamField::Lfo1Amount,
                format!("{:.1}", group.lfo1_params.map_or(0.0, |p| p.amount)),
            ),
            row![
                text("Lfo1 Wave").size(11),
                pick_list(
                    LfoShape::all(),
                    Some(group.lfo1_params.map_or(LfoShape::Sine, |p| p.shape)),
                    |shape| Message::SetSelectedGroupLfoShape(1, shape),
                )
                .width(Length::Fixed(120.0)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            group_param_slider(
                "Lfo2 Rate",
                group.lfo2_params.map_or(0.0, |p| p.rate),
                0.0..=20.0,
                0.1,
                GroupParamField::Lfo2Rate,
                format!("{:.1}", group.lfo2_params.map_or(0.0, |p| p.rate)),
            ),
            group_param_slider(
                "Lfo2 Amt",
                group.lfo2_params.map_or(0.0, |p| p.amount),
                0.0..=100.0,
                1.0,
                GroupParamField::Lfo2Amount,
                format!("{:.1}", group.lfo2_params.map_or(0.0, |p| p.amount)),
            ),
            row![
                text("Lfo2 Wave").size(11),
                pick_list(
                    LfoShape::all(),
                    Some(group.lfo2_params.map_or(LfoShape::Sine, |p| p.shape)),
                    |shape| Message::SetSelectedGroupLfoShape(2, shape),
                )
                .width(Length::Fixed(120.0)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            group_param_slider(
                "Lfo3 Rate",
                group.lfo3_params.map_or(0.0, |p| p.rate),
                0.0..=20.0,
                0.1,
                GroupParamField::Lfo3Rate,
                format!("{:.1}", group.lfo3_params.map_or(0.0, |p| p.rate)),
            ),
            group_param_slider(
                "Lfo3 Amt",
                group.lfo3_params.map_or(0.0, |p| p.amount),
                0.0..=100.0,
                1.0,
                GroupParamField::Lfo3Amount,
                format!("{:.1}", group.lfo3_params.map_or(0.0, |p| p.amount)),
            ),
            row![
                text("Lfo3 Wave").size(11),
                pick_list(
                    LfoShape::all(),
                    Some(group.lfo3_params.map_or(LfoShape::Sine, |p| p.shape)),
                    |shape| Message::SetSelectedGroupLfoShape(3, shape),
                )
                .width(Length::Fixed(120.0)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            group_param_slider(
                "Lfo4 Rate",
                group.lfo4_params.map_or(0.0, |p| p.rate),
                0.0..=20.0,
                0.1,
                GroupParamField::Lfo4Rate,
                format!("{:.1}", group.lfo4_params.map_or(0.0, |p| p.rate)),
            ),
            group_param_slider(
                "Lfo4 Amt",
                group.lfo4_params.map_or(0.0, |p| p.amount),
                0.0..=100.0,
                1.0,
                GroupParamField::Lfo4Amount,
                format!("{:.1}", group.lfo4_params.map_or(0.0, |p| p.amount)),
            ),
            row![
                text("Lfo4 Wave").size(11),
                pick_list(
                    LfoShape::all(),
                    Some(group.lfo4_params.map_or(LfoShape::Sine, |p| p.shape)),
                    |shape| Message::SetSelectedGroupLfoShape(4, shape),
                )
                .width(Length::Fixed(120.0)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            text("Filter")
                .size(10)
                .color(Color::from_rgb(0.68, 0.70, 0.76)),
            row![
                text("Type").size(11),
                pick_list(
                    filter_type_options(),
                    Some(
                        group
                            .filter_params
                            .map_or(FilterType::Lowpass, |p| p.filter_type)
                    ),
                    Message::SetSelectedGroupFilterType,
                )
                .width(Length::Fixed(120.0)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            row![
                text("Sub").size(11),
                pick_list(
                    filter_subtype_options(),
                    Some(
                        group
                            .filter_params
                            .map_or(FilterSubtype::Clean, |p| p.subtype)
                    ),
                    Message::SetSelectedGroupFilterSubtype,
                )
                .width(Length::Fixed(120.0)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            group_param_slider(
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
            group_param_slider(
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
            group_param_slider(
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
            group_param_slider(
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
                        .map_or(FilterParams::default().vel_tracking, |p| p.vel_tracking)
                ),
            ),
            text("SFZ")
                .size(10)
                .color(Color::from_rgb(0.68, 0.70, 0.76)),
            text_input("opcode=value ...", &state.extra_sfz_opcode_text)
                .on_input(Message::SetSelectedGroupExtraSfz)
                .size(10)
                .width(Length::Fill),
        ]
        .spacing(4)
        .width(Length::Fill)
        .into(),
    ))
}

fn zones_panel<'a>(state: &'a State) -> Element<'a, Message> {
    let shared_zones = state.shared.zones.load();
    let shared_groups = state.shared.groups.load();
    let group_controls = selected_group_controls(state, &shared_groups);

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
        let group_name_for_press = group_name.clone();
        let group_name_for_header = group_name.clone();
        let header = mouse_area(
            container(
                text(group_name_for_header)
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
        let group_box_content = container(column![header, zones].spacing(4).width(Length::Fill))
            .width(Length::Fill)
            .padding([6, 6])
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
        let group_box = mouse_area(group_box_content)
            .on_enter(Message::HoverZoneDropGroup(Some(group_name_for_enter)))
            .on_exit(Message::HoverZoneDropGroup(None))
            .on_release(Message::FinishZoneListDrag);
        panel = panel.push(group_box);
    }

    let header = row![
        section_title("Zones"),
        pick_list(
            vec![ZoneCreateKind::Group, ZoneCreateKind::Zone],
            None::<ZoneCreateKind>,
            Message::CreateZoneListItem,
        )
        .placeholder("New")
        .width(Length::Fixed(78.0)),
    ]
    .spacing(6)
    .width(Length::Fill)
    .align_y(Alignment::Center);

    let mut content = column![header]
        .spacing(8)
        .height(Length::Fill)
        .align_x(Alignment::Start);
    if let Some(group_controls) = group_controls {
        content = content.push(group_controls);
    }
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
        .width(Length::Fill)
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
            .width(Length::Fill)
            .into()
    } else if is_instrument {
        button(content)
            .padding(1)
            .on_press(Message::LoadInstrument(entry.path.clone()))
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
                text(
                    state
                        .browser_path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| state.browser_path.display().to_string())
                )
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

fn zone_slider<'a>(
    label: &'static str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    field: ZoneEditField,
    value_text: String,
) -> Element<'a, Message> {
    let slider = Slider::new(range, value, move |v| {
        Message::SetEditingZoneValue(field, v)
    })
    .step(step)
    .width(Length::Fixed(150.0))
    .height(Length::Fixed(11.0))
    .horizontal();

    row![text(label).size(11), slider, text(value_text).size(10)]
        .spacing(6)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
}

fn zone_toggle<'a>(label: &'static str, value: bool) -> Element<'a, Message> {
    checkbox(value)
        .label(label)
        .on_toggle(Message::ToggleEditingZoneReverse)
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

fn cc_mod_route_row<'a>(index: usize, route: &ModRoute) -> Element<'a, Message> {
    let target = CcModTargetOption::from_target(route.target);
    let curve = CcCurveOption::from_curve(&route.source_curve);
    let amount = cc_mod_amount(route);
    let amount_range = cc_mod_amount_range(route.target);
    let amount_text = cc_mod_amount_text(route.target, amount);
    let show_cc = route.source == ModSource::MidiCc;
    let cc_slider: Element<'a, Message> = if show_cc {
        Slider::new(0.0..=127.0, route.source_cc as f32, move |value| {
            Message::SetEditingZoneCcRouteCc(index, value)
        })
        .step(1.0)
        .width(Length::Fixed(92.0))
        .height(Length::Fixed(20.0))
        .horizontal()
        .into()
    } else {
        container(text("").size(10))
            .width(Length::Fixed(92.0))
            .into()
    };
    let cc_text: Element<'a, Message> = if show_cc {
        text(route.source_cc.to_string()).size(9).into()
    } else {
        container(text("").size(9))
            .width(Length::Fixed(20.0))
            .into()
    };
    let amount_slider = Slider::new(amount_range, amount, move |value| {
        Message::SetEditingZoneCcRouteDepth(index, value)
    })
    .step(1.0)
    .width(Length::Fixed(120.0))
    .height(Length::Fixed(20.0))
    .horizontal();

    row![
        pick_list(
            ModSource::all_routable(),
            Some(route.source),
            move |source| { Message::SetEditingZoneCcRouteSource(index, source) },
        )
        .width(Length::Fixed(110.0)),
        cc_slider,
        cc_text,
        pick_list(CcModTargetOption::all(), target, move |target| {
            Message::SetEditingZoneCcRouteTarget(index, target)
        },)
        .width(Length::Fixed(112.0)),
        pick_list(CcCurveOption::all(), Some(curve), move |curve| {
            Message::SetEditingZoneCcRouteCurve(index, curve)
        },)
        .width(Length::Fixed(92.0)),
        amount_slider,
        text(amount_text).size(9),
        button(text("Del").size(10))
            .padding([3, 6])
            .on_press(Message::RemoveEditingZoneCcRoute(index)),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

fn cc_mod_routes_controls<'a>(zone: &SampleZone) -> Element<'a, Message> {
    let mut routes = column![].spacing(4).width(Length::Fill);
    let mut count = 0usize;
    for (index, route) in zone.mod_matrix.routes.iter().enumerate() {
        if route.active && CcModTargetOption::from_target(route.target).is_some() {
            routes = routes.push(cc_mod_route_row(index, route));
            count += 1;
        }
    }
    if count == 0 {
        routes = routes.push(
            text("No mod routes")
                .size(10)
                .color(Color::from_rgb(0.58, 0.60, 0.66)),
        );
    }

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
    .width(Length::Fill)
    .into()
}

fn cc_condition_row<'a>(index: usize, condition: &CcCondition) -> Element<'a, Message> {
    let cc_slider = Slider::new(0.0..=127.0, condition.cc as f32, move |value| {
        Message::SetEditingZoneCcConditionCc(index, value)
    })
    .step(1.0)
    .width(Length::Fixed(92.0))
    .height(Length::Fixed(20.0))
    .horizontal();
    let low_slider = Slider::new(0.0..=127.0, condition.low as f32, move |value| {
        Message::SetEditingZoneCcConditionLow(index, value)
    })
    .step(1.0)
    .width(Length::Fixed(92.0))
    .height(Length::Fixed(20.0))
    .horizontal();
    let high_slider = Slider::new(0.0..=127.0, condition.high as f32, move |value| {
        Message::SetEditingZoneCcConditionHigh(index, value)
    })
    .step(1.0)
    .width(Length::Fixed(92.0))
    .height(Length::Fixed(20.0))
    .horizontal();

    row![
        text("CC").size(10),
        cc_slider,
        text(condition.cc.to_string()).size(9),
        text("Lo").size(10),
        low_slider,
        text(condition.low.to_string()).size(9),
        text("Hi").size(10),
        high_slider,
        text(condition.high.to_string()).size(9),
        button(text("Del").size(10))
            .padding([3, 6])
            .on_press(Message::RemoveEditingZoneCcCondition(index)),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

fn cc_conditions_controls<'a>(zone: &SampleZone) -> Element<'a, Message> {
    let mut conditions = column![].spacing(4).width(Length::Fill);
    if zone.cc_conditions.is_empty() {
        conditions = conditions.push(
            text("No CC conditions")
                .size(10)
                .color(Color::from_rgb(0.58, 0.60, 0.66)),
        );
    } else {
        for (index, condition) in zone.cc_conditions.iter().enumerate() {
            conditions = conditions.push(cc_condition_row(index, condition));
        }
    }

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
    .width(Length::Fill)
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
    let loop_mode = ZoneLoopModeOption::from_mode(zone.loop_mode);
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

    let mapping = column![
        zone_slider(
            "Low Key",
            zone.start_note as f32,
            0.0..=127.0,
            1.0,
            ZoneEditField::StartNote,
            zone.start_note.to_string(),
        ),
        zone_slider(
            "High Key",
            zone.end_note as f32,
            0.0..=127.0,
            1.0,
            ZoneEditField::EndNote,
            zone.end_note.to_string(),
        ),
        zone_slider(
            "Low Vel",
            zone.vel_low as f32,
            0.0..=127.0,
            1.0,
            ZoneEditField::VelLow,
            zone.vel_low.to_string(),
        ),
        zone_slider(
            "High Vel",
            zone.vel_high as f32,
            0.0..=127.0,
            1.0,
            ZoneEditField::VelHigh,
            zone.vel_high.to_string(),
        ),
        zone_slider(
            "Root",
            zone.root_key as f32,
            0.0..=127.0,
            1.0,
            ZoneEditField::RootKey,
            zone.root_key.to_string(),
        ),
        zone_slider(
            "Key Trk",
            zone.key_tracking * 100.0,
            0.0..=200.0,
            1.0,
            ZoneEditField::KeyTracking,
            format!("{:.0}%", zone.key_tracking * 100.0),
        ),
        row![
            text("Vel Curve").size(11),
            pick_list(
                ZoneCurveOption::all(),
                Some(ZoneCurveOption::from_curve(zone.velocity_curve)),
                Message::SetEditingZoneVelocityCurve,
            )
            .width(Length::Fixed(120.0)),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    ]
    .spacing(4)
    .width(Length::Fixed(235.0));

    let playback = column![
        zone_slider(
            "Gain",
            zone.gain_db,
            -96.0..=24.0,
            0.1,
            ZoneEditField::GainDb,
            format!("{:.1} dB", zone.gain_db),
        ),
        zone_slider(
            "Pan",
            zone.pan * 100.0,
            -100.0..=100.0,
            1.0,
            ZoneEditField::Pan,
            format!("{:.0}", zone.pan * 100.0),
        ),
        zone_slider(
            "Width",
            zone.width * 100.0,
            0.0..=200.0,
            1.0,
            ZoneEditField::Width,
            format!("{:.0}", zone.width * 100.0),
        ),
        zone_slider(
            "Position",
            zone.position * 100.0,
            -100.0..=100.0,
            1.0,
            ZoneEditField::Position,
            format!("{:.0}", zone.position * 100.0),
        ),
        zone_slider(
            "Amp Key",
            zone.amp_keytrack_db,
            -12.0..=12.0,
            0.1,
            ZoneEditField::AmpKeytrack,
            format!("{:.1}", zone.amp_keytrack_db),
        ),
        zone_slider(
            "Vel Track",
            zone.amp_veltrack,
            -100.0..=100.0,
            1.0,
            ZoneEditField::AmpVeltrack,
            format!("{:.0}", zone.amp_veltrack),
        ),
        zone_slider(
            "Tune",
            zone.pitch_offset,
            -1200.0..=1200.0,
            1.0,
            ZoneEditField::PitchOffset,
            format!("{:.0} c", zone.pitch_offset),
        ),
        zone_slider(
            "Offset",
            zone.start_offset as f32,
            0.0..=frame_max,
            1.0,
            ZoneEditField::StartOffset,
            zone.start_offset.to_string(),
        ),
        zone_slider(
            "Offset R",
            zone.offset_random as f32,
            0.0..=frame_max,
            1.0,
            ZoneEditField::OffsetRandom,
            zone.offset_random.to_string(),
        ),
        zone_slider(
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
        ),
        row![
            text("Trigger").size(11),
            pick_list(
                ZonePlayModeOption::all(),
                Some(play_mode),
                Message::SetEditingZonePlayMode,
            )
            .width(Length::Fixed(120.0)),
            zone_toggle("Reverse", zone.reverse),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    ]
    .spacing(4)
    .width(Length::Fixed(300.0));

    let fades = column![
        zone_slider(
            "Key In L",
            key_fade_in.0 as f32,
            0.0..=127.0,
            1.0,
            ZoneEditField::KeyFadeInLow,
            key_fade_in.0.to_string(),
        ),
        zone_slider(
            "Key In H",
            key_fade_in.1 as f32,
            0.0..=127.0,
            1.0,
            ZoneEditField::KeyFadeInHigh,
            key_fade_in.1.to_string(),
        ),
        zone_slider(
            "Key Out L",
            key_fade_out.0 as f32,
            0.0..=127.0,
            1.0,
            ZoneEditField::KeyFadeOutLow,
            key_fade_out.0.to_string(),
        ),
        zone_slider(
            "Key Out H",
            key_fade_out.1 as f32,
            0.0..=127.0,
            1.0,
            ZoneEditField::KeyFadeOutHigh,
            key_fade_out.1.to_string(),
        ),
        zone_slider(
            "Vel In L",
            vel_fade_in.0 as f32,
            0.0..=127.0,
            1.0,
            ZoneEditField::VelFadeInLow,
            vel_fade_in.0.to_string(),
        ),
        zone_slider(
            "Vel In H",
            vel_fade_in.1 as f32,
            0.0..=127.0,
            1.0,
            ZoneEditField::VelFadeInHigh,
            vel_fade_in.1.to_string(),
        ),
        zone_slider(
            "Vel Out L",
            vel_fade_out.0 as f32,
            0.0..=127.0,
            1.0,
            ZoneEditField::VelFadeOutLow,
            vel_fade_out.0.to_string(),
        ),
        zone_slider(
            "Vel Out H",
            vel_fade_out.1 as f32,
            0.0..=127.0,
            1.0,
            ZoneEditField::VelFadeOutHigh,
            vel_fade_out.1.to_string(),
        ),
    ]
    .spacing(4)
    .width(Length::Fixed(235.0));

    let loop_controls = column![
        row![
            text("Loop").size(11),
            pick_list(
                ZoneLoopModeOption::all(),
                Some(loop_mode),
                Message::SetEditingZoneLoopMode,
            )
            .width(Length::Fixed(120.0)),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        zone_slider(
            "Start",
            zone.loop_start as f32,
            0.0..=frame_max,
            1.0,
            ZoneEditField::LoopStart,
            zone.loop_start.to_string(),
        ),
        zone_slider(
            "End",
            zone.loop_end as f32,
            0.0..=frame_max,
            1.0,
            ZoneEditField::LoopEnd,
            zone.loop_end.to_string(),
        ),
        zone_slider(
            "Xfade",
            zone.loop_crossfade as f32,
            0.0..=frame_max,
            1.0,
            ZoneEditField::LoopCrossfade,
            zone.loop_crossfade.to_string(),
        ),
        zone_slider(
            "Count",
            zone.loop_count as f32,
            0.0..=128.0,
            1.0,
            ZoneEditField::LoopCount,
            zone.loop_count.to_string(),
        ),
        zone_slider(
            "Bend Up",
            zone.pitch_bend_up,
            0.0..=12000.0,
            1.0,
            ZoneEditField::PitchBendUp,
            format!("{:.0} c", zone.pitch_bend_up),
        ),
        zone_slider(
            "Bend Down",
            zone.pitch_bend_down,
            0.0..=12000.0,
            1.0,
            ZoneEditField::PitchBendDown,
            format!("{:.0} c", zone.pitch_bend_down),
        ),
    ]
    .spacing(4)
    .width(Length::Fixed(235.0));

    let conditions = column![
        zone_slider(
            "Chan Lo",
            zone.channel_low as f32,
            1.0..=16.0,
            1.0,
            ZoneEditField::ChannelLow,
            zone.channel_low.to_string(),
        ),
        zone_slider(
            "Chan Hi",
            zone.channel_high as f32,
            1.0..=16.0,
            1.0,
            ZoneEditField::ChannelHigh,
            zone.channel_high.to_string(),
        ),
        zone_slider(
            "Bend Lo",
            zone.pitch_bend_low as f32,
            -8192.0..=8192.0,
            1.0,
            ZoneEditField::PitchBendLow,
            zone.pitch_bend_low.to_string(),
        ),
        zone_slider(
            "Bend Hi",
            zone.pitch_bend_high as f32,
            -8192.0..=8192.0,
            1.0,
            ZoneEditField::PitchBendHigh,
            zone.pitch_bend_high.to_string(),
        ),
        zone_slider(
            "Rand Lo",
            zone.random_low,
            0.0..=1.0,
            0.01,
            ZoneEditField::RandomLow,
            format!("{:.2}", zone.random_low),
        ),
        zone_slider(
            "Rand Hi",
            zone.random_high,
            0.0..=1.0,
            0.01,
            ZoneEditField::RandomHigh,
            format!("{:.2}", zone.random_high),
        ),
        zone_slider(
            "Seq Len",
            zone.seq_length as f32,
            0.0..=128.0,
            1.0,
            ZoneEditField::SeqLength,
            zone.seq_length.to_string(),
        ),
        zone_slider(
            "Seq Pos",
            zone.seq_position as f32,
            0.0..=128.0,
            1.0,
            ZoneEditField::SeqPosition,
            zone.seq_position.to_string(),
        ),
        zone_slider(
            "Off By",
            zone.off_by as f32,
            0.0..=127.0,
            1.0,
            ZoneEditField::OffBy,
            zone.off_by.to_string(),
        ),
        row![
            text("Off Mode").size(11),
            pick_list(
                ZoneOffModeOption::all(),
                Some(ZoneOffModeOption::from_mode(zone.off_mode)),
                Message::SetEditingZoneOffMode,
            )
            .width(Length::Fixed(120.0)),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        zone_slider(
            "Count",
            zone.count as f32,
            0.0..=128.0,
            1.0,
            ZoneEditField::Count,
            zone.count.to_string(),
        ),
        zone_slider(
            "Output",
            zone.output as f32,
            0.0..=31.0,
            1.0,
            ZoneEditField::Output,
            zone.output.to_string(),
        ),
    ]
    .spacing(4)
    .width(Length::Fixed(235.0));

    panel_no_title(
        column![
            row![mapping, playback, fades, loop_controls]
                .spacing(12)
                .align_y(Alignment::Start),
            row![conditions].spacing(12).align_y(Alignment::Start),
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
        .width(Length::Fill)
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
            maolan_editor::app::embedded_view_without_vu_meter(editor).map(Message::AudioEditor)
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
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Left)
        .align_y(Vertical::Top)
        .into()
}

fn view(state: &State) -> Element<'_, Message> {
    if let Some(index) = state.editing_zone_index {
        return sampler_editor_view(state, index);
    }

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
        hslider(ParamId::AmpAttack, "A", state),
        hslider(ParamId::AmpDecay, "D", state),
        hslider(ParamId::AmpSustain, "S", state),
        hslider(ParamId::AmpRelease, "R", state),
    ]));

    let filter_eg = panel_no_title(knob_row(vec![
        hslider(ParamId::FilterAttack, "A", state),
        hslider(ParamId::FilterDecay, "D", state),
        hslider(ParamId::FilterSustain, "S", state),
        hslider(ParamId::FilterRelease, "R", state),
    ]));

    let pitch_eg = panel_no_title(knob_row(vec![
        hslider(ParamId::Eg2Attack, "A", state),
        hslider(ParamId::Eg2Decay, "D", state),
        hslider(ParamId::Eg2Sustain, "S", state),
        hslider(ParamId::Eg2Release, "R", state),
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
        hslider(ParamId::Eg3Attack, "A", state),
        hslider(ParamId::Eg3Decay, "D", state),
        hslider(ParamId::Eg3Sustain, "S", state),
        hslider(ParamId::Eg3Release, "R", state),
    ]));

    let eg4 = panel_no_title(knob_row(vec![
        hslider(ParamId::Eg4Attack, "A", state),
        hslider(ParamId::Eg4Decay, "D", state),
        hslider(ParamId::Eg4Sustain, "S", state),
        hslider(ParamId::Eg4Release, "R", state),
    ]));

    let eg5 = panel_no_title(knob_row(vec![
        hslider(ParamId::Eg5Attack, "A", state),
        hslider(ParamId::Eg5Decay, "D", state),
        hslider(ParamId::Eg5Sustain, "S", state),
        hslider(ParamId::Eg5Release, "R", state),
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

    let main_content = column![instrument_panel(state), sample_map,]
        .spacing(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Start);
    let main_content = main_content
        .push(top_row)
        .push(bottom_row)
        .spacing(12)
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
        .width(Length::Fill)
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
    let editor_sub = state
        .audio_editor
        .as_ref()
        .map(|editor| maolan_editor::app::subscription(editor).map(Message::AudioEditor));
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
