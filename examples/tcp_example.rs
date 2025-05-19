//! Example of TCP-based module communication with PyWatt SDK
//!
//! This example demonstrates:
//! 1. Creating and connecting a TCP channel
//! 2. Registering a module with an orchestrator
//! 3. Setting up an HTTP-over-TCP server
//! 4. Making HTTP-over-TCP requests to other modules

use pywatt_sdk::prelude::*;
use pywatt_sdk::tcp_types::{ConnectionConfig, ReconnectPolicy};
use pywatt_sdk::registration::ModuleInfo;
use pywatt_sdk::http_tcp::{HttpTcpRouter, HttpTcpClient, HttpTcpRequest, HttpTcpResponse};

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{info, error};

/// Simple state for our application
#[derive(Clone)]
struct AppState {
    counter: Arc<tokio::sync::Mutex<i32>>,
}

/// Response data for our counter
#[derive(Debug, Serialize, Deserialize)]
struct CounterResponse {
    value: i32,
}

/// Main function
#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    // Create application state
    let state = AppState {
        counter: Arc::new(tokio::sync::Mutex::new(0)),
    };
    
    // Configuration for TCP connection to orchestrator
    let config = ConnectionConfig::new("localhost", 9000)
        .with_timeout(Duration::from_secs(5))
        .with_reconnect_policy(ReconnectPolicy::ExponentialBackoff {
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            multiplier: 2.0,
        });
    
    // Register with the orchestrator
    info!("Registering with orchestrator...");
    let module_info = ModuleInfo::new(
        "example-module",
        "1.0.0",
        "Example TCP-based module",
    );
    
    let module = register_module(config.clone(), module_info).await?;
    info!("Successfully registered as module: {}", module.id);
    
    // Create HTTP-over-TCP router
    let router = create_router();
    
    // Start HTTP server
    info!("Starting HTTP-over-TCP server...");
    let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
    let server_handle = serve_http_tcp(router, module.clone(), state, Some(shutdown_rx)).await?;
    
    // Send a request to ourselves as an example
    info!("Sending HTTP request to self...");
    let client = HttpTcpClient::new(ConnectionConfig::new(
        module.orchestrator_host.clone(),
        module.orchestrator_port,
    )).await?;
    
    // Send a GET request to the increment endpoint
    let response = client.get("/counter/increment").send().await?;
    info!("Received response: {:?}", response);
    
    // Check if the response was successful
    if response.is_success() {
        if let Some(body) = response.body {
            // Parse the body as JSON
            let counter_response: CounterResponse = serde_json::from_slice(&body)?;
            info!("Counter value: {}", counter_response.value);
        }
    } else {
        error!("Request failed with status code: {}", response.status_code);
    }
    
    // Wait for a moment before shutting down
    tokio::time::sleep(Duration::from_secs(5)).await;
    
    // Shut down the HTTP server
    info!("Shutting down...");
    let _ = shutdown_tx.send(());
    
    // Wait for the server to shut down
    let _ = server_handle.await;
    
    // Unregister from the orchestrator
    info!("Unregistering from orchestrator...");
    unregister_module(&module).await?;
    
    info!("Done!");
    Ok(())
}

/// Create a router with our endpoints
fn create_router() -> HttpTcpRouter<AppState> {
    HttpTcpRouter::new()
        .route("GET", "/counter", get_counter)
        .route("GET", "/counter/increment", increment_counter)
        .route("POST", "/counter/reset", reset_counter)
}

/// Handler for GET /counter
async fn get_counter(_request: HttpTcpRequest, state: Arc<AppState>) -> std::result::Result<HttpTcpResponse, pywatt_sdk::http_tcp::router::HttpTcpError> {
    let counter = *state.counter.lock().await;
    let response = CounterResponse { value: counter };
    Ok(pywatt_sdk::http_tcp::success(&_request.request_id, response))
}

/// Handler for GET /counter/increment
async fn increment_counter(_request: HttpTcpRequest, state: Arc<AppState>) -> std::result::Result<HttpTcpResponse, pywatt_sdk::http_tcp::router::HttpTcpError> {
    let mut counter = state.counter.lock().await;
    *counter += 1;
    let response = CounterResponse { value: *counter };
    Ok(pywatt_sdk::http_tcp::success(&_request.request_id, response))
}

/// Handler for POST /counter/reset
async fn reset_counter(_request: HttpTcpRequest, state: Arc<AppState>) -> std::result::Result<HttpTcpResponse, pywatt_sdk::http_tcp::router::HttpTcpError> {
    let mut counter = state.counter.lock().await;
    *counter = 0;
    let response = CounterResponse { value: *counter };
    Ok(pywatt_sdk::http_tcp::success(&_request.request_id, response))
} 