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
    widget::{canvas, column, container, row, text},
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
    ReleaseParam(ParamId),
    SetFilterType(ParamId, u8),
    SetWaveform(ParamId, u8),
    SetDistortionType(ParamId, u8),
    SetNoiseType(u8),
    SetActiveInstrument(u8),
    CopyInstrument,
    PasteInstrument,
    EnvelopeEdit(EnvelopeEditorMsg),
    PresetNameChanged(String),
    SavePreset,
    LoadPreset(String),
    RefreshPresets,
    SamplePathChanged(String),
    LoadSample,
}

struct State {
    shared: Arc<SharedState>,
    active_gestures: Vec<bool>,
    show_envelope_editor: bool,
    preset_name_input: String,
    sample_path_input: String,
    preset_files: Vec<String>,
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
    (
        State {
            shared,
            active_gestures: vec![false; ParamId::COUNT],
            show_envelope_editor: true,
            preset_name_input: String::new(),
            sample_path_input: String::new(),
            preset_files: scan_presets(),
        },
        Task::none(),
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
            state.shared.set_param_outbound_only(
                ParamId::new(0, ParamType::ActiveInstrument),
                inst as f64,
            );
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
        Message::EnvelopeEdit(msg) => {
            let active_inst = state
                .shared
                .params
                .get(ParamId::new(0, ParamType::ActiveInstrument))
                as usize;
            let mut kit = state.shared.kit.lock();
            let env = &mut kit.instruments[active_inst].global_amp_env;
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
                let osc = &mut kit.instruments[active_inst].layers[0].oscillators[0];
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
    }
    Task::none()
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

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
        let env = kit.instruments[active_inst].global_amp_env.clone();
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

    // Instrument selector
    let mut inst_buttons = vec![];
    for i in 0..16 {
        let label = format!("{}", i + 1);
        let is_active = i == active_inst;
        inst_buttons.push(
            maolan_baseview::iced::widget::button(
                container(text(label).size(11))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(24.0))
            .padding(0)
            .style(move |theme: &Theme, status| {
                let mut base = if is_active {
                    maolan_baseview::iced::widget::button::primary(theme, status)
                } else {
                    maolan_baseview::iced::widget::button::secondary(theme, status)
                };
                base.border.radius = 4.0.into();
                base
            })
            .on_press(Message::SetActiveInstrument(i as u8))
            .into(),
        );
    }
    let inst_selector = row(inst_buttons)
        .spacing(2)
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
            knob(
                "NO Enab",
                ap(ParamType::MasterNoteOffEnabled),
                p(ap(ParamType::MasterNoteOffEnabled)),
                "",
                1.0
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
            knob(
                "Enabled",
                ap(ParamType::Layer0Enabled),
                p(ap(ParamType::Layer0Enabled)),
                "",
                1.0
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
    let env_section = column![
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
    ]
    .spacing(6);

    // Preset browser section
    let preset_items: Vec<Element<'_, Message>> = state
        .preset_files
        .iter()
        .map(|name| {
            maolan_baseview::iced::widget::button(name.as_str())
                .on_press(Message::LoadPreset(name.clone()))
                .into()
        })
        .collect();
    let preset_list = maolan_baseview::iced::widget::scrollable(column(preset_items).spacing(2))
        .height(Length::Fixed(80.0));

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
        preset_list,
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
    ]
    .spacing(6);

    let controls = row![
        column![kit_section, master_section, layer0_section, preset_section]
            .spacing(10)
            .align_x(Alignment::Start),
        column![osc0, osc1, osc2, sample_section]
            .spacing(10)
            .align_x(Alignment::Start),
        column![noise_section, env_section]
            .spacing(10)
            .align_x(Alignment::Start),
    ]
    .spacing(10)
    .align_y(Alignment::Start);

    let mut content = column![top_row, inst_selector, controls]
        .spacing(8)
        .align_x(Alignment::Start);
    if let Some(editor) = envelope_editor {
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

fn build_app(shared: Arc<SharedState>) -> impl maolan_baseview::iced::Program {
    maolan_baseview::iced::application(move || init(shared.clone()), update, view)
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
