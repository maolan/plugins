//! Generic GUI bridge shared by the Maolan plugin editors.
//!
//! Each plugin's `gui.rs` provides only its iced application (message type,
//! `update`/`view`, widgets) through the [`GuiApp`] trait; this module owns
//! the duplicated scaffolding: window open/close via baseview (parented or
//! floating), the `GuiHooks` integration with the CLAP harness, and the
//! GUI-closed notification protocol.

use std::ffi::CStr;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use raw_window_handle::HasWindowHandle;

use crate::common::clap_harness::{GuiHooks, SharedBase};

struct AnyWindowHandle {
    _inner: Box<dyn std::any::Any>,
}

unsafe impl Send for AnyWindowHandle {}

/// Per-plugin editor application: the only part of a GUI that is genuinely
/// plugin-specific.
pub trait GuiApp<S>: Sized + 'static {
    /// Parent window handle type (X11/Win32/Cocoa).
    type Parent: HasWindowHandle;

    /// Window title.
    fn title() -> String;

    /// Fixed editor size.
    fn size() -> (u32, u32);

    /// Whether the iced shell should redraw continuously.
    fn always_redraw() -> bool {
        false
    }

    /// Whether `hide` asks the host to close the editor window.
    fn notify_closed_on_hide() -> bool {
        false
    }

    /// Whether `set_parent` opens the editor into the host window. Plugins
    /// that only record the shared state here keep the historical behavior.
    fn opens_parented_window() -> bool {
        true
    }

    /// API acceptance; the default accepts only the preferred API.
    fn is_api_supported(api: &CStr, _is_floating: bool) -> bool {
        api == Self::preferred_api()
    }

    /// Preferred windowing API.
    fn preferred_api() -> &'static CStr;

    /// Extracts the platform parent handle from a CLAP window, or `None` if
    /// the API/window combination is unsupported on this platform.
    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "freebsd"
    ))]
    fn parent_from_window(
        window: &maolan_clap::ffi::clap_window,
        api: &CStr,
    ) -> Option<Self::Parent>;

    /// Builds the iced application for a fresh editor window.
    fn build_app(shared: Arc<S>) -> impl maolan_baseview::iced::Program;
}

/// Window-management half of a plugin editor; pairs with a [`GuiApp`]
/// implementation to form a complete `GuiHooks` implementation.
pub struct GuiBridge<S, A: GuiApp<S>> {
    created: bool,
    floating: bool,
    shared: Option<Arc<S>>,
    floating_open: Arc<AtomicBool>,
    window_handle: Option<AnyWindowHandle>,
    _marker: PhantomData<fn() -> (S, A)>,
}

impl<S, A: GuiApp<S>> Default for GuiBridge<S, A> {
    fn default() -> Self {
        Self {
            created: false,
            floating: false,
            shared: None,
            floating_open: Arc::new(AtomicBool::new(false)),
            window_handle: None,
            _marker: PhantomData,
        }
    }
}

impl<S: SharedBase + Send + Sync, A: GuiApp<S>> GuiBridge<S, A> {
    pub fn create(&mut self, shared: Arc<S>, api: &CStr, is_floating: bool) -> bool {
        if !A::is_api_supported(api, is_floating) {
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
        self.window_handle = None;
    }

    pub fn set_parent(&mut self, shared: Arc<S>, parent: A::Parent) -> bool {
        if !self.created {
            return false;
        }
        if self.floating || !A::opens_parented_window() {
            self.shared = Some(shared);
            return true;
        }

        let (width, height) = A::size();
        let settings = maolan_baseview::iced::IcedBaseviewSettings {
            window: maolan_baseview::iced::baseview::WindowOpenOptions {
                title: A::title(),
                size: maolan_baseview::iced::baseview::Size::new(width as f64, height as f64),
                scale: maolan_baseview::iced::baseview::WindowScalePolicy::SystemScaleFactor,
            },
            ignore_non_modifier_keys: false,
            always_redraw: A::always_redraw(),
        };

        let handle = maolan_baseview::iced::shell::open_parented(
            &parent,
            settings,
            maolan_baseview::iced::PollSubNotifier::new(),
            move || A::build_app(shared),
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
            let notifier = maolan_baseview::iced::PollSubNotifier::new();
            shared.set_poll_notifier(notifier.clone());
            let open_flag = Arc::clone(&self.floating_open);
            let (width, height) = A::size();
            thread::spawn(move || {
                let settings = maolan_baseview::iced::IcedBaseviewSettings {
                    window: maolan_baseview::iced::baseview::WindowOpenOptions {
                        title: A::title(),
                        size: maolan_baseview::iced::baseview::Size::new(
                            width as f64,
                            height as f64,
                        ),
                        scale:
                            maolan_baseview::iced::baseview::WindowScalePolicy::SystemScaleFactor,
                    },
                    ignore_non_modifier_keys: false,
                    always_redraw: A::always_redraw(),
                };
                maolan_baseview::iced::shell::open_blocking(settings, notifier, move || {
                    A::build_app(shared)
                });
                open_flag.store(false, Ordering::Release);
            });
        }
        true
    }

    pub fn hide(&mut self, shared: Arc<S>) -> bool {
        if A::notify_closed_on_hide() {
            shared.notify_gui_closed();
        }
        if self.floating {
            self.floating_open.store(false, Ordering::Release);
        }
        true
    }
}

impl<S: SharedBase + Send + Sync, A: GuiApp<S>> GuiHooks<S> for GuiBridge<S, A> {
    type Parent = A::Parent;

    fn is_api_supported(api: &CStr, is_floating: bool) -> bool {
        A::is_api_supported(api, is_floating)
    }

    fn preferred_api() -> &'static CStr {
        A::preferred_api()
    }

    fn editor_size() -> (u32, u32) {
        A::size()
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
        A::parent_from_window(window, api)
    }

    fn create(&mut self, shared: Arc<S>, api: &CStr, is_floating: bool) -> bool {
        GuiBridge::create(self, shared, api, is_floating)
    }

    fn destroy(&mut self) {
        GuiBridge::destroy(self)
    }

    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "freebsd"
    ))]
    fn set_parent(&mut self, shared: Arc<S>, parent: Self::Parent) -> bool {
        GuiBridge::set_parent(self, shared, parent)
    }

    fn show(&mut self) -> bool {
        GuiBridge::show(self)
    }

    fn hide(&mut self, shared: Arc<S>) -> bool {
        GuiBridge::hide(self, shared)
    }
}
