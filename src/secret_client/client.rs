use crate::ipc_types::{
    ClientRequest, GetSecretRequest, RotatedNotification, RotationAckRequest as RotationAck,
    ServerResponse,
};
use crate::secret_client::error::SecretClientError;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use secrecy::SecretString;
use std::io;
use std::io::{BufRead, BufReader, Write};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tokio::time::{Duration, timeout};
use tracing::{debug, error, info};

// Global singleton for the default client
// This might need rethinking in the SDK context. Maybe it shouldn't be global?
// Or maybe it's initialized differently.
static DEFAULT_CLIENT: Lazy<Arc<SecretClient>> = Lazy::new(|| {
    // Use BufReader to wrap stdin since it doesn't implement BufRead
    let stdin = BufReader::new(io::stdin());
    Arc::new(SecretClient::new_with_io(stdin, io::stdout()))
});

/// Request handling mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestMode {
    /// Use cache first, fallback to remote if not found
    CacheThenRemote,
    /// Force request to remote, update cache
    ForceRemote,
    /// Use cache only, fail if not found
    CacheOnly,
}

/// A client for communicating with the orchestrator's secret provider
// Make SecretClient fields private or pub(crate) unless they need to be public API
pub struct SecretClient {
    cache: DashMap<String, SecretString>,
    rotation_tracking: DashMap<String, String>,
    last_ack: Mutex<Option<String>>,
    rotation_tx: broadcast::Sender<Vec<String>>,
    input: Mutex<Box<dyn BufRead + Send>>,
    output: Mutex<Box<dyn Write + Send>>,
}

// Implement Debug manually to avoid exposing sensitive information
impl std::fmt::Debug for SecretClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretClient")
            .field("cache_size", &self.cache.len())
            .field("rotation_tracking_size", &self.rotation_tracking.len())
            .field(
                "has_last_ack",
                &self
                    .last_ack
                    .try_lock()
                    .map(|guard| guard.is_some())
                    .unwrap_or(false),
            )
            .field("broadcast_receivers", &self.rotation_tx.receiver_count())
            .finish_non_exhaustive()
    }
}

impl SecretClient {
    /// Creates a new SecretClient with the given I/O channels
    pub fn new_with_io<R, W>(input: R, output: W) -> Self
    where
        R: BufRead + Send + 'static,
        W: Write + Send + 'static,
    {
        let (tx, _) = broadcast::channel(10);
        Self {
            cache: DashMap::new(),
            rotation_tracking: DashMap::new(),
            last_ack: Mutex::new(None),
            rotation_tx: tx,
            input: Mutex::new(Box::new(input)),
            output: Mutex::new(Box::new(output)),
        }
    }

    /// Gets the default client instance (consider if this is still needed)
    pub fn global() -> Arc<Self> {
        DEFAULT_CLIENT.clone()
    }

    /// Gets a secret value, with caching based on mode
    pub async fn get_secret(
        &self,
        key: &str,
        mode: RequestMode,
    ) -> Result<SecretString, SecretClientError> {
        // Check cache first if not forcing remote
        if mode != RequestMode::ForceRemote {
            if let Some(cached) = self.cache.get(key) {
                debug!("Found secret '{}' in cache", key);
                return Ok(cached.clone());
            } else if mode == RequestMode::CacheOnly {
                return Err(SecretClientError::NotFound(key.to_string()));
            }
        }

        // Not in cache or forcing remote - request from orchestrator
        debug!("Requesting secret '{}' from orchestrator", key);
        let request = ClientRequest::GetSecret(GetSecretRequest {
            name: key.to_string(),
        });

        let response = self.send_request(&request).await?;

        match response {
            ServerResponse::Secret(secret_resp) => {
                if secret_resp.name != key {
                    return Err(SecretClientError::Unexpected(format!(
                        "Requested key '{}' but got '{}'",
                        key, secret_resp.name
                    )));
                }
                // Wrap the returned String into a SecretString
                let secret_value = SecretString::new(secret_resp.value.clone().into());
                // Update cache and rotation tracking
                if let Some(rotation_id) = &secret_resp.rotation_id {
                    self.rotation_tracking
                        .insert(secret_resp.name.clone(), rotation_id.clone());
                }
                self.cache.insert(key.to_string(), secret_value.clone());
                Ok(secret_value)
            }
            other => Err(SecretClientError::Unexpected(format!(
                "Expected Secret response, got {:?}",
                other
            ))),
        }
    }

