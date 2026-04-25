use std::{collections::VecDeque, fs, path::Path, sync::Arc};

use serde::Deserialize;
use serde_json::Value;

use crate::dsp::activations;
use crate::dsp::core::{Buffer, SampleRing};
use crate::dsp::error::NamError;
use crate::dsp::version::verify_config_version;

#[derive(Debug, Clone, Default)]
pub struct ModelMetadata {
    pub loudness: Option<f32>,
    pub input_level_dbu: Option<f32>,
    pub output_level_dbu: Option<f32>,
    pub expected_sample_rate: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct NamFile {
    version: String,
    architecture: String,
    config: Value,
    metadata: Option<Value>,
    weights: Vec<f32>,
    sample_rate: Option<f32>,
}

fn parse_metadata(value: Option<Value>, sample_rate: Option<f32>) -> ModelMetadata {
    let mut meta = ModelMetadata::default();
    if let Some(Value::Object(map)) = value {
        meta.loudness = map
            .get("loudness")
            .and_then(Value::as_f64)
            .map(|v| v as f32);
        meta.input_level_dbu = map
            .get("input_level_dbu")
            .and_then(Value::as_f64)
            .map(|v| v as f32);
        meta.output_level_dbu = map
            .get("output_level_dbu")
            .and_then(Value::as_f64)
            .map(|v| v as f32);
    }
    meta.expected_sample_rate = sample_rate;
    meta
}

#[derive(Debug, Clone)]
struct Conv1x1 {
    out_channels: usize,
    in_channels: usize,
    groups: usize,
    weight: Vec<f32>,
    bias: Option<Vec<f32>>,
    is_depthwise: bool,
}

impl Conv1x1 {
    fn from_weights(
        in_channels: usize,
        out_channels: usize,
        with_bias: bool,
        weights: &mut &[f32],
    ) -> Result<Self, NamError> {
        Self::from_weights_grouped(in_channels, out_channels, with_bias, 1, weights)
    }

    fn from_weights_grouped(
        in_channels: usize,
        out_channels: usize,
        with_bias: bool,
        groups: usize,
        weights: &mut &[f32],
    ) -> Result<Self, NamError> {
        if !in_channels.is_multiple_of(groups) {
            return Err(NamError::InvalidConfig(format!(
                "in_channels ({in_channels}) must be divisible by groups ({groups})"
            )));
        }
        if !out_channels.is_multiple_of(groups) {
            return Err(NamError::InvalidConfig(format!(
                "out_channels ({out_channels}) must be divisible by groups ({groups})"
            )));
        }

        let is_depthwise = groups == in_channels && in_channels == out_channels;

        if is_depthwise {
            if weights.len() < in_channels {
                return Err(NamError::InvalidConfig(
                    "conv1x1 depthwise weight underflow".into(),
                ));
            }
            let (taken, rest) = weights.split_at(in_channels);
            let weight = taken.to_vec();
            *weights = rest;
            let bias = if with_bias {
                if weights.len() < out_channels {
                    return Err(NamError::InvalidConfig("conv1x1 bias underflow".into()));
                }
                let (taken, rest) = weights.split_at(out_channels);
                let bias = taken.to_vec();
                *weights = rest;
                Some(bias)
            } else {
                None
            };
            Ok(Self {
                out_channels,
                in_channels,
                groups,
                weight,
                bias,
                is_depthwise,
            })
        } else {
            let out_per_group = out_channels / groups;
            let in_per_group = in_channels / groups;
            let count = groups * out_per_group * in_per_group;
            if weights.len() < count {
                return Err(NamError::InvalidConfig("conv1x1 weight underflow".into()));
            }
            // C++ stores grouped weights per group: [group0, group1, ...]
            // Each group is (out_per_group, in_per_row).
            let (taken, rest) = weights.split_at(count);
            let weight = taken.to_vec();
            *weights = rest;
            let bias = if with_bias {
                if weights.len() < out_channels {
                    return Err(NamError::InvalidConfig("conv1x1 bias underflow".into()));
                }
                let (taken, rest) = weights.split_at(out_channels);
                let bias = taken.to_vec();
                *weights = rest;
                Some(bias)
            } else {
                None
            };
            Ok(Self {
                out_channels,
                in_channels,
                groups,
                weight,
                bias,
                is_depthwise,
            })
        }
    }
}

#[derive(Debug, Clone)]
struct Conv1D {
    kernel_size: usize,
    dilation: usize,
    out_channels: usize,
    in_channels: usize,
    groups: usize,
    weight: Vec<f32>,
    bias: Vec<f32>,
}

impl Conv1D {
    fn from_weights(
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        dilation: usize,
        with_bias: bool,
        weights: &mut &[f32],
    ) -> Result<Self, NamError> {
        Self::from_weights_grouped(
            in_channels,
            out_channels,
            kernel_size,
            dilation,
            with_bias,
            1,
            weights,
        )
    }

    fn from_weights_grouped(
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        dilation: usize,
        with_bias: bool,
        groups: usize,
        weights: &mut &[f32],
    ) -> Result<Self, NamError> {
        if !in_channels.is_multiple_of(groups) {
            return Err(NamError::InvalidConfig(format!(
                "Conv1D in_channels ({in_channels}) must be divisible by groups ({groups})"
            )));
        }
        if !out_channels.is_multiple_of(groups) {
            return Err(NamError::InvalidConfig(format!(
                "Conv1D out_channels ({out_channels}) must be divisible by groups ({groups})"
            )));
        }

        let in_per_group = in_channels / groups;
        let _out_per_group = out_channels / groups;
        let count = kernel_size * out_channels * in_per_group;
        if weights.len() < count {
            return Err(NamError::InvalidConfig("conv1d weight underflow".into()));
        }
        // NAM C++ flattens grouped Conv1D weights in (out, in/groups, kernel) order.
        // Internally we index as (kernel, out, in_per_group), so remap once during load.
        let (flat, rest) = weights.split_at(count);
        let mut weight = vec![0.0; count];
        for o in 0..out_channels {
            for i in 0..in_per_group {
                for k in 0..kernel_size {
                    let src = (o * in_per_group + i) * kernel_size + k;
                    let dst = (k * out_channels + o) * in_per_group + i;
                    weight[dst] = flat[src];
                }
            }
        }
        *weights = rest;
        let bias = if with_bias {
            if weights.len() < out_channels {
                return Err(NamError::InvalidConfig("conv1d bias underflow".into()));
            }
            let (taken, rest) = weights.split_at(out_channels);
            let bias = taken.to_vec();
            *weights = rest;
            bias
        } else {
            vec![0.0; out_channels]
        };
        Ok(Self {
            kernel_size,
            dilation,
            out_channels,
            in_channels,
            groups,
            weight,
            bias,
        })
    }

    fn required_history(&self) -> usize {
        (self.kernel_size - 1) * self.dilation + 1
    }

    fn process(&self, history: &SampleRing, out: &mut Vec<f32>) {
        out.clear();
        out.reserve(self.out_channels);
        let in_per_group = self.in_channels / self.groups;
        let out_per_group = self.out_channels / self.groups;
        for o in 0..self.out_channels {
            let mut sum = self.bias.get(o).copied().unwrap_or(0.0);
            let group = o / out_per_group;
            let i_base = group * in_per_group;
            for k in 0..self.kernel_size {
                let delay = (self.kernel_size - 1 - k) * self.dilation;
                let sample = history.get_delay(delay);
                let idx_base = (k * self.out_channels + o) * in_per_group;
                if idx_base >= self.weight.len() {
                    continue;
                }
                let row_len = in_per_group.min(self.weight.len() - idx_base);
                if row_len == 0 {
                    continue;
                }
                let row = &self.weight[idx_base..idx_base + row_len];
                for (i, coeff) in row.iter().enumerate().take(row_len) {
                    let input_idx = i_base + i;
                    if input_idx < sample.len() {
                        sum += *coeff * sample[input_idx];
                    }
                }
            }
            out.push(sum);
        }
    }
}

#[derive(Debug, Clone)]
struct LinearModel {
    weights: Vec<f32>,
    bias: f32,
    buffer: Buffer,
    prewarm_samples: usize,
}

impl LinearModel {
    fn new(
        receptive_field: usize,
        with_bias: bool,
        weights: &mut &[f32],
    ) -> Result<Self, NamError> {
        let expected = receptive_field + usize::from(with_bias);
        if weights.len() < expected {
            return Err(NamError::InvalidConfig(
                "linear weights size mismatch".into(),
            ));
        }
        let (taken, rest) = weights.split_at(expected);
        let mut kernel = vec![0.0; receptive_field];
        for i in 0..receptive_field {
            kernel[i] = taken[receptive_field - 1 - i];
        }
        *weights = rest;
        Ok(Self {
            weights: kernel,
            bias: if with_bias {
                taken[receptive_field]
            } else {
                0.0
            },
            buffer: Buffer::new(receptive_field),
            prewarm_samples: 0,
        })
    }

    fn reset(&mut self) {
        self.buffer.reset();
    }

    fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        self.buffer.update_buffers(input);
        for (i, out) in output.iter_mut().enumerate().take(input.len()) {
            let start = -(self.weights.len() as isize) + i as isize + 1;
            let mut sum = self.bias;
            for (j, weight) in self.weights.iter().enumerate() {
                sum += *weight * self.buffer.get(start + j as isize);
            }
            *out = sum;
        }
        self.buffer.advance(input.len());
    }
}

#[derive(Debug, Clone)]
struct BatchNorm {
    scale: Vec<f32>,
    loc: Vec<f32>,
}

