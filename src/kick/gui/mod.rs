use std::{
    ffi::CStr,
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
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{canvas, checkbox, column, container, pick_list, row, slider, text},
};
use maolan_widgets::arch_slider::arch_slider;
use maolan_widgets::meters::meters;

use crate::common::ui::{
    FADER_MAX_DB, FADER_MIN_DB, SmallKnob, VerticalSlider, small_knob, vertical_slider,
};
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};

mod envelope_editor;

use crate::common::distortion::DistortionType;
use crate::common::filter::FilterType;
use crate::kick::dsp::{INSTRUMENTS_PER_KIT, noise::NoiseType, oscillator::Waveform};
use crate::kick::gui::envelope_editor::{EnvelopeEditor, EnvelopeEditorMsg, EnvelopeScale};
use crate::kick::params::{ParamId, ParamType, param_type_def};
use crate::kick::plugin::SharedState;

pub const EDITOR_WIDTH: u32 = 1024;
pub const EDITOR_HEIGHT: u32 = 720;

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
    AddInstrument,
    RemoveInstrument,
    EnvelopeEdit(EnvelopeEditorMsg),
    SavePreset,
    EnvelopeKindChanged(u8),
    EnvelopeLayerChanged(u8),
    EnvelopeOscChanged(u8),
    SelectOscEnvelope {
        kind: u8,
        osc: u8,
    },
    EnvelopeRenderSourceChanged {
        layer: usize,
        source: usize,
        enabled: bool,
    },
    LayerTabChanged(u8),
    OscTabChanged(u8),
    InstrumentNameChanged(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvelopeKind {
    OscAmp = 0,
    OscFreq = 1,
    NoiseAmp = 2,
    NoiseDensity = 3,
}

const NO_ENVELOPE_SELECTION: u8 = u8::MAX;

impl EnvelopeKind {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::OscFreq,
            2 => Self::NoiseAmp,
            3 => Self::NoiseDensity,
            _ => Self::OscAmp,
        }
    }

    fn scale(self, base_freq_hz: f32) -> EnvelopeScale {
        match self {
            Self::OscFreq => EnvelopeScale::Frequency {
                base_hz: base_freq_hz,
            },
            Self::OscAmp | Self::NoiseAmp => EnvelopeScale::Bipolar,
            _ => EnvelopeScale::Normal,
        }
    }
}

#[derive(Clone, Copy)]
struct GuiOscParams {
    waveform: ParamType,
    freq: ParamType,
    amp: ParamType,
    phase: ParamType,
    fm_amount: ParamType,
    filter_type: ParamType,
    filter_cutoff: ParamType,
    filter_q: ParamType,
    distortion_type: ParamType,
    distortion_drive: ParamType,
}

#[derive(Clone, Copy)]
struct GuiNoiseParams {
    noise_type: ParamType,
    amp: ParamType,
    density: ParamType,
    filter_type: ParamType,
    filter_cutoff: ParamType,
    filter_q: ParamType,
}

#[derive(Clone, Copy)]
struct GuiLayerParams {
    osc0: GuiOscParams,
    osc1: GuiOscParams,
    noise: GuiNoiseParams,
}

fn gui_layer_params(layer_idx: u8) -> GuiLayerParams {
    match layer_idx {
        1 => GuiLayerParams {
            osc0: GuiOscParams {
                waveform: ParamType::Layer1Osc0Waveform,
                freq: ParamType::Layer1Osc0Freq,
                amp: ParamType::Layer1Osc0Amp,
                phase: ParamType::Layer1Osc0Phase,
                fm_amount: ParamType::Layer1Osc0FmAmount,
                filter_type: ParamType::Layer1Osc0FilterType,
                filter_cutoff: ParamType::Layer1Osc0FilterCutoff,
                filter_q: ParamType::Layer1Osc0FilterQ,
                distortion_type: ParamType::Layer1Osc0DistortionType,
                distortion_drive: ParamType::Layer1Osc0DistortionDrive,
            },
            osc1: GuiOscParams {
                waveform: ParamType::Layer1Osc1Waveform,
                freq: ParamType::Layer1Osc1Freq,
                amp: ParamType::Layer1Osc1Amp,
                phase: ParamType::Layer1Osc1Phase,
                fm_amount: ParamType::Layer1Osc1FmAmount,
                filter_type: ParamType::Layer1Osc1FilterType,
                filter_cutoff: ParamType::Layer1Osc1FilterCutoff,
                filter_q: ParamType::Layer1Osc1FilterQ,
                distortion_type: ParamType::Layer1Osc1DistortionType,
                distortion_drive: ParamType::Layer1Osc1DistortionDrive,
            },
            noise: GuiNoiseParams {
                noise_type: ParamType::Layer1NoiseType,
                amp: ParamType::Layer1NoiseAmp,
                density: ParamType::Layer1NoiseDensity,
                filter_type: ParamType::Layer1NoiseFilterType,
                filter_cutoff: ParamType::Layer1NoiseFilterCutoff,
                filter_q: ParamType::Layer1NoiseFilterQ,
            },
        },
        2 => GuiLayerParams {
            osc0: GuiOscParams {
                waveform: ParamType::Layer2Osc0Waveform,
                freq: ParamType::Layer2Osc0Freq,
                amp: ParamType::Layer2Osc0Amp,
                phase: ParamType::Layer2Osc0Phase,
                fm_amount: ParamType::Layer2Osc0FmAmount,
                filter_type: ParamType::Layer2Osc0FilterType,
                filter_cutoff: ParamType::Layer2Osc0FilterCutoff,
                filter_q: ParamType::Layer2Osc0FilterQ,
                distortion_type: ParamType::Layer2Osc0DistortionType,
                distortion_drive: ParamType::Layer2Osc0DistortionDrive,
            },
            osc1: GuiOscParams {
                waveform: ParamType::Layer2Osc1Waveform,
                freq: ParamType::Layer2Osc1Freq,
                amp: ParamType::Layer2Osc1Amp,
                phase: ParamType::Layer2Osc1Phase,
                fm_amount: ParamType::Layer2Osc1FmAmount,
                filter_type: ParamType::Layer2Osc1FilterType,
                filter_cutoff: ParamType::Layer2Osc1FilterCutoff,
                filter_q: ParamType::Layer2Osc1FilterQ,
                distortion_type: ParamType::Layer2Osc1DistortionType,
                distortion_drive: ParamType::Layer2Osc1DistortionDrive,
            },
            noise: GuiNoiseParams {
                noise_type: ParamType::Layer2NoiseType,
                amp: ParamType::Layer2NoiseAmp,
                density: ParamType::Layer2NoiseDensity,
                filter_type: ParamType::Layer2NoiseFilterType,
                filter_cutoff: ParamType::Layer2NoiseFilterCutoff,
                filter_q: ParamType::Layer2NoiseFilterQ,
            },
        },
        _ => GuiLayerParams {
            osc0: GuiOscParams {
                waveform: ParamType::Osc0Waveform,
                freq: ParamType::Osc0Freq,
                amp: ParamType::Osc0Amp,
                phase: ParamType::Osc0Phase,
                fm_amount: ParamType::Osc0FmAmount,
                filter_type: ParamType::Osc0FilterType,
                filter_cutoff: ParamType::Osc0FilterCutoff,
                filter_q: ParamType::Osc0FilterQ,
                distortion_type: ParamType::Osc0DistortionType,
                distortion_drive: ParamType::Osc0DistortionDrive,
            },
            osc1: GuiOscParams {
                waveform: ParamType::Osc1Waveform,
                freq: ParamType::Osc1Freq,
                amp: ParamType::Osc1Amp,
                phase: ParamType::Osc1Phase,
                fm_amount: ParamType::Osc1FmAmount,
                filter_type: ParamType::Osc1FilterType,
                filter_cutoff: ParamType::Osc1FilterCutoff,
                filter_q: ParamType::Osc1FilterQ,
                distortion_type: ParamType::Osc1DistortionType,
                distortion_drive: ParamType::Osc1DistortionDrive,
            },
            noise: GuiNoiseParams {
                noise_type: ParamType::NoiseType,
                amp: ParamType::NoiseAmp,
                density: ParamType::NoiseDensity,
                filter_type: ParamType::NoiseFilterType,
                filter_cutoff: ParamType::NoiseFilterCutoff,
                filter_q: ParamType::NoiseFilterQ,
            },
        },
    }
}

