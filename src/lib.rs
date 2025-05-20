// Re-export core types and functions
mod announce;
#[cfg(test)]
mod announce_tests;
mod bootstrap;
#[cfg(test)]
mod bootstrap_tests;
#[cfg(all(test, feature = "database"))]
mod database_tests;
pub mod error;
#[cfg(test)]
mod feature_tests;
mod handshake;
pub mod http_ipc;
pub mod http_tcp;
#[cfg(feature = "ipc_channel")]
pub mod ipc;
pub mod ipc_types;
pub mod internal_messaging;
mod logging;
pub mod message;
pub mod model_manager;
pub mod registration;
mod router_discovery;
mod secrets;
mod server;
// Export all server module components from a single place
pub mod state;
#[cfg(test)]
mod state_tests;
pub mod tcp_channel;
pub mod tcp_types;
pub mod typed_secret;
mod utils;
pub mod ext;
pub use ext::OrchestratorInitExt;
pub mod builder;
pub mod build;

// Modules from merged crates
pub mod secret_client;
pub mod secret_provider;

// Database module for standardized database access
#[cfg(feature = "database")]
pub mod database;

// Cache module for standardized cache access
#[cfg(feature = "cache")]
pub mod cache;

// Conditionally include and re-export JWT auth functionality
#[cfg(feature = "jwt_auth")]
pub mod jwt_auth;

// Backward compatibility module for jwt_auth
#[cfg(feature = "jwt_auth")]
#[path = "jwt_auth_compat.rs"]
#[deprecated(since = "0.2.5", note = "Use the jwt_auth module directly")]
pub mod jwt_auth_compat;

// Conditionally include and re-export proc macros
#[cfg(feature = "proc_macros")]
pub use pywatt_macros::module;

#[cfg(feature = "proc_macros")]
#[deprecated(note = "Use the `#[pywatt_sdk::module]` attribute from the `pywatt_macros` crate")]
pub mod macros;

// Re-export ipc_types
pub use ipc_types::{
    Announce as ModuleAnnounce,
    AnnounceBlob,
    ClientRequest,
    Endpoint as AnnouncedEndpoint,
    EndpointAnnounce,
    GetSecretRequest,
    // Legacy aliases
    Init as OrchestratorInit,
    InitBlob,
    ListenAddress,
    ModuleToOrchestrator,
    OrchestratorToModule,
    RotatedNotification,
    RotationAckRequest,
    SecretValueResponse,
    ServerResponse,
};

// Re-export message types
pub use message::{
    EncodedMessage, EncodedStream, EncodingFormat, Message, MessageError, MessageMetadata,
    MessageResult,
};

// Re-export TCP types
pub use tcp_types::{
    ConnectionConfig, ConnectionState, NetworkError, NetworkResult, ReconnectPolicy, TlsConfig,
};

// Re-export TCP channel
pub use tcp_channel::{MessageChannel, TcpChannel};

// Re-export registration types
pub use registration::{
    Capabilities, Endpoint, HealthStatus, ModuleInfo, RegisteredModule, RegistrationError,
    advertise_capabilities, heartbeat, register_module, start_heartbeat_loop, unregister_module,
};

// Re-export HTTP-over-TCP types
pub use http_tcp::{
    ApiResponse as HttpTcpApiResponse, HttpTcpClient, HttpTcpRequest, HttpTcpResponse, 
    HttpTcpRouter, serve as serve_http_tcp,
};

// Database re-exports
#[cfg(feature = "database")]
pub use database::{
    DatabaseConfig, DatabaseConnection, DatabaseError, DatabaseResult, DatabaseRow,
    DatabaseTransaction, DatabaseType, DatabaseValue, create_database_connection, extensions,
};

// Cache re-exports
#[cfg(feature = "cache")]
pub use cache::{
    CacheConfig, CacheError, CacheResult, CacheService, CacheStats, CacheType, create_cache_service,
};

// Re-export model_manager types
pub use model_manager::ModelManager;

// Re-export functions & types from SDK modules
pub use announce::{AnnounceError, send_announce};
pub use bootstrap::{BootstrapError, bootstrap_module};
pub use builder::ModuleBuilder;
pub use error::{Error, Result};
pub use handshake::{InitError, read_init};
#[cfg(feature = "ipc_channel")]
pub use ipc::IpcManager;
pub use internal_messaging::{InternalMessagingClient, InternalMessagingError, InternalMessagingResult};
pub use logging::init_module;
pub use router_discovery::announce_from_router;
pub use secrets::{get_module_secret_client, get_secret, get_secrets, subscribe_secret_rotations};
pub use server::{Error as ServeError, serve_module, serve_with_options, ServeOptions};
pub use state::AppState;
pub use typed_secret::{Secret, TypedSecretError, get_typed_secret};
pub use utils::{eprint_json, eprint_pretty_json, print_json, print_pretty_json};