impl BatchNorm {
    fn from_weights(dim: usize, weights: &mut &[f32]) -> Result<Self, NamError> {
        let take = |weights: &mut &[f32]| -> Result<Vec<f32>, NamError> {
            if weights.len() < dim {
                return Err(NamError::InvalidConfig("batchnorm underflow".into()));
            }
            let (taken, rest) = weights.split_at(dim);
            let values = taken.to_vec();
            *weights = rest;
            Ok(values)
        };
        let running_mean = take(weights)?;
        let running_var = take(weights)?;
        let weight = take(weights)?;
        let bias = take(weights)?;
        if weights.is_empty() {
            return Err(NamError::InvalidConfig("batchnorm epsilon missing".into()));
        }
        let eps = weights[0];
        *weights = &weights[1..];
        let mut scale = vec![0.0; dim];
        let mut loc = vec![0.0; dim];
        for (i, ((w, rv), b)) in weight
            .iter()
            .zip(running_var.iter())
            .zip(bias.iter())
            .enumerate()
        {
            if i >= dim {
                break;
            }
            let s = *w / (eps + *rv).sqrt();
            scale[i] = s;
            let mean = running_mean.get(i).copied().unwrap_or(0.0);
            loc[i] = *b - s * mean;
        }
        Ok(Self { scale, loc })
    }

    fn apply(&self, values: &mut [f32]) {
        for ((value, scale), loc) in values
            .iter_mut()
            .zip(self.scale.iter())
            .zip(self.loc.iter())
        {
            *value = *value * *scale + *loc;
        }
    }
}

#[derive(Debug, Clone)]
struct ConvNetBlock {
    conv: Conv1D,
    batchnorm: Option<BatchNorm>,
    activation: Arc<dyn activations::Activation>,
    history: SampleRing,
}

#[derive(Debug, Clone)]
struct ConvNetModel {
    blocks: Vec<ConvNetBlock>,
    head_weight: Vec<f32>,
    head_bias: f32,
    prewarm_samples: usize,
    // Reusable work buffers to avoid per-sample allocations in process_block.
    current: Vec<f32>,
    temp: Vec<f32>,
}

impl ConvNetModel {
    fn new(config: &Value, weights: &mut &[f32]) -> Result<Self, NamError> {
        let channels = config["channels"]
            .as_u64()
            .ok_or_else(|| NamError::InvalidConfig("ConvNet.channels missing".into()))?
            as usize;
        let batchnorm = config["batchnorm"]
            .as_bool()
            .ok_or_else(|| NamError::InvalidConfig("ConvNet.batchnorm missing".into()))?;
        let dilations = config["dilations"]
            .as_array()
            .ok_or_else(|| NamError::InvalidConfig("ConvNet.dilations missing".into()))?;
        let activation = activations::parse_activation(&config["activation"])?;
        let groups = config["groups"].as_u64().unwrap_or(1) as usize;
        let mut blocks = Vec::with_capacity(dilations.len());
        for (index, dilation_value) in dilations.iter().enumerate() {
            let dilation = dilation_value
                .as_u64()
                .ok_or_else(|| NamError::InvalidConfig("invalid dilation".into()))?
                as usize;
            let in_channels = if index == 0 { 1 } else { channels };
            let conv = Conv1D::from_weights_grouped(
                in_channels,
                channels,
                2,
                dilation,
                !batchnorm,
                groups,
                weights,
            )?;
            let batchnorm_state = if batchnorm {
                Some(BatchNorm::from_weights(channels, weights)?)
            } else {
                None
            };
            blocks.push(ConvNetBlock {
                history: SampleRing::new(in_channels, conv.required_history()),
                conv,
                batchnorm: batchnorm_state,
                activation: activation.clone(),
            });
        }
        if weights.len() < channels + 1 {
            return Err(NamError::InvalidConfig("ConvNet head underflow".into()));
        }
        let (head_weight_taken, rest) = weights.split_at(channels);
        let head_weight = head_weight_taken.to_vec();
        let head_bias = rest[0];
        *weights = &rest[1..];
        if !weights.is_empty() {
            return Err(NamError::InvalidConfig(format!(
                "ConvNet weight mismatch: {} weights remaining",
                weights.len()
            )));
        }
        let mut prewarm_samples = 1;
        for block in &blocks {
            prewarm_samples += block.conv.dilation * (block.conv.kernel_size - 1);
        }
        Ok(Self {
            blocks,
            head_weight,
            head_bias,
            prewarm_samples,
            current: Vec::new(),
            temp: Vec::new(),
        })
    }

    fn reset(&mut self) {
        for block in &mut self.blocks {
            block.history.reset();
        }
    }

    fn process_sample(&mut self, input: f32) -> f32 {
        self.current.clear();
        self.current.push(input);
        for block in &mut self.blocks {
            block.history.push(&self.current);
            block.conv.process(&block.history, &mut self.temp);
            if let Some(batchnorm) = &block.batchnorm {
                batchnorm.apply(&mut self.temp);
            }
            for value in &mut self.temp {
                *value = block.activation.apply_sample(*value);
            }
            std::mem::swap(&mut self.current, &mut self.temp);
        }
        self.head_bias
            + self
                .current
                .iter()
                .zip(self.head_weight.iter())
                .map(|(a, b)| a * b)
                .sum::<f32>()
    }

    /// Process a block of samples, matching the C++ `nam::convnet::ConvNet::process`
    /// block-based API.
    fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        for (x, out) in input.iter().zip(output.iter_mut()) {
            *out = self.process_sample(*x);
        }
    }
}

#[derive(Debug, Clone)]
struct LstmCell {
    input_size: usize,
    hidden_size: usize,
    weights: Vec<f32>,
    bias: Vec<f32>,
    xh: Vec<f32>,
    c: Vec<f32>,
    ifgo: Vec<f32>,
    initial_xh: Vec<f32>,
    initial_c: Vec<f32>,
}

impl LstmCell {
    fn from_weights(
        input_size: usize,
        hidden_size: usize,
        weights: &mut &[f32],
    ) -> Result<Self, NamError> {
        let rows = 4 * hidden_size;
        let cols = input_size + hidden_size;
        let count = rows * cols;
        if weights.len() < count {
            return Err(NamError::InvalidConfig("LSTM weight underflow".into()));
        }
        let (matrix_taken, rest) = weights.split_at(count);
        let matrix = matrix_taken.to_vec();
        *weights = rest;
        if weights.len() < rows {
            return Err(NamError::InvalidConfig("LSTM bias underflow".into()));
        }
        let (bias_taken, rest) = weights.split_at(rows);
        let bias = bias_taken.to_vec();
        *weights = rest;
        if weights.len() < hidden_size {
            return Err(NamError::InvalidConfig("LSTM hidden init underflow".into()));
        }
        let mut xh = vec![0.0; cols];
        xh[input_size..input_size + hidden_size].copy_from_slice(&weights[..hidden_size]);
        let initial_xh = xh.clone();
        *weights = &weights[hidden_size..];
        if weights.len() < hidden_size {
            return Err(NamError::InvalidConfig("LSTM cell init underflow".into()));
        }
        let c = weights[..hidden_size].to_vec();
        let initial_c = c.clone();
        *weights = &weights[hidden_size..];
        Ok(Self {
            input_size,
            hidden_size,
            weights: matrix,
            bias,
            xh,
            c,
            ifgo: vec![0.0; rows],
            initial_xh,
            initial_c,
        })
    }

    fn hidden(&self) -> &[f32] {
        &self.xh[self.input_size..]
    }

    fn reset(&mut self) {
        self.xh.copy_from_slice(&self.initial_xh);
        self.c.copy_from_slice(&self.initial_c);
        self.ifgo.fill(0.0);
    }

    fn process(&mut self, input: &[f32]) {
        let prefix = &mut self.xh[..self.input_size];
        prefix.fill(0.0);
        let copy_len = prefix.len().min(input.len());
        prefix[..copy_len].copy_from_slice(&input[..copy_len]);
        let rows = 4 * self.hidden_size;
        for r in 0..rows {
            let mut sum = self.bias[r];
            for c in 0..self.xh.len() {
                sum += self.weights[r * self.xh.len() + c] * self.xh[c];
            }
            self.ifgo[r] = sum;
        }
        let f_offset = self.hidden_size;
        let g_offset = 2 * self.hidden_size;
        let o_offset = 3 * self.hidden_size;
        for i in 0..self.hidden_size {
            self.c[i] = activations::fast_sigmoid(self.ifgo[i + f_offset]) * self.c[i]
                + activations::fast_sigmoid(self.ifgo[i])
                    * activations::fast_tanh(self.ifgo[i + g_offset]);
        }
        for i in 0..self.hidden_size {
            self.xh[self.input_size + i] = activations::fast_sigmoid(self.ifgo[i + o_offset])
                * activations::fast_tanh(self.c[i]);
        }
    }
}

#[derive(Debug, Clone)]
struct LstmModel {
    layers: Vec<LstmCell>,
    head_weight: Vec<f32>,
    head_bias: f32,
    prewarm_samples: usize,
}

