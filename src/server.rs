// Server module
use thiserror::Error;
#[cfg(feature = "ipc_channel")]
use crate::ipc::IpcManager;
use crate::ipc_types::{IpcHttpResponse, IpcPortNegotiation, IpcPortNegotiationResponse};
use axum::body::Body as AxumBody;
use axum::http::Request as AxumRequest;
use tower::util::ServiceExt;
use std::collections::HashMap;
use std::net::SocketAddr;
use http_body::Body;
use tracing::{debug, info, warn};

/// Errors that may occur during server bootstrap and run.
#[derive(Debug, Error)]
pub enum Error {
    /// IO error during socket operations.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Error during announcement.
    #[error("Announcement error: {0}")]
    Announce(String),
    
    /// Invalid configuration or state.
    #[error("Configuration error: {0}")]
    Config(String),
    
    /// Server error.
    #[error("Server error: {0}")]
    Server(String),

    /// Other error.
    #[error("Internal error: {0}")]
    Internal(Box<dyn std::error::Error + Send + Sync>),
}

/// Serve the given router over IPC (stdio) by subscribing to HTTP requests and responding via IPC.
#[cfg(feature = "ipc_channel")]
async fn serve_ipc(app: axum::Router) -> Result<(), Error> {
    // Subscribe to HTTP-over-IPC requests
    let manager = IpcManager::new();
    let mut rx = manager.subscribe_http_requests();
    // Treat the Router itself as a Service and clone it for dispatch
    let svc = app.clone();
    while let Ok(ipc_req) = rx.recv().await {
        // Build an Axum request from the IPC data
        let mut builder = AxumRequest::builder()
            .method(ipc_req.method.as_str())
            .uri(&ipc_req.uri);
        for (k, v) in &ipc_req.headers {
            builder = builder.header(k, v);
        }
        let req = builder
            .body(AxumBody::from(ipc_req.body.unwrap_or_default()))
            .map_err(|e| Error::Server(e.to_string()))?;
        // Dispatch to the router
        let resp = svc
            .clone()
            .oneshot(req)
            .await
            .map_err(|e| Error::Server(e.to_string()))?;
        // Convert response into IpcHttpResponse
        let status = resp.status().as_u16();
        let mut headers = HashMap::new();
        for (k, v) in resp.headers() {
            headers.insert(k.to_string(), v.to_str().unwrap_or_default().to_string());
        }
        // Collect full body and extract bytes
        let full = resp.into_body()
            .collect()
            .await
            .map_err(|e| Error::Server(e.to_string()))?;
        let bytes = full.to_bytes();
        let ipc_resp = IpcHttpResponse {
            request_id: ipc_req.request_id.clone(),
            status_code: status,
            headers,
            body: Some(bytes.to_vec()),
        };
        manager.send_http_response(ipc_resp)
            .await
            .map_err(Error::Server)?;
    }
    Ok(())
}

/// Options for serving a module.
#[derive(Debug, Clone)]
pub struct ServeOptions {
    /// Whether to bind an HTTP server (if false, only IPC will be used)
    pub bind_http: bool,
    /// Specific port to request from orchestrator (if None, the orchestrator will assign one)
    pub specific_port: Option<u16>,
    /// Alternative listen address (defaults to 127.0.0.1)
    pub listen_addr: Option<String>,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            bind_http: true,
            specific_port: None,
            listen_addr: None,
        }
    }
}

/// Request a port from the orchestrator via IPC.
#[cfg(feature = "ipc_channel")]
async fn negotiate_port(specific_port: Option<u16>) -> Result<u16, Error> {
    let manager = IpcManager::new();
    
    // Create port negotiation request
    let request = IpcPortNegotiation {
        request_id: uuid::Uuid::new_v4().to_string(),
        specific_port,
    };
    
    // Send request to orchestrator
    info!("Requesting port from orchestrator: {:?}", specific_port);
    manager.send_port_negotiation(request)
        .await
        .map_err(|e| Error::Config(format!("Failed to send port request: {}", e)))?;
    
    // Wait for response
    let response = manager.wait_for_port_response()
        .await
        .map_err(|e| Error::Config(format!("Failed to receive port response: {}", e)))?;
    
    if !response.success {
        return Err(Error::Config(response.error_message.unwrap_or_else(|| 
            "Port negotiation failed with no error message".to_string())));
    }
    
    info!("Received port from orchestrator: {}", response.port);
    Ok(response.port)
}

/// Serve the given router with the specified options.
#[cfg(feature = "ipc_channel")]
pub async fn serve_with_options(app: axum::Router, options: ServeOptions) -> Result<(), Error> {
    // Always set up IPC serving
    let ipc_task = tokio::spawn(serve_ipc(app.clone()));
    
    // If HTTP binding is requested, negotiate a port and start HTTP server
    if options.bind_http {
        // Negotiate port with orchestrator
        let port = negotiate_port(options.specific_port).await?;
        
        // Bind HTTP server
        let listen_addr = options.listen_addr.unwrap_or_else(|| "127.0.0.1".to_string());
        let addr: SocketAddr = format!("{listen_addr}:{port}").parse()
            .map_err(|e| Error::Config(format!("Invalid address or port: {}", e)))?;
        
        info!("Starting HTTP server on {}", addr);
        let server = axum::Server::bind(&addr)
            .serve(app.into_make_service());
        
        // Run the server
        tokio::select! {
            result = server => {
                result.map_err(|e| Error::Server(e.to_string()))?;
            }
            result = ipc_task => {
                result.map_err(|e| Error::Internal(Box::new(e)))??;
            }
        }
    } else {
        // Only serve via IPC
        info!("Module serving via IPC only");
        ipc_task.await.map_err(|e| Error::Internal(Box::new(e)))??;
    }
    
    Ok(())
}

#[cfg(not(feature = "ipc_channel"))]
pub async fn serve_with_options(app: axum::Router, options: ServeOptions) -> Result<(), Error> {
    // Without IPC channel, we can only serve HTTP
    let listen_addr = options.listen_addr.unwrap_or_else(|| "127.0.0.1".to_string());
    let port = options.specific_port.unwrap_or(0); // Use ephemeral port if none specified
    let addr: SocketAddr = format!("{listen_addr}:{port}").parse()
        .map_err(|e| Error::Config(format!("Invalid address or port: {}", e)))?;
    
    info!("Starting HTTP server on {}", addr);
    let server = axum::Server::bind(&addr)
        .serve(app.into_make_service());
    
    server.await.map_err(|e| Error::Server(e.to_string()))?;
    Ok(())
}

/// Serve the given router, either over IPC (stdio) if the "ipc" feature is enabled,
/// or as an HTTP server on an ephemeral port otherwise.
#[cfg(feature = "ipc_channel")]
pub async fn serve_module(app: axum::Router) -> Result<(), Error> {
    serve_with_options(app, ServeOptions::default()).await
}

#[cfg(not(feature = "ipc_channel"))]
pub async fn serve_module(app: axum::Router) -> Result<(), Error> {
    // Use default options which will create an HTTP server on an ephemeral port
    serve_with_options(app, ServeOptions::default()).await
}
