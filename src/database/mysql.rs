use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json;
use sqlx::{MySql, MySqlPool, Row, mysql::MySqlPoolOptions};
use std::sync::Arc;
use std::time::Duration;

use crate::database::{
    DatabaseConfig, DatabaseConnection, DatabaseError, DatabaseResult, DatabaseRow,
    DatabaseTransaction, DatabaseType, DatabaseValue,
};

/// MySQL implementation of the database connection interface
#[derive(Clone)]
pub struct MySqlConnection {
    pool: Arc<MySqlPool>,
}

impl MySqlConnection {
    /// Create a new MySQL connection from a configuration
    pub async fn connect(config: &DatabaseConfig) -> DatabaseResult<Self> {
        let database_url = build_mysql_connection_string(config)?;

        let pool_options = MySqlPoolOptions::new()
            .max_connections(config.pool.max_connections)
            .min_connections(config.pool.min_connections)
            .idle_timeout(Duration::from_secs(config.pool.idle_timeout_seconds))
            .max_lifetime(Duration::from_secs(config.pool.max_lifetime_seconds))
            .acquire_timeout(Duration::from_secs(config.pool.acquire_timeout_seconds));

        let pool = pool_options
            .connect(&database_url)
            .await
            .map_err(|e| DatabaseError::Connection(e.to_string()))?;

        Ok(Self {
            pool: Arc::new(pool),
        })
    }
}

/// Convert a DatabaseConfig to a MySQL connection string
fn build_mysql_connection_string(config: &DatabaseConfig) -> DatabaseResult<String> {
    let host = config.host.as_deref().unwrap_or("localhost");
    let port = config.port.unwrap_or(3306);
    let database = &config.database;

    let mut connection_string = format!("mysql://");

    if let Some(username) = &config.username {
        connection_string.push_str(&format!("{}", username));

        if let Some(password) = &config.password {
            connection_string.push_str(&format!(":{}", password));
        }

        connection_string.push_str(&format!("@"));
    }

    connection_string.push_str(&format!("{}:{}/{}", host, port, database));

    // Add any extra parameters
    let mut param_added = false;
    for (key, value) in &config.extra_params {
        if !param_added {
            connection_string.push_str("?");
            param_added = true;
        } else {
            connection_string.push_str("&");
        }
        connection_string.push_str(&format!("{}={}", key, value));
    }

    Ok(connection_string)
}

/// Converts DatabaseValue to MySQL-compatible parameters
#[allow(dead_code)]
fn convert_mysql_params(
    params: &[DatabaseValue],
) -> Result<Vec<serde_json::Value>, serde_json::Error> {
    // Instead of trying to use non-existent MySqlValue variants,
    // serialize to JSON which can be bound to parameters
    let mut values = Vec::with_capacity(params.len());
    for param in params {
        match param {
            DatabaseValue::Null => values.push(serde_json::Value::Null),
            DatabaseValue::Boolean(b) => values.push(serde_json::Value::Bool(*b)),
            DatabaseValue::Integer(i) => values.push(serde_json::json!(*i)),
            DatabaseValue::Float(f) => values.push(serde_json::json!(*f)),
            DatabaseValue::Text(s) => values.push(serde_json::Value::String(s.clone())),
            DatabaseValue::Blob(b) => values.push(serde_json::Value::String(STANDARD.encode(b))),
            DatabaseValue::Array(arr) => {
                let array_json: Vec<serde_json::Value> = arr
                    .iter()
                    .map(|v| match v {
                        DatabaseValue::Null => serde_json::Value::Null,
                        DatabaseValue::Boolean(b) => serde_json::Value::Bool(*b),
                        DatabaseValue::Integer(i) => serde_json::json!(*i),
                        DatabaseValue::Float(f) => serde_json::json!(*f),
                        DatabaseValue::Text(s) => serde_json::Value::String(s.clone()),
                        DatabaseValue::Blob(b) => serde_json::Value::String(STANDARD.encode(b)),
                        DatabaseValue::Array(_) => serde_json::Value::Null,
                    })
                    .collect();
                values.push(serde_json::Value::Array(array_json));
            }
        }
    }
    Ok(values)
}

/// MySQL implementation of the database row interface
pub struct MySqlRow {
    row: sqlx::mysql::MySqlRow,
}

impl DatabaseRow for MySqlRow {
    fn get_string(&self, column: &str) -> DatabaseResult<String> {
        self.row.try_get(column).map_err(|e| {
            DatabaseError::Query(format!("Failed to get string column {}: {}", column, e))
        })
    }

