use async_trait::async_trait;
#[cfg(not(feature = "integration_tests"))]
use sqlx::Sqlite;
use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::database::{
    DatabaseConfig, DatabaseConnection, DatabaseError, DatabaseResult, DatabaseRow,
    DatabaseTransaction, DatabaseType, DatabaseValue,
};

/// SQLite implementation of the database connection interface
pub struct SqliteConnection {
    pool: Arc<SqlitePool>,
}

impl SqliteConnection {
    /// Create a new SQLite connection from a configuration
    pub async fn connect(config: &DatabaseConfig) -> DatabaseResult<Self> {
        let database_url = build_sqlite_connection_string(config)?;

        // Ensure the directory exists if file-based
        if config.database != ":memory:" && !config.database.starts_with("file:") {
            if let Some(parent) = Path::new(&config.database).parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    DatabaseError::Connection(format!(
                        "Failed to create directory for SQLite database: {}",
                        e
                    ))
                })?;
            }
        }

        let pool_options = SqlitePoolOptions::new()
            .max_connections(config.pool.max_connections)
            .min_connections(config.pool.min_connections)
            .idle_timeout(Duration::from_secs(config.pool.idle_timeout_seconds))
            .max_lifetime(Duration::from_secs(config.pool.max_lifetime_seconds))
            .acquire_timeout(Duration::from_secs(config.pool.acquire_timeout_seconds));

        let pool = pool_options
            .connect(&database_url)
            .await
            .map_err(|e| DatabaseError::Connection(e.to_string()))?;

        // Enable foreign keys by default
        sqlx::query("PRAGMA foreign_keys = ON;")
            .execute(&pool)
            .await
            .map_err(|e| {
                DatabaseError::Configuration(format!("Failed to enable foreign keys: {}", e))
            })?;

        Ok(Self {
            pool: Arc::new(pool),
        })
    }
}

/// Convert a DatabaseConfig to a SQLite connection string
fn build_sqlite_connection_string(config: &DatabaseConfig) -> DatabaseResult<String> {
    let database = &config.database;

    // Create a connection string
    // In SQLite, the database is a file path or a special name like ":memory:"
    let mut connection_string = if database == ":memory:" {
        // SQLite in-memory database needs special syntax
        "sqlite::memory:".to_string()
    } else {
        // Normal file paths
        format!("sqlite:{}", database)
    };

    // Add any extra parameters
    let mut param_added = false;
    for (key, value) in &config.extra_params {
        if !param_added {
            connection_string.push('?');
            param_added = true;
        } else {
            connection_string.push('&');
        }
        connection_string.push_str(&format!("{}={}", key, value));
    }

    Ok(connection_string)
}

// Note: SQLite adapter doesn't use parameter binding like this anymore
// The function is kept for potential future implementation
// The current implementation directly uses the query methods without parameters
fn _convert_params(_params: &[DatabaseValue]) -> Vec<sqlx::sqlite::SqliteArguments<'_>> {
    // This function is not used anymore as newer SQLite versions
    // handle parameters differently
    vec![]
}

/// SQLite implementation of the database row interface
pub struct SqliteRow {
    row: sqlx::sqlite::SqliteRow,
}

impl DatabaseRow for SqliteRow {
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

// Define separate transaction implementations for regular use and integration tests
#[cfg(not(feature = "integration_tests"))]
mod transaction_impl {
    use super::*;

    /// SQLite implementation of the database transaction interface
    pub struct SqliteTransaction {
        // Normal implementation - keep a real SQLite transaction
        pub(crate) transaction: sqlx::Transaction<'static, Sqlite>,
    }

    // Declare SqliteTransaction as Send + Sync
    // This is safe because we properly manage access to transaction
    unsafe impl Send for SqliteTransaction {}
    unsafe impl Sync for SqliteTransaction {}

