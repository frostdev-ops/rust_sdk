use std::error::Error as StdError;
use std::fmt;

/// Error types for the database model manager
#[derive(Debug)]
pub enum Error {
    /// Error in model definition
    ModelDefinition(String),
    /// Error from a database adapter
    Adapter(String),
    /// Error generating SQL
    SqlGeneration(String),
    /// Unsupported feature for the target database
    UnsupportedFeature(String),
    /// Generic error
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ModelDefinition(msg) => write!(f, "Model definition error: {}", msg),
            Error::Adapter(msg) => write!(f, "Adapter error: {}", msg),
            Error::SqlGeneration(msg) => write!(f, "SQL generation error: {}", msg),
            Error::UnsupportedFeature(msg) => write!(f, "Unsupported feature: {}", msg),
            Error::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        None
    }
}

/// Convenience Result type for model manager operations
pub type Result<T> = std::result::Result<T, Error>;

/// Helper to create a ModelDefinition error
pub fn model_definition_error<S: Into<String>>(msg: S) -> Error {
    Error::ModelDefinition(msg.into())
}

/// Helper to create an Adapter error
pub fn adapter_error<S: Into<String>>(msg: S) -> Error {
    Error::Adapter(msg.into())
}

/// Helper to create a SqlGeneration error
pub fn sql_generation_error<S: Into<String>>(msg: S) -> Error {
    Error::SqlGeneration(msg.into())
}

/// Helper to create an UnsupportedFeature error
pub fn unsupported_feature_error<S: Into<String>>(msg: S) -> Error {
    Error::UnsupportedFeature(msg.into())
}

/// Helper to create an Other error
pub fn other_error<S: Into<String>>(msg: S) -> Error {
    Error::Other(msg.into())
}

// Note: We don't use a blanket impl<E: StdError> From<E> for Error here
// because it would conflict with the built-in impl<T> From<T> for T
// Instead, implement From for specific error types as needed when implementing adapters
