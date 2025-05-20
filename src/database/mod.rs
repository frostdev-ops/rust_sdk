use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};
#[allow(unused_imports)]
use std::sync::Arc;
use thiserror::Error;

/// Error type for database operations
#[derive(Debug, Error)]
pub enum DatabaseError {
    /// Connection error
    #[error("connection error: {0}")]
    Connection(String),

    /// Query error
    #[error("query error: {0}")]
    Query(String),

    /// Transaction error
    #[error("transaction error: {0}")]
    Transaction(String),

    /// Pool error
    #[error("pool error: {0}")]
    Pool(String),

    /// Configuration error
    #[error("configuration error: {0}")]
    Configuration(String),

    /// Migration error
    #[error("migration error: {0}")]
    Migration(String),

    /// Serialization/deserialization error
    #[error("serialization error: {0}")]
    Serialization(String),
}

/// Result type for database operations
pub type DatabaseResult<T> = Result<T, DatabaseError>;

/// Supported database types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseType {
    /// PostgreSQL database
    Postgres,
    /// MySQL database
    MySql,
    /// SQLite database
    Sqlite,
}

impl Display for DatabaseType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatabaseType::Postgres => write!(f, "postgres"),
            DatabaseType::MySql => write!(f, "mysql"),
            DatabaseType::Sqlite => write!(f, "sqlite"),
        }
    }
}

/// Database configuration for establishing connections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Type of database to connect to
    pub db_type: DatabaseType,

    /// Database host (for Postgres/MySQL)
    #[serde(default)]
    pub host: Option<String>,

    /// Database port (for Postgres/MySQL)
    #[serde(default)]
    pub port: Option<u16>,

    /// Database name (for Postgres/MySQL) or file path (for SQLite)
    pub database: String,

    /// Database username (for Postgres/MySQL)
    #[serde(default)]
    pub username: Option<String>,

    /// Database password (for Postgres/MySQL)
    #[serde(default)]
    pub password: Option<String>,

    /// SSL mode (for Postgres)
    #[serde(default)]
    pub ssl_mode: Option<String>,

    /// Connection pool settings
    #[serde(default)]
    pub pool: PoolConfig,

    /// Additional connection parameters as key-value pairs
    #[serde(default)]
    pub extra_params: std::collections::HashMap<String, String>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        // Default to SQLite in-memory database
        Self {
            db_type: DatabaseType::Sqlite,
            host: None,
            port: None,
            database: ":memory:".to_string(),
            username: None,
            password: None,
            ssl_mode: None,
            pool: PoolConfig::default(),
            extra_params: std::collections::HashMap::new(),
        }
    }
}

/// Configuration for connection pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Maximum number of connections in the pool
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,

    /// Minimum number of connections to maintain
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,

    /// Connection idle timeout
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_seconds: u64,

    /// Connection max lifetime
    #[serde(default = "default_max_lifetime")]
    pub max_lifetime_seconds: u64,

    /// Connection acquisition timeout
    #[serde(default = "default_acquire_timeout")]
    pub acquire_timeout_seconds: u64,
}

fn default_max_connections() -> u32 {
    10
}
fn default_min_connections() -> u32 {
    2
}
fn default_idle_timeout() -> u64 {
    300
} // 5 minutes
fn default_max_lifetime() -> u64 {
    1800
} // 30 minutes
fn default_acquire_timeout() -> u64 {
    30
}

impl Default for PoolConfig {
    fn default() -> Self {
        PoolConfig {
            max_connections: default_max_connections(),
            min_connections: default_min_connections(),
            idle_timeout_seconds: default_idle_timeout(),
            max_lifetime_seconds: default_max_lifetime(),
            acquire_timeout_seconds: default_acquire_timeout(),
        }
    }
}

/// Represents a row from a database query
pub trait DatabaseRow: Send + Sync {
    /// Get a column value by name
    fn get_string(&self, column: &str) -> DatabaseResult<String>;
    fn get_i64(&self, column: &str) -> DatabaseResult<i64>;
    fn get_f64(&self, column: &str) -> DatabaseResult<f64>;
    fn get_bool(&self, column: &str) -> DatabaseResult<bool>;
    fn get_bytes(&self, column: &str) -> DatabaseResult<Vec<u8>>;

