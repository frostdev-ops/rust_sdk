use crate::database::DatabaseType;
use crate::model_manager::adapters::{
    DatabaseAdapter, MySqlAdapter, PostgresAdapter, SqliteAdapter,
};
use crate::model_manager::{Error, Result};

/// Configuration for the model manager, selecting the appropriate database adapter.
#[derive(Debug, Clone)]
pub struct ModelManagerConfig {
    /// The type of database for SQL generation.
    pub database_type: DatabaseType,
}

impl ModelManagerConfig {
    /// Create a new `ModelManagerConfig` with the given database type.
    pub fn new(database_type: DatabaseType) -> Self {
        Self { database_type }
    }

    /// Get the database adapter based on the configured database type.
    pub fn get_adapter(&self) -> Result<Box<dyn DatabaseAdapter<Error = Error>>> {
        match self.database_type {
            DatabaseType::Sqlite => Ok(Box::new(SqliteAdapter::new())),
            DatabaseType::MySql => Ok(Box::new(MySqlAdapter::new())),
            DatabaseType::Postgres => Ok(Box::new(PostgresAdapter::new())),
        }
    }
}
