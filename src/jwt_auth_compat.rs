// This file is now replaced by the jwt_auth/ directory with mod.rs, error.rs, and middleware.rs
// It is kept for backward compatibility but will be deprecated in the future

#[deprecated(
    since = "0.2.5",
    note = "Use crate::jwt_auth::error::JwtAuthError instead"
)]
pub use crate::jwt_auth::error::JwtAuthError;

#[deprecated(
    since = "0.2.5",
    note = "Use crate::jwt_auth::middleware::JwtAuthenticationLayer instead"
)]
pub use crate::jwt_auth::middleware::JwtAuthenticationLayer;

#[deprecated(
    since = "0.2.5",
    note = "Use crate::jwt_auth::middleware::JwtAuthService instead"
)]
pub use crate::jwt_auth::middleware::JwtAuthService;

#[deprecated(since = "0.2.5", note = "Use crate::jwt_auth::RouterJwtExt instead")]
pub use crate::jwt_auth::RouterJwtExt;

#[deprecated(since = "0.2.5", note = "Use crate::jwt_auth::JwtAuthLayer instead")]
pub use crate::jwt_auth::JwtAuthLayer;

// Re-export for backward compatibility
#[deprecated(since = "0.2.5", note = "Use JwtAuthService directly")]
pub type JwtAuthenticationMiddleware<S, T = serde_json::Value> = JwtAuthService<S, T>;
