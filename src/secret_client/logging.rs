// Import tracing macros using `use` statement
use tracing::{debug, error};

use aho_corasick::AhoCorasick;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;

// Global singleton for tracking secrets that need redaction
static REDACTION_REGISTRY: Lazy<Arc<DashMap<String, ()>>> = Lazy::new(|| Arc::new(DashMap::new()));

/// Registers a secret for redaction
pub fn register_for_redaction(secret_value: &str) {
    if !secret_value.is_empty() {
        REDACTION_REGISTRY.insert(secret_value.to_string(), ());
        debug!("Registered secret value for redaction");
    }
}

/// Registers a secret for redaction via Secret wrapper
pub fn register_secret_for_redaction(secret: &SecretString) {
    register_for_redaction(secret.expose_secret());
}

/// Redacts all registered secrets from a string
pub fn redact(input: &str) -> String {
    if REDACTION_REGISTRY.is_empty() {
        return input.to_string();
    }

    // Collect all secret values for the automaton
    let secret_values: Vec<String> = REDACTION_REGISTRY
        .iter()
        .map(|entry| entry.key().clone())
        .collect();

    if secret_values.is_empty() {
        return input.to_string();
    }

    // Create automaton for efficient multiple pattern matching
    match AhoCorasick::new(secret_values) {
        Ok(ac) => {
            // Create a vector of replacements, one "[REDACTED]" per pattern
            let replacements = vec!["[REDACTED]"; ac.patterns_len()];
            ac.replace_all(input, replacements.as_slice())
        }
        Err(e) => {
            error!("Failed to build redaction automaton: {}", e);
            // Fall back to returning the original if we can't redact
            input.to_string()
        }
    }
}

// Test-only helper functions
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn is_registered_for_redaction(key: &str) -> bool {
    REDACTION_REGISTRY.contains_key(key)
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn clear_redaction_registry() {
    REDACTION_REGISTRY.clear();
}

/// Initialize tracing subscriber with JSON logs and EnvFilter based on RUST_LOG
/// This might be superseded by the main SDK `init_module` eventually.
pub fn init_logging() {
    let env_filter = tracing_subscriber::filter::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::filter::EnvFilter::new("info"));
    let subscriber = tracing_subscriber::fmt::Subscriber::builder()
        .with_env_filter(env_filter)
        .json()
        .with_writer(std::io::stderr)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
}

/// Log macro that redacts sensitive information before logging
#[macro_export]
macro_rules! safe_log {
    (error, $($arg:tt)+) => {{
        let msg = format!($($arg)+);
        // Use crate::secret_client path now
        ::tracing::error!("{}", $crate::secret_client::logging::redact(&msg));
    }};
    (warn, $($arg:tt)+) => {{
        let msg = format!($($arg)+);
        // Use crate::secret_client path now
        ::tracing::warn!("{}", $crate::secret_client::logging::redact(&msg));
    }};
    (info, $($arg:tt)+) => {{
        let msg = format!($($arg)+);
        // Use crate::secret_client path now
        ::tracing::info!("{}", $crate::secret_client::logging::redact(&msg));
    }};
    (debug, $($arg:tt)+) => {{
        let msg = format!($($arg)+);
        // Use crate::secret_client path now
        ::tracing::debug!("{}", $crate::secret_client::logging::redact(&msg));
    }};
}

/// Writes to stderr with redaction
#[allow(dead_code)]
pub fn stderr(msg: &str) {
    let redacted = redact(msg);
    eprintln!("{}", redacted);
}