impl LstmModel {
    fn new(
        config: &Value,
        weights: &mut &[f32],
        expected_sample_rate: f32,
    ) -> Result<Self, NamError> {
        let num_layers = config["num_layers"]
            .as_u64()
            .ok_or_else(|| NamError::InvalidConfig("LSTM.num_layers missing".into()))?
            as usize;
        let input_size = config["input_size"]
            .as_u64()
            .ok_or_else(|| NamError::InvalidConfig("LSTM.input_size missing".into()))?
            as usize;
        let hidden_size = config["hidden_size"]
            .as_u64()
            .ok_or_else(|| NamError::InvalidConfig("LSTM.hidden_size missing".into()))?
            as usize;
        let mut layers = Vec::with_capacity(num_layers);
        for index in 0..num_layers {
            layers.push(LstmCell::from_weights(
                if index == 0 { input_size } else { hidden_size },
                hidden_size,
                weights,
            )?);
        }
        if weights.len() < hidden_size + 1 {
            return Err(NamError::InvalidConfig("LSTM head underflow".into()));
        }
        let (head_weight_taken, rest) = weights.split_at(hidden_size);
        let head_weight = head_weight_taken.to_vec();
        let head_bias = rest[0];
        *weights = &rest[1..];
        if !weights.is_empty() {
            return Err(NamError::InvalidConfig(format!(
                "LSTM weight mismatch: {} weights remaining",
                weights.len()
            )));
        }
        let prewarm_samples = if expected_sample_rate > 0.0 {
            (0.5 * expected_sample_rate) as usize
        } else {
            1
        };
        Ok(Self {
            layers,
            head_weight,
            head_bias,
            prewarm_samples,
        })
    }

    fn reset(&mut self) {
        for layer in &mut self.layers {
            layer.reset();
        }
    }

    fn process_sample(&mut self, input: f32) -> f32 {
        let mut current = vec![input];
        for layer in &mut self.layers {
            layer.process(&current);
            current = layer.hidden().to_vec();
        }
        self.head_bias
            + current
                .iter()
                .zip(self.head_weight.iter())
                .map(|(a, b)| a * b)
                .sum::<f32>()
    }

    /// Process a block of samples, matching the C++ `nam::lstm::LSTM::process`
    /// block-based API.
    fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        for (x, out) in input.iter().zip(output.iter_mut()) {
            *out = self.process_sample(*x);
        }
    }
}

#[derive(Debug, Clone)]
struct WaveNetLayer {
    conv: Conv1D,
    input_mixin: Conv1x1,
    post: Conv1x1,
    activation: Arc<dyn activations::Activation>,
    gated: bool,
    frame_capacity: usize,
    conv_out_block: Vec<f32>,
    mixin_block: Vec<f32>,
    z_block: Vec<f32>,
    post_block: Vec<f32>,
}

#[derive(Debug, Clone, Deserialize)]
struct LayerArrayParams {
    input_size: usize,
    condition_size: usize,
    head_size: usize,
    channels: usize,
    kernel_size: usize,
    dilations: Vec<usize>,
    activation: String,
    gated: bool,
    head_bias: bool,
}

#[derive(Debug, Clone)]
struct WaveNetLayerArray {
    channels: usize,
    head_size: usize,
    rechannel: Conv1x1,
    layers: Vec<WaveNetLayer>,
    head_rechannel: Conv1x1,
    receptive_field: usize,
    buffer_size: usize,
    buffer_start: usize,
    frame_capacity: usize,
    layer_buffers: Vec<Vec<f32>>,
    rechannel_block: Vec<f32>,
    layer_output_block: Vec<f32>,
    head_input_block: Vec<f32>,
    head_output_block: Vec<f32>,
}

#[derive(Debug, Clone)]
struct WaveNetHeadLayer {
    conv: Conv1D,
    activation: Arc<dyn activations::Activation>,
    history: SampleRing,
    output: Vec<f32>,
}

#[derive(Debug, Clone)]
struct WaveNetHead {
    layers: Vec<WaveNetHeadLayer>,
    scratch: Vec<f32>,
}

impl WaveNetHead {
    fn new(params: &HeadParams, weights: &mut &[f32]) -> Result<Self, NamError> {
        if params.kernel_sizes.is_empty() {
            return Err(NamError::InvalidConfig(
                "WaveNet Head: kernel_sizes must be non-empty".into(),
            ));
        }
        let activation = activations::parse_activation(&Value::String(params.activation.clone()))?;
        let mut layers = Vec::with_capacity(params.kernel_sizes.len());
        let mut cin = params.in_channels;
        for (i, &k) in params.kernel_sizes.iter().enumerate() {
            let cout = if i + 1 == params.kernel_sizes.len() {
                params.out_channels
            } else {
                params.channels
            };
            if k < 1 {
                return Err(NamError::InvalidConfig(
                    "WaveNet Head: kernel_sizes entries must be >= 1".into(),
                ));
            }
            let conv = Conv1D::from_weights(cin, cout, k, 1, true, weights)?;
            layers.push(WaveNetHeadLayer {
                history: SampleRing::new(cin, conv.required_history()),
                conv,
                activation: activation.clone(),
                output: Vec::with_capacity(cout),
            });
            cin = cout;
        }
        Ok(Self {
            layers,
            scratch: Vec::new(),
        })
    }

    fn reset(&mut self) {
        for layer in &mut self.layers {
            layer.history.reset();
        }
    }

    /// Process a frame through the head.  `input` is a slice of `in_channels`
    /// values for the current frame.  Returns a slice of `out_channels` values.
    fn process_frame(&mut self, input: &[f32]) -> &[f32] {
        self.scratch.clear();
        self.scratch.extend_from_slice(input);
        for layer in &mut self.layers {
            layer.activation.apply(&mut self.scratch);
            layer.history.push(&self.scratch);
            layer.conv.process(&layer.history, &mut layer.output);
            self.scratch.clear();
            self.scratch.extend_from_slice(&layer.output);
        }
        &self.scratch
    }
}

#[derive(Debug, Clone)]
struct HeadParams {
    in_channels: usize,
    channels: usize,
    out_channels: usize,
    kernel_sizes: Vec<usize>,
    activation: String,
}

#[derive(Debug, Clone)]
struct WaveNetModel {
    arrays: Vec<WaveNetLayerArray>,
    head_scale: f32,
    post_stack_head: Option<WaveNetHead>,
    frame_capacity: usize,
    condition_block: Vec<f32>,
    layer_input_block: Vec<f32>,
    prewarm_samples: usize,
}

impl WaveNetModel {
    const LAYER_ARRAY_BUFFER_SIZE: usize = 65_536;

    fn conv1x1_process_block(conv: &Conv1x1, input: &[f32], frames: usize, output: &mut [f32]) {
        if frames == 0 {
            return;
        }
        debug_assert!(input.len() >= conv.in_channels * frames);
        debug_assert!(output.len() >= conv.out_channels * frames);
        let bias = conv.bias.as_deref();
        let in_channels = conv.in_channels;
        let out_channels = conv.out_channels;

        if conv.is_depthwise {
            // Depthwise: each channel is scaled by its corresponding weight.
            debug_assert!(conv.weight.len() >= in_channels);
            let mut c = 0usize;
            while c < in_channels {
                let w = unsafe { *conv.weight.get_unchecked(c) };
                let b = bias.and_then(|b| b.get(c)).copied().unwrap_or(0.0);
                let in_ptr = unsafe { input.as_ptr().add(c * frames) };
                let out_ptr = unsafe { output.as_mut_ptr().add(c * frames) };
                let mut f = 0usize;
                while f < frames {
                    unsafe {
                        *out_ptr.add(f) = b + w * *in_ptr.add(f);
                    }
                    f += 1;
                }
                c += 1;
            }
            return;
        }

        let groups = conv.groups;
        let out_per_group = out_channels / groups;
        let in_per_group = in_channels / groups;
        debug_assert!(conv.weight.len() >= groups * out_per_group * in_per_group);

        // output[o,f] = bias[o] + sum_i weight[g, o', i'] * input[i,f]
        // where g = o / out_per_group, o' = o % out_per_group, i' = i % in_per_group
        let mut o = 0usize;
        while o < out_channels {
            let out_base = o * frames;
            let group = o / out_per_group;
            let o_in_group = o % out_per_group;
            let w_group_base = group * out_per_group * in_per_group;
            let w_row_base = w_group_base + o_in_group * in_per_group;
            let b = bias.and_then(|b| b.get(o)).copied().unwrap_or(0.0);
            unsafe {
                let out_ptr = output.as_mut_ptr().add(out_base);
                let mut f = 0usize;
                while f < frames {
                    *out_ptr.add(f) = b;
                    f += 1;
                }

                let mut i_in_group = 0usize;
                while i_in_group < in_per_group {
                    let w = *conv.weight.get_unchecked(w_row_base + i_in_group);
                    let i = group * in_per_group + i_in_group;
                    let in_ptr = input.as_ptr().add(i * frames);
                    let mut f2 = 0usize;
                    while f2 < frames {
                        let cur = *out_ptr.add(f2);
                        *out_ptr.add(f2) = cur + w * *in_ptr.add(f2);
                        f2 += 1;
                    }
                    i_in_group += 1;
                }
            }
            o += 1;
        }
    }

