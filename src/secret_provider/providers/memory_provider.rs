use crate::secret_provider::{SecretError, SecretEvent, SecretProvider};
use async_trait::async_trait;
use dashmap::DashMap;
use secrecy::SecretString;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, warn};

/// A `SecretProvider` that stores secrets in an in-memory, concurrent map.
///
/// Ideal for testing purposes. Supports `set` and basic `watch` notifications.
#[derive(Debug, Default)]
pub struct MemoryProvider {
    secrets: Arc<DashMap<String, SecretString>>,
    // Optional sender for watch notifications. Arc<Mutex<...>> might seem heavy,
    // but broadcast::Sender itself is Clone + Send + Sync. We only need Mutex
    // for initial setup or potential replacement, which is rare.
    // An alternative is `tokio::sync::watch` if only the latest state matters,
    // but broadcast allows multiple subscribers to see all events.
    watcher: Arc<tokio::sync::Mutex<Option<broadcast::Sender<SecretEvent>>>>,
}

impl MemoryProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new MemoryProvider initialized with the given secrets.
    pub fn with_secrets(initial_secrets: DashMap<String, SecretString>) -> Self {
        Self {
            secrets: Arc::new(initial_secrets),
            watcher: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }
}

#[async_trait]
impl SecretProvider for MemoryProvider {
    async fn get(&self, key: &str) -> Result<SecretString, SecretError> {
        self.secrets
            .get(key)
            .map(|secret_ref| secret_ref.value().clone())
            .ok_or_else(|| SecretError::NotFound(key.to_string()))
    }

    async fn set(&self, key: &str, value: SecretString) -> Result<(), SecretError> {
        let event = if self.secrets.contains_key(key) {
            SecretEvent::Rotated(vec![key.to_string()])
        } else {
            SecretEvent::Added(key.to_string())
        };

        self.secrets.insert(key.to_string(), value);
        debug!(secret_key = %key, provider = "MemoryProvider", "Set secret value");

        // Send notification if a watcher is attached
        let guard = self.watcher.lock().await;
        if let Some(tx) = guard.as_ref() {
            // If sending fails, it likely means no active receivers.
            // Log a warning but don't error out the set operation.
            if let Err(e) = tx.send(event) {
                warn!(secret_key = %key, error = %e, provider = "MemoryProvider", "Failed to send watch notification");
            }
        }

        Ok(())
    }

    async fn keys(&self) -> Result<Vec<String>, SecretError> {
        let keys = self
            .secrets
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        Ok(keys)
    }

    async fn watch(&self, tx: broadcast::Sender<SecretEvent>) -> Result<(), SecretError> {
        let mut guard = self.watcher.lock().await;
        if guard.is_some() {
            // Maybe return an error if already watched? Or replace?
            // For simplicity, let's just replace for now.
            warn!(
                provider = "MemoryProvider",
                "Watcher already exists, replacing."
            );
        }
        *guard = Some(tx);
        debug!(provider = "MemoryProvider", "Watcher attached.");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use secrecy::{ExposeSecret, SecretString};

    #[tokio::test]
    async fn memory_provider_get_set_roundtrip() -> Result<()> {
        let provider = MemoryProvider::new();
        let key = "test_key";
        let value = SecretString::new("test_value".into());

        // Set
        provider.set(key, value.clone()).await?;

        // Get
        let retrieved = provider.get(key).await?;
        assert_eq!(retrieved.expose_secret(), value.expose_secret());
        Ok(())
    }

    #[tokio::test]
    async fn memory_provider_get_not_found() -> Result<()> {
        let provider = MemoryProvider::new();
        let result = provider.get("non_existent_key").await;
        assert!(matches!(result, Err(SecretError::NotFound(_))));
        Ok(())
    }

    #[tokio::test]
    async fn memory_provider_keys() -> Result<()> {
        let provider = MemoryProvider::new();
        provider
            .set("key1", SecretString::new("val1".into()))
            .await?;
        provider
            .set("key2", SecretString::new("val2".into()))
            .await?;

        let mut keys = provider.keys().await?;
        keys.sort(); // Sort for consistent assertion
        assert_eq!(keys, vec!["key1", "key2"]);
        Ok(())
    }

    #[tokio::test]
    async fn memory_provider_overwrite() -> Result<()> {
        let provider = MemoryProvider::new();
        let key = "overwrite_key";
        let initial_value = SecretString::new("initial".into());
        let new_value = SecretString::new("new_value".into());

        provider.set(key, initial_value).await?;
        provider.set(key, new_value.clone()).await?;

        let retrieved = provider.get(key).await?;
        assert_eq!(retrieved.expose_secret(), new_value.expose_secret());
        Ok(())
    }

    #[tokio::test]
    async fn memory_provider_watch_notification() -> Result<()> {
        let provider = MemoryProvider::new();
        let (tx, mut rx1) = broadcast::channel(10);
        let mut rx2 = tx.subscribe(); // Add a second receiver

        // Attach watcher
        provider.watch(tx).await?;

        // Set a new key
        let added_key = "new_key";
        provider
            .set(added_key, SecretString::new("new_val".into()))
            .await?;

        // Set an existing key (rotate)
        let rotated_key = "existing_key";
        provider
            .set(rotated_key, SecretString::new("val1".into()))
            .await?;
        provider
            .set(rotated_key, SecretString::new("val2".into()))
            .await?;

        // Check receiver 1
        assert_eq!(rx1.recv().await?, SecretEvent::Added(added_key.to_string()));
        assert_eq!(
            rx1.recv().await?,
            SecretEvent::Added(rotated_key.to_string())
        );
        assert_eq!(
            rx1.recv().await?,
            SecretEvent::Rotated(vec![rotated_key.to_string()])
        );

        // Check receiver 2
        assert_eq!(rx2.recv().await?, SecretEvent::Added(added_key.to_string()));
        assert_eq!(
            rx2.recv().await?,
            SecretEvent::Added(rotated_key.to_string())
        );
        assert_eq!(
            rx2.recv().await?,
            SecretEvent::Rotated(vec![rotated_key.to_string()])
        );
        Ok(())
    }

    #[tokio::test]
    async fn memory_provider_watch_replace() -> Result<()> {
        let provider = MemoryProvider::new();
        let (tx1, _rx1) = broadcast::channel(1);
        let (tx2, mut rx2) = broadcast::channel(1);

        provider.watch(tx1).await?; // First watch
        provider.watch(tx2).await?; // Replace watch

        // Set a key, only the second watcher should receive
        provider
            .set("key", SecretString::new("value".into()))
            .await?;

        let event = rx2.recv().await?;
        assert!(matches!(event, SecretEvent::Added(_)));
        Ok(())
    }
}