    fn get_i64(&self, column: &str) -> DatabaseResult<i64> {
        self.row.try_get(column).map_err(|e| {
            DatabaseError::Query(format!("Failed to get i64 column {}: {}", column, e))
        })
    }

    fn get_f64(&self, column: &str) -> DatabaseResult<f64> {
        self.row.try_get(column).map_err(|e| {
            DatabaseError::Query(format!("Failed to get f64 column {}: {}", column, e))
        })
    }

    fn get_bool(&self, column: &str) -> DatabaseResult<bool> {
        self.row.try_get(column).map_err(|e| {
            DatabaseError::Query(format!("Failed to get bool column {}: {}", column, e))
        })
    }

    fn get_bytes(&self, column: &str) -> DatabaseResult<Vec<u8>> {
        self.row.try_get(column).map_err(|e| {
            DatabaseError::Query(format!("Failed to get bytes column {}: {}", column, e))
        })
    }

    fn try_get_string(&self, column: &str) -> DatabaseResult<Option<String>> {
        match self.row.try_get(column) {
            Ok(value) => Ok(Some(value)),
            Err(sqlx::Error::ColumnNotFound(_)) => Ok(None),
            Err(sqlx::Error::TypeNotFound { .. }) => Ok(None),
            Err(e) => Err(DatabaseError::Query(format!(
                "Failed to get string column {}: {}",
                column, e
            ))),
        }
    }

    fn try_get_i64(&self, column: &str) -> DatabaseResult<Option<i64>> {
        match self.row.try_get(column) {
            Ok(value) => Ok(Some(value)),
            Err(sqlx::Error::ColumnNotFound(_)) => Ok(None),
            Err(sqlx::Error::TypeNotFound { .. }) => Ok(None),
            Err(e) => Err(DatabaseError::Query(format!(
                "Failed to get i64 column {}: {}",
                column, e
            ))),
        }
    }

    fn try_get_f64(&self, column: &str) -> DatabaseResult<Option<f64>> {
        match self.row.try_get(column) {
            Ok(value) => Ok(Some(value)),
            Err(sqlx::Error::ColumnNotFound(_)) => Ok(None),
            Err(sqlx::Error::TypeNotFound { .. }) => Ok(None),
            Err(e) => Err(DatabaseError::Query(format!(
                "Failed to get f64 column {}: {}",
                column, e
            ))),
        }
    }

    fn try_get_bool(&self, column: &str) -> DatabaseResult<Option<bool>> {
        match self.row.try_get(column) {
            Ok(value) => Ok(Some(value)),
            Err(sqlx::Error::ColumnNotFound(_)) => Ok(None),
            Err(sqlx::Error::TypeNotFound { .. }) => Ok(None),
            Err(e) => Err(DatabaseError::Query(format!(
                "Failed to get bool column {}: {}",
                column, e
            ))),
        }
    }

    fn try_get_bytes(&self, column: &str) -> DatabaseResult<Option<Vec<u8>>> {
        match self.row.try_get(column) {
            Ok(value) => Ok(Some(value)),
            Err(sqlx::Error::ColumnNotFound(_)) => Ok(None),
            Err(sqlx::Error::TypeNotFound { .. }) => Ok(None),
            Err(e) => Err(DatabaseError::Query(format!(
                "Failed to get bytes column {}: {}",
                column, e
            ))),
        }
    }
}

/// MySQL implementation of the database transaction
pub struct MySqlTransaction {
    transaction: sqlx::Transaction<'static, MySql>,
}

// Declare MySqlTransaction as Send + Sync
// This is safe because we properly manage access to transaction
unsafe impl Send for MySqlTransaction {}
unsafe impl Sync for MySqlTransaction {}

#[async_trait]
impl DatabaseTransaction for MySqlTransaction {
    async fn execute(&mut self, query: &str, params: &[DatabaseValue]) -> DatabaseResult<u64> {
        let mut query_builder = sqlx::query(query);

        // Add parameters
        for param in params {
            match param {
                DatabaseValue::Null => {
                    query_builder = query_builder.bind(None::<String>);
                }
                DatabaseValue::Boolean(b) => {
                    query_builder = query_builder.bind(*b);
                }
                DatabaseValue::Integer(i) => {
                    query_builder = query_builder.bind(*i);
                }
                DatabaseValue::Float(f) => {
                    query_builder = query_builder.bind(*f);
                }
                DatabaseValue::Text(s) => {
                    let owned_string = s.clone();
                    query_builder = query_builder.bind(owned_string);
                }
                DatabaseValue::Blob(b) => {
                    let owned_bytes = b.clone();
                    query_builder = query_builder.bind(owned_bytes);
                }
                DatabaseValue::Array(_) => {
                    return Err(DatabaseError::Query(
                        "MySQL doesn't support array parameters directly".to_string(),
                    ));
                }
            }
        }

        let result = query_builder
            .execute(self.transaction.as_mut())
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(result.rows_affected())
    }

