use crate::secret_client::SecretClient;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::Mutex;
use std::future::Future;
use std::pin::Pin;

#[cfg(feature = "builder")]
use crate::builder::AppStateBuilder;

use crate::message::EncodingFormat;
use crate::message::EncodedMessage;
use crate::tcp_channel::TcpChannel;
use crate::{
    internal_messaging::{InternalMessagingClient, PendingInternalResponses},
    Error,
};

/// Function signature for module-to-module message handlers
pub type ModuleMessageHandler = Arc<dyn Fn(String, uuid::Uuid, EncodedMessage) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send>> + Send + Sync>;

/// Configuration for the application.
#[derive(Clone, Debug, Default)]
pub struct AppConfig {
    /// Primary message format for encoding outgoing messages
    pub message_format_primary: Option<EncodingFormat>,
    /// Secondary message format for fallback encoding
    pub message_format_secondary: Option<EncodingFormat>,
    /// Timeout for IPC requests in milliseconds
    pub ipc_timeout_ms: Option<u64>,
}

/// Shared application state for a PyWatt module.
///
/// Contains SDK-provided fields (`module_id`, `orchestrator_api`, `secret_client`)
/// plus user-defined state of type `T`.
#[derive(Clone)]
pub struct AppState<T> {
    module_id: String,
    orchestrator_api: String,
    secret_client: Arc<SecretClient>,
    /// User-provided application state
    pub user_state: T,
    pub internal_messaging_client: Option<InternalMessagingClient>,
    pub pending_internal_responses: Option<PendingInternalResponses>,
    /// Application configuration
    pub config: Option<AppConfig>,
    /// TCP channel for communication with the orchestrator
    pub tcp_channel: Option<Arc<TcpChannel>>,
    /// Handlers for module-to-module messages, keyed by source module ID
    pub module_message_handlers: Option<Arc<Mutex<HashMap<String, ModuleMessageHandler>>>>,
}

impl<T> AppState<T> {
    /// Create a new `AppState` with the given SDK context and user state.
    pub fn new(
        module_id: String,
        orchestrator_api: String,
        secret_client: Arc<SecretClient>,
        user_state: T,
    ) -> Self {
        Self {
            module_id,
            orchestrator_api,
            secret_client,
            user_state,
            internal_messaging_client: None,
            pending_internal_responses: None,
            config: Some(AppConfig::default()),
            tcp_channel: None,
            module_message_handlers: Some(Arc::new(Mutex::new(HashMap::new()))),
        }
    }

    /// Returns the module's ID.
    pub fn module_id(&self) -> &str {
        &self.module_id
    }

    /// Returns the orchestrator API URL.
    pub fn orchestrator_api(&self) -> &str {
        &self.orchestrator_api
    }

    /// Returns the configured `SecretClient` instance.
    pub fn secret_client(&self) -> &Arc<SecretClient> {
        &self.secret_client
    }

    /// Retrieves a reference to the `InternalMessagingClient` if available.
    pub fn internal_messaging_client(&self) -> Option<&InternalMessagingClient> {
        self.internal_messaging_client.as_ref()
    }

    /// Retrieves a clone of the `PendingInternalResponses` map if available.
    pub fn pending_internal_responses_map(&self) -> Option<PendingInternalResponses> {
        self.pending_internal_responses.as_ref().cloned()
    }

    /// Creates a new `AppStateBuilder` for fluent API construction.
    #[cfg(feature = "builder")]
    pub fn builder() -> AppStateBuilder<T> {
        AppStateBuilder::new()
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for AppState<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("module_id", &self.module_id)
            .field("orchestrator_api", &self.orchestrator_api)
            .field("user_state", &self.user_state)
            .field("config", &self.config)
            .field("tcp_channel", &self.tcp_channel)
            .field("internal_messaging_client", &self.internal_messaging_client)
            .field("pending_internal_responses", &"<pending_responses>")
            .field("module_message_handlers", &"<message_handlers>")
            .finish()
    }
}
