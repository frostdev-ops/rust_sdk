//! HTTP result types and error handling for the HTTP IPC module.
//!
//! This module provides standardized error types and response helper functions
//! for HTTP over IPC communication.

use crate::ipc_types::IpcHttpResponse;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// HTTP IPC specific errors
#[derive(Debug, Error)]
pub enum HttpIpcError {
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("SDK error: {0}")]
    Sdk(#[from] crate::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Other error: {0}")]
    Other(String),
}

impl From<std::io::Error> for HttpIpcError {
    fn from(err: std::io::Error) -> Self {
        Self::Internal(format!("IO error: {}", err))
    }
}

/// Result type for HTTP IPC operations
pub type HttpResult<T> = std::result::Result<T, HttpIpcError>;

/// Parse JSON request body
pub fn parse_json_body<T: for<'de> Deserialize<'de>>(body: &Option<Vec<u8>>) -> HttpResult<T> {
    match body {
        Some(data) => serde_json::from_slice(data)
            .map_err(|e| HttpIpcError::InvalidRequest(format!("Invalid JSON body: {}", e))),
        None => Err(HttpIpcError::InvalidRequest(
            "Request body is required".to_string(),
        )),
    }
}

/// Create a JSON response
pub fn json_response<T: Serialize>(data: T, status_code: u16) -> HttpResult<IpcHttpResponse> {
    let response = super::ApiResponse {
        status: "success".to_string(),
        data: Some(data),
        message: None,
    };

    // Serialize the response to JSON
    let body = serde_json::to_vec(&response).map_err(HttpIpcError::Json)?;

    // Create headers with content type
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());

    Ok(IpcHttpResponse {
        request_id: String::new(), // Will be replaced before sending
        status_code,
        headers,
        body: Some(body),
    })
}

/// Create a success response
pub fn success<T: Serialize>(data: T) -> HttpResult<IpcHttpResponse> {
    json_response(data, 200)
}

/// Create an error response
pub fn error_response<T: Serialize>(
    message: &str,
    status_code: u16,
) -> HttpResult<IpcHttpResponse> {
    let response = super::ApiResponse::<T> {
        status: "error".to_string(),
        data: None,
        message: Some(message.to_string()),
    };

    // Serialize the response to JSON
    let body = serde_json::to_vec(&response).map_err(HttpIpcError::Json)?;

    // Create headers with content type
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());

    Ok(IpcHttpResponse {
        request_id: String::new(), // Will be replaced before sending
        status_code,
        headers,
        body: Some(body),
    })
}

/// Create a not found response
pub fn not_found(path: &str) -> HttpResult<IpcHttpResponse> {
    error_response::<()>(&format!("Endpoint not found: {}", path), 404)
}

/// Convert an error to an HTTP response
pub(crate) fn error_to_response(error: HttpIpcError, request_id: &str) -> IpcHttpResponse {
    let status_code = match &error {
        HttpIpcError::InvalidRequest(_) => 400,
        HttpIpcError::NotFound(_) => 404,
        HttpIpcError::Unauthorized(_) => 401,
        HttpIpcError::Forbidden(_) => 403,
        _ => 500,
    };

    let response = super::ApiResponse::<()> {
        status: "error".to_string(),
        data: None,
        message: Some(error.to_string()),
    };

    // Create headers with content type
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());

    // Attempt to serialize the response, fallback to plain text if that fails
    match serde_json::to_vec(&response) {
        Ok(body) => IpcHttpResponse {
            request_id: request_id.to_string(),
            status_code,
            headers,
            body: Some(body),
        },
        Err(_) => {
            headers.insert("Content-Type".to_string(), "text/plain".to_string());
            IpcHttpResponse {
                request_id: request_id.to_string(),
                status_code,
                headers,
                body: Some(error.to_string().into_bytes()),
            }
        }
    }
}