    async fn query(
        &mut self,
        query: &str,
        params: &[DatabaseValue],
    ) -> DatabaseResult<Vec<Box<dyn DatabaseRow>>> {
        let mut query_builder = sqlx::query(query);

        // Add parameters
        for param in params {
            match param {
                DatabaseValue::Null => {
                    query_builder = query_builder.bind(None::<String>);
                }
                DatabaseValue::Boolean(b) => {
                    query_builder = query_builder.bind(*b);
                }
                DatabaseValue::Integer(i) => {
                    query_builder = query_builder.bind(*i);
                }
                DatabaseValue::Float(f) => {
                    query_builder = query_builder.bind(*f);
                }
                DatabaseValue::Text(s) => {
                    let owned_string = s.clone();
                    query_builder = query_builder.bind(owned_string);
                }
                DatabaseValue::Blob(b) => {
                    let owned_bytes = b.clone();
                    query_builder = query_builder.bind(owned_bytes);
                }
                DatabaseValue::Array(_) => {
                    return Err(DatabaseError::Query(
                        "MySQL doesn't support array parameters directly".to_string(),
                    ));
                }
            }
        }

        let rows = query_builder
            .fetch_all(self.transaction.as_mut())
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|row| Box::new(MySqlRow { row }) as Box<dyn DatabaseRow>)
            .collect())
    }

    async fn query_one(
        &mut self,
        query: &str,
        params: &[DatabaseValue],
    ) -> DatabaseResult<Option<Box<dyn DatabaseRow>>> {
        let mut query_builder = sqlx::query(query);

        // Add parameters
        for param in params {
            match param {
                DatabaseValue::Null => {
                    query_builder = query_builder.bind(None::<String>);
                }
                DatabaseValue::Boolean(b) => {
                    query_builder = query_builder.bind(*b);
                }
                DatabaseValue::Integer(i) => {
                    query_builder = query_builder.bind(*i);
                }
                DatabaseValue::Float(f) => {
                    query_builder = query_builder.bind(*f);
                }
                DatabaseValue::Text(s) => {
                    let owned_string = s.clone();
                    query_builder = query_builder.bind(owned_string);
                }
                DatabaseValue::Blob(b) => {
                    let owned_bytes = b.clone();
                    query_builder = query_builder.bind(owned_bytes);
                }
                DatabaseValue::Array(_) => {
                    return Err(DatabaseError::Query(
                        "MySQL doesn't support array parameters directly".to_string(),
                    ));
                }
            }
        }

        let row = query_builder
            .fetch_optional(self.transaction.as_mut())
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(row.map(|r| Box::new(MySqlRow { row: r }) as Box<dyn DatabaseRow>))
    }

    async fn commit(self: Box<Self>) -> DatabaseResult<()> {
        self.transaction
            .commit()
            .await
            .map_err(|e| DatabaseError::Transaction(e.to_string()))
    }

    async fn rollback(self: Box<Self>) -> DatabaseResult<()> {
        self.transaction
            .rollback()
            .await
            .map_err(|e| DatabaseError::Transaction(e.to_string()))
    }
}

#[async_trait]
impl DatabaseConnection for MySqlConnection {
    async fn execute(&self, query: &str, params: &[DatabaseValue]) -> DatabaseResult<u64> {
        let mut query_builder = sqlx::query(query);

        // Add parameters
        for param in params {
            match param {
                DatabaseValue::Null => {
                    query_builder = query_builder.bind(None::<String>);
                }
                DatabaseValue::Boolean(b) => {
                    query_builder = query_builder.bind(*b);
                }
                DatabaseValue::Integer(i) => {
                    query_builder = query_builder.bind(*i);
                }
                DatabaseValue::Float(f) => {
                    query_builder = query_builder.bind(*f);
                }
                DatabaseValue::Text(s) => {
                    let owned_string = s.clone();
                    query_builder = query_builder.bind(owned_string);
                }
                DatabaseValue::Blob(b) => {
                    let owned_bytes = b.clone();
                    query_builder = query_builder.bind(owned_bytes);
                }
                DatabaseValue::Array(_) => {
                    return Err(DatabaseError::Query(
                        "MySQL doesn't support array parameters directly".to_string(),
                    ));
                }
            }
        }

        query_builder
            .execute(&*self.pool)
            .await
            .map(|r| r.rows_affected())
            .map_err(|e| DatabaseError::Query(e.to_string()))
    }