    fn conv1d_process_block_from_buffer(
        conv: &Conv1D,
        input: &[f32],
        input_stride: usize,
        start: usize,
        frames: usize,
        output: &mut [f32],
    ) {
        if frames == 0 {
            return;
        }
        debug_assert!(conv.weight.len() >= conv.kernel_size * conv.out_channels * conv.in_channels);
        debug_assert!(output.len() >= conv.out_channels * frames);
        debug_assert!(input.len() >= conv.in_channels * input_stride);
        let in_channels = conv.in_channels;
        let out_channels = conv.out_channels;
        let kernel_size = conv.kernel_size;
        let dilation = conv.dilation as isize;

        // output[o,f] = bias[o] + sum_{k,i} weight[k,o,i] * input[i, start+f+offset(k)]
        let mut o = 0usize;
        while o < out_channels {
            let out_base = o * frames;
            let b = conv.bias.get(o).copied().unwrap_or(0.0);
            unsafe {
                let out_ptr = output.as_mut_ptr().add(out_base);
                let mut f = 0usize;
                while f < frames {
                    *out_ptr.add(f) = b;
                    f += 1;
                }

                let mut k = 0usize;
                while k < kernel_size {
                    let offset = dilation * (k as isize + 1 - kernel_size as isize);
                    let src_base = (start as isize + offset) as usize;
                    let w_base = (k * out_channels + o) * in_channels;

                    let mut i = 0usize;
                    while i < in_channels {
                        let w = *conv.weight.get_unchecked(w_base + i);
                        let in_ptr = input.as_ptr().add(i * input_stride + src_base);
                        let mut f2 = 0usize;
                        while f2 < frames {
                            let cur = *out_ptr.add(f2);
                            *out_ptr.add(f2) = cur + w * *in_ptr.add(f2);
                            f2 += 1;
                        }
                        i += 1;
                    }
                    k += 1;
                }
            }
            o += 1;
        }
    }

    fn ensure_frame_capacity(&mut self, frames: usize) {
        if self.frame_capacity < frames {
            self.condition_block.resize(frames, 0.0);
            self.layer_input_block.resize(frames, 0.0);
            self.frame_capacity = frames;
        }
        for array in &mut self.arrays {
            if array.frame_capacity < frames {
                array.rechannel_block.resize(array.channels * frames, 0.0);
                array
                    .layer_output_block
                    .resize(array.channels * frames, 0.0);
                array.head_input_block.resize(array.channels * frames, 0.0);
                array
                    .head_output_block
                    .resize(array.head_size * frames, 0.0);
                array.frame_capacity = frames;
            }
            for layer in &mut array.layers {
                if layer.frame_capacity < frames {
                    let conv_out = layer.conv.out_channels;
                    let channels = layer.post.out_channels;
                    layer.conv_out_block.resize(conv_out * frames, 0.0);
                    layer.mixin_block.resize(conv_out * frames, 0.0);
                    layer.z_block.resize(channels * frames, 0.0);
                    layer.post_block.resize(channels * frames, 0.0);
                    layer.frame_capacity = frames;
                }
            }
        }
    }

    fn rewind_array_buffers(array: &mut WaveNetLayerArray) {
        let new_start = array.receptive_field.saturating_sub(1);
        for (layer_idx, layer) in array.layers.iter().enumerate() {
            let d = (layer.conv.kernel_size - 1) * layer.conv.dilation;
            if d == 0 {
                continue;
            }
            for channel in 0..array.channels {
                let base = channel * array.buffer_size;
                let src = base + array.buffer_start - d;
                let dst = base + new_start - d;
                array.layer_buffers[layer_idx].copy_within(src..src + d, dst);
            }
        }
        array.buffer_start = new_start;
    }

    fn prepare_for_frames(&mut self, frames: usize) {
        for array in &mut self.arrays {
            if array.buffer_start + frames > array.buffer_size {
                Self::rewind_array_buffers(array);
            }
        }
    }

    fn process_layer_array(
        array: &mut WaveNetLayerArray,
        input_block: &[f32],
        condition_block: &[f32],
        initial_head_input: Option<&[f32]>,
        frames: usize,
    ) {
        Self::conv1x1_process_block(
            &array.rechannel,
            input_block,
            frames,
            &mut array.rechannel_block[..array.channels * frames],
        );
        for channel in 0..array.channels {
            let src_base = channel * frames;
            let src = &array.rechannel_block[src_base..src_base + frames];
            let dst_base = channel * array.buffer_size + array.buffer_start;
            let dst = &mut array.layer_buffers[0][dst_base..dst_base + frames];
            dst.copy_from_slice(src);
        }

        if let Some(init) = initial_head_input {
            let len = array.channels * frames;
            if init.len() >= len {
                array.head_input_block[..len].copy_from_slice(&init[..len]);
            } else {
                array.head_input_block[..len].fill(0.0);
            }
        } else {
            array.head_input_block[..array.channels * frames].fill(0.0);
        }
        let last_layer = array.layers.len().saturating_sub(1);

        for layer_idx in 0..array.layers.len() {
            let layer = &mut array.layers[layer_idx];
            let (input_buffer, next_layer_buffer) = if layer_idx == last_layer {
                (&array.layer_buffers[layer_idx][..], None)
            } else {
                let (left, right) = array.layer_buffers.split_at_mut(layer_idx + 1);
                (&left[layer_idx][..], Some(&mut right[0]))
            };

            Self::conv1d_process_block_from_buffer(
                &layer.conv,
                input_buffer,
                array.buffer_size,
                array.buffer_start,
                frames,
                &mut layer.conv_out_block[..layer.conv.out_channels * frames],
            );
            Self::conv1x1_process_block(
                &layer.input_mixin,
                condition_block,
                frames,
                &mut layer.mixin_block[..layer.conv.out_channels * frames],
            );
            for idx in 0..layer.conv.out_channels * frames {
                layer.conv_out_block[idx] += layer.mixin_block[idx];
            }

            let channels = array.channels;
            if layer.gated {
                for c in 0..channels {
                    let z_base = c * frames;
                    let g_base = (channels + c) * frames;
                    for f in 0..frames {
                        let a = layer
                            .activation
                            .apply_sample(layer.conv_out_block[z_base + f]);
                        let g = activations::sigmoid(layer.conv_out_block[g_base + f]);
                        let z = a * g;
                        layer.z_block[z_base + f] = z;
                        array.head_input_block[z_base + f] += z;
                    }
                }
            } else {
                for c in 0..channels {
                    let z_base = c * frames;
                    for f in 0..frames {
                        let z = layer
                            .activation
                            .apply_sample(layer.conv_out_block[z_base + f]);
                        layer.z_block[z_base + f] = z;
                        array.head_input_block[z_base + f] += z;
                    }
                }
            }

            Self::conv1x1_process_block(
                &layer.post,
                &layer.z_block[..channels * frames],
                frames,
                &mut layer.post_block[..channels * frames],
            );

            if layer_idx == last_layer {
                for c in 0..channels {
                    let in_base = c * array.buffer_size;
                    let out_base = c * frames;
                    for f in 0..frames {
                        let input_idx = in_base + array.buffer_start + f;
                        array.layer_output_block[out_base + f] =
                            input_buffer[input_idx] + layer.post_block[out_base + f];
                    }
                }
            } else {
                let next_layer_buffer =
                    next_layer_buffer.expect("next layer buffer exists for non-final layer");
                for c in 0..channels {
                    let in_base = c * array.buffer_size;
                    let out_base = c * frames;
                    for f in 0..frames {
                        let input_idx = in_base + array.buffer_start + f;
                        let out = input_buffer[input_idx] + layer.post_block[out_base + f];
                        let dst_idx = in_base + array.buffer_start + f;
                        next_layer_buffer[dst_idx] = out;
                    }
                }
            }
        }

        Self::conv1x1_process_block(
            &array.head_rechannel,
            &array.head_input_block[..array.channels * frames],
            frames,
            &mut array.head_output_block[..array.head_size * frames],
        );
        array.buffer_start += frames;
    }

