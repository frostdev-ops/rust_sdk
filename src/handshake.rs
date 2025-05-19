use crate::ipc_types::Init as InitBlob;
use serde::de::Error as SerdeError;
use thiserror::Error;
use tokio::io::{self};

/// Errors that may occur while reading the initial handshake from stdin.
#[derive(Debug, Error)]
pub enum InitError {
    #[error("stdin closed unexpectedly during handshake")]
    StdinClosed,
    #[error("stdin read error: {0}")]
    Io(#[from] io::Error),
    #[error("failed to parse Init JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Read the `Init` message sent by the orchestrator over **stdin**.
///
/// This version purposefully avoids using a buffered reader that might
/// pre-fetch more than the first line.  We read **one byte at a time** until we
/// encounter the first `\n`, guaranteeing that *only* the handshake message is
/// consumed from STDIN.  All subsequent bytes (e.g. early `http_request`
/// messages) remain unread so that the runtime IPC loop can process them.
///
/// Although this byte-wise loop is a little less efficient, it only runs once
/// at start-up and therefore has negligible impact while fixing a critical
/// correctness bug where the buffered implementation consumed follow-up IPC
/// messages and caused orchestrator time-outs.
pub async fn read_init() -> Result<InitBlob, InitError> {
    use tokio::io::AsyncReadExt;

    let mut stdin = io::stdin();
    let mut buf = Vec::with_capacity(1024);
    let mut byte = [0u8; 1];

    loop {
        let n = stdin.read(&mut byte).await?;
        if n == 0 {
            // EOF before newline – orchestrator closed stdin unexpectedly.
            return Err(InitError::StdinClosed);
        }

        if byte[0] == b'\n' {
            break; // reached end of the first line
        }
        buf.push(byte[0]);

        // Protect against insanely long handshake lines ( > 1 MiB )
        if buf.len() > 1_048_576 {
            return Err(InitError::Json(serde_json::Error::custom(
                "Init line exceeded 1 MiB – possible protocol corruption",
            )));
        }
    }

    // Convert collected bytes to UTF-8 string
    let line = String::from_utf8(buf)
        .map_err(|e| InitError::Json(serde_json::Error::custom(e.to_string())))?;

    let init: InitBlob = serde_json::from_str(&line)?;
    Ok(init)
}
