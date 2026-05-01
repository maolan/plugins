#[derive(Debug, Clone, Default)]
pub struct Channel {
    pub name: String,
    pub num: usize,
}

impl Channel {
    pub fn new(name: impl Into<String>, num: usize) -> Self {
        Self {
            name: name.into(),
            num,
        }
    }
}