struct State {
    shared: Arc<SharedState>,
    active_gestures: Vec<bool>,
    show_envelope_editor: bool,
    envelope_kind: u8,
    envelope_layer: u8,
    envelope_osc: u8,
    envelope_selections: [[u8; 3]; 3],
    envelope_render_sources: [[bool; 3]; 3],
    active_layer_tab: u8,
    active_osc_tab: u8,
    instrument_name_input: String,
}

fn presets_dir() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("maolan").join("kick").join("presets"))
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
            envelope_kind: EnvelopeKind::OscAmp as u8,
            envelope_layer: 0,
            envelope_osc: 0,
            envelope_selections: default_envelope_selections(),
            envelope_render_sources: [
                [true, false, false],
                [false, false, false],
                [false, false, false],
            ],
            active_layer_tab: 0,
            active_osc_tab: 0,
            instrument_name_input,
        },
        Task::none(),
    )
}

fn default_envelope_selections() -> [[u8; 3]; 3] {
    let mut selections = [[NO_ENVELOPE_SELECTION; 3]; 3];
    selections[0][0] = EnvelopeKind::OscAmp as u8;
    selections
}

fn envelope_source_idx(kind: EnvelopeKind, osc: u8) -> usize {
    match kind {
        EnvelopeKind::NoiseAmp | EnvelopeKind::NoiseDensity => 2,
        EnvelopeKind::OscAmp | EnvelopeKind::OscFreq => osc.min(1) as usize,
    }
}

fn set_selected_envelope(state: &mut State, layer: u8, source: u8, kind: u8) {
    let layer = layer.min(2);
    let source = source.min(2);
    let kind = kind.min(3);
    state.envelope_layer = layer;
    state.envelope_osc = if source == 2 { 0 } else { source };
    state.envelope_kind = kind;
    state.envelope_selections[layer as usize][source as usize] = kind;
    state.show_envelope_editor = true;
}

fn sync_envelope_to_active_source(state: &mut State) {
    let layer = state.active_layer_tab.min(2);
    let source = state.active_osc_tab.min(2);
    let kind = state.envelope_selections[layer as usize][source as usize];
    state.envelope_layer = layer;
    state.envelope_osc = if source == 2 { 0 } else { source };
    if kind != NO_ENVELOPE_SELECTION {
        state.envelope_kind = kind.min(3);
        state.show_envelope_editor = true;
    }
}

fn selected_env(
    inst: &mut crate::kick::dsp::Instrument,
    kind: EnvelopeKind,
    layer: usize,
    osc: usize,
) -> &mut crate::kick::dsp::Envelope {
    let layer = layer.min(2);
    let osc = osc.min(1);
    match kind {
        EnvelopeKind::OscAmp => inst.layers[layer].oscillators[osc].amp_env_mut(),
        EnvelopeKind::OscFreq => inst.layers[layer].oscillators[osc].freq_env_mut(),
        EnvelopeKind::NoiseAmp => &mut inst.layers[layer].noise.amp_env,
        EnvelopeKind::NoiseDensity => &mut inst.layers[layer].noise.density_env,
    }
}

