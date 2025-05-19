use crate::secret_provider::{SecretError, SecretEvent, SecretProvider};
use async_trait::async_trait;
use secrecy::SecretString;
use std::{collections::HashSet, sync::Arc};
use tokio::sync::broadcast;

/// Chains multiple `SecretProvider` instances. Resolves `get()` by the first provider that returns `Ok`.
/// Broadcasts combined events from all providers to the same subscriber channel.
#[derive(Debug, Clone)]
pub struct ChainedProvider {
    providers: Vec<Arc<dyn SecretProvider>>,
}

impl ChainedProvider {
    /// Creates a new `ChainedProvider` with the given providers in order.
    pub fn new(providers: Vec<Arc<dyn SecretProvider>>) -> Self {
        Self { providers }
    }
}

#[async_trait]
impl SecretProvider for ChainedProvider {
    async fn get(&self, key: &str) -> Result<SecretString, SecretError> {
        for provider in &self.providers {
            match provider.get(key).await {
                Ok(val) => return Ok(val),
                Err(SecretError::NotFound(_)) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(SecretError::NotFound(key.to_string()))
    }

    async fn set(&self, key: &str, value: SecretString) -> Result<(), SecretError> {
        let mut last_error: Option<SecretError> = None;
        for provider in &self.providers {
            match provider.set(key, value.clone()).await {
                Ok(()) => return Ok(()),
                Err(SecretError::UnsupportedOperation(_)) => continue,
                Err(e) => {
                    last_error = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = last_error {
            Err(e)
        } else {
            Err(SecretError::UnsupportedOperation(format!(
                "No providers supported `set` for key '{}'.",
                key
            )))
        }
    }

    async fn keys(&self) -> Result<Vec<String>, SecretError> {
        let mut set = HashSet::new();
        for provider in &self.providers {
            if let Ok(keys) = provider.keys().await {
                for k in keys {
                    set.insert(k);
                }
            }
        }
        Ok(set.into_iter().collect())
    }

    async fn watch(&self, tx: broadcast::Sender<SecretEvent>) -> Result<(), SecretError> {
        for provider in &self.providers {
            provider.watch(tx.clone()).await?;
        }
        Ok(())
    }
}
