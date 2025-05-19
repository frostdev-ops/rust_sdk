//! HTTP-over-TCP router and server implementation.
//!
//! This module provides routing and server functionality for HTTP requests over TCP
//! connections. It allows modules to define routes, middleware, and error handlers
//! for handling HTTP requests.

use std::collections::HashMap;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::sync::{broadcast, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info};

use crate::http_tcp::{HttpTcpRequest, HttpTcpResponse, not_found};
use crate::message::Message;
use crate::registration::{
    Capabilities, Endpoint, HealthStatus, RegisteredModule, advertise_capabilities,
    start_heartbeat_loop,
};
use crate::tcp_channel::TcpChannel;
use crate::tcp_types::ConnectionConfig;
use crate::tcp_channel::MessageChannel;

/// Errors specific to HTTP-over-TCP operations.
#[derive(Debug, Error)]
pub enum HttpTcpError {
    /// Invalid request format.
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    
    /// Resource not found.
    #[error("Not found: {0}")]
    NotFound(String),
    
    /// Authorization error.
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    
    /// Permission error.
    #[error("Forbidden: {0}")]
    Forbidden(String),
    
    /// Internal server error.
    #[error("Internal error: {0}")]
    Internal(String),
    
    /// Error from SDK.
    #[error("SDK error: {0}")]
    Sdk(#[from] Box<crate::error::Error>),
    
    /// JSON serialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    
    /// Timeout error.
    #[error("Timeout after {0:?}")]
    Timeout(Duration),
    
    /// Other errors.
    #[error("Other error: {0}")]
    Other(String),
}

/// Result type for HTTP-over-TCP operations.
pub type HttpTcpResult<T> = std::result::Result<T, HttpTcpError>;

/// Handler function type for route handlers.
pub type HandlerFn<S> = Arc<
    dyn Fn(
            HttpTcpRequest,
            Arc<S>,
        ) -> Pin<Box<dyn Future<Output = HttpTcpResult<HttpTcpResponse>> + Send>>
        + Send
        + Sync,
>;

/// A route entry in the router.
#[derive(Clone)]
pub struct Route<S> {
    /// HTTP method for this route.
    pub method: String,
    
    /// Path pattern for this route.
    pub path: String,
    
    /// Handler function for this route.
    pub handler: HandlerFn<S>,
}

/// HTTP router for handling requests over TCP.
#[derive(Clone)]
pub struct HttpTcpRouter<S: Send + Sync + Clone + 'static> {
    /// Routes registered with this router.
    routes: Arc<RwLock<Vec<Route<S>>>>,
    
    /// Handler for requests that don't match any route.
    not_found_handler: Arc<RwLock<Option<HandlerFn<S>>>>,
}

impl<S: Send + Sync + Clone + 'static> Default for HttpTcpRouter<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Send + Sync + Clone + 'static> HttpTcpRouter<S> {
    /// Create a new HTTP router.
    pub fn new() -> Self {
        Self {
            routes: Arc::new(RwLock::new(Vec::new())),
            not_found_handler: Arc::new(RwLock::new(None)),
        }
    }
    
