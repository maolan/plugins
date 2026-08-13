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
use maolan_widgets::meters::meters;

use crate::common::ui::{SmallKnob, VerticalSlider, small_knob, vertical_slider};
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};

mod envelope_editor;

use crate::common::distortion::DistortionType;
use crate::common::filter::FilterType;
use crate::kick::dsp::{
    INSTRUMENTS_PER_KIT,
    oscillator::{Oscillator, Waveform, set_waveform},
};
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
}

struct State {
    shared: Arc<SharedState>,
    active_gestures: Vec<bool>,
    show_envelope_editor: bool,
    envelope_kind: u8,
    envelope_layer: u8,
    envelope_osc: u8,
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
            envelope_kind: 0,
            envelope_layer: 0,
            envelope_osc: 0,
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
        EnvelopeKind::OscAmp => inst.layers[layer].oscillators[osc].amp_env_mut(),
        EnvelopeKind::OscPitch => inst.layers[layer].oscillators[osc].pitch_env_mut(),
        EnvelopeKind::OscFreq => inst.layers[layer].oscillators[osc].freq_env_mut(),
        EnvelopeKind::OscFilterCutoff => {
            inst.layers[layer].oscillators[osc].filter_cutoff_env_mut()
        }
        EnvelopeKind::OscFilterQ => inst.layers[layer].oscillators[osc].filter_q_env_mut(),
        EnvelopeKind::OscDistDrive => {
            inst.layers[layer].oscillators[osc].distortion_drive_env_mut()
        }
        EnvelopeKind::OscPitchShift => inst.layers[layer].oscillators[osc].pitch_shift_env_mut(),
        EnvelopeKind::NoiseAmp => &mut inst.layers[layer].noise.amp_env,
        EnvelopeKind::NoiseDensity => &mut inst.layers[layer].noise.density_env,
        EnvelopeKind::MasterDistVol => &mut inst.master_distortion.volume_env,
        EnvelopeKind::LayerDistVol => &mut inst.layers[layer].distortion.volume_env,
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
            state.envelope_kind = kind.min(11);
        }
        Message::EnvelopeLayerChanged(layer) => {
            state.envelope_layer = layer.min(2);
        }
        Message::EnvelopeOscChanged(osc) => {
            state.envelope_osc = osc.min(2);
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
        }
        Message::OscTabChanged(tab) => {
            state.active_osc_tab = tab.min(2);
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
        let waveform = preview_waveform(
            p(ap(ParamType::MasterLength)),
            p(ap(ParamType::Osc0Freq)),
            p(ap(ParamType::Osc0Waveform)),
        );
        Some(
            canvas(EnvelopeEditor::new(env, waveform))
                .width(Length::Fill)
                .height(Length::Fill),
        )
    } else {
        None
    };

    let meter = container(meters(2, &[peak_db_l, peak_db_r], 1.0))
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
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let osc_section = |waveform_ty: ParamType,
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
        row![
            waveform_dropdown(w, p(w)),
            knob("Freq", f, p(f), "Hz", 1.0),
            knob("Amp", a, p(a), "", 0.01),
            knob("Phase", ph, p(ph), "", 0.01),
            knob("FM", fm, p(fm), "", 0.01),
            filter_type_dropdown(ft, p(ft)),
            knob("Cutoff", c, p(c), "Hz", 1.0),
            knob("Q", qv, p(qv), "", 0.01),
            distortion_type_dropdown(dt, p(dt)),
            knob("DistDrive", dd, p(dd), "", 0.01),
        ]
        .spacing(6)
    };

    let osc0 = osc_section(
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
            filter_type_dropdown(
                ap(ParamType::NoiseFilterType),
                p(ap(ParamType::NoiseFilterType))
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

    let mute_checkbox = checkbox_param(
        "Mute",
        ap(ParamType::MasterMuted),
        state.shared.params.get_bool(ap(ParamType::MasterMuted)),
    );
    let solo_checkbox = checkbox_param(
        "Solo",
        ap(ParamType::MasterSoloed),
        state.shared.params.get_bool(ap(ParamType::MasterSoloed)),
    );

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
            active_layer_enabled,
            mute_checkbox,
            solo_checkbox,
            active_layer_amp,
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

fn section_header(label: &'static str) -> Element<'static, Message> {
    text(label).size(11).into()
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
        FilterType::Notch,
        FilterType::Peak,
        FilterType::CombPos,
        FilterType::CombNeg,
        FilterType::Allpass,
        FilterType::Ladder,
        FilterType::K35Lp,
        FilterType::K35Hp,
        FilterType::DiodeLadder,
        FilterType::CutoffWarp,
        FilterType::ResonanceWarp,
        FilterType::Lowpass12dB,
        FilterType::Highpass12dB,
        FilterType::Bandpass12dB,
        FilterType::LowShelf,
        FilterType::HighShelf,
        FilterType::Bell,
        FilterType::Notch12dB,
        FilterType::VintageLadder,
        FilterType::CytomicLp,
        FilterType::CytomicHp,
        FilterType::CytomicBp,
        FilterType::CytomicNotch,
        FilterType::CytomicPeak,
        FilterType::CytomicAp,
        FilterType::CytomicBell,
        FilterType::CytomicLs,
        FilterType::CytomicHs,
        FilterType::TriPole,
        FilterType::SampleHold,
        FilterType::CutoffWarpHp,
        FilterType::CutoffWarpBp,
        FilterType::CutoffWarpNotch,
        FilterType::CutoffWarpAp,
        FilterType::ResonanceWarpLp,
        FilterType::ResonanceWarpHp,
        FilterType::ResonanceWarpNotch,
        FilterType::ResonanceWarpAp,
        FilterType::Obxd2PoleLp,
        FilterType::Obxd2PoleHp,
        FilterType::Obxd2PoleBp,
        FilterType::Obxd2PoleNotch,
        FilterType::Obxd4Pole,
        FilterType::ObxdXpander,
        FilterType::Notch24dB,
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
    let value_text = if units.is_empty() {
        format!("{value:.2}")
    } else if units == "Hz" {
        format!("{value:.0} {units}")
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

fn preview_waveform(length_ms: f32, freq_hz: f32, wave_val: f32) -> Option<Vec<f32>> {
    if length_ms <= 0.0 || freq_hz <= 0.0 {
        return None;
    }

    const SAMPLES: usize = 2048;
    let length_sec = length_ms / 1000.0;
    let sample_rate = SAMPLES as f32 / length_sec;

    let mut osc = Oscillator::new(sample_rate);
    osc.set_base_freq_hz(freq_hz.max(1.0));
    osc.set_amplitude(1.0);
    set_waveform(&mut osc, Waveform::from_u8(wave_val as u8));
    osc.set_pitch_env(None);
    osc.set_amp_env(None);
    osc.set_filter_type(FilterType::Off);
    osc.set_distortion(None);

    let mut buf = vec![0.0f32; SAMPLES];
    osc.render(&mut buf, SAMPLES, None);
    Some(buf)
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
