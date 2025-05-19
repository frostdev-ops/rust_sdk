//! JWT Authentication middleware for Axum.
//!
//! This module provides JWT authentication middleware for Axum applications.
//! It allows you to validate JWT tokens in the `Authorization: Bearer <token>` header
//! and extract claims into request extensions.
//!
//! The middleware can be used either with generic types for strong typing of claims,
//! or with dynamic types for flexibility.
//!
//! # Example
//! ```rust,no_run
//! use axum::{Router, routing::get, extract::Extension};
//! use pywatt_sdk::jwt_auth::{JwtAuthLayer, RouterJwtExt};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Serialize, Deserialize, Clone)]
//! struct MyClaims {
//!     sub: String,
//!     exp: usize,
//! }
//!
//! #[axum::debug_handler]
//! async fn handler(Extension(claims): Extension<MyClaims>) {
//!     println!("User: {}", claims.sub);
//! }
//!
//! // Method 1: Using the extension trait
//! let app1: Router = Router::new()
//!     .route("/", get(handler))
//!     .with_jwt::<MyClaims>("my-secret".to_string());
//!
//! // Method 2: Using the layer directly
//! let app2: Router = Router::new()
//!     .route("/", get(handler))
//!     .layer(JwtAuthLayer::<MyClaims>::new("my-secret".to_string()));
//! ```

pub mod error;
pub mod middleware;

#[cfg(feature = "ipc")]
pub mod proxy_adapter;

#[cfg(test)]
mod tests;

// Re-export the RouterJwtExt trait and JwtAuthLayer
use axum::Router;

use middleware::JwtAuthenticationLayer;

// Re-export proxy components when IPC is enabled
#[cfg(feature = "ipc")]
pub use proxy_adapter::{
    JwtProxyConfig, JwtProxyService, generate_token_proxy, validate_token_proxy,
};

/// Check if running as a module
#[inline]
fn is_running_as_module() -> bool {
    std::env::var("PYWATT_MODULE_ID").is_ok()
}

/// Extension trait for Axum Router to apply JWT auth.
pub trait RouterJwtExt {
    /// Layer this router with `JwtAuthenticationLayer` using the given HMAC secret.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The claims type to decode from the JWT. Defaults to `serde_json::Value`.
    fn with_jwt<T>(self, secret_key: String) -> Self
    where
        T: serde::de::DeserializeOwned + Send + Sync + Clone + 'static;
}

impl RouterJwtExt for Router {
    fn with_jwt<T>(self, secret_key: String) -> Self
    where
        T: serde::de::DeserializeOwned + Send + Sync + Clone + 'static,
    {
        self.layer(JwtAuthenticationLayer::<T>::new(secret_key))
    }
}

// For backward compatibility, also export as JwtAuthLayer
/// JWT Authentication layer for Axum
///
/// This type alias is provided for backward compatibility.
/// New code should use `JwtAuthenticationLayer` directly.
///
/// # Type Parameters
///
/// * `T` - The claims type to decode from the JWT. Defaults to `serde_json::Value`.
pub type JwtAuthLayer<T = serde_json::Value> = JwtAuthenticationLayer<T>;

use axum::body::{Bytes, HttpBody};
use futures::pin_mut;

/// Consume the provided HTTP body and concatenate all chunks into a UTF-8 `String`.
pub async fn body_to_string<B>(body: B) -> String
where
    B: HttpBody<Data = Bytes, Error = axum::Error> + Send + 'static,
{
    let mut bytes = Vec::new();
    pin_mut!(body);
    while let Some(chunk_result) = body.data().await {
        let chunk = chunk_result.unwrap();
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).unwrap()
}
