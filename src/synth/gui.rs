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
use maolan_baseview::iced::{
    Alignment, Background, Border, Color, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{column, container, row, text},
};
use maolan_widgets::arch_slider::arch_slider;
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};

use crate::synth::{
    params::{PARAMS, ParamId},
    plugin::SharedState,
};

pub const EDITOR_WIDTH: u32 = 1500;
pub const EDITOR_HEIGHT: u32 = 1000;

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
    X11(u32),
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
                handle.window = *window as u64;
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

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum Message {
    SetParam(ParamId, f32),
    ReleaseParam(ParamId),
}

struct State {
    shared: Arc<SharedState>,
    active_gestures: Vec<bool>,
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    (
        State {
            shared,
            active_gestures: vec![false; ParamId::COUNT],
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
    }
    Task::none()
}

// ---------------------------------------------------------------------------
// Layout helpers
// ---------------------------------------------------------------------------

fn small_knob<'a>(
    id: ParamId,
    label: &'a str,
    state: &'a State,
    step: f32,
) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let def = &PARAMS[id.as_index()];
    let slider = arch_slider(def.min as f32..=def.max as f32, value, move |v| {
        Message::SetParam(id, v)
    })
    .step(step)
    .double_click_reset(def.default as f32)
    .on_release(Message::ReleaseParam(id))
    .fill_from_start()
    .width(Length::Fixed(48.0))
    .height(Length::Fixed(48.0));

    let value_text = if def.step >= 1.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    };

    container(
        column![text(label).size(11), slider, text(value_text).size(10)]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(56.0))
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

fn knob_row<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut r = row![].spacing(6).align_y(Alignment::Center);
    for item in items {
        r = r.push(item);
    }
    r.into()
}

// ---------------------------------------------------------------------------
// Main view – organised like Surge XT
// ---------------------------------------------------------------------------

