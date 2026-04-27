use std::sync::atomic::{AtomicPtr, AtomicU8, AtomicU32, Ordering};

use clap_clap::ffi::{
    clap_host, clap_host_latency, clap_host_note_name, clap_host_params, clap_host_state,
};
use parking_lot::RwLock;

use crate::params::{ParamId, ParamStore, sanitize_param_value};

#[derive(Debug)]
pub struct SharedState {
    pub params: ParamStore,
    pub kit_path: RwLock<String>,
    pub midimap_path: RwLock<String>,
    pub variation: RwLock<String>,
    pub last_error: RwLock<Option<String>>,
    pub pending_kit_path: RwLock<Option<String>>,
    pub pending_midimap_path: RwLock<Option<String>>,
    pub pending_param_notifications: AtomicU32,
    pub local_param_overrides: AtomicU32,
    pub host: AtomicPtr<clap_host>,
    pub active_channels: AtomicU32,
    pub state_id: RwLock<String>,
    pub loading_progress: AtomicU8,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            params: ParamStore::default(),
            kit_path: RwLock::new(String::new()),
            midimap_path: RwLock::new(String::new()),
            variation: RwLock::new(String::new()),
            last_error: RwLock::new(None),
            pending_kit_path: RwLock::new(None),
            pending_midimap_path: RwLock::new(None),
            pending_param_notifications: AtomicU32::new(0),
            local_param_overrides: AtomicU32::new(0),
            host: AtomicPtr::new(std::ptr::null_mut()),
            active_channels: AtomicU32::new(0),
            state_id: RwLock::new(String::new()),
            loading_progress: AtomicU8::new(0),
        }
    }
}

impl SharedState {
    pub fn set_param(&self, id: ParamId, value: f64) {
        self.params.set(id, sanitize_param_value(id, value));
        let bit = 1_u32 << (id.as_index() as u32);
        self.local_param_overrides.fetch_or(bit, Ordering::AcqRel);
        self.pending_param_notifications
            .fetch_or(bit, Ordering::AcqRel);
        self.request_flush();
        self.mark_dirty();
    }

    pub fn set_param_from_host(&self, id: ParamId, value: f64) {
        self.params.set(id, sanitize_param_value(id, value));
    }

    fn request_flush(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            if let Some(get_ext) = (*host).get_extension {
                let ext = get_ext(host, c"clap.host.params".as_ptr());
                if !ext.is_null() {
                    let params = &*(ext as *const clap_host_params);
                    if let Some(f) = params.request_flush {
                        f(host);
                    }
                }
            }
        }
    }

    pub(crate) fn mark_dirty(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            if let Some(get_ext) = (*host).get_extension {
                let ext = get_ext(host, c"clap.host.state".as_ptr());
                if !ext.is_null() {
                    let state = &*(ext as *const clap_host_state);
                    if let Some(f) = state.mark_dirty {
                        f(host);
                    }
                }
            }
        }
    }

    pub fn latency_changed(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            if let Some(get_ext) = (*host).get_extension {
                let ext = get_ext(host, c"clap.host.latency".as_ptr());
                if !ext.is_null() {
                    let lat = &*(ext as *const clap_host_latency);
                    if let Some(f) = lat.changed {
                        f(host);
                    }
                }
            }
        }
    }

    pub fn note_names_changed(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            if let Some(get_ext) = (*host).get_extension {
                let ext = get_ext(host, c"clap.host.note-name".as_ptr());
                if !ext.is_null() {
                    let nn = &*(ext as *const clap_host_note_name);
                    if let Some(f) = nn.changed {
                        f(host);
                    }
                }
            }
        }
    }

    pub fn take_pending_param_notifications(&self) -> u32 {
        self.pending_param_notifications.swap(0, Ordering::AcqRel)
    }

    pub fn requeue_pending_param_notifications(&self, bits: u32) {
        if bits != 0 {
            self.pending_param_notifications
                .fetch_or(bits, Ordering::AcqRel);
        }
    }

    pub fn has_local_param_override(&self, id: ParamId) -> bool {
        let bit = 1_u32 << (id.as_index() as u32);
        (self.local_param_overrides.load(Ordering::Acquire) & bit) != 0
    }

    pub fn clear_local_param_override(&self, id: ParamId) {
        let bit = !(1_u32 << (id.as_index() as u32));
        self.local_param_overrides.fetch_and(bit, Ordering::AcqRel);
    }

    pub fn set_host(&self, host: *const clap_host) {
        self.host.store(host.cast_mut(), Ordering::Release);
    }
}
