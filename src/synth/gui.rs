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
    Alignment, Background, Border, Color, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{button, checkbox, column, container, mouse_area, row, text},
};
use maolan_widgets::arch_slider::arch_slider;
use maolan_widgets::horizontal_slider::HorizontalSlider;
use maolan_widgets::slider::Slider;
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};

use crate::{
    common::{
        filter::FilterType,
        lfo_assignment::{LfoAssignmentConfig, LfoAssignmentState, ModRouteParamIds},
    },
    synth::{
        dsp::{ClassicWaveform, LfoShape, ModTarget, ModernSubWaveform, OscType},
        params::{ParamDef, ParamId, param_def},
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
    X11(u32),
    #[cfg(target_os = "windows")]
    Win32(*mut std::ffi::c_void),
}

impl HasWindowHandle for ParentWindowHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        match self {
            #[cfg(unix)]
            ParentWindowHandle::X11(window) => {
                let handle = raw_window_handle::XlibWindowHandle::new(*window as u64);
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
    Poll,
    SetParam(ParamId, f32),
    ReleaseParam(ParamId),
    ToggleParam(ParamId, bool),
    AssignLfoToParam(ParamId),
    ToggleLfoAssignment(usize),
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
    lfo_assignment: LfoAssignmentState,
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
            lfo_assignment: LfoAssignmentState::default(),
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
        Message::Poll => {}
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
        Message::AssignLfoToParam(id) => {
            if let Some(lfo_index) = state.lfo_assignment.armed_lfo() {
                assign_lfo_to_param(state, lfo_index, id);
            }
        }
        Message::ToggleLfoAssignment(index) => {
            state.lfo_assignment.toggle(index);
            state.selected_lfo = index;
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

const MOD_ROUTES: [ModRouteParamIds<ParamId>; 12] = [
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
    ModRouteParamIds {
        source: ParamId::ModRoute7Source,
        target: ParamId::ModRoute7Target,
        depth: ParamId::ModRoute7Depth,
    },
    ModRouteParamIds {
        source: ParamId::ModRoute8Source,
        target: ParamId::ModRoute8Target,
        depth: ParamId::ModRoute8Depth,
    },
    ModRouteParamIds {
        source: ParamId::ModRoute9Source,
        target: ParamId::ModRoute9Target,
        depth: ParamId::ModRoute9Depth,
    },
    ModRouteParamIds {
        source: ParamId::ModRoute10Source,
        target: ParamId::ModRoute10Target,
        depth: ParamId::ModRoute10Depth,
    },
    ModRouteParamIds {
        source: ParamId::ModRoute11Source,
        target: ParamId::ModRoute11Target,
        depth: ParamId::ModRoute11Depth,
    },
    ModRouteParamIds {
        source: ParamId::ModRoute12Source,
        target: ParamId::ModRoute12Target,
        depth: ParamId::ModRoute12Depth,
    },
];

const LFO_ASSIGNMENT_CONFIG: LfoAssignmentConfig<'static, ParamId> = LfoAssignmentConfig {
    routes: &MOD_ROUTES,
    first_lfo_source: 5,
    lfo_count: 6,
    default_depth: 0.5,
};

fn assign_lfo_to_param(state: &mut State, lfo_index: usize, id: ParamId) {
    let Some(target) = mod_target_for_param(id) else {
        return;
    };
    let shared = Arc::clone(&state.shared);
    LFO_ASSIGNMENT_CONFIG.assign(
        &state.shared.params,
        lfo_index,
        target as u8,
        |id, value| {
            shared.mark_gesture_begin_pending(id);
            shared.set_param_outbound_only(id, value as f64);
            shared.mark_gesture_end_pending(id);
        },
    );
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

fn visual_param_value(state: &State, id: ParamId, base_value: f32) -> f32 {
    let Some(target) = mod_target_for_param(id) else {
        return base_value;
    };
    let delta = if let Some(lfo_index) = state.lfo_assignment.armed_lfo() {
        if LFO_ASSIGNMENT_CONFIG.has_lfo_assignment(&state.shared.params, lfo_index, target as u8) {
            state.shared.visual_lfo_mod_value(lfo_index, target)
        } else {
            0.0
        }
    } else {
        (0..LFO_ASSIGNMENT_CONFIG.lfo_count as usize)
            .filter(|lfo_index| {
                LFO_ASSIGNMENT_CONFIG.has_lfo_assignment(
                    &state.shared.params,
                    *lfo_index,
                    target as u8,
                )
            })
            .map(|lfo_index| state.shared.visual_lfo_mod_value(lfo_index, target))
            .sum()
    };
    if delta.abs() <= f32::EPSILON {
        return base_value;
    }
    let def = param_def(id).expect("valid param id");
    let span = visual_mod_span(id, def);
    (base_value + delta * span).clamp(def.min as f32, def.max as f32)
}

fn visual_mod_span(id: ParamId, def: &ParamDef) -> f32 {
    match id {
        ParamId::F1Cutoff | ParamId::F2Cutoff | ParamId::FlavorCutoff => 10000.0,
        _ => (def.max - def.min) as f32,
    }
}

fn mod_target_for_param(id: ParamId) -> Option<ModTarget> {
    match id {
        ParamId::Osc1Octave | ParamId::Osc1Semitone | ParamId::Osc1Fine => {
            Some(ModTarget::Osc1Pitch)
        }
        ParamId::Osc2Octave | ParamId::Osc2Semitone | ParamId::Osc2Fine => {
            Some(ModTarget::Osc2Pitch)
        }
        ParamId::Osc3Octave | ParamId::Osc3Semitone | ParamId::Osc3Fine => {
            Some(ModTarget::Osc3Pitch)
        }
        ParamId::Osc1Level => Some(ModTarget::Osc1Level),
        ParamId::Osc2Level => Some(ModTarget::Osc2Level),
        ParamId::Osc3Level => Some(ModTarget::Osc3Level),
        ParamId::Osc1Shape => Some(ModTarget::Osc1Shape),
        ParamId::Osc2Shape => Some(ModTarget::Osc2Shape),
        ParamId::Osc3Shape => Some(ModTarget::Osc3Shape),
        ParamId::Osc1Skew => Some(ModTarget::Osc1Skew),
        ParamId::Osc2Skew => Some(ModTarget::Osc2Skew),
        ParamId::Osc3Skew => Some(ModTarget::Osc3Skew),
        ParamId::Osc3Formant => Some(ModTarget::Osc3Formant),
        ParamId::F1Cutoff => Some(ModTarget::Filter1Cutoff),
        ParamId::F1Resonance => Some(ModTarget::Filter1Resonance),
        ParamId::F1EgAmount => Some(ModTarget::Filter1EgAmount),
        ParamId::F1Drive => Some(ModTarget::Filter1Drive),
        ParamId::F2Cutoff => Some(ModTarget::Filter2Cutoff),
        ParamId::F2Resonance => Some(ModTarget::Filter2Resonance),
        ParamId::F2EgAmount => Some(ModTarget::Filter2EgAmount),
        ParamId::F2Drive => Some(ModTarget::Filter2Drive),
        ParamId::AmpAttack => Some(ModTarget::AmpAttack),
        ParamId::AmpDecay => Some(ModTarget::AmpDecay),
        ParamId::AmpSustain => Some(ModTarget::AmpSustain),
        ParamId::AmpRelease => Some(ModTarget::AmpRelease),
        ParamId::FilterAttack => Some(ModTarget::FilterAttack),
        ParamId::FilterDecay => Some(ModTarget::FilterDecay),
        ParamId::FilterSustain => Some(ModTarget::FilterSustain),
        ParamId::FilterRelease => Some(ModTarget::FilterRelease),
        ParamId::PitchAttack => Some(ModTarget::PitchAttack),
        ParamId::PitchDecay => Some(ModTarget::PitchDecay),
        ParamId::PitchSustain => Some(ModTarget::PitchSustain),
        ParamId::PitchRelease => Some(ModTarget::PitchRelease),
        ParamId::Volume => Some(ModTarget::OutputVolume),
        ParamId::Pan => Some(ModTarget::OutputPan),
        ParamId::Width => Some(ModTarget::OutputWidth),
        ParamId::NoiseLevel => Some(ModTarget::NoiseLevel),
        ParamId::WaveshaperDrive => Some(ModTarget::WaveshaperDrive),
        ParamId::Portamento => Some(ModTarget::Portamento),
        ParamId::FlavorCutoff => Some(ModTarget::FlavorCutoff),
        ParamId::FilterBalance => Some(ModTarget::FilterBalance),
        ParamId::OscFmDepth => Some(ModTarget::OscFmDepth),
        ParamId::Osc1Sync => Some(ModTarget::Osc1Sync),
        ParamId::Osc2Sync => Some(ModTarget::Osc2Sync),
        ParamId::Osc3Sync => Some(ModTarget::Osc3Sync),
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
    let def = param_def(id).expect("valid param id");
    let assigned = param_has_lfo_assignment(state, id);
    let display_value = visual_param_value(state, id, value);
    let mut slider = arch_slider(def.min as f32..=def.max as f32, display_value, move |v| {
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

    let value_text = param_value_text(id, display_value, def.step);

    let content = container(
        column![text(label).size(11), slider, text(value_text).size(10)]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(56.0))
    .padding(2)
    .style(move |_theme: &Theme| {
        let border_color = if assigned {
            assignment_color()
        } else {
            Color::TRANSPARENT
        };
        container::Style {
            background: assigned
                .then(|| Background::Color(Color::from_rgba(1.0, 0.83, 0.10, 0.10))),
            border: Border {
                color: border_color,
                width: if assigned { 1.0 } else { 0.0 },
                radius: 3.0.into(),
            },
            ..container::Style::default()
        }
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

fn waveform_dropdown<'a>(state: &'a State, osc_index: usize) -> Element<'a, Message> {
    let (type_id, wave_id) = match osc_index {
        0 => (ParamId::Osc1Type, ParamId::Osc1Waveform),
        1 => (ParamId::Osc2Type, ParamId::Osc2Waveform),
        _ => (ParamId::Osc3Type, ParamId::Osc3Waveform),
    };
    let osc_type = OscType::from_u8(state.shared.params.get(type_id) as u8);
    let value = state.shared.params.get(wave_id) as f32;

    match osc_type {
        OscType::Classic => {
            let waveform = ClassicWaveform::from_u8(value as u8);
            let options = vec![
                ClassicWaveform::Saw,
                ClassicWaveform::Square,
                ClassicWaveform::Pulse,
                ClassicWaveform::Triangle,
            ];
            let dropdown =
                maolan_baseview::iced::widget::pick_list(options, Some(waveform), move |t| {
                    Message::SetParam(wave_id, t as u8 as f32)
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
        OscType::Modern => {
            let waveform = ModernSubWaveform::from_u8(value as u8);
            let options = vec![
                ModernSubWaveform::Square,
                ModernSubWaveform::Triangle,
                ModernSubWaveform::Saw,
            ];
            let dropdown =
                maolan_baseview::iced::widget::pick_list(options, Some(waveform), move |t| {
                    Message::SetParam(wave_id, t as u8 as f32)
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
        _ => param_control(wave_id, "Wave", state),
    }
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

fn lfo_shape_dropdown<'a>(id: ParamId, state: &'a State) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let shape = LfoShape::from_u8(value as u8);
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
    let assigned = param_has_lfo_assignment(state, id);
    let display_value = visual_param_value(state, id, value);
    let slider = Slider::new(def.min as f32..=def.max as f32, display_value, move |v| {
        Message::SetParam(id, v)
    })
    .step(def.step as f32)
    .double_click_reset(def.default as f32)
    .on_release(Message::ReleaseParam(id))
    .width(Length::Fixed(20.0))
    .height(Length::Fixed(80.0));

    let value_text = param_value_text(id, display_value, def.step);

    let content = container(
        column![text(label).size(11), slider, text(value_text).size(10)]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(32.0))
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

#[allow(dead_code)]
fn hslider<'a>(id: ParamId, label: &'a str, state: &'a State) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let def = param_def(id).expect("valid param id");
    let assigned = param_has_lfo_assignment(state, id);
    let display_value = visual_param_value(state, id, value);
    let slider = HorizontalSlider::new(def.min as f32..=def.max as f32, display_value, move |v| {
        Message::SetParam(id, v)
    })
    .step(def.step as f32)
    .double_click_reset(def.default as f32)
    .on_release(Message::ReleaseParam(id))
    .width(Length::Fixed(120.0))
    .height(Length::Fixed(16.0));

    let value_text = param_value_text(id, display_value, def.step);

    let content = container(
        column![text(label).size(11), slider, text(value_text).size(10)]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(128.0))
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
                waveform_dropdown(state, 0),
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
                waveform_dropdown(state, 1),
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
                waveform_dropdown(state, 2),
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
        row![
            column![eg_selector, selected_eg_panel].spacing(10),
            column![misc_selector, selected_misc_panel].spacing(10),
        ]
        .spacing(10),
    ]
    .spacing(10);

    let lfo1 = panel_no_title(knob_row(vec![
        lfo_shape_dropdown(ParamId::Lfo1Shape, state),
        param_control(ParamId::Lfo1Rate, "Rate", state),
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
        lfo_shape_dropdown(ParamId::Lfo2Shape, state),
        param_control(ParamId::Lfo2Rate, "Rate", state),
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
        lfo_shape_dropdown(ParamId::Lfo3Shape, state),
        param_control(ParamId::Lfo3Rate, "Rate", state),
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
        lfo_shape_dropdown(ParamId::Lfo4Shape, state),
        param_control(ParamId::Lfo4Rate, "Rate", state),
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
        lfo_shape_dropdown(ParamId::Lfo5Shape, state),
        param_control(ParamId::Lfo5Rate, "Rate", state),
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
        lfo_shape_dropdown(ParamId::Lfo6Shape, state),
        param_control(ParamId::Lfo6Rate, "Rate", state),
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
        lfo_tab_button("LFO 1", 0, state),
        lfo_tab_button("LFO 2", 1, state),
        lfo_tab_button("LFO 3", 2, state),
        lfo_tab_button("LFO 4", 3, state),
        lfo_tab_button("LFO 5", 4, state),
        lfo_tab_button("LFO 6", 5, state),
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
        .subscription(|_state| maolan_baseview::iced::poll_events().map(|_| Message::Poll))
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

        let notifier = maolan_baseview::iced::PollSubNotifier::new();
        *shared.poll_notifier.lock() = Some(notifier.clone());

        let handle =
            maolan_baseview::iced::shell::open_parented(&parent, settings, notifier, move || {
                build_app(shared)
            });

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
        let notifier = maolan_baseview::iced::PollSubNotifier::new();
        *shared.poll_notifier.lock() = Some(notifier.clone());
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
            maolan_baseview::iced::shell::open_blocking(settings, notifier, move || {
                build_app(shared)
            });
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
