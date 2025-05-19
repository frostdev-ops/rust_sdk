# PyWatt SDK

[![Latest Version](https://img.shields.io/crates/v/pywatt_sdk.svg)](https://crates.io/crates/pywatt_sdk)
[![Docs](https://docs.rs/pywatt_sdk/badge.svg)](https://docs.rs/pywatt_sdk)

Standardized SDK for building PyWatt modules in Rust.

## Overview

This crate provides the core building blocks for creating PyWatt modules that integrate seamlessly with the PyWatt orchestrator. It handles:

-   **IPC Handshake**: Standardized startup communication (`read_init`, `send_announce`).
-   **Logging**: Consistent logging to `stderr` with secret redaction (`init_module`).
-   **Secret Management**: Secure retrieval and rotation handling via the integrated `secret_client` module (`get_secret`, `subscribe_secret_rotations`).
-   **Typed Secrets**: Type-safe secret retrieval with automatic parsing (`get_typed_secret`, `client.get_typed<T>`, etc.)
-   **Runtime IPC**: Background task for processing orchestrator messages (`process_ipc_messages`).
-   **Core Types**: Re-exports essential types from the integrated `ipc_types` module (`OrchestratorInit`, `ModuleAnnounce`, etc.).
-   **(Optional) Macros**: Proc macros for simplifying module definition (requires `proc_macros` feature).
-   **(Optional) JWT Auth**: Middleware for Axum route protection (requires `jwt_auth` feature).

## Installation

Add this to your module's `Cargo.toml`:

```toml
[dependencies]
# Use the version from crates.io
pywatt_sdk = "0.2.5"
# Or use a path dependency during development
# pywatt_sdk = { path = "../pywatt_sdk" } 

# Other dependencies like axum, tokio, etc.
tokio = { version = "1", features = ["full"] }
axum = "0.7"
tracing = "0.1"
```

To enable the `#[pywatt_sdk::module]` attribute macro, enable the `proc_macros` feature on your PyWatt SDK dependency:

```toml
[dependencies]
pywatt_sdk = { version = "0.2.9", features = ["proc_macros"] }
```

## Quickstart Example

Here's a minimal module using the SDK with Axum:

```rust
// Use the prelude for common types and functions
use pywatt_sdk::prelude::*;
use axum::{routing::get, Router, extract::State};
use tokio::net::{TcpListener, UnixListener};
use std::sync::Arc;

// Define your module-specific state if needed
#[derive(Clone)]
struct MyModuleState {
    // Example: Database connection pool or config
    message: String,
    port: u16,
}

// Use the SDK's AppState wrapper for shared state
type SharedState = AppState<MyModuleState>;

async fn health_handler() -> &'static str {
    "OK"
}

async fn custom_handler(State(state): State<SharedState>) -> String {
    format!("Module {} says: {} on port {}", 
        state.module_id(), 
        state.user_state.message,
        state.user_state.port)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 1. Initialize logging (must be first!)
    init_module();

    // 2. Perform handshake with orchestrator
    let init: OrchestratorInit = read_init().await?;
    tracing::info!(?init, "Received orchestrator initialization");

    // 3. Initialize secret client (built-in)
    let secret_client = get_module_secret_client(&init.orchestrator_api, &init.module_id).await?;

    // 4. Fetch initial secrets with type-safety
    let message = secret_client.get_string("INITIAL_MESSAGE").await
        .unwrap_or_else(|_| {
            tracing::warn!("INITIAL_MESSAGE secret not found, using default");
            typed_secret::Secret::new("Default message".to_string())
        });
        
    // Get a typed numeric value
    let port = secret_client.get_typed::<u16>("PORT").await
        .unwrap_or_else(|_| {
            tracing::warn!("PORT secret not found, using default");
            typed_secret::Secret::new(8080u16)
        });

    // 5. Create shared state
    let my_state = MyModuleState { 
        message: message.expose_secret().clone(),
        port: *port.expose_secret() 
    };
    let app_state = AppState::new(
        init.module_id.clone(),
        init.orchestrator_api.clone(),
        secret_client.clone(),
        my_state
    );

    // 6. Subscribe to secret rotations (example)
    let app_state_clone = app_state.clone(); // Clone AppState for the task
    let keys = vec!["INITIAL_MESSAGE".to_string(), "PORT".to_string()];
    tokio::spawn(subscribe_secret_rotations(secret_client.clone(), keys, move |key, new_val| {
        // Note: To update shared state *mutably* here, MyModuleState would
        // typically contain Arcs/Mutexes, or you'd use a channel to communicate
        // back to the main thread/state manager.
        tracing::info!(%key, "Secret rotated, state needs update (implementation detail)");
    }));

    // 7. Set up Axum router
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/custom", get(custom_handler))
        .with_state(app_state.clone());

    // 8. Bind listener (TCP or UDS)
    // Use the `.into_make_service()` for Axum 0.7+
    let serve_future = match &init.listen {
        ListenAddress::Tcp(addr) => {
            tracing::info!(%addr, "Binding TCP listener");
            let listener = TcpListener::bind(addr).await?;
            axum::serve(listener, app.into_make_service())
        }
        ListenAddress::Unix(path) => {
            tracing::info!(path = %path.display(), "Binding Unix listener");
            // Ensure the socket file doesn't exist or clean it up
            if path.exists() {
                tokio::fs::remove_file(path).await?;
            }
            let listener = UnixListener::bind(path)?;
            axum::serve(listener, app.into_make_service())
        }
    };

    // 9. Announce endpoints to orchestrator
    let announce = ModuleAnnounce {
        listen: init.listen_address().to_string_lossy(), // Use helper from ext trait
        endpoints: vec![
            AnnouncedEndpoint { path: "/health".into(), methods: vec!["GET".into()], auth: None },
            AnnouncedEndpoint { path: "/custom".into(), methods: vec!["GET".into()], auth: None },
        ],
    };
    send_announce(&announce)?;
    tracing::info!(?announce, "Sent announcement to orchestrator");

    // 10. Spawn runtime IPC message processor
    let ipc_handle = tokio::spawn(process_ipc_messages());

    // 11. Run the server
    tracing::info!("Module server starting");
    tokio::select! {
        result = serve_future => {
            if let Err(e) = result {
                tracing::error!(error = %e, "Server exited with error");
            }
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received Ctrl+C");
        }
        ipc_res = ipc_handle => {
            match ipc_res {
                Ok(_) => tracing::info!("IPC handler finished cleanly"),
                Err(e) => tracing::error!(error = %e, "IPC handler finished with error"),
            }
        }
    }

    tracing::info!("Module shutting down gracefully");
    Ok(())
}

## Python SDK

Install the Python SDK:

```
pip install pywatt_sdk
```

Or use the development requirements:

```
pip install -r requirements.txt
```

### Quickstart Example

```python
from pywatt_sdk import (
    init_module,
    read_init,
    send_announce,
    process_ipc_messages,
    get_module_secret_client,
    get_secret,
    subscribe_secret_rotations,
    AppState,
    EndpointAnnounce,
    serve_module,
    ModuleBuilder,
)
from starlette.applications import Starlette
from starlette.routing import Route

# Define a simple ASGI app
async def health(request):
    return JSONResponse({"status": "OK"})

# Entry point
if __name__ == "__main__":
    def state_builder(init, secrets):
        # Build user state here
        return {"secrets": secrets}

    def app_builder(app_state):
        routes = [Route("/health", health)]
        app = Starlette(routes=routes)
        return app

    # Define endpoints to announce
    endpoints = [EndpointAnnounce(path="/health", methods=["GET"])]

    # Serve module
    serve_module(
        secret_keys=["MY_SECRET_KEY"],
        endpoints=endpoints,
        state_builder=state_builder,
        app_builder=app_builder,
    )
```

## Type-Safe Secrets

The SDK provides type-safe secret retrieval to make working with strongly-typed configuration values easier:

```rust
use pywatt_sdk::{secret_client::SecretClient, typed_secret::{Secret, get_typed_secret}};
use std::sync::Arc;

// Fetch and parse secrets of different types
let client = Arc::new(SecretClient::new("https://api.example.com", "my-module").await?);

// Using the typed_secret module directly
let api_key: Secret<String> = get_typed_secret(&client, "API_KEY").await?;
let timeout: Secret<u64> = get_typed_secret(&client, "TIMEOUT_SECONDS").await?;
let debug_mode: Secret<bool> = get_typed_secret(&client, "DEBUG_MODE").await?;

// Or using convenience methods on SecretClient
let pool_size = client.get_typed::<usize>("DB_POOL_SIZE").await?;
let api_url = client.get_string("API_URL").await?;
let feature_enabled = client.get_bool("FEATURE_ENABLED").await?;

// Values are protected from accidental exposure in logs
println!("API key: {:?}", api_key); // Prints: "API key: Secret([REDACTED])"

// Use values only when needed
if *debug_mode.expose_secret() {
    println!("Using API at {} with timeout {}s", 
        api_url.expose_secret(),
        timeout.expose_secret());
}
```

## Database Model Manager

The PyWatt SDK includes a powerful Database Model Manager (feature-gated by `database`) that simplifies schema definition and management for SQL databases. It allows you to:

-   Define database tables (models, columns, types, constraints, indexes) in a database-agnostic way using Rust structs.
-   Generate Data Definition Language (DDL) statements (e.g., `CREATE TABLE`, `CREATE INDEX`) for supported database backends:
    -   SQLite
    -   MySQL / MariaDB
    -   PostgreSQL
-   Apply these models to a live database connection, creating or synchronizing tables.
-   Use a command-line tool (`database-tool`) for generating DDL, applying schemas, validating models, and generating Rust structs from YAML model definitions.

This toolkit promotes code reusability, enhances developer productivity by streamlining schema operations, and leverages Rust's type safety.

**Example Usage (Conceptual):**

```rust
use pywatt_sdk::database::{DatabaseConfig, create_database_connection, DatabaseType};
use pywatt_sdk::model_manager::{
    ModelManager, // Extension trait for DatabaseConnection
    definitions::{ModelDescriptor, ColumnDescriptor, DataType, IntegerSize}
};

async fn manage_schema(db_config: DatabaseConfig) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = create_database_connection(&db_config).await?;

    let user_model = ModelDescriptor {
        name: "users".to_string(),
        columns: vec![
            ColumnDescriptor {
                name: "id".to_string(),
                data_type: DataType::Integer(IntegerSize::I64),
                is_primary_key: true,
                auto_increment: true,
                ..Default::default()
            },
            ColumnDescriptor {
                name: "email".to_string(),
                data_type: DataType::Varchar(255),
                is_unique: true,
                is_nullable: false,
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    // Apply the model (creates table if not exists, attempts to add missing columns/indexes)
    conn.sync_schema(&[user_model]).await?;
    
    Ok(())
}
```

For detailed information on defining models, generating SQL, CLI usage, and more, please refer to the [Model Manager Documentation](./docs/model_manager.md).

## Core Functions & Types

(See the `