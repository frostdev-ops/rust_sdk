//! Module registration protocol for PyWatt modules.
//!
//! This module provides the functionality for modules to register with the orchestrator,
//! advertise their capabilities, and maintain their lifecycle.

pub mod models;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::message::{Message, MessageMetadata};
use crate::tcp_channel::{MessageChannel, TcpChannel};
use crate::tcp_types::ConnectionConfig;

pub use models::*;

/// Result type for registration operations.
pub type Result<T> = std::result::Result<T, RegistrationError>;

// Default timeout for registration operations
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Shared module registry to maintain connection to registered modules.
struct Registry {
    /// Map of module ID to channel
    modules: HashMap<Uuid, Arc<TcpChannel>>,
}

// Singleton registry for all registered modules
static REGISTRY: once_cell::sync::Lazy<Mutex<Registry>> = once_cell::sync::Lazy::new(|| {
    Mutex::new(Registry {
        modules: HashMap::new(),
    })
});

/// Register a module with the orchestrator.
///
/// # Arguments
/// * `config` - Connection configuration for the orchestrator
/// * `info` - Information about the module to register
///
/// # Returns
/// A registered module if successful
pub async fn register_module(config: ConnectionConfig, info: ModuleInfo) -> Result<RegisteredModule> {
    // Connect to the orchestrator
    let channel = TcpChannel::connect(config.clone()).await
        .map_err(|e| RegistrationError::ConnectionError(e.to_string()))?;
    
    // Create registration request
    let request = RegistrationRequest {
        request_type: "register".to_string(),
        module_info: info.clone(),
    };
    
    // Create message with request ID
    let mut metadata = MessageMetadata::new();
    metadata.id = Some(Uuid::new_v4().to_string());
    
    let message = Message::with_metadata(request, metadata);
    let encoded = message.encode()
        .map_err(|e| RegistrationError::SerializationError(e.to_string()))?;
    
    // Send the request
    debug!("Sending registration request to orchestrator");
    channel.send(encoded).await
        .map_err(|e| RegistrationError::ChannelError(e.to_string()))?;
    
    // Wait for response with timeout
    debug!("Waiting for registration response");
    let response_encoded = timeout(DEFAULT_TIMEOUT, channel.receive()).await
        .map_err(|_| RegistrationError::Timeout(DEFAULT_TIMEOUT))?
        .map_err(|e| RegistrationError::ChannelError(e.to_string()))?;
    
    // Decode response
    let response_data: RegistrationResponse = serde_json::from_slice(response_encoded.data())
        .map_err(|e| RegistrationError::SerializationError(e.to_string()))?;
    
    // Check response
    if !response_data.success {
        return Err(RegistrationError::Rejected(
            response_data.error.clone().unwrap_or_else(|| "Unknown error".to_string())
        ));
    }
    
    // Get registered module from response
    let registered_module = response_data.module.clone()
        .ok_or_else(|| RegistrationError::Rejected("No module information in response".to_string()))?;
    
    // Store channel in registry for later use
    let mut registry = REGISTRY.lock().await;
    registry.modules.insert(registered_module.id, Arc::new(channel));
    
    info!("Successfully registered module {}", registered_module.id);
    Ok(registered_module)
}

/// Unregister a module from the orchestrator.
///
/// # Arguments
/// * `module` - The registered module to unregister
///
/// # Returns
/// `()` if successful
pub async fn unregister_module(module: &RegisteredModule) -> Result<()> {
    // Get channel from registry
    let channel = {
        let registry = REGISTRY.lock().await;
        registry.modules.get(&module.id).cloned()
    };
    
    let channel = match channel {
        Some(channel) => channel,
        None => {
            // Connect to the orchestrator
            let config = ConnectionConfig::new(
                module.orchestrator_host.clone(),
                module.orchestrator_port,
            );
            
            Arc::new(TcpChannel::connect(config).await
                .map_err(|e| RegistrationError::ConnectionError(e.to_string()))?)
        }
    };
    
    // Create unregistration request
    let request = UnregistrationRequest {
        request_type: "unregister".to_string(),
        module_id: module.id,
        token: module.token.clone(),
    };
    
    // Create message with request ID
    let mut metadata = MessageMetadata::new();
    metadata.id = Some(Uuid::new_v4().to_string());
    
    let message = Message::with_metadata(request, metadata);
    let encoded = message.encode()
        .map_err(|e| RegistrationError::SerializationError(e.to_string()))?;
    
    // Send the request
    debug!("Sending unregistration request to orchestrator");
    channel.send(encoded).await
        .map_err(|e| RegistrationError::ChannelError(e.to_string()))?;
    
    // Wait for response with timeout
    debug!("Waiting for unregistration response");
    let response_encoded = timeout(DEFAULT_TIMEOUT, channel.receive()).await
        .map_err(|_| RegistrationError::Timeout(DEFAULT_TIMEOUT))?
        .map_err(|e| RegistrationError::ChannelError(e.to_string()))?;
    
    // Decode response
    let response_data: UnregistrationResponse = serde_json::from_slice(response_encoded.data())
        .map_err(|e| RegistrationError::SerializationError(e.to_string()))?;
    
    // Check response
    if !response_data.success {
        return Err(RegistrationError::Rejected(
            response_data.error.clone().unwrap_or_else(|| "Unknown error".to_string())
        ));
    }
    
    // Remove channel from registry
    let mut registry = REGISTRY.lock().await;
    registry.modules.remove(&module.id);
    
    info!("Successfully unregistered module {}", module.id);
    Ok(())
}

