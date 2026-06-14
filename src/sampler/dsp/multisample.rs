use crate::sampler::dsp::patch::Patch;

pub fn parse_multisample(_path: &str) -> Result<Patch, String> {
    Err("Generic multisample format not yet implemented".to_string())
}
