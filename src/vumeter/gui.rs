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
    widget::{column, container, row, text},
};
use maolan_widgets::meters;

use crate::vumeter::plugin::SharedState;

pub const EDITOR_WIDTH: u32 = 280;
pub const EDITOR_HEIGHT: u32 = 180;

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

// ─── Iced app ───

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
        format!("{:+.1}", v)
    }
}

fn view(state: &State) -> Element<'_, ()> {
    let in_l = f64::from_bits(state.shared.in_l_rms.load(Ordering::Relaxed));
    let in_r = f64::from_bits(state.shared.in_r_rms.load(Ordering::Relaxed));
    let out_l = f64::from_bits(state.shared.out_l_rms.load(Ordering::Relaxed));
    let out_r = f64::from_bits(state.shared.out_r_rms.load(Ordering::Relaxed));

    let in_levels = [db(in_l), db(in_r)];
    let out_levels = [db(out_l), db(out_r)];
    let meter_h = 100.0;

    let input_col = column![
        text("Input").size(13),
        container(meters::meters(2, &in_levels, meter_h))
            .height(Length::Fixed(meter_h))
            .width(Length::Shrink),
        row![
            text(format!("L {}", db_str(in_l))).size(11),
            text(format!("R {}", db_str(in_r))).size(11),
        ]
        .spacing(8),
    ]
    .spacing(6)
    .align_x(Alignment::Center);

    let output_col = column![
        text("Output").size(13),
        container(meters::meters(2, &out_levels, meter_h))
            .height(Length::Fixed(meter_h))
            .width(Length::Shrink),
        row![
            text(format!("L {}", db_str(out_l))).size(11),
            text(format!("R {}", db_str(out_r))).size(11),
        ]
        .spacing(8),
    ]
    .spacing(6)
    .align_x(Alignment::Center);

    let content = column![
        text("Maolan VU Meter").size(16),
        row![input_col, output_col]
            .spacing(24)
            .align_y(Alignment::Center),
    ]
    .spacing(12)
    .align_x(Alignment::Center);

    container(content)
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

// ─── GuiBridge ───

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
                        title: String::from("Maolan VU Meter"),
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