    #[async_trait]
    impl DatabaseTransaction for SqliteTransaction {
        async fn execute(&mut self, query: &str, params: &[DatabaseValue]) -> DatabaseResult<u64> {
            // Build a query with proper parameter binding
            let mut query_builder = sqlx::query(query);

            // Add parameters
            for param in params {
                match param {
                    DatabaseValue::Null => {
                        query_builder = query_builder.bind(None::<String>);
                    }
                    DatabaseValue::Boolean(b) => {
                        query_builder = query_builder.bind(b);
                    }
                    DatabaseValue::Integer(i) => {
                        query_builder = query_builder.bind(i);
                    }
                    DatabaseValue::Float(f) => {
                        query_builder = query_builder.bind(f);
                    }
                    DatabaseValue::Text(s) => {
                        query_builder = query_builder.bind(s);
                    }
                    DatabaseValue::Blob(b) => {
                        query_builder = query_builder.bind(b.clone());
                    }
                    DatabaseValue::Array(_) => {
                        // SQLite doesn't have array type, return error
                        return Err(DatabaseError::Query(
                            "SQLite doesn't support array parameters".to_string(),
                        ));
                    }
                }
            }

            // Execute with parameters
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
            // Build a query with proper parameter binding
            let mut query_builder = sqlx::query(query);

            // Add parameters
            for param in params {
                match param {
                    DatabaseValue::Null => {
                        query_builder = query_builder.bind(None::<String>);
                    }
                    DatabaseValue::Boolean(b) => {
                        query_builder = query_builder.bind(b);
                    }
                    DatabaseValue::Integer(i) => {
                        query_builder = query_builder.bind(i);
                    }
                    DatabaseValue::Float(f) => {
                        query_builder = query_builder.bind(f);
                    }
                    DatabaseValue::Text(s) => {
                        query_builder = query_builder.bind(s);
                    }
                    DatabaseValue::Blob(b) => {
                        query_builder = query_builder.bind(b.clone());
                    }
                    DatabaseValue::Array(_) => {
                        // SQLite doesn't have array type, return error
                        return Err(DatabaseError::Query(
                            "SQLite doesn't support array parameters".to_string(),
                        ));
                    }
                }
            }

            // Execute with parameters
            let rows = query_builder
                .fetch_all(self.transaction.as_mut())
                .await
                .map_err(|e| DatabaseError::Query(e.to_string()))?;

            Ok(rows
                .into_iter()
                .map(|row| Box::new(SqliteRow { row }) as Box<dyn DatabaseRow>)
                .collect())
        }

        async fn query_one(
            &mut self,
            query: &str,
            params: &[DatabaseValue],
        ) -> DatabaseResult<Option<Box<dyn DatabaseRow>>> {
            // Build a query with proper parameter binding
            let mut query_builder = sqlx::query(query);

            // Add parameters
            for param in params {
                match param {
                    DatabaseValue::Null => {
                        query_builder = query_builder.bind(None::<String>);
                    }
                    DatabaseValue::Boolean(b) => {
                        query_builder = query_builder.bind(b);
                    }
                    DatabaseValue::Integer(i) => {
                        query_builder = query_builder.bind(i);
                    }
                    DatabaseValue::Float(f) => {
                        query_builder = query_builder.bind(f);
                    }
                    DatabaseValue::Text(s) => {
                        query_builder = query_builder.bind(s);
                    }
                    DatabaseValue::Blob(b) => {
                        query_builder = query_builder.bind(b.clone());
                    }
                    DatabaseValue::Array(_) => {
                        // SQLite doesn't have array type, return error
                        return Err(DatabaseError::Query(
                            "SQLite doesn't support array parameters".to_string(),
                        ));
                    }
                }
            }

            // Execute with parameters
            let row = query_builder
                .fetch_optional(self.transaction.as_mut())
                .await
                .map_err(|e| DatabaseError::Query(e.to_string()))?;

            Ok(row.map(|r| Box::new(SqliteRow { row: r }) as Box<dyn DatabaseRow>))
        }

        async fn commit(self: Box<Self>) -> DatabaseResult<()> {
            self.transaction.commit().await.map_err(|e| {
                DatabaseError::Transaction(format!("Failed to commit transaction: {}", e))
            })
        }

        async fn rollback(self: Box<Self>) -> DatabaseResult<()> {
            self.transaction.rollback().await.map_err(|e| {
                DatabaseError::Transaction(format!("Failed to rollback transaction: {}", e))
            })
        }
    }
}

// For integration tests, provide a mock implementation that delegates to the connection
#[cfg(feature = "integration_tests")]
mod transaction_impl {
    use super::*;

    /// Mock SQLite transaction for integration tests
    pub struct SqliteTransaction {
        /// Store the connection to delegate operations to
        pub(crate) connection: Arc<SqliteConnection>,
    }

    #[async_trait]
    impl DatabaseTransaction for SqliteTransaction {
        async fn execute(&mut self, query: &str, params: &[DatabaseValue]) -> DatabaseResult<u64> {
            // Simply delegate to the connection
            self.connection.execute(query, params).await
        }

        async fn query(
            &mut self,
            query: &str,
            params: &[DatabaseValue],
        ) -> DatabaseResult<Vec<Box<dyn DatabaseRow>>> {
            // Simply delegate to the connection
            self.connection.query(query, params).await
        }

        async fn query_one(
            &mut self,
            query: &str,
            params: &[DatabaseValue],
        ) -> DatabaseResult<Option<Box<dyn DatabaseRow>>> {
            // Simply delegate to the connection
            self.connection.query_one(query, params).await
        }

        async fn commit(self: Box<Self>) -> DatabaseResult<()> {
            // No-op for integration tests
            Ok(())
        }

