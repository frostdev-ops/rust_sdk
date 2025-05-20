use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::de::Error as DeError;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::database::{
    DatabaseConfig, DatabaseConnection, DatabaseError, DatabaseResult, DatabaseRow,
    DatabaseTransaction, DatabaseType, DatabaseValue,
};
use crate::ipc::send_request;
use crate::ipc_types::{
    ServiceOperation, ServiceOperationResult, ServiceRequest, ServiceResponse, ServiceType,
};

// Row implementation for proxy connections
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProxyDatabaseRow {
    data: std::collections::HashMap<String, serde_json::Value>,
}

impl DatabaseRow for ProxyDatabaseRow {
    fn get_string(&self, column: &str) -> DatabaseResult<String> {
        match self.data.get(column) {
            Some(value) => {
                if let Some(s) = value.as_str() {
                    Ok(s.to_string())
                } else {
                    Err(DatabaseError::Query(format!(
                        "Column '{}' is not a string",
                        column
                    )))
                }
            }
            None => Err(DatabaseError::Query(format!(
                "Column '{}' not found",
                column
            ))),
        }
    }

    fn get_i64(&self, column: &str) -> DatabaseResult<i64> {
        match self.data.get(column) {
            Some(value) => {
                if let Some(n) = value.as_i64() {
                    Ok(n)
                } else {
                    Err(DatabaseError::Query(format!(
                        "Column '{}' is not an integer",
                        column
                    )))
                }
            }
            None => Err(DatabaseError::Query(format!(
                "Column '{}' not found",
                column
            ))),
        }
    }

    fn get_f64(&self, column: &str) -> DatabaseResult<f64> {
        match self.data.get(column) {
            Some(value) => {
                if let Some(n) = value.as_f64() {
                    Ok(n)
                } else {
                    Err(DatabaseError::Query(format!(
                        "Column '{}' is not a float",
                        column
                    )))
                }
            }
            None => Err(DatabaseError::Query(format!(
                "Column '{}' not found",
                column
            ))),
        }
    }

    fn get_bool(&self, column: &str) -> DatabaseResult<bool> {
        match self.data.get(column) {
            Some(value) => {
                if let Some(b) = value.as_bool() {
                    Ok(b)
                } else {
                    Err(DatabaseError::Query(format!(
                        "Column '{}' is not a boolean",
                        column
                    )))
                }
            }
            None => Err(DatabaseError::Query(format!(
                "Column '{}' not found",
                column
            ))),
        }
    }

    fn get_bytes(&self, column: &str) -> DatabaseResult<Vec<u8>> {
        match self.data.get(column) {
            Some(value) => {
                if let Some(s) = value.as_str() {
                    match STANDARD.decode(s) {
                        Ok(bytes) => Ok(bytes),
                        Err(e) => Err(DatabaseError::Query(format!(
                            "Column '{}' has invalid base64: {}",
                            column, e
                        ))),
                    }
                } else if let Some(array) = value.as_array() {
                    let bytes: Result<Vec<u8>, _> = array
                        .iter()
                        .map(|v| {
                            if let Some(n) = v.as_u64() {
                                if n <= 255 {
                                    Ok(n as u8)
                                } else {
                                    Err(DatabaseError::Query(format!(
                                        "Invalid byte value {} in column '{}'",
                                        n, column
                                    )))
                                }
                            } else {
                                Err(DatabaseError::Query(format!(
                                    "Invalid byte value in column '{}'",
                                    column
                                )))
                            }
                        })
                        .collect();
                    bytes
                } else {
                    Err(DatabaseError::Query(format!(
                        "Column '{}' is not bytes",
                        column
                    )))
                }
            }
            None => Err(DatabaseError::Query(format!(
                "Column '{}' not found",
                column
            ))),
        }
    }

    fn try_get_string(&self, column: &str) -> DatabaseResult<Option<String>> {
        match self.data.get(column) {
            Some(value) => {
                if value.is_null() {
                    Ok(None)
                } else if let Some(s) = value.as_str() {
                    Ok(Some(s.to_string()))
                } else {
                    Err(DatabaseError::Query(format!(
                        "Column '{}' is not a string",
                        column
                    )))
                }
            }
            None => Ok(None),
        }
    }