    /// Sends a request to the orchestrator and waits for response
    async fn send_request(
        &self,
        request: &ClientRequest,
    ) -> Result<ServerResponse, SecretClientError> {
        // Serialize request
        let json = serde_json::to_string(request)?;

        // Write request to orchestrator via stdout
        {
            let mut output = self.output.lock().await;
            writeln!(*output, "{}", json)?;
            output.flush()?;
        }

        // Await response with timeout
        let result = timeout(Duration::from_secs(5), async {
            let mut input = self.input.lock().await;
            let mut line = String::new();
            input.read_line(&mut line)?;
            let resp: ServerResponse = serde_json::from_str(line.trim())?;
            Ok(resp)
        })
        .await;

        match result {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(SecretClientError::Other(
                "timeout waiting for response".into(),
            )),
        }
    }

    /// Acknowledges a rotation with success or failure
    pub async fn acknowledge_rotation(
        &self,
        rotation_id: &str,
        status: &str,
    ) -> Result<(), SecretClientError> {
        let request = ClientRequest::RotationAck(RotationAck {
            rotation_id: rotation_id.to_string(),
            status: status.to_string(),
            message: None,
        });

        let _ = self.send_request(&request).await?;

        // Store acknowledgment
        let mut last_ack = self.last_ack.lock().await;
        *last_ack = Some(rotation_id.to_string());

        Ok(())
    }

    /// Subscribes to rotation events
    pub fn subscribe_to_rotations(&self) -> broadcast::Receiver<Vec<String>> {
        self.rotation_tx.subscribe()
    }

    /// Handles an incoming rotation notification
    pub async fn handle_rotation_notification(
        &self,
        notification: RotatedNotification,
    ) -> Result<(), SecretClientError> {
        info!(
            "Received rotation notification for {} keys with ID {}",
            notification.keys.len(),
            notification.rotation_id
        );

        // Invalidate cache for the rotated keys
        for key in &notification.keys {
            self.cache.remove(key);
        }

        // Notify subscribers
        let _ = self.rotation_tx.send(notification.keys.clone());

        // Auto-acknowledge the rotation
        self.acknowledge_rotation(&notification.rotation_id, "success")
            .await?;

        Ok(())
    }

    /// Process an incoming server message
    pub async fn process_server_message(&self, message: &str) -> Result<(), SecretClientError> {
        let response: ServerResponse = serde_json::from_str(message)?;

        match response {
            ServerResponse::Init(_) => {
                // Ignore initialization messages handled elsewhere (e.g., handshake module)
            }
            ServerResponse::Rotated(notification) => {
                self.handle_rotation_notification(notification).await?;
            }
            ServerResponse::Secret(secret_resp) => {
                // Wrap the returned String into a SecretString and store in cache
                let secret_value = SecretString::new(secret_resp.value.clone().into());
                self.cache
                    .insert(secret_resp.name.clone(), secret_value.clone());

                // Track rotation ID if present
                if let Some(rotation_id) = &secret_resp.rotation_id {
                    self.rotation_tracking
                        .insert(secret_resp.name.clone(), rotation_id.clone());
                }
            }
            ServerResponse::Shutdown => {
                // Handle shutdown signal if needed by the client itself
                info!("Received shutdown signal from orchestrator");
            }
            // Note: The catch-all pattern `_` below is technically unreachable because
            // all variants of ServerResponse are explicitly handled above. Leaving it
            // for future-proofing in case new variants are added to ipc_types,
            // although a compiler warning will occur.
            #[allow(unreachable_patterns)]
            _ => {
                debug!(
                    "Received unexpected message type from orchestrator: {:?}",
                    response
                );
            }
        }

        Ok(())
    }

