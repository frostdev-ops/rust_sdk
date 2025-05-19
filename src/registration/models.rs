//! Data models for module registration and capability advertisement.
//!
//! This module provides the data models used in the module registration protocol,
//! including module information, capabilities, and health status.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use thiserror::Error;
// Comment out the bincode imports that are causing issues
// use bincode::{Encode, Decode};

/// Information about a module for registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInfo {
    /// Unique name of the module
    pub name: String,
    
    /// Version of the module
    pub version: String,
    
    /// Description of the module
    pub description: String,
    
    /// Unique identifier for the module, if already known
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    
    /// Additional metadata about the module
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
}

impl ModuleInfo {
    /// Create a new module info with the given name, version, and description.
    pub fn new<S: Into<String>>(name: S, version: S, description: S) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: description.into(),
            id: None,
            metadata: None,
        }
    }
    
    /// Set a unique identifier for the module.
    pub fn with_id(mut self, id: Uuid) -> Self {
        self.id = Some(id);
        self
    }
    
    /// Add a metadata entry to the module info.
    pub fn with_metadata<S: Into<String>>(mut self, key: S, value: S) -> Self {
        if self.metadata.is_none() {
            self.metadata = Some(HashMap::new());
        }
        
        if let Some(metadata) = self.metadata.as_mut() {
            metadata.insert(key.into(), value.into());
        }
        
        self
    }
}

/// Information about an HTTP endpoint provided by a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    /// Path of the endpoint
    pub path: String,
    
    /// HTTP methods supported by the endpoint
    pub methods: Vec<String>,
    
    /// Authentication requirements for the endpoint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
    
    /// Additional metadata about the endpoint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
}

impl Endpoint {
    /// Create a new endpoint with the given path and methods.
    pub fn new<S: Into<String>>(path: S, methods: Vec<&str>) -> Self {
        Self {
            path: path.into(),
            methods: methods.into_iter().map(String::from).collect(),
            auth: None,
            metadata: None,
        }
    }
    
    /// Set authentication requirements for the endpoint.
    pub fn with_auth<S: Into<String>>(mut self, auth: S) -> Self {
        self.auth = Some(auth.into());
        self
    }
    
    /// Add a metadata entry to the endpoint.
    pub fn with_metadata<S: Into<String>>(mut self, key: S, value: S) -> Self {
        if self.metadata.is_none() {
            self.metadata = Some(HashMap::new());
        }
        
        if let Some(metadata) = self.metadata.as_mut() {
            metadata.insert(key.into(), value.into());
        }
        
        self
    }
}

/// Capabilities advertised by a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    /// HTTP endpoints provided by the module
    pub endpoints: Vec<Endpoint>,
    
    /// Message types that the module can handle
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_types: Option<Vec<String>>,
    
    /// Additional capabilities of the module
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_capabilities: Option<HashMap<String, serde_json::Value>>,
}

impl Capabilities {
    /// Create a new empty capabilities object.
    pub fn new() -> Self {
        Self {
            endpoints: Vec::new(),
            message_types: None,
            additional_capabilities: None,
        }
    }
    
    /// Add an HTTP endpoint to the capabilities.
    pub fn with_http_endpoint<S: Into<String>>(mut self, path: S, methods: Vec<&str>) -> Self {
        self.endpoints.push(Endpoint::new(path, methods));
        self
    }
    
    /// Add a message type to the capabilities.
    pub fn with_message_type<S: Into<String>>(mut self, message_type: S) -> Self {
        if self.message_types.is_none() {
            self.message_types = Some(Vec::new());
        }
        
        if let Some(message_types) = self.message_types.as_mut() {
            message_types.push(message_type.into());
        }
        
        self
    }
    
