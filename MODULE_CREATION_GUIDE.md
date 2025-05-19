# PyWatt Rust SDK Module Creation Guide

This guide walks you through building a PyWatt module with the Rust SDK—either with the high-level `#[pywatt_sdk::module]` procedural macro (recommended) or the manual `serve_module` API for full control.

---

## 1. Getting Started

### Cargo.toml

```toml
[package]
name = "my-pywatt-module"
version = "0.1.0"
edition = "2021"

[dependencies]
# PyWatt SDK with proc-macro support
pywatt_sdk = { path = "../pywatt_sdk", features = ["proc_macros", "router_ext", "metrics", "jwt_auth"] }
# Async runtime
tokio = { version = "1", features = ["full"] }
# Web framework
axum = "0.7"
# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
# Secrets
secrecy = { version = "1", features = ["serde"] }

[build-dependencies]
pywatt_sdk = { path = "../pywatt_sdk" }
```

### build.rs  
(needed for health endpoint build info)

```rust
// build.rs
fn main() {
    // Emits GIT_HASH, BUILD_TIME_UTC, RUSTC_VERSION
    pywatt_sdk::build::emit_build_info();
}
```

---

## 2. Macro-Based Module (Recommended)

Use `#[pywatt_sdk::module(...)]` to eliminate boilerplate. You define:
1. A state-builder function (`state = ...`) that produces your custom `T` from orchestrator init + fetched secrets.
2. A router-builder function (the annotated `async fn`) that takes `AppState<T>` and returns an `axum::Router`.

### Attribute Syntax

```rust
#[pywatt_sdk::module(
  // Build your custom state from init+secrets
  state      = build_state,              // fn(&OrchestratorInit, Vec<SecretString>) -> MyState
  // Secrets fetched at startup; also registers for rotation if `rotate = true`
  secrets    = ["DB_URL", "API_KEY"],
  rotate     = true,
  // HTTP endpoints to announce (required, no discovery)
  endpoints  = [
    EndpointAnnounce { path = "/foo", methods = ["GET"],  auth = None      },
    EndpointAnnounce { path = "/bar", methods = ["POST"], auth = Some("jwt") }
  ],
  // Health & metrics
  health     = "/healthz",             // default: "/health"
  metrics    = true,                     // requires `metrics` feature
  // Version applied only to announcement paths
  version    = "v1",
)]
async fn build_router(state: AppState<MyState>) -> Router {
    // your route definitions
}
```

### What the Macro Generates

Under the hood, `main()` will:
1. `init_module()` sets up JSON-structured logs.  
2. `read_init().await?` performs handshake (module_id, API URL, listen address).  
3. `get_module_secret_client()` + `get_secrets()` prefetch listed secrets.  
4. If `rotate=true`, subscribes to secret rotations.  
5. Calls your `build_state(&init, secrets)` → `MyState`.  
6. Wraps into `AppState<MyState>`.  
7. Calls `register_module()` + `start_heartbeat_loop()`.  
8. Invokes your `build_router(AppState)` → `Router`.  
9. Layers on `Extension(app_state)`, health & metrics endpoints.  
10. Sends `ModuleAnnounce` with your `endpoints`, applying `version` prefix only in the announce.  
11. Binds TCP and serves via `axum::serve`.  
12. Spawns `process_ipc_messages()` to handle rotations & shutdown.

All errors propagate as `pywatt_sdk::Error`.

### Example

```rust
use pywatt_sdk::prelude::*;
use axum::{Router, routing::get, Extension};
use secrecy::SecretString;

// Custom state
#[derive(Clone)]
struct MyState { db_url: String }

// State builder
fn build_state(
    init: &OrchestratorInit,
    secrets: Vec<SecretString>
) -> MyState {
    let db_url = secrets
        .get(0)
        .map(|s| s.expose_secret().clone())
        .unwrap_or_default();
    MyState { db_url }
}

// Annotated router builder
#[pywatt_sdk::module(
    state     = build_state,
    secrets   = ["DB_URL"],
    rotate    = false,
    endpoints = [
      EndpointAnnounce { path = "/status", methods = ["GET"], auth = None }
    ],
    health    = "/healthz",
    metrics   = true,
    version   = "v1",
)]
async fn build_router(state: AppState<MyState>) -> Router {
    Router::new()
      .route("/status", get(|Extension(s)| async move {
         format!("DB URL: {}", s.user_state.db_url)
      }))
      .layer(Extension(state))
}
// `main()` is generated; no further code needed.
```

---

## 3. Manual Setup with `serve_module`

For fine-grained control, call `serve_module(secret_keys, endpoints, state_builder, router_builder)`.  

```rust
#[tokio::main]
async fn main() -> Result<()> {
    // 1. List secrets to fetch
    let secret_keys = vec!["DB_URL".to_string(), "API_KEY".to_string()];

    // 2. Declare endpoints (manual announcement)
    let endpoints = vec![
      EndpointAnnounce { path: "/status".into(), methods: vec!["GET".into()], auth: None }
    ];

    // 3. State builder
    let state_builder = |init: &OrchestratorInit, secrets: Vec<SecretString>| MyState {
        db_url: secrets.get(0).map(|s| s.expose_secret().clone()).unwrap_or_default()
    };

    // 4. Router builder
    let router_builder = |app_state: AppState<MyState>| {
      Router::new()
        .route("/status", get(|Extension(s)| async move {
           format!("DB URL: {}", s.user_state.db_url)
        }))
        .layer(Extension(app_state))
    };

    // 5. Run server
    serve_module(secret_keys, endpoints, state_builder, router_builder).await?;
    Ok(())
}
```

`serve_module` follows the same lifecycle as the macro: handshake, secrets, state, announce, IPC loop, serve.

---

## 4. Key Concepts

- **AppState<T>**: Shared across handlers. Contains `module_id()`, `orchestrator_api()`, `secret_client()`, and your `user_state: T`.
- **Secrets**: Use `get_secret` / `get_secrets` / `get_typed_secret`.  
- **Rotation**: `subscribe_secret_rotations(client, keys, callback)` auto-handles secret refresh.
- **Logging**: `init_module()` sets up `tracing` with JSON and orchestrator-driven log levels.
- **Announcement**: `ModuleAnnounce` (listen + endpoints) tells the orchestrator your API surface.
- **Error Handling**: Functions return `pywatt_sdk::Result<T>`, use `?` to propagate.

---

### Project Layout

```
my-module/
├── Cargo.toml
├── build.rs
└── src/
    └── main.rs   # Contains either the macro-annotated fn or manual main
```

That's it—happy coding!   
Refer to `cargo doc --open` for full API reference.  
Support and examples live in the `examples/` folder of the SDK repo. 