    /// Add a route to the router.
    pub fn route<F, Fut>(self, method: &str, path: &str, handler: F) -> Self
    where
        F: Fn(HttpTcpRequest, Arc<S>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HttpTcpResult<HttpTcpResponse>> + Send + 'static,
    {
        let handler = Arc::new(move |req, state| {
            let fut = handler(req, state);
            Box::pin(fut) as Pin<Box<dyn Future<Output = HttpTcpResult<HttpTcpResponse>> + Send>>
        });
        
        let route = Route {
            method: method.to_uppercase(),
            path: path.to_string(),
            handler,
        };
        
        let routes_clone = self.routes.clone();
        tokio::spawn(async move {
            let mut routes = routes_clone.write().await;
            routes.push(route);
        });
        
        self
    }
    
    /// Set a custom not-found handler.
    pub fn not_found_handler<F, Fut>(self, handler: F) -> Self
    where
        F: Fn(HttpTcpRequest, Arc<S>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HttpTcpResult<HttpTcpResponse>> + Send + 'static,
    {
        let handler = Arc::new(move |req, state| {
            let fut = handler(req, state);
            Box::pin(fut) as Pin<Box<dyn Future<Output = HttpTcpResult<HttpTcpResponse>> + Send>>
        });
        
        let not_found_handler_clone = self.not_found_handler.clone();
        tokio::spawn(async move {
            let mut not_found = not_found_handler_clone.write().await;
            *not_found = Some(handler);
        });
        
        self
    }
    
    /// Add multiple methods for the same path.
    pub fn methods<F, Fut>(self, methods: Vec<&str>, path: &str, handler: F) -> Self
    where
        F: Fn(HttpTcpRequest, Arc<S>) -> Fut + Send + Sync + Clone + 'static,
        Fut: Future<Output = HttpTcpResult<HttpTcpResponse>> + Send + 'static,
    {
        let mut result = self;
        for method in methods {
            let handler_clone = handler.clone();
            result = result.route(method, path, handler_clone);
        }
        result
    }
    
    /// Handle a request.
    pub async fn handle_request(&self, request: HttpTcpRequest, state: Arc<S>) -> HttpTcpResponse {
        let method = request.method.to_uppercase();
        let path = request.path();
        
        debug!("Routing request: {} {}", method, path);
        
        // Find a matching route
        let routes = self.routes.read().await;
        for route in routes.iter() {
            if route.method == method && route.path == path {
                debug!("Found matching route: {} {}", route.method, route.path);
                
                // Call the handler and return the response
                match (route.handler)(request.clone(), state.clone()).await {
                    Ok(response) => {
                        return response;
                    }
                    Err(e) => {
                        error!("Handler error: {}", e);
                        return error_to_response(e, &request.request_id);
                    }
                }
            }
        }
        
        // No route found, use the not_found_handler if available
        let not_found_handler = self.not_found_handler.read().await;
        if let Some(handler) = not_found_handler.as_ref() {
            match handler(request.clone(), state).await {
                Ok(response) => {
                    return response;
                }
                Err(e) => {
                    error!("Not found handler error: {}", e);
                    return error_to_response(e, &request.request_id);
                }
            }
        } else {
            // Default not found response
            return not_found(&request.request_id, path);
        }
    }
    
    /// Get the list of routes for capability advertisement.
    pub async fn get_routes(&self) -> Vec<Endpoint> {
        let routes = self.routes.read().await;
        
        // Group routes by path
        let mut path_methods: HashMap<String, Vec<String>> = HashMap::new();
        
        for route in routes.iter() {
            let entry = path_methods.entry(route.path.clone()).or_insert_with(Vec::new);
            entry.push(route.method.clone());
        }
        
        // Convert to endpoints
        path_methods
            .into_iter()
            .map(|(path, methods)| Endpoint::new(path, methods.iter().map(|s| s.as_str()).collect()))
            .collect()
    }
}

/// Convert an error to an HTTP response.
fn error_to_response(error: HttpTcpError, request_id: &str) -> HttpTcpResponse {
    let status_code = match &error {
        HttpTcpError::InvalidRequest(_) => 400,
        HttpTcpError::NotFound(_) => 404,
        HttpTcpError::Unauthorized(_) => 401,
        HttpTcpError::Forbidden(_) => 403,
        HttpTcpError::Timeout(_) => 408,
        _ => 500,
    };
    
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    
    match serde_json::to_vec(&crate::http_tcp::ApiResponse::<()> {
        status: "error".to_string(),
        data: None,
        message: Some(error.to_string()),
    }) {
        Ok(body) => HttpTcpResponse {
            request_id: request_id.to_string(),
            status_code,
            headers,
            body: Some(body),
        },
        Err(_) => {
            // Fallback to plain text if JSON serialization fails
            headers.insert("Content-Type".to_string(), "text/plain".to_string());
            HttpTcpResponse {
                request_id: request_id.to_string(),
                status_code,
                headers,
                body: Some(error.to_string().into_bytes()),
            }
        }
    }
}

/// Receiver for HTTP requests.
#[derive(Debug, Clone)]
pub struct HttpReceiver {
    /// The TCP channel for receiving requests.
    pub channel: Arc<TcpChannel>,
    
    /// The registered module information.
    pub module: RegisteredModule,
}

/// Start the HTTP server with the given router.
///
/// # Arguments
/// * `router` - The HTTP router to use
/// * `module` - The registered module
/// * `state` - The application state
/// * `shutdown_signal` - Optional signal to stop the server
///
/// # Returns
/// A Result containing the server's join handle if successful
pub async fn start_http_server<S: Send + Sync + Clone + 'static>(
    router: HttpTcpRouter<S>,
    module: RegisteredModule,
    state: S,
    shutdown_signal: Option<broadcast::Receiver<()>>,
) -> crate::error::Result<JoinHandle<()>> {
    // Create a TCP channel for receiving requests
    let config = ConnectionConfig::new(
        module.orchestrator_host.clone(),
        module.orchestrator_port,
    );
    
    let channel = Arc::new(TcpChannel::connect(config).await?);
    
    // Get routes from the router
    let endpoints = router.get_routes().await;
    
    // Create capabilities
    let capabilities = Capabilities {
        endpoints,
        message_types: None,
        additional_capabilities: None,
    };
    
    // Advertise capabilities
    advertise_capabilities(&module, capabilities).await?;
    
    // Create a heartbeat task
    let _heartbeat_task = start_heartbeat_loop(
        module.clone(),
        Duration::from_secs(30),
        || (HealthStatus::Healthy, None),
    );
    
    // State for sharing
    let state = Arc::new(state);
    
    // Create the HTTP receiver
    let _receiver = HttpReceiver {
        channel: channel.clone(),
        module: module.clone(),
    };
    
    // Start the server
    info!("Starting HTTP-over-TCP server for module {}", module.info.name);
    
    let server_handle = if let Some(mut shutdown_signal) = shutdown_signal {
        tokio::spawn(async move {
            let router = router.clone();
            let state = state.clone();
            
            loop {
                tokio::select! {
                    _ = shutdown_signal.recv() => {
                        info!("Shutdown signal received, stopping HTTP-over-TCP server");
                        break;
                    }
                    message_result = channel.receive() => {
                        match message_result {
                            Ok(encoded_message) => {
                                // Decode the message
                                let request_json = match encoded_message.format() {
                                    crate::message::EncodingFormat::Json => {
                                        // If already JSON, just convert to string
                                        match std::str::from_utf8(encoded_message.data()) {
                                            Ok(s) => s.to_string(),
                                            Err(e) => {
                                                error!("Invalid UTF-8: {}", e);
                                                return;
                                            }
                                        }
                                    },
                                    _ => {
                                        // Convert to JSON first if not already
                                        let json_encoded = match encoded_message.to_format(crate::message::EncodingFormat::Json) {
                                            Ok(j) => j,
                                            Err(e) => {
                                                error!("Failed to convert message to JSON: {}", e);
                                                return;
                                            }
                                        };
                                        
                                        match std::str::from_utf8(json_encoded.data()) {
                                            Ok(s) => s.to_string(),
                                            Err(e) => {
                                                error!("Invalid UTF-8: {}", e);
                                                return;
                                            }
                                        }
                                    }
                                };

                                let message: Message<HttpTcpRequest> = match serde_json::from_str(&request_json) {
                                    Ok(msg) => msg,
                                    Err(e) => {
                                        error!("Failed to deserialize request: {}", e);
                                        return;
                                    }
                                };
                                        
                                let request = message.content();
                                info!("Received HTTP-over-TCP request: {} {}", request.method, request.uri);
                                
                                // Process in a separate task
                                let router_clone = router.clone();
                                let state_clone = state.clone();
                                let channel_clone = channel.clone();
                                let request_clone = request.clone();
                                
                                tokio::spawn(async move {
                                    // Handle the request
                                    let response = router_clone.handle_request(request_clone, state_clone).await;
                                    
                                    // Send the response
                                    let response_message = Message::new(response);
                                    match response_message.encode() {
                                        Ok(encoded) => {
                                            if let Err(e) = MessageChannel::send(&*channel_clone, encoded).await {
                                                error!("Failed to send HTTP-over-TCP response: {}", e);
                                            }
                                        }
                                        Err(e) => {
                                            error!("Failed to encode HTTP-over-TCP response: {}", e);
                                        }
                                    }
                                });
                            }
                            Err(e) => {
                                error!("Failed to receive HTTP-over-TCP request: {}", e);
                            }
                        }
                    }
                }
            }
            
            info!("HTTP-over-TCP server stopped");
        })
    } else {
        tokio::spawn(async move {
            let router = router.clone();
            let state = state.clone();
            
            loop {
                match MessageChannel::receive(&*channel).await {
                    Ok(encoded_message) => {
                        // Decode the message
                        let request_json = match encoded_message.format() {
                            crate::message::EncodingFormat::Json => {
                                // If already JSON, just convert to string
                                match std::str::from_utf8(encoded_message.data()) {
                                    Ok(s) => s.to_string(),
                                    Err(e) => {
                                        error!("Invalid UTF-8: {}", e);
                                        continue;
                                    }
                                }
                            },
                            _ => {
                                // Convert to JSON first if not already
                                let json_encoded = match encoded_message.to_format(crate::message::EncodingFormat::Json) {
                                    Ok(j) => j,
                                    Err(e) => {
                                        error!("Failed to convert message to JSON: {}", e);
                                        continue;
                                    }
                                };
                                
                                match std::str::from_utf8(json_encoded.data()) {
                                    Ok(s) => s.to_string(),
                                    Err(e) => {
                                        error!("Invalid UTF-8: {}", e);
                                        continue;
                                    }
                                }
                            }
                        };

                        let message: Message<HttpTcpRequest> = match serde_json::from_str(&request_json) {
                            Ok(msg) => msg,
                            Err(e) => {
                                error!("Failed to deserialize request: {}", e);
                                continue;
                            }
                        };
                                
                        let request = message.content();
                        info!("Received HTTP-over-TCP request: {} {}", request.method, request.uri);
                                
                        // Process in a separate task
                        let router_clone = router.clone();
                        let state_clone = state.clone();
                        let channel_clone = channel.clone();
                        let request_clone = request.clone();
                        
                        tokio::spawn(async move {
                            // Handle the request
                            let response = router_clone.handle_request(request_clone, state_clone).await;
                            
                            // Send the response
                            let response_message = Message::new(response);
                            match response_message.encode() {
                                Ok(encoded) => {
                                    if let Err(e) = MessageChannel::send(&*channel_clone, encoded).await {
                                        error!("Failed to send HTTP-over-TCP response: {}", e);
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to encode HTTP-over-TCP response: {}", e);
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to receive HTTP-over-TCP request: {}", e);
                        // Attempt to reconnect on next iteration
                    }
                }
            }
        })
    };
    
    Ok(server_handle)
} 