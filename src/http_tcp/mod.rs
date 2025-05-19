//! HTTP-over-TCP utilities for PyWatt modules.
//!
//! This module provides standardized utilities for handling HTTP requests and responses
//! over TCP connections. It simplifies the process of routing requests, formatting responses,
//! and handling errors in a consistent way across modules.

pub mod router;
pub mod client;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::registration::RegisteredModule;

pub use self::router::{HttpTcpRouter, Route, start_http_server};
pub use self::client::HttpTcpClient;

/// HTTP request transported over TCP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpTcpRequest {
    /// Unique identifier for the request
    pub request_id: String,
    
    /// HTTP method (GET, POST, etc.)
    pub method: String,
    
    /// Request URI (path and query)
    pub uri: String,
    
    /// HTTP headers
    pub headers: HashMap<String, String>,
    
    /// Request body, if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Vec<u8>>,
}

impl HttpTcpRequest {
    /// Create a new HTTP request.
    pub fn new<S: Into<String>>(method: S, uri: S) -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            method: method.into(),
            uri: uri.into(),
            headers: HashMap::new(),
            body: None,
        }
    }
    
    /// Add a header to the request.
    pub fn with_header<S: Into<String>>(mut self, key: S, value: S) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }
    
    /// Add multiple headers to the request.
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers.extend(headers);
        self
    }
    
    /// Set the request body.
    pub fn with_body<B: Into<Vec<u8>>>(mut self, body: B) -> Self {
        self.body = Some(body.into());
        self
    }
    
    /// Set the request ID.
    pub fn with_request_id<S: Into<String>>(mut self, request_id: S) -> Self {
        self.request_id = request_id.into();
        self
    }
    
    /// Get a reference to a specific header.
    pub fn header(&self, key: &str) -> Option<&String> {
        self.headers.get(key)
    }
    
    /// Parse the query parameters from the URI.
    pub fn query_params(&self) -> HashMap<String, String> {
        let mut params = HashMap::new();
        
        if let Some(query_str) = self.uri.split('?').nth(1) {
            for pair in query_str.split('&') {
                let mut parts = pair.split('=');
                if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                    params.insert(key.to_string(), value.to_string());
                }
            }
        }
        
        params
    }
    
    /// Get a specific query parameter.
    pub fn query_param(&self, key: &str) -> Option<String> {
        self.query_params().get(key).cloned()
    }
    
    /// Get the path part of the URI.
    pub fn path(&self) -> &str {
        self.uri.split('?').next().unwrap_or(&self.uri)
    }
}

/// HTTP response transported over TCP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpTcpResponse {
    /// Unique identifier matching the request
    pub request_id: String,
    
    /// HTTP status code
    pub status_code: u16,
    
    /// HTTP headers
    pub headers: HashMap<String, String>,
    
    /// Response body, if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Vec<u8>>,
}

impl HttpTcpResponse {
    /// Create a new HTTP response.
    pub fn new(request_id: &str, status_code: u16) -> Self {
        Self {
            request_id: request_id.to_string(),
            status_code,
            headers: HashMap::new(),
            body: None,
        }
    }
    
    /// Add a header to the response.
    pub fn with_header<S: Into<String>>(mut self, key: S, value: S) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }
    
    /// Add multiple headers to the response.
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers.extend(headers);
        self
    }
    
    /// Set the response body.
    pub fn with_body<B: Into<Vec<u8>>>(mut self, body: B) -> Self {
        self.body = Some(body.into());
        self
    }
    
    /// Get a reference to a specific header.
    pub fn header(&self, key: &str) -> Option<&String> {
        self.headers.get(key)
    }
    
    /// Get the response body as a string, if possible.
    pub fn body_as_string(&self) -> Option<Result<String, std::string::FromUtf8Error>> {
        self.body.as_ref().map(|b| String::from_utf8(b.clone()))
    }
    
    /// Get the response body as an object, if possible.
    pub fn body_as_json<T: for<'de> Deserialize<'de>>(&self) -> Option<Result<T, serde_json::Error>> {
        self.body.as_ref().map(|b| serde_json::from_slice(b))
    }
    
    /// Check if the response status code indicates success (2xx).
    pub fn is_success(&self) -> bool {
        self.status_code >= 200 && self.status_code < 300
    }
    
    /// Check if the response status code indicates a client error (4xx).
    pub fn is_client_error(&self) -> bool {
        self.status_code >= 400 && self.status_code < 500
    }
    
    /// Check if the response status code indicates a server error (5xx).
    pub fn is_server_error(&self) -> bool {
        self.status_code >= 500
    }
}

/// Standard response data structure for API responses.
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    /// Status of the response (success, error)
    pub status: String,
    
    /// Optional data payload
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    
    /// Optional message for additional context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Create a standard HTTP response with a JSON body.
pub fn json_response<T: Serialize>(
    request_id: &str,
    status_code: u16,
    data: Option<T>,
    message: Option<String>,
) -> HttpTcpResponse {
    let api_response = ApiResponse {
        status: if status_code < 400 { "success" } else { "error" }.to_string(),
        data,
        message,
    };
    
    let body = match serde_json::to_vec(&api_response) {
        Ok(body) => Some(body),
        Err(_) => None,
    };
    
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    
    HttpTcpResponse {
        request_id: request_id.to_string(),
        status_code,
        headers,
        body,
    }
}

/// Create a success response with data.
pub fn success<T: Serialize>(request_id: &str, data: T) -> HttpTcpResponse {
    json_response(request_id, 200, Some(data), None)
}

/// Create a created response with data.
pub fn created<T: Serialize>(request_id: &str, data: T) -> HttpTcpResponse {
    json_response(request_id, 201, Some(data), None)
}

/// Create an error response with a message.
pub fn error_response(request_id: &str, status_code: u16, message: &str) -> HttpTcpResponse {
    json_response::<()>(request_id, status_code, None, Some(message.to_string()))
}

/// Create a not found response.
pub fn not_found(request_id: &str, path: &str) -> HttpTcpResponse {
    error_response(request_id, 404, &format!("Not found: {}", path))
}

/// Create a bad request response.
pub fn bad_request(request_id: &str, message: &str) -> HttpTcpResponse {
    error_response(request_id, 400, message)
}

/// Create an internal server error response.
pub fn internal_error(request_id: &str, message: &str) -> HttpTcpResponse {
    error_response(request_id, 500, message)
}

/// Parse JSON request body.
pub fn parse_json_body<T: for<'de> Deserialize<'de>>(
    request: &HttpTcpRequest,
) -> Result<T, String> {
    match &request.body {
        Some(body) => serde_json::from_slice(body)
            .map_err(|e| format!("Invalid JSON body: {}", e)),
        None => Err("Request body is required".to_string()),
    }
}

/// Serve a module over HTTP-TCP.
///
/// This is a convenience function that registers the module, advertises its capabilities,
/// and starts the HTTP server.
///
/// # Arguments
/// * `router` - HTTP router for handling requests
/// * `module` - Pre-registered module
/// * `shutdown_signal` - Optional signal to gracefully shut down the server
///
/// # Returns
/// Result indicating success or failure
pub async fn serve<S: Send + Sync + Clone + 'static>(
    router: HttpTcpRouter<S>,
    module: RegisteredModule,
    state: S,
    shutdown_signal: Option<tokio::sync::broadcast::Receiver<()>>,
) -> Result<tokio::task::JoinHandle<()>, crate::error::Error> {
    // Start the HTTP server
    start_http_server(router, module, state, shutdown_signal).await
} 