    /// Starts a background task that continuously processes incoming messages on the input channel.
    /// Returns a [`tokio::task::JoinHandle`] for the spawned task.
    pub fn start_background_task(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let client = Arc::clone(self);
        tokio::spawn(async move {
            let mut buffer = String::new();
            loop {
                buffer.clear();
                // Read a line from input (blocking)
                let bytes_read = {
                    let mut input = client.input.lock().await;
                    match input.read_line(&mut buffer) {
                        Ok(n) => n,
                        Err(e) => {
                            error!("Error reading from orchestrator: {}", e);
                            break; // Exit task on read error
                        }
                    }
                };

                if bytes_read == 0 {
                    // EOF or error – break loop
                    info!("Orchestrator input stream closed.");
                    break;
                }

                if let Err(e) = client.process_server_message(buffer.trim()).await {
                    error!("Error processing message from orchestrator: {}", e);
                    // Decide whether to continue or break on processing error
                }
            }
        })
    }

    /// Establishes a connection to the orchestrator based on the given API URL and module ID.
    /// This is likely now handled by the main SDK `get_module_secret_client` function.
    pub async fn connect(api_url: &str, module_id: &str) -> Result<Arc<Self>, SecretClientError> {
        // In the merged SDK, this is a simpler implementation that creates a client using standard IO
        let _ = (api_url, module_id); // For future compatibility with API params
        let reader = BufReader::new(io::stdin());
        let client = Arc::new(SecretClient::new_with_io(reader, io::stdout()));
        client.start_background_task(); // Start watching for messages
        Ok(client)
    }

    /// Convenience alias for `connect`, returning a shared `Arc`.
    pub async fn new(api_url: &str, module_id: &str) -> Result<Arc<Self>, SecretClientError> {
        SecretClient::connect(api_url, module_id).await
    }

    /// Convenience helper matching older API: fetches secret with default mode
    pub async fn get(&self, key: &str) -> Result<SecretString, SecretClientError> {
        self.get_secret(key, RequestMode::CacheThenRemote).await
    }

    /// Creates a dummy in-memory client for tests or smoke runs.
    pub fn new_dummy() -> Self {
        // Use empty reader/writer so no IPC is attempted
        let reader = BufReader::new(io::empty());
        let writer = io::sink();
        SecretClient::new_with_io(reader, writer)
    }

    #[cfg(test)]
    /// Creates a SecretClient suitable for testing
    pub fn for_test() -> Self {
        use tokio::sync::broadcast;

        // Use empty reader/writer so no IPC is attempted
        let reader = std::io::BufReader::new(std::io::empty());
        let writer = std::io::sink();
        let (tx, _) = broadcast::channel(10);

        Self {
            cache: dashmap::DashMap::new(),
            rotation_tracking: dashmap::DashMap::new(),
            last_ack: tokio::sync::Mutex::new(None),
            rotation_tx: tx,
            input: tokio::sync::Mutex::new(Box::new(reader)),
            output: tokio::sync::Mutex::new(Box::new(writer)),
        }
    }

    #[cfg(test)]
    /// Testing helper to insert a secret directly into the cache.
    pub async fn insert_test_secret(&self, key: &str, value: &str) {
        self.cache
            .insert(key.to_string(), SecretString::new(value.to_string().into()));
    }

    #[cfg(test)]
    /// Testing helper to send a rotation notification for given keys.
    pub fn send_test_rotation(
        &self,
        keys: Vec<String>,
    ) -> Result<usize, broadcast::error::SendError<Vec<String>>> {
        self.rotation_tx.send(keys)
    }

