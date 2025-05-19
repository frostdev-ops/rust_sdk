//! HTTP-over-TCP client implementation.
//!
//! This module provides a client for making HTTP requests over TCP connections.
//! It allows modules to communicate with other modules via HTTP semantics using
//! the TCP-based message transport.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use tokio::time::timeout;
use tracing::{debug, error, warn};
use uuid::Uuid;

use crate::http_tcp::{HttpTcpRequest, HttpTcpResponse};
use crate::message::Message;
use crate::tcp_channel::TcpChannel;
use crate::tcp_types::{ConnectionConfig, ConnectionState};
use crate::tcp_channel::MessageChannel;

/// Default timeout for HTTP requests.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Errors specific to the HTTP-over-TCP client.
#[derive(Debug, Error)]
pub enum HttpClientError {
    /// Connection error.
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    /// Request timeout.
    #[error("Request timed out after {0:?}")]
    Timeout(Duration),
    
    /// Response error.
    #[error("Response error ({status}): {message}")]
    ResponseError {
        /// HTTP status code
        status: u16,
        /// Error message
        message: String,
    },
    
    /// Serialization error.
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    
    /// I/O error.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
    
    /// SDK error.
    #[error("SDK error: {0}")]
    SdkError(String),
    
    /// Other error.
    #[error("Error: {0}")]
    Other(String),
}

/// Result type for HTTP-over-TCP client operations.
pub type HttpClientResult<T> = Result<T, HttpClientError>;

/// Builder for HTTP-over-TCP requests.
pub struct RequestBuilder {
    /// The HTTP method.
    method: String,
    
    /// The request URI.
    uri: String,
    
    /// The TCP channel.
    channel: Arc<TcpChannel>,
    
    /// Headers for the request.
    headers: HashMap<String, String>,
    
    /// Query parameters for the request.
    query_params: HashMap<String, String>,
    
    /// Request body.
    body: Option<Vec<u8>>,
    
    /// Timeout for the request.
    timeout: Duration,
    
    /// Number of retry attempts for the request.
    retries: u32,
}

impl RequestBuilder {
    /// Create a new request builder.
    fn new(method: &str, uri: String, channel: Arc<TcpChannel>) -> Self {
        Self {
            method: method.to_string(),
            uri,
            channel,
            headers: HashMap::new(),
            query_params: HashMap::new(),
            body: None,
            timeout: DEFAULT_TIMEOUT,
            retries: 1,
        }
    }
    
