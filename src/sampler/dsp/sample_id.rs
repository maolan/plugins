use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SampleId {
    pub hash: String,

    pub path: String,
}

impl SampleId {
    pub fn from_path(path: &str) -> Self {
        let hash = compute_file_hash(path);
        Self {
            hash,
            path: path.to_string(),
        }
    }

    pub fn from_data(data: &[f32], path: &str) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();

        let samples_to_hash = data.len().min(16384);
        for sample in &data[..samples_to_hash] {
            hasher.update(sample.to_le_bytes());
        }
        let result = hasher.finalize();
        Self {
            hash: hex_encode(&result),
            path: path.to_string(),
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn compute_file_hash(path: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    if let Ok(data) = std::fs::read(path) {
        let to_hash = data.len().min(65536);
        hasher.update(&data[..to_hash]);
    } else {
        hasher.update(path.as_bytes());
    }
    let result = hasher.finalize();
    hex_encode(&result)
}

pub struct SampleCache {
    by_hash: HashMap<String, std::sync::Arc<crate::sampler::dsp::sample::Sample>>,

    by_path: HashMap<String, std::sync::Arc<crate::sampler::dsp::sample::Sample>>,

    aliases: HashMap<String, String>,
}

impl Default for SampleCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SampleCache {
    pub fn new() -> Self {
        Self {
            by_hash: HashMap::new(),
            by_path: HashMap::new(),
            aliases: HashMap::new(),
        }
    }

    pub fn set_alias(&mut self, missing_path: &str, replacement_path: &str) {
        self.aliases
            .insert(missing_path.to_string(), replacement_path.to_string());
    }

    pub fn get_by_path(
        &self,
        path: &str,
    ) -> Option<std::sync::Arc<crate::sampler::dsp::sample::Sample>> {
        if let Some(sample) = self.by_path.get(path) {
            return Some(sample.clone());
        }
        if let Some(alias) = self.aliases.get(path) {
            return self.by_path.get(alias).cloned();
        }
        None
    }

    pub fn insert(
        &mut self,
        id: &SampleId,
        sample: std::sync::Arc<crate::sampler::dsp::sample::Sample>,
    ) {
        self.by_hash.insert(id.hash.clone(), sample.clone());
        self.by_path.insert(id.path.clone(), sample);
    }

    pub fn has_hash(&self, hash: &str) -> bool {
        self.by_hash.contains_key(hash)
    }

    pub fn get_by_hash(
        &self,
        hash: &str,
    ) -> Option<std::sync::Arc<crate::sampler::dsp::sample::Sample>> {
        self.by_hash.get(hash).cloned()
    }

    pub fn clear(&mut self) {
        self.by_hash.clear();
        self.by_path.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_id_from_data() {
        let data = vec![1.0f32, 2.0, 3.0, 4.0];
        let id = SampleId::from_data(&data, "/test/sample.wav");
        assert_eq!(id.path, "/test/sample.wav");
        assert!(!id.hash.is_empty());

        let id2 = SampleId::from_data(&data, "/test/sample.wav");
        assert_eq!(id.hash, id2.hash);
    }

    #[test]
    fn test_sample_cache() {
        let mut cache = SampleCache::new();
        let id = SampleId::from_data(&[1.0f32, 2.0], "/test.wav");
        let sample = std::sync::Arc::new(crate::sampler::dsp::sample::Sample::silent(48000.0));
        cache.insert(&id, sample.clone());

        assert!(cache.get_by_path("/test.wav").is_some());
        assert!(cache.get_by_hash(&id.hash).is_some());
    }
}
