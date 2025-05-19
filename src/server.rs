// Server module
use thiserror::Error;
#[cfg(feature = "ipc_channel")]
use crate::ipc::IpcManager;
use crate::ipc_types::IpcHttpResponse;
use axum::body::Body as AxumBody;
use axum::http::Request as AxumRequest;
use tower::util::ServiceExt;
use std::collections::HashMap;
use http_body::Body;
#[cfg(not(feature = "ipc_channel"))]
use std::net::SocketAddr;

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
            .map_err(|e| Error::Server(e))?;
    }
    Ok(())
}

/// Serve the given router, either over IPC (stdio) if the "ipc" feature is enabled,
/// or as an HTTP server on an ephemeral port otherwise.
#[cfg(feature = "ipc_channel")]
pub async fn serve_module(app: axum::Router) -> Result<(), Error> {
    serve_ipc(app).await
}
#[cfg(not(feature = "ipc_channel"))]
pub async fn serve_module(app: axum::Router) -> Result<(), Error> {
    // Fallback to binding an ephemeral HTTP socket
    let addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let server = axum::Server::bind(&addr)
        .serve(app.into_make_service());
    server.await.map_err(|e| Error::Server(e.to_string()))?;
    Ok(())
}
