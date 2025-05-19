use crate::secret_client::error::SecretClientError;
use serde::Serialize;
use std::io::{self, Write};

/// A wrapper that enforces JSON serialization for stdout
pub struct JsonStdout {
    inner: io::Stdout,
}

impl JsonStdout {
    /// Creates a new JsonStdout
    pub fn new() -> Self {
        Self {
            inner: io::stdout(),
        }
    }

    /// Writes a serializable value to stdout as JSON
    pub fn write<T: Serialize>(&mut self, value: &T) -> Result<(), SecretClientError> {
        let json = serde_json::to_string(value)?;
        writeln!(self.inner, "{}", json)?;
        self.inner.flush()?;
        Ok(())
    }
}

impl Default for JsonStdout {
    fn default() -> Self {
        Self::new()
    }
}

/// A convenience macro for writing to JsonStdout
#[macro_export]
macro_rules! json_println {
    ($value:expr) => {{
        // Use crate::secret_client path now
        let mut stdout = $crate::secret_client::stdout::JsonStdout::new();
        stdout.write($value)
    }};
}

// This feature seems unused/undefined, comment it out for now.
// #[cfg(feature = "json-stdout-only")]
mod guards {
    use std::fmt;

    pub struct DisabledPrintln;

    impl fmt::Display for DisabledPrintln {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("println! is disabled - use json_println! instead")
        }
    }

    // #[macro_export] // Keep macro private to this module if cfg is disabled
    #[allow(unused_macros)]
    macro_rules! println {
        ($($arg:tt)*) => {
            compile_error!(
                "println! is disabled in this module. Use json_println! or stderr! instead."
            );
        };
    }

    // #[macro_export] // Keep macro private to this module if cfg is disabled
    #[allow(unused_macros)]
    macro_rules! print {
        ($($arg:tt)*) => {
            compile_error!(
                "print! is disabled in this module. Use json_println! or stderr! instead."
            );
        };
    }
}

/// Writes to stderr (allowed for non-JSON output)
pub fn stderr(msg: &str) -> io::Result<()> {
    let stderr = io::stderr();
    let mut handle = stderr.lock();
    writeln!(handle, "{}", msg)?;
    Ok(())
}

/// A convenience macro for writing to stderr
#[macro_export]
macro_rules! stderr {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        // Use crate::secret_client path now
        $crate::secret_client::stdout::stderr(&msg)
    }};
}
