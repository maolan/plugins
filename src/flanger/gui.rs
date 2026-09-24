use std::{
    ffi::CStr,
    sync::{Arc, atomic::Ordering},
};

#[cfg(target_os = "macos")]
use maolan_clap::ffi::CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
use maolan_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use maolan_clap::ffi::CLAP_WINDOW_API_X11;

use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{column, container, pick_list, row, text, toggler},
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
    common::ui::{SmallKnob, VerticalSlider, small_knob, vertical_slider, vu_meter},
    flanger::{
        params::{FRACTION_NAMES, LFO_TYPE_NAMES, LFO2_TYPE_NAMES, PARAMS, PERIOD_NAMES, ParamId},
        plugin::SharedState,
    },
};

use maolan_widgets::multi_toggler::horizontal_multi_toggler;

pub const EDITOR_WIDTH: u32 = 1000;
pub const EDITOR_HEIGHT: u32 = 360;

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
    X11(u64),
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
enum Message {
    ParamChanged(ParamId, f32),
    ParamReleased(ParamId),
    SetBoolParam(ParamId, bool),
    SetCombo(ParamId, f64),
    Poll,
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
        Message::ParamChanged(id, value) => {
            let idx = id.as_index();
            if !state.active_gestures[idx] {
                state.active_gestures[idx] = true;
                state.shared.mark_gesture_begin_pending(id);
            }
            state.shared.set_param_outbound_only(id, value as f64);
        }
        Message::ParamReleased(id) => {
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
        Message::SetCombo(id, value) => {
            state.shared.set_param_outbound_only(id, value);
            state.shared.mark_dirty();
        }
        Message::Poll => {}
    }
    Task::none()
}

fn knob<'a>(id: ParamId, label: &'a str, state: &'a State) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let def = PARAMS[id.as_index()];
    let value_text = match id {
        ParamId::Rate => format!("{value:.2} Hz"),
        ParamId::Tempo => format!("{value:.0} BPM"),
        ParamId::Crossfade => format!("{value:.0}%"),
        ParamId::InitPhase | ParamId::PhaseDiff => format!("{value:.0}°"),
        ParamId::MinDepth | ParamId::Depth | ParamId::FeedbackDelay => {
            format!("{value:.2} ms")
        }
        ParamId::FeedbackGain => format!("{value:.2}"),
        ParamId::FeedbackDrive => format!("{value:.2}"),
        ParamId::InputGain | ParamId::OutputGain => format!("{value:.1} dB"),
        ParamId::DryWet => format!("{:.0}%", value * 100.0),
        _ => format!("{value:.2}"),
    };

    small_knob(
        SmallKnob {
            label: label.to_string(),
            value,
            range: def.min as f32..=def.max as f32,
            default: def.default as f32,
            step: if def.step > 0.0 {
                def.step as f32
            } else {
                0.01
            },
            value_text,
        },
        move |v| Message::ParamChanged(id, v),
        Message::ParamReleased(id),
    )
}

fn switch<'a>(id: ParamId, label: &'a str, state: &'a State) -> Element<'a, Message> {
    let value = state.shared.params.get(id) >= 0.5;
    row![
        text(label).size(13),
        toggler(value).on_toggle(move |v| Message::SetBoolParam(id, v)),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

fn combo<'a>(
    id: ParamId,
    label: &'a str,
    options: &'a [&'a str],
    state: &'a State,
) -> Element<'a, Message> {
    let def = PARAMS[id.as_index()];
    let index = (state.shared.params.get(id).round() as usize).min(options.len() - 1);
    let selected = options[index].to_string();
    row![
        text(label).size(13),
        pick_list(
            options.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            Some(selected),
            move |name| {
                let idx = options
                    .iter()
                    .position(|&o| o == name.as_str())
                    .unwrap_or(0);
                Message::SetCombo(id, (def.min + idx as f64).clamp(def.min, def.max))
            },
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

fn period_toggler<'a>(id: ParamId, state: &'a State) -> Element<'a, Message> {
    let def = PARAMS[id.as_index()];
    let selected = (state.shared.params.get(id).round() as usize).min(PERIOD_NAMES.len() - 1);
    horizontal_multi_toggler(PERIOD_NAMES, selected, move |index| {
        Message::SetCombo(id, (def.min + index as f64).clamp(def.min, def.max))
    })
    .into()
}