    /// Retrieves and parses a secret value into a strongly-typed `Secret<T>`.
    ///
    /// This method uses `get_typed_secret` from the `typed_secret` module and provides
    /// a convenient way to get typed secrets directly from the `SecretClient`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use pywatt_sdk::secret_client::SecretClient;
    /// use std::sync::Arc;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let client = Arc::new(SecretClient::new("http://localhost:9000", "my-module").await?);
    ///
    ///     // Get a port number as u16
    ///     let port = client.get_typed::<u16>("PORT").await?;
    ///     println!("Using port: {}", port.expose_secret());
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn get_typed<T>(
        &self,
        key: &str,
    ) -> Result<crate::typed_secret::Secret<T>, crate::typed_secret::TypedSecretError>
    where
        T: std::str::FromStr,
        T::Err: std::fmt::Display,
    {
        crate::typed_secret::get_typed_secret(self, key).await
    }

    /// Retrieves a secret as a string value.
    ///
    /// This is a convenience wrapper around `get_typed<String>`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use pywatt_sdk::secret_client::SecretClient;
    /// use std::sync::Arc;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let client = Arc::new(SecretClient::new("http://localhost:9000", "my-module").await?);
    ///
    ///     // Get an API key as a string
    ///     let api_key = client.get_string("API_KEY").await?;
    ///
    ///     // Use the API key
    ///     let auth_header = format!("Authorization: Bearer {}", api_key.expose_secret());
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn get_string(
        &self,
        key: &str,
    ) -> Result<crate::typed_secret::Secret<String>, crate::typed_secret::TypedSecretError> {
        crate::typed_secret::get_string_secret(self, key).await
    }

    /// Retrieves a secret as a boolean value.
    ///
    /// This is a convenience wrapper around `get_typed<bool>`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use pywatt_sdk::secret_client::SecretClient;
    /// use std::sync::Arc;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let client = Arc::new(SecretClient::new("http://localhost:9000", "my-module").await?);
    ///
    ///     // Get a feature flag as a boolean
    ///     let feature_enabled = client.get_bool("FEATURE_ENABLED").await?;
    ///
    ///     if *feature_enabled.expose_secret() {
    ///         println!("Feature is enabled!");
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn get_bool(
        &self,
        key: &str,
    ) -> Result<crate::typed_secret::Secret<bool>, crate::typed_secret::TypedSecretError> {
        crate::typed_secret::get_bool_secret(self, key).await
    }

    /// Retrieves a secret as an integer value of type T.
    ///
    /// This is a convenience wrapper around `get_typed<T>` for numeric types.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use pywatt_sdk::secret_client::SecretClient;
    /// use std::sync::Arc;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let client = Arc::new(SecretClient::new("http://localhost:9000", "my-module").await?);
    ///
    ///     // Get a timeout value as u64 seconds
    ///     let timeout_secs = client.get_int::<u64>("TIMEOUT_SECONDS").await?;
    ///
    ///     println!("Using timeout: {} seconds", timeout_secs.expose_secret());
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn get_int<T>(
        &self,
        key: &str,
    ) -> Result<crate::typed_secret::Secret<T>, crate::typed_secret::TypedSecretError>
    where
        T: std::str::FromStr + std::fmt::Debug,
        T::Err: std::fmt::Display,
    {
        crate::typed_secret::get_int_secret(self, key).await
    }
}

/// Convenience function to get a secret from the default client (if global client is kept)
pub async fn get_secret(key: &str) -> Result<SecretString, SecretClientError> {
    SecretClient::global()
        .get_secret(key, RequestMode::CacheThenRemote)
        .await
}

/// Safe way to print a string to stdout as JSON (if needed outside of IPC)
pub fn print_json<T: serde::Serialize>(value: &T) -> Result<(), SecretClientError> {
    let json = serde_json::to_string(value)?;
    // Use std::println! directly, assuming stdout isn't exclusively for IPC
    println!("{}", json);
    Ok(())
}

/// Safe way to print a message to stderr
pub fn print_stderr(msg: &str) -> Result<(), SecretClientError> {
    eprintln!("{}", msg);
    Ok(())
}
