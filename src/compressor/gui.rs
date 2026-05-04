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
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{checkbox, column, container, row, scrollable, text},
};
use maolan_widgets::arch_slider::arch_slider;
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};

use crate::compressor::{
    params::{PARAMS, ParamId},
    plugin::SharedState,
};

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

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum Message {
    SetParam(ParamId, f32),
    SetBoolParam(ParamId, bool),
    SetScMode(u8),
    SetMode(u8),
    SetScBoost(u8),
    SetTopology(u8),
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
        Message::SetScMode(mode) => state.shared.set_param(ParamId::ScMode, mode as f64),
        Message::SetMode(mode) => state.shared.set_param(ParamId::Mode, mode as f64),
        Message::SetScBoost(mode) => state.shared.set_param(ParamId::ScBoost, mode as f64),
        Message::SetTopology(mode) => state.shared.set_param(ParamId::Topology, mode as f64),
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;
    let b = |id: ParamId| state.shared.params.get_bool(id);

    let mut content = column![text("Maolan MB Compressor (Stereo)").size(24)]
        .spacing(12)
        .align_x(Alignment::Start);

    content = content.push(
        row![
            knob(
                "Input",
                ParamId::InputGain,
                p(ParamId::InputGain),
                "dB",
                0.1
            ),
            knob(
                "Output",
                ParamId::OutputGain,
                p(ParamId::OutputGain),
                "dB",
                0.1
            ),
            knob("Dry", ParamId::DryGain, p(ParamId::DryGain), "", 0.01),
            knob("Wet", ParamId::WetGain, p(ParamId::WetGain), "", 0.01),
            knob(
                "Lookahead",
                ParamId::Lookahead,
                p(ParamId::Lookahead),
                "ms",
                0.01
            ),
        ]
        .spacing(12),
    );

    content = content.push(
        row![
            knob("Split 1", ParamId::Split1, p(ParamId::Split1), "Hz", 1.0),
            knob("Split 2", ParamId::Split2, p(ParamId::Split2), "Hz", 1.0),
            knob("Split 3", ParamId::Split3, p(ParamId::Split3), "Hz", 1.0),
        ]
        .spacing(12),
    );

    content = content.push(band_row(
        "Band 1",
        state,
        BandIds {
            th: ParamId::B1Threshold,
            ratio: ParamId::B1Ratio,
            att: ParamId::B1Attack,
            rel: ParamId::B1Release,
            knee: ParamId::B1Knee,
            makeup: ParamId::B1Makeup,
        },
    ));
    content = content.push(band_row(
        "Band 2",
        state,
        BandIds {
            th: ParamId::B2Threshold,
            ratio: ParamId::B2Ratio,
            att: ParamId::B2Attack,
            rel: ParamId::B2Release,
            knee: ParamId::B2Knee,
            makeup: ParamId::B2Makeup,
        },
    ));
    content = content.push(band_row(
        "Band 3",
        state,
        BandIds {
            th: ParamId::B3Threshold,
            ratio: ParamId::B3Ratio,
            att: ParamId::B3Attack,
            rel: ParamId::B3Release,
            knee: ParamId::B3Knee,
            makeup: ParamId::B3Makeup,
        },
    ));
    content = content.push(band_row(
        "Band 4",
        state,
        BandIds {
            th: ParamId::B4Threshold,
            ratio: ParamId::B4Ratio,
            att: ParamId::B4Attack,
            rel: ParamId::B4Release,
            knee: ParamId::B4Knee,
            makeup: ParamId::B4Makeup,
        },
    ));

    let sc_mode = state.shared.params.get_enum(ParamId::ScMode).min(1);
    let mode = state.shared.params.get_enum(ParamId::Mode).min(2);
    let sc_boost = state.shared.params.get_enum(ParamId::ScBoost).min(4);
    let topology = state.shared.params.get_enum(ParamId::Topology).min(1);
    content = content.push(
        row![
            text("Sidechain").size(16),
            maolan_baseview::iced::widget::radio(
                "Peak",
                0u8,
                Some(sc_mode as u8),
                Message::SetScMode
            ),
            maolan_baseview::iced::widget::radio(
                "RMS",
                1u8,
                Some(sc_mode as u8),
                Message::SetScMode
            ),
            checkbox(b(ParamId::Bypass))
                .label("Bypass")
                .on_toggle(|v| Message::SetBoolParam(ParamId::Bypass, v)),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    );
    content = content.push(
        row![
            text("Mode").size(16),
            maolan_baseview::iced::widget::radio(
                "Downward",
                0u8,
                Some(mode as u8),
                Message::SetMode
            ),
            maolan_baseview::iced::widget::radio("Upward", 1u8, Some(mode as u8), Message::SetMode),
            maolan_baseview::iced::widget::radio(
                "Boosting",
                2u8,
                Some(mode as u8),
                Message::SetMode
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    );
    content = content.push(
        row![
            text("SC Boost").size(16),
            maolan_baseview::iced::widget::radio(
                "Off",
                0u8,
                Some(sc_boost as u8),
                Message::SetScBoost
            ),
            maolan_baseview::iced::widget::radio(
                "BT+3",
                1u8,
                Some(sc_boost as u8),
                Message::SetScBoost
            ),
            maolan_baseview::iced::widget::radio(
                "MT+3",
                2u8,
                Some(sc_boost as u8),
                Message::SetScBoost
            ),
            maolan_baseview::iced::widget::radio(
                "BT+6",
                3u8,
                Some(sc_boost as u8),
                Message::SetScBoost
            ),
            maolan_baseview::iced::widget::radio(
                "MT+6",
                4u8,
                Some(sc_boost as u8),
                Message::SetScBoost
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    );
    content = content.push(
        row![
            text("Topology").size(16),
            maolan_baseview::iced::widget::radio(
                "Classic",
                0u8,
                Some(topology as u8),
                Message::SetTopology
            ),
            maolan_baseview::iced::widget::radio(
                "Modern",
                1u8,
                Some(topology as u8),
                Message::SetTopology
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    );

    container(scrollable(content))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Left)
        .align_y(Vertical::Top)
        .into()
}

struct BandIds {
    th: ParamId,
    ratio: ParamId,
    att: ParamId,
    rel: ParamId,
    knee: ParamId,
    makeup: ParamId,
}

fn band_row<'a>(title: &'static str, state: &'a State, ids: BandIds) -> Element<'a, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;
    container(
        column![
            text(title).size(16),
            row![
                knob("Th", ids.th, p(ids.th), "dB", 0.1),
                knob("Ratio", ids.ratio, p(ids.ratio), "", 0.1),
                knob("Atk", ids.att, p(ids.att), "ms", 0.1),
                knob("Rel", ids.rel, p(ids.rel), "ms", 0.1),
                knob("Knee", ids.knee, p(ids.knee), "dB", 0.1),
                knob("Makeup", ids.makeup, p(ids.makeup), "dB", 0.1),
            ]
            .spacing(12),
        ]
        .spacing(8),
    )
    .into()
}

fn theme(_state: &State) -> Theme {
    Theme::TokyoNight
}

fn knob(
    label: &'static str,
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
    .width(Length::Fixed(86.0))
    .height(Length::Fixed(86.0));

    let value_text = if units.is_empty() {
        format!("{value:.1}")
    } else if units == "Hz" {
        format!("{value:.0} {units}")
    } else {
        format!("{value:.1} {units}")
    };

    container(
        column![text(label).size(14), slider, text(value_text).size(13)]
            .spacing(4)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(96.0))
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
                title: String::from("Maolan MB Compressor"),
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
                        title: String::from("Maolan MB Compressor"),
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