    fn try_get_i64(&self, column: &str) -> DatabaseResult<Option<i64>> {
        match self.data.get(column) {
            Some(value) => {
                if value.is_null() {
                    Ok(None)
                } else if let Some(n) = value.as_i64() {
                    Ok(Some(n))
                } else {
                    Err(DatabaseError::Query(format!(
                        "Column '{}' is not an integer",
                        column
                    )))
                }
            }
            None => Ok(None),
        }
    }

    fn try_get_f64(&self, column: &str) -> DatabaseResult<Option<f64>> {
        match self.data.get(column) {
            Some(value) => {
                if value.is_null() {
                    Ok(None)
                } else if let Some(n) = value.as_f64() {
                    Ok(Some(n))
                } else {
                    Err(DatabaseError::Query(format!(
                        "Column '{}' is not a float",
                        column
                    )))
                }
            }
            None => Ok(None),
        }
    }

    fn try_get_bool(&self, column: &str) -> DatabaseResult<Option<bool>> {
        match self.data.get(column) {
            Some(value) => {
                if value.is_null() {
                    Ok(None)
                } else if let Some(b) = value.as_bool() {
                    Ok(Some(b))
                } else {
                    Err(DatabaseError::Query(format!(
                        "Column '{}' is not a boolean",
                        column
                    )))
                }
            }
            None => Ok(None),
        }
    }

    fn try_get_bytes(&self, column: &str) -> DatabaseResult<Option<Vec<u8>>> {
        match self.data.get(column) {
            Some(value) => {
                if value.is_null() {
                    Ok(None)
                } else if let Some(s) = value.as_str() {
                    match STANDARD.decode(s) {
                        Ok(bytes) => Ok(Some(bytes)),
                        Err(e) => Err(DatabaseError::Query(format!(
                            "Column '{}' has invalid base64: {}",
                            column, e
                        ))),
                    }
                } else if let Some(array) = value.as_array() {
                    let bytes: Result<Vec<u8>, _> = array
                        .iter()
                        .map(|v| {
                            if let Some(n) = v.as_u64() {
                                if n <= 255 {
                                    Ok(n as u8)
                                } else {
                                    Err(DatabaseError::Query(format!(
                                        "Invalid byte value {} in column '{}'",
                                        n, column
                                    )))
                                }
                            } else {
                                Err(DatabaseError::Query(format!(
                                    "Invalid byte value in column '{}'",
                                    column
                                )))
                            }
                        })
                        .collect();
                    bytes.map(Some)
                } else {
                    Err(DatabaseError::Query(format!(
                        "Column '{}' is not bytes",
                        column
                    )))
                }
            }
            None => Ok(None),
        }
    }
}

// ProxyDatabaseTransaction for handling transactions via IPC
pub struct ProxyDatabaseTransaction {
    connection_id: String,
    transaction_id: String,
}

