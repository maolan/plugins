use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::Value;

use crate::rural_modeler::dsp::core::Dsp;
use crate::rural_modeler::dsp::error::NamError;


pub trait ModelConfig: std::fmt::Debug + Send {


    fn create(&self, weights: Vec<f32>, sample_rate: f64) -> Result<Box<dyn Dsp>, NamError>;
}


pub type ConfigParserFunction =
    Box<dyn Fn(&Value, f64) -> Result<Box<dyn ModelConfig>, NamError> + Send + Sync>;


pub struct ConfigParserRegistry {
    parsers: HashMap<String, ConfigParserFunction>,
}

impl ConfigParserRegistry {
    pub fn new() -> Self {
        Self {
            parsers: HashMap::new(),
        }
    }


    pub fn register(&mut self, name: &str, parser: ConfigParserFunction) {
        if self.parsers.contains_key(name) {
            panic!("Config parser already registered for: {name}");
        }
        self.parsers.insert(name.to_string(), parser);
    }


    pub fn has(&self, name: &str) -> bool {
        self.parsers.contains_key(name)
    }


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


use std::sync::OnceLock;

static REGISTRY: OnceLock<Mutex<ConfigParserRegistry>> = OnceLock::new();


pub fn config_parser_registry() -> &'static Mutex<ConfigParserRegistry> {
    REGISTRY.get_or_init(|| Mutex::new(ConfigParserRegistry::new()))
}


pub fn parse_model_config_json(
    architecture: &str,
    config: &Value,
    sample_rate: f64,
) -> Result<Box<dyn ModelConfig>, NamError> {
    let reg = config_parser_registry().lock().unwrap();
    reg.parse(architecture, config, sample_rate)
}