    /// Add a header to the request.
    pub fn header<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }
    
    /// Add multiple headers to the request.
    pub fn headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers.extend(headers);
        self
    }
    
    /// Add a query parameter to the request.
    pub fn query<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.query_params.insert(key.into(), value.into());
        self
    }
    
    /// Add multiple query parameters to the request.
    pub fn queries(mut self, params: HashMap<String, String>) -> Self {
        self.query_params.extend(params);
        self
    }
    
    /// Set the request body.
    pub fn body<B: Into<Vec<u8>>>(mut self, body: B) -> Self {
        self.body = Some(body.into());
        self
    }
    
    /// Set a JSON body for the request.
    pub fn json<T: Serialize>(mut self, data: &T) -> HttpClientResult<Self> {
        let json_body = serde_json::to_vec(data)?;
        self.headers.insert("Content-Type".to_string(), "application/json".to_string());
        self.body = Some(json_body);
        Ok(self)
    }
    
    /// Set the timeout for the request.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    
    /// Set the number of retry attempts for the request.
    pub fn retries(mut self, retries: u32) -> Self {
        self.retries = retries;
        self
    }
    
    /// Build the final URI with query parameters.
    fn build_uri(&self) -> String {
        let mut uri = self.uri.clone();
        
        if !self.query_params.is_empty() {
            if uri.contains('?') {
                if !uri.ends_with('?') {
                    uri.push('&');
                }
            } else {
                uri.push('?');
            }
            
            let query_string = self.query_params.iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("&");
            
            uri.push_str(&query_string);
        }
        
        uri
    }
    
    /// Build the HTTP-over-TCP request.
    fn build_request(&self) -> HttpTcpRequest {
        let uri = self.build_uri();
        
        let mut request = HttpTcpRequest::new(self.method.clone(), uri)
            .with_headers(self.headers.clone())
            .with_request_id(Uuid::new_v4().to_string());
        
        if let Some(body) = &self.body {
            request = request.with_body(body.clone());
        }
        
        request
    }
    
    /// Send the request and return the response.
    pub async fn send(self) -> HttpClientResult<HttpTcpResponse> {
        // Ensure the channel is connected
        if MessageChannel::state(&*self.channel).await != ConnectionState::Connected {
            match MessageChannel::connect(&*self.channel).await {
                Ok(_) => debug!("Connected to TCP server"),
                Err(e) => return Err(HttpClientError::ConnectionError(format!("Failed to connect: {}", e))),
            }
        }
        
        // Build request
        let request = self.build_request();
        let request_id = request.request_id.clone();
        
        debug!("Sending HTTP-over-TCP request: {} {}", request.method, request.uri);
        
        // Create message
        let message = Message::new(request);
        let encoded = message.encode()
            .map_err(|e| HttpClientError::SdkError(e.to_string()))?;
        
        // Send request
        MessageChannel::send(&*self.channel, encoded).await
            .map_err(|e| HttpClientError::SdkError(e.to_string()))?;
        
        // Wait for response with timeout
        let receive_future = MessageChannel::receive(&*self.channel);
        let response_encoded = match timeout(self.timeout, receive_future).await {
            Ok(Ok(response)) => response,
            Ok(Err(e)) => return Err(HttpClientError::SdkError(e.to_string())),
            Err(_) => return Err(HttpClientError::Timeout(self.timeout)),
        };
        
        // Decode response - use a different approach that doesn't require bincode::Decode
        let response_json = match response_encoded.format() {
            crate::message::EncodingFormat::Json => {
                // If already JSON, just convert to string
                std::str::from_utf8(response_encoded.data())
                    .map_err(|e| HttpClientError::SdkError(format!("Invalid UTF-8: {}", e)))?
                    .to_string()
            },
            _ => {
                // Convert to JSON first if not already
                let json_encoded = response_encoded.to_format(crate::message::EncodingFormat::Json)
                    .map_err(|e| HttpClientError::SdkError(e.to_string()))?;
                
                std::str::from_utf8(json_encoded.data())
                    .map_err(|e| HttpClientError::SdkError(format!("Invalid UTF-8: {}", e)))?
                    .to_string()
            }
        };
        
        let response: Message<HttpTcpResponse> = serde_json::from_str(&response_json)
            .map_err(|e| HttpClientError::SdkError(format!("Failed to deserialize response: {}", e)))?;
        
        let response_data = response.content().clone();
        
        // Verify the response matches our request ID
        if response_data.request_id != request_id {
            warn!(
                "Response ID mismatch: expected {}, got {}",
                request_id, response_data.request_id
            );
        }
        
        Ok(response_data)
    }
    
    /// Send the request and return the response text.
    pub async fn send_text(self) -> HttpClientResult<String> {
        let response = self.send().await?;
        
        // Check if status code indicates success
        if !response.is_success() {
            return Err(HttpClientError::ResponseError {
                status: response.status_code,
                message: response.body_as_string()
                    .unwrap_or_else(|| Ok("No response body".to_string()))
                    .unwrap_or_else(|_| "Failed to decode response body".to_string()),
            });
        }
        
        if let Some(body) = &response.body {
            String::from_utf8(body.clone())
                .map_err(|_| HttpClientError::Other("Failed to decode response body as UTF-8".to_string()))
        } else {
            Ok(String::new())
        }
    }
    
    /// Send the request and return the response as a JSON object.
    pub async fn send_json<T: DeserializeOwned>(self) -> HttpClientResult<T> {
        let response = self.send().await?;
        
        // Check if status code indicates success
        if !response.is_success() {
            return Err(HttpClientError::ResponseError {
                status: response.status_code,
                message: response.body_as_string()
                    .unwrap_or_else(|| Ok("No response body".to_string()))
                    .unwrap_or_else(|_| "Failed to decode response body".to_string()),
            });
        }
        
        if let Some(body) = &response.body {
            serde_json::from_slice(body)
                .map_err(HttpClientError::SerializationError)
        } else {
            Err(HttpClientError::Other("No response body".to_string()))
        }
    }
}

