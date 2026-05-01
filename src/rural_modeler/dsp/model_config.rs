use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::Value;

use crate::rural_modeler::dsp::core::Dsp;
use crate::rural_modeler::dsp::error::NamError;

// =============================================================================
// ModelConfig trait
// =============================================================================

/// Abstract base class for architecture-specific configuration.
///
/// Each architecture defines a concrete config struct that implements this
/// trait and provides `create()` to construct the DSP object.
///
/// Matches NAM C++ `nam::ModelConfig`.
pub trait ModelConfig: std::fmt::Debug + Send {
    /// Construct a DSP object from this configuration.
    ///
    /// `weights` are taken by value to allow move semantics.
    fn create(&self, weights: Vec<f32>, sample_rate: f64) -> Result<Box<dyn Dsp>, NamError>;
}

// =============================================================================
// ConfigParserRegistry
// =============================================================================

pub type ConfigParserFunction =
    Box<dyn Fn(&Value, f64) -> Result<Box<dyn ModelConfig>, NamError> + Send + Sync>;

/// Singleton registry mapping architecture names to config parser functions.
///
/// Both built-in and external architectures register here. There is one
/// construction path for all architectures.
///
/// Matches NAM C++ `nam::ConfigParserRegistry`.
pub struct ConfigParserRegistry {
    parsers: HashMap<String, ConfigParserFunction>,
}

impl ConfigParserRegistry {
    pub fn new() -> Self {
        Self {
            parsers: HashMap::new(),
        }
    }

    /// Register a config parser for an architecture.
    ///
    /// # Panics
    /// Panics if the name is already registered.
    pub fn register(&mut self, name: &str, parser: ConfigParserFunction) {
        if self.parsers.contains_key(name) {
            panic!("Config parser already registered for: {name}");
        }
        self.parsers.insert(name.to_string(), parser);
    }

    /// Check whether an architecture name is registered.
    pub fn has(&self, name: &str) -> bool {
        self.parsers.contains_key(name)
    }

    /// Parse a `ModelConfig` from an architecture name, JSON config, and sample rate.
    ///
    /// # Errors
    /// Returns `NamError::UnsupportedArchitecture` if no parser is registered.
    pub fn parse(
        &self,
        name: &str,
        config: &Value,
        sample_rate: f64,
    ) -> Result<Box<dyn ModelConfig>, NamError> {
        let parser = self
            .parsers
            .get(name)
            .ok_or_else(|| NamError::UnsupportedArchitecture(name.to_string()))?;
        parser(config, sample_rate)
    }
}

impl Default for ConfigParserRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Global instance
// =============================================================================

use std::sync::OnceLock;

static REGISTRY: OnceLock<Mutex<ConfigParserRegistry>> = OnceLock::new();

/// Get the global `ConfigParserRegistry` instance.
pub fn config_parser_registry() -> &'static Mutex<ConfigParserRegistry> {
    REGISTRY.get_or_init(|| Mutex::new(ConfigParserRegistry::new()))
}

/// Parse a `ModelConfig` using the global registry.
pub fn parse_model_config_json(
    architecture: &str,
    config: &Value,
    sample_rate: f64,
) -> Result<Box<dyn ModelConfig>, NamError> {
    let reg = config_parser_registry().lock().unwrap();
    reg.parse(architecture, config, sample_rate)
}
