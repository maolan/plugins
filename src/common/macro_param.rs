//! Part macro definitions — 16 user-controllable parameters per part.

/// A macro is a user-defined control parameter for a part.
/// Macros can be bipolar (-1..1) or unipolar (0..1), optionally stepped.
#[derive(Debug, Clone)]
pub struct MacroParam {
    /// User-defined name.
    pub name: String,
    /// Current value (-1..1 for bipolar, 0..1 for unipolar).
    pub value: f32,
    /// Whether the macro is bipolar (-1..1) instead of unipolar (0..1).
    pub bipolar: bool,
    /// Number of steps (0 = continuous).
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

    /// Create a new macro with the given name and range.
    pub fn new(name: &str, bipolar: bool) -> Self {
        Self {
            name: name.to_string(),
            value: 0.0,
            bipolar,
            steps: 0,
        }
    }

    /// Set the value, clamped to the macro's range.
    pub fn set_value(&mut self, value: f32) {
        self.value = if self.bipolar {
            value.clamp(-1.0, 1.0)
        } else {
            value.clamp(0.0, 1.0)
        };
    }

    /// Get the normalized value (always 0..1, with bipolar centered at 0.5).
    pub fn normalized_value(&self) -> f32 {
        if self.bipolar {
            (self.value + 1.0) * 0.5
        } else {
            self.value
        }
    }
}

/// Default array of 16 macros for a part.
pub fn default_macros() -> [MacroParam; 16] {
    let mut macros: [MacroParam; 16] = std::array::from_fn(|_| MacroParam::default());
    for (i, m) in macros.iter_mut().enumerate() {
        m.name = format!("Macro {}", i + 1);
    }
    macros
}