    async fn query(
        &self,
        query: &str,
        params: &[DatabaseValue],
    ) -> DatabaseResult<Vec<Box<dyn DatabaseRow>>> {
        let mut query_builder = sqlx::query(query);

        // Add parameters
        for param in params {
            match param {
                DatabaseValue::Null => {
                    query_builder = query_builder.bind(None::<String>);
                }
                DatabaseValue::Boolean(b) => {
                    query_builder = query_builder.bind(*b);
                }
                DatabaseValue::Integer(i) => {
                    query_builder = query_builder.bind(*i);
                }
                DatabaseValue::Float(f) => {
                    query_builder = query_builder.bind(*f);
                }
                DatabaseValue::Text(s) => {
                    let owned_string = s.clone();
                    query_builder = query_builder.bind(owned_string);
                }
                DatabaseValue::Blob(b) => {
                    let owned_bytes = b.clone();
                    query_builder = query_builder.bind(owned_bytes);
                }
                DatabaseValue::Array(_) => {
                    return Err(DatabaseError::Query(
                        "MySQL doesn't support array parameters directly".to_string(),
                    ));
                }
            }
        }

        query_builder
            .fetch_all(&*self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))
            .map(|rows| {
                rows.into_iter()
                    .map(|row| Box::new(MySqlRow { row }) as Box<dyn DatabaseRow>)
                    .collect()
            })
    }

    async fn query_one(
        &self,
        query: &str,
        params: &[DatabaseValue],
    ) -> DatabaseResult<Option<Box<dyn DatabaseRow>>> {
        let mut query_builder = sqlx::query(query);

        // Add parameters
        for param in params {
            match param {
                DatabaseValue::Null => {
                    query_builder = query_builder.bind(None::<String>);
                }
                DatabaseValue::Boolean(b) => {
                    query_builder = query_builder.bind(*b);
                }
                DatabaseValue::Integer(i) => {
                    query_builder = query_builder.bind(*i);
                }
                DatabaseValue::Float(f) => {
                    query_builder = query_builder.bind(*f);
                }
                DatabaseValue::Text(s) => {
                    let owned_string = s.clone();
                    query_builder = query_builder.bind(owned_string);
                }
                DatabaseValue::Blob(b) => {
                    let owned_bytes = b.clone();
                    query_builder = query_builder.bind(owned_bytes);
                }
                DatabaseValue::Array(_) => {
                    return Err(DatabaseError::Query(
                        "MySQL doesn't support array parameters directly".to_string(),
                    ));
                }
            }
        }

        query_builder
            .fetch_optional(&*self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))
            .map(|opt_row| opt_row.map(|row| Box::new(MySqlRow { row }) as Box<dyn DatabaseRow>))
    }

    async fn begin_transaction(&self) -> DatabaseResult<Box<dyn DatabaseTransaction>> {
        #[cfg(not(feature = "integration_tests"))]
        {
            let transaction = self.pool.begin().await.map_err(|e| {
                DatabaseError::Transaction(format!("Failed to begin transaction: {}", e))
            })?;

            Ok(Box::new(MySqlTransaction { transaction }) as Box<dyn DatabaseTransaction>)
        }

        #[cfg(feature = "integration_tests")]
        {
            // For integration tests, return a mock transaction that delegates to the connection
            Ok(Box::new(super::mock::MockTransaction {
                connection: Arc::new(self.clone()),
            }) as Box<dyn DatabaseTransaction>)
        }
    }

    fn get_database_type(&self) -> DatabaseType {
        DatabaseType::MySql
    }

    async fn ping(&self) -> DatabaseResult<()> {
        sqlx::query("SELECT 1")
            .execute(&*self.pool)
            .await
            .map_err(|e| DatabaseError::Connection(format!("Failed to ping database: {}", e)))?;

        Ok(())
    }

    async fn close(&self) -> DatabaseResult<()> {
        self.pool.close().await;
        Ok(())
    }
}
