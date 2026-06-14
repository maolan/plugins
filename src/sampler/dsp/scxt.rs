use crate::sampler::dsp::patch::Patch;

pub fn parse_scxt(_path: &str) -> Result<Patch, String> {
    Err("SCXT monolith format is proprietary and not yet implemented".to_string())
}
