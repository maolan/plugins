use std::collections::HashMap;
use std::sync::Arc;

/// A generic cache for decoded audio samples keyed by hash and file path.
///
/// Used by the Sampler and optionally by Drust to avoid loading the same
/// audio file more than once per session. The value type `V` is typically
/// `common::audio_file::AudioFile` or a plugin-specific sample wrapper.
#[derive(Debug)]
pub struct SampleCache<V> {
    by_hash: HashMap<String, Arc<V>>,
    by_path: HashMap<String, Arc<V>>,
    aliases: HashMap<String, String>,
}

impl<V> Default for SampleCache<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V> SampleCache<V> {
    pub fn new() -> Self {
        Self {
            by_hash: HashMap::new(),
            by_path: HashMap::new(),
            aliases: HashMap::new(),
        }
    }

    /// Map a missing path to an existing cached path.
    pub fn set_alias(&mut self, missing_path: &str, replacement_path: &str) {
        self.aliases
            .insert(missing_path.to_string(), replacement_path.to_string());
    }

    /// Look up a sample by its original file path, following aliases.
    pub fn get_by_path(&self, path: &str) -> Option<Arc<V>> {
        if let Some(sample) = self.by_path.get(path) {
            return Some(sample.clone());
        }
        if let Some(alias) = self.aliases.get(path) {
            return self.by_path.get(alias).cloned();
        }
        None
    }

    /// Insert a sample keyed by both hash and path.
    pub fn insert(&mut self, hash: &str, path: &str, sample: Arc<V>) {
        self.by_hash.insert(hash.to_string(), sample.clone());
        self.by_path.insert(path.to_string(), sample);
    }

    /// Check whether a hash is already cached.
    pub fn has_hash(&self, hash: &str) -> bool {
        self.by_hash.contains_key(hash)
    }

    /// Look up a sample by hash.
    pub fn get_by_hash(&self, hash: &str) -> Option<Arc<V>> {
        self.by_hash.get(hash).cloned()
    }

    /// Remove all cached entries.
    pub fn clear(&mut self) {
        self.by_hash.clear();
        self.by_path.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_insert_and_lookup() {
        let mut cache = SampleCache::<u32>::new();
        cache.insert("hash1", "/test.wav", Arc::new(42));
        assert_eq!(*cache.get_by_path("/test.wav").unwrap(), 42);
        assert_eq!(*cache.get_by_hash("hash1").unwrap(), 42);
    }

    #[test]
    fn test_cache_alias() {
        let mut cache = SampleCache::<u32>::new();
        cache.insert("hash1", "/real.wav", Arc::new(42));
        cache.set_alias("/missing.wav", "/real.wav");
        assert_eq!(*cache.get_by_path("/missing.wav").unwrap(), 42);
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = SampleCache::<u32>::new();
        cache.insert("hash1", "/test.wav", Arc::new(42));
        cache.clear();
        assert!(cache.get_by_path("/test.wav").is_none());
        assert!(cache.get_by_hash("hash1").is_none());
    }
}
