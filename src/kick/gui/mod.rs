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
#[cfg(all(unix, not(target_os = "macos")))]
use clap_clap::ffi::CLAP_WINDOW_API_X11;
use maolan_baseview::iced::widget::canvas::{Frame, Geometry, Path, Program, Stroke, Text};
use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{canvas, checkbox, column, container, pick_list, row, text},
};
use maolan_widgets::arch_slider::arch_slider;
use maolan_widgets::meters::meters;
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};

mod envelope_editor;

use crate::kick::gui::envelope_editor::{EnvelopeEditor, EnvelopeEditorMsg};
use crate::kick::params::{ParamId, ParamType, param_type_def};
use crate::kick::plugin::SharedState;

pub const EDITOR_WIDTH: u32 = 1024;
pub const EDITOR_HEIGHT: u32 = 720;

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

// ---------------------------------------------------------------------------
// Waveform Canvas
// ---------------------------------------------------------------------------

struct WaveformState {
    shared: Arc<SharedState>,
}

impl Program<Message> for WaveformState {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &maolan_baseview::iced::Renderer,
        _theme: &Theme,
        bounds: maolan_baseview::iced::Rectangle,
        _cursor: maolan_baseview::iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let width = bounds.width;
        let height = bounds.height;

        frame.fill_rectangle(
            maolan_baseview::iced::Point::new(0.0, 0.0),
            maolan_baseview::iced::Size::new(width, height),
            maolan_baseview::iced::Color::from_rgb(0.08, 0.08, 0.10),
        );

        for i in 1..5 {
            let y = height * i as f32 / 5.0;
            let grid_path = Path::line(
                maolan_baseview::iced::Point::new(0.0, y),
                maolan_baseview::iced::Point::new(width, y),
            );
            frame.stroke(
                &grid_path,
                Stroke::default()
                    .with_color(maolan_baseview::iced::Color::from_rgb(0.15, 0.15, 0.18))
                    .with_width(0.5),
            );
        }

        let waveform = self.shared.waveform_display.lock();
        if !waveform.0.is_empty() {
            let center_y = height / 2.0;
            let samples = waveform.0.len();
            let peak = waveform
                .0
                .iter()
                .chain(waveform.1.iter())
                .fold(0.0f32, |a, &b| a.max(b.abs()))
                .max(1.0e-12);
            let scale_y = (height * 0.45) / peak;

            let path = Path::new(|builder| {
                let first_y = center_y - waveform.0[0] * scale_y;
                builder.move_to(maolan_baseview::iced::Point::new(0.0, first_y));
                let step = (samples as f32 / width).max(1.0);
                let mut x = 0.0f32;
                while x < width {
                    let idx = ((x / width) * samples as f32) as usize;
                    let idx = idx.min(samples - 1);
                    let y = center_y - waveform.0[idx] * scale_y;
                    builder.line_to(maolan_baseview::iced::Point::new(x, y));
                    x += step.max(1.0);
                }
            });
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(maolan_baseview::iced::Color::from_rgb(0.2, 0.85, 0.4))
                    .with_width(1.5),
            );
        } else {
            let line = Path::line(
                maolan_baseview::iced::Point::new(0.0, height / 2.0),
                maolan_baseview::iced::Point::new(width, height / 2.0),
            );
            frame.stroke(
                &line,
                Stroke::default()
                    .with_color(maolan_baseview::iced::Color::from_rgb(0.25, 0.25, 0.28))
                    .with_width(1.0),
            );
        }

        frame.fill_text(Text {
            content: "Maolan Kick".to_string(),
            position: maolan_baseview::iced::Point::new(8.0, 14.0),
            color: maolan_baseview::iced::Color::from_rgb(0.7, 0.7, 0.7),
            size: 14.0.into(),
            font: maolan_baseview::iced::Font::DEFAULT,
            ..Text::default()
        });

        vec![frame.into_geometry()]
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Message {
    SetParam(ParamId, f32),
    SetBoolParam(ParamId, bool),
    ReleaseParam(ParamId),
    SetFilterType(ParamId, u8),
    SetWaveform(ParamId, u8),
    SetDistortionType(ParamId, u8),
    SetNoiseType(u8),
    SetActiveInstrument(u8),
    CopyInstrument,
    PasteInstrument,
    DuplicateInstrument,
    ClearInstrument,
    EnvelopeEdit(EnvelopeEditorMsg),
    PresetNameChanged(String),
    SavePreset,
    LoadPreset(String),
    RefreshPresets,
    SamplePathChanged(String),
    SampleTargetLayerChanged(u8),
    SampleTargetOscChanged(u8),
    EnvelopeKindChanged(u8),
    EnvelopeLayerChanged(u8),
    EnvelopeOscChanged(u8),
    ExportPathChanged(String),
    ExportFormatChanged(u8),
    ExportChannelsChanged(u8),
    ExportMidiNoteChanged(u8),
    ExportCurrentInstrument,
    LoadSample,
    MainTabChanged(u8),
    LayerTabChanged(u8),
    OscTabChanged(u8),
    InstrumentNameChanged(String),
}

#[derive(Debug, Clone, Copy)]
enum EnvelopeKind {
    GlobalAmp = 0,
    OscAmp = 1,
    OscPitch = 2,
    OscFreq = 3,
    OscFilterCutoff = 4,
    OscFilterQ = 5,
    OscDistDrive = 6,
    OscPitchShift = 7,
    NoiseAmp = 8,
    NoiseDensity = 9,
    MasterDistVol = 10,
    LayerDistVol = 11,
}

impl EnvelopeKind {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::OscAmp,
            2 => Self::OscPitch,
            3 => Self::OscFreq,
            4 => Self::OscFilterCutoff,
            5 => Self::OscFilterQ,
            6 => Self::OscDistDrive,
            7 => Self::OscPitchShift,
            8 => Self::NoiseAmp,
            9 => Self::NoiseDensity,
            10 => Self::MasterDistVol,
            11 => Self::LayerDistVol,
            _ => Self::GlobalAmp,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::GlobalAmp => "Global Amp",
            Self::OscAmp => "Osc Amp",
            Self::OscPitch => "Osc Pitch",
            Self::OscFreq => "Osc Freq",
            Self::OscFilterCutoff => "Osc Filter Cutoff",
            Self::OscFilterQ => "Osc Filter Q",
            Self::OscDistDrive => "Osc Dist Drive",
            Self::OscPitchShift => "Osc Pitch Shift",
            Self::NoiseAmp => "Noise Amp",
            Self::NoiseDensity => "Noise Density",
            Self::MasterDistVol => "Master Dist Vol",
            Self::LayerDistVol => "Layer Dist Vol",
        }
    }
}

struct State {
    shared: Arc<SharedState>,
    active_gestures: Vec<bool>,
    show_envelope_editor: bool,
    preset_name_input: String,
    sample_path_input: String,
    sample_target_layer: u8,
    sample_target_osc: u8,
    envelope_kind: u8,
    envelope_layer: u8,
    envelope_osc: u8,
    export_path_input: String,
    export_format: u8,
    export_channels: u8,
    export_midi_note: u8,
    export_status: String,
    preset_files: Vec<String>,
    main_tab: u8,
    active_layer_tab: u8,
    active_osc_tab: u8,
    instrument_name_input: String,
}

fn presets_dir() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("maolan").join("kick").join("presets"))
}

fn scan_presets() -> Vec<String> {
    let dir = match presets_dir() {
        Some(d) => d,
        None => return Vec::new(),
    };
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json")
                && let Some(name) = path.file_stem().and_then(|s| s.to_str())
            {
                files.push(name.to_string());
            }
        }
    }
    files.sort();
    files
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    let active_inst = shared
        .params
        .get(ParamId::new(0, ParamType::ActiveInstrument)) as usize;
    let kit = shared.kit.lock();
    let instrument_name_input = if active_inst < kit.instruments.len() {
        kit.instruments[active_inst].name.clone()
    } else {
        String::new()
    };
    drop(kit);
    (
        State {
            shared,
            active_gestures: vec![false; ParamId::COUNT],
            show_envelope_editor: true,
            preset_name_input: String::new(),
            sample_path_input: String::new(),
            sample_target_layer: 0,
            sample_target_osc: 0,
            envelope_kind: 0,
            envelope_layer: 0,
            envelope_osc: 0,
            export_path_input: String::new(),
            export_format: 0,
            export_channels: 1,
            export_midi_note: 36,
            export_status: String::new(),
            preset_files: scan_presets(),
            main_tab: 0,
            active_layer_tab: 0,
            active_osc_tab: 0,
            instrument_name_input,
        },
        Task::none(),
    )
}