fn view(state: &State) -> Element<'_, Message> {
    // -----------------------------------------------------------------------
    // Top bar: Output / Play Mode / Drift
    // -----------------------------------------------------------------------
    let top_bar = row![
        panel(
            "Output",
            knob_row(vec![
                small_knob(ParamId::Volume, "Vol", state, 0.01),
                small_knob(ParamId::Pan, "Pan", state, 0.01),
                small_knob(ParamId::Width, "Width", state, 0.01),
            ])
        ),
        panel(
            "Play Mode",
            knob_row(vec![
                small_knob(ParamId::Polyphony, "Poly", state, 1.0),
                small_knob(ParamId::PlayMode, "Mode", state, 1.0),
                small_knob(ParamId::VoicePriority, "Priority", state, 1.0),
                small_knob(ParamId::Portamento, "Port", state, 0.01),
                small_knob(ParamId::PitchBendRange, "Bend", state, 1.0),
            ])
        ),
        panel(
            "Scene",
            knob_row(vec![
                small_knob(ParamId::OscDrift, "Drift", state, 0.01),
                small_knob(ParamId::NoiseColor, "Noise Col", state, 1.0),
            ])
        ),
    ]
    .spacing(10)
    .align_y(Alignment::Start);

    // -----------------------------------------------------------------------
    // Oscillators (left column)
    // -----------------------------------------------------------------------
    let osc1 = panel(
        "Osc 1",
        column![
            knob_row(vec![
                small_knob(ParamId::Osc1Type, "Type", state, 1.0),
                small_knob(ParamId::Osc1Waveform, "Wave", state, 1.0),
                small_knob(ParamId::Osc1Octave, "Oct", state, 1.0),
                small_knob(ParamId::Osc1Semitone, "Semi", state, 1.0),
                small_knob(ParamId::Osc1Fine, "Fine", state, 0.01),
                small_knob(ParamId::Osc1Shape, "Shape", state, 0.01),
                small_knob(ParamId::Osc1Skew, "Skew", state, 0.01),
            ]),
            knob_row(vec![
                small_knob(ParamId::Osc1Sync, "Sync", state, 1.0),
                small_knob(ParamId::Osc1Unison, "Unison", state, 1.0),
                small_knob(ParamId::Osc1UnisonDetune, "UniDet", state, 0.01),
                small_knob(ParamId::Osc1UnisonSpread, "UniSpr", state, 0.01),
                small_knob(ParamId::Osc1Level, "Level", state, 0.01),
                small_knob(ParamId::Osc1Enabled, "On", state, 1.0),
            ]),
        ]
        .spacing(6)
        .into(),
    );

    let osc2 = panel(
        "Osc 2",
        column![
            knob_row(vec![
                small_knob(ParamId::Osc2Type, "Type", state, 1.0),
                small_knob(ParamId::Osc2Waveform, "Wave", state, 1.0),
                small_knob(ParamId::Osc2Octave, "Oct", state, 1.0),
                small_knob(ParamId::Osc2Semitone, "Semi", state, 1.0),
                small_knob(ParamId::Osc2Fine, "Fine", state, 0.01),
                small_knob(ParamId::Osc2Shape, "Shape", state, 0.01),
                small_knob(ParamId::Osc2Skew, "Skew", state, 0.01),
            ]),
            knob_row(vec![
                small_knob(ParamId::Osc2Sync, "Sync", state, 1.0),
                small_knob(ParamId::Osc2Unison, "Unison", state, 1.0),
                small_knob(ParamId::Osc2UnisonDetune, "UniDet", state, 0.01),
                small_knob(ParamId::Osc2UnisonSpread, "UniSpr", state, 0.01),
                small_knob(ParamId::Osc2Level, "Level", state, 0.01),
                small_knob(ParamId::Osc2Enabled, "On", state, 1.0),
            ]),
        ]
        .spacing(6)
        .into(),
    );

    let osc3 = panel(
        "Osc 3",
        column![
            knob_row(vec![
                small_knob(ParamId::Osc3Type, "Type", state, 1.0),
                small_knob(ParamId::Osc3Waveform, "Wave", state, 1.0),
                small_knob(ParamId::Osc3Octave, "Oct", state, 1.0),
                small_knob(ParamId::Osc3Semitone, "Semi", state, 1.0),
                small_knob(ParamId::Osc3Fine, "Fine", state, 0.01),
                small_knob(ParamId::Osc3Shape, "Shape", state, 0.01),
                small_knob(ParamId::Osc3Formant, "Formant", state, 0.01),
            ]),
            knob_row(vec![
                small_knob(ParamId::Osc3Sync, "Sync", state, 1.0),
                small_knob(ParamId::Osc3Unison, "Unison", state, 1.0),
                small_knob(ParamId::Osc3UnisonDetune, "UniDet", state, 0.01),
                small_knob(ParamId::Osc3UnisonSpread, "UniSpr", state, 0.01),
                small_knob(ParamId::Osc3Level, "Level", state, 0.01),
                small_knob(ParamId::Osc3Enabled, "On", state, 1.0),
            ]),
        ]
        .spacing(6)
        .into(),
    );

    let left_column = column![osc1, osc2, osc3].spacing(10);

    // -----------------------------------------------------------------------
    // Middle column: FM / Filters / Waveshaper / Envelopes
    // -----------------------------------------------------------------------
    let fm_panel = panel(
        "FM Routing",
        knob_row(vec![
            small_knob(ParamId::OscFmMode, "Mode", state, 1.0),
            small_knob(ParamId::OscFmDepth, "Depth", state, 0.01),
        ]),
    );

    let filter1 = panel(
        "Filter 1",
        knob_row(vec![
            small_knob(ParamId::F1Type, "Type", state, 1.0),
            small_knob(ParamId::F1Subtype, "Sub", state, 1.0),
            small_knob(ParamId::F1Cutoff, "Cut", state, 1.0),
            small_knob(ParamId::F1Resonance, "Res", state, 0.01),
            small_knob(ParamId::F1EgAmount, "EG", state, 0.01),
            small_knob(ParamId::F1KeyTrack, "Key", state, 0.01),
            small_knob(ParamId::F1Drive, "Drive", state, 0.01),
            small_knob(ParamId::F1Enabled, "On", state, 1.0),
        ]),
    );

    let filter2 = panel(
        "Filter 2",
        knob_row(vec![
            small_knob(ParamId::F2Type, "Type", state, 1.0),
            small_knob(ParamId::F2Subtype, "Sub", state, 1.0),
            small_knob(ParamId::F2Cutoff, "Cut", state, 1.0),
            small_knob(ParamId::F2Resonance, "Res", state, 0.01),
            small_knob(ParamId::F2EgAmount, "EG", state, 0.01),
            small_knob(ParamId::F2KeyTrack, "Key", state, 0.01),
            small_knob(ParamId::F2Drive, "Drive", state, 0.01),
            small_knob(ParamId::F2Enabled, "On", state, 1.0),
        ]),
    );

    let filter_routing = panel(
        "Filter Routing",
        knob_row(vec![
            small_knob(ParamId::FilterRouting, "Route", state, 1.0),
            small_knob(ParamId::FilterBalance, "Balance", state, 0.01),
        ]),
    );

    let waveshaper = panel(
        "Waveshaper",
        knob_row(vec![
            small_knob(ParamId::WaveshaperShape, "Shape", state, 1.0),
            small_knob(ParamId::WaveshaperDrive, "Drive", state, 0.01),
            small_knob(ParamId::WaveshaperMix, "Mix", state, 0.01),
            small_knob(ParamId::WaveshaperEnabled, "On", state, 1.0),
        ]),
    );

    let amp_eg = panel(
        "Amp EG",
        knob_row(vec![
            small_knob(ParamId::AmpAttack, "A", state, 0.01),
            small_knob(ParamId::AmpDecay, "D", state, 0.01),
            small_knob(ParamId::AmpSustain, "S", state, 0.01),
            small_knob(ParamId::AmpRelease, "R", state, 0.01),
            small_knob(ParamId::AmpEgMode, "Mode", state, 1.0),
        ]),
    );

    let filter_eg = panel(
        "Filter EG",
        knob_row(vec![
            small_knob(ParamId::FilterAttack, "A", state, 0.01),
            small_knob(ParamId::FilterDecay, "D", state, 0.01),
            small_knob(ParamId::FilterSustain, "S", state, 0.01),
            small_knob(ParamId::FilterRelease, "R", state, 0.01),
            small_knob(ParamId::FilterEgMode, "Mode", state, 1.0),
        ]),
    );

    let pitch_eg = panel(
        "Pitch EG",
        knob_row(vec![
            small_knob(ParamId::PitchAttack, "A", state, 0.01),
            small_knob(ParamId::PitchDecay, "D", state, 0.01),
            small_knob(ParamId::PitchSustain, "S", state, 0.01),
            small_knob(ParamId::PitchRelease, "R", state, 0.01),
            small_knob(ParamId::PitchEgMode, "Mode", state, 1.0),
        ]),
    );

    let middle_column = column![
        row![fm_panel, filter_routing, waveshaper].spacing(10),
        filter1,
        filter2,
        row![amp_eg, filter_eg].spacing(10),
        pitch_eg,
    ]
    .spacing(10);

    // -----------------------------------------------------------------------
    // Right column: Noise / Character / Macros
    // -----------------------------------------------------------------------
    let noise = panel(
        "Noise",
        column![
            knob_row(vec![
                small_knob(ParamId::NoiseType, "Type", state, 1.0),
                small_knob(ParamId::NoiseLevel, "Level", state, 0.01),
                small_knob(ParamId::NoiseFilterType, "FType", state, 1.0),
                small_knob(ParamId::NoiseFilterCutoff, "FCut", state, 1.0),
            ]),
            knob_row(vec![
                small_knob(ParamId::NoiseFilterResonance, "FRes", state, 0.01),
                small_knob(ParamId::NoiseFilterEnabled, "FOn", state, 1.0),
                small_knob(ParamId::NoiseEnabled, "On", state, 1.0),
            ]),
        ]
        .spacing(6)
        .into(),
    );

    let character = panel(
        "Character",
        knob_row(vec![
            small_knob(ParamId::CharacterType, "Type", state, 1.0),
            small_knob(ParamId::CharacterCutoff, "Cut", state, 1.0),
            small_knob(ParamId::CharacterResonance, "Res", state, 0.01),
        ]),
    );

    let macros = panel(
        "Macros",
        knob_row(vec![
            small_knob(ParamId::Macro1, "M1", state, 0.01),
            small_knob(ParamId::Macro2, "M2", state, 0.01),
            small_knob(ParamId::Macro3, "M3", state, 0.01),
            small_knob(ParamId::Macro4, "M4", state, 0.01),
            small_knob(ParamId::Macro5, "M5", state, 0.01),
            small_knob(ParamId::Macro6, "M6", state, 0.01),
        ]),
    );

    let right_column = column![noise, character, macros].spacing(10);

    // -----------------------------------------------------------------------
    // Main content row
    // -----------------------------------------------------------------------
    let main_content = row![left_column, middle_column, right_column]
        .spacing(12)
        .align_y(Alignment::Start);

    // -----------------------------------------------------------------------
    // Bottom: LFOs
    // -----------------------------------------------------------------------
    let lfo1 = panel(
        "LFO 1",
        knob_row(vec![
            small_knob(ParamId::Lfo1Rate, "Rate", state, 0.01),
            small_knob(ParamId::Lfo1Shape, "Shape", state, 1.0),
            small_knob(ParamId::Lfo1Amount, "Amt", state, 0.01),
            small_knob(ParamId::Lfo1Deform, "Deform", state, 0.01),
            small_knob(ParamId::Lfo1Phase, "Phase", state, 0.01),
            small_knob(ParamId::Lfo1Unipolar, "Uni", state, 1.0),
            small_knob(ParamId::Lfo1SyncMode, "Sync", state, 1.0),
            small_knob(ParamId::Lfo1Trigger, "Trig", state, 1.0),
        ]),
    );

    let lfo2 = panel(
        "LFO 2",
        knob_row(vec![
            small_knob(ParamId::Lfo2Rate, "Rate", state, 0.01),
            small_knob(ParamId::Lfo2Shape, "Shape", state, 1.0),
            small_knob(ParamId::Lfo2Amount, "Amt", state, 0.01),
            small_knob(ParamId::Lfo2Deform, "Deform", state, 0.01),
            small_knob(ParamId::Lfo2Phase, "Phase", state, 0.01),
            small_knob(ParamId::Lfo2Unipolar, "Uni", state, 1.0),
            small_knob(ParamId::Lfo2SyncMode, "Sync", state, 1.0),
            small_knob(ParamId::Lfo2Trigger, "Trig", state, 1.0),
        ]),
    );

    let lfo3 = panel(
        "LFO 3",
        knob_row(vec![
            small_knob(ParamId::Lfo3Rate, "Rate", state, 0.01),
            small_knob(ParamId::Lfo3Shape, "Shape", state, 1.0),
            small_knob(ParamId::Lfo3Amount, "Amt", state, 0.01),
            small_knob(ParamId::Lfo3Deform, "Deform", state, 0.01),
            small_knob(ParamId::Lfo3Phase, "Phase", state, 0.01),
            small_knob(ParamId::Lfo3Unipolar, "Uni", state, 1.0),
            small_knob(ParamId::Lfo3SyncMode, "Sync", state, 1.0),
            small_knob(ParamId::Lfo3Trigger, "Trig", state, 1.0),
        ]),
    );

    let lfo4 = panel(
        "LFO 4",
        knob_row(vec![
            small_knob(ParamId::Lfo4Rate, "Rate", state, 0.01),
            small_knob(ParamId::Lfo4Shape, "Shape", state, 1.0),
            small_knob(ParamId::Lfo4Amount, "Amt", state, 0.01),
            small_knob(ParamId::Lfo4Deform, "Deform", state, 0.01),
            small_knob(ParamId::Lfo4Phase, "Phase", state, 0.01),
            small_knob(ParamId::Lfo4Unipolar, "Uni", state, 1.0),
            small_knob(ParamId::Lfo4SyncMode, "Sync", state, 1.0),
            small_knob(ParamId::Lfo4Trigger, "Trig", state, 1.0),
        ]),
    );

    let lfo5 = panel(
        "LFO 5",
        knob_row(vec![
            small_knob(ParamId::Lfo5Rate, "Rate", state, 0.01),
            small_knob(ParamId::Lfo5Shape, "Shape", state, 1.0),
            small_knob(ParamId::Lfo5Amount, "Amt", state, 0.01),
            small_knob(ParamId::Lfo5Deform, "Deform", state, 0.01),
            small_knob(ParamId::Lfo5Phase, "Phase", state, 0.01),
            small_knob(ParamId::Lfo5Unipolar, "Uni", state, 1.0),
            small_knob(ParamId::Lfo5SyncMode, "Sync", state, 1.0),
            small_knob(ParamId::Lfo5Trigger, "Trig", state, 1.0),
        ]),
    );

    let lfo6 = panel(
        "LFO 6",
        knob_row(vec![
            small_knob(ParamId::Lfo6Rate, "Rate", state, 0.01),
            small_knob(ParamId::Lfo6Shape, "Shape", state, 1.0),
            small_knob(ParamId::Lfo6Amount, "Amt", state, 0.01),
            small_knob(ParamId::Lfo6Deform, "Deform", state, 0.01),
            small_knob(ParamId::Lfo6Phase, "Phase", state, 0.01),
            small_knob(ParamId::Lfo6Unipolar, "Uni", state, 1.0),
            small_knob(ParamId::Lfo6SyncMode, "Sync", state, 1.0),
            small_knob(ParamId::Lfo6Trigger, "Trig", state, 1.0),
        ]),
    );

    let lfo_row = row![lfo1, lfo2, lfo3, lfo4].spacing(10);
    let lfo_row2 = row![lfo5, lfo6].spacing(10);

    // -----------------------------------------------------------------------
    // Root
    // -----------------------------------------------------------------------
    let content = column![top_bar, main_content, lfo_row, lfo_row2]
        .spacing(12)
        .padding(16)
        .align_x(Alignment::Start);

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
                title: String::from("Maolan Synth"),
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
                    title: String::from("Maolan Synth"),
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