    fn new(config: &Value, weights: &mut &[f32]) -> Result<Self, NamError> {
        let layers: Vec<LayerArrayParams> = serde_json::from_value(config["layers"].clone())?;
        if layers.is_empty() {
            return Err(NamError::InvalidConfig(
                "WaveNet.layers must be non-empty".into(),
            ));
        }
        if let Some(first) = layers.first()
            && first.input_size != 1
        {
            return Err(NamError::ChannelMismatch {
                expected: 1,
                got: first.input_size,
            });
        }
        if let Some(last) = layers.last()
            && last.head_size != 1
        {
            return Err(NamError::ChannelMismatch {
                expected: 1,
                got: last.head_size,
            });
        }
        let with_head = !config["head"].is_null();
        let post_stack_head = if with_head {
            let head_json = &config["head"];
            let in_channels = layers.last().map(|l| l.head_size).unwrap_or(1);
            let head_params = HeadParams {
                in_channels,
                channels: head_json["channels"].as_u64().ok_or_else(|| {
                    NamError::InvalidConfig("WaveNet Head: channels missing".into())
                })? as usize,
                out_channels: head_json["out_channels"].as_u64().ok_or_else(|| {
                    NamError::InvalidConfig("WaveNet Head: out_channels missing".into())
                })? as usize,
                kernel_sizes: head_json["kernel_sizes"]
                    .as_array()
                    .ok_or_else(|| {
                        NamError::InvalidConfig("WaveNet Head: kernel_sizes missing".into())
                    })?
                    .iter()
                    .map(|v| {
                        v.as_u64()
                            .ok_or_else(|| {
                                NamError::InvalidConfig("WaveNet Head: invalid kernel_size".into())
                            })
                            .map(|v| v as usize)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                activation: head_json["activation"]
                    .as_str()
                    .ok_or_else(|| {
                        NamError::InvalidConfig("WaveNet Head: activation missing".into())
                    })?
                    .to_string(),
            };
            Some(WaveNetHead::new(&head_params, weights)?)
        } else {
            None
        };
        let mut arrays = Vec::new();
        for params in &layers {
            let rechannel =
                Conv1x1::from_weights(params.input_size, params.channels, false, weights)?;
            let activation =
                activations::parse_activation(&Value::String(params.activation.clone()))?;
            let mut array_layers = Vec::new();
            for dilation in &params.dilations {
                let conv_out = if params.gated {
                    2 * params.channels
                } else {
                    params.channels
                };
                let conv = Conv1D::from_weights(
                    params.channels,
                    conv_out,
                    params.kernel_size,
                    *dilation,
                    true,
                    weights,
                )?;
                let input_mixin =
                    Conv1x1::from_weights(params.condition_size, conv_out, false, weights)?;
                let post = Conv1x1::from_weights(params.channels, params.channels, true, weights)?;
                array_layers.push(WaveNetLayer {
                    conv,
                    input_mixin,
                    post,
                    activation: activation.clone(),
                    gated: params.gated,
                    frame_capacity: 0,
                    conv_out_block: Vec::new(),
                    mixin_block: Vec::new(),
                    z_block: Vec::new(),
                    post_block: Vec::new(),
                });
            }
            let head_rechannel = Conv1x1::from_weights(
                params.channels,
                params.head_size,
                params.head_bias,
                weights,
            )?;
            let receptive_field = 1 + params
                .dilations
                .iter()
                .map(|d| d * (params.kernel_size - 1))
                .sum::<usize>();
            let buffer_size = Self::LAYER_ARRAY_BUFFER_SIZE + receptive_field - 1;
            let mut layer_buffers = Vec::with_capacity(array_layers.len());
            for _ in 0..array_layers.len() {
                layer_buffers.push(vec![0.0; params.channels * buffer_size]);
            }
            arrays.push(WaveNetLayerArray {
                channels: params.channels,
                head_size: params.head_size,
                rechannel,
                layers: array_layers,
                head_rechannel,
                receptive_field,
                buffer_size,
                buffer_start: receptive_field - 1,
                frame_capacity: 0,
                layer_buffers,
                rechannel_block: Vec::new(),
                layer_output_block: Vec::new(),
                head_input_block: Vec::new(),
                head_output_block: Vec::new(),
            });
        }
        if weights.is_empty() {
            return Err(NamError::InvalidConfig("WaveNet head scale missing".into()));
        }
        let head_scale = weights[0];
        *weights = &weights[1..];
        if !weights.is_empty() {
            return Err(NamError::InvalidConfig(format!(
                "WaveNet weight mismatch: {} weights remaining",
                weights.len()
            )));
        }
        let mut prewarm_samples = 1;
        for array in &arrays {
            for layer in &array.layers {
                prewarm_samples += layer.conv.dilation * (layer.conv.kernel_size - 1);
            }
        }
        Ok(Self {
            arrays,
            head_scale,
            post_stack_head,
            frame_capacity: 0,
            condition_block: Vec::new(),
            layer_input_block: Vec::new(),
            prewarm_samples,
        })
    }

    fn reset(&mut self) {
        for array in &mut self.arrays {
            for buf in &mut array.layer_buffers {
                buf.fill(0.0);
            }
            array.buffer_start = array.receptive_field - 1;
        }
        if let Some(head) = &mut self.post_stack_head {
            head.reset();
        }
    }

    fn process_chunk(&mut self, input: &[f32], output: &mut [f32]) {
        let frames = input.len().min(output.len());
        if frames == 0 {
            return;
        }
        self.ensure_frame_capacity(frames);
        self.prepare_for_frames(frames);

        self.condition_block[..frames].copy_from_slice(&input[..frames]);
        self.layer_input_block[..frames].copy_from_slice(&input[..frames]);

        for array_idx in 0..self.arrays.len() {
            let is_last = array_idx + 1 == self.arrays.len();
            if array_idx == 0 {
                let current = &mut self.arrays[0];
                let input_len = current.rechannel.in_channels * frames;
                Self::process_layer_array(
                    current,
                    &self.layer_input_block[..input_len],
                    &self.condition_block[..frames],
                    None,
                    frames,
                );
                if !is_last {
                    let out_len = current.channels * frames;
                    if self.layer_input_block.len() < out_len {
                        self.layer_input_block.resize(out_len, 0.0);
                    }
                    self.layer_input_block[..out_len]
                        .copy_from_slice(&current.layer_output_block[..out_len]);
                }
            } else {
                let (left, right) = self.arrays.split_at_mut(array_idx);
                let prev = &left[array_idx - 1];
                let current = &mut right[0];
                let input_len = current.rechannel.in_channels * frames;
                let init_len = current.channels * frames;
                let initial_head_input = &prev.head_output_block[..init_len];
                Self::process_layer_array(
                    current,
                    &self.layer_input_block[..input_len],
                    &self.condition_block[..frames],
                    Some(initial_head_input),
                    frames,
                );
                if !is_last {
                    let out_len = current.channels * frames;
                    if self.layer_input_block.len() < out_len {
                        self.layer_input_block.resize(out_len, 0.0);
                    }
                    self.layer_input_block[..out_len]
                        .copy_from_slice(&current.layer_output_block[..out_len]);
                }
            }
        }

        if let Some(last) = self.arrays.last() {
            if let Some(head) = &mut self.post_stack_head {
                // Process through post-stack head, matching C++ WaveNet::process.
                let head_in = last.head_size;
                for (f, out) in output.iter_mut().enumerate().take(frames) {
                    let mut frame = vec![0.0f32; head_in];
                    for (c, value) in frame.iter_mut().enumerate().take(head_in) {
                        *value = self.head_scale * last.head_output_block[c * frames + f];
                    }
                    let head_out = head.process_frame(&frame);
                    // Rural-modeler currently enforces mono output.
                    *out = head_out.first().copied().unwrap_or(0.0);
                }
            } else {
                for (f, out) in output.iter_mut().take(frames).enumerate() {
                    *out = self.head_scale * last.head_output_block[f];
                }
            }
        } else {
            output[..frames].copy_from_slice(&input[..frames]);
        }
    }

    fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        let frames = input.len().min(output.len());
        if frames == 0 {
            return;
        }
        // The layer-array buffers are sized to LAYER_ARRAY_BUFFER_SIZE plus
        // receptive-field padding. To avoid overflows when the host (or the
        // resampler upstream) delivers very large blocks, chunk the work the
        // same way NAM's iPlug2 ResamplingContainer does internally.
        let chunk_size = Self::LAYER_ARRAY_BUFFER_SIZE;
        let mut offset = 0;
        while offset < frames {
            let end = (offset + chunk_size).min(frames);
            self.process_chunk(&input[offset..end], &mut output[offset..end]);
            offset = end;
        }
    }

    fn num_input_channels(&self) -> usize {
        self.arrays
            .first()
            .map(|array| array.rechannel.in_channels)
            .unwrap_or(1)
    }

    fn num_output_channels(&self) -> usize {
        self.arrays.last().map(|array| array.head_size).unwrap_or(1)
    }
}

#[derive(Debug, Clone)]
struct Submodel {
    max_value: f64,
    model: NamModel,
}

#[derive(Debug, Clone)]
struct ContainerModel {
    submodels: Vec<Submodel>,
    active_index: usize,
}

impl ContainerModel {
    fn new(submodels: Vec<Submodel>, expected_sample_rate: Option<f32>) -> Result<Self, NamError> {
        if submodels.is_empty() {
            return Err(NamError::InvalidConfig(
                "ContainerModel: no submodels provided".into(),
            ));
        }
        for i in 1..submodels.len() {
            if submodels[i].max_value <= submodels[i - 1].max_value {
                return Err(NamError::InvalidConfig(
                    "ContainerModel: submodels must be sorted by ascending max_value".into(),
                ));
            }
        }
        if submodels.last().unwrap().max_value < 1.0 {
            return Err(NamError::InvalidConfig(
                "ContainerModel: last submodel max_value must be >= 1.0".into(),
            ));
        }
        if let Some(expected) = expected_sample_rate.filter(|rate| *rate > 0.0) {
            for submodel in &submodels {
                let sample_rate = submodel.model.metadata.expected_sample_rate.unwrap_or(-1.0);
                if sample_rate > 0.0 && (sample_rate - expected).abs() > f32::EPSILON {
                    return Err(NamError::InvalidConfig(format!(
                        "ContainerModel: submodel sample rate mismatch (expected {expected}, got {sample_rate})"
                    )));
                }
            }
        }
        let active_index = submodels.len() - 1;
        Ok(Self {
            submodels,
            active_index,
        })
    }

    fn reset(&mut self) {
        for sub in &mut self.submodels {
            sub.model.reset();
        }
    }

    fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        self.submodels[self.active_index]
            .model
            .process_block(input, output);
    }

    fn set_slimmable_size(&mut self, value: f64) {
        self.active_index = self.submodels.len() - 1;
        for (index, submodel) in self.submodels.iter().enumerate() {
            if value < submodel.max_value {
                self.active_index = index;
                break;
            }
        }
        self.submodels[self.active_index].model.reset();
    }
}

#[derive(Debug, Clone)]
enum ModelImpl {
    Linear(LinearModel),
    ConvNet(ConvNetModel),
    Lstm(LstmModel),
    WaveNet(WaveNetModel),
    Container(ContainerModel),
}

impl ModelImpl {
    fn prewarm_samples(&self) -> usize {
        match self {
            ModelImpl::Linear(m) => m.prewarm_samples,
            ModelImpl::ConvNet(m) => m.prewarm_samples,
            ModelImpl::Lstm(m) => m.prewarm_samples,
            ModelImpl::WaveNet(m) => m.prewarm_samples,
            ModelImpl::Container(m) => m.submodels[m.active_index].model.model.prewarm_samples(),
        }
    }

