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
    widget::{Column, Space, column, container, row, text},
};
use maolan_widgets::{
    meters,
    multi_toggler::horizontal_multi_toggler,
    pan_slider::{ReadoutPosition, pan_slider},
    vertical_slider::{VerticalSlider, vertical_slider},
};
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd"
))]
use raw_window_handle::RawWindowHandle;
use raw_window_handle::{HandleError, HasWindowHandle, WindowHandle};

use crate::volume::params::{PARAMS, ParamId};
use crate::volume::plugin::SharedState;

pub const EDITOR_WIDTH: u32 = 560;
pub const EDITOR_HEIGHT: u32 = 310;

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
    SetVolume(f32),
    ReleaseVolume,
    SetPan(f32),
    ReleasePan,
    SetChannels(usize),
    Poll,
}

struct State {
    shared: Arc<SharedState>,
    volume_gesture_active: bool,
    pan_gesture_active: bool,
}

fn init(shared: Arc<SharedState>) -> (State, Task<Message>) {
    (
        State {
            shared,
            volume_gesture_active: false,
            pan_gesture_active: false,
        },
        Task::none(),
    )
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::SetVolume(value) => {
            if !state.volume_gesture_active {
                state.volume_gesture_active = true;
                state.shared.mark_gesture_begin_pending(ParamId::Volume);
            }
            state
                .shared
                .set_param_outbound_only(ParamId::Volume, value as f64);
        }
        Message::ReleaseVolume => {
            if state.volume_gesture_active {
                state.volume_gesture_active = false;
                state.shared.mark_gesture_end_pending(ParamId::Volume);
            }
        }
        Message::SetPan(value) => {
            if !state.pan_gesture_active {
                state.pan_gesture_active = true;
                state.shared.mark_gesture_begin_pending(ParamId::Pan);
            }
            state
                .shared
                .set_param_outbound_only(ParamId::Pan, value as f64);
        }
        Message::ReleasePan => {
            if state.pan_gesture_active {
                state.pan_gesture_active = false;
                state.shared.mark_gesture_end_pending(ParamId::Pan);
            }
        }
        Message::SetChannels(index) => {
            state.shared.mark_gesture_begin_pending(ParamId::Channels);
            state
                .shared
                .set_param_outbound_only(ParamId::Channels, (index + 1) as f64);
            state.shared.mark_gesture_end_pending(ParamId::Channels);
        }
        Message::Poll => {}
    }
    Task::none()
}

fn db(level: f64) -> f32 {
    if level <= 0.000_000_1 {
        -90.0
    } else {
        (20.0 * level.log10()).clamp(-90.0, 20.0) as f32
    }
}

fn db_str(level: f64) -> String {
    let v = db(level);
    if v <= -90.0 {
        String::from("-inf")
    } else {
        format!("{v:+06.2}")
    }
}

fn lufs_str(value: f64) -> String {
    if !value.is_finite() || value <= -120.0 {
        String::from("-inf")
    } else {
        format!("{value:+06.2}")
    }
}

struct MeterReadouts {
    rms_l: f64,
    rms_r: f64,
    peak_l: f64,
    peak_r: f64,
    lufs_momentary: f64,
    lufs_short_term: f64,
    lufs_integrated: f64,
}

fn side_column<'a>(
    title: &'a str,
    channels: usize,
    levels: [f32; 2],
    readouts: MeterReadouts,
) -> Column<'a, Message> {
    let meter_h = 100.0;
    column![
        text(title).size(13),
        container(meters::meters(channels, &levels, meter_h))
            .height(Length::Fixed(meter_h))
            .width(Length::Shrink),
        row![
            text("Pk").size(11),
            text(format!("L {}", db_str(readouts.peak_l))).size(11),
            text(format!("R {}", db_str(readouts.peak_r))).size(11),
        ]
        .spacing(8),
        row![
            text("RMS").size(11),
            text(format!("L {}", db_str(readouts.rms_l))).size(11),
            text(format!("R {}", db_str(readouts.rms_r))).size(11),
        ]
        .spacing(8),
        row![
            text("LUFS").size(11),
            text(format!("M {}", lufs_str(readouts.lufs_momentary))).size(11),
            text(format!("S {}", lufs_str(readouts.lufs_short_term))).size(11),
            text(format!("I {}", lufs_str(readouts.lufs_integrated))).size(11),
        ]
        .spacing(6),
    ]
    .spacing(6)
    .align_x(Alignment::Center)
}

