use std::ffi::CStr;

pub const EDITOR_WIDTH: u32 = 1024;
pub const EDITOR_HEIGHT: u32 = 939;

#[derive(Debug, Default)]
pub struct GuiBridge {
    active: bool,
}

impl GuiBridge {
    pub fn create(&mut self, _api: &CStr, _is_floating: bool) -> bool {
        self.active = true;
        true
    }

    pub fn destroy(&mut self) {
        self.active = false;
    }

    pub fn show(&mut self) -> bool {
        self.active
    }

    pub fn hide(&mut self) -> bool {
        self.active = false;
        true
    }
}

pub fn is_api_supported(api: &CStr, is_floating: bool) -> bool {
    if is_floating {
        return false;
    }
    matches!(api.to_bytes(), b"win32" | b"cocoa" | b"x11")
}

pub fn preferred_api() -> &'static CStr {
    c"x11"
}