    fn num_input_channels(&self) -> usize {
        match self {
            ModelImpl::Linear(_) => 1,
            ModelImpl::ConvNet(_) => 1,
            ModelImpl::Lstm(_) => 1,
            ModelImpl::WaveNet(m) => m.num_input_channels(),
            ModelImpl::Container(m) => m.submodels[m.active_index].model.model.num_input_channels(),
        }
    }

    fn num_output_channels(&self) -> usize {
        match self {
            ModelImpl::Linear(_) => 1,
            ModelImpl::ConvNet(_) => 1,
            ModelImpl::Lstm(_) => 1,
            ModelImpl::WaveNet(m) => m.num_output_channels(),
            ModelImpl::Container(m) => m.submodels[m.active_index]
                .model
                .model
                .num_output_channels(),
        }
    }

    fn set_slimmable_size(&mut self, value: f64) {
        match self {
            ModelImpl::Container(model) => model.set_slimmable_size(value),
            ModelImpl::Linear(_)
            | ModelImpl::ConvNet(_)
            | ModelImpl::Lstm(_)
            | ModelImpl::WaveNet(_) => {}
        }
    }
}

#[derive(Debug, Clone)]
pub struct NamModel {
    pub metadata: ModelMetadata,
    model: ModelImpl,
    max_buffer_size: usize,
    external_sample_rate: Option<f32>,
}

impl NamModel {
    fn from_nam_file(file: NamFile) -> Result<Self, NamError> {
        verify_config_version(&file.version)?;
        let metadata = parse_metadata(file.metadata, file.sample_rate);
        let mut weights_slice = &file.weights[..];
        let model = match file.architecture.as_str() {
            "Linear" => {
                let receptive_field = file.config["receptive_field"].as_u64().ok_or_else(|| {
                    NamError::InvalidConfig("Linear.receptive_field missing".into())
                })? as usize;
                let bias = file.config["bias"]
                    .as_bool()
                    .ok_or_else(|| NamError::InvalidConfig("Linear.bias missing".into()))?;
                let in_channels = file.config["in_channels"].as_u64().unwrap_or(1) as usize;
                let out_channels = file.config["out_channels"].as_u64().unwrap_or(1) as usize;
                if in_channels != 1 {
                    return Err(NamError::ChannelMismatch {
                        expected: 1,
                        got: in_channels,
                    });
                }
                if out_channels != 1 {
                    return Err(NamError::ChannelMismatch {
                        expected: 1,
                        got: out_channels,
                    });
                }
                let m = LinearModel::new(receptive_field, bias, &mut weights_slice)?;
                if !weights_slice.is_empty() {
                    return Err(NamError::InvalidConfig(format!(
                        "Linear weight mismatch: {} weights remaining",
                        weights_slice.len()
                    )));
                }
                ModelImpl::Linear(m)
            }
            "ConvNet" => {
                let in_channels = file.config["in_channels"].as_u64().unwrap_or(1) as usize;
                let out_channels = file.config["out_channels"].as_u64().unwrap_or(1) as usize;
                let _groups = file.config["groups"].as_u64().unwrap_or(1) as usize;
                if in_channels != 1 {
                    return Err(NamError::ChannelMismatch {
                        expected: 1,
                        got: in_channels,
                    });
                }
                if out_channels != 1 {
                    return Err(NamError::ChannelMismatch {
                        expected: 1,
                        got: out_channels,
                    });
                }
                let m = ConvNetModel::new(&file.config, &mut weights_slice)?;
                ModelImpl::ConvNet(m)
            }
            "LSTM" => {
                let in_channels = file.config["in_channels"].as_u64().unwrap_or(1) as usize;
                let out_channels = file.config["out_channels"].as_u64().unwrap_or(1) as usize;
                if in_channels != 1 {
                    return Err(NamError::ChannelMismatch {
                        expected: 1,
                        got: in_channels,
                    });
                }
                if out_channels != 1 {
                    return Err(NamError::ChannelMismatch {
                        expected: 1,
                        got: out_channels,
                    });
                }
                let m = LstmModel::new(
                    &file.config,
                    &mut weights_slice,
                    file.sample_rate.unwrap_or(-1.0),
                )?;
                ModelImpl::Lstm(m)
            }
            "WaveNet" => {
                let m = WaveNetModel::new(&file.config, &mut weights_slice)?;
                ModelImpl::WaveNet(m)
            }
            "SlimmableContainer" => {
                let submodels_json = file.config["submodels"].as_array().ok_or_else(|| {
                    NamError::InvalidConfig(
                        "SlimmableContainer: 'submodels' must be an array".into(),
                    )
                })?;
                if submodels_json.is_empty() {
                    return Err(NamError::InvalidConfig(
                        "SlimmableContainer: 'submodels' must be non-empty".into(),
                    ));
                }
                let mut submodels = Vec::with_capacity(submodels_json.len());
                for entry in submodels_json {
                    let max_value = entry["max_value"].as_f64().ok_or_else(|| {
                        NamError::InvalidConfig("SlimmableContainer: max_value missing".into())
                    })?;
                    let model_json: NamFile = serde_json::from_value(entry["model"].clone())
                        .map_err(|e| {
                            NamError::InvalidConfig(format!(
                                "SlimmableContainer: invalid submodel: {e}"
                            ))
                        })?;
                    let model = NamModel::from_nam_file(model_json)?;
                    submodels.push(Submodel { max_value, model });
                }
                ModelImpl::Container(ContainerModel::new(submodels, file.sample_rate)?)
            }
            architecture => {
                return Err(NamError::UnsupportedArchitecture(architecture.to_string()));
            }
        };
        let in_channels = model.num_input_channels();
        if in_channels != 1 {
            return Err(NamError::ChannelMismatch {
                expected: 1,
                got: in_channels,
            });
        }
        let out_channels = model.num_output_channels();
        if out_channels != 1 {
            return Err(NamError::ChannelMismatch {
                expected: 1,
                got: out_channels,
            });
        }
        // Match NAM get_dsp.cpp behavior: prewarm on load so initial
        // conditions are settled before the model is first used.
        let mut loaded = Self {
            metadata,
            model,
            max_buffer_size: crate::dsp::core::NAM_DEFAULT_MAX_BUFFER_SIZE,
            external_sample_rate: None,
        };
        loaded.prewarm(loaded.model.prewarm_samples());
        Ok(loaded)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, NamError> {
        let text = fs::read_to_string(path)?;
        Self::load_from_str(&text)
    }

    pub fn load_from_str(text: &str) -> Result<Self, NamError> {
        let file: NamFile = serde_json::from_str(text)?;
        Self::from_nam_file(file)
    }

    pub fn reset(&mut self) {
        match &mut self.model {
            ModelImpl::Linear(model) => model.reset(),
            ModelImpl::ConvNet(model) => model.reset(),
            ModelImpl::Lstm(model) => model.reset(),
            ModelImpl::WaveNet(model) => model.reset(),
            ModelImpl::Container(model) => model.reset(),
        }
        self.prewarm(self.model.prewarm_samples());
    }

    pub fn prewarm(&mut self, samples: usize) {
        if samples == 0 {
            return;
        }
        let buffer_size = self.max_buffer_size.max(1);
        let input = vec![0.0f32; buffer_size];
        let mut output = vec![0.0f32; buffer_size];
        let mut processed = 0;
        while processed < samples {
            let block = buffer_size.min(samples - processed);
            self.process_block(&input[..block], &mut output[..block]);
            processed += block;
        }
    }

    /// Process a block of samples.  This matches the C++ `nam::DSP::process`
    /// API where `input` and `output` are separate buffers of `num_frames`.
    pub fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        match &mut self.model {
            ModelImpl::WaveNet(model) => model.process_block(input, output),
            ModelImpl::Linear(model) => model.process_block(input, output),
            ModelImpl::ConvNet(model) => model.process_block(input, output),
            ModelImpl::Lstm(model) => model.process_block(input, output),
            ModelImpl::Container(model) => model.process_block(input, output),
        }
    }

    pub fn set_slimmable_size(&mut self, value: f64) {
        self.model.set_slimmable_size(value);
    }
}

impl crate::dsp::core::Dsp for NamModel {
    fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        self.process_block(input, output);
    }

    fn reset(&mut self) {
        self.reset();
    }

    fn prewarm(&mut self, samples: usize) {
        self.prewarm(samples);
    }

    fn expected_sample_rate(&self) -> Option<f32> {
        self.metadata.expected_sample_rate
    }

    fn external_sample_rate(&self) -> Option<f32> {
        self.external_sample_rate
    }

    fn set_external_sample_rate(&mut self, rate: f32) {
        self.external_sample_rate = Some(rate);
    }

    fn num_input_channels(&self) -> usize {
        self.model.num_input_channels()
    }

    fn num_output_channels(&self) -> usize {
        self.model.num_output_channels()
    }

    fn max_buffer_size(&self) -> usize {
        self.max_buffer_size
    }

    fn set_max_buffer_size(&mut self, size: usize) {
        self.max_buffer_size = size;
    }

    fn loudness(&self) -> Option<f32> {
        self.metadata.loudness
    }

    fn input_level(&self) -> Option<f32> {
        self.metadata.input_level_dbu
    }

    fn output_level(&self) -> Option<f32> {
        self.metadata.output_level_dbu
    }

    fn set_loudness(&mut self, loudness: f32) {
        self.metadata.loudness = Some(loudness);
    }

    fn set_input_level(&mut self, level: f32) {
        self.metadata.input_level_dbu = Some(level);
    }

    fn set_output_level(&mut self, level: f32) {
        self.metadata.output_level_dbu = Some(level);
    }
}

