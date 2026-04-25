use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use serde_json::Value;

use crate::dsp::error::NamError;

// =============================================================================
// Activation trait
// =============================================================================

/// Re-implementation of NAM C++ `nam::activations::Activation`.
///
/// Each activation type implements this trait.  The global registry stores
/// `Arc<dyn Activation>` so that `enable_fast_tanh()` can replace the
/// singleton "Tanh" entry with "Fasttanh" exactly like the reference.
pub trait Activation: Send + Sync + std::fmt::Debug {
    /// Apply the activation in-place to a flat array of floats.
    fn apply(&self, data: &mut [f32]);

    /// Convenience helper for a single sample.
    fn apply_sample(&self, x: f32) -> f32 {
        let mut buf = [x];
        self.apply(&mut buf);
        buf[0]
    }
}

// =============================================================================
// Low-level activation functions (match C++ constants exactly)
// =============================================================================

#[inline]
pub fn relu(x: f32) -> f32 {
    if x > 0.0 { x } else { 0.0 }
}

#[inline]
pub fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

#[inline]
pub fn hard_tanh(x: f32) -> f32 {
    x.clamp(-1.0, 1.0)
}

#[inline]
pub fn leaky_hardtanh(x: f32, min_val: f32, max_val: f32, min_slope: f32, max_slope: f32) -> f32 {
    if x < min_val {
        (x - min_val) * min_slope + min_val
    } else if x > max_val {
        (x - max_val) * max_slope + max_val
    } else {
        x
    }
}

/// Fast tanh approximation using the same coefficients as NAM C++.
#[inline]
pub fn fast_tanh(x: f32) -> f32 {
    let ax = x.abs();
    let x2 = x * x;
    let num = x * (2.455_507_5 + 2.455_507_5 * ax + (0.893_229_85 + 0.821_226_67 * ax) * x2);
    let den = 2.445_066_4 + (2.445_066_4 + x2) * (x + 0.814_642_7 * x * ax).abs();
    num / den
}

#[inline]
pub fn fast_sigmoid(x: f32) -> f32 {
    0.5 * (fast_tanh(x * 0.5) + 1.0)
}

#[inline]
pub fn leaky_relu(x: f32, negative_slope: f32) -> f32 {
    if x > 0.0 { x } else { negative_slope * x }
}

#[inline]
pub fn swish(x: f32) -> f32 {
    x * sigmoid(x)
}

#[inline]
pub fn hardswish(x: f32) -> f32 {
    let t = x + 3.0;
    let clamped = t.clamp(0.0, 6.0);
    x * clamped * (1.0 / 6.0)
}

#[inline]
pub fn softsign(x: f32) -> f32 {
    x / (1.0 + x.abs())
}

// =============================================================================
// Concrete activation types
// =============================================================================

#[derive(Debug)]
pub struct Tanh;
impl Activation for Tanh {
    fn apply(&self, data: &mut [f32]) {
        for x in data {
            *x = x.tanh();
        }
    }
}

#[derive(Debug)]
pub struct HardTanh;
impl Activation for HardTanh {
    fn apply(&self, data: &mut [f32]) {
        for x in data {
            *x = hard_tanh(*x);
        }
    }
}

#[derive(Debug)]
pub struct FastTanh;
impl Activation for FastTanh {
    fn apply(&self, data: &mut [f32]) {
        for x in data {
            *x = fast_tanh(*x);
        }
    }
}

#[derive(Debug)]
pub struct ReLU;
impl Activation for ReLU {
    fn apply(&self, data: &mut [f32]) {
        for x in data {
            *x = relu(*x);
        }
    }
}

#[derive(Debug, Clone)]
pub struct LeakyReLU {
    pub negative_slope: f32,
}

impl Default for LeakyReLU {
    fn default() -> Self {
        Self {
            negative_slope: 0.01,
        }
    }
}

impl Activation for LeakyReLU {
    fn apply(&self, data: &mut [f32]) {
        for x in data {
            *x = leaky_relu(*x, self.negative_slope);
        }
    }
}

#[derive(Debug, Clone)]
pub struct PReLU {
    pub negative_slopes: Vec<f32>,
}

