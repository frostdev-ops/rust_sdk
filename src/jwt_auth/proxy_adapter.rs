use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::ipc::send_request;
use crate::ipc_types::{
    ServiceOperation, ServiceOperationResult, ServiceRequest, ServiceResponse, ServiceType,
};
use crate::jwt_auth::error::JwtAuthError;

/// JWT configuration for using a remote JWT validation service
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct JwtProxyConfig {
    /// Custom config parameters for the JWT service
    #[serde(default)]
    pub config: HashMap<String, String>,
}

/// JWT proxy service for validating tokens via IPC
pub struct JwtProxyService {
    connection_id: String,
}

impl JwtProxyService {
    pub async fn connect(config: &JwtProxyConfig) -> Result<Self, JwtAuthError> {
        // Create a unique ID for this connection request
        let request_id = format!("jwt_request_{}", uuid::Uuid::new_v4());

        // Create a service request
        let request = ServiceRequest {
            id: request_id.clone(),
            service_type: ServiceType::Jwt,
            config: Some(serde_json::to_value(config).map_err(|e| {
                JwtAuthError::InvalidToken(format!("Failed to serialize JWT config: {}", e))
            })?),
        };

        // Send the request to the orchestrator
        let response = send_request(&request)
            .await
            .map_err(|e| JwtAuthError::PayloadError(format!("Failed to send request: {}", e)))?;

        // Deserialize the response
        let service_response: ServiceResponse = serde_json::from_str(&response)
            .map_err(|e| JwtAuthError::PayloadError(format!("Failed to parse response: {}", e)))?;

        // Check if the request was successful
        if !service_response.success {
            return Err(JwtAuthError::PayloadError(
                service_response
                    .error
                    .unwrap_or_else(|| "Unknown connection error".to_string()),
            ));
        }

        // Get the connection ID
        let connection_id = service_response
            .connection_id
            .ok_or_else(|| JwtAuthError::PayloadError("No connection ID returned".to_string()))?;

        Ok(Self { connection_id })
    }

    /// Validate a JWT token and return the claims
    pub async fn validate_token<T>(&self, token: &str) -> Result<T, JwtAuthError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Jwt,
            operation: "validate".to_string(),
            params: serde_json::json!({
                "token": token,
            }),
        };

        let result = self.send_operation(operation).await?;
        match result.result {
            Some(value) => serde_json::from_value(value).map_err(|e| {
                JwtAuthError::InvalidToken(format!("Failed to deserialize claims: {}", e))
            }),
            None => Err(JwtAuthError::InvalidToken(
                "No claims returned from validation".to_string(),
            )),
        }
    }

    /// Generate a JWT token from claims
    pub async fn generate_token<T>(&self, claims: &T) -> Result<String, JwtAuthError>
    where
        T: Serialize,
    {
        let claims_value = serde_json::to_value(claims).map_err(|e| {
            JwtAuthError::InvalidToken(format!("Failed to serialize claims: {}", e))
        })?;

        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Jwt,
            operation: "generate".to_string(),
            params: serde_json::json!({
                "claims": claims_value,
            }),
        };

        let result = self.send_operation(operation).await?;
        match result.result {
            Some(value) => {
                if let Some(token) = value.as_str() {
                    Ok(token.to_string())
                } else {
                    Err(JwtAuthError::PayloadError(
                        "Invalid token format".to_string(),
                    ))
                }
            }
            None => Err(JwtAuthError::PayloadError(
                "No token returned from generation".to_string(),
            )),
        }
    }

    /// Close the JWT service connection
    pub async fn close(&self) -> Result<(), JwtAuthError> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Jwt,
            operation: "close".to_string(),
            params: serde_json::json!({}),
        };

        let result = self.send_operation(operation).await?;
        if result.success {
            Ok(())
        } else {
            Err(JwtAuthError::PayloadError(
                result.error.unwrap_or_else(|| "Close failed".to_string()),
            ))
        }
    }

    // Helper function to send an operation and receive the result
    async fn send_operation(
        &self,
        operation: ServiceOperation,
    ) -> Result<ServiceOperationResult, JwtAuthError> {
        let response = send_request(&operation)
            .await
            .map_err(|e| JwtAuthError::PayloadError(format!("Failed to send operation: {}", e)))?;

        let result: ServiceOperationResult = serde_json::from_str(&response)
            .map_err(|e| JwtAuthError::PayloadError(format!("Failed to parse response: {}", e)))?;

        if !result.success {
            let error_msg = result.error.unwrap_or_else(|| "Unknown error".to_string());
            return Err(JwtAuthError::InvalidToken(error_msg));
        }

        Ok(result)
    }
}

/// Middleware helper function to validate a JWT token via the proxy service
pub async fn validate_token_proxy<T>(
    jwt_service: &JwtProxyService,
    token: &str,
) -> Result<T, JwtAuthError>
where
    T: for<'de> Deserialize<'de>,
{
    jwt_service.validate_token(token).await
}

/// Helper function to generate a JWT token via the proxy service
pub async fn generate_token_proxy<T>(
    jwt_service: &JwtProxyService,
    claims: &T,
) -> Result<String, JwtAuthError>
where
    T: Serialize,
{
    jwt_service.generate_token(claims).await
}
