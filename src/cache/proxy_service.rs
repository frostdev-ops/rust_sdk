use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;

use crate::cache::{CacheConfig, CacheError, CacheResult, CacheService, CacheStats, CacheType};
use crate::ipc::send_request;
use crate::ipc_types::{
    ServiceOperation, ServiceOperationResult, ServiceRequest, ServiceResponse, ServiceType,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};

// ProxyCacheService for handling cache operations via IPC
pub struct ProxyCacheService {
    connection_id: String,
    cache_type: CacheType,
    default_ttl: Duration,
}

impl ProxyCacheService {
    pub async fn connect(config: &CacheConfig) -> CacheResult<Self> {
        // Create a unique ID for this connection request
        let request_id = format!("cache_request_{}", uuid::Uuid::new_v4());

        // Create a service request
        let request = ServiceRequest {
            id: request_id.clone(),
            service_type: ServiceType::Cache,
            config: Some(serde_json::to_value(config).map_err(|e| {
                CacheError::Configuration(format!("Failed to serialize cache config: {}", e))
            })?),
        };

        // Send the request to the orchestrator
        let response = send_request(&request)
            .await
            .map_err(|e| CacheError::Connection(format!("Failed to send request: {}", e)))?;

        // Deserialize the response
        let service_response: ServiceResponse = serde_json::from_str(&response)
            .map_err(|e| CacheError::Connection(format!("Failed to parse response: {}", e)))?;

        // Check if the request was successful
        if !service_response.success {
            return Err(CacheError::Connection(
                service_response
                    .error
                    .unwrap_or_else(|| "Unknown connection error".to_string()),
            ));
        }

        // Get the connection ID
        let connection_id = service_response
            .connection_id
            .ok_or_else(|| CacheError::Connection("No connection ID returned".to_string()))?;

        Ok(Self {
            connection_id,
            cache_type: match config.cache_type {
                CacheType::Redis => CacheType::Redis,
                CacheType::Memcached => CacheType::Memcached,
                CacheType::InMemory => CacheType::InMemory,
                CacheType::File => CacheType::File,
            },
            default_ttl: config.get_default_ttl(),
        })
    }
}

#[async_trait]
impl CacheService for ProxyCacheService {
    async fn get(&self, key: &str) -> CacheResult<Option<Vec<u8>>> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "get".to_string(),
            params: serde_json::json!({
                "key": key,
            }),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                if value.is_null() {
                    return Ok(None);
                }

