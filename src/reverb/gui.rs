use std::{ffi::CStr, sync::Arc};

use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{column, container, pick_list, row},
};
#[cfg(target_os = "macos")]
use maolan_clap::ffi::CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
use maolan_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use maolan_clap::ffi::CLAP_WINDOW_API_X11;
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd"
))]
use raw_window_handle::RawWindowHandle;
use raw_window_handle::{HandleError, HasWindowHandle, WindowHandle};

use crate::{
    common::{
        preset::{self, PresetProvider, PresetUi},
        ui::{SmallKnob, panel, small_knob},
    },
    reverb::{
        params::{MODES, PARAMS, ParamId, value_text},
        plugin::SharedState,
    },
};

use maolan_widgets::multi_toggler::{LabelPosition, horizontal_multi_toggler};

pub const EDITOR_WIDTH: u32 = 900;
pub const EDITOR_HEIGHT: u32 = 570;

pub fn preferred_api() -> &'static CStr {
    #[cfg(target_os = "windows")]
    {
        CLAP_WINDOW_API_WIN32
    }
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        CLAP_WINDOW_API_X11
    }
    #[cfg(target_os = "macos")]
    {
        CLAP_WINDOW_API_COCOA
    }
}

pub fn is_api_supported(api: &CStr, _is_floating: bool) -> bool {
    api == preferred_api()
}

pub enum ParentWindowHandle {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    X11(usize),
    #[cfg(target_os = "macos")]
    Cocoa(*mut std::ffi::c_void),
    #[cfg(target_os = "windows")]
    Win32(*mut std::ffi::c_void),
}

impl HasWindowHandle for ParentWindowHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        match self {
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
            ParentWindowHandle::X11(window) => {
                let handle = raw_window_handle::XlibWindowHandle::new(*window as _);
                Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Xlib(handle)) })
            }
            #[cfg(target_os = "windows")]
            ParentWindowHandle::Win32(hwnd) => {
                let handle = raw_window_handle::Win32WindowHandle::new(
                    std::num::NonZeroIsize::new(*hwnd as isize).unwrap(),
                );
                Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
            }
            #[cfg(target_os = "macos")]
            ParentWindowHandle::Cocoa(ns_view) => {
                let handle = raw_window_handle::AppKitWindowHandle::new(
                    std::ptr::NonNull::new(*ns_view).expect("null NSView"),
                );
                Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::AppKit(handle)) })
            }
        }
    }
}

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

#[derive(Debug, Clone)]
pub enum Message {
    SetParam(ParamId, f32),
    ReleaseParam(ParamId),
    SetChannels(ChannelMode),
    Preset(preset::Msg),
    Select(ParamId, usize),
}

impl From<preset::Msg> for Message {
    fn from(msg: preset::Msg) -> Self {
        Message::Preset(msg)
    }
}