    /// Try to get a column value by name, returning None if the column doesn't exist or is NULL
    fn try_get_string(&self, column: &str) -> DatabaseResult<Option<String>>;
    fn try_get_i64(&self, column: &str) -> DatabaseResult<Option<i64>>;
    fn try_get_f64(&self, column: &str) -> DatabaseResult<Option<f64>>;
    fn try_get_bool(&self, column: &str) -> DatabaseResult<Option<bool>>;
    fn try_get_bytes(&self, column: &str) -> DatabaseResult<Option<Vec<u8>>>;
}

/// Core database connection interface
#[async_trait]
pub trait DatabaseConnection: Send + Sync {
    /// Execute a query that returns no rows
    async fn execute(&self, query: &str, params: &[DatabaseValue]) -> DatabaseResult<u64>;

    /// Execute a query that returns rows
    async fn query(
        &self,
        query: &str,
        params: &[DatabaseValue],
    ) -> DatabaseResult<Vec<Box<dyn DatabaseRow>>>;

    /// Execute a query that returns a single row
    async fn query_one(
        &self,
        query: &str,
        params: &[DatabaseValue],
    ) -> DatabaseResult<Option<Box<dyn DatabaseRow>>>;

    /// Begin a transaction
    async fn begin_transaction(&self) -> DatabaseResult<Box<dyn DatabaseTransaction>>;

    /// Get the underlying database type
    fn get_database_type(&self) -> DatabaseType;

    /// Check if the connection is alive
    async fn ping(&self) -> DatabaseResult<()>;

    /// Close the connection
    async fn close(&self) -> DatabaseResult<()>;
}

/// Database transaction interface
#[async_trait]
pub trait DatabaseTransaction: Send + Sync {
    /// Execute a query within the transaction that returns no rows
    async fn execute(&mut self, query: &str, params: &[DatabaseValue]) -> DatabaseResult<u64>;

    /// Execute a query within the transaction that returns rows
    async fn query(
        &mut self,
        query: &str,
        params: &[DatabaseValue],
    ) -> DatabaseResult<Vec<Box<dyn DatabaseRow>>>;

    /// Execute a query within the transaction that returns a single row
    async fn query_one(
        &mut self,
        query: &str,
        params: &[DatabaseValue],
    ) -> DatabaseResult<Option<Box<dyn DatabaseRow>>>;

    /// Commit the transaction
    async fn commit(self: Box<Self>) -> DatabaseResult<()>;

    /// Rollback the transaction
    async fn rollback(self: Box<Self>) -> DatabaseResult<()>;
}

/// Represents a parameter value for database queries
#[derive(Debug, Clone)]
pub enum DatabaseValue {
    /// Null value
    Null,
    /// Boolean value
    Boolean(bool),
    /// Integer value
    Integer(i64),
    /// Floating point value
    Float(f64),
    /// String value
    Text(String),
    /// Binary data
    Blob(Vec<u8>),
    /// Array of values
    Array(Vec<DatabaseValue>),
    // Additional types can be added as needed
}

// Mock implementation for testing model_manager
#[cfg(all(feature = "integration_tests", feature = "database"))]
mod mock {
    use super::*;

    // Mock transaction for tests
    pub struct MockTransaction {
        pub connection: Arc<dyn DatabaseConnection>,
    }

    #[async_trait]
    impl DatabaseTransaction for MockTransaction {
        async fn execute(&mut self, query: &str, params: &[DatabaseValue]) -> DatabaseResult<u64> {
            self.connection.execute(query, params).await
        }

        async fn query(
            &mut self,
            query: &str,
            params: &[DatabaseValue],
        ) -> DatabaseResult<Vec<Box<dyn DatabaseRow>>> {
            self.connection.query(query, params).await
        }

        async fn query_one(
            &mut self,
            query: &str,
            params: &[DatabaseValue],
        ) -> DatabaseResult<Option<Box<dyn DatabaseRow>>> {
            self.connection.query_one(query, params).await
        }

