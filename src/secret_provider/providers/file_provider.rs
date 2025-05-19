use crate::secret_provider::{SecretError, SecretEvent, SecretProvider};
use async_trait::async_trait;
use dashmap::DashMap;
use notify::{
    Error, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher, event::ModifyKind,
};
use secrecy::SecretString;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    fs,
    sync::{Mutex, broadcast, mpsc},
    time::sleep,
};
use tracing::{debug, error, warn};

/// Helper type for deserializing secret maps from files.
type SecretMap = HashMap<String, String>;

/// Supported file formats for loading secrets.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FileFormat {
    Toml,
    // Extendable for JSON, YAML, etc.
}

/// A SecretProvider that loads and watches a file for secret values.
#[derive(Debug)]
pub struct FileProvider {
    file_path: PathBuf,
    format: FileFormat,
    secrets: Arc<DashMap<String, SecretString>>,
    watcher_tx: Arc<Mutex<Option<broadcast::Sender<SecretEvent>>>>,
    is_watching: Arc<AtomicBool>,
    _watcher_handle: Arc<Mutex<Option<RecommendedWatcher>>>,
}

impl FileProvider {
    /// Create a new FileProvider, loading the file initially.
    pub async fn new(
        file_path: impl Into<PathBuf>,
        format: FileFormat,
    ) -> Result<Self, SecretError> {
        let provider = Self {
            file_path: file_path.into(),
            format,
            secrets: Arc::new(DashMap::new()),
            watcher_tx: Arc::new(Mutex::new(None)),
            is_watching: Arc::new(AtomicBool::new(false)),
            _watcher_handle: Arc::new(Mutex::new(None)),
        };

        provider.load_secrets().await?;
        Ok(provider)
    }

    /// Load or reload secrets from the configured file.
    async fn load_secrets(&self) -> Result<Vec<String>, SecretError> {
        debug!(path = %self.file_path.display(), "Loading secrets from file");
        let content = fs::read_to_string(&self.file_path).await.map_err(|e| {
            SecretError::Backend(anyhow::Error::new(e).context(format!(
                "Failed to read secret file: {}",
                self.file_path.display()
            )))
        })?;

        let loaded: SecretMap = match self.format {
            FileFormat::Toml => toml::from_str(&content).map_err(|e| {
                SecretError::Configuration(format!(
                    "TOML parse error ({}): {}",
                    self.file_path.display(),
                    e
                ))
            })?,
        };

        // Determine changes
        let previous: HashSet<String> = self.secrets.iter().map(|e| e.key().clone()).collect();
        let mut changed = HashSet::new();

        // Replace map
        self.secrets.clear();
        for (k, v) in loaded {
            if !previous.contains(&k) {
                changed.insert(k.clone());
            }
            self.secrets.insert(k, SecretString::new(v.into()));
        }
        for old in previous {
            if !self.secrets.contains_key(&old) {
                changed.insert(old);
            }
        }

        Ok(changed.into_iter().collect())
    }

    /// Start a background task to watch the file and reload on changes.
    fn start_watching_task(
        file_path: PathBuf,
        secrets: Arc<DashMap<String, SecretString>>,
        tx: broadcast::Sender<SecretEvent>,
        watching: Arc<AtomicBool>,
        handle: Arc<Mutex<Option<RecommendedWatcher>>>,
        format: FileFormat,
    ) {
        tokio::spawn(async move {
            let (notify_tx, mut notify_rx) = mpsc::channel(1);
            let mut watcher = match RecommendedWatcher::new(
                move |res: Result<Event, Error>| match res {
                    Ok(event)
                        if matches!(
                            event.kind,
                            EventKind::Modify(ModifyKind::Data(_))
                                | EventKind::Create(_)
                                | EventKind::Remove(_)
                        ) =>
                    {
                        let _ = notify_tx.try_send(event);
                    }
                    Err(e) => error!(error = %e, "watch error"),
                    _ => {}
                },
                notify::Config::default(),
            ) {
                Ok(w) => w,
                Err(e) => {
                    error!(error = %e, "Failed to init watcher");
                    watching.store(false, Ordering::SeqCst);
                    return;
                }
            };

            let dir = file_path.parent().unwrap_or_else(|| Path::new("."));
            if let Err(e) = watcher.watch(dir, RecursiveMode::NonRecursive) {
                error!(error = %e, path = %dir.display(), "watch failed");
                watching.store(false, Ordering::SeqCst);
                return;
            }
            handle.lock().await.replace(watcher);

            loop {
                match tokio::time::timeout(Duration::from_secs(1), notify_rx.recv()).await {
                    Ok(Some(ev)) if ev.paths.iter().any(|p| p == &file_path) => {
                        debug!("file changed, reloading");
                        // reload
                        let provider = FileProvider {
                            file_path: file_path.clone(),
                            format,
                            secrets: secrets.clone(),
                            watcher_tx: Arc::new(Mutex::new(None)),
                            is_watching: watching.clone(),
                            _watcher_handle: handle.clone(),
                        };
                        if let Ok(changed_keys) = provider.load_secrets().await {
                            if !changed_keys.is_empty() {
                                let _ = tx.send(SecretEvent::Rotated(changed_keys));
                            }
                        }
                    }
                    _ => {}
                }
                sleep(Duration::from_millis(500)).await;
            }
        });
    }
}

#[async_trait]
impl SecretProvider for FileProvider {
    async fn get(&self, key: &str) -> Result<SecretString, SecretError> {
        self.secrets
            .get(key)
            .map(|s| s.clone())
            .ok_or_else(|| SecretError::NotFound(key.to_owned()))
    }

    async fn set(&self, _: &str, _: SecretString) -> Result<(), SecretError> {
        warn!("FileProvider is read-only");
        Err(SecretError::UnsupportedOperation(
            "set not supported".to_string(),
        ))
    }

    async fn keys(&self) -> Result<Vec<String>, SecretError> {
        Ok(self.secrets.iter().map(|e| e.key().clone()).collect())
    }

    async fn watch(&self, tx: broadcast::Sender<SecretEvent>) -> Result<(), SecretError> {
        let mut guard = self.watcher_tx.lock().await;
        guard.replace(tx.clone());
        if self
            .is_watching
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            Self::start_watching_task(
                self.file_path.clone(),
                self.secrets.clone(),
                tx,
                self.is_watching.clone(),
                self._watcher_handle.clone(),
                self.format,
            );
        }
        Ok(())
    }
}
