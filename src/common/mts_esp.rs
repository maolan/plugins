#![allow(dead_code)]

use std::sync::Arc;

use libloading::Library;
use parking_lot::Mutex;

type MTSRegisterClient = unsafe extern "C" fn() -> *mut std::ffi::c_void;
type MTSDeregisterClient = unsafe extern "C" fn(client: *mut std::ffi::c_void);
type MTSHasMaster = unsafe extern "C" fn(client: *mut std::ffi::c_void) -> bool;
type MTSNoteToFrequency =
    unsafe extern "C" fn(client: *mut std::ffi::c_void, note: i8, channel: i8) -> f64;

pub struct MtsEspClient {
    _lib: Arc<Library>,
    handle: *mut std::ffi::c_void,
    deregister: MTSDeregisterClient,
    has_master: MTSHasMaster,
    note_to_freq: MTSNoteToFrequency,
}

impl std::fmt::Debug for MtsEspClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MtsEspClient")
            .field("handle", &self.handle)
            .field("connected", &self.is_connected())
            .finish()
    }
}

unsafe impl Send for MtsEspClient {}
unsafe impl Sync for MtsEspClient {}

impl MtsEspClient {
    pub fn try_new() -> Option<Arc<Mutex<Self>>> {
        let lib = Arc::new(Self::load_library()?);

        unsafe {
            let register: libloading::Symbol<MTSRegisterClient> =
                lib.get(b"MTS_RegisterClient\0").ok()?;
            let deregister: libloading::Symbol<MTSDeregisterClient> =
                lib.get(b"MTS_DeregisterClient\0").ok()?;
            let has_master: libloading::Symbol<MTSHasMaster> = lib.get(b"MTS_HasMaster\0").ok()?;
            let note_to_freq: libloading::Symbol<MTSNoteToFrequency> =
                lib.get(b"MTS_NoteToFrequency\0").ok()?;

            let handle = register();
            if handle.is_null() {
                return None;
            }

            let lib2 = Arc::clone(&lib);

            Some(Arc::new(Mutex::new(MtsEspClient {
                _lib: lib2,
                handle,
                deregister: *deregister,
                has_master: *has_master,
                note_to_freq: *note_to_freq,
            })))
        }
    }

    #[cfg(target_os = "linux")]
    fn load_library() -> Option<Library> {
        unsafe {
            Library::new("libMTS.so")
                .or_else(|_| Library::new("/usr/local/lib/libMTS.so"))
                .or_else(|_| Library::new("/usr/lib/libMTS.so"))
                .ok()
        }
    }

    #[cfg(target_os = "windows")]
    fn load_library() -> Option<Library> {
        unsafe {
            Library::new("LIBMTS.dll")
                .or_else(|_| Library::new(r"C:\Program Files\MTS-ESP\LIBMTS.dll"))
                .ok()
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    fn load_library() -> Option<Library> {
        unsafe {
            Library::new("libMTS.so")
                .or_else(|_| Library::new("/usr/local/lib/libMTS.so"))
                .or_else(|_| Library::new("/usr/lib/libMTS.so"))
                .ok()
        }
    }

    pub fn is_connected(&self) -> bool {
        unsafe { (self.has_master)(self.handle) }
    }

    pub fn note_to_frequency(&self, note: u8, channel: u8) -> Option<f32> {
        if !self.is_connected() {
            return None;
        }
        let freq = unsafe { (self.note_to_freq)(self.handle, note as i8, channel as i8) };
        if freq > 0.0 { Some(freq as f32) } else { None }
    }
}

impl Drop for MtsEspClient {
    fn drop(&mut self) {
        unsafe {
            (self.deregister)(self.handle);
        }
    }
}
