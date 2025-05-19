/// Initialize stderr logging with JSON format and secret redaction.
///
/// This should be the **first** call in every module's `main` before any logging
/// or secret retrieval occurs.
///
/// Internally this just delegates to `secret_client::init_logging()` which sets
/// up a `tracing` subscriber that writes JSON logs to `stderr` and automatically
/// redacts any secrets previously registered via
/// `secret_client::register_secret_for_redaction`.
pub fn init_module() {
    // The secret_client helper already installs a global subscriber using the
    // same settings we want (JSON logs → stderr, env-filter via RUST_LOG, plus
    // redaction layer).  We simply invoke it here so that SDK users have a
    // single entry-point.
    crate::secret_client::init_logging();

    // Optionally append additional layers (e.g. pretty print when RUST_LOG
    // enables debug).  For now we rely entirely on secret_client's setup to
    // avoid double-installing subscribers.
}

// ---------------------------------------------------------------------------
// Re-exports so that consumers can use these helpers directly from the SDK.
// ---------------------------------------------------------------------------

// The macro is defined with #[macro_export] so it's available at crate root
#[allow(unused_imports)]
pub use crate::safe_log;
