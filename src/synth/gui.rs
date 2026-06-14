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
    widget::{button, checkbox, column, container, row, text},
};
use maolan_widgets::arch_slider::arch_slider;
use maolan_widgets::horizontal_slider::HorizontalSlider;
use maolan_widgets::slider::Slider;
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};

use crate::{
    common::filter::FilterType,
    synth::{
        dsp::OscType,
        params::{ParamId, param_def},
        plugin::SharedState,
    },
};

pub const EDITOR_WIDTH: u32 = 1000;
pub const EDITOR_HEIGHT: u32 = 700;

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
    ToggleParam(ParamId, bool),
    SelectLfo(usize),
    SelectOsc(usize),
    SelectFilter(usize),
    SelectEg(usize),
    SelectMisc(usize),
    SelectRouting(usize),
}

struct State {
    shared: Arc<SharedState>,
    active_gestures: Vec<bool>,
    selected_lfo: usize,
    selected_osc: usize,
    selected_filter: usize,
    selected_eg: usize,
    selected_misc: usize,
    selected_routing: usize,
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    (
        State {
            shared,
            active_gestures: vec![false; ParamId::COUNT],
            selected_lfo: 0,
            selected_osc: 0,
            selected_filter: 0,
            selected_eg: 0,
            selected_misc: 0,
            selected_routing: 0,
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
        Message::SelectOsc(index) => {
            state.selected_osc = index;
        }
        Message::SelectFilter(index) => {
            state.selected_filter = index;
        }
        Message::SelectEg(index) => {
            state.selected_eg = index;
        }
        Message::SelectMisc(index) => {
            state.selected_misc = index;
        }
        Message::SelectRouting(index) => {
            state.selected_routing = index;
        }
    }
    Task::none()
}

fn small_knob<'a>(
    id: ParamId,
    label: &'a str,
    state: &'a State,
    step: f32,
) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let def = param_def(id).expect("valid param id");
    let slider = arch_slider(def.min as f32..=def.max as f32, value, move |v| {
        Message::SetParam(id, v)
    })
    .step(step)
    .double_click_reset(def.default as f32)
    .on_release(Message::ReleaseParam(id))
    .fill_from_start()
    .width(Length::Fixed(48.0))
    .height(Length::Fixed(48.0));

    let value_text = param_value_text(id, value, def.step);

    container(
        column![text(label).size(11), slider, text(value_text).size(10)]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(56.0))
    .into()
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
    let def = param_def(id).expect("valid param id");
    if def.flags == crate::synth::params::TOGGLE {
        small_checkbox(id, label, state)
    } else {
        small_knob(id, label, state, def.step as f32)
    }
}

fn osc_type_dropdown<'a>(id: ParamId, state: &'a State) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let osc_type = OscType::from_u8(value as u8);
    let options: Vec<OscType> = (0..=10).map(OscType::from_u8).collect();
    let dropdown = maolan_baseview::iced::widget::pick_list(options, Some(osc_type), move |t| {
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

fn filter_type_dropdown<'a>(id: ParamId, state: &'a State) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let filter_type = FilterType::from_u8(value as u8);
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

fn param_value_text(id: ParamId, value: f32, step: f64) -> String {
    match id {
        ParamId::Osc1Octave | ParamId::Osc2Octave | ParamId::Osc3Octave => {
            format!("{:.0}", value - 3.0)
        }
        ParamId::F1Type | ParamId::F2Type | ParamId::NoiseFilterType => {
            FilterType::from_u8(value as u8).name().to_string()
        }
        _ if step >= 1.0 => format!("{value:.0}"),
        _ => format!("{value:.2}"),
    }
}

fn vslider<'a>(id: ParamId, label: &'a str, state: &'a State) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let def = param_def(id).expect("valid param id");
    let slider = Slider::new(def.min as f32..=def.max as f32, value, move |v| {
        Message::SetParam(id, v)
    })
    .step(def.step as f32)
    .double_click_reset(def.default as f32)
    .on_release(Message::ReleaseParam(id))
    .width(Length::Fixed(20.0))
    .height(Length::Fixed(80.0));

    let value_text = param_value_text(id, value, def.step);

    container(
        column![text(label).size(11), slider, text(value_text).size(10)]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(32.0))
    .into()
}