    /// Add an additional capability to the module.
    pub fn with_capability<S: Into<String>, V: Serialize>(
        mut self,
        key: S,
        value: V,
    ) -> Result<Self, serde_json::Error> {
        if self.additional_capabilities.is_none() {
            self.additional_capabilities = Some(HashMap::new());
        }
        
        if let Some(capabilities) = self.additional_capabilities.as_mut() {
            capabilities.insert(key.into(), serde_json::to_value(value)?);
        }
        
        Ok(self)
    }
}

impl Default for Capabilities {
    fn default() -> Self {
        Self::new()
    }
}

/// Health status of a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Module is healthy and functioning normally
    Healthy,
    
    /// Module is functioning but with degraded performance or capabilities
    Degraded,
    
    /// Module is unhealthy and not functioning correctly
    Unhealthy,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthStatus::Healthy => write!(f, "healthy"),
            HealthStatus::Degraded => write!(f, "degraded"),
            HealthStatus::Unhealthy => write!(f, "unhealthy"),
        }
    }
}

/// A registered module with the orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredModule {
    /// Information about the module
    pub info: ModuleInfo,
    
    /// Unique identifier assigned by the orchestrator
    pub id: Uuid,
    
    /// Connection token for secure communication
    pub token: String,
    
    /// Hostname of the orchestrator
    pub orchestrator_host: String,
    
    /// Port of the orchestrator
    pub orchestrator_port: u16,
    
    /// Environment variables provided by the orchestrator
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    
    /// Additional registration data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_data: Option<HashMap<String, serde_json::Value>>,
}

/// Errors that can occur during module registration.
#[derive(Debug, Error)]
pub enum RegistrationError {
    /// The module name is already taken.
    #[error("Module name '{0}' is already taken")]
    NameTaken(String),
    
    /// The connection to the orchestrator failed.
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    /// The registration was rejected by the orchestrator.
    #[error("Registration rejected: {0}")]
    Rejected(String),
    
    /// Authentication failed.
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),
    
    /// An error occurred during serialization or deserialization.
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    /// An error occurred in the TCP channel.
    #[error("Channel error: {0}")]
    ChannelError(String),
    
    /// The registration timed out.
    #[error("Registration timed out after {0:?}")]
    Timeout(std::time::Duration),
    
    /// An IO error occurred.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Registration request message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationRequest {
    /// Type of request
    pub request_type: String,
    /// Module information
    pub module_info: ModuleInfo,
}

/// Registration response message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationResponse {
    /// Response type identifier
    pub response_type: String,
    
    /// Whether registration was successful
    pub success: bool,
    
    /// Error message if registration failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    
    /// Registered module information if successful
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<RegisteredModule>,
}

/// Heartbeat request message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    /// Type of request
    pub request_type: String,
    /// Module ID
    pub module_id: Uuid,
    /// Authentication token
    pub token: String,
    /// Current health status
    pub status: HealthStatus,
    /// Additional health information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<HashMap<String, String>>,
}

/// Heartbeat response message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    /// Response type identifier
    pub response_type: String,
    
    /// Whether heartbeat was successful
    pub success: bool,
    
    /// Error message if heartbeat failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    
    /// Current health status from orchestrator's perspective
    pub status: HealthStatus,
    
    /// Additional context about the heartbeat
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<HashMap<String, String>>,
}

/// Capabilities advertisement request message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesRequest {
    /// Type of request
    pub request_type: String,
    /// Module ID
    pub module_id: Uuid,
    /// Authentication token
    pub token: String,
    /// Module capabilities
    pub capabilities: Capabilities,
}

/// Capabilities advertisement response message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesResponse {
    /// Type of response
    pub response_type: String,
    /// Success status
    pub success: bool,
    /// Error message if unsuccessful
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Unregistration request message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnregistrationRequest {
    /// Type of request
    pub request_type: String,
    /// Module ID
    pub module_id: Uuid,
    /// Authentication token
    pub token: String,
}

/// Unregistration response message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnregistrationResponse {
    /// Type of response
    pub response_type: String,
    /// Success status
    pub success: bool,
    /// Error message if unsuccessful
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
} 