/// Send a heartbeat to the orchestrator to report health status.
///
/// # Arguments
/// * `module` - The registered module
/// * `status` - Current health status
/// * `details` - Optional additional health details
///
/// # Returns
/// Current health status from orchestrator's perspective
pub async fn heartbeat(
    module: &RegisteredModule,
    status: HealthStatus,
    details: Option<HashMap<String, String>>,
) -> Result<HealthStatus> {
    // Get channel from registry
    let channel = {
        let registry = REGISTRY.lock().await;
        registry.modules.get(&module.id).cloned()
    };
    
    let channel = match channel {
        Some(channel) => channel,
        None => {
            // Connect to the orchestrator
            let config = ConnectionConfig::new(
                module.orchestrator_host.clone(),
                module.orchestrator_port,
            );
            
            Arc::new(TcpChannel::connect(config).await
                .map_err(|e| RegistrationError::ConnectionError(e.to_string()))?)
        }
    };
    
    // Create heartbeat request
    let request = HeartbeatRequest {
        request_type: "heartbeat".to_string(),
        module_id: module.id,
        token: module.token.clone(),
        status,
        details,
    };
    
    // Create message with request ID
    let mut metadata = MessageMetadata::new();
    metadata.id = Some(Uuid::new_v4().to_string());
    
    let message = Message::with_metadata(request, metadata);
    let encoded = message.encode()
        .map_err(|e| RegistrationError::SerializationError(e.to_string()))?;
    
    // Send the request
    debug!("Sending heartbeat request to orchestrator");
    channel.send(encoded).await
        .map_err(|e| RegistrationError::ChannelError(e.to_string()))?;
    
    // Wait for response with timeout
    debug!("Waiting for heartbeat response");
    let response_encoded = timeout(DEFAULT_TIMEOUT, channel.receive()).await
        .map_err(|_| RegistrationError::Timeout(DEFAULT_TIMEOUT))?
        .map_err(|e| RegistrationError::ChannelError(e.to_string()))?;
    
    // Decode response
    let response_data: HeartbeatResponse = serde_json::from_slice(response_encoded.data())
        .map_err(|e| RegistrationError::SerializationError(e.to_string()))?;
    
    // Check response
    if !response_data.success {
        return Err(RegistrationError::Rejected(
            response_data.error.clone().unwrap_or_else(|| "Unknown error".to_string())
        ));
    }
    
    // Return health status
    let status = response_data.status;
    debug!("Heartbeat successful, status: {:?}", status);
    Ok(status)
}