/// Callback type for asynchronous responses.
pub type ResponseCallback = Box<dyn FnOnce(HttpClientResult<HttpTcpResponse>) + Send + 'static>;

/// Client for making HTTP-over-TCP requests.
pub struct HttpTcpClient {
    /// Configuration for the connection.
    config: ConnectionConfig,
    
    /// TCP channel for communication.
    channel: Arc<TcpChannel>,
    
    /// Default request timeout.
    default_timeout: Duration,
    
    /// Default number of retry attempts.
    default_retries: u32,
    
    /// Base URL for requests.
    base_url: Option<String>,
}

impl HttpTcpClient {
    /// Create a new HTTP-over-TCP client with the given configuration.
    pub async fn new(config: ConnectionConfig) -> HttpClientResult<Self> {
        // Create and connect the channel
        let channel = TcpChannel::connect(config.clone()).await
            .map_err(|e| HttpClientError::ConnectionError(e.to_string()))?;
        
        Ok(Self {
            config,
            channel: Arc::new(channel),
            default_timeout: DEFAULT_TIMEOUT,
            default_retries: 1,
            base_url: None,
        })
    }
    
    /// Create a new HTTP-over-TCP client with the given channel.
    pub fn with_channel(channel: Arc<TcpChannel>, config: ConnectionConfig) -> Self {
        Self {
            config,
            channel,
            default_timeout: DEFAULT_TIMEOUT,
            default_retries: 1,
            base_url: None,
        }
    }
    
    /// Set the base URL for requests.
    pub fn with_base_url<S: Into<String>>(mut self, base_url: S) -> Self {
        self.base_url = Some(base_url.into());
        self
    }
    