struct State {
    shared: Arc<SharedState>,
    active_gestures: Vec<bool>,
    preset_ui: PresetUi,
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    (
        State {
            shared,
            active_gestures: vec![false; ParamId::COUNT],
            preset_ui: PresetUi::new(SharedState::DIR_NAME),
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
        Message::SetChannels(mode) => {
            state
                .shared
                .set_param_outbound_only(ParamId::Channels, u32::from(mode) as f64);
            state.shared.sync_channels_from_params();
            state.shared.request_audio_ports_rescan();
        }
        Message::Select(id, index) => {
            state.shared.mark_gesture_begin_pending(id);
            state.shared.set_param_outbound_only(id, index as f64);
            state.shared.mark_gesture_end_pending(id);
        }
        Message::Preset(msg) => {
            preset::update(&mut state.preset_ui, msg, state.shared.as_ref());
        }
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    fn knob<'a>(id: ParamId, label: &'a str, state: &'a State) -> Element<'a, Message> {
        let value = state.shared.params.get(id) as f32;
        let def = &PARAMS[id.as_index()];
        let value_text = value_text(id, value as f64);

        let logarithmic = matches!(
            id,
            ParamId::Decay
                | ParamId::BassMult
                | ParamId::BassFreq
                | ParamId::HighFreq
                | ParamId::HighCut
        );
        let to_normalized = |v: f32| {
            if logarithmic {
                (v as f64 / def.min).ln() as f32 / (def.max / def.min).ln() as f32
            } else {
                v
            }
        };
        let (min, max) = (def.min, def.max);
        small_knob(
            SmallKnob {
                label: label.to_string(),
                value: to_normalized(value),
                range: if logarithmic {
                    0.0..=1.0
                } else {
                    min as f32..=max as f32
                },
                default: to_normalized(def.default as f32),
                step: if logarithmic { 0.001 } else { def.step as f32 },
                value_text,
            },
            move |v| {
                Message::SetParam(
                    id,
                    if logarithmic {
                        (min * (max / min).powf(v as f64)) as f32
                    } else {
                        v
                    },
                )
            },
            Message::ReleaseParam(id),
        )
    }

    let channels = state.shared.params.get(ParamId::Channels).round() as u32;
    let channels_selected = if channels >= 2 { 1 } else { 0 };
    let channels_toggle =
        horizontal_multi_toggler(["Mono", "Stereo"], channels_selected, |index| {
            Message::SetChannels(if index >= 1 {
                ChannelMode::Stereo
            } else {
                ChannelMode::Mono
            })
        })
        .label_position(LabelPosition::Below);

    let mode = state.shared.params.get(ParamId::Mode) as usize;
    let mode_picker = pick_list(
        MODES.to_vec(),
        Some(MODES[mode.min(MODES.len() - 1)]),
        |name| {
            Message::Select(
                ParamId::Mode,
                MODES.iter().position(|&s| s == name).unwrap_or(0),
            )
        },
    );
    let group = |title: &'static str, ids: &[ParamId]| -> Element<'_, Message> {
        let knobs = row(ids
            .iter()
            .map(|&id| knob(id, PARAMS[id.as_index()].name, state)))
        .spacing(10)
        .align_y(Alignment::Center);
        panel(title, knobs.into())
    };
    let controls: Element<'_, Message> = column![
        row![
            group("Reverb", &[ParamId::PreDelay, ParamId::Decay]),
            group("Shape", &[ParamId::Size, ParamId::Attack]),
            group("Diffusion", &[ParamId::Early, ParamId::Late]),
        ]
        .spacing(12),
        row![
            group(
                "Damping",
                &[
                    ParamId::BassMult,
                    ParamId::BassFreq,
                    ParamId::HighShelf,
                    ParamId::HighFreq
                ]
            ),
            group("Modulation", &[ParamId::ModRate, ParamId::ModDepth]),
            group("EQ", &[ParamId::LowCut, ParamId::HighCut]),
        ]
        .spacing(12),
    ]
    .spacing(14)
    .into();
    let content = column![
        preset::bar(&state.preset_ui),
        row![
            mode_picker,
            knob(ParamId::DryWet, "Mix", state),
            channels_toggle
        ]
        .spacing(12)
        .align_y(Alignment::Center),
        controls,
    ]
    .spacing(14)
    .align_x(Alignment::Start);

    container(content)
        .padding(24)
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
        .font(maolan_widgets::iced_fonts::LUCIDE_FONT_BYTES)
        .theme(theme)
        .run()
}

pub struct App;

impl crate::common::gui_bridge::GuiApp<super::plugin::SharedState> for App {
    type Parent = ParentWindowHandle;

    fn title() -> String {
        String::from("Maolan Reverb")
    }

    fn size() -> (u32, u32) {
        (EDITOR_WIDTH, EDITOR_HEIGHT)
    }

    fn always_redraw() -> bool {
        false
    }

    fn notify_closed_on_hide() -> bool {
        true
    }

    fn opens_parented_window() -> bool {
        true
    }

    fn preferred_api() -> &'static CStr {
        preferred_api()
    }

    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "freebsd"
    ))]
    fn parent_from_window(
        window: &maolan_clap::ffi::clap_window,
        api: &CStr,
    ) -> Option<Self::Parent> {
        crate::reverb::gui::parent_from_window(window, api)
    }

    fn build_app(
        shared: std::sync::Arc<super::plugin::SharedState>,
    ) -> impl maolan_baseview::iced::Program {
        build_app(shared)
    }
}

pub type GuiBridge = crate::common::gui_bridge::GuiBridge<super::plugin::SharedState, App>;
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd"
))]
pub fn parent_from_window(
    window: &maolan_clap::ffi::clap_window,
    api: &CStr,
) -> Option<ParentWindowHandle> {
    use maolan_clap::ffi::{CLAP_WINDOW_API_COCOA, CLAP_WINDOW_API_WIN32, CLAP_WINDOW_API_X11};
    if api == CLAP_WINDOW_API_X11 {
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        {
            Some(ParentWindowHandle::X11(unsafe {
                window.clap_window__.x11 as usize
            }))
        }
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        {
            None
        }
    } else if api == CLAP_WINDOW_API_WIN32 {
        #[cfg(target_os = "windows")]
        {
            Some(ParentWindowHandle::Win32(unsafe {
                window.clap_window__.win32
            }))
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    } else if api == CLAP_WINDOW_API_COCOA {
        #[cfg(target_os = "macos")]
        {
            Some(ParentWindowHandle::Cocoa(unsafe {
                window.clap_window__.cocoa
            }))
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    } else {
        None
    }
}