impl Default for PReLU {
    fn default() -> Self {
        Self {
            negative_slopes: vec![0.01],
        }
    }
}

impl Activation for PReLU {
    fn apply(&self, data: &mut [f32]) {
        let n = self.negative_slopes.len();
        for (pos, x) in data.iter_mut().enumerate() {
            let slope = self.negative_slopes[pos % n];
            *x = leaky_relu(*x, slope);
        }
    }
}

#[derive(Debug)]
pub struct Sigmoid;
impl Activation for Sigmoid {
    fn apply(&self, data: &mut [f32]) {
        for x in data {
            *x = sigmoid(*x);
        }
    }
}

#[derive(Debug)]
pub struct SiLU;
impl Activation for SiLU {
    fn apply(&self, data: &mut [f32]) {
        for x in data {
            *x = swish(*x);
        }
    }
}

#[derive(Debug)]
pub struct HardSwish;
impl Activation for HardSwish {
    fn apply(&self, data: &mut [f32]) {
        for x in data {
            *x = hardswish(*x);
        }
    }
}

#[derive(Debug, Clone)]
pub struct LeakyHardTanh {
    pub min_val: f32,
    pub max_val: f32,
    pub min_slope: f32,
    pub max_slope: f32,
}

impl Default for LeakyHardTanh {
    fn default() -> Self {
        Self {
            min_val: -1.0,
            max_val: 1.0,
            min_slope: 0.01,
            max_slope: 0.01,
        }
    }
}

impl Activation for LeakyHardTanh {
    fn apply(&self, data: &mut [f32]) {
        for x in data {
            *x = leaky_hardtanh(
                *x,
                self.min_val,
                self.max_val,
                self.min_slope,
                self.max_slope,
            );
        }
    }
}

#[derive(Debug)]
pub struct Softsign;
impl Activation for Softsign {
    fn apply(&self, data: &mut [f32]) {
        for x in data {
            *x = softsign(*x);
        }
    }
}

use std::sync::Arc;

static REGISTRY: LazyLock<Mutex<HashMap<String, Arc<dyn Activation>>>> = LazyLock::new(|| {
    let mut map: HashMap<String, Arc<dyn Activation>> = HashMap::new();
    map.insert("Tanh".into(), Arc::new(Tanh));
    map.insert("Hardtanh".into(), Arc::new(HardTanh));
    map.insert("Fasttanh".into(), Arc::new(FastTanh));
    map.insert("ReLU".into(), Arc::new(ReLU));
    map.insert("LeakyReLU".into(), Arc::new(LeakyReLU::default()));
    map.insert("Sigmoid".into(), Arc::new(Sigmoid));
    map.insert("SiLU".into(), Arc::new(SiLU));
    map.insert("Hardswish".into(), Arc::new(HardSwish));
    map.insert("LeakyHardtanh".into(), Arc::new(LeakyHardTanh::default()));
    map.insert("LeakyHardTanh".into(), Arc::new(LeakyHardTanh::default()));
    map.insert("PReLU".into(), Arc::new(PReLU::default()));
    map.insert("Softsign".into(), Arc::new(Softsign));
    Mutex::new(map)
});

static TANH_BACKUP: LazyLock<Mutex<Option<Arc<dyn Activation>>>> =
    LazyLock::new(|| Mutex::new(None));

/// Look up a singleton activation by name from the global registry.
pub fn get_activation(name: &str) -> Option<Arc<dyn Activation>> {
    let reg = REGISTRY.lock().unwrap();
    reg.get(name).cloned()
}

/// Enable fast tanh globally by replacing the "Tanh" registry entry with
/// "Fasttanh", matching NAM C++ behavior.
pub fn enable_fast_tanh() {
    let mut reg = REGISTRY.lock().unwrap();
    let fast = reg
        .get("Fasttanh")
        .cloned()
        .unwrap_or_else(|| Arc::new(FastTanh));
    let current = reg.get("Tanh").cloned();
    let mut backup = TANH_BACKUP.lock().unwrap();
    if backup.is_none() {
        *backup = current;
    }
    reg.insert("Tanh".into(), fast);
}