    /// Set the default timeout for requests.
    pub fn with_default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }
    
    /// Set the default number of retry attempts.
    pub fn with_default_retries(mut self, retries: u32) -> Self {
        self.default_retries = retries;
        self
    }
    
    /// Resolve a URL with the base URL, if set.
    fn resolve_url(&self, url: &str) -> String {
        if url.starts_with("http://") || url.starts_with("https://") {
            url.to_string()
        } else if let Some(base) = &self.base_url {
            let mut resolved = base.clone();
            
            if !resolved.ends_with('/') && !url.starts_with('/') {
                resolved.push('/');
            } else if resolved.ends_with('/') && url.starts_with('/') {
                resolved.pop();
            }
            
            resolved.push_str(url);
            resolved
        } else {
            url.to_string()
        }
    }
    
    /// Create a GET request.
    pub fn get<S: Into<String>>(&self, url: S) -> RequestBuilder {
        let url_str = self.resolve_url(&url.into());
        RequestBuilder::new("GET", url_str, self.channel.clone())
            .timeout(self.default_timeout)
            .retries(self.default_retries)
    }
    
    /// Create a POST request.
    pub fn post<S: Into<String>>(&self, url: S) -> RequestBuilder {
        let url_str = self.resolve_url(&url.into());
        RequestBuilder::new("POST", url_str, self.channel.clone())
            .timeout(self.default_timeout)
            .retries(self.default_retries)
    }
    
    /// Create a PUT request.
    pub fn put<S: Into<String>>(&self, url: S) -> RequestBuilder {
        let url_str = self.resolve_url(&url.into());
        RequestBuilder::new("PUT", url_str, self.channel.clone())
            .timeout(self.default_timeout)
            .retries(self.default_retries)
    }
    
    /// Create a DELETE request.
    pub fn delete<S: Into<String>>(&self, url: S) -> RequestBuilder {
        let url_str = self.resolve_url(&url.into());
        RequestBuilder::new("DELETE", url_str, self.channel.clone())
            .timeout(self.default_timeout)
            .retries(self.default_retries)
    }
    
    /// Create a PATCH request.
    pub fn patch<S: Into<String>>(&self, url: S) -> RequestBuilder {
        let url_str = self.resolve_url(&url.into());
        RequestBuilder::new("PATCH", url_str, self.channel.clone())
            .timeout(self.default_timeout)
            .retries(self.default_retries)
    }
    
    /// Create a HEAD request.
    pub fn head<S: Into<String>>(&self, url: S) -> RequestBuilder {
        let url_str = self.resolve_url(&url.into());
        RequestBuilder::new("HEAD", url_str, self.channel.clone())
            .timeout(self.default_timeout)
            .retries(self.default_retries)
    }
    
    /// Create a OPTIONS request.
    pub fn options<S: Into<String>>(&self, url: S) -> RequestBuilder {
        let url_str = self.resolve_url(&url.into());
        RequestBuilder::new("OPTIONS", url_str, self.channel.clone())
            .timeout(self.default_timeout)
            .retries(self.default_retries)
    }
    
    /// Send a GET request and return JSON.
    pub async fn get_json<T: DeserializeOwned, S: Into<String>>(&self, url: S) -> HttpClientResult<T> {
        self.get(url).send_json().await
    }
    
    /// Send a GET request and return text.
    pub async fn get_text<S: Into<String>>(&self, url: S) -> HttpClientResult<String> {
        self.get(url).send_text().await
    }
    
    /// Send a POST request with JSON data.
    pub async fn post_json<T: Serialize, R: DeserializeOwned, S: Into<String>>(
        &self,
        url: S,
        data: &T,
    ) -> HttpClientResult<R> {
        self.post(url).json(data)?.send_json().await
    }
    
    /// Get the current state of the connection.
    pub async fn connection_state(&self) -> ConnectionState {
        MessageChannel::state(&*self.channel).await
    }
    
    /// Reconnect the client.
    pub async fn reconnect(&self) -> HttpClientResult<()> {
        MessageChannel::connect(&*self.channel).await
            .map_err(|e| HttpClientError::ConnectionError(e.to_string()))
    }
}

impl fmt::Debug for HttpTcpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpTcpClient")
            .field("config", &self.config)
            .field("default_timeout", &self.default_timeout)
            .field("default_retries", &self.default_retries)
            .field("base_url", &self.base_url)
            .finish()
    }
}

/// A client pool for managing multiple HTTP clients.
pub struct HttpClientPool {
    /// Clients in the pool.
    clients: HashMap<String, Arc<HttpTcpClient>>,
}

impl HttpClientPool {
    /// Create a new client pool.
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }
    
    /// Add a client to the pool.
    pub fn add_client<S: Into<String>>(&mut self, name: S, client: HttpTcpClient) {
        self.clients.insert(name.into(), Arc::new(client));
    }
    
    /// Get a client from the pool.
    pub fn get_client<S: AsRef<str>>(&self, name: S) -> Option<Arc<HttpTcpClient>> {
        self.clients.get(name.as_ref()).cloned()
    }
    
    /// Remove a client from the pool.
    pub fn remove_client<S: AsRef<str>>(&mut self, name: S) -> Option<Arc<HttpTcpClient>> {
        self.clients.remove(name.as_ref())
    }
    
    /// Clear all clients from the pool.
    pub fn clear(&mut self) {
        self.clients.clear();
    }
    
    /// Get the number of clients in the pool.
    pub fn len(&self) -> usize {
        self.clients.len()
    }
    
    /// Check if the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }
    
    /// Get the names of all clients in the pool.
    pub fn client_names(&self) -> Vec<String> {
        self.clients.keys().cloned().collect()
    }
}

impl Default for HttpClientPool {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for HttpClientPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpClientPool")
            .field("clients", &self.client_names())
            .finish()
    }
} 