fn osc_location_for_freq_param(param_type: ParamType) -> Option<(usize, usize)> {
    match param_type {
        ParamType::Osc0Freq => Some((0, 0)),
        ParamType::Osc1Freq => Some((0, 1)),
        ParamType::Layer1Osc0Freq => Some((1, 0)),
        ParamType::Layer1Osc1Freq => Some((1, 1)),
        ParamType::Layer2Osc0Freq => Some((2, 0)),
        ParamType::Layer2Osc1Freq => Some((2, 1)),
        _ => None,
    }
}

fn preserve_osc_freq_envelope_for_param_change(state: &State, id: ParamId, new_freq_hz: f32) {
    let Some((layer, osc)) = osc_location_for_freq_param(id.param_type()) else {
        return;
    };
    let old_freq_hz = (state.shared.params.get(id) as f32).max(0.1);
    let new_freq_hz = new_freq_hz.max(0.1);
    if (old_freq_hz - new_freq_hz).abs() <= f32::EPSILON {
        return;
    }

    let mut kit = state.shared.kit.lock();
    let Some(instrument) = kit.instruments.get_mut(id.instrument() as usize) else {
        return;
    };
    let Some(layer) = instrument.layers.get_mut(layer) else {
        return;
    };
    let Some(oscillator) = layer.oscillators.get_mut(osc) else {
        return;
    };
    oscillator.set_base_freq_hz_preserving_freq_env(new_freq_hz);
    state.shared.mark_kit_changed();
}

fn freq_to_log_knob(freq: f32, min: f32, max: f32) -> f32 {
    let min = min.max(0.1);
    let max = max.max(min);
    let freq = freq.clamp(min, max);
    if max <= min {
        0.0
    } else {
        (freq / min).ln() / (max / min).ln()
    }
}

fn log_knob_to_freq(position: f32, min: f32, max: f32) -> f32 {
    let min = min.max(0.1);
    let max = max.max(min);
    if max <= min {
        min
    } else {
        min * (max / min).powf(position.clamp(0.0, 1.0))
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
            preserve_osc_freq_envelope_for_param_change(state, id, value);
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
                state.shared.mark_kit_changed();
            }
        }
        Message::DuplicateInstrument => {
            let active_inst = state
                .shared
                .params
                .get(ParamId::new(0, ParamType::ActiveInstrument))
                as usize;
            let mut kit = state.shared.kit.lock();
            if active_inst < kit.instruments.len() && kit.instruments.len() < INSTRUMENTS_PER_KIT {
                let clone = kit.instruments[active_inst].clone();
                let new_idx = kit.instruments.len();
                kit.instruments.push(clone);
                for ty_idx in 0..ParamType::COUNT {
                    let src = ParamId::new(active_inst as u8, unsafe {
                        std::mem::transmute::<u8, ParamType>(ty_idx as u8)
                    });
                    let dst_id = ParamId::new(new_idx as u8, unsafe {
                        std::mem::transmute::<u8, ParamType>(ty_idx as u8)
                    });
                    state
                        .shared
                        .params
                        .set(dst_id, state.shared.params.get(src));
                }
                drop(kit);
                state.shared.set_param_outbound_only(
                    ParamId::new(0, ParamType::ActiveInstrument),
                    new_idx as f64,
                );
                state.instrument_name_input =
                    state.shared.kit.lock().instruments[new_idx].name.clone();
                state.shared.mark_kit_changed();
                state.shared.sync_output_port_count();
                state.shared.request_audio_ports_rescan();
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
                state.instrument_name_input = String::new();
                state.shared.mark_kit_changed();
            }
        }
        Message::AddInstrument => {
            let mut kit = state.shared.kit.lock();
            if kit.instruments.len() < INSTRUMENTS_PER_KIT {
                let new_idx = kit.instruments.len();
                kit.instruments.push(crate::kick::dsp::Instrument::new(
                    state.shared.sample_rate(),
                ));
                for ty_idx in 0..ParamType::COUNT {
                    let ty = unsafe { std::mem::transmute::<u8, ParamType>(ty_idx as u8) };
                    let id = ParamId::new(new_idx as u8, ty);
                    let def = param_type_def(ty);
                    state.shared.params.set(id, def.default);
                }
                drop(kit);
                state.shared.set_param_outbound_only(
                    ParamId::new(0, ParamType::ActiveInstrument),
                    new_idx as f64,
                );
                state.instrument_name_input = String::new();
                state.shared.mark_kit_changed();
                state.shared.sync_output_port_count();
                state.shared.request_audio_ports_rescan();
            }
        }
        Message::RemoveInstrument => {
            let active_inst = state
                .shared
                .params
                .get(ParamId::new(0, ParamType::ActiveInstrument))
                as usize;
            let mut kit = state.shared.kit.lock();
            if active_inst < kit.instruments.len() && kit.instruments.len() > 1 {
                kit.instruments.remove(active_inst);
                let count = kit.instruments.len();
                for src_idx in active_inst + 1..=count {
                    for ty_idx in 0..ParamType::COUNT {
                        let ty = unsafe { std::mem::transmute::<u8, ParamType>(ty_idx as u8) };
                        let src_id = ParamId::new(src_idx as u8, ty);
                        let dst_id = ParamId::new((src_idx - 1) as u8, ty);
                        state
                            .shared
                            .params
                            .set(dst_id, state.shared.params.get(src_id));
                    }
                }
                for ty_idx in 0..ParamType::COUNT {
                    let ty = unsafe { std::mem::transmute::<u8, ParamType>(ty_idx as u8) };
                    let id = ParamId::new(count as u8, ty);
                    let def = param_type_def(ty);
                    state.shared.params.set(id, def.default);
                }
                drop(kit);
                let new_active = active_inst.min(count.saturating_sub(1));
                state.shared.set_param_outbound_only(
                    ParamId::new(0, ParamType::ActiveInstrument),
                    new_active as f64,
                );
                state.instrument_name_input = if new_active < count {
                    state.shared.kit.lock().instruments[new_active].name.clone()
                } else {
                    String::new()
                };
                state.shared.mark_kit_changed();
                state.shared.sync_output_port_count();
                state.shared.request_audio_ports_rescan();
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
                EnvelopeEditorMsg::Move(idx, t, v) => {
                    if let Some(p) = env.points_mut().get_mut(idx) {
                        p.t = t.clamp(0.0, 1.0);
                        p.v = v.clamp(0.0, 1.0);
                    }
                }
                EnvelopeEditorMsg::Add(t, v) => {
                    let mut points: Vec<_> = env.points().to_vec();
                    points.push(crate::kick::dsp::envelope::EnvPoint::new(t, v));
                    points.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
                    *env = crate::kick::dsp::envelope::Envelope::new(points);
                }
                EnvelopeEditorMsg::Remove(idx) => {
                    let mut points: Vec<_> = env.points().to_vec();
                    if points.len() > 2 && idx < points.len() {
                        points.remove(idx);
                        *env = crate::kick::dsp::envelope::Envelope::new(points);
                    }
                }
            }
            state.shared.mark_kit_changed();
        }
        Message::SavePreset => {
            if let Some(dir) = presets_dir() {
                let _ = std::fs::create_dir_all(&dir);
                let active_inst = state
                    .shared
                    .params
                    .get(ParamId::new(0, ParamType::ActiveInstrument))
                    as usize;
                let kit = state.shared.kit.lock();
                let name = if active_inst < kit.instruments.len() {
                    kit.instruments[active_inst].name.trim().to_string()
                } else {
                    String::new()
                };
                let kit_cfg = crate::kick::plugin::kit_to_config(&kit);
                drop(kit);
                if !name.is_empty() {
                    let path = dir.join(format!("{name}.json"));
                    let state_obj =
                        crate::kick::state::KitState::from_runtime(&state.shared.params, &kit_cfg);
                    if let Ok(bytes) = state_obj.to_bytes() {
                        let _ = std::fs::write(&path, bytes);
                    }
                }
            }
        }
        Message::EnvelopeKindChanged(kind) => {
            let source = envelope_source_idx(EnvelopeKind::from_u8(kind), state.envelope_osc);
            set_selected_envelope(state, state.envelope_layer, source as u8, kind);
        }
        Message::EnvelopeLayerChanged(layer) => {
            state.envelope_layer = layer.min(2);
            let source = envelope_source_idx(
                EnvelopeKind::from_u8(state.envelope_kind),
                state.envelope_osc,
            );
            state.envelope_selections[state.envelope_layer as usize][source] = state.envelope_kind;
        }
        Message::EnvelopeOscChanged(osc) => {
            let source = osc.min(1);
            set_selected_envelope(state, state.envelope_layer, source, state.envelope_kind);
        }
        Message::SelectOscEnvelope { kind, osc } => {
            let kind = EnvelopeKind::from_u8(kind);
            let source = envelope_source_idx(kind, osc);
            set_selected_envelope(
                state,
                state.active_layer_tab.min(2),
                source as u8,
                kind as u8,
            );
        }
        Message::EnvelopeRenderSourceChanged {
            layer,
            source,
            enabled,
        } => {
            if let Some(source_enabled) = state
                .envelope_render_sources
                .get_mut(layer)
                .and_then(|sources| sources.get_mut(source))
            {
                *source_enabled = enabled;
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
                state.shared.mark_kit_changed();
            }
        }
        Message::LayerTabChanged(tab) => {
            state.active_layer_tab = tab.min(2);
            sync_envelope_to_active_source(state);
        }
        Message::OscTabChanged(tab) => {
            state.active_osc_tab = tab.min(2);
            sync_envelope_to_active_source(state);
        }
    }
    Task::none()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InstOption {
    idx: u8,
    name: String,
}

