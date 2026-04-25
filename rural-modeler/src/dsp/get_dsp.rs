use std::path::Path;

use serde_json::Value;

use crate::dsp::core::Dsp;
use crate::dsp::error::NamError;
use crate::dsp::nam::{NamModel, ResamplingNamModel};
use crate::dsp::version::verify_config_version;

/// Re-implementation of NAM C++ `nam::dspData`.
///
/// Holds all information needed to instantiate and configure a DSP model.
#[derive(Debug, Clone)]
pub struct DspData {
    pub version: String,
    pub architecture: String,
    pub config: Value,
    pub metadata: Value,
    pub weights: Vec<f32>,
    pub expected_sample_rate: f64,
}

/// Load a `.nam` model from a file path and return a [`DspData`] struct.
///
/// This mirrors `nam::get_dsp(const std::filesystem::path, dspData&)`.
pub fn get_dsp_data(path: impl AsRef<Path>) -> Result<DspData, NamError> {
    let text = std::fs::read_to_string(path)?;
    let config: Value = serde_json::from_str(&text)?;

    let version = config["version"]
        .as_str()
        .ok_or_else(|| NamError::InvalidConfig("version missing".into()))?
        .to_string();
    verify_config_version(&version)?;

    let architecture = config["architecture"]
        .as_str()
        .ok_or_else(|| NamError::InvalidConfig("architecture missing".into()))?
        .to_string();

    let config_block = config["config"].clone();
    let metadata = config.get("metadata").cloned().unwrap_or(Value::Null);

    let weights: Vec<f32> = config["weights"]
        .as_array()
        .ok_or_else(|| NamError::InvalidConfig("weights missing".into()))?
        .iter()
        .filter_map(|v| v.as_f64().map(|f| f as f32))
        .collect();

    let expected_sample_rate = get_sample_rate_from_nam_file(&config);

    Ok(DspData {
        version,
        architecture,
        config: config_block,
        metadata,
        weights,
        expected_sample_rate,
    })
}

/// Get the sample rate from a NAM JSON object.
///
/// Matches C++ `nam::get_sample_rate_from_nam_file`.
pub fn get_sample_rate_from_nam_file(j: &Value) -> f64 {
    if let Some(rate) = j.get("sample_rate")
        && let Some(f) = rate.as_f64()
    {
        return f;
    }
    if let Some(meta) = j.get("metadata")
        && let Some(rate) = meta.get("sample_rate")
        && let Some(f) = rate.as_f64()
    {
        return f;
    }
    -1.0
}

/// Load a `.nam` model from a file path and return it as a boxed [`Dsp`].
///
/// This is the primary entry point matching `nam::get_dsp(path)`.
pub fn get_dsp(path: impl AsRef<Path>) -> Result<Box<dyn Dsp>, NamError> {
    let model = NamModel::load(path)?;
    Ok(Box::new(model))
}

/// Load a `.nam` model from a JSON string and return it as a boxed [`Dsp`].
///
/// Matches `nam::get_dsp(const nlohmann::json&)`.
pub fn get_dsp_from_json(text: &str) -> Result<Box<dyn Dsp>, NamError> {
    let model = NamModel::load_from_str(text)?;
    Ok(Box::new(model))
}

/// Wrap a [`NamModel`] in a [`ResamplingNamModel`] for transparent sample-rate
/// conversion.  Returns a boxed [`Dsp`].
pub fn get_resampling_dsp(
    path: impl AsRef<Path>,
    host_rate: f32,
) -> Result<Box<dyn Dsp>, NamError> {
    let model = NamModel::load(path)?;
    Ok(Box::new(ResamplingNamModel::new(model, host_rate)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn get_dsp_from_json_loads_linear_model() {
        let json = serde_json::to_string(&json!({
            "version": "0.7.0",
            "architecture": "Linear",
            "config": {
                "receptive_field": 1,
                "bias": false,
                "in_channels": 1,
                "out_channels": 1
            },
            "weights": [1.0],
            "sample_rate": 48_000.0
        }))
        .unwrap();

        let mut dsp = get_dsp_from_json(&json).expect("should load");
        assert_eq!(dsp.num_input_channels(), 1);
        assert_eq!(dsp.num_output_channels(), 1);
        assert_eq!(dsp.expected_sample_rate(), Some(48_000.0));

        let mut out = [0.0f32];
        dsp.process_block(&[0.5], &mut out);
        assert!(
            (out[0] - 0.5).abs() < 1.0e-6,
            "expected identity linear model"
        );
    }

    #[test]
    fn get_dsp_data_extracts_fields() {
        let json = serde_json::to_string(&json!({
            "version": "0.7.0",
            "architecture": "Linear",
            "config": { "receptive_field": 1, "bias": false },
            "weights": [1.0],
            "sample_rate": 48_000.0,
            "metadata": { "loudness": -18.0 }
        }))
        .unwrap();

        let data = get_dsp_data_from_str(&json).expect("should parse");
        assert_eq!(data.version, "0.7.0");
        assert_eq!(data.architecture, "Linear");
        assert_eq!(data.expected_sample_rate, 48_000.0);
        assert_eq!(data.metadata["loudness"], -18.0);
    }

    #[test]
    fn get_sample_rate_from_top_level() {
        let j = json!({ "sample_rate": 44_100.0 });
        assert_eq!(get_sample_rate_from_nam_file(&j), 44_100.0);
    }

    #[test]
    fn get_sample_rate_from_metadata() {
        let j = json!({ "metadata": { "sample_rate": 48_000.0 } });
        assert_eq!(get_sample_rate_from_nam_file(&j), 48_000.0);
    }

    #[test]
    fn get_sample_rate_returns_minus_one_when_missing() {
        let j = json!({});
        assert_eq!(get_sample_rate_from_nam_file(&j), -1.0);
    }

    fn get_dsp_data_from_str(text: &str) -> Result<DspData, NamError> {
        let config: Value = serde_json::from_str(text)?;

        let version = config["version"]
            .as_str()
            .ok_or_else(|| NamError::InvalidConfig("version missing".into()))?
            .to_string();
        verify_config_version(&version)?;

        let architecture = config["architecture"]
            .as_str()
            .ok_or_else(|| NamError::InvalidConfig("architecture missing".into()))?
            .to_string();

        let config_block = config["config"].clone();
        let metadata = config.get("metadata").cloned().unwrap_or(Value::Null);

        let weights: Vec<f32> = config["weights"]
            .as_array()
            .ok_or_else(|| NamError::InvalidConfig("weights missing".into()))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        let expected_sample_rate = get_sample_rate_from_nam_file(&config);

        Ok(DspData {
            version,
            architecture,
            config: config_block,
            metadata,
            weights,
            expected_sample_rate,
        })
    }
}
