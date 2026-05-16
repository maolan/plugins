use crate::eq::common::params::{ParamIdExt, ParamStore};
use clap_clap::ffi::clap_host;
use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering};

const FADER_MIN_DB: f32 = -90.0;
pub const SPECTRUM_BINS: usize = 96;

pub struct SharedState<T: ParamIdExt> {
    pub params: ParamStore<T>,
    pub sample_rate_bits: AtomicU64,
    pub pending_param_notifications: Vec<AtomicU32>,
    pub pending_gesture_begin: Vec<AtomicU32>,
    pub pending_gesture_end: Vec<AtomicU32>,
    pub pending_param_values_bits: Vec<AtomicU64>,
    pub active_gesture_bits: Vec<AtomicU32>,
    pub active_gesture_count: AtomicU32,
    pub local_param_overrides: Vec<AtomicU32>,
    pub host: AtomicPtr<clap_host>,
    pub input_level_left_db_bits: AtomicU32,
    pub input_level_right_db_bits: AtomicU32,
    pub output_level_left_db_bits: AtomicU32,
    pub output_level_right_db_bits: AtomicU32,
    pub output_spectrum_db_bits: [AtomicU32; SPECTRUM_BINS],
    pub ui_visible: AtomicU32,
    pub channels: AtomicU32,
}

impl<T: ParamIdExt> SharedState<T> {
    fn decrement_gesture_count(&self) {
        let mut current = self.active_gesture_count.load(Ordering::Acquire);
        while current != 0 {
            match self.active_gesture_count.compare_exchange_weak(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(next) => current = next,
            }
        }
    }