impl std::fmt::Display for InstOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.idx == u8::MAX {
            return write!(f, "+");
        }
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

    let active_inst = {
        let kit = state.shared.kit.lock();
        let active = state
            .shared
            .params
            .get(ParamId::new(0, ParamType::ActiveInstrument)) as usize;
        active.min(kit.instruments.len().saturating_sub(1))
    };
    let ap = |ty: ParamType| ParamId::new(active_inst as u8, ty);
    let active_layer_params = gui_layer_params(state.active_layer_tab.min(2));

    let envelope_editor = if state.show_envelope_editor {
        let kit = state.shared.kit.lock();
        let env_kind = EnvelopeKind::from_u8(state.envelope_kind);
        let inst = kit.instruments[active_inst].clone();
        let mut env_inst = inst.clone();
        let env = selected_env(
            &mut env_inst,
            env_kind,
            state.envelope_layer as usize,
            state.envelope_osc as usize,
        )
        .clone();
        drop(kit);
        let length_ms = p(ap(ParamType::MasterLength));
        let layer_params = [
            gui_layer_params(0),
            gui_layer_params(1),
            gui_layer_params(2),
        ];
        let layer_enabled = [
            state.shared.params.get_bool(ap(ParamType::Layer0Enabled)),
            state.shared.params.get_bool(ap(ParamType::Layer1Enabled)),
            state.shared.params.get_bool(ap(ParamType::Layer2Enabled)),
        ];
        let layer_amp = [
            p(ap(ParamType::Layer0Amp)),
            p(ap(ParamType::Layer1Amp)),
            p(ap(ParamType::Layer2Amp)),
        ];
        let osc_freq_hz =
            layer_params.map(|params| [p(ap(params.osc0.freq)), p(ap(params.osc1.freq))]);
        let osc_amp = layer_params.map(|params| [p(ap(params.osc0.amp)), p(ap(params.osc1.amp))]);
        let noise_amp = layer_params.map(|params| p(ap(params.noise.amp)));
        let noise_density = layer_params.map(|params| p(ap(params.noise.density)));
        let osc_idx = state.envelope_osc.min(1) as usize;
        let base_freq_hz = osc_freq_hz[state.envelope_layer.min(2) as usize][osc_idx];
        let waveform = preview_waveform(
            length_ms,
            &inst,
            PreviewLayerParams {
                render: state.envelope_render_sources,
                enabled: layer_enabled,
                amp: layer_amp,
                osc_freq_hz,
                osc_amp,
                noise_amp,
                noise_density,
            },
        );
        Some(
            canvas(EnvelopeEditor::new(
                env,
                waveform,
                length_ms,
                env_kind.scale(base_freq_hz),
            ))
            .width(Length::Fill)
            .height(Length::Fill),
        )
    } else {
        None
    };

    let peak_db = peak_db_l.max(peak_db_r).clamp(FADER_MIN_DB, FADER_MAX_DB);
    let meter_readout = if peak_db <= FADER_MIN_DB {
        "-inf dB".to_string()
    } else {
        format!("{peak_db:.1} dB")
    };
    let meter = container(
        column![
            container(meters(2, &[peak_db_l, peak_db_r], 1.0))
                .height(Length::Fill)
                .width(Length::Shrink),
            text(meter_readout).size(10),
        ]
        .spacing(4)
        .align_x(Alignment::Center),
    )
    .height(Length::Fill)
    .width(Length::Fixed(48.0));

    let gain_id = ap(ParamType::MasterOutputGain);
    let gain_value = p(gain_id);
    let gain_def = param_type_def(gain_id.param_type());
    let gain_slider = vertical_slider(
        VerticalSlider {
            value: gain_value,
            range: gain_def.min as f32..=gain_def.max as f32,
            default: gain_def.default as f32,
            step: 0.1,
            value_text: format!("{gain_value:.1} dB"),
        },
        move |v| Message::SetParam(gain_id, v),
        Message::ReleaseParam(gain_id),
    );
    let gain_slider: Element<'_, Message> = container(gain_slider).height(Length::Fill).into();

    let top_display: Element<'_, Message> = if let Some(editor) = envelope_editor {
        let editor_el: Element<'_, EnvelopeEditorMsg> = editor.into();
        editor_el.map(Message::EnvelopeEdit)
    } else {
        column![].spacing(0).height(Length::Fill).into()
    };

    let meter_gain = row![meter, gain_slider]
        .spacing(0)
        .align_y(Alignment::Start)
        .height(Length::Fill);
    let top_row = row![top_display, meter_gain]
        .spacing(8)
        .align_y(Alignment::Start)
        .height(Length::Fill);

    let length_id = ap(ParamType::MasterLength);
    let length_value = p(length_id);
    let length_def = param_type_def(length_id.param_type());
    let length_slider = column![
        text("Length").size(11),
        slider(
            length_def.min as f32..=length_def.max as f32,
            length_value,
            move |v| { Message::SetParam(length_id, v) }
        )
        .step(1.0_f32)
        .width(Length::Fill),
    ]
    .spacing(2)
    .align_x(Alignment::Start);

    const ADD_INSTRUMENT_IDX: u8 = u8::MAX;
    let inst_options: Vec<InstOption> = {
        let kit = state.shared.kit.lock();
        let mut opts: Vec<InstOption> = kit
            .instruments
            .iter()
            .enumerate()
            .map(|(i, inst)| InstOption {
                idx: i as u8,
                name: inst.name.clone(),
            })
            .collect();
        opts.push(InstOption {
            idx: ADD_INSTRUMENT_IDX,
            name: "+".to_string(),
        });
        opts
    };
    let selected_inst = if active_inst < inst_options.len() - 1 {
        Some(inst_options[active_inst].clone())
    } else {
        None
    };
    let inst_dropdown = pick_list(inst_options, selected_inst, |opt| {
        if opt.idx == ADD_INSTRUMENT_IDX {
            Message::AddInstrument
        } else {
            Message::SetActiveInstrument(opt.idx)
        }
    })
    .placeholder("Select instrument...")
    .width(Length::Fixed(200.0));

    let inst_name_input =
        maolan_baseview::iced::widget::text_input("Instrument name", &state.instrument_name_input)
            .on_input(Message::InstrumentNameChanged)
            .width(Length::Fixed(160.0));

    let layer0_enabled = checkbox_param(
        "Enabled",
        ap(ParamType::Layer0Enabled),
        state.shared.params.get_bool(ap(ParamType::Layer0Enabled)),
    );
    let layer0_amp = amp_slider(ap(ParamType::Layer0Amp), p(ap(ParamType::Layer0Amp)));

    let layer1_enabled = checkbox_param(
        "Enabled",
        ap(ParamType::Layer1Enabled),
        state.shared.params.get_bool(ap(ParamType::Layer1Enabled)),
    );
    let layer1_amp = amp_slider(ap(ParamType::Layer1Amp), p(ap(ParamType::Layer1Amp)));

    let layer2_enabled = checkbox_param(
        "Enabled",
        ap(ParamType::Layer2Enabled),
        state.shared.params.get_bool(ap(ParamType::Layer2Enabled)),
    );
    let layer2_amp = amp_slider(ap(ParamType::Layer2Amp), p(ap(ParamType::Layer2Amp)));

    let active_layer_enabled = match state.active_layer_tab {
        0 => layer0_enabled,
        1 => layer1_enabled,
        2 => layer2_enabled,
        _ => layer0_enabled,
    };
    let active_layer_amp = match state.active_layer_tab {
        0 => layer0_amp,
        1 => layer1_amp,
        2 => layer2_amp,
        _ => layer0_amp,
    };

    let midi_ch_knob = knob(
        "MidiCh",
        ap(ParamType::MasterMidiChannel),
        p(ap(ParamType::MasterMidiChannel)),
        "",
        1.0,
    );
    let key_min_knob = knob(
        "KeyMin",
        ap(ParamType::MasterKeyMin),
        p(ap(ParamType::MasterKeyMin)),
        "",
        1.0,
    );
    let key_max_knob = knob(
        "KeyMax",
        ap(ParamType::MasterKeyMax),
        p(ap(ParamType::MasterKeyMax)),
        "",
        1.0,
    );
    let pitch_note_knob = knob(
        "PitchNote",
        ap(ParamType::MasterPitchToNote),
        p(ap(ParamType::MasterPitchToNote)),
        "",
        1.0,
    );
    let note_off_checkbox = checkbox_param(
        "NoteOff",
        ap(ParamType::MasterNoteOffEnabled),
        state
            .shared
            .params
            .get_bool(ap(ParamType::MasterNoteOffEnabled)),
    );
    let layer0_toggle = render_source_column("L1", 0, state.envelope_render_sources[0]);
    let layer1_toggle = render_source_column("L2", 1, state.envelope_render_sources[1]);
    let layer2_toggle = render_source_column("L3", 2, state.envelope_render_sources[2]);

    let remove_inst_button =
        maolan_baseview::iced::widget::button("-").on_press(Message::RemoveInstrument);

    let inst_selector = row![inst_dropdown, inst_name_input, remove_inst_button]
        .spacing(8)
        .align_y(Alignment::Center);

    let edit_buttons = row![
        maolan_baseview::iced::widget::button("Copy")
            .padding(3)
            .on_press(Message::CopyInstrument),
        maolan_baseview::iced::widget::button("Paste")
            .padding(3)
            .on_press(Message::PasteInstrument),
        maolan_baseview::iced::widget::button("Dup")
            .padding(3)
            .on_press(Message::DuplicateInstrument),
        maolan_baseview::iced::widget::button("Clear")
            .padding(3)
            .on_press(Message::ClearInstrument),
        maolan_baseview::iced::widget::button("Save")
            .padding(3)
            .on_press(Message::SavePreset),
        midi_ch_knob,
        key_min_knob,
        key_max_knob,
        pitch_note_knob,
        note_off_checkbox,
        layer0_toggle,
        layer1_toggle,
        layer2_toggle,
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let osc_section = |osc_idx: u8,
                       waveform_ty: ParamType,
                       freq_ty: ParamType,
                       amp_ty: ParamType,
                       phase_ty: ParamType,
                       fm_ty: Option<ParamType>,
                       filter_type_ty: ParamType,
                       cutoff_ty: ParamType,
                       q_ty: ParamType,
                       dist_type_ty: ParamType,
                       dist_drive_ty: ParamType| {
        let w = ap(waveform_ty);
        let f = ap(freq_ty);
        let a = ap(amp_ty);
        let ph = ap(phase_ty);
        let ft = ap(filter_type_ty);
        let c = ap(cutoff_ty);
        let qv = ap(q_ty);
        let dt = ap(dist_type_ty);
        let dd = ap(dist_drive_ty);
        let active_layer = state.active_layer_tab.min(2) as usize;
        let selected_env_kind = state.envelope_selections[active_layer][osc_idx as usize];
        let mut controls = row![
            waveform_dropdown(w, p(w)),
            clickable_knob(
                "Amp",
                a,
                p(a),
                "",
                0.01,
                Message::SelectOscEnvelope {
                    kind: EnvelopeKind::OscAmp as u8,
                    osc: osc_idx,
                },
                selected_env_kind == EnvelopeKind::OscAmp as u8,
            ),
            clickable_knob(
                "Freq",
                f,
                p(f),
                "Hz",
                1.0,
                Message::SelectOscEnvelope {
                    kind: EnvelopeKind::OscFreq as u8,
                    osc: osc_idx,
                },
                selected_env_kind == EnvelopeKind::OscFreq as u8,
            ),
            knob("Phase", ph, p(ph), "deg", 1.0),
            filter_type_dropdown(ft, p(ft)),
            knob("Cutoff", c, p(c), "Hz", 1.0),
            knob("Q", qv, p(qv), "", 0.01),
            distortion_type_dropdown(dt, p(dt)),
            knob("DistDrive", dd, p(dd), "", 0.01),
        ]
        .spacing(6);
        if let Some(fm_ty) = fm_ty {
            let fm = ap(fm_ty);
            controls = controls.push(knob("FM", fm, p(fm), "", 0.01));
        }
        controls
    };

    let osc0 = osc_section(
        0,
        active_layer_params.osc0.waveform,
        active_layer_params.osc0.freq,
        active_layer_params.osc0.amp,
        active_layer_params.osc0.phase,
        None,
        active_layer_params.osc0.filter_type,
        active_layer_params.osc0.filter_cutoff,
        active_layer_params.osc0.filter_q,
        active_layer_params.osc0.distortion_type,
        active_layer_params.osc0.distortion_drive,
    );
    let osc1 = osc_section(
        1,
        active_layer_params.osc1.waveform,
        active_layer_params.osc1.freq,
        active_layer_params.osc1.amp,
        active_layer_params.osc1.phase,
        Some(active_layer_params.osc1.fm_amount),
        active_layer_params.osc1.filter_type,
        active_layer_params.osc1.filter_cutoff,
        active_layer_params.osc1.filter_q,
        active_layer_params.osc1.distortion_type,
        active_layer_params.osc1.distortion_drive,
    );

    let active_layer = state.active_layer_tab.min(2) as usize;
    let selected_noise_env_kind = state.envelope_selections[active_layer][2];
    let noise_section = row![
        noise_type_dropdown(p(ap(active_layer_params.noise.noise_type))),
        clickable_knob(
            "Amp",
            ap(active_layer_params.noise.amp),
            p(ap(active_layer_params.noise.amp)),
            "",
            0.01,
            Message::SelectOscEnvelope {
                kind: EnvelopeKind::NoiseAmp as u8,
                osc: 0,
            },
            selected_noise_env_kind == EnvelopeKind::NoiseAmp as u8,
        ),
        clickable_knob(
            "Density",
            ap(active_layer_params.noise.density),
            p(ap(active_layer_params.noise.density)),
            "",
            0.01,
            Message::SelectOscEnvelope {
                kind: EnvelopeKind::NoiseDensity as u8,
                osc: 0,
            },
            selected_noise_env_kind == EnvelopeKind::NoiseDensity as u8,
        ),
        filter_type_dropdown(
            ap(active_layer_params.noise.filter_type),
            p(ap(active_layer_params.noise.filter_type))
        ),
        knob(
            "Cutoff",
            ap(active_layer_params.noise.filter_cutoff),
            p(ap(active_layer_params.noise.filter_cutoff)),
            "Hz",
            1.0
        ),
        knob(
            "Q",
            ap(active_layer_params.noise.filter_q),
            p(ap(active_layer_params.noise.filter_q)),
            "",
            0.01
        ),
    ]
    .spacing(6);

    let osc_tabs = row![
        tab_button("Osc1", state.active_osc_tab == 0, Message::OscTabChanged(0)),
        tab_button("Osc2", state.active_osc_tab == 1, Message::OscTabChanged(1)),
        tab_button(
            "Noise",
            state.active_osc_tab == 2,
            Message::OscTabChanged(2)
        ),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let active_osc: Element<'_, Message> = match state.active_osc_tab {
        0 => osc0.into(),
        1 => osc1.into(),
        2 => noise_section.into(),
        _ => osc0.into(),
    };

    let solo_checkbox = checkbox_param(
        "Solo",
        ap(ParamType::MasterSoloed),
        state.shared.params.get_bool(ap(ParamType::MasterSoloed)),
    );
    let layer_state_controls = row![active_layer_enabled, solo_checkbox, active_layer_amp,]
        .spacing(0)
        .align_y(Alignment::Center);

    let controls: Element<'_, Message> = column![
        row![
            inst_selector,
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
            layer_state_controls,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        osc_tabs,
        active_osc,
    ]
    .spacing(8)
    .align_x(Alignment::Start)
    .into();

    let content = column![edit_buttons, top_row, length_slider, controls]
        .spacing(8)
        .align_x(Alignment::Start);

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

fn waveform_dropdown(id: ParamId, value: f32) -> Element<'static, Message> {
    let waveform = Waveform::from_u8(value as u8);
    let options = vec![
        Waveform::Sine,
        Waveform::Square,
        Waveform::Triangle,
        Waveform::Saw,
        Waveform::Sample,
    ];
    let dropdown = pick_list(options, Some(waveform), move |t| {
        Message::SetWaveform(id, t as u8)
    })
    .placeholder("Wave")
    .width(Length::Fixed(84.0));

    container(
        column![text("Wave").size(11), dropdown]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(90.0))
    .into()
}