#[async_trait]
impl DatabaseTransaction for ProxyDatabaseTransaction {
    async fn execute(&mut self, query: &str, params: &[DatabaseValue]) -> DatabaseResult<u64> {
        let serialized_params =
            serialize_params(params).map_err(|e| DatabaseError::Serialization(e.to_string()))?;

        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Database,
            operation: "transaction_execute".to_string(),
            params: serde_json::json!({
                "transaction_id": self.transaction_id,
                "query": query,
                "params": serialized_params
            }),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                let affected = value.as_u64().ok_or_else(|| {
                    DatabaseError::Query("Invalid affected rows count".to_string())
                })?;
                Ok(affected)
            }
            None => Err(DatabaseError::Query("No result from operation".to_string())),
        }
    }

    async fn query(
        &mut self,
        query: &str,
        params: &[DatabaseValue],
    ) -> DatabaseResult<Vec<Box<dyn DatabaseRow>>> {
        let serialized_params =
            serialize_params(params).map_err(|e| DatabaseError::Serialization(e.to_string()))?;

        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Database,
            operation: "transaction_query".to_string(),
            params: serde_json::json!({
                "transaction_id": self.transaction_id,
                "query": query,
                "params": serialized_params
            }),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                let rows: Vec<ProxyDatabaseRow> = serde_json::from_value(value).map_err(|e| {
                    DatabaseError::Serialization(format!("Failed to deserialize rows: {}", e))
                })?;

                Ok(rows
                    .into_iter()
                    .map(|r| Box::new(r) as Box<dyn DatabaseRow>)
                    .collect())
            }
            None => Err(DatabaseError::Query("No result from operation".to_string())),
        }
    }

    async fn query_one(
        &mut self,
        query: &str,
        params: &[DatabaseValue],
    ) -> DatabaseResult<Option<Box<dyn DatabaseRow>>> {
        let serialized_params =
            serialize_params(params).map_err(|e| DatabaseError::Serialization(e.to_string()))?;

        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Database,
            operation: "transaction_query_one".to_string(),
            params: serde_json::json!({
                "transaction_id": self.transaction_id,
                "query": query,
                "params": serialized_params
            }),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                if value.is_null() {
                    return Ok(None);
                }

                let row: ProxyDatabaseRow = serde_json::from_value(value).map_err(|e| {
                    DatabaseError::Serialization(format!("Failed to deserialize row: {}", e))
                })?;

                Ok(Some(Box::new(row) as Box<dyn DatabaseRow>))
            }
            None => Ok(None),
        }
    }

    async fn commit(self: Box<Self>) -> DatabaseResult<()> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Database,
            operation: "transaction_commit".to_string(),
            params: serde_json::json!({
                "transaction_id": self.transaction_id,
            }),
        };

        let result = send_operation(operation).await?;
        if result.success {
            Ok(())
        } else {
            Err(DatabaseError::Transaction(
                result
                    .error
                    .unwrap_or_else(|| "Unknown transaction error".to_string()),
            ))
        }
    }

    async fn rollback(self: Box<Self>) -> DatabaseResult<()> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Database,
            operation: "transaction_rollback".to_string(),
            params: serde_json::json!({
                "transaction_id": self.transaction_id,
            }),
        };

        let result = send_operation(operation).await?;
        if result.success {
            Ok(())
        } else {
            Err(DatabaseError::Transaction(
                result
                    .error
                    .unwrap_or_else(|| "Unknown transaction error".to_string()),
            ))
        }
    }
}

// ProxyDatabaseConnection for handling database connections via IPC
pub struct ProxyDatabaseConnection {
    connection_id: String,
    db_type: DatabaseType,
    active_transactions: Arc<Mutex<Vec<String>>>,
}

impl ProxyDatabaseConnection {
    pub async fn connect(config: &DatabaseConfig) -> DatabaseResult<Self> {
        // Create a unique ID for this connection request
        let request_id = format!("db_request_{}", uuid::Uuid::new_v4());

        // Create a service request
        let request = ServiceRequest {
            id: request_id.clone(),
            service_type: ServiceType::Database,
            config: Some(serde_json::to_value(config).map_err(|e| {
                DatabaseError::Configuration(format!("Failed to serialize database config: {}", e))
            })?),
        };

        // Send the request to the orchestrator
        let response = send_request(&request)
            .await
            .map_err(|e| DatabaseError::Connection(format!("Failed to send request: {}", e)))?;

        // Deserialize the response
        let service_response: ServiceResponse = serde_json::from_str(&response)
            .map_err(|e| DatabaseError::Connection(format!("Failed to parse response: {}", e)))?;

        // Check if the request was successful
        if !service_response.success {
            return Err(DatabaseError::Connection(
                service_response
                    .error
                    .unwrap_or_else(|| "Unknown connection error".to_string()),
            ));
        }

        // Get the connection ID
        let connection_id = service_response
            .connection_id
            .ok_or_else(|| DatabaseError::Connection("No connection ID returned".to_string()))?;

        Ok(Self {
            connection_id,
            db_type: match config.db_type {
                DatabaseType::Postgres => DatabaseType::Postgres,
                DatabaseType::MySql => DatabaseType::MySql,
                DatabaseType::Sqlite => DatabaseType::Sqlite,
            },
            active_transactions: Arc::new(Mutex::new(Vec::new())),
        })
    }
}

