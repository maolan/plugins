//! GUI bridge for Maolan Synth.
//!
//! Stub implementation — GUI can be added later using the same
//! baseview/iced pattern as other Maolan plugins.

use std::{ffi::CStr, sync::Arc};

use crate::synth::plugin::SharedState;

pub const EDITOR_WIDTH: u32 = 1200;
pub const EDITOR_HEIGHT: u32 = 700;

#[derive(Debug, Default)]
pub struct GuiBridge {
    // Placeholder for future GUI implementation
}

impl GuiBridge {
    pub fn create(
        &mut self,
        _shared: Arc<SharedState>,
        _api: &CStr,
        _is_floating: bool,
    ) -> bool {
        false
    }

    pub fn destroy(&mut self) {}

    pub fn show(&mut self) -> bool {
        false
    }

    pub fn hide(&mut self, _shared: Arc<SharedState>) -> bool {
        false
    }

    pub fn set_parent(
        &mut self,
        _shared: Arc<SharedState>,
        _parent: ParentWindowHandle,
    ) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ParentWindowHandle {
    #[cfg(all(unix, not(target_os = "macos")))]
    X11(u32),
    #[cfg(target_os = "macos")]
    Cocoa(*mut std::ffi::c_void),
    #[cfg(target_os = "windows")]
    Win32(*mut std::ffi::c_void),
}

pub fn is_api_supported(api: &CStr, is_floating: bool) -> bool {
    let _ = (api, is_floating);
    false
}

pub fn preferred_api() -> &'static CStr {
    c""
}
