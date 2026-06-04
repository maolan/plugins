//! Shortcircuit XT monolith format parser (stub).
//!
//! The SCXT monolith format is proprietary. This stub provides the
//! expected API surface; full implementation requires reverse-engineering
//! the binary format.

use crate::sampler::dsp::patch::Patch;

/// Parse a Shortcircuit XT monolith file.
/// Currently returns an error as the format is proprietary.
pub fn parse_scxt(_path: &str) -> Result<Patch, String> {
    Err("SCXT monolith format is proprietary and not yet implemented".to_string())
}