fn view(state: &State) -> Element<'_, Message> {
    let time_mode_tempo = state.shared.params.get(ParamId::TimeMode) >= 0.5;
    let rate_knob = if time_mode_tempo {
        knob(ParamId::Tempo, "Tempo", state)
    } else {
        knob(ParamId::Rate, "Rate", state)
    };

    let lfo_row = row![
        combo(ParamId::LfoType, "LFO", &LFO_TYPE_NAMES, state),
        period_toggler(ParamId::LfoPeriod, state),
        combo(ParamId::Lfo2Type, "LFO2", &LFO2_TYPE_NAMES, state),
        period_toggler(ParamId::Lfo2Period, state),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let content = column![
        row![
            knob(ParamId::MinDepth, "Min Depth", state),
            knob(ParamId::Depth, "Depth", state),
            rate_knob,
            combo(ParamId::Fraction, "Fraction", &FRACTION_NAMES, state),
            knob(ParamId::Crossfade, "Crossfade", state),
            combo(
                ParamId::CrossfadeType,
                "XF Type",
                &["Const power", "Linear"],
                state
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        lfo_row,
        row![
            knob(ParamId::InitPhase, "Init Phase", state),
            knob(ParamId::PhaseDiff, "Phase Diff", state),
            knob(ParamId::FeedbackGain, "FB Gain", state),
            knob(ParamId::FeedbackDrive, "FB Drive", state),
            knob(ParamId::FeedbackDelay, "FB Delay", state),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        row![
            column![
                switch(ParamId::Stereo, "Stereo", state),
                switch(ParamId::MidSide, "Mid/Side", state),
                switch(ParamId::SignalPhase, "Signal Phase", state),
            ]
            .spacing(6),
            column![
                switch(ParamId::TimeMode, "Tempo Mode", state),
                switch(ParamId::TempoSync, "Tempo Sync", state),
                switch(ParamId::ResetPhase, "Reset Phase", state),
            ]
            .spacing(6),
            column![
                switch(ParamId::FeedbackOn, "Feedback", state),
                switch(ParamId::FeedbackPhase, "FB Phase", state),
            ]
            .spacing(6),
            column![knob(ParamId::DryWet, "Dry/Wet", state),],
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    ]
    .spacing(14)
    .align_x(Alignment::Start);

    let meter_channels = if state.shared.channels.load(Ordering::Acquire) >= 2 {
        2
    } else {
        1
    };

    container(
        row![
            gain_slider(ParamId::InputGain, "Input", state),
            vu_meter(meter_channels, state.shared.input_levels_db()),
            content,
            vu_meter(meter_channels, state.shared.output_levels_db()),
            gain_slider(ParamId::OutputGain, "Output", state),
        ]
        .spacing(12)
        .height(Length::Fill)
        .align_y(Alignment::Center),
    )
    .padding(16)
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Horizontal::Left)
    .align_y(Vertical::Top)
    .into()
}

fn gain_slider<'a>(id: ParamId, label: &'a str, state: &'a State) -> Element<'a, Message> {
    let value = state.shared.params.get(id) as f32;
    let def = PARAMS[id.as_index()];
    let value_text = format!("{value:.1} dB");

    column![
        text(label).size(11),
        vertical_slider(
            VerticalSlider {
                value,
                range: def.min as f32..=def.max as f32,
                default: def.default as f32,
                step: def.step as f32,
                value_text,
            },
            move |v| Message::ParamChanged(id, v),
            Message::ParamReleased(id),
        ),
    ]
    .spacing(4)
    .height(Length::Fill)
    .align_x(Alignment::Center)
    .into()
}

fn theme(_state: &State) -> Theme {
    Theme::TokyoNight
}

fn build_app(shared: Arc<SharedState>) -> impl maolan_baseview::iced::Program {
    maolan_baseview::iced::application(move || init(shared.clone()), update, view)
        .subscription(|_state| maolan_baseview::iced::poll_events().map(|_| Message::Poll))
        .theme(theme)
        .run()
}

pub struct App;

impl crate::common::gui_bridge::GuiApp<super::plugin::SharedState> for App {
    type Parent = ParentWindowHandle;

    fn title() -> String {
        String::from("Maolan Flanger")
    }

    fn size() -> (u32, u32) {
        (EDITOR_WIDTH, EDITOR_HEIGHT)
    }

    fn always_redraw() -> bool {
        true
    }

    fn notify_closed_on_hide() -> bool {
        false
    }

    fn opens_parented_window() -> bool {
        false
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
        crate::flanger::gui::parent_from_window(window, api)
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
            Some(ParentWindowHandle::X11(unsafe { window.clap_window__.x11 }))
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
