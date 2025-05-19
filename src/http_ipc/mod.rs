//! HTTP over IPC utilities for PyWatt modules.
//!
//! This module provides standardized utilities for handling HTTP requests and responses
//! over the PyWatt IPC mechanism. It simplifies the process of routing requests,
//! formatting responses, and handling errors in a consistent way across modules.

pub mod result;

use crate::ipc_types::{IpcHttpRequest, IpcHttpResponse};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::Mutex;
use tracing::{debug, error, info};
use once_cell::sync::Lazy;

pub use self::result::*;

// Static channels for HTTP request/response handling
static HTTP_REQUEST_CHANNEL: Lazy<(broadcast::Sender<IpcHttpRequest>, Mutex<Option<broadcast::Receiver<IpcHttpRequest>>>)> = 
    Lazy::new(|| {
        let (tx, rx) = broadcast::channel(100);
        (tx, Mutex::new(Some(rx)))
    });

static HTTP_RESPONSE_CHANNEL: Lazy<(broadcast::Sender<IpcHttpResponse>, Mutex<Option<broadcast::Receiver<IpcHttpResponse>>>)> = 
    Lazy::new(|| {
        let (tx, rx) = broadcast::channel(100);
        (tx, Mutex::new(Some(rx)))
    });

/// Subscribe to HTTP requests
pub fn subscribe_http_requests() -> broadcast::Receiver<IpcHttpRequest> {
    HTTP_REQUEST_CHANNEL.0.subscribe()
}

/// Send an HTTP response
pub async fn send_http_response(response: IpcHttpResponse) -> std::result::Result<(), crate::Error> {
    if let Err(e) = HTTP_RESPONSE_CHANNEL.0.send(response) {
        error!("Failed to send HTTP response: {}", e);
        return Err(crate::Error::Config(crate::error::ConfigError::Invalid(
            format!("Failed to send HTTP response: {}", e)
        )));
    }
    Ok(())
}

/// Standard response data structure for API responses.
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    /// Status of the response (success, error)
    pub status: String,
    /// Optional data payload
    pub data: Option<T>,
    /// Optional message for additional context
    pub message: Option<String>,
}

/// Handler function type for route handlers
pub type HandlerFn<S> = Arc<
    dyn Fn(
            IpcHttpRequest,
            Arc<S>,
        ) -> Pin<Box<dyn Future<Output = HttpResult<IpcHttpResponse>> + Send>>
        + Send
        + Sync,
>;

/// A route entry in the router
#[derive(Clone)]
struct Route<S> {
    method: String,
    path: String,
    handler: HandlerFn<S>,
}

/// HTTP IPC Router for handling requests
#[derive(Clone)]
pub struct HttpIpcRouter<S: Send + Sync + Clone + 'static> {
    routes: Arc<Vec<Route<S>>>,
    not_found_handler: Option<HandlerFn<S>>,
}

impl<S: Send + Sync + Clone + 'static> Default for HttpIpcRouter<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Send + Sync + Clone + 'static> HttpIpcRouter<S> {
    /// Create a new router.
    pub fn new() -> Self {
        Self {
            routes: Arc::new(Vec::new()),
            not_found_handler: None,
        }
    }

