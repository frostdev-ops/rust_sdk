//! Provides a client for modules to send messages to other modules via the orchestrator.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::{oneshot, Mutex};
use uuid::Uuid;

use crate::{
    ipc_types::ModuleToOrchestrator,
    message::{EncodedMessage, Message, MessageError, EncodingFormat},
    tcp_channel::{TcpChannel, MessageChannel}, // Import MessageChannel trait
    Error as SdkError,
};

/// Errors that can occur during internal module-to-module messaging.
#[derive(thiserror::Error, Debug)]
pub enum InternalMessagingError {
    #[error("Failed to serialize request payload: {0}")]
    SerializationError(#[from] MessageError),

    #[error("Network error while sending message to orchestrator: {0}")]
    NetworkError(SdkError), // Assuming SdkError can represent TcpChannel send errors

    #[error("Request timed out while waiting for response from target module")]
    Timeout,

    #[error("Orchestrator indicated target module '{0}' was not found")]
    TargetModuleNotFound(String),

    #[error("Orchestrator indicated target endpoint '{0}' on module '{1}' was not found")]
    TargetEndpointNotFound(String, String),

    #[error("Target module responded with an application error: {0}")]
    ApplicationError(String), // This would be deserialized from the response payload

    #[error("Failed to deserialize response payload: {0}")]
    DeserializationError(MessageError),

    #[error("Internal SDK error: {0}")]
    InternalSDKError(String),

    #[error("Orchestrator error: {0}")]
    OrchestratorError(String),
}

impl From<SdkError> for InternalMessagingError {
    fn from(e: SdkError) -> Self {
        InternalMessagingError::NetworkError(e)
    }
}

impl From<MessageError> for SdkError {
    fn from(e: MessageError) -> Self {
        // Use a suitable variant of SdkError for MessageError
        SdkError::Config(crate::error::ConfigError::Invalid(e.to_string()))
    }
}

/// Alias for a Result type using `InternalMessagingError`.
pub type InternalMessagingResult<T> = Result<T, InternalMessagingError>;

// Type alias for the map of pending responses.
// The SdkError here is for when the orchestrator itself sends an error response instead of a payload.
// The EncodedMessage is the successful payload from the target module.
pub(crate) type PendingInternalResponses =
    Arc<Mutex<HashMap<Uuid, oneshot::Sender<Result<EncodedMessage, SdkError>>>>>;

/// Client for sending messages to other modules via the orchestrator.
#[derive(Clone, Debug)]
pub struct InternalMessagingClient {
    // Channel to send messages to the orchestrator.
    orchestrator_channel: Arc<TcpChannel>, // This is the module's main channel to Wattson
    // Map of pending requests awaiting responses.
    pending_responses: PendingInternalResponses,
    // Default encoding format for requests.
    default_encoding: EncodingFormat,
}

impl InternalMessagingClient {
    /// Creates a new `InternalMessagingClient`.
///
/// # Arguments
/// * `module_id` - The ID of this module
/// * `pending_responses` - A shared map for tracking pending internal requests.
/// * `orchestrator_channel` - Optional TCP channel for direct orchestrator communication
#[cfg(feature = "ipc_channel")]
pub fn new(
    _module_id: String,
    pending_responses: Option<PendingInternalResponses>,
    orchestrator_channel: Option<Arc<TcpChannel>>,
) -> Self {
    // Use the provided pending_responses or create a new empty one
    let responses = pending_responses.unwrap_or_else(|| {
        Arc::new(Mutex::new(HashMap::new()))
    });
    
    // Create client with the provided channel or a default empty one that will be initialized later
    let channel = orchestrator_channel.unwrap_or_else(|| {
        // This is a placeholder - in real code, you would want to initialize with a valid channel or handle the None case
        tracing::warn!("Creating InternalMessagingClient without a valid orchestrator_channel");
        Arc::new(TcpChannel::new(Default::default()))
    });
    
    Self {
        orchestrator_channel: channel,
        pending_responses: responses,
        default_encoding: EncodingFormat::Json,
    }
}

/// Creates a new `InternalMessagingClient` when TCP channel feature is disabled.
///
/// # Arguments
/// * `module_id` - The ID of this module
/// * `pending_responses` - A shared map for tracking pending internal requests.
#[cfg(not(feature = "ipc_channel"))]
pub fn new(
    _module_id: String,
    pending_responses: Option<PendingInternalResponses>,
) -> Self {
    // Use the provided pending_responses or create a new empty one
    let responses = pending_responses.unwrap_or_else(|| {
        Arc::new(Mutex::new(HashMap::new()))
    });
    
    Self {
        // This is a dummy channel that will never be used
        orchestrator_channel: Arc::new(TcpChannel::new(Default::default())),
        pending_responses: responses,
        default_encoding: EncodingFormat::Json,
    }
}