        async fn commit(self: Box<Self>) -> DatabaseResult<()> {
            // Mock commit just succeeds
            Ok(())
        }

        async fn rollback(self: Box<Self>) -> DatabaseResult<()> {
            // Mock rollback just succeeds
            Ok(())
        }
    }

    // Mock connection wrapper that delegates all calls to the inner connection
    pub struct MockConnection {
        inner: Arc<dyn DatabaseConnection>,
    }

    #[async_trait]
    impl DatabaseConnection for MockConnection {
        async fn execute(&self, query: &str, params: &[DatabaseValue]) -> DatabaseResult<u64> {
            self.inner.execute(query, params).await
        }

        async fn query(
            &self,
            query: &str,
            params: &[DatabaseValue],
        ) -> DatabaseResult<Vec<Box<dyn DatabaseRow>>> {
            self.inner.query(query, params).await
        }

        async fn query_one(
            &self,
            query: &str,
            params: &[DatabaseValue],
        ) -> DatabaseResult<Option<Box<dyn DatabaseRow>>> {
            self.inner.query_one(query, params).await
        }

        async fn begin_transaction(&self) -> DatabaseResult<Box<dyn DatabaseTransaction>> {
            // Create a mock transaction that uses this connection
            Ok(Box::new(MockTransaction {
                connection: Arc::clone(&self.inner),
            }))
        }

        fn get_database_type(&self) -> DatabaseType {
            self.inner.get_database_type()
        }

        async fn ping(&self) -> DatabaseResult<()> {
            self.inner.ping().await
        }

        async fn close(&self) -> DatabaseResult<()> {
            self.inner.close().await
        }
    }

    // Function to wrap a connection in a mock for tests
    pub fn create_mock_db_connection(
        conn: Box<dyn DatabaseConnection>,
    ) -> Box<dyn DatabaseConnection> {
        Box::new(MockConnection { inner: Arc::from(conn) })
    }
}

// Then modify the create_database_connection function to use the mock for tests
pub async fn create_database_connection(
    config: &DatabaseConfig,
) -> DatabaseResult<Box<dyn DatabaseConnection>> {
    // First check if we're running as a module (under orchestrator)
    if is_running_as_module() {
        #[cfg(feature = "ipc")]
        {
            // Create an IPC-based connection
            let conn = proxy_connection::ProxyDatabaseConnection::connect(config).await?;
            return Ok(Box::new(conn) as Box<dyn DatabaseConnection>);
        }
        #[cfg(not(feature = "ipc"))]
        {
            return Err(DatabaseError::Configuration(
                "IPC support is not enabled but running as a module. Enable the 'ipc' feature."
                    .to_string(),
            ));
        }
    }

    // If not running as a module, use direct connections
    let connection = match config.db_type {
        DatabaseType::Postgres => {
            #[cfg(feature = "postgres")]
            {
                let conn = postgres::PostgresConnection::connect(config).await?;
                Box::new(conn) as Box<dyn DatabaseConnection>
            }
            #[cfg(not(feature = "postgres"))]
            {
                return Err(DatabaseError::Configuration(
                    "PostgreSQL support is not enabled. Enable the 'postgres' feature.".to_string(),
                ));
            }
        }
        DatabaseType::MySql => {
            #[cfg(feature = "mysql")]
            {
                let conn = mysql::MySqlConnection::connect(config).await?;
                Box::new(conn) as Box<dyn DatabaseConnection>
            }
            #[cfg(not(feature = "mysql"))]
            {
                return Err(DatabaseError::Configuration(
                    "MySQL support is not enabled. Enable the 'mysql' feature.".to_string(),
                ));
            }
        }
        DatabaseType::Sqlite => {
            #[cfg(feature = "sqlite")]
            {
                let conn = sqlite::SqliteConnection::connect(config).await?;
                Box::new(conn) as Box<dyn DatabaseConnection>
            }
            #[cfg(not(feature = "sqlite"))]
            {
                return Err(DatabaseError::Configuration(
                    "SQLite support is not enabled. Enable the 'sqlite' feature.".to_string(),
                ));
            }
        }
    };

    // For integration tests, wrap the connection in a mock
    #[cfg(all(feature = "integration_tests", feature = "database"))]
    {
        Ok(mock::create_mock_db_connection(connection))
    }

    #[cfg(not(all(feature = "integration_tests", feature = "database")))]
    {
        Ok(connection)
    }
}