#[async_trait]
impl DatabaseConnection for ProxyDatabaseConnection {
    async fn execute(&self, query: &str, params: &[DatabaseValue]) -> DatabaseResult<u64> {
        let serialized_params =
            serialize_params(params).map_err(|e| DatabaseError::Serialization(e.to_string()))?;

        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Database,
            operation: "execute".to_string(),
            params: serde_json::json!({
                "query": query,
                "params": serialized_params
            }),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                let affected = value.as_u64().ok_or_else(|| {
                    DatabaseError::Query("Invalid affected rows count".to_string())
                })?;
                Ok(affected)
            }
            None => Err(DatabaseError::Query("No result from operation".to_string())),
        }
    }

    async fn query(
        &self,
        query: &str,
        params: &[DatabaseValue],
    ) -> DatabaseResult<Vec<Box<dyn DatabaseRow>>> {
        let serialized_params =
            serialize_params(params).map_err(|e| DatabaseError::Serialization(e.to_string()))?;

        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Database,
            operation: "query".to_string(),
            params: serde_json::json!({
                "query": query,
                "params": serialized_params
            }),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                let rows: Vec<ProxyDatabaseRow> = serde_json::from_value(value).map_err(|e| {
                    DatabaseError::Serialization(format!("Failed to deserialize rows: {}", e))
                })?;

                Ok(rows
                    .into_iter()
                    .map(|r| Box::new(r) as Box<dyn DatabaseRow>)
                    .collect())
            }
            None => Err(DatabaseError::Query("No result from operation".to_string())),
        }
    }

    async fn query_one(
        &self,
        query: &str,
        params: &[DatabaseValue],
    ) -> DatabaseResult<Option<Box<dyn DatabaseRow>>> {
        let serialized_params =
            serialize_params(params).map_err(|e| DatabaseError::Serialization(e.to_string()))?;

        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Database,
            operation: "query_one".to_string(),
            params: serde_json::json!({
                "query": query,
                "params": serialized_params
            }),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                if value.is_null() {
                    return Ok(None);
                }

                let row: ProxyDatabaseRow = serde_json::from_value(value).map_err(|e| {
                    DatabaseError::Serialization(format!("Failed to deserialize row: {}", e))
                })?;

                Ok(Some(Box::new(row) as Box<dyn DatabaseRow>))
            }
            None => Ok(None),
        }
    }

    async fn begin_transaction(&self) -> DatabaseResult<Box<dyn DatabaseTransaction>> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Database,
            operation: "begin_transaction".to_string(),
            params: serde_json::json!({}),
        };

        let result = send_operation(operation).await?;
        match result.result {
            Some(value) => {
                let transaction_id = value
                    .as_str()
                    .ok_or_else(|| {
                        DatabaseError::Transaction("Invalid transaction ID".to_string())
                    })?
                    .to_string();

                // Store the transaction ID
                let mut transactions = self.active_transactions.lock().await;
                transactions.push(transaction_id.clone());

                Ok(Box::new(ProxyDatabaseTransaction {
                    connection_id: self.connection_id.clone(),
                    transaction_id,
                }) as Box<dyn DatabaseTransaction>)
            }
            None => Err(DatabaseError::Transaction(
                "Failed to begin transaction".to_string(),
            )),
        }
    }

    fn get_database_type(&self) -> DatabaseType {
        self.db_type
    }

    async fn ping(&self) -> DatabaseResult<()> {
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Database,
            operation: "ping".to_string(),
            params: serde_json::json!({}),
        };

        let result = send_operation(operation).await?;
        if result.success {
            Ok(())
        } else {
            Err(DatabaseError::Connection(
                result.error.unwrap_or_else(|| "Ping failed".to_string()),
            ))
        }
    }

    async fn close(&self) -> DatabaseResult<()> {
        // Roll back any active transactions first
        let mut transactions = self.active_transactions.lock().await;
        let transaction_ids = std::mem::take(&mut *transactions);
        drop(transactions);

        for transaction_id in transaction_ids {
            let operation = ServiceOperation {
                connection_id: self.connection_id.clone(),
                service_type: ServiceType::Database,
                operation: "transaction_rollback".to_string(),
                params: serde_json::json!({
                    "transaction_id": transaction_id,
                }),
            };

            // Just try to roll back, but ignore any errors
            let _ = send_operation(operation).await;
        }

        // Now close the connection
        let operation = ServiceOperation {
            connection_id: self.connection_id.clone(),
            service_type: ServiceType::Database,
            operation: "close".to_string(),
            params: serde_json::json!({}),
        };

        let result = send_operation(operation).await?;
        if result.success {
            Ok(())
        } else {
            Err(DatabaseError::Connection(
                result.error.unwrap_or_else(|| "Close failed".to_string()),
            ))
        }
    }
}

