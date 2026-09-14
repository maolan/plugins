use std::{
    ffi::CStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
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
    widget::{Column, column, container, row, text},
};
use maolan_widgets::meters;
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd"
))]
use raw_window_handle::RawWindowHandle;
use raw_window_handle::{HandleError, HasWindowHandle, WindowHandle};

use crate::vumeter::plugin::SharedState;

pub const EDITOR_WIDTH: u32 = 500;
pub const EDITOR_HEIGHT: u32 = 250;

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

struct State {
    shared: Arc<SharedState>,
}

fn init(shared: Arc<SharedState>) -> (State, Task<()>) {
    (State { shared }, Task::none())
}

fn update(_state: &mut State, _message: ()) -> Task<()> {
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

fn side_column<'a>(title: &'a str, levels: [f32; 2], readouts: MeterReadouts) -> Column<'a, ()> {
    let meter_h = 100.0;
    column![
        text(title).size(13),
        container(meters::meters(2, &levels, meter_h))
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

fn view(state: &State) -> Element<'_, ()> {
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

    let input_col = side_column("Input", in_levels, in_readouts);
    let output_col = side_column("Output", out_levels, out_readouts);

    container(
        row![input_col, output_col]
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
        .subscription(|_state| maolan_baseview::iced::poll_events())
        .theme(theme)
        .run()
}

#[derive(Default)]
pub struct GuiBridge {
    created: bool,
    floating: bool,
    shared: Option<Arc<SharedState>>,
    floating_open: Arc<AtomicBool>,
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
        self.created = false;
        self.floating = false;
        self.shared = None;
    }

    pub fn set_parent(&mut self, shared: Arc<SharedState>, _parent: ParentWindowHandle) -> bool {
        if !self.created {
            return false;
        }
        if !self.floating {
            self.shared = Some(shared);
        }
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
            let notifier = maolan_baseview::iced::PollSubNotifier::new();
            *shared.poll_notifier.lock() = Some(notifier.clone());
            let open_flag = self.floating_open.clone();
            thread::spawn(move || {
                let settings = maolan_baseview::iced::IcedBaseviewSettings {
                    window: maolan_baseview::iced::baseview::WindowOpenOptions {
                        title: String::from("Maolan VU"),
                        size: maolan_baseview::iced::baseview::Size::new(
                            EDITOR_WIDTH as f64,
                            EDITOR_HEIGHT as f64,
                        ),
                        scale:
                            maolan_baseview::iced::baseview::WindowScalePolicy::SystemScaleFactor,
                    },
                    ignore_non_modifier_keys: false,
                    always_redraw: true,
                };
                maolan_baseview::iced::shell::open_blocking(settings, notifier, move || {
                    build_app(shared)
                });
                open_flag.store(false, Ordering::Release);
            });
        }
        true
    }

    pub fn hide(&mut self, _shared: Arc<SharedState>) -> bool {
        if self.floating {
            self.floating_open.store(false, Ordering::Release);
        }
        true
    }
}
