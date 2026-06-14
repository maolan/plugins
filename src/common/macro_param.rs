#[derive(Debug, Clone)]
pub struct MacroParam {
    pub name: String,

    pub value: f32,

    pub bipolar: bool,

    pub steps: usize,
}

impl Default for MacroParam {
    fn default() -> Self {
        Self {
            name: String::new(),
            value: 0.0,
            bipolar: false,
            steps: 0,
        }
    }
}

impl MacroParam {
    pub const COUNT: usize = 16;

    pub fn new(name: &str, bipolar: bool) -> Self {
        Self {
            name: name.to_string(),
            value: 0.0,
            bipolar,
            steps: 0,
        }
    }

    pub fn set_value(&mut self, value: f32) {
        self.value = if self.bipolar {
            value.clamp(-1.0, 1.0)
        } else {
            value.clamp(0.0, 1.0)
        };
    }

    pub fn normalized_value(&self) -> f32 {
        if self.bipolar {
            (self.value + 1.0) * 0.5
        } else {
            self.value
        }
    }
}

pub fn default_macros() -> [MacroParam; 16] {
    let mut macros: [MacroParam; 16] = std::array::from_fn(|_| MacroParam::default());
    for (i, m) in macros.iter_mut().enumerate() {
        m.name = format!("Macro {}", i + 1);
    }
    macros
}