fn selected_env(
    inst: &mut crate::kick::dsp::Instrument,
    kind: EnvelopeKind,
    layer: usize,
    osc: usize,
) -> &mut crate::kick::dsp::Envelope {
    let layer = layer.min(2);
    let osc = osc.min(2);
    match kind {
        EnvelopeKind::GlobalAmp => &mut inst.global_amp_env,
        EnvelopeKind::OscAmp => &mut inst.layers[layer].oscillators[osc].amp_env,
        EnvelopeKind::OscPitch => &mut inst.layers[layer].oscillators[osc].pitch_env,
        EnvelopeKind::OscFreq => &mut inst.layers[layer].oscillators[osc].freq_env,
        EnvelopeKind::OscFilterCutoff => &mut inst.layers[layer].oscillators[osc].filter_cutoff_env,
        EnvelopeKind::OscFilterQ => &mut inst.layers[layer].oscillators[osc].filter_q_env,
        EnvelopeKind::OscDistDrive => &mut inst.layers[layer].oscillators[osc].distortion_drive_env,
        EnvelopeKind::OscPitchShift => &mut inst.layers[layer].oscillators[osc].pitch_shift_env,
        EnvelopeKind::NoiseAmp => &mut inst.layers[layer].noise.amp_env,
        EnvelopeKind::NoiseDensity => &mut inst.layers[layer].noise.density_env,
        EnvelopeKind::MasterDistVol => &mut inst.master_distortion.volume_env,
        EnvelopeKind::LayerDistVol => &mut inst.layers[layer].distortion.volume_env,
    }
}

