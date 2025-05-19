use secrecy::SecretString;
use std::sync::Arc;
use thiserror::Error;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, trace, warn};
use std::collections::HashMap;
use tokio::sync::Mutex;
// bootstrap module
#[cfg(feature = "tcp")]
use url::Url;
use tokio::net::TcpStream;

use crate::announce::{AnnounceError, send_announce};
use crate::ext::OrchestratorInitExt;
use crate::handshake::{InitError, read_init};
#[cfg(feature = "ipc_channel")]
use crate::ipc::process_ipc_messages;
use crate::ipc_types::{ModuleToOrchestrator, OrchestratorToModule};
use crate::internal_messaging;
use crate::logging::init_module;
use crate::message::{EncodedMessage, EncodingFormat, Message, MessageError};
use crate::secrets::{ModuleSecretError, get_module_secret_client, get_secret};
use crate::state::AppState;
#[cfg(feature = "tcp")]
use crate::tcp_channel::{TcpChannel, MessageChannel};
use crate::tcp_types::ConnectionState;
use crate::{AnnouncedEndpoint, Error, ModuleAnnounce, OrchestratorInit};

/// Type alias for the pending internal responses map to reduce type complexity
type PendingMap = Arc<Mutex<HashMap<uuid::Uuid, tokio::sync::oneshot::Sender<Result<EncodedMessage, Error>>>>>;