                if let Some(s) = value.as_str() {
                    // Value is base64 encoded
                    match STANDARD.decode(s) {
                        Ok(bytes) => Ok(Some(bytes)),
                        Err(e) => Err(CacheError::Serialization(format!("Invalid base64: {}", e))),
                    }
                } else if let Some(array) = value.as_array() {
                    // Value is a byte array
                    let bytes: Result<Vec<u8>, _> = array
                        .iter()
                        .map(|v| {
                            if let Some(n) = v.as_u64() {
                                if n <= 255 {
                                    Ok(n as u8)
                                } else {
                                    Err(CacheError::Serialization(format!(
                                        "Invalid byte value: {}",
                                        n
                                    )))
                                }
                            } else {
                                Err(CacheError::Serialization("Invalid byte value".to_string()))
                            }
                        })
                        .collect();

                    Ok(Some(bytes?))
                } else {
                    Err(CacheError::Serialization(
                        "Invalid value format".to_string(),
                    ))
                }
            }
            None => Ok(None),
        }
    }

    async fn set(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> CacheResult<()> {
        // Convert value to base64 for transport
        let value_base64 = STANDARD.encode(value);

        // Convert ttl to seconds
        let ttl_seconds = ttl.map(|d| d.as_secs());

        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "set".to_string(),
            params: serde_json::json!({
                "key": key,
                "value": value_base64,
                "ttl_seconds": ttl_seconds,
            }),
        };

        let result = send_operation(operation).await?;
        if result.success {
            Ok(())
        } else {
            Err(CacheError::Operation(
                result
                    .error
                    .unwrap_or_else(|| "Set operation failed".to_string()),
            ))
        }
    }

    async fn delete(&self, key: &str) -> CacheResult<bool> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "delete".to_string(),
            params: serde_json::json!({
                "key": key,
            }),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                if let Some(success) = value.as_bool() {
                    Ok(success)
                } else {
                    Err(CacheError::Operation("Invalid delete result".to_string()))
                }
            }
            None => Ok(false), // Assume not found
        }
    }

    async fn exists(&self, key: &str) -> CacheResult<bool> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "exists".to_string(),
            params: serde_json::json!({
                "key": key,
            }),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                if let Some(exists) = value.as_bool() {
                    Ok(exists)
                } else {
                    Err(CacheError::Operation("Invalid exists result".to_string()))
                }
            }
            None => Ok(false), // Assume not found
        }
    }

    async fn set_nx(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> CacheResult<bool> {
        // Convert value to base64 for transport
        let value_base64 = STANDARD.encode(value);

        // Convert ttl to seconds
        let ttl_seconds = ttl.map(|d| d.as_secs());

        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "set_nx".to_string(),
            params: serde_json::json!({
                "key": key,
                "value": value_base64,
                "ttl_seconds": ttl_seconds,
            }),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                if let Some(success) = value.as_bool() {
                    Ok(success)
                } else {
                    Err(CacheError::Operation("Invalid set_nx result".to_string()))
                }
            }
            None => Ok(false), // Assume not set
        }
    }

    async fn get_set(&self, key: &str, value: &[u8]) -> CacheResult<Option<Vec<u8>>> {
        // Convert value to base64 for transport
        let value_base64 = STANDARD.encode(value);

        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "get_set".to_string(),
            params: serde_json::json!({
                "key": key,
                "value": value_base64,
            }),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                if value.is_null() {
                    return Ok(None);
                }

                if let Some(s) = value.as_str() {
                    // Value is base64 encoded
                    match STANDARD.decode(s) {
                        Ok(bytes) => Ok(Some(bytes)),
                        Err(e) => Err(CacheError::Serialization(format!("Invalid base64: {}", e))),
                    }
                } else {
                    Err(CacheError::Serialization(
                        "Invalid value format".to_string(),
                    ))
                }
            }
            None => Ok(None),
        }
    }

    async fn increment(&self, key: &str, delta: i64) -> CacheResult<i64> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "increment".to_string(),
            params: serde_json::json!({
                "key": key,
                "delta": delta,
            }),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                if let Some(n) = value.as_i64() {
                    Ok(n)
                } else {
                    Err(CacheError::Operation(
                        "Invalid increment result".to_string(),
                    ))
                }
            }
            None => Err(CacheError::Operation(
                "No result from increment operation".to_string(),
            )),
        }
    }

    async fn set_many(
        &self,
        items: &HashMap<String, Vec<u8>>,
        ttl: Option<Duration>,
    ) -> CacheResult<()> {
        // Convert values to base64 for transport
        let mut items_base64 = HashMap::new();
        for (key, value) in items {
            items_base64.insert(key.clone(), STANDARD.encode(value));
        }

        // Convert ttl to seconds
        let ttl_seconds = ttl.map(|d| d.as_secs());

        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "set_many".to_string(),
            params: serde_json::json!({
                "items": items_base64,
                "ttl_seconds": ttl_seconds,
            }),
        };

        let result = send_operation(operation).await?;
        if result.success {
            Ok(())
        } else {
            Err(CacheError::Operation(
                result
                    .error
                    .unwrap_or_else(|| "Set many operation failed".to_string()),
            ))
        }
    }

    async fn get_many(&self, keys: &[String]) -> CacheResult<HashMap<String, Vec<u8>>> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "get_many".to_string(),
            params: serde_json::json!({
                "keys": keys,
            }),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                let map: HashMap<String, String> = serde_json::from_value(value).map_err(|e| {
                    CacheError::Serialization(format!("Failed to deserialize result: {}", e))
                })?;

                let mut result_map = HashMap::new();
                for (key, value_base64) in map {
                    match STANDARD.decode(&value_base64) {
                        Ok(bytes) => {
                            result_map.insert(key, bytes);
                        }
                        Err(e) => {
                            return Err(CacheError::Serialization(format!(
                                "Invalid base64: {}",
                                e
                            )));
                        }
                    }
                }

                Ok(result_map)
            }
            None => Ok(HashMap::new()), // Return empty map if no results
        }
    }

    async fn delete_many(&self, keys: &[String]) -> CacheResult<u64> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "delete_many".to_string(),
            params: serde_json::json!({
                "keys": keys,
            }),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                if let Some(n) = value.as_u64() {
                    Ok(n)
                } else {
                    Err(CacheError::Operation(
                        "Invalid delete_many result".to_string(),
                    ))
                }
            }
            None => Ok(0), // Assume none deleted
        }
    }

    async fn clear(&self, namespace: Option<&str>) -> CacheResult<()> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "clear".to_string(),
            params: serde_json::json!({
                "namespace": namespace,
            }),
        };

        let result = send_operation(operation).await?;
        if result.success {
            Ok(())
        } else {
            Err(CacheError::Operation(
                result
                    .error
                    .unwrap_or_else(|| "Clear operation failed".to_string()),
            ))
        }
    }

    async fn lock(&self, key: &str, ttl: Duration) -> CacheResult<Option<String>> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "lock".to_string(),
            params: serde_json::json!({
                "key": key,
                "ttl_seconds": ttl.as_secs(),
            }),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                if value.is_null() {
                    return Ok(None);
                }

                if let Some(s) = value.as_str() {
                    Ok(Some(s.to_string()))
                } else {
                    Err(CacheError::Operation(
                        "Invalid lock token format".to_string(),
                    ))
                }
            }
            None => Ok(None), // Lock not acquired
        }
    }

    async fn unlock(&self, key: &str, lock_token: &str) -> CacheResult<bool> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "unlock".to_string(),
            params: serde_json::json!({
                "key": key,
                "lock_token": lock_token,
            }),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                if let Some(success) = value.as_bool() {
                    Ok(success)
                } else {
                    Err(CacheError::Operation("Invalid unlock result".to_string()))
                }
            }
            None => Ok(false), // Assume unlock failed
        }
    }

    fn get_cache_type(&self) -> CacheType {
        self.cache_type
    }

    async fn ping(&self) -> CacheResult<()> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "ping".to_string(),
            params: serde_json::json!({}),
        };

        let result = send_operation(operation).await?;
        if result.success {
            Ok(())
        } else {
            Err(CacheError::Connection(
                result.error.unwrap_or_else(|| "Ping failed".to_string()),
            ))
        }
    }

    async fn close(&self) -> CacheResult<()> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "close".to_string(),
            params: serde_json::json!({}),
        };

        let result = send_operation(operation).await?;
        if result.success {
            Ok(())
        } else {
            Err(CacheError::Connection(
                result.error.unwrap_or_else(|| "Close failed".to_string()),
            ))
        }
    }

    fn get_default_ttl(&self) -> Duration {
        self.default_ttl
    }

    async fn stats(&self) -> CacheResult<CacheStats> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "stats".to_string(),
            params: serde_json::json!({}),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => serde_json::from_value(value).map_err(|e| {
                CacheError::Serialization(format!("Failed to deserialize stats: {}", e))
            }),
            None => Err(CacheError::Operation("No stats available".to_string())),
        }
    }

    async fn flush(&self) -> CacheResult<()> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Cache,
            operation: "flush".to_string(),
            params: serde_json::json!({}),
        };

        let result = send_operation(operation).await?;
        if result.success {
            Ok(())
        } else {
            Err(CacheError::Operation(
                result
                    .error
                    .unwrap_or_else(|| "Flush operation failed".to_string()),
            ))
        }
    }
}

// Helper function to send an operation and receive the result
async fn send_operation(operation: ServiceOperation) -> CacheResult<ServiceOperationResult> {
    let response = send_request(&operation)
        .await
        .map_err(|e| CacheError::Operation(format!("Failed to send operation: {}", e)))?;

    let result: ServiceOperationResult = serde_json::from_str(&response)
        .map_err(|e| CacheError::Operation(format!("Failed to parse response: {}", e)))?;

    if !result.success {
        let error_msg = result.error.unwrap_or_else(|| "Unknown error".to_string());
        return Err(CacheError::Operation(error_msg));
    }

    Ok(result)
}

// Add IPC error to the CacheError enum
impl From<String> for CacheError {
    fn from(error: String) -> Self {
        CacheError::Connection(format!("IPC error: {}", error))
    }
}
