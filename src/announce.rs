use crate::ipc_types::Announce as AnnounceBlob;
use std::io::Write;
use thiserror::Error;
use tracing::info;

/// Errors that may occur when serializing or sending announcement
#[derive(Error, Debug)]
pub enum AnnounceError {
    /// Error serializing announcement to JSON
    #[error("Error serializing announcement: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Error writing to stdout
    #[error("Error writing to stdout: {0}")]
    Io(#[from] std::io::Error),
}

/// Send the module's announcement blob to the orchestrator via **stdout**.
///
/// This will write a single-line JSON message and flush stdout.
pub fn send_announce(announce: &AnnounceBlob) -> Result<(), AnnounceError> {
    let message_to_orchestrator =
        crate::ipc_types::ModuleToOrchestrator::Announce(announce.clone());
    let json = serde_json::to_string(&message_to_orchestrator)?;
    // Use safe_log to ensure secrets are redacted (if any) and log to stderr
    // But announcement must go to stdout, so we bypass safe_log and print raw JSON
    println!("{}", json);
    std::io::stdout().flush()?;

    info!("Sent announcement to orchestrator");
    Ok(())
}
