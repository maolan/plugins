#[cfg(unix)]
use std::fs::File;
use std::path::Path;

#[cfg(unix)]
pub struct MmapView {
    ptr: *mut u8,
    len: usize,
}

#[cfg(not(unix))]
pub struct MmapView {
    data: Vec<u8>,
}

#[cfg(unix)]
impl MmapView {
    pub fn map_file(path: &Path) -> Result<Self, String> {
        use std::os::unix::io::AsRawFd;

        let file = File::open(path).map_err(|e| format!("Failed to open file for mmap: {}", e))?;
        let len = file
            .metadata()
            .map_err(|e| format!("Failed to get file metadata: {}", e))?
            .len() as usize;

        if len == 0 {
            return Err("Cannot mmap empty file".to_string());
        }

        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                file.as_raw_fd(),
                0,
            )
        };

        if ptr == libc::MAP_FAILED {
            return Err("mmap failed".to_string());
        }

        Ok(Self {
            ptr: ptr as *mut u8,
            len,
        })
    }

    pub fn as_bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(unix)]
impl Drop for MmapView {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr as *mut libc::c_void, self.len);
        }
    }
}

#[cfg(not(unix))]
impl MmapView {
    pub fn map_file(path: &Path) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
        Ok(Self { data })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_mmap_file() {
        let path = std::env::temp_dir().join("test_mmap_sample.txt");
        let mut file = File::create(&path).unwrap();
        file.write_all(b"Hello, mmap!").unwrap();
        drop(file);

        let view = MmapView::map_file(&path).unwrap();
        assert_eq!(view.len(), 12);
        assert_eq!(view.as_bytes(), b"Hello, mmap!");
    }
}