    /// Add a route to the router
    pub fn route<F, Fut>(mut self, method: &str, path: &str, handler: F) -> Self
    where
        F: Fn(IpcHttpRequest, Arc<S>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HttpResult<IpcHttpResponse>> + Send + 'static,
    {
        let handler = Arc::new(move |req, state| {
            let fut = handler(req, state);
            Box::pin(fut) as Pin<Box<dyn Future<Output = HttpResult<IpcHttpResponse>> + Send>>
        });

        let mut routes = (*self.routes).clone();
        routes.push(Route {
            method: method.to_uppercase(),
            path: path.to_string(),
            handler,
        });
        self.routes = Arc::new(routes);

        self
    }

    /// Set a custom not-found handler
    pub fn not_found_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(IpcHttpRequest, Arc<S>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HttpResult<IpcHttpResponse>> + Send + 'static,
    {
        let handler = Arc::new(move |req, state| {
            let fut = handler(req, state);
            Box::pin(fut) as Pin<Box<dyn Future<Output = HttpResult<IpcHttpResponse>> + Send>>
        });

        self.not_found_handler = Some(handler);
        self
    }

    /// Match a request to a route handler
    pub async fn handle_request(&self, request: IpcHttpRequest, state: Arc<S>) -> IpcHttpResponse {
        info!(
            "SDK_HTTP_IPC_ROUTER: handle_request ENTERED. Request ID: {}, Method: {}, URI: {}",
            request.request_id, request.method, request.uri
        );
        let method = request.method.to_uppercase();
        let path = request
            .uri
            .split('?')
            .next()
            .unwrap_or(&request.uri)
            .to_string();

        debug!("Routing request: {} {}", method, path);

        // Find matching route - log if we're inspecting many routes for performance reasons
        let routes_count = self.routes.len();
        if routes_count > 10 {
            debug!("Searching through {} routes for match", routes_count);
        }
        
        for route in self.routes.as_ref() {
            if route.method == method && route.path == path {
                debug!("Found matching route: {} {}", route.method, route.path);

                // Call the handler
                let handler_result = (route.handler)(request.clone(), state.clone()).await;
                return match handler_result {
                    Ok(mut response) => {
                        response.request_id = request.request_id;
                        response
                    }
                    Err(err) => {
                        error!("Handler error for {} {}: {}", method, path, err);
                        result::error_to_response(err, &request.request_id)
                    }
                };
            }
        }

        // No route found, use not_found_handler if available
        if let Some(handler) = &self.not_found_handler {
            match handler(request.clone(), state).await {
                Ok(mut response) => {
                    response.request_id = request.request_id;
                    return response;
                }
                Err(err) => {
                    error!("Handler error: {}", err);
                    return result::error_to_response(err, &request.request_id);
                }
            }
        } else {
            // Default not found response
            let response = ApiResponse::<()> {
                status: "error".to_string(),
                data: None,
                message: Some(format!("Endpoint not found: {} {}", method, path)),
            };

            let body = match serde_json::to_vec(&response) {
                Ok(body) => Some(body),
                Err(_) => Some(format!("Endpoint not found: {} {}", method, path).into_bytes()),
            };

            let mut headers = HashMap::new();
            headers.insert("Content-Type".to_string(), "application/json".to_string());

            IpcHttpResponse {
                request_id: request.request_id,
                status_code: 404,
                headers,
                body,
            }
        }
    }
}

/// Start the HTTP IPC server with the given router and state
pub async fn start_http_ipc_server<S: Send + Sync + Clone + 'static>(
    router: HttpIpcRouter<S>,
    state: S,
    shutdown_signal: Option<broadcast::Receiver<()>>,
) -> std::result::Result<(), crate::Error> {
    let state = Arc::new(state);

    // Ensure we're subscribed to HTTP requests
    // This is critical - calling this function creates the subscription so
    // the main IPC loop will recognize "http_request" variant messages
    let mut req_rx = subscribe_http_requests();

    info!("SDK_HTTP_IPC_SERVER: Starting HTTP IPC server");

    if let Some(mut user_shutdown_rx) = shutdown_signal {
        info!("SDK_HTTP_IPC_SERVER: Using provided user shutdown signal.");
        loop {
            info!("SDK_HTTP_IPC_SERVER: Top of select loop (with user shutdown).");
            tokio::select! {
                biased;
                _ = user_shutdown_rx.recv() => {
                    info!("SDK_HTTP_IPC_SERVER: Received user shutdown signal, stopping HTTP IPC server");
                    break;
                }
                result = req_rx.recv() => {
                    info!("SDK_HTTP_IPC_SERVER: req_rx.recv() returned.");
                    match result {
                        Ok(ipc_req) => {
                            info!(
                                "SDK_HTTP_IPC_SERVER: Received IpcHttpRequest (ID: {}) from broadcast. Spawning handler task.",
                                ipc_req.request_id
                            );
                            let router_ref = router.clone();
                            let state_ref = state.clone();
                            let request_id = ipc_req.request_id.clone();

                            tokio::spawn(async move {
                                info!(
                                    "SDK_HTTP_IPC_SERVER_TASK: Handling request (ID: {}). Calling router.handle_request...",
                                    request_id
                                );
                                let response = router_ref.handle_request(ipc_req, state_ref).await;
                                info!(
                                    "SDK_HTTP_IPC_SERVER_TASK: router.handle_request finished for (ID: {}). Status: {}. Sending response...",
                                    request_id, response.status_code
                                );
                                if let Err(e) = send_http_response(response).await {
                                    error!("SDK_HTTP_IPC_SERVER_TASK: Failed to send HTTP response for request {}: {}", request_id, e);
                                }
                            });
                        }
                        Err(e) => {
                            match e {
                                broadcast::error::RecvError::Closed => {
                                    error!("SDK_HTTP_IPC_SERVER: HTTP request broadcast channel closed. Shutting down server loop.");
                                    break;
                                }
                                broadcast::error::RecvError::Lagged(missed_count) => {
                                    error!(
                                        "SDK_HTTP_IPC_SERVER: HTTP request broadcast receiver lagged. Missed {} messages.",
                                        missed_count
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    } else {
        info!(
            "SDK_HTTP_IPC_SERVER: No user shutdown signal provided. Running until request channel closes."
        );
        loop {
            info!("SDK_HTTP_IPC_SERVER: Top of select loop (no user shutdown).");
            match req_rx.recv().await {
                Ok(ipc_req) => {
                    info!(
                        "SDK_HTTP_IPC_SERVER: Received IpcHttpRequest (ID: {}) from broadcast (no user shutdown). Spawning task.",
                        ipc_req.request_id
                    );
                    let router_ref = router.clone();
                    let state_ref = state.clone();
                    let request_id = ipc_req.request_id.clone();

                    tokio::spawn(async move {
                        info!(
                            "SDK_HTTP_IPC_SERVER_TASK: Handling request (ID: {}). Calling router.handle_request...",
                            request_id
                        );
                        let response = router_ref.handle_request(ipc_req, state_ref).await;
                        info!(
                            "SDK_HTTP_IPC_SERVER_TASK: router.handle_request finished for (ID: {}). Status: {}. Sending response...",
                            request_id, response.status_code
                        );
                        if let Err(e) = send_http_response(response).await {
                            error!(
                                "SDK_HTTP_IPC_SERVER_TASK: Failed to send HTTP response for request {}: {}",
                                request_id, e
                            );
                        }
                    });
                }
                Err(e) => match e {
                    broadcast::error::RecvError::Closed => {
                        error!(
                            "SDK_HTTP_IPC_SERVER: HTTP request broadcast channel closed (no user shutdown). Shutting down server loop."
                        );
                        break;
                    }
                    broadcast::error::RecvError::Lagged(missed_count) => {
                        error!(
                            "SDK_HTTP_IPC_SERVER: HTTP request broadcast receiver lagged (no user shutdown). Missed {} messages.",
                            missed_count
                        );
                    }
                },
            }
        }
    }

    info!("SDK_HTTP_IPC_SERVER: HTTP IPC server stopped");
    Ok(())
}
