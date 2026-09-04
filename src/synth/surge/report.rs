//! Human-readable report of what a Surge preset import did and could not do.

/// Result report for one preset import.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Hard problems that prevented individual elements from loading.
    pub errors: Vec<String>,
    /// Elements that were deliberately not converted: (element, reason).
    pub skipped: Vec<(String, String)>,
    /// Approximations made while converting.
    pub notes: Vec<String>,
}

impl Report {
    pub fn error(&mut self, message: impl Into<String>) {
        self.errors.push(message.into());
    }

    pub fn skip(&mut self, element: impl Into<String>, reason: impl Into<String>) {
        self.skipped.push((element.into(), reason.into()));
    }

    pub fn note(&mut self, message: impl Into<String>) {
        self.notes.push(message.into());
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty() && self.skipped.is_empty() && self.notes.is_empty()
    }

    /// Render the report for on-screen display.
    pub fn to_display_string(&self) -> String {
        let mut out = String::new();
        for error in &self.errors {
            out.push_str(&format!("ERROR: {error}\n"));
        }
        for (element, reason) in &self.skipped {
            out.push_str(&format!("skipped {element}: {reason}\n"));
        }
        for note in &self.notes {
            out.push_str(&format!("note: {note}\n"));
        }
        out
    }
}