/// Errors that may occur during bootstrap initialization.
#[derive(Debug, Error)]
pub enum BootstrapError {
    #[error("handshake failed: {0}")]
    Init(#[from] InitError),

    #[error("secret client error: {0}")]
    Secret(#[from] ModuleSecretError),

    #[error("announcement error: {0}")]
    Announce(#[from] AnnounceError),

    #[error("other error: {0}")]
    Other(String),
}

/// Main processing task for handling messages from the orchestrator over TCP.
///
/// This task runs in the background and processes messages received from the orchestrator,
/// including routing internal module-to-module messages and responses, and handling
/// heartbeats and shutdown signals.
///
/// The task will continue running until either the orchestrator sends a shutdown signal
/// or the TCP connection is permanently closed.
async fn main_processing_task<S: Send + Sync + 'static>(
    app_state: Arc<AppState<S>>,
    orchestrator_channel: Arc<TcpChannel>,
) {
    debug!("Main TCP processing task started.");

    // Clone necessary Arcs for the loop
    let pending_responses_map_for_task = app_state.pending_internal_responses.clone();

    loop {
        match orchestrator_channel.receive().await {
            Ok(encoded_message) => {
                // Attempt to decode the EncodedMessage into an OrchestratorToModule variant
                // OrchestratorToModule doesn't implement bincode::Decode, so use JSON deserialization directly
                let message = match encoded_message.format() {
                    crate::message::EncodingFormat::Json => {
                        // For JSON, we can deserialize directly
                        match serde_json::from_slice::<OrchestratorToModule>(encoded_message.data()) {
                            Ok(msg) => msg,
                            Err(e) => {
                                error!("Failed to decode OrchestratorToModule message: {}", e);
                                continue;
                            }
                        }
                    },
                    _ => {
                        // For other formats, attempt to convert to JSON first
                        match encoded_message.to_format(crate::message::EncodingFormat::Json) {
                            Ok(json_encoded) => {
                                match serde_json::from_slice::<OrchestratorToModule>(json_encoded.data()) {
                                    Ok(msg) => msg,
                                    Err(e) => {
                                        error!("Failed to decode OrchestratorToModule message: {}", e);
                                        continue;
                                    }
                                }
                            },
                            Err(e) => {
                                error!("Failed to convert message to JSON: {}", e);
                                continue;
                            }
                        }
                    }
                };

                match message {
                    OrchestratorToModule::Heartbeat => {
                        debug!("Received Heartbeat from orchestrator. Sending Ack.");
                        let ack = ModuleToOrchestrator::HeartbeatAck;
                        let msg = Message::new(ack);
                        let default_encoding = app_state.config
                            .as_ref()
                            .and_then(|cfg| cfg.message_format_primary)
                            .unwrap_or_default();
                        match EncodedMessage::encode_with_format(&msg, default_encoding) {
                            Ok(encoded_ack) => {
                                if let Err(e) = orchestrator_channel.send(encoded_ack).await {
                                    error!("Failed to send HeartbeatAck to orchestrator: {}", e);
                                }
                            }
                            Err(e) => {
                                error!("Failed to encode HeartbeatAck: {}", e);
                            }
                        }
                    }
                    OrchestratorToModule::Shutdown => {
                        warn!("Received Shutdown signal from orchestrator. Terminating module.");
                        break;
                    }
                    #[allow(unused_variables)]
                    OrchestratorToModule::RoutedModuleResponse { request_id, source_module_id, payload } => {
                        debug!(request_id = %request_id, source_module_id = %source_module_id, "Received RoutedModuleResponse, dispatching.");
                        if let Some(ref pending_map) = pending_responses_map_for_task {
                            // The payload is an EncodedMessage from the target module.
                            // process_routed_module_response expects Result<EncodedMessage, Error>.
                            // If we are here, the orchestrator successfully relayed it, so it's Ok(payload).
                            internal_messaging::process_routed_module_response(
                                request_id,
                                Ok(payload), // Pass the EncodedMessage payload as Ok
                                pending_map.clone(),
                            ).await;
                        } else {
                            warn!(request_id = %request_id, "Received RoutedModuleResponse but no pending_responses_map in AppState. Discarding.");
                        }
                    }
                    #[allow(unused_variables)]
                    OrchestratorToModule::RoutedModuleMessage { source_module_id, original_request_id, payload } => {
                        debug!(
                            source_module_id = %source_module_id, 
                            request_id = %original_request_id,
                            "Received RoutedModuleMessage from another module."
                        );
                        // Check if there's a registered handler for module-to-module messages in the app state
                        if let Some(ref message_handlers) = app_state.module_message_handlers {
                            // If there's a handler, attempt to process the message
                            let handler = message_handlers.lock().await;
                            if let Some(handler_fn) = handler.get(&source_module_id) {
                                debug!("Found handler for messages from module {}", source_module_id);
                                // Spawn a task to process the message asynchronously
                                let handler_clone = handler_fn.clone();
                                let source_id = source_module_id.clone();
                                let req_id = original_request_id;
                                let payload_clone = payload.clone();
                                let orchestrator_channel_clone = orchestrator_channel.clone();
                                
                                tokio::spawn(async move {
                                    // Call the handler
                                    match handler_clone(source_id, req_id, payload_clone).await {
                                        Ok(response) => {
                                            // Send response back if needed
                                            debug!("Module-to-module message handler completed successfully");
                                        },
                                        Err(e) => {
                                            error!("Error processing module-to-module message: {}", e);
                                        }
                                    }
                                });
                                return;
                            }
                        }
                        
                        // No handler found
                        info!("No handler registered for module-to-module messages from {}. Message discarded.", source_module_id);
                    }
                    OrchestratorToModule::HttpRequest(http_request) => {
                        trace!("Received HttpRequest from orchestrator: {:?}", http_request.uri);
                        // TODO: Handle HTTP-over-TCP requests if needed
                    }
                    other => {
                        trace!("Received unhandled OrchestratorToModule variant: {:?}", other);
                    }
                }
            }
            Err(e) => {
                // Handle message error type
                match &e {
                    MessageError::InvalidFormat(error_msg) if error_msg.contains("connection aborted") => {
                        warn!("Orchestrator connection closed. Attempting reconnect if configured...");
                        if orchestrator_channel.is_permanently_closed().await {
                            error!("Orchestrator connection permanently closed. Main processing task terminating.");
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    },
                    _ => {
                        error!("Fatal error receiving message from orchestrator: {}. Main processing task terminating.", e);
                        break;
                    }
                }
            }
        }
    }
    debug!("Main TCP processing task finished.");
}

/// Bootstraps a PyWatt module with handshake, secret init, announcement, and IPC.
///
/// This function provides the complete setup flow for a PyWatt module:
/// 1. Initialize logging and redaction
/// 2. Perform handshake with orchestrator
/// 3. Set up secret client and fetch initial secrets
/// 4. Build application state
/// 5. Announce the module and its endpoints to the orchestrator
/// 6. Set up TCP connection to orchestrator (when feature enabled)
/// 7. Start IPC processing loop
/// 
/// - `secret_keys`: list of environment secret names to fetch initially.
/// - `endpoints`: list of HTTP/WebSocket endpoints for announcement.
/// - `state_builder`: callback to build module-specific state from the orchestrator init and fetched secrets.
///
/// Returns a tuple of `(AppState<T>, JoinHandle<()>)` where the join handle is a spawned task running the IPC loop.
/// The join handle can be awaited to detect when the IPC loop terminates.
pub async fn bootstrap_module<T, F>(
    secret_keys: Vec<String>,
    endpoints: Vec<AnnouncedEndpoint>,
    state_builder: F,
) -> Result<(AppState<T>, JoinHandle<()>), BootstrapError>
where
    F: Fn(&OrchestratorInit, Vec<SecretString>) -> T + Send + Sync + 'static,
    T: Send + Sync + Clone + 'static,
{
    // 1. Initialize logging and redaction
    init_module();

    // 2. Handshake: read orchestrator init
    let init: OrchestratorInit = read_init().await?;

    // 3. Secret client
    let client = get_module_secret_client(&init.orchestrator_api, &init.module_id).await?;

    // 4. Fetch initial secrets
    let mut secrets = Vec::new();
    for key in &secret_keys {
        let s = get_secret(&client, key).await?;
        secrets.push(s);
    }

    // 5. Build application state
    let user_state = state_builder(&init, secrets);
    let mut app_state = AppState::new(
        init.module_id.clone(),
        init.orchestrator_api.clone(),
        client.clone(),
        user_state,
    );

    // Add configuration to AppState
    app_state.config = Some(crate::state::AppConfig {
        message_format_primary: Some(EncodingFormat::Json),
        // Add other config fields as needed
        ..Default::default()
    });

    // Initialize the pending map for internal messaging
    let pending_map: PendingMap = 
        Arc::new(Mutex::new(HashMap::new()));
    app_state.pending_internal_responses = Some(pending_map);

    // 6. Send announcement
    let listen_str = init.listen_to_string();
    let announce = ModuleAnnounce {
        listen: listen_str,
        endpoints,
    };
    send_announce(&announce)?;

    // 7. Set up TCP connection to orchestrator for registration when feature enabled
    #[cfg(feature = "ipc_channel")]
    {
        // Extract host and port from orchestrator_api to connect to TCP
        if let Ok(url) = Url::parse(&init.orchestrator_api) {
            let host = url.host_str().unwrap_or("localhost").to_string();
            // Extract port, adding 1 to HTTP port as that's what Wattson does
            let http_port = url.port().unwrap_or(80);
            let tcp_port = http_port + 1;
            
            debug!("Establishing TCP connection to orchestrator at {}:{}", host, tcp_port);
            
            // Create TCP connection and initialize the TcpChannel
            match TcpStream::connect(format!("{0}:{1}", host, tcp_port)).await {
                Ok(_tcp_stream) => {
                    info!("Successfully connected to orchestrator TCP channel on port {}", tcp_port);
                    
                    // Create connection config from host and port
                    let config = crate::tcp_types::ConnectionConfig::new(host.clone(), tcp_port);
                    let channel = TcpChannel::new(config);

                    // Create TCP channel
                    let channel_arc = Arc::new(channel);
                    app_state.tcp_channel = Some(channel_arc.clone());
                    
                    // Start the TCP message processing task
                    let app_state_clone = Arc::new(app_state.clone());
                    tokio::spawn(main_processing_task(app_state_clone, channel_arc));
                    
                    info!("Started TCP main processing task");
                },
                Err(e) => {
                    warn!("Failed to connect to orchestrator: {}", e);
                }
            }
        } else {
            warn!("Failed to parse orchestrator_api as URL: {}", init.orchestrator_api);
        }
    }

    // 8. Create an internal messaging client and add it to app_state
    #[cfg(feature = "ipc_channel")]
    let internal_client = crate::internal_messaging::InternalMessagingClient::new(
        init.module_id.clone(),
        app_state.pending_internal_responses.clone(),
        app_state.tcp_channel.clone(),
    );
    #[cfg(not(feature = "ipc_channel"))]
    let internal_client = crate::internal_messaging::InternalMessagingClient::new(
        init.module_id.clone(),
        app_state.pending_internal_responses.clone(),
    );
    app_state.internal_messaging_client = Some(internal_client);
    
    // 9. Set up module message handlers manager
    app_state.module_message_handlers = Some(Arc::new(Mutex::new(HashMap::new())));
    
    // 10. Spawn IPC processing loop for message handling
    #[cfg(feature = "ipc_channel")]
    let processing_handle = tokio::spawn(process_ipc_messages());
    #[cfg(not(feature = "ipc_channel"))]
    let processing_handle = tokio::spawn(async { /* Empty task */ });

    debug!("Bootstrapped PyWatt module successfully");
    info!("Module bootstrap complete - ID: {}, listening on: {}", init.module_id, init.listen_to_string());

    Ok((app_state, processing_handle))
}

/// Extension trait for TCP channels to check connection status
#[allow(dead_code)]
trait TcpChannelExt {
    /// Checks if the TCP channel is permanently closed and cannot be reconnected
    async fn is_permanently_closed(&self) -> bool;
    
    /// Attempts to reconnect a closed TCP channel
    async fn try_reconnect(&self) -> Result<(), Error>;
}

impl TcpChannelExt for TcpChannel {
    async fn is_permanently_closed(&self) -> bool {
        // Check if the connection has been explicitly marked as permanently closed
        if matches!(self.state().await, ConnectionState::Failed) {
            // You might want to implement additional logic here to determine
            // if it's a temporary or permanent closure
            return true;
        }
        false
    }
    
    async fn try_reconnect(&self) -> Result<(), Error> {
        // Implement reconnection logic here
        // This would try to re-establish the connection if it was temporarily lost
        if matches!(self.state().await, ConnectionState::Disconnected | ConnectionState::Failed) {
            // In a real implementation, this would attempt to recreate the TCP connection
            // For now, we just return an error indicating we can't reconnect
            return Err(Error::Config(crate::error::ConfigError::Invalid(
                "TCP channel is closed and reconnection is not yet implemented".to_string(),
            )));
        }
        Ok(())
    }
}

/// Provides method to register a module message handler for a specific source module
#[allow(dead_code)]
pub trait AppStateExt<T: Send + Sync + 'static> {
    /// Register a handler for module-to-module messages from a specific source module
    async fn register_module_message_handler<F, Fut>(
        &self,
        source_module_id: String,
        handler: F,
    ) -> Result<(), Error>
    where
        F: Fn(String, uuid::Uuid, EncodedMessage) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), Error>> + Send + 'static;
        
    /// Remove a registered handler for a specific source module
    async fn remove_module_message_handler(&self, source_module_id: &str) -> Result<(), Error>;
}

impl<T: Send + Sync + 'static> AppStateExt<T> for AppState<T> {
    async fn register_module_message_handler<F, Fut>(
        &self,
        source_module_id: String,
        handler: F,
    ) -> Result<(), Error>
    where
        F: Fn(String, uuid::Uuid, EncodedMessage) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), Error>> + Send + 'static,
    {
        if let Some(ref handlers) = self.module_message_handlers {
            let mut handlers_lock = handlers.lock().await;
            
            // Create a handler function that wraps the provided closure
            let handler_fn = Arc::new(move |src: String, req_id: uuid::Uuid, payload: EncodedMessage| {
                let fut = handler(src, req_id, payload);
                Box::pin(fut) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Error>> + Send>>
            });
            
            // Insert the handler into the map
            handlers_lock.insert(source_module_id, handler_fn);
            Ok(())
        } else {
            Err(Error::Config(crate::error::ConfigError::Invalid(
                "Module message handlers not initialized in AppState".to_string(),
            )))
        }
    }
    
    async fn remove_module_message_handler(&self, source_module_id: &str) -> Result<(), Error> {
        if let Some(ref handlers) = self.module_message_handlers {
            let mut handlers_lock = handlers.lock().await;
            handlers_lock.remove(source_module_id);
            Ok(())
        } else {
            Err(Error::Config(crate::error::ConfigError::Invalid(
                "Module message handlers not initialized in AppState".to_string(),
            )))
        }
    }
}