fn envelope_param_types(
    kind: EnvelopeKind,
    _layer: u8,
    osc: u8,
) -> Option<(ParamType, ParamType, ParamType, ParamType)> {
    let osc = osc.min(2);
    match kind {
        EnvelopeKind::GlobalAmp => Some((
            ParamType::MasterGlobalAmpEnvAttack,
            ParamType::MasterGlobalAmpEnvDecay,
            ParamType::MasterGlobalAmpEnvSustain,
            ParamType::MasterGlobalAmpEnvRelease,
        )),
        EnvelopeKind::OscAmp => match osc {
            0 => Some((
                ParamType::Osc0AmpEnvAttack,
                ParamType::Osc0AmpEnvDecay,
                ParamType::Osc0AmpEnvSustain,
                ParamType::Osc0AmpEnvRelease,
            )),
            1 => Some((
                ParamType::Osc1AmpEnvAttack,
                ParamType::Osc1AmpEnvDecay,
                ParamType::Osc1AmpEnvSustain,
                ParamType::Osc1AmpEnvRelease,
            )),
            2 => Some((
                ParamType::Osc2AmpEnvAttack,
                ParamType::Osc2AmpEnvDecay,
                ParamType::Osc2AmpEnvSustain,
                ParamType::Osc2AmpEnvRelease,
            )),
            _ => None,
        },
        EnvelopeKind::OscPitch => match osc {
            0 => Some((
                ParamType::Osc0PitchShiftEnvAttack,
                ParamType::Osc0PitchShiftEnvDecay,
                ParamType::Osc0PitchShiftEnvSustain,
                ParamType::Osc0PitchShiftEnvRelease,
            )),
            1 => Some((
                ParamType::Osc1PitchShiftEnvAttack,
                ParamType::Osc1PitchShiftEnvDecay,
                ParamType::Osc1PitchShiftEnvSustain,
                ParamType::Osc1PitchShiftEnvRelease,
            )),
            2 => Some((
                ParamType::Osc2PitchShiftEnvAttack,
                ParamType::Osc2PitchShiftEnvDecay,
                ParamType::Osc2PitchShiftEnvSustain,
                ParamType::Osc2PitchShiftEnvRelease,
            )),
            _ => None,
        },
        EnvelopeKind::OscFreq => match osc {
            0 => Some((
                ParamType::Osc0FreqEnvAttack,
                ParamType::Osc0FreqEnvDecay,
                ParamType::Osc0FreqEnvSustain,
                ParamType::Osc0FreqEnvRelease,
            )),
            1 => Some((
                ParamType::Osc1FreqEnvAttack,
                ParamType::Osc1FreqEnvDecay,
                ParamType::Osc1FreqEnvSustain,
                ParamType::Osc1FreqEnvRelease,
            )),
            2 => Some((
                ParamType::Osc2FreqEnvAttack,
                ParamType::Osc2FreqEnvDecay,
                ParamType::Osc2FreqEnvSustain,
                ParamType::Osc2FreqEnvRelease,
            )),
            _ => None,
        },
        EnvelopeKind::OscFilterCutoff => match osc {
            0 => Some((
                ParamType::Osc0FilterCutoffEnvAttack,
                ParamType::Osc0FilterCutoffEnvDecay,
                ParamType::Osc0FilterCutoffEnvSustain,
                ParamType::Osc0FilterCutoffEnvRelease,
            )),
            1 => Some((
                ParamType::Osc1FilterCutoffEnvAttack,
                ParamType::Osc1FilterCutoffEnvDecay,
                ParamType::Osc1FilterCutoffEnvSustain,
                ParamType::Osc1FilterCutoffEnvRelease,
            )),
            2 => Some((
                ParamType::Osc2FilterCutoffEnvAttack,
                ParamType::Osc2FilterCutoffEnvDecay,
                ParamType::Osc2FilterCutoffEnvSustain,
                ParamType::Osc2FilterCutoffEnvRelease,
            )),
            _ => None,
        },
        EnvelopeKind::OscFilterQ => match osc {
            0 => Some((
                ParamType::Osc0FilterQEnvAttack,
                ParamType::Osc0FilterQEnvDecay,
                ParamType::Osc0FilterQEnvSustain,
                ParamType::Osc0FilterQEnvRelease,
            )),
            1 => Some((
                ParamType::Osc1FilterQEnvAttack,
                ParamType::Osc1FilterQEnvDecay,
                ParamType::Osc1FilterQEnvSustain,
                ParamType::Osc1FilterQEnvRelease,
            )),
            2 => Some((
                ParamType::Osc2FilterQEnvAttack,
                ParamType::Osc2FilterQEnvDecay,
                ParamType::Osc2FilterQEnvSustain,
                ParamType::Osc2FilterQEnvRelease,
            )),
            _ => None,
        },
        EnvelopeKind::OscDistDrive => match osc {
            0 => Some((
                ParamType::Osc0DistortionDriveEnvAttack,
                ParamType::Osc0DistortionDriveEnvDecay,
                ParamType::Osc0DistortionDriveEnvSustain,
                ParamType::Osc0DistortionDriveEnvRelease,
            )),
            1 => Some((
                ParamType::Osc1DistortionDriveEnvAttack,
                ParamType::Osc1DistortionDriveEnvDecay,
                ParamType::Osc1DistortionDriveEnvSustain,
                ParamType::Osc1DistortionDriveEnvRelease,
            )),
            2 => Some((
                ParamType::Osc2DistortionDriveEnvAttack,
                ParamType::Osc2DistortionDriveEnvDecay,
                ParamType::Osc2DistortionDriveEnvSustain,
                ParamType::Osc2DistortionDriveEnvRelease,
            )),
            _ => None,
        },
        EnvelopeKind::NoiseAmp => Some((
            ParamType::NoiseAmpEnvAttack,
            ParamType::NoiseAmpEnvDecay,
            ParamType::NoiseAmpEnvSustain,
            ParamType::NoiseAmpEnvRelease,
        )),
        EnvelopeKind::NoiseDensity => Some((
            ParamType::NoiseDensityEnvAttack,
            ParamType::NoiseDensityEnvDecay,
            ParamType::NoiseDensityEnvSustain,
            ParamType::NoiseDensityEnvRelease,
        )),
        EnvelopeKind::MasterDistVol => Some((
            ParamType::MasterDistortionVolEnvAttack,
            ParamType::MasterDistortionVolEnvDecay,
            ParamType::MasterDistortionVolEnvSustain,
            ParamType::MasterDistortionVolEnvRelease,
        )),
        EnvelopeKind::OscPitchShift => match osc {
            0 => Some((
                ParamType::Osc0PitchShiftEnvAttack,
                ParamType::Osc0PitchShiftEnvDecay,
                ParamType::Osc0PitchShiftEnvSustain,
                ParamType::Osc0PitchShiftEnvRelease,
            )),
            1 => Some((
                ParamType::Osc1PitchShiftEnvAttack,
                ParamType::Osc1PitchShiftEnvDecay,
                ParamType::Osc1PitchShiftEnvSustain,
                ParamType::Osc1PitchShiftEnvRelease,
            )),
            2 => Some((
                ParamType::Osc2PitchShiftEnvAttack,
                ParamType::Osc2PitchShiftEnvDecay,
                ParamType::Osc2PitchShiftEnvSustain,
                ParamType::Osc2PitchShiftEnvRelease,
            )),
            _ => None,
        },
        EnvelopeKind::LayerDistVol => Some((
            ParamType::Layer0DistortionVolEnvAttack,
            ParamType::Layer0DistortionVolEnvDecay,
            ParamType::Layer0DistortionVolEnvSustain,
            ParamType::Layer0DistortionVolEnvRelease,
        )),
    }
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
        Message::SetBoolParam(id, value) => {
            let idx = id.as_index();
            if !state.active_gestures[idx] {
                state.active_gestures[idx] = true;
                state.shared.mark_gesture_begin_pending(id);
            }
            state.shared.set_bool_param_outbound_only(id, value);
        }
        Message::ReleaseParam(id) => {
            let idx = id.as_index();
            if state.active_gestures[idx] {
                state.active_gestures[idx] = false;
                state.shared.mark_gesture_end_pending(id);
            }
        }
        Message::SetFilterType(id, v) => {
            state.shared.set_param_outbound_only(id, v as f64);
        }
        Message::SetWaveform(id, v) => {
            state.shared.set_param_outbound_only(id, v as f64);
        }
        Message::SetDistortionType(id, v) => {
            state.shared.set_param_outbound_only(id, v as f64);
        }
        Message::SetNoiseType(v) => {
            let active_inst = state
                .shared
                .params
                .get(ParamId::new(0, ParamType::ActiveInstrument))
                as usize;
            state.shared.set_param_outbound_only(
                ParamId::new(active_inst as u8, ParamType::NoiseType),
                v as f64,
            );
        }
        Message::SetActiveInstrument(inst) => {
            state
                .shared
                .set_param_outbound_only(ParamId::new(0, ParamType::ActiveInstrument), inst as f64);
            let kit = state.shared.kit.lock();
            state.instrument_name_input = if (inst as usize) < kit.instruments.len() {
                kit.instruments[inst as usize].name.clone()
            } else {
                String::new()
            };
        }
        Message::CopyInstrument => {
            let active_inst = state
                .shared
                .params
                .get(ParamId::new(0, ParamType::ActiveInstrument))
                as usize;
            let kit = state.shared.kit.lock();
            let inst = kit.instruments[active_inst].clone();
            drop(kit);
            let mut clip = state.shared.instrument_clipboard.lock();
            *clip = Some(inst);
        }
        Message::PasteInstrument => {
            let active_inst = state
                .shared
                .params
                .get(ParamId::new(0, ParamType::ActiveInstrument))
                as usize;
            let mut kit = state.shared.kit.lock();
            if let Some(ref inst) = *state.shared.instrument_clipboard.lock() {
                kit.instruments[active_inst] = inst.clone();
                state
                    .shared
                    .kit_version
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            }
        }
        Message::DuplicateInstrument => {
            let active_inst = state
                .shared
                .params
                .get(ParamId::new(0, ParamType::ActiveInstrument))
                as usize;
            let mut kit = state.shared.kit.lock();
            if active_inst < kit.instruments.len() {
                let clone = kit.instruments[active_inst].clone();
                let dst = (active_inst + 1).min(kit.instruments.len() - 1);
                kit.instruments[dst] = clone;
                for ty_idx in 0..ParamType::COUNT {
                    let src = ParamId::new(active_inst as u8, unsafe {
                        std::mem::transmute::<u8, ParamType>(ty_idx as u8)
                    });
                    let dst_id = ParamId::new(dst as u8, unsafe {
                        std::mem::transmute::<u8, ParamType>(ty_idx as u8)
                    });
                    state
                        .shared
                        .params
                        .set(dst_id, state.shared.params.get(src));
                }
                state
                    .shared
                    .kit_version
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            }
        }
        Message::ClearInstrument => {
            let active_inst = state
                .shared
                .params
                .get(ParamId::new(0, ParamType::ActiveInstrument))
                as usize;
            let mut kit = state.shared.kit.lock();
            if active_inst < kit.instruments.len() {
                kit.instruments[active_inst] =
                    crate::kick::dsp::Instrument::new(state.shared.sample_rate());
                for ty_idx in 0..ParamType::COUNT {
                    let ty = unsafe { std::mem::transmute::<u8, ParamType>(ty_idx as u8) };
                    let id = ParamId::new(active_inst as u8, ty);
                    let def = param_type_def(ty);
                    state.shared.params.set(id, def.default);
                }
                state
                    .shared
                    .kit_version
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            }
        }
        Message::EnvelopeEdit(msg) => {
            let active_inst = state
                .shared
                .params
                .get(ParamId::new(0, ParamType::ActiveInstrument))
                as usize;
            let mut kit = state.shared.kit.lock();
            let env_kind = EnvelopeKind::from_u8(state.envelope_kind);
            let env = selected_env(
                &mut kit.instruments[active_inst],
                env_kind,
                state.envelope_layer as usize,
                state.envelope_osc as usize,
            );
            match msg {
                EnvelopeEditorMsg::PointMoved(idx, t, v) => {
                    if let Some(p) = env.points_mut().get_mut(idx) {
                        p.t = t.clamp(0.0, 1.0);
                        p.v = v.clamp(0.0, 1.0);
                    }
                }
                EnvelopeEditorMsg::ControlPointMoved(idx, is_left, t, v) => {
                    if let Some(p) = env.points_mut().get_mut(idx) {
                        if is_left {
                            p.cp_t = (p.t - t).clamp(0.0, 1.0);
                            p.cp_v = v - p.v;
                        } else {
                            p.cp_t = (t - p.t).clamp(0.0, 1.0);
                            p.cp_v = v - p.v;
                        }
                    }
                }
                EnvelopeEditorMsg::PointAdded(t, v) => {
                    let mut points: Vec<_> = env.points().to_vec();
                    points.push(crate::kick::dsp::envelope::EnvPoint::new(t, v));
                    points.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
                    *env = crate::kick::dsp::envelope::Envelope::new(points);
                }
                EnvelopeEditorMsg::PointRemoved(idx) => {
                    let mut points: Vec<_> = env.points().to_vec();
                    if points.len() > 2 && idx < points.len() {
                        points.remove(idx);
                        *env = crate::kick::dsp::envelope::Envelope::new(points);
                    }
                }
            }
            state
                .shared
                .kit_version
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        Message::PresetNameChanged(name) => {
            state.preset_name_input = name;
        }
        Message::SavePreset => {
            if let Some(dir) = presets_dir() {
                let _ = std::fs::create_dir_all(&dir);
                let name = state.preset_name_input.trim();
                if !name.is_empty() {
                    let path = dir.join(format!("{name}.json"));
                    let kit = state.shared.kit.lock();
                    let kit_cfg = crate::kick::plugin::kit_to_config(&kit);
                    drop(kit);
                    let state_obj =
                        crate::kick::state::KitState::from_runtime(&state.shared.params, &kit_cfg);
                    if let Ok(bytes) = state_obj.to_bytes() {
                        let _ = std::fs::write(&path, bytes);
                    }
                    state.preset_files = scan_presets();
                }
            }
        }
        Message::LoadPreset(name) => {
            if let Some(dir) = presets_dir() {
                let path = dir.join(format!("{name}.json"));
                if let Ok(bytes) = std::fs::read(&path)
                    && let Ok(kit_state) = crate::kick::state::KitState::from_bytes(&bytes)
                {
                    let kit_cfg = kit_state.kit.clone();
                    kit_state.apply_params(&state.shared.params);
                    let mut kit = state.shared.kit.lock();
                    *kit = crate::kick::plugin::config_to_kit(&kit_cfg, state.shared.sample_rate());
                    state
                        .shared
                        .kit_version
                        .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                }
            }
        }
        Message::RefreshPresets => {
            state.preset_files = scan_presets();
        }
        Message::SamplePathChanged(path) => {
            state.sample_path_input = path;
        }
        Message::SampleTargetLayerChanged(layer) => {
            state.sample_target_layer = layer.min(2);
        }
        Message::SampleTargetOscChanged(osc) => {
            state.sample_target_osc = osc.min(2);
        }
        Message::EnvelopeKindChanged(kind) => {
            state.envelope_kind = kind.min(11);
        }
        Message::EnvelopeLayerChanged(layer) => {
            state.envelope_layer = layer.min(2);
        }
        Message::EnvelopeOscChanged(osc) => {
            state.envelope_osc = osc.min(2);
        }
        Message::ExportPathChanged(path) => {
            state.export_path_input = path;
            state.export_status.clear();
        }
        Message::ExportFormatChanged(v) => {
            state.export_format = v.min(3);
        }
        Message::ExportChannelsChanged(v) => {
            state.export_channels = if v == 0 { 1 } else { 2 };
        }
        Message::ExportMidiNoteChanged(v) => {
            state.export_midi_note = v.clamp(0, 127);
        }
        Message::ExportCurrentInstrument => {
            let path = std::path::Path::new(&state.export_path_input);
            if state.export_path_input.trim().is_empty() {
                state.export_status = "Export failed: output path is empty".to_string();
                return Task::none();
            }
            if path.extension().is_none() {
                state.export_status =
                    "Export failed: output path must include file extension".to_string();
                return Task::none();
            }
            let Some(parent) = path.parent() else {
                state.export_status = "Export failed: invalid output path".to_string();
                return Task::none();
            };
            if !parent.exists() {
                state.export_status = format!(
                    "Export failed: directory does not exist ({})",
                    parent.display()
                );
                return Task::none();
            }

            let active_inst = state
                .shared
                .params
                .get(ParamId::new(0, ParamType::ActiveInstrument))
                as usize;
            let mut kit = state.shared.kit.lock();
            if active_inst >= kit.instruments.len() {
                state.export_status = "Export failed: active instrument out of range".to_string();
                return Task::none();
            }

            let inst = &mut kit.instruments[active_inst];
            let num_samples =
                ((inst.length_ms.max(1.0) * 0.001) * state.shared.sample_rate()) as usize;
            let mut left = vec![0.0f32; num_samples];
            let mut right = vec![0.0f32; num_samples];
            inst.render(&mut left, &mut right, num_samples, state.export_midi_note);

            let format = match state.export_format {
                1 => "flac",
                2 => "ogg",
                3 => "mp3",
                _ => "wav",
            };
            match crate::kick::export::export_audio(
                path,
                &left,
                &right,
                state.shared.sample_rate() as u32,
                format,
                state.export_channels as u16,
            ) {
                Ok(_) => {
                    if format == "wav" {
                        let sfz = path.with_extension("sfz");
                        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                            let _ =
                                crate::kick::export::export_sfz(&sfz, name, state.export_midi_note);
                        }
                    }
                    state.export_status = format!(
                        "Exported {} ({}ch, note {})",
                        path.display(),
                        state.export_channels,
                        state.export_midi_note
                    );
                }
                Err(err) => {
                    state.export_status = format!("Export failed: {err}");
                }
            }
        }
        Message::LoadSample => {
            let path = std::path::Path::new(&state.sample_path_input);
            if path.exists()
                && let Ok((left, right, sr)) = crate::kick::export::decode_audio_to_f32(path)
            {
                let active_inst = state
                    .shared
                    .params
                    .get(ParamId::new(0, ParamType::ActiveInstrument))
                    as usize;
                let mut kit = state.shared.kit.lock();
                let layer_idx = state.sample_target_layer.min(2) as usize;
                let osc_idx = state.sample_target_osc.min(2) as usize;
                let osc = &mut kit.instruments[active_inst].layers[layer_idx].oscillators[osc_idx];
                // Use mono mix if stereo
                let samples: Vec<f32> = left
                    .iter()
                    .zip(right.iter())
                    .map(|(l, r)| (l + r) * 0.5)
                    .collect();
                osc.sample_buffer = Some(crate::kick::dsp::oscillator::SampleBuffer::new(
                    samples, sr as f32,
                ));
                osc.waveform = crate::kick::dsp::oscillator::Waveform::Sample;
                state
                    .shared
                    .kit_version
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            }
        }
        Message::InstrumentNameChanged(name) => {
            state.instrument_name_input = name.clone();
            let active_inst = state
                .shared
                .params
                .get(ParamId::new(0, ParamType::ActiveInstrument))
                as usize;
            let mut kit = state.shared.kit.lock();
            if active_inst < kit.instruments.len() {
                kit.instruments[active_inst].name = name;
                state
                    .shared
                    .kit_version
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            }
        }
        Message::MainTabChanged(tab) => {
            state.main_tab = tab.min(1);
        }
        Message::LayerTabChanged(tab) => {
            state.active_layer_tab = tab.min(2);
        }
        Message::OscTabChanged(tab) => {
            state.active_osc_tab = tab.min(2);
        }
    }
    Task::none()
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
struct InstOption {
    idx: u8,
    name: String,
}

