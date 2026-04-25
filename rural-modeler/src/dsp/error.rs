use thiserror::Error;

#[derive(Debug, Error)]
pub enum NamError {
    #[error("failed to read model: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse model JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported model version {0}")]
    UnsupportedVersion(String),
    #[error("unsupported architecture {0}")]
    UnsupportedArchitecture(String),
    #[error("invalid model config: {0}")]
    InvalidConfig(String),
    #[error("model channel mismatch: expected {expected}, got {got}")]
    ChannelMismatch { expected: usize, got: usize },
}