        async fn rollback(self: Box<Self>) -> DatabaseResult<()> {
            // No-op for integration tests
            Ok(())
        }
    }
}

// Re-export the implementation
pub use transaction_impl::SqliteTransaction;

#[async_trait]
impl DatabaseConnection for SqliteConnection {
    async fn execute(&self, query: &str, params: &[DatabaseValue]) -> DatabaseResult<u64> {
        // Build a query with proper parameter binding
        let mut query_builder = sqlx::query(query);

        // Add parameters
        for param in params {
            match param {
                DatabaseValue::Null => {
                    query_builder = query_builder.bind(None::<String>);
                }
                DatabaseValue::Boolean(b) => {
                    query_builder = query_builder.bind(b);
                }
                DatabaseValue::Integer(i) => {
                    query_builder = query_builder.bind(i);
                }
                DatabaseValue::Float(f) => {
                    query_builder = query_builder.bind(f);
                }
                DatabaseValue::Text(s) => {
                    query_builder = query_builder.bind(s);
                }
                DatabaseValue::Blob(b) => {
                    query_builder = query_builder.bind(b.clone());
                }
                DatabaseValue::Array(_) => {
                    // SQLite doesn't have array type, return error
                    return Err(DatabaseError::Query(
                        "SQLite doesn't support array parameters".to_string(),
                    ));
                }
            }
        }

        // Execute with parameters
        let result = query_builder
            .execute(&*self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(result.rows_affected())
    }

    async fn query(
        &self,
        query: &str,
        params: &[DatabaseValue],
    ) -> DatabaseResult<Vec<Box<dyn DatabaseRow>>> {
        // Build a query with proper parameter binding
        let mut query_builder = sqlx::query(query);

        // Add parameters
        for param in params {
            match param {
                DatabaseValue::Null => {
                    query_builder = query_builder.bind(None::<String>);
                }
                DatabaseValue::Boolean(b) => {
                    query_builder = query_builder.bind(b);
                }
                DatabaseValue::Integer(i) => {
                    query_builder = query_builder.bind(i);
                }
                DatabaseValue::Float(f) => {
                    query_builder = query_builder.bind(f);
                }
                DatabaseValue::Text(s) => {
                    query_builder = query_builder.bind(s);
                }
                DatabaseValue::Blob(b) => {
                    query_builder = query_builder.bind(b.clone());
                }
                DatabaseValue::Array(_) => {
                    // SQLite doesn't have array type, return error
                    return Err(DatabaseError::Query(
                        "SQLite doesn't support array parameters".to_string(),
                    ));
                }
            }
        }

        // Execute with parameters
        let rows = query_builder
            .fetch_all(&*self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|row| Box::new(SqliteRow { row }) as Box<dyn DatabaseRow>)
            .collect())
    }

    async fn query_one(
        &self,
        query: &str,
        params: &[DatabaseValue],
    ) -> DatabaseResult<Option<Box<dyn DatabaseRow>>> {
        // Build a query with proper parameter binding
        let mut query_builder = sqlx::query(query);

        // Add parameters
        for param in params {
            match param {
                DatabaseValue::Null => {
                    query_builder = query_builder.bind(None::<String>);
                }
                DatabaseValue::Boolean(b) => {
                    query_builder = query_builder.bind(b);
                }
                DatabaseValue::Integer(i) => {
                    query_builder = query_builder.bind(i);
                }
                DatabaseValue::Float(f) => {
                    query_builder = query_builder.bind(f);
                }
                DatabaseValue::Text(s) => {
                    query_builder = query_builder.bind(s);
                }
                DatabaseValue::Blob(b) => {
                    query_builder = query_builder.bind(b.clone());
                }
                DatabaseValue::Array(_) => {
                    // SQLite doesn't have array type, return error
                    return Err(DatabaseError::Query(
                        "SQLite doesn't support array parameters".to_string(),
                    ));
                }
            }
        }

        // Execute with parameters
        let row = query_builder
            .fetch_optional(&*self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(row.map(|r| Box::new(SqliteRow { row: r }) as Box<dyn DatabaseRow>))
    }

    async fn begin_transaction(&self) -> DatabaseResult<Box<dyn DatabaseTransaction>> {
        #[cfg(not(feature = "integration_tests"))]
        {
            let transaction = self.pool.begin().await.map_err(|e| {
                DatabaseError::Transaction(format!("Failed to begin transaction: {}", e))
            })?;

            Ok(Box::new(SqliteTransaction { transaction }) as Box<dyn DatabaseTransaction>)
        }

        #[cfg(feature = "integration_tests")]
        {
            // For integration tests, return a mock transaction that delegates to the connection
            Ok(Box::new(SqliteTransaction {
                connection: Arc::new(Self {
                    pool: Arc::clone(&self.pool),
                }),
            }) as Box<dyn DatabaseTransaction>)
        }
    }

    fn get_database_type(&self) -> DatabaseType {
        DatabaseType::Sqlite
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