#[allow(dead_code)]
fn hslider<'a>(id: ParamId, label: &'a str, state: &'a State) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let def = param_def(id).expect("valid param id");
    let slider = HorizontalSlider::new(def.min as f32..=def.max as f32, value, move |v| {
        Message::SetParam(id, v)
    })
    .step(def.step as f32)
    .double_click_reset(def.default as f32)
    .on_release(Message::ReleaseParam(id))
    .width(Length::Fixed(120.0))
    .height(Length::Fixed(16.0));

    let value_text = param_value_text(id, value, def.step);

    container(
        column![text(label).size(11), slider, text(value_text).size(10)]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(128.0))
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

fn panel_inner<'a>(
    title: Option<&'static str>,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    let inner = if let Some(t) = title {
        column![section_title(t), content]
            .spacing(6)
            .align_x(Alignment::Start)
            .into()
    } else {
        content
    };
    container(inner)
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

fn panel<'a>(title: &'static str, content: Element<'a, Message>) -> Element<'a, Message> {
    panel_inner(Some(title), content)
}

fn panel_no_title<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    panel_inner(None, content)
}

fn knob_row<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut r = row![].spacing(6).align_y(Alignment::Center);
    for item in items {
        r = r.push(item);
    }
    r.into()
}

fn knob_column<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut c = column![].spacing(6).align_x(Alignment::Center);
    for item in items {
        c = c.push(item);
    }
    c.into()
}

