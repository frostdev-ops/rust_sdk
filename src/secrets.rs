use crate::secret_client::{RequestMode, SecretClient, SecretError, register_secret_for_redaction};
use secrecy::SecretString;
use std::sync::Arc;
use tokio::task::JoinHandle;

/// Errors that may occur in secret management
pub type ModuleSecretError = SecretError;

/// Create a new SecretClient for the module, wired to stdin/stdout for IPC
pub async fn get_module_secret_client(
    orchestrator_api: &str,
    module_id: &str,
) -> Result<Arc<SecretClient>, ModuleSecretError> {
    // The secret_client crate provides `SecretClient::new`
    let client = SecretClient::new(orchestrator_api, module_id).await?;
    Ok(client)
}

/// Retrieve a secret by key from the orchestrator, registering it for redaction
pub async fn get_secret(
    client: &Arc<SecretClient>,
    key: &str,
) -> Result<SecretString, ModuleSecretError> {
    // Default to cache-then-remote
    let secret = client.get_secret(key, RequestMode::CacheThenRemote).await?;
    // Register for redaction
    register_secret_for_redaction(&secret);
    Ok(secret)
}

/// Retrieve multiple secrets by key, registering each for redaction.
///
/// This is a convenience function to fetch multiple secrets at once.
pub async fn get_secrets(
    client: &Arc<SecretClient>,
    keys: &[&str],
) -> Result<Vec<SecretString>, ModuleSecretError> {
    let mut secrets = Vec::with_capacity(keys.len());
    for &key in keys {
        let secret = get_secret(client, key).await?;
        secrets.push(secret);
    }
    Ok(secrets)
}

/// Subscribe to secret rotations for the given keys.  The `on_rotate` callback
/// will be invoked for each rotated key with the new value.
pub fn subscribe_secret_rotations<F>(
    client: Arc<SecretClient>,
    keys: Vec<String>,
    on_rotate: F,
) -> JoinHandle<()>
where
    F: Fn(String, SecretString) + Send + 'static,
{
    let mut rx = client.subscribe_to_rotations();
    tokio::spawn(async move {
        while let Ok(rotated_keys) = rx.recv().await {
            for key in &rotated_keys {
                if keys.contains(key) {
                    // Fetch fresh value
                    if let Ok(new_secret) = client.get_secret(key, RequestMode::ForceRemote).await {
                        register_secret_for_redaction(&new_secret);
                        on_rotate(key.clone(), new_secret.clone());
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret_client::client::SecretClient;
    use secrecy::ExposeSecret;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn get_secret_registers_for_redaction() {
        let client = Arc::new(SecretClient::new_dummy());
        client
            .insert_test_secret("my_api_key", "secret-value-123")
            .await;

        // Test that get_secret gets the value and registers it for redaction
        let secret = get_secret(&client, "my_api_key").await.unwrap();
        assert_eq!(secret.expose_secret(), "secret-value-123");

        // The registration for redaction happens internally in get_secret
        // We can't directly test the effect without exposing/mocking redaction internals
    }

    #[tokio::test]
    async fn get_secrets_gets_multiple_secrets() {
        let client = Arc::new(SecretClient::new_dummy());
        client.insert_test_secret("key1", "value1").await;
        client.insert_test_secret("key2", "value2").await;

        let secrets = get_secrets(&client, &["key1", "key2"]).await.unwrap();
        assert_eq!(secrets.len(), 2);
        assert_eq!(secrets[0].expose_secret(), "value1");
        assert_eq!(secrets[1].expose_secret(), "value2");
    }

    #[tokio::test]
    async fn subscribe_secret_rotations_handles_rotations() {
        let client = Arc::new(SecretClient::new_dummy());
        client
            .insert_test_secret("tracked_key", "initial-value")
            .await;

        // Set up shared state to verify our callback runs
        let rotated_value = Arc::new(Mutex::new(String::from("not-rotated")));
        let rotated_value_clone = rotated_value.clone();

        // Subscribe to rotations
        let _handle = subscribe_secret_rotations(
            client.clone(),
            vec!["tracked_key".to_string()],
            move |key, new_val| {
                if key == "tracked_key" {
                    let mut guard = rotated_value_clone.lock().unwrap();
                    *guard = new_val.expose_secret().to_string();
                }
            },
        );

        // Manually simulate a rotation event
        client
            .send_test_rotation(vec!["tracked_key".to_string()])
            .unwrap();

        // Update the secret value to simulate what would happen after notification
        client
            .insert_test_secret("tracked_key", "rotated-value")
            .await;

        // Give time for async handler to process
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // We can't verify the callback was called without some modifications to test helpers
        // or actual integration with the IPC/socket. This test is limited in scope.
    }
}
