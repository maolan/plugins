use crate::common::sample_cache::SampleCache as CommonSampleCache;
use crate::sampler::dsp::sample::Sample;

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

/// Sampler-specific cache backed by the common implementation.
pub type SampleCache = CommonSampleCache<Sample>;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

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
        let sample = Arc::new(Sample::silent(48000.0));
        cache.insert(&id.hash, &id.path, sample);

        assert!(cache.get_by_path("/test.wav").is_some());
        assert!(cache.get_by_hash(&id.hash).is_some());
    }
}