fn view(state: &State) -> Element<'_, Message> {
    let in_readouts = MeterReadouts {
        rms_l: state.shared.in_l_rms.load(Ordering::Relaxed),
        rms_r: state.shared.in_r_rms.load(Ordering::Relaxed),
        peak_l: state.shared.in_l_peak.load(Ordering::Relaxed),
        peak_r: state.shared.in_r_peak.load(Ordering::Relaxed),
        lufs_momentary: state.shared.in_lufs_momentary.load(Ordering::Relaxed),
        lufs_short_term: state.shared.in_lufs_short_term.load(Ordering::Relaxed),
        lufs_integrated: state.shared.in_lufs_integrated.load(Ordering::Relaxed),
    };
    let out_readouts = MeterReadouts {
        rms_l: state.shared.out_l_rms.load(Ordering::Relaxed),
        rms_r: state.shared.out_r_rms.load(Ordering::Relaxed),
        peak_l: state.shared.out_l_peak.load(Ordering::Relaxed),
        peak_r: state.shared.out_r_peak.load(Ordering::Relaxed),
        lufs_momentary: state.shared.out_lufs_momentary.load(Ordering::Relaxed),
        lufs_short_term: state.shared.out_lufs_short_term.load(Ordering::Relaxed),
        lufs_integrated: state.shared.out_lufs_integrated.load(Ordering::Relaxed),
    };

    let in_levels = [db(in_readouts.rms_l), db(in_readouts.rms_r)];
    let out_levels = [db(out_readouts.rms_l), db(out_readouts.rms_r)];

    let channels = state.shared.channels() as usize;
    let input_col = side_column("Input", channels, in_levels, in_readouts);
    let output_col = side_column("Output", channels, out_levels, out_readouts);
    let volume = state.shared.params.get(ParamId::Volume) as f32;
    let volume_text = if volume <= PARAMS[0].min as f32 {
        "-inf dB".to_string()
    } else {
        format!("{volume:.1} dB")
    };
    let pan = state.shared.params.get(ParamId::Pan) as f32;
    let pan_control: Element<'_, Message> = if channels == 2 {
        pan_slider(
            pan,
            Message::SetPan,
            Message::ReleasePan,
            Some(ReadoutPosition::Top),
        )
    } else {
        Space::new().height(Length::Fixed(12.0)).into()
    };
    let volume_control = column![
        pan_control,
        vertical_slider(
            VerticalSlider {
                value: volume,
                range: PARAMS[0].min as f32..=PARAMS[0].max as f32,
                default: PARAMS[0].default as f32,
                step: PARAMS[0].step as f32,
                value_text: volume_text,
            },
            Message::SetVolume,
            Message::ReleaseVolume,
        ),
        horizontal_multi_toggler(
            ["Mono", "Stereo"],
            channels.saturating_sub(1),
            Message::SetChannels,
        )
    ]
    .spacing(6)
    .align_x(Horizontal::Center)
    .width(Length::Fixed(140.0))
    .height(Length::Fixed(210.0));

    container(
        row![input_col, volume_control, output_col]
            .spacing(24)
            .align_y(Alignment::Center),
    )
    .padding(16)
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Horizontal::Center)
    .align_y(Vertical::Center)
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
        String::from("Maolan Volume")
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
        crate::volume::gui::parent_from_window(window, api)
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