// Feature-gated re-exports
#[cfg(feature = "router_ext")]
pub use ext::RouterExt;
// Compatibility re-exports for backward compatibility
#[cfg(feature = "jwt_auth")]
/// Re-export JWT auth extension trait and layer at crate root
#[deprecated(since = "0.2.5", note = "Import from jwt_auth module directly")]
pub use crate::jwt_auth::{JwtAuthLayer, RouterJwtExt};

/// Re-export secret redaction helpers for backward compatibility
pub use crate::secret_client::{register_for_redaction, register_secret_for_redaction};

/// Prelude module, exposing the most commonly used items.
pub mod prelude {
    pub use crate::{
        AnnouncedEndpoint,
        AppState,
        // Add message types to prelude
        EncodedMessage,
        EncodedStream,
        EncodingFormat,
        Error,
        Message,
        MessageError,
        MessageMetadata,
        MessageResult,
        ModuleAnnounce,
        OrchestratorInit,
        Result,
        Secret,
        TypedSecretError,
        build,
        get_module_secret_client,
        get_secret,
        get_secrets,
        get_typed_secret,
        http_ipc,
        init_module,
    };

    // Import IPC-related functions conditionally
    #[cfg(feature = "ipc_channel")]
    pub use crate::ipc::process_ipc_messages;

    // Continue with other exports
    pub use crate::{
        ipc_types,
        read_init,
        send_announce,
        subscribe_secret_rotations,
        
        // Add TCP-related exports to prelude
        tcp_channel::MessageChannel,
        tcp_channel::TcpChannel,
        tcp_types::{ConnectionConfig, ConnectionState, NetworkError, NetworkResult, ReconnectPolicy, TlsConfig},
        
        // Add registration exports to prelude
        registration::{
            Capabilities, Endpoint, HealthStatus, ModuleInfo, RegisteredModule, RegistrationError,
            advertise_capabilities, heartbeat, register_module, start_heartbeat_loop, unregister_module,
        },
        
        // Add HTTP-over-TCP exports to prelude
        http_tcp::{
            ApiResponse as HttpTcpApiResponse, HttpTcpClient, HttpTcpRequest, HttpTcpResponse, 
            HttpTcpRouter, serve as serve_http_tcp,
        },
    };

    // HTTP-over-IPC specific exports
    #[cfg(feature = "ipc_channel")]
    pub use crate::http_ipc::{
        send_http_response,
        subscribe_http_requests,
    };

    // Re-export HTTP IPC utilities for convenience
    pub use crate::http_ipc::{
        ApiResponse, HttpIpcError, HttpIpcRouter, HttpResult, error_response, json_response,
        not_found, parse_json_body, start_http_ipc_server, success,
    };

    // Database API
    #[cfg(feature = "database")]
    pub use crate::database::{
        DatabaseConfig, DatabaseConnection, DatabaseError, DatabaseResult, DatabaseRow,
        DatabaseTransaction, DatabaseType, DatabaseValue, create_database_connection, extensions,
    };

    #[cfg(all(feature = "database", feature = "postgres"))]
    pub use crate::database::PostgresConnection;

    #[cfg(all(feature = "database", feature = "mysql"))]
    pub use crate::database::MySqlConnection;

    #[cfg(all(feature = "database", feature = "sqlite"))]
    pub use crate::database::SqliteConnection;

    // Cache API
    #[cfg(feature = "cache")]
    pub use crate::cache::{
        CacheConfig, CacheError, CacheResult, CacheService, CacheStats, CacheType,
        create_cache_service,
    };

    #[cfg(all(feature = "cache", feature = "redis_cache"))]
    pub use crate::cache::RedisCache;

    #[cfg(all(feature = "cache", feature = "memcached"))]
    pub use crate::cache::MemcachedCache;

    #[cfg(feature = "cache")]
    pub use crate::cache::InMemoryCache;

    #[cfg(all(feature = "cache", feature = "file_cache"))]
    pub use crate::cache::FileCache;

    #[cfg(feature = "cache")]
    pub use crate::cache::patterns::{
        DistributedLock, EventInvalidationStrategy, InvalidationStrategy, LockError,
        LruInvalidationStrategy, RetryConfig, TtlInvalidationStrategy, cache_aside, write_through,
    };

    // Secret client APIs
    pub use crate::secret_client::{RequestMode, SecretClient, SecretError as SecretClientError};

    // Secret provider APIs
    pub use crate::secret_provider::{
        SecretError as SecretProviderError, SecretEvent, SecretProvider,
    };

    #[cfg(feature = "jwt_auth")]
    pub use crate::jwt_auth::error::JwtAuthError;
    #[cfg(feature = "jwt_auth")]
    pub use crate::jwt_auth::middleware::JwtAuthenticationLayer;

    #[cfg(feature = "proc_macros")]
    pub use crate::module;

    #[cfg(feature = "router_ext")]
    pub use crate::ext::RouterExt;

    #[cfg(feature = "discover_endpoints")]
    pub use crate::router_discovery::announce_from_router;
}

// Add the test module
#[cfg(test)]
#[cfg(feature = "ipc")]
mod ipc_proxy_tests;
