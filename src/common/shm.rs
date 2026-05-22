//! POSIX shared-memory helpers for the plugin billboard.

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

/// Try to open an existing segment; if it does not exist, create it.
/// Returns `(mapping, created)`.
pub fn open_or_create(name: &str, size: usize) -> Result<(ShmMapping, bool), String> {
    match ShmMapping::open_existing(name, size) {
        Ok(m) => Ok((m, false)),
        Err(_) => ShmMapping::create(name, size).map(|m| (m, true)),
    }
}
