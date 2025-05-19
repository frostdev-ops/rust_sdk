use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tracing::{Span, field};

/// Hash a key before logging to avoid exposing sensitive names.
/// This hashes the secret key to protect potentially sensitive key names
/// (e.g., "STRIPE_API_KEY" or "ADMIN_PASSWORD") from appearing in logs.
pub fn hash_key(key: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

/// Adds a hashed key to the current span for tracing purposes.
/// This avoids logging the actual key name, which might be sensitive.
pub fn add_hashed_key(key: &str) {
    let hash = hash_key(key);
    Span::current().record("secret_key_hash", field::display(hash));
}

/// Creates an instrumented span for secret provider operations.
/// Use this macro to wrap secret provider operations with proper tracing.
///
/// Example (using direct tracing macro):
/// ```ignore
/// use secrecy::SecretString;
/// use secret_provider::SecretError;
///
/// #[tracing::instrument(
///     name = "get_secret",
///     skip_all,
///     fields(
///         secret_op = "get",
///         secret_key_hash = tracing::field::Empty,
///         outcome = tracing::field::Empty
///     )
/// )]
/// async fn get_secret(key: &str) -> Result<SecretString, SecretError> {
///     // Implementation
///     # Ok(SecretString::new("".into()))
/// }
/// ```
#[macro_export]
macro_rules! instrument_secret_op {
    (name = $name:expr) => {
        #[tracing::instrument(
            name = $name,
            skip_all,
            fields(
                secret_op = $name,
                secret_key_hash = tracing::field::Empty,
                outcome = tracing::field::Empty
            )
        )]
    };
}

/// Record a successful operation outcome in the current span.
pub fn record_success() {
    Span::current().record("outcome", field::display("success"));
}

/// Record a failed operation outcome in the current span with error info.
pub fn record_error(error: &crate::secret_provider::errors::SecretError) {
    Span::current().record("outcome", field::display("error"));
    match error {
        crate::secret_provider::errors::SecretError::NotFound(_) => {
            Span::current().record("error_type", field::display("not_found"));
        }
        crate::secret_provider::errors::SecretError::Backend(_) => {
            Span::current().record("error_type", field::display("backend"));
        }
        crate::secret_provider::errors::SecretError::Configuration(_) => {
            Span::current().record("error_type", field::display("configuration"));
        }
        crate::secret_provider::errors::SecretError::UnsupportedOperation(_) => {
            Span::current().record("error_type", field::display("unsupported"));
        }
    }
}