/// Disable fast tanh, restoring the original "Tanh" entry.
pub fn disable_fast_tanh() {
    let mut reg = REGISTRY.lock().unwrap();
    let mut backup = TANH_BACKUP.lock().unwrap();
    if let Some(original) = backup.take() {
        reg.insert("Tanh".into(), original);
    }
}

/// Check whether the "Tanh" entry currently points to FastTanh.
pub fn is_fast_tanh_enabled() -> bool {
    let reg = REGISTRY.lock().unwrap();
    if let Some(tanh) = reg.get("Tanh")
        && let Some(fast) = reg.get("Fasttanh")
    {
        return Arc::ptr_eq(tanh, fast);
    }
    false
}

// =============================================================================
// JSON parsing
// =============================================================================

/// Parse an activation from JSON, either a simple string or an object with
/// parameters.  Returns a fresh `Arc<dyn Activation>` (parameterized instances
/// are allocated; singletons are cloned from the registry).
pub fn parse_activation(config: &Value) -> Result<Arc<dyn Activation>, NamError> {
    let name = if let Some(s) = config.as_str() {
        s.to_string()
    } else if let Some(obj) = config.as_object() {
        obj.get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| NamError::InvalidConfig("activation type missing".into()))?
            .to_string()
    } else {
        return Err(NamError::InvalidConfig(
            "activation must be a string or object".into(),
        ));
    };

    match name.as_str() {
        "Tanh" => Ok(get_activation("Tanh").unwrap_or_else(|| Arc::new(Tanh))),
        "Hardtanh" => Ok(get_activation("Hardtanh").unwrap_or_else(|| Arc::new(HardTanh))),
        "Fasttanh" => Ok(get_activation("Fasttanh").unwrap_or_else(|| Arc::new(FastTanh))),
        "ReLU" => Ok(get_activation("ReLU").unwrap_or_else(|| Arc::new(ReLU))),
        "LeakyReLU" => {
            let slope = config
                .get("negative_slope")
                .and_then(Value::as_f64)
                .map(|v| v as f32)
                .unwrap_or(0.01);
            Ok(Arc::new(LeakyReLU {
                negative_slope: slope,
            }))
        }
        "PReLU" => {
            if let Some(slopes) = config.get("negative_slopes") {
                let slopes: Vec<f32> = serde_json::from_value(slopes.clone())
                    .map_err(|e| NamError::InvalidConfig(format!("PReLU negative_slopes: {e}")))?;
                Ok(Arc::new(PReLU {
                    negative_slopes: slopes,
                }))
            } else {
                let slope = config
                    .get("negative_slope")
                    .and_then(Value::as_f64)
                    .map(|v| v as f32)
                    .unwrap_or(0.01);
                Ok(Arc::new(PReLU {
                    negative_slopes: vec![slope],
                }))
            }
        }
        "Sigmoid" => Ok(get_activation("Sigmoid").unwrap_or_else(|| Arc::new(Sigmoid))),
        "SiLU" => Ok(get_activation("SiLU").unwrap_or_else(|| Arc::new(SiLU))),
        "Hardswish" => Ok(get_activation("Hardswish").unwrap_or_else(|| Arc::new(HardSwish))),
        "LeakyHardtanh" | "LeakyHardTanh" => {
            let min_val = config
                .get("min_val")
                .and_then(Value::as_f64)
                .map(|v| v as f32)
                .unwrap_or(-1.0);
            let max_val = config
                .get("max_val")
                .and_then(Value::as_f64)
                .map(|v| v as f32)
                .unwrap_or(1.0);
            let min_slope = config
                .get("min_slope")
                .and_then(Value::as_f64)
                .map(|v| v as f32)
                .unwrap_or(0.01);
            let max_slope = config
                .get("max_slope")
                .and_then(Value::as_f64)
                .map(|v| v as f32)
                .unwrap_or(0.01);
            Ok(Arc::new(LeakyHardTanh {
                min_val,
                max_val,
                min_slope,
                max_slope,
            }))
        }
        "Softsign" => Ok(get_activation("Softsign").unwrap_or_else(|| Arc::new(Softsign))),
        _ => Err(NamError::InvalidConfig(format!(
            "unknown activation {name}"
        ))),
    }
}