fn filter_type_dropdown(id: ParamId, value: f32) -> Element<'static, Message> {
    let filter_type = FilterType::from_u8(value as u8);
    let options = vec![
        FilterType::Off,
        FilterType::Lowpass,
        FilterType::Bandpass,
        FilterType::Highpass,
    ];
    let dropdown = pick_list(options, Some(filter_type), move |t| {
        Message::SetFilterType(id, t as u8)
    })
    .placeholder("Filter")
    .width(Length::Fixed(84.0));

    container(
        column![text("Filter").size(11), dropdown]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(90.0))
    .into()
}

fn noise_type_dropdown(value: f32) -> Element<'static, Message> {
    let noise_type = NoiseType::from_u8(value as u8);
    let options = vec![NoiseType::White, NoiseType::Pink, NoiseType::Brownian];
    let dropdown = pick_list(options, Some(noise_type), move |t| {
        Message::SetNoiseType(t as u8)
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

fn distortion_type_dropdown(id: ParamId, value: f32) -> Element<'static, Message> {
    let distortion_type = DistortionType::from_u8(value as u8);
    let options = vec![
        DistortionType::HardClip,
        DistortionType::SoftClipTanh,
        DistortionType::Arctangent,
        DistortionType::Exponential,
        DistortionType::Polynomial,
        DistortionType::Logarithmic,
        DistortionType::Foldback,
        DistortionType::HalfWaveRect,
        DistortionType::FullWaveRect,
    ];
    let dropdown = pick_list(options, Some(distortion_type), move |t| {
        Message::SetDistortionType(id, t as u8)
    })
    .placeholder("Dist")
    .width(Length::Fixed(84.0));

    container(
        column![text("Dist").size(11), dropdown]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(90.0))
    .into()
}

fn amp_slider(id: ParamId, value: f32) -> Element<'static, Message> {
    let def = param_type_def(id.param_type());
    let slider_widget = slider(def.min as f32..=def.max as f32, value, move |v| {
        Message::SetParam(id, v)
    })
    .step(0.01_f32)
    .width(Length::Fixed(120.0));

    container(
        column![text("Amp").size(11), slider_widget]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(126.0))
    .into()
}