/// Advertise capabilities to the orchestrator.
///
/// # Arguments
/// * `module` - The registered module
/// * `capabilities` - Capabilities to advertise
///
/// # Returns
/// `()` if successful
pub async fn advertise_capabilities(module: &RegisteredModule, capabilities: Capabilities) -> Result<()> {
    // Get channel from registry
    let channel = {
        let registry = REGISTRY.lock().await;
        registry.modules.get(&module.id).cloned()
    };
    
    let channel = match channel {
        Some(channel) => channel,
        None => {
            // Connect to the orchestrator
            let config = ConnectionConfig::new(
                module.orchestrator_host.clone(),
                module.orchestrator_port,
            );
            
            Arc::new(TcpChannel::connect(config).await
                .map_err(|e| RegistrationError::ConnectionError(e.to_string()))?)
        }
    };
    
    // Create capabilities request
    let request = CapabilitiesRequest {
        request_type: "capabilities".to_string(),
        module_id: module.id,
        token: module.token.clone(),
        capabilities,
    };
    
    // Create message with request ID
    let mut metadata = MessageMetadata::new();
    metadata.id = Some(Uuid::new_v4().to_string());
    
    let message = Message::with_metadata(request, metadata);
    let encoded = message.encode()
        .map_err(|e| RegistrationError::SerializationError(e.to_string()))?;
    
    // Send the request
    debug!("Sending capabilities request to orchestrator");
    channel.send(encoded).await
        .map_err(|e| RegistrationError::ChannelError(e.to_string()))?;
    
    // Wait for response with timeout
    debug!("Waiting for capabilities response");
    let response_encoded = timeout(DEFAULT_TIMEOUT, channel.receive()).await
        .map_err(|_| RegistrationError::Timeout(DEFAULT_TIMEOUT))?
        .map_err(|e| RegistrationError::ChannelError(e.to_string()))?;
    
    // Decode response
    let response_data: CapabilitiesResponse = serde_json::from_slice(response_encoded.data())
        .map_err(|e| RegistrationError::SerializationError(e.to_string()))?;
    
    // Check response
    if !response_data.success {
        return Err(RegistrationError::Rejected(
            response_data.error.clone().unwrap_or_else(|| "Unknown error".to_string())
        ));
    }
    
    info!("Successfully advertised capabilities for module {}", module.id);
    Ok(())
}

/// Start a heartbeat loop in a background task.
///
/// # Arguments
/// * `module` - The registered module
/// * `interval` - Interval between heartbeats
/// * `status_provider` - Function that returns the current health status
///
/// # Returns
/// A handle to the heartbeat task
pub fn start_heartbeat_loop<F>(
    module: RegisteredModule,
    interval: Duration,
    mut status_provider: F,
) -> tokio::task::JoinHandle<()>
where
    F: FnMut() -> (HealthStatus, Option<HashMap<String, String>>) + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            // Get current status
            let (status, details) = status_provider();
            
            // Send heartbeat
            match heartbeat(&module, status, details).await {
                Ok(_) => {
                    debug!("Heartbeat sent successfully");
                }
                Err(e) => {
                    warn!("Failed to send heartbeat: {}", e);
                }
            }
            
            // Wait for next interval
            tokio::time::sleep(interval).await;
        }
    })
}