// Helper function to check if running as a module
fn is_running_as_module() -> bool {
    std::env::var("PYWATT_MODULE_ID").is_ok()
}

// Implementation modules for specific database types
#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "postgres")]
pub use postgres::PostgresConnection;

#[cfg(feature = "mysql")]
mod mysql;
#[cfg(feature = "mysql")]
pub use mysql::MySqlConnection;

#[cfg(feature = "sqlite")]
mod sqlite;
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteConnection;

#[cfg(feature = "ipc")]
pub mod proxy_connection;
#[cfg(feature = "ipc")]
pub use proxy_connection::ProxyDatabaseConnection;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_type_display() {
        assert_eq!(DatabaseType::Postgres.to_string(), "postgres");
        assert_eq!(DatabaseType::MySql.to_string(), "mysql");
        assert_eq!(DatabaseType::Sqlite.to_string(), "sqlite");
    }

    #[test]
    fn test_pool_config_defaults() {
        let config = PoolConfig::default();
        assert_eq!(config.max_connections, 10);
        assert_eq!(config.min_connections, 2);
        assert_eq!(config.idle_timeout_seconds, 300);
        assert_eq!(config.max_lifetime_seconds, 1800);
        assert_eq!(config.acquire_timeout_seconds, 30);
    }
}

/// Utility function to get a database connection from AppState
///
/// This function should be used within request handlers to access the
/// database connection stored in the Axum Extension layer.
///
/// # Example
/// ```rust
/// # use axum::Extension;
/// # use axum::response::IntoResponse;
/// # use std::sync::Arc;
/// # use pywatt_sdk::AppState;
/// # use pywatt_sdk::database::DatabaseConnection;
/// async fn get_users(
///     Extension(state): Extension<Arc<AppState<()>>>,
///     Extension(db): Extension<Arc<Box<dyn DatabaseConnection>>>,
/// ) -> impl IntoResponse {
///     # Ok::<String, axum::http::StatusCode>("".to_string())
///     // let rows = db.query("SELECT id, name FROM users", &[]).await?;
///     // Process rows...
/// }
/// ```
pub mod extensions {
    use super::DatabaseConfig;

    /// Create a PostgreSQL connection configuration
    pub fn postgres_config(
        host: impl Into<String>,
        port: u16,
        database: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> DatabaseConfig {
        let mut config = DatabaseConfig {
            db_type: crate::database::DatabaseType::Postgres,
            host: Some(host.into()),
            port: Some(port),
            database: database.into(),
            username: Some(username.into()),
            password: Some(password.into()),
            ssl_mode: Some("prefer".to_string()),
            ..Default::default()
        };

        // Add default parameter for PostgreSQL
        config
            .extra_params
            .insert("application_name".to_string(), "pywatt_sdk".to_string());

        config
    }

    /// Create a MySQL connection configuration
    pub fn mysql_config(
        host: impl Into<String>,
        port: u16,
        database: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> DatabaseConfig {
        let mut config = DatabaseConfig {
            db_type: crate::database::DatabaseType::MySql,
            host: Some(host.into()),
            port: Some(port),
            database: database.into(),
            username: Some(username.into()),
            password: Some(password.into()),
            ..Default::default()
        };

        // Add default parameters for MySQL
        config
            .extra_params
            .insert("charset".to_string(), "utf8mb4".to_string());
        config
            .extra_params
            .insert("collation".to_string(), "utf8mb4_unicode_ci".to_string());

        config
    }

    /// Create a SQLite connection configuration
    pub fn sqlite_config(database_path: impl Into<String>) -> DatabaseConfig {
        DatabaseConfig {
            db_type: crate::database::DatabaseType::Sqlite,
            database: database_path.into(),
            ..Default::default()
        }
    }
}