fn knob(
    label: &'static str,
    id: ParamId,
    value: f32,
    units: &'static str,
    step: f32,
) -> Element<'static, Message> {
    let def = param_type_def(id.param_type());
    let value_text = knob_value_text(value, units);

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

fn clickable_knob(
    label: &'static str,
    id: ParamId,
    value: f32,
    units: &'static str,
    step: f32,
    label_message: Message,
    active: bool,
) -> Element<'static, Message> {
    let def = param_type_def(id.param_type());
    let min = def.min as f32;
    let max = def.max as f32;
    let slider_widget = if osc_location_for_freq_param(id.param_type()).is_some() {
        arch_slider(0.0..=1.0, freq_to_log_knob(value, min, max), move |v| {
            Message::SetParam(id, log_knob_to_freq(v, min, max))
        })
        .step(0.001)
        .double_click_reset(freq_to_log_knob(def.default as f32, min, max))
        .on_release(Message::ReleaseParam(id))
        .fill_from_start()
        .width(Length::Fixed(41.0))
        .height(Length::Fixed(41.0))
    } else {
        arch_slider(min..=max, value.clamp(min, max), move |v| {
            Message::SetParam(id, v)
        })
        .step(step)
        .double_click_reset(def.default as f32)
        .on_release(Message::ReleaseParam(id))
        .fill_from_start()
        .width(Length::Fixed(41.0))
        .height(Length::Fixed(41.0))
    };

    let label_button = maolan_baseview::iced::widget::button(text(label).size(11))
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
        .on_press(label_message);

    container(
        column![
            label_button,
            slider_widget,
            text(knob_value_text(value, units)).size(10)
        ]
        .spacing(2)
        .align_x(Alignment::Center),
    )
    .width(Length::Fixed(50.0))
    .into()
}