    /// Sends a request to another module and awaits its response.
    ///
    /// # Type Parameters
    /// * `Req`: The type of the request payload (must be `Serialize`).
    /// * `Res`: The type of the expected response payload (must be `DeserializeOwned`).
    ///
    /// # Arguments
    /// * `target_module_id` - The ID of the module to send the request to.
    /// * `target_endpoint` - A string identifying the specific service or endpoint on the target module.
    /// * `request_payload` - The actual data to send as the request.
    /// * `timeout` - Optional duration to wait for a response.
    ///
    /// # Returns
    /// A `Result` containing the deserialized response payload `Res` on success, or an `InternalMessagingError`.
    pub async fn send_request<
        Req: Serialize + Debug + Clone,
        Res: DeserializeOwned + Debug,
    >(
        &self,
        target_module_id: String,
        target_endpoint: String,
        request_payload: Req,
        _timeout: Option<std::time::Duration>, // Timeout functionality to be implemented
    ) -> InternalMessagingResult<Res> {
        let request_id = Uuid::new_v4();
        
        let message = Message::new(request_payload);
        let encoded_payload = EncodedMessage::encode_with_format(&message, self.default_encoding)
            .map_err(InternalMessagingError::SerializationError)?;

        let orchestrator_message = ModuleToOrchestrator::RouteToModule {
            target_module_id,
            target_endpoint,
            request_id,
            payload: encoded_payload,
        };

        // Create a one-shot channel to receive the response
        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self.pending_responses.lock().await;
            pending.insert(request_id, tx);
        }
        
        // Send the message to the orchestrator
        let outer_message = Message::new(orchestrator_message); // Wrap the enum variant
        let encoded_orchestrator_message = EncodedMessage::encode_with_format(&outer_message, self.default_encoding)
            .map_err(|e| InternalMessagingError::InternalSDKError(format!("Failed to encode orchestrator message: {}", e)))?;

        self.orchestrator_channel
            .send(encoded_orchestrator_message)
            .await
            .map_err(|e| InternalMessagingError::NetworkError(e.into()))?; // Convert SDK's network error

        // Await the response from the one-shot channel
        // TODO: Implement timeout logic
        match rx.await {
            Ok(Ok(encoded_response_payload)) => {
                // Successfully received EncodedMessage from target module
                // Use serde_json for decoding rather than using bincode::Decode
                let response: Res = match encoded_response_payload.format() {
                    crate::message::EncodingFormat::Json => {
                        // For JSON, we can use serde_json directly
                        let json_str = std::str::from_utf8(encoded_response_payload.data())
                            .map_err(|e| InternalMessagingError::DeserializationError(
                                MessageError::InvalidFormat(e.to_string())
                            ))?;
                        serde_json::from_str(json_str)
                            .map_err(|e| InternalMessagingError::DeserializationError(MessageError::JsonSerializationError(e)))?
                    },
                    _ => {
                        // For other formats, convert to JSON first, then decode
                        let json_encoded = encoded_response_payload.to_format(crate::message::EncodingFormat::Json)
                            .map_err(InternalMessagingError::DeserializationError)?;
                        let json_str = std::str::from_utf8(json_encoded.data())
                            .map_err(|e| InternalMessagingError::DeserializationError(
                                MessageError::InvalidFormat(e.to_string())
                            ))?;
                        serde_json::from_str(json_str)
                            .map_err(|e| InternalMessagingError::DeserializationError(MessageError::JsonSerializationError(e)))?
                    }
                };
                Ok(response)
            }
            Ok(Err(sdk_error)) => {
                // Orchestrator or SDK's TCP processing loop signaled an error for this request_id
                Err(InternalMessagingError::OrchestratorError(sdk_error.to_string()))
            }
            Err(_oneshot_cancelled) => {
                // The sender was dropped, implies the response processing part of the SDK shut down or panicked.
                Err(InternalMessagingError::InternalSDKError(
                    "Response channel closed prematurely; SDK might be shutting down".to_string(),
                ))
            }
        }
    }
}

// This function would be part of the main TCP processing loop in the SDK.
// It's responsible for dispatching incoming RoutedModuleResponse messages.
// This is illustrative and its actual integration depends on the SDK's TCP handling architecture.
#[allow(dead_code)]
pub(crate) async fn process_routed_module_response(
    response_id: Uuid,
    response_payload_or_error: Result<EncodedMessage, SdkError>,
    pending_responses: PendingInternalResponses,
) {
    if let Some(tx) = pending_responses.lock().await.remove(&response_id) {
        if let Err(_e) = tx.send(response_payload_or_error) {
            tracing::warn!(
                request_id = %response_id,
                "Failed to send routed module response to waiting task; receiver was dropped"
            );
        }
    } else {
        tracing::warn!(
            request_id = %response_id,
            "Received routed module response for an unknown or already handled request ID"
        );
    }
}
