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
use clap_clap::ffi::CLAP_WINDOW_API_X11;
use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{checkbox, column, container, row, scrollable, text},
};
use maolan_widgets::arch_slider::arch_slider;
use maolan_widgets::slider::slider;
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};

use crate::{
    params::{PARAMS, ParamId},
    plugin::SharedState,
};

pub const EDITOR_WIDTH: u32 = 1024;
pub const EDITOR_HEIGHT: u32 = 780;

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
pub enum Message {
    SetParam(ParamId, f32),
    SetBoolParam(ParamId, bool),
}

struct State {
    shared: Arc<SharedState>,
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    (State { shared }, Task::none())
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::SetParam(id, value) => state.shared.set_param(id, value as f64),
        Message::SetBoolParam(id, value) => {
            state.shared.set_param(id, if value { 1.0 } else { 0.0 })
        }
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;
    let b = |id: ParamId| state.shared.params.get_bool(id);

    let mut content = column![text("Maolan Equalizer x32 Stereo").size(24)]
        .spacing(10)
        .align_x(Alignment::Start);

    content = content.push(
        row![
            knob(
                "Input".to_string(),
                ParamId::InputGain,
                p(ParamId::InputGain),
                "dB",
                0.1
            ),
            knob(
                "Output".to_string(),
                ParamId::OutputGain,
                p(ParamId::OutputGain),
                "dB",
                0.1
            ),
            checkbox(b(ParamId::ParametricBypass))
                .label("Bypass Parametric")
                .on_toggle(|v| Message::SetBoolParam(ParamId::ParametricBypass, v)),
            checkbox(b(ParamId::GraphicBypass))
                .label("Bypass Graphic")
                .on_toggle(|v| Message::SetBoolParam(ParamId::GraphicBypass, v)),
        ]
        .spacing(14)
        .align_y(Alignment::Center),
    );

    content = content.push(text("Top: Parametric EQ (32 bands)").size(18));
    let mut parametric_bands = row![].spacing(10);
    for i in 0..32 {
        parametric_bands = parametric_bands.push(parametric_band(state, i));
    }
    content = content.push(
        scrollable(parametric_bands)
            .direction(scrollable::Direction::Horizontal(Default::default()))
            .height(Length::Fixed(350.0)),
    );

    content = content.push(text("Bottom: Graphic EQ (32 bands)").size(18));
    let mut graphic_bands = row![].spacing(10);
    for i in 0..32 {
        graphic_bands = graphic_bands.push(graphic_band(state, i));
    }
    content = content.push(
        scrollable(graphic_bands)
            .direction(scrollable::Direction::Horizontal(Default::default()))
            .height(Length::Fixed(150.0)),
    );

    container(scrollable(content))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Left)
        .align_y(Vertical::Top)
        .into()
}

fn parametric_band(state: &State, index: usize) -> Element<'_, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;
    let fid = ParamId::para_freq(index);
    let gid = ParamId::para_gain(index);
    let qid = ParamId::para_q(index);
    let label = format!("P{:02}", index + 1);

    column![
        text(label).size(14),
        knob("Freq".to_string(), fid, p(fid), "Hz", 1.0),
        knob("Gain".to_string(), gid, p(gid), "dB", 0.1),
        knob("Q".to_string(), qid, p(qid), "", 0.01),
    ]
    .spacing(5)
    .align_x(Alignment::Center)
    .into()
}

fn graphic_band(state: &State, index: usize) -> Element<'_, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;
    let id = ParamId::graphic_gain(index);
    let label = format!("G{:02}", index + 1);

    vertical_knob(label, id, p(id), "dB", 0.1)
}

fn theme(_state: &State) -> Theme {
    Theme::TokyoNight
}

fn vertical_knob(
    label: String,
    id: ParamId,
    value: f32,
    _units: &'static str,
    step: f32,
) -> Element<'static, Message> {
    let def = PARAMS[id.as_index()];
    let slider = slider(def.min as f32..=def.max as f32, value, move |v| {
        Message::SetParam(id, v)
    })
    .step(step)
    .double_click_reset(def.default as f32)
    .width(Length::Fixed(20.0))
    .height(Length::Fixed(100.0));

    let value_text = format!("{value:.1}");

    container(
        column![text(label).size(12), slider, text(value_text).size(11)]
            .spacing(3)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(40.0))
    .into()
}

fn knob(
    label: String,
    id: ParamId,
    value: f32,
    units: &'static str,
    step: f32,
) -> Element<'static, Message> {
    let def = PARAMS[id.as_index()];
    let slider = arch_slider(def.min as f32..=def.max as f32, value, move |v| {
        Message::SetParam(id, v)
    })
    .step(step)
    .double_click_reset(def.default as f32)
    .fill_from_start()
    .width(Length::Fixed(41.0))
    .height(Length::Fixed(41.0));

    let value_text = if units.is_empty() {
        format!("{value:.2}")
    } else if units == "Hz" {
        format!("{value:.0} {units}")
    } else {
        format!("{value:.1} {units}")
    };

    container(
        column![text(label).size(11), slider, text(value_text).size(10)]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(50.0))
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

pub struct GuiBridge {
    created: bool,
    floating: bool,
    shared: Option<Arc<SharedState>>,
    floating_open: Arc<AtomicBool>,
    window_handle: Option<AnyWindowHandle>,
}

impl Default for GuiBridge {
    fn default() -> Self {
        Self {
            created: false,
            floating: false,
            shared: None,
            floating_open: Arc::new(AtomicBool::new(false)),
            window_handle: None,
        }
    }
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
                title: String::from("Maolan Equalizer"),
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
                        title: String::from("Maolan Equalizer"),
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
}