// Utility function to serialize database parameters
pub fn serialize_params(params: &[DatabaseValue]) -> Result<serde_json::Value, serde_json::Error> {
    let serialized_results: Vec<Result<serde_json::Value, serde_json::Error>> = params
        .iter()
        .map(|value| -> Result<serde_json::Value, serde_json::Error> {
            match value {
                DatabaseValue::Null => Ok(serde_json::Value::Null),
                DatabaseValue::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
                DatabaseValue::Integer(i) => Ok(serde_json::Value::Number((*i).into())),
                DatabaseValue::Float(f) => serde_json::Number::from_f64(*f)
                    .map(serde_json::Value::Number)
                    .ok_or_else(|| {
                        serde_json::Error::custom(format!("Invalid float value: {}", f))
                    }),
                DatabaseValue::Text(s) => Ok(serde_json::Value::String(s.clone())),
                DatabaseValue::Blob(b) => Ok(serde_json::Value::String(STANDARD.encode(b))),
                DatabaseValue::Array(arr) => {
                    let values_vec_result: Result<Vec<serde_json::Value>, serde_json::Error> = arr
                        .iter()
                        .map(|v_item| match v_item {
                            DatabaseValue::Null => Ok(serde_json::Value::Null),
                            DatabaseValue::Boolean(b_inner) => {
                                Ok(serde_json::Value::Bool(*b_inner))
                            }
                            DatabaseValue::Integer(i_inner) => {
                                Ok(serde_json::Value::Number((*i_inner).into()))
                            }
                            DatabaseValue::Float(f_inner) => serde_json::Number::from_f64(*f_inner)
                                .map(serde_json::Value::Number)
                                .ok_or_else(|| {
                                    serde_json::Error::custom(format!(
                                        "Invalid float value in array: {}",
                                        f_inner
                                    ))
                                }),
                            DatabaseValue::Text(s_inner) => {
                                Ok(serde_json::Value::String(s_inner.clone()))
                            }
                            DatabaseValue::Blob(b_inner) => {
                                Ok(serde_json::Value::String(STANDARD.encode(b_inner)))
                            }
                            DatabaseValue::Array(_) => Err(serde_json::Error::custom(
                                "Nested arrays not supported for direct IPC serialization",
                            )),
                        })
                        .collect();

                    Ok(serde_json::Value::Array(values_vec_result?))
                }
            }
        })
        .collect();

    let final_serialized_values: Result<Vec<serde_json::Value>, serde_json::Error> =
        serialized_results.into_iter().collect();

    Ok(serde_json::Value::Array(final_serialized_values?))
}

// Helper function to send an operation and receive the result
async fn send_operation(operation: ServiceOperation) -> DatabaseResult<ServiceOperationResult> {
    let response = send_request(&operation)
        .await
        .map_err(|e| DatabaseError::Query(format!("Failed to send operation: {}", e)))?;

    let result: ServiceOperationResult = serde_json::from_str(&response)
        .map_err(|e| DatabaseError::Query(format!("Failed to parse response: {}", e)))?;

    if !result.success {
        let error_msg = result.error.unwrap_or_else(|| "Unknown error".to_string());
        return Err(DatabaseError::Query(error_msg));
    }

    Ok(result)
}

// Add IPC error to the DatabaseError enum
impl From<String> for DatabaseError {
    fn from(error: String) -> Self {
        DatabaseError::Connection(format!("IPC error: {}", error))
    }
}