fn tab_button(label: &'static str, active: bool, msg: Message) -> Element<'static, Message> {
    button(
        container(text(label).size(11))
            .width(Length::Fixed(40.0))
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

fn view(state: &State) -> Element<'_, Message> {
    let fm_panel = panel_no_title(knob_row(vec![
        param_control(ParamId::OscFmMode, "Mode", state),
        param_control(ParamId::OscFmDepth, "Depth", state),
    ]));

    let filter_routing = panel_no_title(knob_row(vec![
        param_control(ParamId::FilterRouting, "Route", state),
        param_control(ParamId::FilterBalance, "Balance", state),
    ]));

    let top_bar = row![
        panel(
            "Output",
            knob_row(vec![
                param_control(ParamId::Volume, "Vol", state),
                param_control(ParamId::Pan, "Pan", state),
                param_control(ParamId::Width, "Width", state),
            ])
        ),
        panel(
            "Play Mode",
            knob_row(vec![
                param_control(ParamId::Polyphony, "Poly", state),
                param_control(ParamId::PlayMode, "Mode", state),
                param_control(ParamId::VoicePriority, "Priority", state),
                param_control(ParamId::Portamento, "Port", state),
                param_control(ParamId::PitchBendRange, "Bend", state),
            ])
        ),
        panel(
            "Scene",
            knob_row(vec![
                param_control(ParamId::OscDrift, "Drift", state),
                param_control(ParamId::NoiseColor, "Noise Col", state),
            ])
        ),
        column![
            row![
                tab_button("FM", state.selected_routing == 0, Message::SelectRouting(0)),
                tab_button(
                    "Filter",
                    state.selected_routing == 1,
                    Message::SelectRouting(1)
                ),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
            match state.selected_routing {
                0 => fm_panel,
                _ => filter_routing,
            },
        ]
        .spacing(4),
    ]
    .spacing(10)
    .align_y(Alignment::Start);

    let osc1 = panel_no_title(
        column![
            knob_row(vec![
                osc_type_dropdown(ParamId::Osc1Type, state),
                param_control(ParamId::Osc1Waveform, "Wave", state),
                param_control(ParamId::Osc1Octave, "Oct", state),
                param_control(ParamId::Osc1Semitone, "Semi", state),
                param_control(ParamId::Osc1Fine, "Fine", state),
                param_control(ParamId::Osc1Shape, "Shape", state),
                param_control(ParamId::Osc1Skew, "Skew", state),
            ]),
            knob_row(vec![
                param_control(ParamId::Osc1Unison, "Unison", state),
                param_control(ParamId::Osc1UnisonDetune, "UniDet", state),
                param_control(ParamId::Osc1Level, "Level", state),
                param_control(ParamId::Osc1UnisonSpread, "UniSpr", state),
                knob_column(vec![
                    param_control(ParamId::Osc1Sync, "Sync", state),
                    param_control(ParamId::Osc1Enabled, "On", state),
                ]),
            ]),
        ]
        .spacing(6)
        .into(),
    );

    let osc2 = panel_no_title(
        column![
            knob_row(vec![
                osc_type_dropdown(ParamId::Osc2Type, state),
                param_control(ParamId::Osc2Waveform, "Wave", state),
                param_control(ParamId::Osc2Octave, "Oct", state),
                param_control(ParamId::Osc2Semitone, "Semi", state),
                param_control(ParamId::Osc2Fine, "Fine", state),
                param_control(ParamId::Osc2Shape, "Shape", state),
                param_control(ParamId::Osc2Skew, "Skew", state),
            ]),
            knob_row(vec![
                param_control(ParamId::Osc2Unison, "Unison", state),
                param_control(ParamId::Osc2UnisonDetune, "UniDet", state),
                param_control(ParamId::Osc2Level, "Level", state),
                param_control(ParamId::Osc2UnisonSpread, "UniSpr", state),
                knob_column(vec![
                    param_control(ParamId::Osc2Sync, "Sync", state),
                    param_control(ParamId::Osc2Enabled, "On", state),
                ]),
            ]),
        ]
        .spacing(6)
        .into(),
    );

    let osc3 = panel_no_title(
        column![
            knob_row(vec![
                osc_type_dropdown(ParamId::Osc3Type, state),
                param_control(ParamId::Osc3Waveform, "Wave", state),
                param_control(ParamId::Osc3Octave, "Oct", state),
                param_control(ParamId::Osc3Semitone, "Semi", state),
                param_control(ParamId::Osc3Fine, "Fine", state),
                param_control(ParamId::Osc3Shape, "Shape", state),
                param_control(ParamId::Osc3Formant, "Formant", state),
            ]),
            knob_row(vec![
                param_control(ParamId::Osc3Unison, "Unison", state),
                param_control(ParamId::Osc3UnisonDetune, "UniDet", state),
                param_control(ParamId::Osc3Level, "Level", state),
                param_control(ParamId::Osc3UnisonSpread, "UniSpr", state),
                knob_column(vec![
                    param_control(ParamId::Osc2Sync, "Sync", state),
                    param_control(ParamId::Osc2Enabled, "On", state),
                ]),
            ]),
        ]
        .spacing(6)
        .into(),
    );

    let osc_selector = row![
        tab_button("Osc 1", state.selected_osc == 0, Message::SelectOsc(0)),
        tab_button("Osc 2", state.selected_osc == 1, Message::SelectOsc(1)),
        tab_button("Osc 3", state.selected_osc == 2, Message::SelectOsc(2)),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let selected_osc_panel = match state.selected_osc {
        0 => osc1,
        1 => osc2,
        _ => osc3,
    };

    let filter1 = panel_no_title(
        column![
            knob_row(vec![
                filter_type_dropdown(ParamId::F1Type, state),
                param_control(ParamId::F1Subtype, "Sub", state),
                param_control(ParamId::F1Cutoff, "Cut", state),
                param_control(ParamId::F1Resonance, "Res", state),
                param_control(ParamId::F1EgAmount, "EG", state),
                param_control(ParamId::F1KeyTrack, "Key", state),
                param_control(ParamId::F1Drive, "Drive", state),
            ]),
            knob_row(vec![param_control(ParamId::F1Enabled, "On", state),]),
        ]
        .spacing(6)
        .into(),
    );

    let filter2 = panel_no_title(
        column![
            knob_row(vec![
                filter_type_dropdown(ParamId::F2Type, state),
                param_control(ParamId::F2Subtype, "Sub", state),
                param_control(ParamId::F2Cutoff, "Cut", state),
                param_control(ParamId::F2Resonance, "Res", state),
                param_control(ParamId::F2EgAmount, "EG", state),
                param_control(ParamId::F2KeyTrack, "Key", state),
                param_control(ParamId::F2Drive, "Drive", state),
            ]),
            knob_row(vec![param_control(ParamId::F2Enabled, "On", state),]),
        ]
        .spacing(6)
        .into(),
    );

    let macros = panel(
        "Macros",
        knob_row(vec![
            param_control(ParamId::Macro1, "M1", state),
            param_control(ParamId::Macro2, "M2", state),
            param_control(ParamId::Macro3, "M3", state),
            param_control(ParamId::Macro4, "M4", state),
            param_control(ParamId::Macro5, "M5", state),
            param_control(ParamId::Macro6, "M6", state),
        ]),
    );

    let waveshaper = panel_no_title(
        column![
            knob_row(vec![
                param_control(ParamId::WaveshaperShape, "Shape", state),
                param_control(ParamId::WaveshaperDrive, "Drive", state),
                param_control(ParamId::WaveshaperMix, "Mix", state),
            ]),
            knob_row(vec![param_control(ParamId::WaveshaperEnabled, "On", state),]),
        ]
        .spacing(6)
        .into(),
    );

    let amp_eg = panel_no_title(knob_row(vec![
        vslider(ParamId::AmpAttack, "A", state),
        vslider(ParamId::AmpDecay, "D", state),
        vslider(ParamId::AmpSustain, "S", state),
        vslider(ParamId::AmpRelease, "R", state),
        param_control(ParamId::AmpEgMode, "Mode", state),
    ]));

    let filter_eg = panel_no_title(knob_row(vec![
        vslider(ParamId::FilterAttack, "A", state),
        vslider(ParamId::FilterDecay, "D", state),
        vslider(ParamId::FilterSustain, "S", state),
        vslider(ParamId::FilterRelease, "R", state),
        param_control(ParamId::FilterEgMode, "Mode", state),
    ]));

    let pitch_eg = panel_no_title(knob_row(vec![
        vslider(ParamId::PitchAttack, "A", state),
        vslider(ParamId::PitchDecay, "D", state),
        vslider(ParamId::PitchSustain, "S", state),
        vslider(ParamId::PitchRelease, "R", state),
        param_control(ParamId::PitchEgMode, "Mode", state),
    ]));

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

    let eg_selector = row![
        tab_button("Amp", state.selected_eg == 0, Message::SelectEg(0)),
        tab_button("Filter", state.selected_eg == 1, Message::SelectEg(1)),
        tab_button("Pitch", state.selected_eg == 2, Message::SelectEg(2)),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let selected_eg_panel = match state.selected_eg {
        0 => amp_eg,
        1 => filter_eg,
        _ => pitch_eg,
    };

    let noise = panel_no_title(
        column![
            knob_row(vec![
                param_control(ParamId::NoiseType, "Type", state),
                param_control(ParamId::NoiseLevel, "Level", state),
                param_control(ParamId::NoiseFilterType, "FType", state),
            ]),
            knob_row(vec![
                param_control(ParamId::NoiseFilterCutoff, "FCut", state),
                param_control(ParamId::NoiseFilterResonance, "FRes", state),
                knob_column(vec![
                    param_control(ParamId::NoiseFilterEnabled, "FOn", state),
                    param_control(ParamId::NoiseEnabled, "On", state),
                ])
            ]),
        ]
        .spacing(6)
        .into(),
    );

    let flavor = panel_no_title(knob_row(vec![
        param_control(ParamId::FlavorType, "Type", state),
        param_control(ParamId::FlavorCutoff, "Cut", state),
        param_control(ParamId::FlavorResonance, "Res", state),
    ]));

    let misc_selector = row![
        tab_button("Shaper", state.selected_misc == 0, Message::SelectMisc(0)),
        tab_button("Noise", state.selected_misc == 1, Message::SelectMisc(1)),
        tab_button("Flavor", state.selected_misc == 2, Message::SelectMisc(2)),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let selected_misc_panel = match state.selected_misc {
        0 => waveshaper,
        1 => noise,
        _ => flavor,
    };

    let right_column = column![
        filter_selector,
        selected_filter_panel,
        macros,
        row![
            column![eg_selector, selected_eg_panel].spacing(10),
            column![misc_selector, selected_misc_panel].spacing(10),
        ]
        .spacing(10),
    ]
    .spacing(10);

    let lfo1 = panel_no_title(knob_row(vec![
        param_control(ParamId::Lfo1Rate, "Rate", state),
        param_control(ParamId::Lfo1Shape, "Shape", state),
        param_control(ParamId::Lfo1Amount, "Amt", state),
        param_control(ParamId::Lfo1Deform, "Deform", state),
        param_control(ParamId::Lfo1Phase, "Phase", state),
        param_control(ParamId::Lfo1Trigger, "Trig", state),
        knob_column(vec![
            param_control(ParamId::Lfo1Unipolar, "Uni", state),
            param_control(ParamId::Lfo1SyncMode, "Sync", state),
        ]),
    ]));

    let lfo2 = panel_no_title(knob_row(vec![
        param_control(ParamId::Lfo2Rate, "Rate", state),
        param_control(ParamId::Lfo2Shape, "Shape", state),
        param_control(ParamId::Lfo2Amount, "Amt", state),
        param_control(ParamId::Lfo2Deform, "Deform", state),
        param_control(ParamId::Lfo2Phase, "Phase", state),
        param_control(ParamId::Lfo2Trigger, "Trig", state),
        knob_column(vec![
            param_control(ParamId::Lfo2Unipolar, "Uni", state),
            param_control(ParamId::Lfo2SyncMode, "Sync", state),
        ]),
    ]));

    let lfo3 = panel_no_title(knob_row(vec![
        param_control(ParamId::Lfo3Rate, "Rate", state),
        param_control(ParamId::Lfo3Shape, "Shape", state),
        param_control(ParamId::Lfo3Amount, "Amt", state),
        param_control(ParamId::Lfo3Deform, "Deform", state),
        param_control(ParamId::Lfo3Phase, "Phase", state),
        param_control(ParamId::Lfo3Trigger, "Trig", state),
        knob_column(vec![
            param_control(ParamId::Lfo3Unipolar, "Uni", state),
            param_control(ParamId::Lfo3SyncMode, "Sync", state),
        ]),
    ]));

    let lfo4 = panel_no_title(knob_row(vec![
        param_control(ParamId::Lfo4Rate, "Rate", state),
        param_control(ParamId::Lfo4Shape, "Shape", state),
        param_control(ParamId::Lfo4Amount, "Amt", state),
        param_control(ParamId::Lfo4Deform, "Deform", state),
        param_control(ParamId::Lfo4Phase, "Phase", state),
        param_control(ParamId::Lfo4Trigger, "Trig", state),
        knob_column(vec![
            param_control(ParamId::Lfo4Unipolar, "Uni", state),
            param_control(ParamId::Lfo4SyncMode, "Sync", state),
        ]),
    ]));

    let lfo5 = panel_no_title(knob_row(vec![
        param_control(ParamId::Lfo5Rate, "Rate", state),
        param_control(ParamId::Lfo5Shape, "Shape", state),
        param_control(ParamId::Lfo5Amount, "Amt", state),
        param_control(ParamId::Lfo5Deform, "Deform", state),
        param_control(ParamId::Lfo5Phase, "Phase", state),
        param_control(ParamId::Lfo5Trigger, "Trig", state),
        knob_column(vec![
            param_control(ParamId::Lfo5Unipolar, "Uni", state),
            param_control(ParamId::Lfo5SyncMode, "Sync", state),
        ]),
    ]));

    let lfo6 = panel_no_title(knob_row(vec![
        param_control(ParamId::Lfo6Rate, "Rate", state),
        param_control(ParamId::Lfo6Shape, "Shape", state),
        param_control(ParamId::Lfo6Amount, "Amt", state),
        param_control(ParamId::Lfo6Deform, "Deform", state),
        param_control(ParamId::Lfo6Phase, "Phase", state),
        param_control(ParamId::Lfo6Trigger, "Trig", state),
        knob_column(vec![
            param_control(ParamId::Lfo6Unipolar, "Uni", state),
            param_control(ParamId::Lfo6SyncMode, "Sync", state),
        ]),
    ]));

    let lfo_selector = row![
        tab_button("LFO 1", state.selected_lfo == 0, Message::SelectLfo(0)),
        tab_button("LFO 2", state.selected_lfo == 1, Message::SelectLfo(1)),
        tab_button("LFO 3", state.selected_lfo == 2, Message::SelectLfo(2)),
        tab_button("LFO 4", state.selected_lfo == 3, Message::SelectLfo(3)),
        tab_button("LFO 5", state.selected_lfo == 4, Message::SelectLfo(4)),
        tab_button("LFO 6", state.selected_lfo == 5, Message::SelectLfo(5)),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let selected_lfo_panel = match state.selected_lfo {
        0 => lfo1,
        1 => lfo2,
        2 => lfo3,
        3 => lfo4,
        4 => lfo5,
        _ => lfo6,
    };

    let left_column = column![
        osc_selector,
        selected_osc_panel,
        lfo_selector,
        selected_lfo_panel
    ]
    .spacing(10);

    let main_content = row![left_column, right_column]
        .spacing(12)
        .align_y(Alignment::Start);

    let content = column![top_bar, main_content]
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

#[cfg(test)]
mod tests {
    use super::{ParamId, param_value_text};

    #[test]
    fn octave_readout_shows_musical_offset() {
        assert_eq!(param_value_text(ParamId::Osc1Octave, 1.0, 1.0), "-2");
        assert_eq!(param_value_text(ParamId::Osc1Octave, 2.0, 1.0), "-1");
        assert_eq!(param_value_text(ParamId::Osc1Octave, 3.0, 1.0), "0");
        assert_eq!(param_value_text(ParamId::Osc1Octave, 4.0, 1.0), "1");
        assert_eq!(param_value_text(ParamId::Osc1Octave, 5.0, 1.0), "2");
    }
}