fn knob_value_text(value: f32, units: &'static str) -> String {
    if units.is_empty() {
        format!("{value:.2}")
    } else if units == "Hz" {
        format!("{value:.0} {units}")
    } else {
        format!("{value:.1} {units}")
    }
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
    .width(Length::Fixed(50.0))
    .into()
}

fn render_source_column(
    label: &'static str,
    layer: usize,
    values: [bool; 3],
) -> Element<'static, Message> {
    container(
        column![
            text(label).size(9),
            checkbox(values[0]).label("").on_toggle(move |enabled| {
                Message::EnvelopeRenderSourceChanged {
                    layer,
                    source: 0,
                    enabled,
                }
            }),
            checkbox(values[1]).label("").on_toggle(move |enabled| {
                Message::EnvelopeRenderSourceChanged {
                    layer,
                    source: 1,
                    enabled,
                }
            }),
            checkbox(values[2]).label("").on_toggle(move |enabled| {
                Message::EnvelopeRenderSourceChanged {
                    layer,
                    source: 2,
                    enabled,
                }
            }),
        ]
        .spacing(1)
        .align_x(Alignment::Center),
    )
    .width(Length::Fixed(34.0))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequency_knob_log_scale_roundtrips_and_favors_low_range() {
        let min = 1000.0;
        let max = 20000.0;
        let midpoint_freq = log_knob_to_freq(0.5, min, max);

        assert!(midpoint_freq < (min + max) * 0.5);
        assert!((freq_to_log_knob(midpoint_freq, min, max) - 0.5).abs() < 1.0e-6);
    }
}

