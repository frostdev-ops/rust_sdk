use thiserror::Error;

/// Errors that can occur within the Secret Provider subsystem.
#[derive(Error, Debug)]
pub enum SecretError {
    /// The requested secret key was not found in this provider.
    #[error("Secret key '{0}' not found")]
    NotFound(String),

    /// The operation (e.g., `set`) is not supported by this provider.
    #[error("Operation not supported by this provider: {0}")]
    UnsupportedOperation(String),

    /// An error occurred while interacting with the underlying secret source (e.g., file I/O, network error, environment variable access).
    #[error("Backend error: {0}")]
    Backend(#[from] anyhow::Error),

    /// An error occurred during configuration of the provider.
    #[error("Configuration error: {0}")]
    Configuration(String),
}
