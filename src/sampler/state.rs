use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, Ordering};

pub use crate::common::state::{PluginState, SamplerZoneState};

#[derive(Debug, Clone)]
pub struct SampleZone {
    pub name: String,
    pub files: Vec<PathBuf>,
    pub start_note: usize,
    pub end_note: usize,
    pub vel_low: u8,
    pub vel_high: u8,
    pub group: String,
}

impl SampleZone {
    pub fn to_state(&self) -> SamplerZoneState {
        SamplerZoneState {
            name: self.name.clone(),
            files: self
                .files
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect(),
            start_note: self.start_note,
            end_note: self.end_note,
            vel_low: self.vel_low,
            vel_high: self.vel_high,
            group: self.group.clone(),
        }
    }

    pub fn from_state(state: &SamplerZoneState) -> Self {
        Self {
            name: state.name.clone(),
            files: state.files.iter().map(PathBuf::from).collect(),
            start_note: state.start_note,
            end_note: state.end_note,
            vel_low: state.vel_low,
            vel_high: state.vel_high,
            group: if state.group.is_empty() {
                String::from("New Group")
            } else {
                state.group.clone()
            },
        }
    }
}

pub struct AtomicArc<T> {
    ptr: AtomicPtr<T>,
}

impl<T> std::fmt::Debug for AtomicArc<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AtomicArc").finish_non_exhaustive()
    }
}

impl<T> AtomicArc<T> {
    pub fn new(value: Arc<T>) -> Self {
        let ptr = Arc::into_raw(value) as *mut T;
        Self {
            ptr: AtomicPtr::new(ptr),
        }
    }

    pub fn load(&self) -> Arc<T> {
        let ptr = self.ptr.load(Ordering::Acquire);
        unsafe {
            Arc::increment_strong_count(ptr);
            Arc::from_raw(ptr)
        }
    }

    pub fn store(&self, value: Arc<T>) {
        let new_ptr = Arc::into_raw(value) as *mut T;
        let old_ptr = self.ptr.swap(new_ptr, Ordering::AcqRel);
        unsafe {
            drop(Arc::from_raw(old_ptr));
        }
    }
}

impl<T: Default> Default for AtomicArc<T> {
    fn default() -> Self {
        Self::new(Arc::new(T::default()))
    }
}

impl<T> Drop for AtomicArc<T> {
    fn drop(&mut self) {
        let ptr = self.ptr.load(Ordering::Acquire);
        unsafe {
            drop(Arc::from_raw(ptr));
        }
    }
}

unsafe impl<T: Send> Send for AtomicArc<T> {}
unsafe impl<T: Send + Sync> Sync for AtomicArc<T> {}