impl std::fmt::Display for InstOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.name.is_empty() {
            write!(f, "{}", self.idx + 1)
        } else {
            write!(f, "{}: {}", self.idx + 1, self.name)
        }
    }
}

fn tab_button(label: &'static str, active: bool, msg: Message) -> Element<'static, Message> {
    maolan_baseview::iced::widget::button(
        container(text(label).size(11))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(Length::Fixed(48.0))
    .height(Length::Fixed(22.0))
    .padding(0)
    .style(move |theme: &Theme, status| {
        let mut base = if active {
            maolan_baseview::iced::widget::button::primary(theme, status)
        } else {
            maolan_baseview::iced::widget::button::secondary(theme, status)
        };
        base.border.radius = 4.0.into();
        base
    })
    .on_press(msg)
    .into()
}

fn view(state: &State) -> Element<'_, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;
    let (peak_db_l, peak_db_r) = state.shared.output_peak_db();

    // Active instrument for per-instrument param editing
    let active_inst = state
        .shared
        .params
        .get(ParamId::new(0, ParamType::ActiveInstrument)) as usize;
    let ap = |ty: ParamType| ParamId::new(active_inst as u8, ty);

    let waveform = canvas(WaveformState {
        shared: state.shared.clone(),
    })
    .width(Length::Fill)
    .height(Length::Fixed(140.0));

    let envelope_editor = if state.show_envelope_editor {
        let kit = state.shared.kit.lock();
        let env_kind = EnvelopeKind::from_u8(state.envelope_kind);
        let mut inst = kit.instruments[active_inst].clone();
        let env = selected_env(
            &mut inst,
            env_kind,
            state.envelope_layer as usize,
            state.envelope_osc as usize,
        )
        .clone();
        drop(kit);
        Some(
            canvas(EnvelopeEditor::new(env))
                .width(Length::Fill)
                .height(Length::Fixed(160.0)),
        )
    } else {
        None
    };

    let meter = container(meters(2, &[peak_db_l, peak_db_r], 120.0))
        .height(Length::Fixed(120.0))
        .width(Length::Fixed(48.0));

    let top_row = row![waveform, meter].spacing(8).align_y(Alignment::Center);

    // Instrument selector dropdown
    let inst_options: Vec<InstOption> = {
        let kit = state.shared.kit.lock();
        kit.instruments
            .iter()
            .enumerate()
            .map(|(i, inst)| InstOption {
                idx: i as u8,
                name: inst.name.clone(),
            })
            .collect()
    };
    let selected_inst = if active_inst < inst_options.len() {
        Some(inst_options[active_inst].clone())
    } else {
        None
    };
    let inst_dropdown = pick_list(inst_options, selected_inst, |opt| {
        Message::SetActiveInstrument(opt.idx)
    })
    .placeholder("Select instrument...")
    .width(Length::Fixed(200.0));

    let inst_name_input =
        maolan_baseview::iced::widget::text_input("Instrument name", &state.instrument_name_input)
            .on_input(Message::InstrumentNameChanged)
            .width(Length::Fixed(160.0));

    let inst_selector = row![inst_dropdown, inst_name_input]
        .spacing(8)
        .align_y(Alignment::Center);

    // Kit section
    let kit_section = column![
        section_header("KIT"),
        row![
            knob(
                "Humanizer Vel",
                ap(ParamType::HumanizerVelocity),
                p(ap(ParamType::HumanizerVelocity)),
                "",
                0.01
            ),
            knob(
                "Humanizer Time",
                ap(ParamType::HumanizerTiming),
                p(ap(ParamType::HumanizerTiming)),
                "ms",
                0.1
            ),
        ]
        .spacing(6),
        row![
            maolan_baseview::iced::widget::button("Copy").on_press(Message::CopyInstrument),
            maolan_baseview::iced::widget::button("Paste").on_press(Message::PasteInstrument),
            maolan_baseview::iced::widget::button("Dup").on_press(Message::DuplicateInstrument),
            maolan_baseview::iced::widget::button("Clear").on_press(Message::ClearInstrument),
        ]
        .spacing(6),
        row![
            knob(
                "MidiCh",
                ap(ParamType::MasterMidiChannel),
                p(ap(ParamType::MasterMidiChannel)),
                "",
                1.0
            ),
            knob(
                "KeyMin",
                ap(ParamType::MasterKeyMin),
                p(ap(ParamType::MasterKeyMin)),
                "",
                1.0
            ),
            knob(
                "KeyMax",
                ap(ParamType::MasterKeyMax),
                p(ap(ParamType::MasterKeyMax)),
                "",
                1.0
            ),
            knob(
                "PitchNote",
                ap(ParamType::MasterPitchToNote),
                p(ap(ParamType::MasterPitchToNote)),
                "",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "Mute",
                ap(ParamType::MasterMuted),
                p(ap(ParamType::MasterMuted)),
                "",
                1.0
            ),
            knob(
                "Solo",
                ap(ParamType::MasterSoloed),
                p(ap(ParamType::MasterSoloed)),
                "",
                1.0
            ),
        ]
        .spacing(6),
    ]
    .spacing(6);

    // Master section
    let master_section = column![
        section_header("MASTER"),
        row![
            knob(
                "Length",
                ap(ParamType::MasterLength),
                p(ap(ParamType::MasterLength)),
                "ms",
                1.0
            ),
            knob(
                "Gain",
                ap(ParamType::MasterOutputGain),
                p(ap(ParamType::MasterOutputGain)),
                "dB",
                0.1
            ),
            knob(
                "NoteOff",
                ap(ParamType::MasterNoteOffDecay),
                p(ap(ParamType::MasterNoteOffDecay)),
                "ms",
                1.0
            ),
            checkbox_param(
                "NO Enab",
                ap(ParamType::MasterNoteOffEnabled),
                state
                    .shared
                    .params
                    .get_bool(ap(ParamType::MasterNoteOffEnabled)),
            ),
        ]
        .spacing(6),
        row![
            knob(
                "FilterType",
                ap(ParamType::MasterFilterType),
                p(ap(ParamType::MasterFilterType)),
                "",
                1.0
            ),
            knob(
                "Cutoff",
                ap(ParamType::MasterFilterCutoff),
                p(ap(ParamType::MasterFilterCutoff)),
                "Hz",
                1.0
            ),
            knob(
                "Q",
                ap(ParamType::MasterFilterQ),
                p(ap(ParamType::MasterFilterQ)),
                "",
                0.01
            ),
        ]
        .spacing(6),
        row![
            knob(
                "DistType",
                ap(ParamType::MasterDistortionType),
                p(ap(ParamType::MasterDistortionType)),
                "",
                1.0
            ),
            knob(
                "DistDrive",
                ap(ParamType::MasterDistortionDrive),
                p(ap(ParamType::MasterDistortionDrive)),
                "",
                0.01
            ),
        ]
        .spacing(6),
        row![
            knob(
                "LimThresh",
                ap(ParamType::MasterLimiterThreshold),
                p(ap(ParamType::MasterLimiterThreshold)),
                "dB",
                0.1
            ),
            knob(
                "LimRel",
                ap(ParamType::MasterLimiterRelease),
                p(ap(ParamType::MasterLimiterRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
    ]
    .spacing(6);

    // Layer 0 section
    let layer0_section = column![
        section_header("LAYER 0"),
        row![
            checkbox_param(
                "Enabled",
                ap(ParamType::Layer0Enabled),
                state.shared.params.get_bool(ap(ParamType::Layer0Enabled)),
            ),
            knob(
                "Amp",
                ap(ParamType::Layer0Amp),
                p(ap(ParamType::Layer0Amp)),
                "",
                0.01
            ),
            knob(
                "FilterType",
                ap(ParamType::Layer0FilterType),
                p(ap(ParamType::Layer0FilterType)),
                "",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "Cutoff",
                ap(ParamType::Layer0FilterCutoff),
                p(ap(ParamType::Layer0FilterCutoff)),
                "Hz",
                1.0
            ),
            knob(
                "Q",
                ap(ParamType::Layer0FilterQ),
                p(ap(ParamType::Layer0FilterQ)),
                "",
                0.01
            ),
            knob(
                "DistType",
                ap(ParamType::Layer0DistortionType),
                p(ap(ParamType::Layer0DistortionType)),
                "",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "DistDrive",
                ap(ParamType::Layer0DistortionDrive),
                p(ap(ParamType::Layer0DistortionDrive)),
                "",
                0.01
            ),
            knob(
                "FM0->0",
                ap(ParamType::Layer0FmRouting0),
                p(ap(ParamType::Layer0FmRouting0)),
                "",
                1.0
            ),
            knob(
                "FM0->1",
                ap(ParamType::Layer0FmRouting1),
                p(ap(ParamType::Layer0FmRouting1)),
                "",
                1.0
            ),
            knob(
                "FM0->2",
                ap(ParamType::Layer0FmRouting2),
                p(ap(ParamType::Layer0FmRouting2)),
                "",
                1.0
            ),
        ]
        .spacing(6),
    ]
    .spacing(6);

    let layer1_section = column![
        section_header("LAYER 1"),
        row![
            checkbox_param(
                "Enabled",
                ap(ParamType::Layer1Enabled),
                state.shared.params.get_bool(ap(ParamType::Layer1Enabled)),
            ),
            knob(
                "Amp",
                ap(ParamType::Layer1Amp),
                p(ap(ParamType::Layer1Amp)),
                "",
                0.01
            ),
        ]
        .spacing(6),
    ]
    .spacing(6);

    let layer2_section = column![
        section_header("LAYER 2"),
        row![
            checkbox_param(
                "Enabled",
                ap(ParamType::Layer2Enabled),
                state.shared.params.get_bool(ap(ParamType::Layer2Enabled)),
            ),
            knob(
                "Amp",
                ap(ParamType::Layer2Amp),
                p(ap(ParamType::Layer2Amp)),
                "",
                0.01
            ),
        ]
        .spacing(6),
    ]
    .spacing(6);

    // Oscillator section (3 oscs)
    let osc_section = |label: &'static str,
                       waveform_ty: ParamType,
                       freq_ty: ParamType,
                       amp_ty: ParamType,
                       phase_ty: ParamType,
                       fm_ty: ParamType,
                       filter_type_ty: ParamType,
                       cutoff_ty: ParamType,
                       q_ty: ParamType,
                       dist_type_ty: ParamType,
                       dist_drive_ty: ParamType| {
        let w = ap(waveform_ty);
        let f = ap(freq_ty);
        let a = ap(amp_ty);
        let ph = ap(phase_ty);
        let fm = ap(fm_ty);
        let ft = ap(filter_type_ty);
        let c = ap(cutoff_ty);
        let qv = ap(q_ty);
        let dt = ap(dist_type_ty);
        let dd = ap(dist_drive_ty);
        column![
            section_header(label),
            row![
                knob("Wave", w, p(w), "", 1.0),
                knob("Freq", f, p(f), "Hz", 1.0),
                knob("Amp", a, p(a), "", 0.01),
            ]
            .spacing(6),
            row![
                knob("Phase", ph, p(ph), "", 0.01),
                knob("FM", fm, p(fm), "", 0.01),
                knob("FiltType", ft, p(ft), "", 1.0),
            ]
            .spacing(6),
            row![
                knob("Cutoff", c, p(c), "Hz", 1.0),
                knob("Q", qv, p(qv), "", 0.01),
                knob("DistType", dt, p(dt), "", 1.0),
            ]
            .spacing(6),
            row![knob("DistDrive", dd, p(dd), "", 0.01),].spacing(6),
        ]
        .spacing(6)
    };

    let osc0 = osc_section(
        "OSC 0",
        ParamType::Osc0Waveform,
        ParamType::Osc0Freq,
        ParamType::Osc0Amp,
        ParamType::Osc0Phase,
        ParamType::Osc0FmAmount,
        ParamType::Osc0FilterType,
        ParamType::Osc0FilterCutoff,
        ParamType::Osc0FilterQ,
        ParamType::Osc0DistortionType,
        ParamType::Osc0DistortionDrive,
    );
    let osc1 = osc_section(
        "OSC 1",
        ParamType::Osc1Waveform,
        ParamType::Osc1Freq,
        ParamType::Osc1Amp,
        ParamType::Osc1Phase,
        ParamType::Osc1FmAmount,
        ParamType::Osc1FilterType,
        ParamType::Osc1FilterCutoff,
        ParamType::Osc1FilterQ,
        ParamType::Osc1DistortionType,
        ParamType::Osc1DistortionDrive,
    );
    let osc2 = osc_section(
        "OSC 2",
        ParamType::Osc2Waveform,
        ParamType::Osc2Freq,
        ParamType::Osc2Amp,
        ParamType::Osc2Phase,
        ParamType::Osc2FmAmount,
        ParamType::Osc2FilterType,
        ParamType::Osc2FilterCutoff,
        ParamType::Osc2FilterQ,
        ParamType::Osc2DistortionType,
        ParamType::Osc2DistortionDrive,
    );

    // Noise section
    let noise_section = column![
        section_header("NOISE"),
        row![
            knob(
                "Type",
                ap(ParamType::NoiseType),
                p(ap(ParamType::NoiseType)),
                "",
                1.0
            ),
            knob(
                "Amp",
                ap(ParamType::NoiseAmp),
                p(ap(ParamType::NoiseAmp)),
                "",
                0.01
            ),
            knob(
                "Density",
                ap(ParamType::NoiseDensity),
                p(ap(ParamType::NoiseDensity)),
                "",
                0.01
            ),
        ]
        .spacing(6),
        row![
            knob(
                "FiltType",
                ap(ParamType::NoiseFilterType),
                p(ap(ParamType::NoiseFilterType)),
                "",
                1.0
            ),
            knob(
                "Cutoff",
                ap(ParamType::NoiseFilterCutoff),
                p(ap(ParamType::NoiseFilterCutoff)),
                "Hz",
                1.0
            ),
            knob(
                "Q",
                ap(ParamType::NoiseFilterQ),
                p(ap(ParamType::NoiseFilterQ)),
                "",
                0.01
            ),
        ]
        .spacing(6),
    ]
    .spacing(6);

    // Envelope section
    let _env_section = column![
        section_header("ENVELOPES"),
        row![
            knob(
                "Osc0A",
                ap(ParamType::Osc0AmpEnvAttack),
                p(ap(ParamType::Osc0AmpEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "Osc0D",
                ap(ParamType::Osc0AmpEnvDecay),
                p(ap(ParamType::Osc0AmpEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "Osc0S",
                ap(ParamType::Osc0AmpEnvSustain),
                p(ap(ParamType::Osc0AmpEnvSustain)),
                "",
                0.01
            ),
            knob(
                "Osc0R",
                ap(ParamType::Osc0AmpEnvRelease),
                p(ap(ParamType::Osc0AmpEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "NoiseA",
                ap(ParamType::NoiseAmpEnvAttack),
                p(ap(ParamType::NoiseAmpEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "NoiseD",
                ap(ParamType::NoiseAmpEnvDecay),
                p(ap(ParamType::NoiseAmpEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "NoiseS",
                ap(ParamType::NoiseAmpEnvSustain),
                p(ap(ParamType::NoiseAmpEnvSustain)),
                "",
                0.01
            ),
            knob(
                "NoiseR",
                ap(ParamType::NoiseAmpEnvRelease),
                p(ap(ParamType::NoiseAmpEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "Osc1A",
                ap(ParamType::Osc1AmpEnvAttack),
                p(ap(ParamType::Osc1AmpEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "Osc1D",
                ap(ParamType::Osc1AmpEnvDecay),
                p(ap(ParamType::Osc1AmpEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "Osc1S",
                ap(ParamType::Osc1AmpEnvSustain),
                p(ap(ParamType::Osc1AmpEnvSustain)),
                "",
                0.01
            ),
            knob(
                "Osc1R",
                ap(ParamType::Osc1AmpEnvRelease),
                p(ap(ParamType::Osc1AmpEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "Osc2A",
                ap(ParamType::Osc2AmpEnvAttack),
                p(ap(ParamType::Osc2AmpEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "Osc2D",
                ap(ParamType::Osc2AmpEnvDecay),
                p(ap(ParamType::Osc2AmpEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "Osc2S",
                ap(ParamType::Osc2AmpEnvSustain),
                p(ap(ParamType::Osc2AmpEnvSustain)),
                "",
                0.01
            ),
            knob(
                "Osc2R",
                ap(ParamType::Osc2AmpEnvRelease),
                p(ap(ParamType::Osc2AmpEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "DensA",
                ap(ParamType::NoiseDensityEnvAttack),
                p(ap(ParamType::NoiseDensityEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "DensD",
                ap(ParamType::NoiseDensityEnvDecay),
                p(ap(ParamType::NoiseDensityEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "DensS",
                ap(ParamType::NoiseDensityEnvSustain),
                p(ap(ParamType::NoiseDensityEnvSustain)),
                "",
                0.01
            ),
            knob(
                "DensR",
                ap(ParamType::NoiseDensityEnvRelease),
                p(ap(ParamType::NoiseDensityEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "MasterA",
                ap(ParamType::MasterGlobalAmpEnvAttack),
                p(ap(ParamType::MasterGlobalAmpEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "MasterD",
                ap(ParamType::MasterGlobalAmpEnvDecay),
                p(ap(ParamType::MasterGlobalAmpEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "MasterS",
                ap(ParamType::MasterGlobalAmpEnvSustain),
                p(ap(ParamType::MasterGlobalAmpEnvSustain)),
                "",
                0.01
            ),
            knob(
                "MasterR",
                ap(ParamType::MasterGlobalAmpEnvRelease),
                p(ap(ParamType::MasterGlobalAmpEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "O0FC A",
                ap(ParamType::Osc0FilterCutoffEnvAttack),
                p(ap(ParamType::Osc0FilterCutoffEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "O0FC D",
                ap(ParamType::Osc0FilterCutoffEnvDecay),
                p(ap(ParamType::Osc0FilterCutoffEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "O0FC S",
                ap(ParamType::Osc0FilterCutoffEnvSustain),
                p(ap(ParamType::Osc0FilterCutoffEnvSustain)),
                "",
                0.01
            ),
            knob(
                "O0FC R",
                ap(ParamType::Osc0FilterCutoffEnvRelease),
                p(ap(ParamType::Osc0FilterCutoffEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "O1FC A",
                ap(ParamType::Osc1FilterCutoffEnvAttack),
                p(ap(ParamType::Osc1FilterCutoffEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "O1FC D",
                ap(ParamType::Osc1FilterCutoffEnvDecay),
                p(ap(ParamType::Osc1FilterCutoffEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "O1FC S",
                ap(ParamType::Osc1FilterCutoffEnvSustain),
                p(ap(ParamType::Osc1FilterCutoffEnvSustain)),
                "",
                0.01
            ),
            knob(
                "O1FC R",
                ap(ParamType::Osc1FilterCutoffEnvRelease),
                p(ap(ParamType::Osc1FilterCutoffEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "O2FC A",
                ap(ParamType::Osc2FilterCutoffEnvAttack),
                p(ap(ParamType::Osc2FilterCutoffEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "O2FC D",
                ap(ParamType::Osc2FilterCutoffEnvDecay),
                p(ap(ParamType::Osc2FilterCutoffEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "O2FC S",
                ap(ParamType::Osc2FilterCutoffEnvSustain),
                p(ap(ParamType::Osc2FilterCutoffEnvSustain)),
                "",
                0.01
            ),
            knob(
                "O2FC R",
                ap(ParamType::Osc2FilterCutoffEnvRelease),
                p(ap(ParamType::Osc2FilterCutoffEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "O0Q A",
                ap(ParamType::Osc0FilterQEnvAttack),
                p(ap(ParamType::Osc0FilterQEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "O0Q D",
                ap(ParamType::Osc0FilterQEnvDecay),
                p(ap(ParamType::Osc0FilterQEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "O0Q S",
                ap(ParamType::Osc0FilterQEnvSustain),
                p(ap(ParamType::Osc0FilterQEnvSustain)),
                "",
                0.01
            ),
            knob(
                "O0Q R",
                ap(ParamType::Osc0FilterQEnvRelease),
                p(ap(ParamType::Osc0FilterQEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "O1Q A",
                ap(ParamType::Osc1FilterQEnvAttack),
                p(ap(ParamType::Osc1FilterQEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "O1Q D",
                ap(ParamType::Osc1FilterQEnvDecay),
                p(ap(ParamType::Osc1FilterQEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "O1Q S",
                ap(ParamType::Osc1FilterQEnvSustain),
                p(ap(ParamType::Osc1FilterQEnvSustain)),
                "",
                0.01
            ),
            knob(
                "O1Q R",
                ap(ParamType::Osc1FilterQEnvRelease),
                p(ap(ParamType::Osc1FilterQEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "O2Q A",
                ap(ParamType::Osc2FilterQEnvAttack),
                p(ap(ParamType::Osc2FilterQEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "O2Q D",
                ap(ParamType::Osc2FilterQEnvDecay),
                p(ap(ParamType::Osc2FilterQEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "O2Q S",
                ap(ParamType::Osc2FilterQEnvSustain),
                p(ap(ParamType::Osc2FilterQEnvSustain)),
                "",
                0.01
            ),
            knob(
                "O2Q R",
                ap(ParamType::Osc2FilterQEnvRelease),
                p(ap(ParamType::Osc2FilterQEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "O0DRV A",
                ap(ParamType::Osc0DistortionDriveEnvAttack),
                p(ap(ParamType::Osc0DistortionDriveEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "O0DRV D",
                ap(ParamType::Osc0DistortionDriveEnvDecay),
                p(ap(ParamType::Osc0DistortionDriveEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "O0DRV S",
                ap(ParamType::Osc0DistortionDriveEnvSustain),
                p(ap(ParamType::Osc0DistortionDriveEnvSustain)),
                "",
                0.01
            ),
            knob(
                "O0DRV R",
                ap(ParamType::Osc0DistortionDriveEnvRelease),
                p(ap(ParamType::Osc0DistortionDriveEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "O1DRV A",
                ap(ParamType::Osc1DistortionDriveEnvAttack),
                p(ap(ParamType::Osc1DistortionDriveEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "O1DRV D",
                ap(ParamType::Osc1DistortionDriveEnvDecay),
                p(ap(ParamType::Osc1DistortionDriveEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "O1DRV S",
                ap(ParamType::Osc1DistortionDriveEnvSustain),
                p(ap(ParamType::Osc1DistortionDriveEnvSustain)),
                "",
                0.01
            ),
            knob(
                "O1DRV R",
                ap(ParamType::Osc1DistortionDriveEnvRelease),
                p(ap(ParamType::Osc1DistortionDriveEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "O2DRV A",
                ap(ParamType::Osc2DistortionDriveEnvAttack),
                p(ap(ParamType::Osc2DistortionDriveEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "O2DRV D",
                ap(ParamType::Osc2DistortionDriveEnvDecay),
                p(ap(ParamType::Osc2DistortionDriveEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "O2DRV S",
                ap(ParamType::Osc2DistortionDriveEnvSustain),
                p(ap(ParamType::Osc2DistortionDriveEnvSustain)),
                "",
                0.01
            ),
            knob(
                "O2DRV R",
                ap(ParamType::Osc2DistortionDriveEnvRelease),
                p(ap(ParamType::Osc2DistortionDriveEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "O0PS A",
                ap(ParamType::Osc0PitchShiftEnvAttack),
                p(ap(ParamType::Osc0PitchShiftEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "O0PS D",
                ap(ParamType::Osc0PitchShiftEnvDecay),
                p(ap(ParamType::Osc0PitchShiftEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "O0PS S",
                ap(ParamType::Osc0PitchShiftEnvSustain),
                p(ap(ParamType::Osc0PitchShiftEnvSustain)),
                "",
                0.01
            ),
            knob(
                "O0PS R",
                ap(ParamType::Osc0PitchShiftEnvRelease),
                p(ap(ParamType::Osc0PitchShiftEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "O1PS A",
                ap(ParamType::Osc1PitchShiftEnvAttack),
                p(ap(ParamType::Osc1PitchShiftEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "O1PS D",
                ap(ParamType::Osc1PitchShiftEnvDecay),
                p(ap(ParamType::Osc1PitchShiftEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "O1PS S",
                ap(ParamType::Osc1PitchShiftEnvSustain),
                p(ap(ParamType::Osc1PitchShiftEnvSustain)),
                "",
                0.01
            ),
            knob(
                "O1PS R",
                ap(ParamType::Osc1PitchShiftEnvRelease),
                p(ap(ParamType::Osc1PitchShiftEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "O2PS A",
                ap(ParamType::Osc2PitchShiftEnvAttack),
                p(ap(ParamType::Osc2PitchShiftEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "O2PS D",
                ap(ParamType::Osc2PitchShiftEnvDecay),
                p(ap(ParamType::Osc2PitchShiftEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "O2PS S",
                ap(ParamType::Osc2PitchShiftEnvSustain),
                p(ap(ParamType::Osc2PitchShiftEnvSustain)),
                "",
                0.01
            ),
            knob(
                "O2PS R",
                ap(ParamType::Osc2PitchShiftEnvRelease),
                p(ap(ParamType::Osc2PitchShiftEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "O0F A",
                ap(ParamType::Osc0FreqEnvAttack),
                p(ap(ParamType::Osc0FreqEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "O0F D",
                ap(ParamType::Osc0FreqEnvDecay),
                p(ap(ParamType::Osc0FreqEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "O0F S",
                ap(ParamType::Osc0FreqEnvSustain),
                p(ap(ParamType::Osc0FreqEnvSustain)),
                "",
                0.01
            ),
            knob(
                "O0F R",
                ap(ParamType::Osc0FreqEnvRelease),
                p(ap(ParamType::Osc0FreqEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "O1F A",
                ap(ParamType::Osc1FreqEnvAttack),
                p(ap(ParamType::Osc1FreqEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "O1F D",
                ap(ParamType::Osc1FreqEnvDecay),
                p(ap(ParamType::Osc1FreqEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "O1F S",
                ap(ParamType::Osc1FreqEnvSustain),
                p(ap(ParamType::Osc1FreqEnvSustain)),
                "",
                0.01
            ),
            knob(
                "O1F R",
                ap(ParamType::Osc1FreqEnvRelease),
                p(ap(ParamType::Osc1FreqEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "O2F A",
                ap(ParamType::Osc2FreqEnvAttack),
                p(ap(ParamType::Osc2FreqEnvAttack)),
                "ms",
                0.1
            ),
            knob(
                "O2F D",
                ap(ParamType::Osc2FreqEnvDecay),
                p(ap(ParamType::Osc2FreqEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "O2F S",
                ap(ParamType::Osc2FreqEnvSustain),
                p(ap(ParamType::Osc2FreqEnvSustain)),
                "",
                0.01
            ),
            knob(
                "O2F R",
                ap(ParamType::Osc2FreqEnvRelease),
                p(ap(ParamType::Osc2FreqEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
        row![
            knob(
                "O0F Mode",
                ap(ParamType::Osc0FreqEnvMode),
                p(ap(ParamType::Osc0FreqEnvMode)),
                "",
                1.0
            ),
            knob(
                "O1F Mode",
                ap(ParamType::Osc1FreqEnvMode),
                p(ap(ParamType::Osc1FreqEnvMode)),
                "",
                1.0
            ),
            knob(
                "O2F Mode",
                ap(ParamType::Osc2FreqEnvMode),
                p(ap(ParamType::Osc2FreqEnvMode)),
                "",
                1.0
            ),
            knob(
                "MDRV A",
                ap(ParamType::MasterDistortionVolEnvAttack),
                p(ap(ParamType::MasterDistortionVolEnvAttack)),
                "ms",
                0.1
            ),
        ]
        .spacing(6),
        row![
            knob(
                "MDRV D",
                ap(ParamType::MasterDistortionVolEnvDecay),
                p(ap(ParamType::MasterDistortionVolEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "MDRV S",
                ap(ParamType::MasterDistortionVolEnvSustain),
                p(ap(ParamType::MasterDistortionVolEnvSustain)),
                "",
                0.01
            ),
            knob(
                "MDRV R",
                ap(ParamType::MasterDistortionVolEnvRelease),
                p(ap(ParamType::MasterDistortionVolEnvRelease)),
                "ms",
                1.0
            ),
            knob(
                "L0DRV A",
                ap(ParamType::Layer0DistortionVolEnvAttack),
                p(ap(ParamType::Layer0DistortionVolEnvAttack)),
                "ms",
                0.1
            ),
        ]
        .spacing(6),
        row![
            knob(
                "L0DRV D",
                ap(ParamType::Layer0DistortionVolEnvDecay),
                p(ap(ParamType::Layer0DistortionVolEnvDecay)),
                "ms",
                1.0
            ),
            knob(
                "L0DRV S",
                ap(ParamType::Layer0DistortionVolEnvSustain),
                p(ap(ParamType::Layer0DistortionVolEnvSustain)),
                "",
                0.01
            ),
            knob(
                "L0DRV R",
                ap(ParamType::Layer0DistortionVolEnvRelease),
                p(ap(ParamType::Layer0DistortionVolEnvRelease)),
                "ms",
                1.0
            ),
        ]
        .spacing(6),
    ]
    .spacing(6);

    // Preset browser section
    let preset_dropdown = pick_list(
        state.preset_files.clone(),
        None::<String>,
        Message::LoadPreset,
    )
    .placeholder("Select preset...")
    .width(Length::Fixed(220.0));

    let preset_section = column![
        section_header("PRESETS"),
        row![
            maolan_baseview::iced::widget::text_input("Preset name", &state.preset_name_input)
                .on_input(Message::PresetNameChanged)
                .width(Length::Fixed(140.0)),
            maolan_baseview::iced::widget::button("Save").on_press(Message::SavePreset),
            maolan_baseview::iced::widget::button("Refresh").on_press(Message::RefreshPresets),
        ]
        .spacing(4),
        preset_dropdown,
    ]
    .spacing(6);

    // Sample browser section
    let sample_section = column![
        section_header("SAMPLE"),
        row![
            maolan_baseview::iced::widget::text_input("Path to sample", &state.sample_path_input)
                .on_input(Message::SamplePathChanged)
                .width(Length::Fixed(200.0)),
            maolan_baseview::iced::widget::button("Load").on_press(Message::LoadSample),
        ]
        .spacing(4),
        row![
            maolan_baseview::iced::widget::text("Layer"),
            maolan_baseview::iced::widget::slider(
                0.0..=2.0,
                state.sample_target_layer as f32,
                |v| { Message::SampleTargetLayerChanged(v.round().clamp(0.0, 2.0) as u8) }
            )
            .step(1.0)
            .width(Length::Fixed(80.0)),
            maolan_baseview::iced::widget::text(format!("{}", state.sample_target_layer + 1)),
            maolan_baseview::iced::widget::text("Osc"),
            maolan_baseview::iced::widget::slider(0.0..=2.0, state.sample_target_osc as f32, |v| {
                Message::SampleTargetOscChanged(v.round().clamp(0.0, 2.0) as u8)
            })
            .step(1.0)
            .width(Length::Fixed(80.0)),
            maolan_baseview::iced::widget::text(format!("{}", state.sample_target_osc + 1)),
        ]
        .spacing(6),
    ]
    .spacing(6);

    let env_kind = EnvelopeKind::from_u8(state.envelope_kind);
    let envelope_target_section = column![
        section_header("ENVELOPE TARGET"),
        row![
            maolan_baseview::iced::widget::text(env_kind.label()),
            maolan_baseview::iced::widget::slider(0.0..=11.0, state.envelope_kind as f32, |v| {
                Message::EnvelopeKindChanged(v.round().clamp(0.0, 11.0) as u8)
            })
            .step(1.0)
            .width(Length::Fixed(170.0)),
        ]
        .spacing(6),
        row![
            maolan_baseview::iced::widget::text("Layer"),
            maolan_baseview::iced::widget::slider(0.0..=2.0, state.envelope_layer as f32, |v| {
                Message::EnvelopeLayerChanged(v.round().clamp(0.0, 2.0) as u8)
            })
            .step(1.0)
            .width(Length::Fixed(90.0)),
            maolan_baseview::iced::widget::text(format!("{}", state.envelope_layer + 1)),
            maolan_baseview::iced::widget::text("Osc"),
            maolan_baseview::iced::widget::slider(0.0..=2.0, state.envelope_osc as f32, |v| {
                Message::EnvelopeOscChanged(v.round().clamp(0.0, 2.0) as u8)
            })
            .step(1.0)
            .width(Length::Fixed(90.0)),
            maolan_baseview::iced::widget::text(format!("{}", state.envelope_osc + 1)),
        ]
        .spacing(6),
    ]
    .spacing(6);

    let export_section = column![
        section_header("EXPORT"),
        row![
            maolan_baseview::iced::widget::text_input("Output path", &state.export_path_input)
                .on_input(Message::ExportPathChanged)
                .width(Length::Fixed(220.0)),
        ]
        .spacing(6),
        row![
            maolan_baseview::iced::widget::text("Fmt"),
            maolan_baseview::iced::widget::slider(0.0..=3.0, state.export_format as f32, |v| {
                Message::ExportFormatChanged(v.round().clamp(0.0, 3.0) as u8)
            })
            .step(1.0)
            .width(Length::Fixed(90.0)),
            maolan_baseview::iced::widget::text(match state.export_format {
                1 => "FLAC",
                2 => "OGG",
                3 => "MP3",
                _ => "WAV",
            }),
            maolan_baseview::iced::widget::text("Ch"),
            maolan_baseview::iced::widget::slider(1.0..=2.0, state.export_channels as f32, |v| {
                Message::ExportChannelsChanged(v.round().clamp(1.0, 2.0) as u8)
            })
            .step(1.0)
            .width(Length::Fixed(70.0)),
            maolan_baseview::iced::widget::text(format!("{}", state.export_channels)),
        ]
        .spacing(6),
        row![
            maolan_baseview::iced::widget::text("MIDI"),
            maolan_baseview::iced::widget::slider(
                0.0..=127.0,
                state.export_midi_note as f32,
                |v| Message::ExportMidiNoteChanged(v.round().clamp(0.0, 127.0) as u8)
            )
            .step(1.0)
            .width(Length::Fixed(180.0)),
            maolan_baseview::iced::widget::text(format!("{}", state.export_midi_note)),
            maolan_baseview::iced::widget::button("Export")
                .on_press(Message::ExportCurrentInstrument),
        ]
        .spacing(6),
        maolan_baseview::iced::widget::text(state.export_status.clone()).size(9),
    ]
    .spacing(6);

    // Main tab bar
    let main_tab_bar = row![
        tab_button("Synth", state.main_tab == 0, Message::MainTabChanged(0)),
        tab_button("Setup", state.main_tab == 1, Message::MainTabChanged(1)),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let controls: Element<'_, Message> = if state.main_tab == 0 {
        // Layer tabs
        let layer_tabs = row![
            tab_button(
                "L1",
                state.active_layer_tab == 0,
                Message::LayerTabChanged(0)
            ),
            tab_button(
                "L2",
                state.active_layer_tab == 1,
                Message::LayerTabChanged(1)
            ),
            tab_button(
                "L3",
                state.active_layer_tab == 2,
                Message::LayerTabChanged(2)
            ),
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        // Osc tabs
        let osc_tabs = row![
            tab_button("Osc1", state.active_osc_tab == 0, Message::OscTabChanged(0)),
            tab_button("Osc2", state.active_osc_tab == 1, Message::OscTabChanged(1)),
            tab_button("Osc3", state.active_osc_tab == 2, Message::OscTabChanged(2)),
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        // Active layer
        let active_layer = match state.active_layer_tab {
            0 => layer0_section,
            1 => layer1_section,
            2 => layer2_section,
            _ => layer0_section,
        };

        // Active osc
        let active_osc = match state.active_osc_tab {
            0 => osc0,
            1 => osc1,
            2 => osc2,
            _ => osc0,
        };

        // Envelope ADSR for selected target
        let env_adsr: Element<'_, Message> = if let Some((a_ty, d_ty, s_ty, r_ty)) =
            envelope_param_types(
                EnvelopeKind::from_u8(state.envelope_kind),
                state.envelope_layer,
                state.envelope_osc,
            ) {
            row![
                knob("Attack", ap(a_ty), p(ap(a_ty)), "ms", 0.1),
                knob("Decay", ap(d_ty), p(ap(d_ty)), "ms", 1.0),
                knob("Sustain", ap(s_ty), p(ap(s_ty)), "", 0.01),
                knob("Release", ap(r_ty), p(ap(r_ty)), "ms", 1.0),
            ]
            .spacing(6)
            .into()
        } else {
            text("No envelope").size(11).into()
        };

        let synth_left = column![
            layer_tabs,
            active_layer,
            osc_tabs,
            active_osc,
            noise_section,
        ]
        .spacing(8)
        .align_x(Alignment::Start);

        let synth_right = column![
            master_section,
            envelope_target_section,
            section_header("ENVELOPE"),
            env_adsr,
        ]
        .spacing(8)
        .align_x(Alignment::Start);

        row![synth_left, synth_right]
            .spacing(8)
            .align_y(Alignment::Start)
            .into()
    } else {
        // Setup tab
        let setup_left = column![kit_section, sample_section]
            .spacing(8)
            .align_x(Alignment::Start);
        let setup_right = column![preset_section, export_section]
            .spacing(8)
            .align_x(Alignment::Start);
        row![setup_left, setup_right]
            .spacing(8)
            .align_y(Alignment::Start)
            .into()
    };

    let mut content = column![top_row, inst_selector, main_tab_bar, controls]
        .spacing(8)
        .align_x(Alignment::Start);
    if state.main_tab == 0
        && let Some(editor) = envelope_editor
    {
        let editor_el: Element<'_, EnvelopeEditorMsg> = editor.into();
        let mapped: Element<'_, Message> = editor_el.map(Message::EnvelopeEdit);
        content = content.push(
            maolan_baseview::iced::widget::container(mapped)
                .width(Length::Fill)
                .height(Length::Fixed(160.0)),
        );
    }

    container(content)
        .padding(10)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Left)
        .align_y(Vertical::Top)
        .into()
}

fn theme(_state: &State) -> Theme {
    Theme::TokyoNight
}

fn section_header(label: &'static str) -> Element<'static, Message> {
    text(label).size(11).into()
}

fn knob(
    label: &'static str,
    id: ParamId,
    value: f32,
    units: &'static str,
    step: f32,
) -> Element<'static, Message> {
    let def = param_type_def(id.param_type());
    let slider = arch_slider(def.min as f32..=def.max as f32, value, move |v| {
        Message::SetParam(id, v)
    })
    .step(step)
    .double_click_reset(def.default as f32)
    .on_release(Message::ReleaseParam(id))
    .fill_from_start()
    .width(Length::Fixed(52.0))
    .height(Length::Fixed(52.0));

    let value_text = if units.is_empty() {
        format!("{value:.2}")
    } else {
        format!("{value:.1} {units}")
    };

    container(
        column![text(label).size(9), slider, text(value_text).size(8)]
            .spacing(1)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(60.0))
    .into()
}

fn checkbox_param(label: &'static str, id: ParamId, value: bool) -> Element<'static, Message> {
    container(
        column![
            text(label).size(9),
            checkbox(value)
                .label("")
                .on_toggle(move |v| Message::SetBoolParam(id, v))
        ]
        .spacing(1)
        .align_x(Alignment::Center),
    )
    .width(Length::Fixed(60.0))
    .into()
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

        let settings = maolan_baseview::iced::IcedBaseviewSettings {
            window: maolan_baseview::iced::baseview::WindowOpenOptions {
                title: String::from("Maolan Kick"),
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
            if self.floating_open.swap(true, Ordering::AcqRel) {
                return true;
            }
            let Some(shared) = self.shared.clone() else {
                self.floating_open.store(false, Ordering::Release);
                return false;
            };
            let open_flag = self.floating_open.clone();
            thread::spawn(move || {
                let settings = maolan_baseview::iced::IcedBaseviewSettings {
                    window: maolan_baseview::iced::baseview::WindowOpenOptions {
                        title: String::from("Maolan Kick"),
                        size: maolan_baseview::iced::baseview::Size::new(
                            EDITOR_WIDTH as f64,
                            EDITOR_HEIGHT as f64,
                        ),
                        scale:
                            maolan_baseview::iced::baseview::WindowScalePolicy::SystemScaleFactor,
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
        }
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

    pub fn size(&self) -> (u32, u32) {
        (EDITOR_WIDTH, EDITOR_HEIGHT)
    }
}
