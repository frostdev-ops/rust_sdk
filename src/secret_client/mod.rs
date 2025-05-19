// Secret client module
// All functionality from the secret_client crate has been migrated here

// Module declarations
pub mod client;
pub mod error;
pub mod logging;
pub mod schema;
pub mod stdout;

// Re-export key types and functions from the client crate's lib.rs
pub use client::{RequestMode, SecretClient};
pub use error::SecretClientError as SecretError;
pub use logging::{init_logging, redact, register_for_redaction, register_secret_for_redaction};
pub use schema::GetSecretResponse;

// Re-export macros defined within the submodules (like json_println, stderr, safe_log)
// These are automatically exported at the crate root if marked with #[macro_export]

/// Initializes the client (if necessary, depends on final implementation)
pub fn init() -> std::sync::Arc<SecretClient> {
    // This might need adjustment depending on how the global client is handled.
    client::SecretClient::global() // Assuming SecretClient::global exists in client.rs
}

/// Convenience function to register a secret for redaction and return it
pub fn with_redaction(secret: secrecy::SecretString) -> secrecy::SecretString {
    register_secret_for_redaction(&secret);
    secret
}

// Note: The original lib.rs had `#[macro_use] extern crate tracing;`.
// This is generally discouraged. Ensure tracing macros are available via `use tracing::{info, error, ...};`
// within the submodules or re-export them from the main SDK lib.rs if needed universally.
