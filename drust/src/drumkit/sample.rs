#[derive(Debug, Clone, Default)]
pub struct AudioFileRef {
    pub channel: String,
    pub file: String,
    pub abs_path: String,
    pub filechannel: usize, // 1-indexed in XML, converted to 0-indexed
}

#[derive(Debug, Clone, Default)]
pub struct Sample {
    pub name: String,
    pub power: f32,
    pub normalized: bool,
    pub audiofiles: Vec<AudioFileRef>,
}

impl Sample {
    pub fn new() -> Self {
        Self::default()
    }
}
