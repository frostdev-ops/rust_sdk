use crate::secret_provider::{
    SecretError, SecretEvent, SecretProvider, tracing as provider_tracing,
};
use async_trait::async_trait;
use secrecy::SecretString;
use std::env;
use tokio::sync::broadcast;
use tracing::{debug, instrument};

/// A `SecretProvider` that retrieves secrets from environment variables.
///
/// This provider is read-only (`set` is unsupported) and does not support
/// watching for changes (`watch` is a no-op).
#[derive(Debug, Default, Clone)]
pub struct EnvProvider;

impl EnvProvider {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl SecretProvider for EnvProvider {
    #[instrument(
        name = "env_provider.get",
        skip_all,
        fields(
            secret_op = "get",
            secret_key_hash = tracing::field::Empty,
            outcome = tracing::field::Empty
        )
    )]
    async fn get(&self, key: &str) -> Result<SecretString, SecretError> {
        // Add hashed key to span
        provider_tracing::add_hashed_key(key);

        // Track metrics
        #[cfg(feature = "metrics")]
        let _timer = crate::secret_provider::metrics::OpTimer::new("env", "get");

        match env::var(key) {
            Ok(value) => {
                // Record success
                provider_tracing::record_success();
                #[cfg(feature = "metrics")]
                crate::secret_provider::metrics::record_operation("env", "get", "success");
                Ok(SecretString::new(value.into()))
            }
            Err(env::VarError::NotPresent) => {
                let err = SecretError::NotFound(key.to_string());
                provider_tracing::record_error(&err);
                #[cfg(feature = "metrics")]
                crate::secret_provider::metrics::record_operation("env", "get", "error");
                Err(err)
            }
            Err(e) => {
                let err = SecretError::Backend(
                    anyhow::Error::new(e)
                        .context(format!("Failed to read environment variable '{}'", key)),
                );
                provider_tracing::record_error(&err);
                #[cfg(feature = "metrics")]
                crate::secret_provider::metrics::record_operation("env", "get", "error");
                Err(err)
            }
        }
    }

    #[instrument(
        name = "env_provider.set",
        skip_all,
        fields(
            secret_op = "set",
            secret_key_hash = tracing::field::Empty,
            outcome = tracing::field::Empty
        )
    )]
    async fn set(&self, key: &str, _value: SecretString) -> Result<(), SecretError> {
        provider_tracing::add_hashed_key(key);
        #[cfg(feature = "metrics")]
        let _timer = crate::secret_provider::metrics::OpTimer::new("env", "set");

        debug!(
            "'set' operation is not supported by EnvProvider for key: {}",
            key
        );

        let err = SecretError::UnsupportedOperation(
            "Setting secrets via environment variables is not supported".to_string(),
        );

        provider_tracing::record_error(&err);
        #[cfg(feature = "metrics")]
        crate::secret_provider::metrics::record_operation("env", "set", "error");

        Err(err)
    }

    #[instrument(
        name = "env_provider.keys",
        skip_all,
        fields(
            secret_op = "keys",
            outcome = tracing::field::Empty
        )
    )]
    async fn keys(&self) -> Result<Vec<String>, SecretError> {
        #[cfg(feature = "metrics")]
        let _timer = crate::secret_provider::metrics::OpTimer::new("env", "keys");

        // Retrieving *all* environment variables can be noisy and potentially expose
        // unrelated sensitive information. Returning an empty list or an error might be safer.
        // Let's return an empty Vec for now, as the primary use is often specific key lookups.
        // If key discovery is crucial, consider adding a prefix filter.
        debug!(
            "'keys' operation on EnvProvider returns an empty list for safety/performance reasons."
        );

        provider_tracing::record_success();
        #[cfg(feature = "metrics")]
        crate::secret_provider::metrics::record_operation("env", "keys", "success");

        Ok(Vec::new())
    }

    #[instrument(
        name = "env_provider.watch",
        skip_all,
        fields(
            secret_op = "watch",
            outcome = tracing::field::Empty
        )
    )]
    async fn watch(&self, _tx: broadcast::Sender<SecretEvent>) -> Result<(), SecretError> {
        #[cfg(feature = "metrics")]
        let _timer = crate::secret_provider::metrics::OpTimer::new("env", "watch");

        // EnvProvider does not actively watch for environment variable changes.
        debug!("'watch' operation is a no-op for EnvProvider.");

        provider_tracing::record_success();
        #[cfg(feature = "metrics")]
        crate::secret_provider::metrics::record_operation("env", "watch", "success");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use secrecy::ExposeSecret;
    use std::env;

    const TEST_ENV_VAR: &str = "SECRET_PROVIDER_TEST_VAR";
    const TEST_SECRET_VALUE: &str = "test_secret_value_123";

    #[tokio::test]
    async fn env_provider_get_success() -> Result<()> {
        unsafe {
            env::set_var(TEST_ENV_VAR, TEST_SECRET_VALUE);
        }
        let provider = EnvProvider::new();
        let secret = provider.get(TEST_ENV_VAR).await?;
        assert_eq!(secret.expose_secret(), TEST_SECRET_VALUE);
        unsafe {
            env::remove_var(TEST_ENV_VAR);
        } // Clean up
        Ok(())
    }

    #[tokio::test]
    async fn env_provider_get_not_found() -> Result<()> {
        let provider = EnvProvider::new();
        let result = provider.get("NON_EXISTENT_VAR_FOR_TESTING").await;
        assert!(matches!(result, Err(SecretError::NotFound(_))));
        Ok(())
    }

    #[tokio::test]
    async fn env_provider_set_unsupported() -> Result<()> {
        let provider = EnvProvider::new();
        let result = provider
            .set("ANY_KEY", SecretString::new("any_value".into()))
            .await;
        assert!(matches!(result, Err(SecretError::UnsupportedOperation(_))));
        Ok(())
    }

    #[tokio::test]
    async fn env_provider_keys_empty() -> Result<()> {
        let provider = EnvProvider::new();
        let keys = provider.keys().await?;
        assert!(keys.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn env_provider_watch_noop() -> Result<()> {
        let provider = EnvProvider::new();
        let (tx, _) = broadcast::channel(1);
        let result = provider.watch(tx).await;
        assert!(result.is_ok());
        // No further check needed as it's a no-op
        Ok(())
    }
}
