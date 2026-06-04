//! Generic multisample format parser (stub).
//!
//! A generic container format for multi-sampled instruments.
//! This stub provides the expected API surface.

use crate::sampler::dsp::patch::Patch;

/// Parse a generic multisample file.
/// Currently returns an error as no standard format is adopted.
pub fn parse_multisample(_path: &str) -> Result<Patch, String> {
    Err("Generic multisample format not yet implemented".to_string())
}