    pub fn new(params: ParamStore<T>, host: *const clap_host, channels: u32) -> Self {
        let count = T::count();
        let words = count.div_ceil(32);
        let mut pending = Vec::with_capacity(words);
        let mut pending_begin = Vec::with_capacity(words);
        let mut pending_end = Vec::with_capacity(words);
        let mut pending_values = Vec::with_capacity(count);
        let mut active = Vec::with_capacity(words);
        let mut local = Vec::with_capacity(words);
        for _ in 0..words {
            pending.push(AtomicU32::new(0));
            pending_begin.push(AtomicU32::new(0));
            pending_end.push(AtomicU32::new(0));
            active.push(AtomicU32::new(0));
            local.push(AtomicU32::new(0));
        }
        for _ in 0..count {
            pending_values.push(AtomicU64::new(f64::NAN.to_bits()));
        }
        Self {
            params,
            sample_rate_bits: AtomicU64::new(48_000.0f64.to_bits()),
            pending_param_notifications: pending,
            pending_gesture_begin: pending_begin,
            pending_gesture_end: pending_end,
            pending_param_values_bits: pending_values,
            active_gesture_bits: active,
            active_gesture_count: AtomicU32::new(0),
            local_param_overrides: local,
            host: AtomicPtr::new(host.cast_mut()),
            input_level_left_db_bits: AtomicU32::new(FADER_MIN_DB.to_bits()),
            input_level_right_db_bits: AtomicU32::new(FADER_MIN_DB.to_bits()),
            output_level_left_db_bits: AtomicU32::new(FADER_MIN_DB.to_bits()),
            output_level_right_db_bits: AtomicU32::new(FADER_MIN_DB.to_bits()),
            output_spectrum_db_bits: std::array::from_fn(|_| {
                AtomicU32::new(FADER_MIN_DB.to_bits())
            }),
            ui_visible: AtomicU32::new(0),
            channels: AtomicU32::new(channels),
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

    pub fn mark_gesture_begin_pending(&self, id: T) {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        self.pending_gesture_begin[word].fetch_or(bit, Ordering::AcqRel);
        self.active_gesture_bits[word].fetch_or(bit, Ordering::AcqRel);
        self.active_gesture_count.fetch_add(1, Ordering::AcqRel);
        self.mark_dirty();
    }

    pub fn mark_gesture_end_pending(&self, id: T) {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        self.pending_gesture_end[word].fetch_or(bit, Ordering::AcqRel);
        self.active_gesture_bits[word].fetch_and(!bit, Ordering::AcqRel);
        self.decrement_gesture_count();
    }

    pub fn set_gesture_active(&self, id: T, active: bool) {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        if active {
            self.active_gesture_bits[word].fetch_or(bit, Ordering::AcqRel);
            self.active_gesture_count.fetch_add(1, Ordering::AcqRel);
        } else {
            self.active_gesture_bits[word].fetch_and(!bit, Ordering::AcqRel);
            self.decrement_gesture_count();
        }
    }

    pub fn is_gesture_active(&self, id: T) -> bool {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        (self.active_gesture_bits[word].load(Ordering::Acquire) & bit) != 0
    }

    pub fn any_gesture_active(&self) -> bool {
        self.active_gesture_count.load(Ordering::Acquire) != 0
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

    pub fn set_input_level_left_db(&self, db: f32) {
        self.input_level_left_db_bits
            .store(db.to_bits(), Ordering::Relaxed);
    }

    pub fn set_input_level_right_db(&self, db: f32) {
        self.input_level_right_db_bits
            .store(db.to_bits(), Ordering::Relaxed);
    }

    pub fn input_level_left_db(&self) -> f32 {
        f32::from_bits(self.input_level_left_db_bits.load(Ordering::Relaxed))
    }

    pub fn input_level_right_db(&self) -> f32 {
        f32::from_bits(self.input_level_right_db_bits.load(Ordering::Relaxed))
    }

    pub fn set_output_level_left_db(&self, db: f32) {
        self.output_level_left_db_bits
            .store(db.to_bits(), Ordering::Relaxed);
    }

    pub fn set_output_level_right_db(&self, db: f32) {
        self.output_level_right_db_bits
            .store(db.to_bits(), Ordering::Relaxed);
    }

    pub fn output_level_left_db(&self) -> f32 {
        f32::from_bits(self.output_level_left_db_bits.load(Ordering::Relaxed))
    }

    pub fn output_level_right_db(&self) -> f32 {
        f32::from_bits(self.output_level_right_db_bits.load(Ordering::Relaxed))
    }

    pub fn set_output_spectrum_db(&self, bins_db: &[f32; SPECTRUM_BINS]) {
        for (i, db) in bins_db.iter().enumerate() {
            self.output_spectrum_db_bits[i].store(db.to_bits(), Ordering::Relaxed);
        }
    }

    pub fn output_spectrum_db(&self) -> [f32; SPECTRUM_BINS] {
        std::array::from_fn(|i| {
            f32::from_bits(self.output_spectrum_db_bits[i].load(Ordering::Relaxed))
        })
    }

    pub fn set_ui_visible(&self, visible: bool) {
        self.ui_visible
            .store(if visible { 1 } else { 0 }, Ordering::Release);
    }

    pub fn is_ui_visible(&self) -> bool {
        self.ui_visible.load(Ordering::Acquire) != 0
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

    pub fn request_audio_ports_rescan(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.host.audio-ports".as_ptr());
            if ext.is_null() {
                return;
            }
            let audio_ports = &*(ext as *const clap_clap::ffi::clap_host_audio_ports);
            if let Some(rescan) = audio_ports.rescan {
                rescan(host, clap_clap::ffi::CLAP_AUDIO_PORTS_RESCAN_LIST);
            }
        }
    }

    pub fn set_param(&self, id: T, value: f64) {
        self.params.set(id, value);
        self.pending_param_values_bits[id.as_index()].store(value.to_bits(), Ordering::Release);
        self.mark_local_param_override(id);
        self.mark_param_notification_pending(id);
        self.request_flush();
        self.mark_dirty();
    }

    pub fn set_param_outbound_only(&self, id: T, value: f64) {
        self.params.set(id, value);
    }

    pub fn take_pending_param_value_or_current(&self, id: T) -> f64 {
        let bits = self.pending_param_values_bits[id.as_index()]
            .swap(f64::NAN.to_bits(), Ordering::AcqRel);
        let value = f64::from_bits(bits);
        if value.is_nan() {
            self.params.get(id)
        } else {
            value
        }
    }

    pub fn take_pending_gesture_begin_bits(&self) -> Vec<u32> {
        let mut bits = vec![0_u32; self.pending_gesture_begin.len()];
        for (i, atomic) in self.pending_gesture_begin.iter().enumerate() {
            bits[i] = atomic.swap(0, Ordering::AcqRel);
        }
        bits
    }

    pub fn take_pending_gesture_end_bits(&self) -> Vec<u32> {
        let mut bits = vec![0_u32; self.pending_gesture_end.len()];
        for (i, atomic) in self.pending_gesture_end.iter().enumerate() {
            bits[i] = atomic.swap(0, Ordering::AcqRel);
        }
        bits
    }
}
