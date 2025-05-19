// Secret provider module
// All functionality from the secret_provider crate will be migrated here

// Add public sub-modules
pub mod errors;
pub mod events;
pub mod metrics;
pub mod providers;
pub mod tracing;

// Re-export key types and traits from the original lib.rs
use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;
use tokio::sync::broadcast;

pub use errors::SecretError;
pub use events::SecretEvent;

// Re-export specific providers if desired at this level
pub use providers::{
    chained_provider::ChainedProvider,
    env_provider::EnvProvider,
    file_provider::{FileFormat, FileProvider},
    memory_provider::MemoryProvider,
};

/// Thread-safe, async-friendly secret source.
#[async_trait]
pub trait SecretProvider: Send + Sync + std::fmt::Debug {
    async fn get(&self, key: &str) -> Result<SecretString, SecretError>;
    async fn set(&self, key: &str, value: SecretString) -> Result<(), SecretError>;
    async fn keys(&self) -> Result<Vec<String>, SecretError>;
    async fn watch(&self, tx: broadcast::Sender<SecretEvent>) -> Result<(), SecretError>;
}

// Keep redact_line function here or move to a utils module?
// Moving it might be cleaner.
/// Redacts secret values from a line by fetching all secrets from the provider.
pub async fn redact_line(line: &str, provider: &dyn SecretProvider) -> String {
    let keys = match provider.keys().await {
        Ok(k) => k,
        Err(_) => return line.to_string(),
    };
    let mut patterns = Vec::new();
    for key in keys {
        if let Ok(secret) = provider.get(&key).await {
            patterns.push(secret.expose_secret().to_owned());
        }
    }
    if patterns.is_empty() {
        return line.to_string();
    }
    match aho_corasick::AhoCorasick::new(&patterns) {
        Ok(ac) => ac.replace_all(
            line,
            &std::iter::repeat_n("[REDACTED]", patterns.len()).collect::<Vec<_>>(),
        ),
        Err(_) => line.to_string(),
    }
}

/// Initialize a [`SecretProvider`] implementation based on environment variables.
/// Potentially move this to an examples/ or test utility section, or keep if it's core SDK init logic.
pub async fn init() -> Result<Arc<dyn SecretProvider>, SecretError> {
    use std::env;
    let chain_raw = env::var("SECRET_PROVIDER_CHAIN").unwrap_or_else(|_| "env".to_string());
    let chain_parts: Vec<&str> = chain_raw
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let mut providers: Vec<Arc<dyn SecretProvider>> = Vec::new();

    for part in chain_parts {
        match part {
            "env" => {
                providers.push(Arc::new(EnvProvider::new()));
            }
            "file" => {
                let path = env::var("SECRET_FILE_PATH").map_err(|_| {
                    SecretError::Configuration(
                        "SECRET_FILE_PATH env var not set for file provider".to_string(),
                    )
                })?;
                let format = match env::var("SECRET_FILE_FORMAT")
                    .unwrap_or_else(|_| "toml".to_string())
                    .to_lowercase()
                    .as_str()
                {
                    "toml" => FileFormat::Toml,
                    other => {
                        return Err(SecretError::Configuration(format!(
                            "Unsupported file format '{}'. Only 'toml' is supported",
                            other
                        )));
                    }
                };
                let fp = FileProvider::new(path, format).await?;
                providers.push(Arc::new(fp));
            }
            other => {
                return Err(SecretError::Configuration(format!(
                    "Unknown provider '{}' in SECRET_PROVIDER_CHAIN",
                    other
                )));
            }
        }
    }

    if providers.is_empty() {
        return Err(SecretError::Configuration(
            "No providers configured".to_string(),
        ));
    }

    // If exactly one provider, return it directly; otherwise chain them
    match providers.as_slice() {
        [single] => Ok(single.clone()),
        _ => Ok(Arc::new(ChainedProvider::new(providers))),
    }
}
