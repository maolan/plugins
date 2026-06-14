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

use crate::sampler::{
    params::{PARAMS, ParamId},
    plugin::SharedState,
};

pub const EDITOR_WIDTH: u32 = 1100;
pub const EDITOR_HEIGHT: u32 = 750;

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

fn view(state: &State) -> Element<'_, Message> {
    let top_bar = row![
        panel(
            "Master",
            knob_row(vec![
                small_knob(ParamId::MasterGain, "Gain", state, 0.01),
                small_knob(ParamId::MasterPan, "Pan", state, 0.01),
            ])
        ),
        panel(
            "Pitch",
            knob_row(vec![
                small_knob(ParamId::PitchBendUp, "Bend Up", state, 1.0),
                small_knob(ParamId::PitchBendDown, "Bend Down", state, 1.0),
            ])
        ),
    ]
    .spacing(10)
    .align_y(Alignment::Start);

    let amp_eg = panel(
        "Amp EG",
        knob_row(vec![
            small_knob(ParamId::AmpAttack, "A", state, 0.01),
            small_knob(ParamId::AmpDecay, "D", state, 0.01),
            small_knob(ParamId::AmpSustain, "S", state, 0.01),
            small_knob(ParamId::AmpRelease, "R", state, 0.01),
        ]),
    );

    let filter = panel(
        "Filter",
        knob_row(vec![
            small_knob(ParamId::FilterType, "Type", state, 1.0),
            small_knob(ParamId::FilterCutoff, "Cut", state, 1.0),
            small_knob(ParamId::FilterResonance, "Res", state, 0.01),
            small_knob(ParamId::FilterEgAmount, "EG", state, 0.01),
            small_knob(ParamId::FilterEnabled, "On", state, 1.0),
        ]),
    );

    let filter_eg = panel(
        "Filter EG",
        knob_row(vec![
            small_knob(ParamId::FilterAttack, "A", state, 0.01),
            small_knob(ParamId::FilterDecay, "D", state, 0.01),
            small_knob(ParamId::FilterSustain, "S", state, 0.01),
            small_knob(ParamId::FilterRelease, "R", state, 0.01),
        ]),
    );

    let middle_row = row![amp_eg, filter, filter_eg]
        .spacing(10)
        .align_y(Alignment::Start);

    let eg2 = panel(
        "EG 2",
        knob_row(vec![
            small_knob(ParamId::Eg2Attack, "A", state, 0.01),
            small_knob(ParamId::Eg2Decay, "D", state, 0.01),
            small_knob(ParamId::Eg2Sustain, "S", state, 0.01),
            small_knob(ParamId::Eg2Release, "R", state, 0.01),
        ]),
    );

    let eg3 = panel(
        "EG 3",
        knob_row(vec![
            small_knob(ParamId::Eg3Attack, "A", state, 0.01),
            small_knob(ParamId::Eg3Decay, "D", state, 0.01),
            small_knob(ParamId::Eg3Sustain, "S", state, 0.01),
            small_knob(ParamId::Eg3Release, "R", state, 0.01),
        ]),
    );

    let eg4 = panel(
        "EG 4",
        knob_row(vec![
            small_knob(ParamId::Eg4Attack, "A", state, 0.01),
            small_knob(ParamId::Eg4Decay, "D", state, 0.01),
            small_knob(ParamId::Eg4Sustain, "S", state, 0.01),
            small_knob(ParamId::Eg4Release, "R", state, 0.01),
        ]),
    );

    let eg5 = panel(
        "EG 5",
        knob_row(vec![
            small_knob(ParamId::Eg5Attack, "A", state, 0.01),
            small_knob(ParamId::Eg5Decay, "D", state, 0.01),
            small_knob(ParamId::Eg5Sustain, "S", state, 0.01),
            small_knob(ParamId::Eg5Release, "R", state, 0.01),
        ]),
    );

    let eg_row = row![eg2, eg3, eg4, eg5]
        .spacing(10)
        .align_y(Alignment::Start);

    let lfo1 = panel(
        "LFO 1",
        knob_row(vec![
            small_knob(ParamId::Lfo1Rate, "Rate", state, 0.01),
            small_knob(ParamId::Lfo1Amount, "Amt", state, 0.01),
            small_knob(ParamId::Lfo1Shape, "Shape", state, 1.0),
            small_knob(ParamId::Lfo1Enabled, "On", state, 1.0),
        ]),
    );

    let lfo2 = panel(
        "LFO 2",
        knob_row(vec![
            small_knob(ParamId::Lfo2Rate, "Rate", state, 0.01),
            small_knob(ParamId::Lfo2Amount, "Amt", state, 0.01),
            small_knob(ParamId::Lfo2Shape, "Shape", state, 1.0),
            small_knob(ParamId::Lfo2Enabled, "On", state, 1.0),
        ]),
    );

    let lfo_row = row![lfo1, lfo2].spacing(10).align_y(Alignment::Start);

    let content = column![top_bar, middle_row, eg_row, lfo_row]
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
