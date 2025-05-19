use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json;
use sqlx::{PgPool, Postgres, Row, postgres::PgPoolOptions};
use std::sync::Arc;
use std::time::Duration;

use crate::database::{
    DatabaseConfig, DatabaseConnection, DatabaseError, DatabaseResult, DatabaseRow,
    DatabaseTransaction, DatabaseType, DatabaseValue,
};

/// PostgreSQL implementation of the database connection interface
pub struct PostgresConnection {
    pool: Arc<PgPool>,
}

impl PostgresConnection {
    /// Create a new PostgreSQL connection from a configuration
    pub async fn connect(config: &DatabaseConfig) -> DatabaseResult<Self> {
        let database_url = build_postgres_connection_string(config)?;

        let pool_options = PgPoolOptions::new()
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

/// Convert a DatabaseConfig to a PostgreSQL connection string
fn build_postgres_connection_string(config: &DatabaseConfig) -> DatabaseResult<String> {
    let host = config.host.as_deref().unwrap_or("localhost");
    let port = config.port.unwrap_or(5432);
    let database = &config.database;

    let mut connection_string = format!("postgres://");

    if let Some(username) = &config.username {
        connection_string.push_str(&format!("{}", username));

        if let Some(password) = &config.password {
            connection_string.push_str(&format!(":{}", password));
        }

        connection_string.push_str(&format!("@"));
    }

    connection_string.push_str(&format!("{}:{}/{}", host, port, database));

    // Add SSL mode if specified
    if let Some(ssl_mode) = &config.ssl_mode {
        connection_string.push_str(&format!("?sslmode={}", ssl_mode));
    }

    // Add any extra parameters
    for (key, value) in &config.extra_params {
        if connection_string.contains('?') {
            connection_string.push_str(&format!("&{}={}", key, value));
        } else {
            connection_string.push_str(&format!("?{}={}", key, value));
        }
    }

    Ok(connection_string)
}

/// Converts DatabaseValue to sqlx::postgres::PgArguments
#[allow(dead_code)]
fn convert_params(params: &[DatabaseValue]) -> Result<Vec<serde_json::Value>, serde_json::Error> {
    // Instead of trying to use non-existent PgValue variants,
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

/// PostgreSQL implementation of the database row interface
pub struct PostgresRow {
    row: sqlx::postgres::PgRow,
}

impl DatabaseRow for PostgresRow {
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

/// PostgreSQL implementation of the database transaction interface
pub struct PostgresTransaction {
    transaction: sqlx::Transaction<'static, Postgres>,
}

// Declare PostgresTransaction as Send + Sync
// This is safe because we properly manage access to transaction
unsafe impl Send for PostgresTransaction {}
unsafe impl Sync for PostgresTransaction {}

#[async_trait]
impl DatabaseTransaction for PostgresTransaction {
    async fn execute(&mut self, query: &str, params: &[DatabaseValue]) -> DatabaseResult<u64> {
        let mut query_builder = sqlx::query(query);

        // Bind parameters sequentially
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
                DatabaseValue::Array(arr) => {
                    // For arrays, convert to a String representation
                    let text_rep = format!("{:?}", arr);
                    query_builder = query_builder.bind(text_rep);
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

        // Bind parameters sequentially
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
                DatabaseValue::Array(arr) => {
                    // For arrays, convert to a String representation
                    let text_rep = format!("{:?}", arr);
                    query_builder = query_builder.bind(text_rep);
                }
            }
        }

        let rows = query_builder
            .fetch_all(self.transaction.as_mut())
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|row| Box::new(PostgresRow { row }) as Box<dyn DatabaseRow>)
            .collect())
    }

    async fn query_one(
        &mut self,
        query: &str,
        params: &[DatabaseValue],
    ) -> DatabaseResult<Option<Box<dyn DatabaseRow>>> {
        let mut query_builder = sqlx::query(query);

        // Bind parameters sequentially
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
                DatabaseValue::Array(arr) => {
                    // For arrays, convert to a String representation
                    let text_rep = format!("{:?}", arr);
                    query_builder = query_builder.bind(text_rep);
                }
            }
        }

        let row = query_builder
            .fetch_optional(self.transaction.as_mut())
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(row.map(|r| Box::new(PostgresRow { row: r }) as Box<dyn DatabaseRow>))
    }

    async fn commit(self: Box<Self>) -> DatabaseResult<()> {
        self.transaction
            .commit()
            .await
            .map_err(|e| DatabaseError::Transaction(format!("Failed to commit transaction: {}", e)))
    }

    async fn rollback(self: Box<Self>) -> DatabaseResult<()> {
        self.transaction.rollback().await.map_err(|e| {
            DatabaseError::Transaction(format!("Failed to rollback transaction: {}", e))
        })
    }
}

#[async_trait]
impl DatabaseConnection for PostgresConnection {
    async fn execute(&self, query: &str, params: &[DatabaseValue]) -> DatabaseResult<u64> {
        let mut query_builder = sqlx::query(query);

        // Bind parameters sequentially
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
                DatabaseValue::Array(arr) => {
                    // For arrays, convert to a String representation
                    let text_rep = format!("{:?}", arr);
                    query_builder = query_builder.bind(text_rep);
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

        // Bind parameters sequentially
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
                DatabaseValue::Array(arr) => {
                    // For arrays, convert to a String representation
                    let text_rep = format!("{:?}", arr);
                    query_builder = query_builder.bind(text_rep);
                }
            }
        }

        query_builder
            .fetch_all(&*self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))
            .map(|rows| {
                rows.into_iter()
                    .map(|row| Box::new(PostgresRow { row }) as Box<dyn DatabaseRow>)
                    .collect()
            })
    }

    async fn query_one(
        &self,
        query: &str,
        params: &[DatabaseValue],
    ) -> DatabaseResult<Option<Box<dyn DatabaseRow>>> {
        let mut query_builder = sqlx::query(query);

        // Bind parameters sequentially
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
                DatabaseValue::Array(arr) => {
                    // For arrays, convert to a String representation
                    let text_rep = format!("{:?}", arr);
                    query_builder = query_builder.bind(text_rep);
                }
            }
        }

        query_builder
            .fetch_optional(&*self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))
            .map(|opt_row| opt_row.map(|row| Box::new(PostgresRow { row }) as Box<dyn DatabaseRow>))
    }

    async fn begin_transaction(&self) -> DatabaseResult<Box<dyn DatabaseTransaction>> {
        let transaction = self.pool.begin().await.map_err(|e| {
            DatabaseError::Transaction(format!("Failed to begin transaction: {}", e))
        })?;

        Ok(Box::new(PostgresTransaction { transaction }) as Box<dyn DatabaseTransaction>)
    }

    fn get_database_type(&self) -> DatabaseType {
        DatabaseType::Postgres
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

// Add a Display implementation for DatabaseValue to fix the format! error
impl std::fmt::Display for super::DatabaseValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => write!(f, "NULL"),
            Self::Boolean(b) => write!(f, "{}", b),
            Self::Integer(i) => write!(f, "{}", i),
            Self::Float(fl) => write!(f, "{}", fl),
            Self::Text(s) => write!(f, "{}", s),
            Self::Blob(b) => write!(f, "<blob of {} bytes>", b.len()),
            Self::Array(a) => {
                write!(f, "[")?;
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
        }
    }
}

#[cfg(not(feature = "integration_tests"))]
#[allow(dead_code)]
struct DummyItem;
