use thiserror::Error;

/// Errors that can occur in the secret client
#[derive(Debug, Error)]
pub enum SecretClientError {
    /// Secret not found in cache or remote
    #[error("secret not found: {0}")]
    NotFound(String),

    /// JSON serialization/deserialization error
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// I/O error during communication
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Unexpected response or behavior
    #[error("unexpected: {0}")]
    Unexpected(String),

    /// Other error types
    #[error("other error: {0}")]
    Other(String),
}

impl From<String> for SecretClientError {
    fn from(msg: String) -> Self {
        SecretClientError::Other(msg)
    }
}

impl From<&str> for SecretClientError {
    fn from(msg: &str) -> Self {
        SecretClientError::Other(msg.to_string())
    }
}
