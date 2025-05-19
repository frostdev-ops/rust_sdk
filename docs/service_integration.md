# PyWatt SDK Service Integration

This document describes how to use the service integration features in the PyWatt SDK.

## Overview

PyWatt SDK supports seamless service integration with the orchestrator. When a module is running under the orchestrator, it can request services like databases, caches, and JWT authentication to be provided by the orchestrator instead of connecting directly.

This approach has several advantages:

1. **Simplified configuration**: Modules don't need to know connection details
2. **Central management**: Credentials and connections are managed by the orchestrator
3. **Resource sharing**: Multiple modules can use the same connection pool
4. **Connection security**: Modules don't need direct access to service credentials

## Enabling Service Integration

To use service integration, add the `ipc` feature to your dependencies:

```toml
[dependencies]
pywatt_sdk = { version = "0.2.7", features = ["ipc"] }
```

## How It Works

When a module is started by the orchestrator, an environment variable `PYWATT_MODULE_ID` is set. The SDK checks for this variable to determine if it should use the orchestrator's services.

The service integration works transparently:

1. When your module calls `create_database_connection()`, `create_cache_service()`, or JWT auth functions
2. The SDK detects if running under the orchestrator
3. If yes, it sends a service request via IPC
4. The orchestrator creates the requested service and assigns a connection ID
5. The SDK returns a proxy adapter that sends operations to the orchestrator
6. Your code interacts with the proxy adapter the same way it would with a direct connection

## Database Connections

Use the standard database connection functions:

```rust
use pywatt_sdk::database::{DatabaseConfig, create_database_connection};

// Configure database
let config = DatabaseConfig {
    db_type: DatabaseType::Postgres,
    // ...other settings
};

// Create connection (will use proxy if running as a module)
let db = create_database_connection(&config).await?;

// Use the connection as normal
let rows = db.query("SELECT * FROM users", &[]).await?;
```

## Cache Services

Use the standard cache service functions:

```rust
use pywatt_sdk::cache::{CacheConfig, create_cache_service};

// Configure cache
let config = CacheConfig {
    cache_type: CacheType::Redis,
    // ...other settings
};

// Create cache service (will use proxy if running as a module)
let cache = create_cache_service(&config).await?;

// Use the cache as normal
cache.set("key", "value".as_bytes(), Some(Duration::from_secs(60))).await?;
```

## JWT Authentication

JWT authentication also supports the proxy approach:

```rust
use pywatt_sdk::jwt_auth::{RouterJwtExt, JwtProxyService};

// For middleware, use as normal
let app = Router::new()
    .route("/protected", get(handler))
    .with_jwt::<MyClaims>("my-secret".to_string());

// For direct token validation
#[cfg(feature = "ipc")]
{
    // Only if running as a module and you need direct validation
    let jwt_config = JwtProxyConfig::default();
    let jwt_service = JwtProxyService::connect(&jwt_config).await?;
    
    // Validate or generate tokens
    let claims: MyClaims = jwt_service.validate_token(&token).await?;
    let token = jwt_service.generate_token(&claims).await?;
}
```

## Serialization Details

The service proxies handle serialization of values automatically:

1. **Database values** are serialized to JSON with special handling for binary data
2. **Cache values** are base64-encoded for safe transport
3. **JWT tokens and claims** are handled using JSON serialization

## Error Handling

Error handling is unified - the proxy adapters map IPC errors to the appropriate error types:

- `DatabaseError::Connection` - For database connection issues
- `CacheError::Connection` - For cache connection issues
- `JwtError::Internal` - For JWT service issues

## Configuration Inheritance

When running as a module, the orchestrator may override configuration settings. The module's configuration is sent with the service request, but the orchestrator might change:

1. Connection parameters (host, port, credentials)
2. Pool settings (max connections, timeouts)
3. Security settings 

Your code should be prepared to work with settings that might differ from what was specified.

## Testing and Development

For testing and development outside the orchestrator:

1. Run without the `PYWATT_MODULE_ID` environment variable
2. The SDK will create direct connections to services
3. Use the same code in both environments for consistency

This allows seamless transition between development and production environments. 