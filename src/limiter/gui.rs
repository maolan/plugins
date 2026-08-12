use std::{
    ffi::CStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

#[cfg(target_os = "windows")]
use clap_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(unix)]
use clap_clap::ffi::CLAP_WINDOW_API_X11;
use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{column, container, row, text},
};
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};

use crate::{
    common::{
        ui::{SmallKnob, VerticalSlider, small_knob, vertical_slider, vertical_ticks, vu_meter},
        waveform::ScrollingWaveformWidget,
    },
    limiter::{
        params::{PARAMS, ParamId},
        plugin::{SharedState, WAVEFORM_POINTS},
    },
};

pub const EDITOR_WIDTH: u32 = 900;
pub const EDITOR_HEIGHT: u32 = 520;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelMode {
    Mono,
    Stereo,
}

impl std::fmt::Display for ChannelMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChannelMode::Mono => write!(f, "Mono"),
            ChannelMode::Stereo => write!(f, "Stereo"),
        }
    }
}

impl From<u32> for ChannelMode {
    fn from(v: u32) -> Self {
        if v >= 2 {
            ChannelMode::Stereo
        } else {
            ChannelMode::Mono
        }
    }
}

impl From<ChannelMode> for u32 {
    fn from(mode: ChannelMode) -> Self {
        match mode {
            ChannelMode::Mono => 1,
            ChannelMode::Stereo => 2,
        }
    }
}

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
#[allow(clippy::enum_variant_names)]
pub enum Message {
    SetParam(ParamId, f32),
    SetMode(u8),
    SetVariant(u8),
    SetChannels(ChannelMode),
    ReleaseParam(ParamId),
    UiTick,
}

struct State {
    shared: Arc<SharedState>,
    active_gestures: Vec<bool>,
    waveform_samples: [f32; WAVEFORM_POINTS],
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    (
        State {
            shared,
            active_gestures: vec![false; ParamId::COUNT],
            waveform_samples: [0.0; WAVEFORM_POINTS],
        },
        next_ui_tick_task(),
    )
}

fn next_ui_tick_task() -> Task<Message> {
    Task::perform(
        async move {
            thread::sleep(Duration::from_millis(33));
        },
        |_| Message::UiTick,
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
        Message::SetMode(mode) => {
            state.shared.mark_gesture_begin_pending(ParamId::Mode);
            state
                .shared
                .set_param_outbound_only(ParamId::Mode, mode as f64);
            state.shared.mark_gesture_end_pending(ParamId::Mode);
        }
        Message::SetVariant(variant) => {
            state.shared.mark_gesture_begin_pending(ParamId::Variant);
            state
                .shared
                .set_param_outbound_only(ParamId::Variant, variant as f64);
            state.shared.mark_gesture_end_pending(ParamId::Variant);
        }
        Message::SetChannels(mode) => {
            state
                .shared
                .set_param_outbound_only(ParamId::Channels, u32::from(mode) as f64);
            state.shared.request_audio_ports_rescan();
        }
        Message::UiTick => {
            state.waveform_samples = state.shared.waveform_snapshot();
            return next_ui_tick_task();
        }
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;

    let waveform = ScrollingWaveformWidget::new(state.waveform_samples).view();

    let mut controls = column![].spacing(10).align_x(Alignment::Start);
    let channels = state.shared.params.get_enum(ParamId::Channels).clamp(1, 2);
    let channels_dropdown = maolan_baseview::iced::widget::pick_list(
        vec![ChannelMode::Mono, ChannelMode::Stereo],
        Some(ChannelMode::from(channels)),
        Message::SetChannels,
    )
    .placeholder("Channels");

    let variant = state.shared.params.get_enum(ParamId::Variant).min(1);
    controls = controls.push(
        row![
            channels_dropdown,
            text("Variant").size(16),
            maolan_baseview::iced::widget::radio(
                "Vintage",
                0u8,
                Some(variant as u8),
                Message::SetVariant
            ),
            maolan_baseview::iced::widget::radio(
                "Modern",
                1u8,
                Some(variant as u8),
                Message::SetVariant
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    );

    if variant == 0 {
        controls = controls.push(
            row![
                knob("Soften", ParamId::Soften, p(ParamId::Soften), "", 0.01),
                knob("Enhance", ParamId::Enhance, p(ParamId::Enhance), "", 0.01),
            ]
            .spacing(16),
        );
    } else {
        controls = controls.push(
            row![knob(
                "Ceiling",
                ParamId::Ceiling,
                p(ParamId::Ceiling),
                "dB",
                0.1
            )]
            .spacing(16),
        );
    }

    let mode = state.shared.params.get_enum(ParamId::Mode).min(7);
    controls = controls.push(
        row![
            text("Mode").size(16),
            maolan_baseview::iced::widget::radio("Normal", 0u8, Some(mode as u8), Message::SetMode),
            maolan_baseview::iced::widget::radio("Atten", 1u8, Some(mode as u8), Message::SetMode),
            maolan_baseview::iced::widget::radio("Clips", 2u8, Some(mode as u8), Message::SetMode),
            maolan_baseview::iced::widget::radio(
                "Afterbr",
                3u8,
                Some(mode as u8),
                Message::SetMode
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    controls = controls.push(
        row![
            maolan_baseview::iced::widget::radio(
                "Explode",
                4u8,
                Some(mode as u8),
                Message::SetMode
            ),
            maolan_baseview::iced::widget::radio("Nuke", 5u8, Some(mode as u8), Message::SetMode),
            maolan_baseview::iced::widget::radio(
                "Apocaly",
                6u8,
                Some(mode as u8),
                Message::SetMode
            ),
            maolan_baseview::iced::widget::radio(
                "Apothes",
                7u8,
                Some(mode as u8),
                Message::SetMode
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );

    let output_control: Element<'_, Message> = row![
        vertical_ticks(),
        gain_slider(ParamId::OutputGain, p(ParamId::OutputGain), "dB", 0.1),
    ]
    .spacing(8)
    .height(Length::Fill)
    .align_y(Alignment::Center)
    .into();

    let display_row = row![
        gain_slider(ParamId::Boost, p(ParamId::Boost), "dB", 0.1),
        vertical_ticks(),
        vu_meter(channels as usize, state.shared.input_levels_db()),
        waveform,
        vu_meter(channels as usize, state.shared.output_levels_db()),
        output_control,
    ]
    .spacing(8)
    .height(Length::Fill)
    .align_y(Alignment::Center);

    container(column![display_row, controls].spacing(14))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Left)
        .align_y(Vertical::Center)
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
    let value_text = match id {
        ParamId::Boost | ParamId::Ceiling | ParamId::OutputGain if units == "dB" => {
            format!("{value:.1} {units}")
        }
        _ => {
            if units.is_empty() {
                format!("{value:.2}")
            } else {
                format!("{value:.1} {units}")
            }
        }
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

fn gain_slider(
    id: ParamId,
    value: f32,
    units: &'static str,
    step: f32,
) -> Element<'static, Message> {
    let def = PARAMS[id.as_index()];
    let value_text = match id {
        ParamId::Boost | ParamId::Ceiling | ParamId::OutputGain if units == "dB" => {
            format!("{value:.1} {units}")
        }
        _ => {
            if units.is_empty() {
                format!("{value:.2}")
            } else {
                format!("{value:.1} {units}")
            }
        }
    };

    vertical_slider(
        VerticalSlider {
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
                title: String::from("Maolan Limiter"),
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
                        title: String::from("Maolan Limiter"),
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
