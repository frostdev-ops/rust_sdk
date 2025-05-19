#[cfg(test)]
mod tests {
    use crate::logging::init_module;
    use std::env;
    use std::sync::Once;
    use tracing::{debug, error, info, trace, warn};

    // We can only call init_module once, so we use this to ensure it's only done once
    static INIT: Once = Once::new();

    // Helper function to initialize the logger once
    fn init_logger_once() {
        INIT.call_once(|| {
            // Store the original value
            let original = env::var("RUST_LOG").ok();

            // Set a test value
            unsafe {
                env::set_var("RUST_LOG", "debug");
            }

            // Initialize
            init_module();

            // Restore the original value or remove if it wasn't set
            match original {
                Some(val) => unsafe { env::set_var("RUST_LOG", val) },
                None => unsafe { env::remove_var("RUST_LOG") },
            }
        });
    }

    // Test that we can initialize the logger without panicking
    #[test]
    fn test_init_module() {
        init_logger_once();
        // Since this is mostly testing that the call doesn't panic,
        // and we can't easily inspect the logger's configuration,
        // we'll just assert that the function returns without error
    }

    // Test logging at various levels
    #[test]
    fn test_logging_macros() {
        init_logger_once();

        // Test the macro with various log levels
        // These should not panic
        trace!("Test trace log");
        debug!("Test debug log");
        info!("Test info log");
        warn!("Test warn log");
        error!("Test error log");

        // Test structured logging
        info!(value = "test", "Structured log");

        // Test with a value that would be redacted if registered
        let secret_value = "password123";
        info!(secret = secret_value, "Log with potentially secret value");

        // We can't easily verify the output, but we can at least ensure
        // that the macro compiles and executes without panicking
    }
}
