// Allow async functions in traits for this module
#![allow(async_fn_in_trait)]

// Model Manager SDK Integration
// Extends DatabaseConnection with schema management features

#[cfg(feature = "database")]
use crate::database::{DatabaseConnection, DatabaseError, DatabaseResult, DatabaseType};

#[cfg(feature = "database")]
use crate::model_manager::{
    Error as ModelError,
    adapters::{DatabaseAdapter, MySqlAdapter, PostgresAdapter, SqliteAdapter},
    definitions::ModelDescriptor,
    generator::ModelGenerator,
};

// For when database feature is disabled, define placeholder types
#[cfg(not(feature = "database"))]
mod db_placeholders {
    #[allow(dead_code)]
    pub type DatabaseError = String;
    #[allow(dead_code)]
    pub type DatabaseResult<T> = Result<T, DatabaseError>;
    #[allow(dead_code)]
    pub enum DatabaseType {
        Sqlite,
    }
    #[allow(dead_code)]
    pub trait DatabaseConnection: Send + Sync {
        fn get_database_type(&self) -> DatabaseType;
    }
}

/// Extension trait to add model management functionality to DatabaseConnection
#[cfg(feature = "database")]
pub trait ModelManager {
    /// Apply a model (create or alter table) to the database
    /// This method attempts to create enum types, then the table, then indexes.
    /// If the table already exists, it might return an error indicating this.
    /// Other "already exists" errors (for enums, indexes) are generally ignored during creation.
    async fn apply_model(&mut self, model: &ModelDescriptor) -> DatabaseResult<()>;

    /// Drop a model (table) from the database
    async fn drop_model(
        &mut self,
        table_name: &str,
        schema_name: Option<&str>,
    ) -> DatabaseResult<()>;

    /// Synchronize a collection of models with the database (create missing tables/columns)
    async fn sync_schema(&mut self, models: &[ModelDescriptor]) -> DatabaseResult<()>;

    /// [INTERNAL] Helper to get the appropriate database adapter
    fn get_db_adapter_internal(
        &self,
    ) -> Result<Box<dyn DatabaseAdapter<Error = ModelError>>, DatabaseError>;
}

// Stub trait when database feature is disabled
#[cfg(not(feature = "database"))]
pub trait ModelManager {}

/// Convert ModelError to DatabaseError
#[cfg(feature = "database")]
fn convert_error(err: ModelError) -> DatabaseError {
    DatabaseError::Query(format!("Model manager error: {}", err))
}

