use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use uuid::Uuid;

// Add missing import for EncodedMessage
use crate::message::EncodedMessage;

/// Address to listen on, either TCP or Unix domain socket.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum ListenAddress {
    /// TCP socket address
    Tcp(SocketAddr),
    /// Unix domain socket path
    Unix(PathBuf),
}

/// Sent from Orchestrator -> Module on startup.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InitBlob {
    pub orchestrator_api: String,
    pub module_id: String,
    /// Any secrets the orchestrator already knows that the module will need immediately
    pub env: HashMap<String, String>,
    /// Listen address assigned by orchestrator
    pub listen: ListenAddress,
}

/// Information about a single HTTP/WebSocket endpoint provided by a module.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EndpointAnnounce {
    pub path: String,
    pub methods: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
}

/// Sent from Module -> Orchestrator once the module has bound its listener.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AnnounceBlob {
    /// The socket address the server actually bound to (e.g. "127.0.0.1:4102").
    pub listen: String,
    /// All endpoints exposed by the module.
    pub endpoints: Vec<EndpointAnnounce>,
}

/// Sent from Module -> Orchestrator to fetch a secret.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetSecretRequest {
    /// Name of the secret.
    pub name: String,
}

/// Sent from Orchestrator -> Module in response to `GetSecret` **or** proactively as part of a
/// rotation flow.  When used for rotation the `rotation_id` field will be `Some(..)` so the module
/// can acknowledge receipt.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SecretValueResponse {
    pub name: String,
    pub value: String,
    /// Identifier used when this message is part of a rotation batch.  `None` means this was a
    /// regular on-demand secret fetch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation_id: Option<String>,
}

/// Batched notification that a group of secrets have been rotated.  The module should invalidate
/// any cached values for the listed keys and call `get_secret` again.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RotatedNotification {
    pub keys: Vec<String>,
    pub rotation_id: String,
}

/// Sent from Module -> Orchestrator after processing a rotation batch.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RotationAckRequest {
    pub rotation_id: String,
    pub status: String, // typically "success" or "error"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>, // optional human-readable context
}

/// Type of service
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServiceType {
    /// Database service
    Database,
    /// Cache service
    Cache,
    /// JWT authentication service
    Jwt,
}

/// Request a service connection from the orchestrator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceRequest {
    /// Identifier for the service
    pub id: String,
    /// Type of service
    pub service_type: ServiceType,
    /// Optional configuration override
    pub config: Option<serde_json::Value>,
}

/// Response to a service request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceResponse {
    /// ID from the request
    pub id: String,
    /// Type of service
    pub service_type: ServiceType,
    /// Whether the request was successful
    pub success: bool,
    /// Error message if unsuccessful
    pub error: Option<String>,
    /// Unique ID for the connection
    pub connection_id: Option<String>,
}

/// Perform an operation on a service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceOperation {
    /// ID of the connection to use
    pub connection_id: String,
    /// Type of service
    pub service_type: ServiceType,
    /// Name of the operation
    pub operation: String,
    /// Parameters for the operation
    pub params: serde_json::Value,
}

/// Result of a service operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceOperationResult {
    /// Whether the operation was successful
    pub success: bool,
    /// Result data if successful
    pub result: Option<serde_json::Value>,
    /// Error message if unsuccessful
    pub error: Option<String>,
}

/// HTTP request from orchestrator to module
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IpcHttpRequest {
    pub request_id: String,
    pub method: String,
    pub uri: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

/// HTTP response from module to orchestrator
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IpcHttpResponse {
    pub request_id: String,
    pub status_code: u16,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

/// Messages sent **from** a module **to** the orchestrator. These map 1-to-1 with what the legacy
/// `secret_client` library called `ClientRequest` but we keep that alias as well.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "op")]
pub enum ModuleToOrchestrator {
    #[serde(rename = "announce")]
    Announce(AnnounceBlob),
    #[serde(rename = "get_secret")]
    GetSecret(GetSecretRequest),
    #[serde(rename = "rotation_ack")]
    RotationAck(RotationAckRequest),
    #[serde(rename = "service_request")]
    ServiceRequest(ServiceRequest),
    #[serde(rename = "service_operation")]
    ServiceOperation(ServiceOperation),
    #[serde(rename = "http_response")]
    HttpResponse(IpcHttpResponse),
    /// Message from a module intended for another module, to be routed by the orchestrator.
    #[serde(rename = "route_to_module")]
    RouteToModule {
        /// The unique identifier of the target module.
        target_module_id: String,
        /// A string identifying the target service or endpoint within the target module.
        target_endpoint: String,
        /// The unique ID for this request, used for correlating responses.
        request_id: Uuid,
        /// The actual payload, encoded using the module's preferred format.
        payload: EncodedMessage,
    },
    /// Acknowledge receipt of a heartbeat from the orchestrator
    #[serde(rename = "heartbeat_ack")]
    HeartbeatAck,
}

/// Messages sent **from** the orchestrator **to** a module.  This replaces the old
/// `ServerResponse` from `secret_client`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "op")]
pub enum OrchestratorToModule {
    #[serde(rename = "init")]
    Init(InitBlob),
    #[serde(rename = "secret")]
    Secret(SecretValueResponse),
    #[serde(rename = "rotated")]
    Rotated(RotatedNotification),
    /// Instructs module to shutdown gracefully
    #[serde(rename = "shutdown")]
    Shutdown,
    #[serde(rename = "service_response")]
    ServiceResponse(ServiceResponse),
    #[serde(rename = "service_operation_result")]
    ServiceOperationResult(ServiceOperationResult),
    #[serde(rename = "http_request")]
    HttpRequest(IpcHttpRequest),
    /// A message routed from another module via the orchestrator.
    #[serde(rename = "routed_module_message")]
    RoutedModuleMessage {
        /// The unique identifier of the module that sent the original message.
        source_module_id: String,
        /// The unique ID of the original request, to be echoed in the response by the handling module.
        original_request_id: Uuid,
        /// The actual payload, encoded using the sender module's preferred format.
        payload: EncodedMessage,
    },
    /// A response to a `RouteToModule` request, routed back by the orchestrator.
    #[serde(rename = "routed_module_response")]
    RoutedModuleResponse {
        /// The unique identifier of the module that is sending this response (the one that handled the request).
        source_module_id: String,
        /// The unique ID of the original request this response corresponds to.
        request_id: Uuid,
        /// The actual payload of the response, encoded. This should ideally be a Result<ActualResponse, ApplicationError>.
        payload: EncodedMessage,
    },
    /// Heartbeat message to check module health
    #[serde(rename = "heartbeat")]
    Heartbeat,
}

// ----- Legacy aliases to ease incremental migration -----

// Concise aliases preferred by module code to avoid deep type names
pub type Init = InitBlob;
pub type Announce = AnnounceBlob;
pub type Endpoint = EndpointAnnounce;

pub use ModuleToOrchestrator as ClientRequest;
pub use OrchestratorToModule as ServerResponse;
