use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct MonolithCache {
    files: HashMap<String, Vec<u8>>,
}

impl MonolithCache {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    pub fn load(&mut self, path: &str) -> Result<&[u8], String> {
        if !self.files.contains_key(path) {
            let data =
                std::fs::read(path).map_err(|e| format!("Failed to read {}: {}", path, e))?;
            self.files.insert(path.to_string(), data);
        }
        Ok(self.files.get(path).unwrap().as_slice())
    }

    pub fn has(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn preload(&mut self, path: &str) -> Result<(), String> {
        let _ = self.load(path)?;
        Ok(())
    }

    pub fn clear(&mut self) {
        self.files.clear();
    }

    pub fn remove(&mut self, path: &str) {
        self.files.remove(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monolith_cache_load() {
        let mut cache = MonolithCache::new();

        let path = std::env::current_exe().unwrap();
        let path_str = path.to_str().unwrap();
        let len1 = cache.load(path_str).unwrap().len();
        let len2 = cache.load(path_str).unwrap().len();
        assert_eq!(len1, len2);
        assert!(cache.has(path_str));
    }
}