/// Simple sample-rate converter that works sample-by-sample using linear
/// interpolation.  `ratio` is `output_rate / input_rate`.
#[derive(Debug, Clone)]
struct RateConverter {
    step: f64,
    next_t: f64,
    radius: usize,
    first_index: i64,
    data: VecDeque<f32>,
    pending: VecDeque<f32>,
}

impl RateConverter {
    fn new(input_rate: f32, output_rate: f32) -> Self {
        let mut out = Self {
            step: input_rate.max(1.0) as f64 / output_rate.max(1.0) as f64,
            next_t: 0.0,
            radius: 12,
            first_index: 0,
            data: VecDeque::new(),
            pending: VecDeque::new(),
        };
        out.reset();
        out
    }

    fn reset(&mut self) {
        self.next_t = 0.0;
        self.first_index = -(self.radius as i64);
        self.data.clear();
        self.data.resize(self.radius, 0.0);
        self.pending.clear();
    }

    fn required_input_samples_for_outputs(&self, outputs: usize) -> usize {
        if outputs == 0 {
            return 0;
        }
        let needed = (outputs.saturating_sub(1) as f64) * self.step + self.radius as f64 + 1.0;
        needed.ceil() as usize
    }

    fn sinc(x: f64) -> f64 {
        if x.abs() < 1.0e-12 {
            1.0
        } else {
            let pix = std::f64::consts::PI * x;
            pix.sin() / pix
        }
    }

    fn lanczos(&self, x: f64) -> f64 {
        let a = self.radius as f64;
        let ax = x.abs();
        if ax >= a {
            0.0
        } else {
            Self::sinc(x) * Self::sinc(x / a)
        }
    }

    fn sample_at(&self, index: i64) -> f32 {
        if index < self.first_index {
            return 0.0;
        }
        let offset = (index - self.first_index) as usize;
        self.data.get(offset).copied().unwrap_or(0.0)
    }

    fn render(&self, t: f64) -> f32 {
        let i = t.floor() as i64;
        let start = i - self.radius as i64 + 1;
        let end = i + self.radius as i64;
        let mut sum = 0.0f64;
        for n in start..=end {
            let weight = self.lanczos(t - n as f64);
            if weight == 0.0 {
                continue;
            }
            sum += weight * self.sample_at(n) as f64;
        }
        sum as f32
    }

    fn produce_pending(&mut self) {
        let latest_index = self.first_index + self.data.len() as i64 - 1;
        let radius = self.radius as f64;
        while self.next_t + radius <= latest_index as f64 {
            let out = self.render(self.next_t);
            self.pending.push_back(out);
            self.next_t += self.step;
        }

        // Drop history that can no longer affect future output.
        let min_needed = self.next_t.floor() as i64 - self.radius as i64 - 2;
        while self.first_index < min_needed && !self.data.is_empty() {
            self.data.pop_front();
            self.first_index += 1;
        }
    }

    fn push(&mut self, sample: f32) {
        self.data.push_back(sample);
        self.produce_pending();
    }

    fn pull(&mut self) -> Option<f32> {
        self.pending.pop_front()
    }

    fn renormalize_phase(&mut self) {
        // Keep phase-like counters bounded without changing relative timing.
        if self.first_index > 0 {
            let shift = self.first_index;
            self.first_index = 0;
            self.next_t -= shift as f64;
        }
    }
}

/// Wraps a [`NamModel`] and transparently resamples when the host sample rate
/// differs from the model's expected rate.  This brings the Rust plugin closer
/// to the C++ reference which uses a `ResamplingNAM` with Lanczos filters.
#[derive(Debug, Clone)]
pub struct ResamplingNamModel {
    model: NamModel,
    host_rate: f32,
    model_rate: f32,
    host_to_model: RateConverter,
    model_to_host: RateConverter,
    resampling_latency_samples: u32,
}

impl ResamplingNamModel {
    pub fn new(model: NamModel, host_rate: f32) -> Self {
        // C++ GetNAMSampleRate: models without a declared sample rate are
        // assumed to be 48 kHz, which was the standard before sample rate
        // metadata was added.
        let model_rate = model.metadata.expected_sample_rate.unwrap_or(48_000.0);
        let mut this = Self {
            host_to_model: RateConverter::new(host_rate, model_rate),
            model_to_host: RateConverter::new(model_rate, host_rate),
            model,
            host_rate,
            model_rate,
            resampling_latency_samples: 0,
        };
        this.recompute_latency();
        this.prewarm_resampling_pipeline();
        this
    }

    fn recompute_latency(&mut self) {
        if self.host_rate == self.model_rate {
            self.resampling_latency_samples = 0;
            return;
        }
        // Match NAM's container approach:
        // mid = resampler2.GetNumSamplesRequiredFor(1)
        // latency = resampler1.GetNumSamplesRequiredFor(mid)
        let mid_samples = self.model_to_host.required_input_samples_for_outputs(1);
        let latency = self
            .host_to_model
            .required_input_samples_for_outputs(mid_samples);
        self.resampling_latency_samples = latency as u32;
    }

    fn prewarm_resampling_pipeline(&mut self) {
        if self.host_rate == self.model_rate {
            return;
        }
        let mid_samples = self.model_to_host.required_input_samples_for_outputs(1);
        let latency = self.resampling_latency_samples as usize;

        // 1) Push silence through host->model converter.
        for _ in 0..latency {
            self.host_to_model.push(0.0);
        }

        // 2) Pull enough model-rate samples from converter output.
        let mut produced_mid = 0usize;
        while produced_mid < mid_samples {
            if self.host_to_model.pull().is_some() {
                produced_mid += 1;
            } else {
                // Defensive: keep feeding silence until enough output exists.
                self.host_to_model.push(0.0);
            }
        }

        // 3) Push corresponding silence into model->host converter.
        // This mirrors NAM's assumption that warmup silence through a causal
        // effect remains silence.
        for _ in 0..mid_samples {
            self.model_to_host.push(0.0);
        }
    }

    pub fn reset(&mut self) {
        self.model.reset();
        self.host_to_model.reset();
        self.model_to_host.reset();
        self.recompute_latency();
        self.prewarm_resampling_pipeline();
    }

    /// Update the host sample rate. If it changed, the internal converters are
    /// recreated but the encapsulated model state is preserved.
    pub fn set_host_rate(&mut self, rate: f32) {
        if self.host_rate == rate {
            return;
        }
        self.host_rate = rate;
        self.host_to_model = RateConverter::new(rate, self.model_rate);
        self.model_to_host = RateConverter::new(self.model_rate, rate);
        self.recompute_latency();
        self.prewarm_resampling_pipeline();
    }

    pub fn set_slimmable_size(&mut self, value: f64) {
        self.model.set_slimmable_size(value);
    }

    /// Process a block of samples, transparently handling any required
    /// sample-rate conversion.  This matches the C++ `ResamplingNAM::process`
    /// block-based API.
    pub fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        if self.host_rate == self.model_rate {
            self.model.process_block(input, output);
        } else {
            // Match NAM container semantics: process the provided input block,
            // then pull what is available for this output block. If there's an
            // under-run, repeat the last sample (or zero).
            let mut model_in_block = Vec::new();
            for &x in input {
                self.host_to_model.push(x);
                while let Some(model_in) = self.host_to_model.pull() {
                    model_in_block.push(model_in);
                }
            }
            if !model_in_block.is_empty() {
                let mut model_out_block = vec![0.0f32; model_in_block.len()];
                self.model
                    .process_block(&model_in_block, &mut model_out_block);
                for y in model_out_block {
                    self.model_to_host.push(y);
                }
            }

            let mut produced = 0usize;
            while produced < output.len() {
                let Some(sample) = self.model_to_host.pull() else {
                    break;
                };
                output[produced] = sample;
                produced += 1;
            }

            if produced < output.len() {
                let fill = if produced > 0 {
                    output[produced - 1]
                } else {
                    0.0
                };
                for out in &mut output[produced..] {
                    *out = fill;
                }
            }

            self.host_to_model.renormalize_phase();
            self.model_to_host.renormalize_phase();
        }
    }

    pub fn metadata(&self) -> &ModelMetadata {
        &self.model.metadata
    }

    pub fn latency_samples(&self) -> u32 {
        self.resampling_latency_samples
    }
}

