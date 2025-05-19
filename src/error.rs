use crate::bootstrap::BootstrapError;
#[cfg(feature = "cache")]
use crate::cache::CacheError;
#[cfg(feature = "database")]
use crate::database::DatabaseError;
#[cfg(feature = "database")]
use crate::model_manager::Error as ModelManagerError;
use crate::secrets::ModuleSecretError;
use crate::{AnnounceError, InitError};
use hyper::Error as HyperError;
#[cfg(feature = "metrics")]
use prometheus::Error as PrometheusError;
use std::io;
use thiserror::Error;

// Import new error types
use crate::tcp_types::NetworkError;
use crate::registration::RegistrationError;
use crate::http_tcp::router::HttpTcpError;

/// Unified error type for the SDK
#[derive(Debug, Error)]
pub enum Error {
    #[error("bootstrap failed: {0}")]
    Bootstrap(#[from] BootstrapError),

    #[error("handshake failed: {0}")]
    Init(#[from] InitError),

    #[error("secret client error: {0}")]
    Secret(#[from] ModuleSecretError),

    #[error("announcement error: {0}")]
    Announce(#[from] AnnounceError),

    #[error("Axum/HTTP error: {0}")]
    Axum(#[from] HyperError),

    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    #[cfg(feature = "metrics")]
    #[error("metrics error: {0}")]
    Metrics(#[from] MetricsError),

    #[cfg(feature = "database")]
    #[error("database error: {0}")]
    Database(#[from] DatabaseError),

    #[cfg(feature = "cache")]
    #[error("cache error: {0}")]
    Cache(#[from] CacheError),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[cfg(feature = "database")]
    #[error("model manager error: {0}")]
    ModelManager(#[from] ModelManagerError),

    #[error("network error: {0}")]
    Network(#[from] NetworkError),

    #[error("registration error: {0}")]
    Registration(#[from] RegistrationError),

    #[error("HTTP/TCP error: {0}")]
    HttpTcp(#[from] HttpTcpError),

    #[error("{0}")]
    StringError(String),
}

/// Convenience result alias
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(feature = "metrics")]
/// Error category for metrics operations
#[derive(Debug, Error)]
pub enum MetricsError {
    #[error("metrics encode error: {0}")]
    Encode(#[from] PrometheusError),

    #[error("response build error: {0}")]
    ResponseBuild(#[from] hyper::Error),
}

/// Error category for configuration issues
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("missing environment variable: {0}")]
    MissingEnvVar(String),

    #[error("invalid configuration: {0}")]
    Invalid(String),
}

// Deprecated alias methods for backward compatibility
impl Error {
    #[deprecated(
        since = "0.2.2",
        note = "Use Error::Config(ConfigError::Invalid(...)) instead of InvalidInput"
    )]
    /// Deprecated alias for the former InvalidInput variant
    pub fn invalid_input(message: String) -> Error {
        Error::Config(ConfigError::Invalid(message))
    }

    #[deprecated(
        since = "0.2.2",
        note = "Use Error::Config(ConfigError::Invalid(...)) instead of Forbidden"
    )]
    /// Deprecated alias for the former Forbidden variant
    pub fn forbidden(message: String) -> Error {
        Error::Config(ConfigError::Invalid(message))
    }
}

// Add From<String> implementation for Error to enable ? operator with string errors
impl From<String> for Error {
    fn from(error: String) -> Self {
        Error::StringError(error)
    }
}