struct PreviewLayerParams {
    render: [[bool; 3]; 3],
    enabled: [bool; 3],
    amp: [f32; 3],
    osc_freq_hz: [[f32; 2]; 3],
    osc_amp: [[f32; 2]; 3],
    noise_amp: [f32; 3],
    noise_density: [f32; 3],
}

fn preview_waveform(
    length_ms: f32,
    instrument: &crate::kick::dsp::Instrument,
    layer_params: PreviewLayerParams,
) -> Option<Vec<f32>> {
    if length_ms <= 0.0
        || !layer_params
            .render
            .iter()
            .any(|sources| sources.iter().any(|enabled| *enabled))
    {
        return None;
    }

    const PREVIEW_SAMPLES: usize = 2048;
    let sample_rate = instrument.layers[0].oscillators[0].sample_rate().max(1.0);
    let render_samples = (length_ms * 0.001 * sample_rate)
        .round()
        .clamp(2.0, 192_000.0) as usize;

    let mut mono = vec![0.0f32; render_samples];
    for (layer_idx, layer) in instrument.layers.iter().enumerate() {
        if !layer_params.enabled[layer_idx] {
            continue;
        }

        let mut layer_mix = vec![0.0f32; render_samples];
        for osc_idx in 0..2 {
            if layer_params.render[layer_idx][osc_idx]
                && let Some(osc) = layer.oscillators.get(osc_idx)
            {
                let mut osc = osc.clone();
                osc.set_base_freq_hz(layer_params.osc_freq_hz[layer_idx][osc_idx]);
                osc.set_amplitude(layer_params.osc_amp[layer_idx][osc_idx]);
                osc.set_midi_note(60);
                let mut source_buf = vec![0.0f32; render_samples];
                osc.render(&mut source_buf, render_samples, None);
                for (mix_sample, source_sample) in layer_mix.iter_mut().zip(source_buf) {
                    *mix_sample += source_sample;
                }
            }
        }
        if layer_params.render[layer_idx][2] {
            let mut noise = layer.noise.clone();
            noise.amplitude = layer_params.noise_amp[layer_idx];
            noise.density = layer_params.noise_density[layer_idx];
            let mut source_buf = vec![0.0f32; render_samples];
            noise.render(&mut source_buf, render_samples);
            for (mix_sample, source_sample) in layer_mix.iter_mut().zip(source_buf) {
                *mix_sample += source_sample;
            }
        }

        for (sample, layer_sample) in mono.iter_mut().zip(layer_mix) {
            *sample += layer_sample * layer_params.amp[layer_idx];
        }
    }

    let mut preview = vec![0.0f32; PREVIEW_SAMPLES];
    for (i, sample) in preview.iter_mut().enumerate() {
        let pos = i as f32 * (render_samples - 1) as f32 / (PREVIEW_SAMPLES - 1) as f32;
        let idx = pos.floor() as usize;
        let frac = pos - idx as f32;
        let a = mono[idx];
        let b = mono.get(idx + 1).copied().unwrap_or(a);
        *sample = a + (b - a) * frac;
    }

    Some(preview)
}

fn build_app(shared: Arc<SharedState>) -> impl maolan_baseview::iced::Program {
    maolan_baseview::iced::application(move || init(shared.clone()), update, view)
        .font(maolan_widgets::iced_fonts::LUCIDE_FONT_BYTES)
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
