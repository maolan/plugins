use crate::eq::common::params::{ParamIdExt, ParamStore};
use clap_clap::ffi::clap_host;
use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering};

pub struct SharedState<T: ParamIdExt> {
    pub params: ParamStore<T>,
    pub sample_rate_bits: AtomicU64,
    pub pending_param_notifications: Vec<AtomicU32>,
    pub local_param_overrides: Vec<AtomicU32>,
    pub host: AtomicPtr<clap_host>,
}

impl<T: ParamIdExt> SharedState<T> {
    pub fn new(params: ParamStore<T>, host: *const clap_host) -> Self {
        let count = T::count();
        let words = count.div_ceil(32);
        let mut pending = Vec::with_capacity(words);
        let mut local = Vec::with_capacity(words);
        for _ in 0..words {
            pending.push(AtomicU32::new(0));
            local.push(AtomicU32::new(0));
        }
        Self {
            params,
            sample_rate_bits: AtomicU64::new(48_000.0f64.to_bits()),
            pending_param_notifications: pending,
            local_param_overrides: local,
            host: AtomicPtr::new(host.cast_mut()),
        }
    }

    pub fn sample_rate(&self) -> f32 {
        f64::from_bits(self.sample_rate_bits.load(Ordering::Acquire)) as f32
    }

    pub fn mark_param_notification_pending(&self, id: T) {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        self.pending_param_notifications[word].fetch_or(bit, Ordering::AcqRel);
    }

    pub fn mark_local_param_override(&self, id: T) {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        self.local_param_overrides[word].fetch_or(bit, Ordering::AcqRel);
    }

    pub fn has_local_param_override(&self, id: T) -> bool {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        (self.local_param_overrides[word].load(Ordering::Acquire) & bit) != 0
    }

    pub fn clear_local_param_override(&self, id: T) {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = !(1_u32 << (idx % 32));
        self.local_param_overrides[word].fetch_and(bit, Ordering::AcqRel);
    }

    pub fn request_flush(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.host.params".as_ptr());
            if ext.is_null() {
                return;
            }
            let params = &*(ext as *const clap_clap::ffi::clap_host_params);
            if let Some(request_flush) = params.request_flush {
                request_flush(host);
            }
        }
    }

    pub fn mark_dirty(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.host.state".as_ptr());
            if ext.is_null() {
                return;
            }
            let state = &*(ext as *const clap_clap::ffi::clap_host_state);
            if let Some(mark_dirty) = state.mark_dirty {
                mark_dirty(host);
            }
        }
    }

    pub fn request_gui_closed(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.host.gui".as_ptr());
            if ext.is_null() {
                return;
            }
            let gui = &*(ext as *const clap_clap::ffi::clap_host_gui);
            if let Some(closed) = gui.closed {
                closed(host, false);
            }
        }
    }

    pub fn set_param(&self, id: T, value: f64) {
        self.params.set(id, value);
        self.mark_local_param_override(id);
        self.mark_param_notification_pending(id);
        self.request_flush();
        self.mark_dirty();
    }
}
