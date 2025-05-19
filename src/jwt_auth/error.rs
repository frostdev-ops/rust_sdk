use thiserror::Error;

/// Errors that can occur during JWT authentication
#[derive(Debug, Error)]
pub enum JwtAuthError {
    /// JWT token is invalid or expired
    #[error("Invalid token: {0}")]
    InvalidToken(String),
    /// Required authorization header is missing
    #[error("Missing Authorization header")]
    MissingHeader,
    /// JWT token is not well-formed
    #[error("Malformed token")]
    MalformedToken,
    /// JWT verification failed
    #[error("Token verification failed: {0}")]
    VerificationFailed(String),
    /// JWT payload could not be processed
    #[error("Payload error: {0}")]
    PayloadError(String),
    /// Error from jsonwebtoken library
    #[error("JWT error: {0}")]
    JwtError(#[from] jsonwebtoken::errors::Error),
}
