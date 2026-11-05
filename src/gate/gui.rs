use std::{ffi::CStr, sync::Arc, thread, time::Duration};

use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{column, container, row, text},
};
#[cfg(target_os = "macos")]
use maolan_clap::ffi::CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
use maolan_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use maolan_clap::ffi::CLAP_WINDOW_API_X11;
use maolan_widgets::{
    knob::Knob,
    meters,
    multi_toggler::{LabelPosition, horizontal_multi_toggler},
};
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd"
))]
use raw_window_handle::RawWindowHandle;
use raw_window_handle::{HandleError, HasWindowHandle, WindowHandle};

use crate::{
    common::preset::{self, PresetProvider, PresetUi},
    gate::{
        dsp::GateStatus,
        params::{PARAMS, ParamId},
        plugin::{SharedState, param_text},
    },
};

pub const EDITOR_WIDTH: u32 = 720;
pub const EDITOR_HEIGHT: u32 = 390;

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

#[derive(Debug, Clone)]
pub enum Message {
    SetParam(ParamId, f32),
    ReleaseParam(ParamId),
    SetBoolParam(ParamId, bool),
    UiTick,
    Preset(preset::Msg),
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
        Message::SetBoolParam(id, value) => {
            state.shared.mark_gesture_begin_pending(id);
            state
                .shared
                .set_param_outbound_only(id, if value { 1.0 } else { 0.0 });
            state.shared.mark_gesture_end_pending(id);
            state.shared.mark_dirty();
        }
        Message::UiTick => return next_ui_tick_task(),
        Message::Preset(msg) => {
            preset::update(&mut state.preset_ui, msg, state.shared.as_ref());
        }
    }
    Task::none()
}

fn knob<'a>(id: ParamId, label: &'a str, state: &'a State) -> Element<'a, Message> {
    let value = state.shared.params.get(id);
    let def = &PARAMS[id.as_index()];
    maolan_widgets::knob::knob(
        Knob {
            label: label.to_string(),
            value: value as f32,
            range: def.min as f32..=def.max as f32,
            default: def.default as f32,
            step: def.step as f32,
            value_text: param_text(id, value),
            assigned: false,
        },
        move |value| Message::SetParam(id, value),
        Message::ReleaseParam(id),
    )
}

fn meter_column<'a>(title: &'a str, level_db: f32, readout: String) -> Element<'a, Message> {
    let levels = [level_db.clamp(-90.0, 20.0)];
    column![
        text(title).size(13),
        container(meters::meters(1, &levels, 110.0))
            .width(Length::Shrink)
            .height(Length::Fixed(110.0)),
        text(readout).size(11),
    ]
    .spacing(5)
    .align_x(Alignment::Center)
    .into()
}

fn view(state: &State) -> Element<'_, Message> {
    let external = state.shared.params.get_bool(ParamId::ExternalSidechain);
    let monitor = state.shared.params.get_bool(ParamId::Monitor);
    let (detector_db, gain_reduction_db, status) = state.shared.meter();
    let status_text = match status {
        GateStatus::Closed => "Closed",
        GateStatus::Holding => "Holding",
        GateStatus::Open => "Open",
    };
    let detector_text = if detector_db <= -200.0 {
        "-inf dB".to_string()
    } else {
        format!("{detector_db:.1} dB")
    };

    let detector_controls = row![
        column![
            text("External SC").size(13),
            horizontal_multi_toggler(["Internal", "External"], usize::from(external), |index| {
                Message::SetBoolParam(ParamId::ExternalSidechain, index == 1)
            },)
            .label_position(LabelPosition::Below),
        ]
        .spacing(5)
        .align_x(Alignment::Center),
        column![
            text("Monitor").size(13),
            horizontal_multi_toggler(["Off", "Monitor"], usize::from(monitor), |index| {
                Message::SetBoolParam(ParamId::Monitor, index == 1)
            },)
            .label_position(LabelPosition::Below),
        ]
        .spacing(5)
        .align_x(Alignment::Center),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let controls = column![
        row![
            knob(ParamId::Threshold, "Threshold", state),
            knob(ParamId::Ratio, "Ratio", state),
            knob(ParamId::Range, "Range", state),
            knob(ParamId::Hysteresis, "Hysteresis", state),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        row![
            knob(ParamId::Attack, "Attack", state),
            knob(ParamId::Hold, "Hold", state),
            knob(ParamId::Release, "Release", state),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        row![
            knob(ParamId::SidechainHpf, "SC HPF", state),
            knob(ParamId::SidechainLpf, "SC LPF", state),
            detector_controls,
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    ]
    .spacing(12);

    let metering = row![
        meter_column("Detector", detector_db, detector_text),
        meter_column(
            "Reduction",
            -gain_reduction_db,
            format!("{gain_reduction_db:.1} dB"),
        ),
        column![
            text("State").size(13),
            text(status_text).size(16),
            text(if monitor {
                "Monitoring key"
            } else {
                "Main output"
            })
            .size(11),
        ]
        .spacing(8)
        .align_x(Alignment::Center),
    ]
    .spacing(20)
    .align_y(Alignment::Center);

    let content = column![
        preset::bar(&state.preset_ui),
        row![controls, metering]
            .spacing(24)
            .align_y(Alignment::Center),
    ]
    .spacing(16)
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
        String::from("Maolan Gate")
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
        crate::gate::gui::parent_from_window(window, api)
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
