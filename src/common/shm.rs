//! Shared-memory helpers for the plugin billboard.

#[cfg(unix)]
mod imp {
    /// An owned POSIX shared-memory mapping.
    pub struct ShmMapping {
        ptr: *mut u8,
        size: usize,
        name: String,
    }

    // SAFETY: the mapped memory is process-shared.
    unsafe impl Send for ShmMapping {}
    unsafe impl Sync for ShmMapping {}

    impl ShmMapping {
        /// Create a new shared-memory segment and map it read/write.
        pub fn create(name: &str, size: usize) -> Result<Self, String> {
            let c_name = std::ffi::CString::new(name).map_err(|e| e.to_string())?;
            let fd = unsafe {
                libc::shm_open(
                    c_name.as_ptr(),
                    libc::O_CREAT | libc::O_RDWR | libc::O_EXCL,
                    0o666,
                )
            };
            if fd < 0 {
                return Err(format!(
                    "shm_open({}) failed: {}",
                    name,
                    std::io::Error::last_os_error()
                ));
            }
            if unsafe { libc::ftruncate(fd, size as libc::off_t) } < 0 {
                unsafe { libc::close(fd) };
                unsafe { libc::shm_unlink(c_name.as_ptr()) };
                return Err(format!(
                    "ftruncate({}) failed: {}",
                    name,
                    std::io::Error::last_os_error()
                ));
            }
            let ptr = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    size,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    fd,
                    0,
                )
            };
            unsafe { libc::close(fd) };
            if ptr == libc::MAP_FAILED {
                unsafe { libc::shm_unlink(c_name.as_ptr()) };
                return Err(format!(
                    "mmap({}) failed: {}",
                    name,
                    std::io::Error::last_os_error()
                ));
            }
            Ok(Self {
                ptr: ptr as *mut u8,
                size,
                name: name.to_string(),
            })
        }

        /// Open an existing shared-memory segment read/write.
        pub fn open_existing(name: &str, size: usize) -> Result<Self, String> {
            let c_name = std::ffi::CString::new(name).map_err(|e| e.to_string())?;
            let fd = unsafe { libc::shm_open(c_name.as_ptr(), libc::O_RDWR, 0o666) };
            if fd < 0 {
                return Err(format!(
                    "shm_open({}) failed: {}",
                    name,
                    std::io::Error::last_os_error()
                ));
            }
            let ptr = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    size,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    fd,
                    0,
                )
            };
            unsafe { libc::close(fd) };
            if ptr == libc::MAP_FAILED {
                return Err(format!(
                    "mmap({}) failed: {}",
                    name,
                    std::io::Error::last_os_error()
                ));
            }
            Ok(Self {
                ptr: ptr as *mut u8,
                size,
                name: name.to_string(),
            })
        }

        pub fn as_ptr(&self) -> *mut u8 {
            self.ptr
        }

        pub fn size(&self) -> usize {
            self.size
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        /// Unlink the shared-memory segment (does not unmap this process).
        pub fn unlink(name: &str) -> Result<(), String> {
            let c_name = std::ffi::CString::new(name).map_err(|e| e.to_string())?;
            let rc = unsafe { libc::shm_unlink(c_name.as_ptr()) };
            if rc < 0 {
                Err(format!(
                    "shm_unlink({}) failed: {}",
                    name,
                    std::io::Error::last_os_error()
                ))
            } else {
                Ok(())
            }
        }
    }

    impl Drop for ShmMapping {
        fn drop(&mut self) {
            unsafe {
                libc::munmap(self.ptr as *mut libc::c_void, self.size);
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::{CString, c_void};
    use std::os::raw::c_char;

    type HANDLE = *mut c_void;

    const PAGE_READWRITE: u32 = 0x04;
    const FILE_MAP_ALL_ACCESS: u32 = 0xF001F;

    unsafe extern "system" {
        fn CreateFileMappingA(
            hFile: HANDLE,
            lpAttributes: *mut c_void,
            flProtect: u32,
            dwMaximumSizeHigh: u32,
            dwMaximumSizeLow: u32,
            lpName: *const c_char,
        ) -> HANDLE;
        fn OpenFileMappingA(
            dwDesiredAccess: u32,
            bInheritHandle: i32,
            lpName: *const c_char,
        ) -> HANDLE;
        fn MapViewOfFile(
            hFileMappingObject: HANDLE,
            dwDesiredAccess: u32,
            dwFileOffsetHigh: u32,
            dwFileOffsetLow: u32,
            dwNumberOfBytesToMap: usize,
        ) -> *mut c_void;
        fn UnmapViewOfFile(lpBaseAddress: *const c_void) -> i32;
        fn CloseHandle(hObject: HANDLE) -> i32;
    }

    /// An owned Windows named file-mapping (shared memory).
    pub struct ShmMapping {
        handle: HANDLE,
        ptr: *mut u8,
        size: usize,
        name: String,
    }

    // SAFETY: the mapped memory is process-shared.
    unsafe impl Send for ShmMapping {}
    unsafe impl Sync for ShmMapping {}

    fn format_name(name: &str) -> CString {
        // POSIX names often start with '/'; Windows object names cannot contain it.
        let stripped = name.strip_prefix('/').unwrap_or(name);
        CString::new(stripped).expect("invalid shared-memory name")
    }

    impl ShmMapping {
        pub fn create(name: &str, size: usize) -> Result<Self, String> {
            let c_name = format_name(name);
            let handle = unsafe {
                CreateFileMappingA(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    PAGE_READWRITE,
                    0,
                    size as u32,
                    c_name.as_ptr(),
                )
            };
            if handle.is_null() {
                return Err(format!(
                    "CreateFileMappingA({}) failed: {}",
                    name,
                    std::io::Error::last_os_error()
                ));
            }
            let ptr = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, size) as *mut u8 };
            if ptr.is_null() {
                unsafe { CloseHandle(handle) };
                return Err(format!(
                    "MapViewOfFile({}) failed: {}",
                    name,
                    std::io::Error::last_os_error()
                ));
            }
            Ok(Self {
                handle,
                ptr,
                size,
                name: name.to_string(),
            })
        }

        pub fn open_existing(name: &str, size: usize) -> Result<Self, String> {
            let c_name = format_name(name);
            let handle = unsafe { OpenFileMappingA(FILE_MAP_ALL_ACCESS, 0, c_name.as_ptr()) };
            if handle.is_null() {
                return Err(format!(
                    "OpenFileMappingA({}) failed: {}",
                    name,
                    std::io::Error::last_os_error()
                ));
            }
            let ptr = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, size) as *mut u8 };
            if ptr.is_null() {
                unsafe { CloseHandle(handle) };
                return Err(format!(
                    "MapViewOfFile({}) failed: {}",
                    name,
                    std::io::Error::last_os_error()
                ));
            }
            Ok(Self {
                handle,
                ptr,
                size,
                name: name.to_string(),
            })
        }

        pub fn as_ptr(&self) -> *mut u8 {
            self.ptr
        }

        pub fn size(&self) -> usize {
            self.size
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        /// Windows file mappings disappear when the last handle is closed;
        /// there is no separate unlink step.
        pub fn unlink(_name: &str) -> Result<(), String> {
            Ok(())
        }
    }

    impl Drop for ShmMapping {
        fn drop(&mut self) {
            unsafe {
                if !self.ptr.is_null() {
                    UnmapViewOfFile(self.ptr as *const c_void);
                }
                if !self.handle.is_null() {
                    CloseHandle(self.handle);
                }
            }
        }
    }
}

pub use imp::ShmMapping;

/// Try to open an existing segment; if it does not exist, create it.
/// Returns `(mapping, created)`.
pub fn open_or_create(name: &str, size: usize) -> Result<(ShmMapping, bool), String> {
    match ShmMapping::open_existing(name, size) {
        Ok(m) => Ok((m, false)),
        Err(_) => ShmMapping::create(name, size).map(|m| (m, true)),
    }
}