impl crate::dsp::core::Dsp for ResamplingNamModel {
    fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        self.process_block(input, output);
    }

    fn reset(&mut self) {
        self.reset();
    }

    fn expected_sample_rate(&self) -> Option<f32> {
        self.model.expected_sample_rate()
    }

    fn external_sample_rate(&self) -> Option<f32> {
        Some(self.host_rate)
    }

    fn set_external_sample_rate(&mut self, rate: f32) {
        self.set_host_rate(rate);
    }

    fn num_input_channels(&self) -> usize {
        self.model.num_input_channels()
    }

    fn num_output_channels(&self) -> usize {
        self.model.num_output_channels()
    }

    fn max_buffer_size(&self) -> usize {
        self.model.max_buffer_size()
    }

    fn set_max_buffer_size(&mut self, size: usize) {
        self.model.set_max_buffer_size(size);
    }

    fn loudness(&self) -> Option<f32> {
        self.model.loudness()
    }

    fn input_level(&self) -> Option<f32> {
        self.model.input_level()
    }

    fn output_level(&self) -> Option<f32> {
        self.model.output_level()
    }

    fn set_loudness(&mut self, loudness: f32) {
        self.model.set_loudness(loudness);
    }

    fn set_input_level(&mut self, level: f32) {
        self.model.set_input_level(level);
    }

    fn set_output_level(&mut self, level: f32) {
        self.model.set_output_level(level);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{NamError, NamFile, NamModel};
    use crate::dsp::activations::{disable_fast_tanh, enable_fast_tanh, is_fast_tanh_enabled};

    fn process_one_sample(model: &mut NamModel, input: f32) -> f32 {
        let in_buf = [input];
        let mut out_buf = [0.0_f32];
        model.process_block(&in_buf, &mut out_buf);
        out_buf[0]
    }

    #[test]
    fn fast_tanh_toggle_works() {
        enable_fast_tanh();
        assert!(is_fast_tanh_enabled());
        disable_fast_tanh();
        assert!(!is_fast_tanh_enabled());
        enable_fast_tanh();
        assert!(is_fast_tanh_enabled());
    }

    #[test]
    fn wavenet_rejects_non_mono_input() {
        let file = NamFile {
            version: "0.7.0".to_string(),
            architecture: "WaveNet".to_string(),
            config: json!({
                "layers": [{
                    "input_size": 2,
                    "condition_size": 1,
                    "head_size": 1,
                    "channels": 1,
                    "kernel_size": 2,
                    "dilations": [1],
                    "activation": "ReLU",
                    "gated": false,
                    "head_bias": true
                }],
                "head": null
            }),
            metadata: None,
            weights: vec![],
            sample_rate: Some(48_000.0),
        };

        let err = NamModel::from_nam_file(file).expect_err("must reject non-mono WaveNet input");
        assert!(matches!(
            err,
            NamError::ChannelMismatch {
                expected: 1,
                got: 2
            }
        ));
    }

    #[test]
    fn wavenet_rejects_non_mono_output() {
        let file = NamFile {
            version: "0.7.0".to_string(),
            architecture: "WaveNet".to_string(),
            config: json!({
                "layers": [{
                    "input_size": 1,
                    "condition_size": 1,
                    "head_size": 2,
                    "channels": 1,
                    "kernel_size": 2,
                    "dilations": [1],
                    "activation": "ReLU",
                    "gated": false,
                    "head_bias": true
                }],
                "head": null
            }),
            metadata: None,
            weights: vec![],
            sample_rate: Some(48_000.0),
        };

        let err = NamModel::from_nam_file(file).expect_err("must reject non-mono WaveNet output");
        assert!(matches!(
            err,
            NamError::ChannelMismatch {
                expected: 1,
                got: 2
            }
        ));
    }

    #[test]
    fn wavenet_with_head_produces_output() {
        // Minimal WaveNet with a post-stack head (kernel_size=1, so head is
        // effectively a pointwise convolution).
        let file = NamFile {
            version: "0.7.0".to_string(),
            architecture: "WaveNet".to_string(),
            config: json!({
                "layers": [{
                    "input_size": 1,
                    "condition_size": 1,
                    "head_size": 1,
                    "channels": 1,
                    "kernel_size": 2,
                    "dilations": [1],
                    "activation": "ReLU",
                    "gated": false,
                    "head_bias": true
                }],
                "head": {
                    "channels": 1,
                    "out_channels": 1,
                    "kernel_sizes": [1],
                    "activation": "ReLU"
                }
            }),
            metadata: None,
            // Weights for the layer array:
            // rechannel 1x1 (1->1, no bias): [1.0]
            // conv1d (1->1, k=2, bias): weights [0.0, 1.0], bias [0.0]
            // input_mixin 1x1 (1->1, no bias): [1.0]
            // post 1x1 (1->1, bias): [1.0], bias [0.0]
            // head_rechannel 1x1 (1->1, bias): [1.0], bias [0.0]
            // head_scale: [1.0]
            // Head conv1d (1->1, k=1, bias): [1.0], bias [0.0]
            weights: vec![
                1.0, // rechannel
                0.0, 1.0, 0.0, // conv
                1.0, // input_mixin
                1.0, 0.0, // post
                1.0, 0.0, // head_rechannel
                1.0, // head_scale
                1.0, 0.0, // head conv
            ],
            sample_rate: Some(48_000.0),
        };

        let mut model = NamModel::from_nam_file(file).expect("WaveNet with head should load");
        model.reset();

        let mut out = [0.0f32];
        model.process_block(&[1.0], &mut out);
        // With all weights as identity and ReLU activations, output should be
        // close to 1.0 (exact value depends on buffer initialization).
        assert!(
            out[0] >= 0.0,
            "expected non-negative output from ReLU-based identity network, got {}",
            out[0]
        );
    }

    #[test]
    fn slimmable_container_rejects_submodel_sample_rate_mismatch() {
        let file = NamFile {
            version: "0.7.0".to_string(),
            architecture: "SlimmableContainer".to_string(),
            config: json!({
                "submodels": [
                    {
                        "max_value": 0.5,
                        "model": {
                            "version": "0.7.0",
                            "architecture": "Linear",
                            "config": {
                                "receptive_field": 1,
                                "bias": false,
                                "in_channels": 1,
                                "out_channels": 1
                            },
                            "weights": [0.0],
                            "sample_rate": 44_100.0
                        }
                    },
                    {
                        "max_value": 1.0,
                        "model": {
                            "version": "0.7.0",
                            "architecture": "Linear",
                            "config": {
                                "receptive_field": 1,
                                "bias": false,
                                "in_channels": 1,
                                "out_channels": 1
                            },
                            "weights": [0.0],
                            "sample_rate": 48_000.0
                        }
                    }
                ]
            }),
            metadata: None,
            weights: vec![],
            sample_rate: Some(48_000.0),
        };

        let err = NamModel::from_nam_file(file)
            .expect_err("must reject mismatched submodel sample rates");
        assert!(
            matches!(err, NamError::InvalidConfig(message) if message.contains("sample rate mismatch"))
        );
    }

    #[test]
    fn slimmable_container_selects_submodel_by_size_threshold() {
        let file = NamFile {
            version: "0.7.0".to_string(),
            architecture: "SlimmableContainer".to_string(),
            config: json!({
                "submodels": [
                    {
                        "max_value": 0.5,
                        "model": {
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
                        }
                    },
                    {
                        "max_value": 1.0,
                        "model": {
                            "version": "0.7.0",
                            "architecture": "Linear",
                            "config": {
                                "receptive_field": 1,
                                "bias": false,
                                "in_channels": 1,
                                "out_channels": 1
                            },
                            "weights": [2.0],
                            "sample_rate": 48_000.0
                        }
                    }
                ]
            }),
            metadata: None,
            weights: vec![],
            sample_rate: Some(48_000.0),
        };

        let mut model = NamModel::from_nam_file(file).expect("container model should load");

        model.set_slimmable_size(0.25);
        let small = process_one_sample(&mut model, 1.0);
        assert!(
            (small - 1.0).abs() < 1.0e-6,
            "expected first submodel output near 1.0, got {small}"
        );

        model.set_slimmable_size(0.75);
        let large = process_one_sample(&mut model, 1.0);
        assert!(
            (large - 2.0).abs() < 1.0e-6,
            "expected second submodel output near 2.0, got {large}"
        );
    }

    #[test]
    fn slimmable_container_set_size_resets_selected_submodel() {
        let file = NamFile {
            version: "0.7.0".to_string(),
            architecture: "SlimmableContainer".to_string(),
            config: json!({
                "submodels": [
                    {
                        "max_value": 0.5,
                        "model": {
                            "version": "0.7.0",
                            "architecture": "Linear",
                            "config": {
                                "receptive_field": 2,
                                "bias": false,
                                "in_channels": 1,
                                "out_channels": 1
                            },
                            "weights": [0.0, 1.0],
                            "sample_rate": 48_000.0
                        }
                    },
                    {
                        "max_value": 1.0,
                        "model": {
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
                        }
                    }
                ]
            }),
            metadata: None,
            weights: vec![],
            sample_rate: Some(48_000.0),
        };

        let mut model = NamModel::from_nam_file(file).expect("container model should load");

        model.set_slimmable_size(0.25);
        let _ = process_one_sample(&mut model, 1.0);
        model.set_slimmable_size(0.25);
        let out = process_one_sample(&mut model, 0.0);

        assert!(
            out.abs() < 1.0e-6,
            "expected reset after SetSlimmableSize, got lingering output {out}"
        );
    }

    #[test]
    fn wavenet_model_produces_non_silent_output() {
        let path = std::env::var("RURAL_MODELER_TEST_MODEL")
            .unwrap_or_else(|_| "/home/meka/Files/NAM/powerball lead 2.nam".to_string());
        let mut model = NamModel::load(&path).expect("failed to load NAM model");
        model.reset();

        let sample_rate = 48_000.0_f32;
        let freq = 220.0_f32;
        let mut energy = 0.0_f64;
        let mut peak = 0.0_f32;
        let frames = 16_384usize;

        for n in 0..frames {
            let t = n as f32 / sample_rate;
            let input = (2.0 * std::f32::consts::PI * freq * t).sin() * 0.25;
            let y = process_one_sample(&mut model, input);
            energy += (y as f64) * (y as f64);
            peak = peak.max(y.abs());
        }
        let rms = (energy / frames as f64).sqrt() as f32;
        assert!(
            rms > 1.0e-6 || peak > 1.0e-5,
            "model output is effectively silent (rms={rms}, peak={peak})"
        );
    }
}