#[cfg(feature = "database")]
fn split_script(script: &str) -> Vec<String> {
    script
        .split(';')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

#[cfg(feature = "database")]
fn is_table_already_exists_error(db_type: DatabaseType, err_msg: &str) -> bool {
    let lower_msg = err_msg.to_lowercase();

    match db_type {
        DatabaseType::Sqlite => lower_msg.contains("table") && lower_msg.contains("already exists"),
        DatabaseType::Postgres => {
            (lower_msg.contains("relation") && lower_msg.contains("already exists"))
                || (lower_msg.contains("table") && lower_msg.contains("already exists"))
                || lower_msg.contains("duplicate table")
        }
        DatabaseType::MySql => {
            (lower_msg.contains("table") && lower_msg.contains("already exists"))
                || lower_msg.contains("error 1050") // MySQL error code for "table already exists"
        }
    }
}

#[cfg(feature = "database")]
fn is_general_already_exists_error(db_type: DatabaseType, err_msg: &str) -> bool {
    let lower_msg = err_msg.to_lowercase();

    // Common patterns across all databases
    let common_patterns = lower_msg.contains("already exist") || // "already exists", "already existed"
        lower_msg.contains("duplicate"); // "duplicate key", "duplicate column name"

    if common_patterns {
        return true;
    }

    // Database-specific patterns
    match db_type {
        DatabaseType::Sqlite => {
            lower_msg.contains("unique constraint failed") // SQLite constraint violation
        }
        DatabaseType::Postgres => {
            // PostgreSQL specific patterns
            lower_msg.contains("duplicate key")
                || lower_msg.contains("duplicate column")
                || (lower_msg.contains("constraint") && lower_msg.contains("already exist"))
                || (lower_msg.contains("type") && lower_msg.contains("already exist"))
                || (lower_msg.contains("column")
                    && lower_msg.contains("of relation")
                    && lower_msg.contains("already exist"))
                || lower_msg.contains("already exists") // Catch-all for PostgreSQL "already exists"
        }
        DatabaseType::MySql => {
            // MySQL specific error codes
            lower_msg.contains("error 1060") || // Duplicate column name
            lower_msg.contains("error 1061") || // Duplicate key name
            lower_msg.contains("error 1062") || // Duplicate entry for key
            lower_msg.contains("error 1050") || // Table already exists

            // MySQL specific text patterns
            (lower_msg.contains("duplicate") && lower_msg.contains("key")) ||
            (lower_msg.contains("duplicate") && lower_msg.contains("column")) ||
            (lower_msg.contains("duplicate") && lower_msg.contains("entry"))
        }
    }
}

#[cfg(feature = "database")]
fn is_enum_already_exists_error(db_type: DatabaseType, err_msg: &str) -> bool {
    let lower_msg = err_msg.to_lowercase();

    match db_type {
        DatabaseType::Postgres => {
            // PostgreSQL specific patterns for enum types
            (lower_msg.contains("type") && lower_msg.contains("already exists"))
                || lower_msg.contains("duplicate type")
        }
        // For MySQL and SQLite, enum types are typically implemented as column constraints
        // So we don't need specific detection for them
        _ => false,
    }
}

#[cfg(feature = "database")]
impl<T: ?Sized + DatabaseConnection> ModelManager for T {
    // Helper to get the adapter instance
    fn get_db_adapter_internal(
        &self,
    ) -> Result<Box<dyn DatabaseAdapter<Error = ModelError>>, DatabaseError> {
        match self.get_database_type() {
            DatabaseType::Sqlite => Ok(Box::new(SqliteAdapter::new())),
            DatabaseType::MySql => Ok(Box::new(MySqlAdapter::new())),
            DatabaseType::Postgres => Ok(Box::new(PostgresAdapter::new())),
        }
    }

    async fn apply_model(&mut self, model: &ModelDescriptor) -> DatabaseResult<()> {
        let adapter = self.get_db_adapter_internal()?;
        let generator = ModelGenerator::new(adapter);
        let db_type = self.get_database_type();

        // 1. Create enum types (idempotently)
        let enum_sqls = generator
            .adapter()
            .generate_enum_types_sql(model)
            .map_err(convert_error)?;
        for enum_sql in enum_sqls {
            if let Err(e) = self.execute(&enum_sql, &[]).await {
                if !is_enum_already_exists_error(db_type, &e.to_string())
                    && !is_general_already_exists_error(db_type, &e.to_string())
                {
                    // If it's not an "already exists" error for the type, propagate
                    return Err(DatabaseError::Query(format!(
                        "Failed to create enum type for model '{}': {}",
                        model.name, e
                    )));
                }
            }
        }

        // 2. Generate and execute CREATE TABLE and CREATE INDEX statements
        let create_script = generator
            .generate_create_table_script(model)
            .map_err(convert_error)?;
        let statements = split_script(&create_script);

        for stmt in statements {
            if let Err(e) = self.execute(&stmt, &[]).await {
                let err_msg = e.to_string();
                // If it's a CREATE TABLE statement that failed because table exists, return specific error
                if stmt.trim_start().to_lowercase().starts_with("create table")
                    && is_table_already_exists_error(db_type, &err_msg)
                {
                    // Return the original error, sync_schema will check for this specific case
                    return Err(e);
                }
                // If it's any other "already exists" error (e.g. for an index), ignore it.
                if !is_general_already_exists_error(db_type, &err_msg) {
                    // For other errors, wrap and return
                    return Err(DatabaseError::Query(format!(
                        "Failed to apply model '{}' (statement: '{}'): {}",
                        model.name, stmt, e
                    )));
                }
            }
        }
        Ok(())
    }

    async fn drop_model(
        &mut self,
        table_name: &str,
        schema_name: Option<&str>,
    ) -> DatabaseResult<()> {
        let adapter = self.get_db_adapter_internal()?;
        let generator = ModelGenerator::new(adapter);

        // Generate SQL for dropping the table
        let sql = generator
            .generate_drop_table_script(table_name, schema_name)
            .map_err(convert_error)?;

        // Execute the SQL
        self.execute(&sql, &[]).await?;

        Ok(())
    }

    async fn sync_schema(&mut self, models: &[ModelDescriptor]) -> DatabaseResult<()> {
        for model_to_apply in models {
            match self.apply_model(model_to_apply).await {
                Ok(_) => { /* Table and associated elements (enums, indexes) created successfully */
                }
                Err(e) => {
                    let db_type = self.get_database_type();
                    // Check if the error from apply_model specifically means the TABLE already exists
                    if is_table_already_exists_error(db_type, &e.to_string()) {
                        // Table exists, now try to apply "additive" migrations
                        let adapter = self.get_db_adapter_internal()?;
                        let generator = ModelGenerator::new(adapter);

                        // Enum types should have been attempted (and ignored if existing) by the apply_model call.
                        // However, if apply_model bailed on CREATE TABLE, enums for *new* columns in a migration
                        // might not have been created if they weren't in the original apply_model's scope.
                        // It's safer to run it again, it's idempotent.
                        let enum_sqls = generator
                            .adapter()
                            .generate_enum_types_sql(model_to_apply)
                            .map_err(convert_error)?;
                        for enum_sql in enum_sqls {
                            if let Err(enum_err) = self.execute(&enum_sql, &[]).await {
                                if !is_enum_already_exists_error(db_type, &enum_err.to_string())
                                    && !is_general_already_exists_error(
                                        db_type,
                                        &enum_err.to_string(),
                                    )
                                {
                                    return Err(DatabaseError::Query(format!(
                                        "Failed to create enum type during migration for model '{}': {}",
                                        model_to_apply.name, enum_err
                                    )));
                                }
                            }
                        }

                        let from_model_for_migration = ModelDescriptor {
                            name: model_to_apply.name.clone(),
                            schema: model_to_apply.schema.clone(),
                            // All other fields are default (empty vectors, None, false)
                            // This ensures migration script only adds things from model_to_apply
                            columns: Vec::new(),     // Explicitly empty
                            primary_key: None,       // Explicitly empty
                            indexes: Vec::new(),     // Explicitly empty
                            constraints: Vec::new(), // Explicitly empty
                            comment: None,
                            engine: None,
                            charset: None,
                            collation: None,
                            options: Default::default(), // Ensure this is HashMap::new() or similar if not defaultable to empty
                        };

                        let migration_script = generator
                            .generate_migration_script(&from_model_for_migration, model_to_apply)
                            .map_err(convert_error)?;
                        let migration_statements = split_script(&migration_script);

                        for stmt in migration_statements {
                            if let Err(mig_err) = self.execute(&stmt, &[]).await {
                                // Ignore "already exists" or "duplicate" errors for columns, constraints, indexes
                                if !is_general_already_exists_error(db_type, &mig_err.to_string()) {
                                    // Provide more context in the error message
                                    return Err(DatabaseError::Query(format!(
                                        "Failed to apply migration for model '{}' (db_type: {}, statement: '{}'): {}",
                                        model_to_apply.name, db_type, stmt, mig_err
                                    )));
                                }
                            }
                        }
                        tracing::info!(
                            "Model '{}' synchronized (table existed, additive migration applied).",
                            model_to_apply.name
                        );
                    } else {
                        // apply_model failed for a reason other than table already existing
                        return Err(DatabaseError::Query(format!(
                            "Failed to synchronize model '{}': {}",
                            model_to_apply.name, e
                        )));
                    }
                }
            }
        }
        Ok(())
    }
}