/// A simple wrapper for the heartbeat function that doesn't require additional details.
///
/// # Arguments
/// * `module` - The registered module
///
/// # Returns
/// Current health status from orchestrator's perspective
pub async fn simple_heartbeat(module: &RegisteredModule) -> Result<HealthStatus> {
    heartbeat(module, HealthStatus::Healthy, None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    
    // Mock orchestrator for testing
    async fn mock_orchestrator(port: u16) -> std::io::Result<tokio::task::JoinHandle<()>> {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
        
        let handle = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                loop {
                    // Read the length header (4 bytes)
                    let mut len_bytes = [0u8; 4];
                    if socket.read_exact(&mut len_bytes).await.is_err() {
                        break;
                    }
                    let len = u32::from_be_bytes(len_bytes) as usize;
                    
                    // Read the format byte (1 byte)
                    let mut format_byte = [0u8; 1];
                    if socket.read_exact(&mut format_byte).await.is_err() {
                        break;
                    }
                    
                    // Read the message data
                    let mut data = vec![0u8; len];
                    if socket.read_exact(&mut data).await.is_err() {
                        break;
                    }
                    
                    // Parse the message
                    let message_str = String::from_utf8_lossy(&data);
                    
                    // Construct response based on request
                    let response = if message_str.contains("\"request_type\":\"register\"") {
                        // Registration response
                        let module = RegisteredModule {
                            info: ModuleInfo::new("test", "1.0.0", "Test module"),
                            id: Uuid::new_v4(),
                            token: "test-token".to_string(),
                            orchestrator_host: "127.0.0.1".to_string(),
                            orchestrator_port: port,
                            env: None,
                            additional_data: None,
                        };
                        
                        // Create a direct RegistrationResponse structure
                        let reg_response = RegistrationResponse {
                            response_type: "register".to_string(),
                            success: true,
                            error: None,
                            module: Some(module),
                        };
                        
                        serde_json::to_value(reg_response).unwrap()
                    } else if message_str.contains("\"request_type\":\"heartbeat\"") {
                        // Heartbeat response
                        let heartbeat_response = HeartbeatResponse {
                            response_type: "heartbeat".to_string(),
                            success: true,
                            error: None,
                            status: HealthStatus::Healthy,
                            context: None,
                        };
                        
                        serde_json::to_value(heartbeat_response).unwrap()
                    } else if message_str.contains("\"request_type\":\"capabilities\"") {
                        // Capabilities response
                        let capabilities_response = CapabilitiesResponse {
                            response_type: "capabilities".to_string(),
                            success: true,
                            error: None,
                        };
                        
                        serde_json::to_value(capabilities_response).unwrap()
                    } else if message_str.contains("\"request_type\":\"unregister\"") {
                        // Unregistration response
                        let unregister_response = UnregistrationResponse {
                            response_type: "unregister".to_string(),
                            success: true,
                            error: None,
                        };
                        
                        serde_json::to_value(unregister_response).unwrap()
                    } else {
                        // Unknown request - doesn't need content/metadata wrapper
                        serde_json::json!({
                            "response_type": "unknown",
                            "success": false,
                            "error": "Unknown request type"
                        })
                    };
                    
                    // Serialize response
                    let response_json = serde_json::to_string(&response).unwrap();
                    let response_bytes = response_json.as_bytes();
                    
                    // Write the length header (4 bytes)
                    let length = response_bytes.len() as u32;
                    let len_bytes = length.to_be_bytes();
                    let _ = socket.write_all(&len_bytes).await;
                    
                    // Write the format byte (1 byte) - JSON
                    let _ = socket.write_all(&[1u8]).await;
                    
                    // Write the response data
                    let _ = socket.write_all(response_bytes).await;
                    let _ = socket.flush().await;
                }
            }
        });
        
        // Give the server a moment to start
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        Ok(handle)
    }
    
    #[tokio::test]
    async fn test_register_module() -> Result<()> {
        // Start mock orchestrator
        let port = 9901;
        let _orchestrator = mock_orchestrator(port).await.unwrap();
        
        // Create module info
        let info = ModuleInfo::new("test-module", "1.0.0", "Test module");
        
        // Create connection config
        let config = ConnectionConfig::new("127.0.0.1", port);
        
        // Register module
        let module = register_module(config, info).await?;
        
        assert_eq!(module.info.name, "test");
        assert_eq!(module.info.version, "1.0.0");
        assert_eq!(module.info.description, "Test module");
        assert_eq!(module.token, "test-token");
        assert_eq!(module.orchestrator_host, "127.0.0.1");
        assert_eq!(module.orchestrator_port, port);
        
        Ok(())
    }
    
    #[tokio::test]
    async fn test_heartbeat() -> Result<()> {
        // Start mock orchestrator
        let port = 9902;
        let _orchestrator = mock_orchestrator(port).await.unwrap();
        
        // Create registered module
        let module = RegisteredModule {
            info: ModuleInfo::new("test", "1.0.0", "Test module"),
            id: Uuid::new_v4(),
            token: "test-token".to_string(),
            orchestrator_host: "127.0.0.1".to_string(),
            orchestrator_port: port,
            env: None,
            additional_data: None,
        };
        
        // Send heartbeat
        let status = heartbeat(&module, HealthStatus::Healthy, None).await?;
        
        assert_eq!(status, HealthStatus::Healthy);
        
        Ok(())
    }
    
    #[tokio::test]
    async fn test_advertise_capabilities() -> Result<()> {
        // Start mock orchestrator
        let port = 9903;
        let _orchestrator = mock_orchestrator(port).await.unwrap();
        
        // Create registered module
        let module = RegisteredModule {
            info: ModuleInfo::new("test", "1.0.0", "Test module"),
            id: Uuid::new_v4(),
            token: "test-token".to_string(),
            orchestrator_host: "127.0.0.1".to_string(),
            orchestrator_port: port,
            env: None,
            additional_data: None,
        };
        
        // Create capabilities
        let capabilities = Capabilities::new()
            .with_http_endpoint("/api/test", vec!["GET", "POST"])
            .with_message_type("test_message");
        
        // Advertise capabilities
        advertise_capabilities(&module, capabilities).await?;
        
        Ok(())
    }
    
    #[tokio::test]
    async fn test_unregister_module() -> Result<()> {
        // Start mock orchestrator
        let port = 9904;
        let _orchestrator = mock_orchestrator(port).await.unwrap();
        
        // Create registered module
        let module = RegisteredModule {
            info: ModuleInfo::new("test", "1.0.0", "Test module"),
            id: Uuid::new_v4(),
            token: "test-token".to_string(),
            orchestrator_host: "127.0.0.1".to_string(),
            orchestrator_port: port,
            env: None,
            additional_data: None,
        };
        
        // Unregister module
        unregister_module(&module).await?;
        
        Ok(())
    }
} 