use crate::model_manager::{
    Result,
    definitions::{ColumnDescriptor, Constraint, DataType, IndexDescriptor, ModelDescriptor},
};
use std::error::Error as StdError;

/// Trait defining database-specific operations for model management
pub trait DatabaseAdapter: Send + Sync {
    /// Associated error type for database adapter operations
    type Error: StdError + Send + Sync + 'static;

    /// Get the name of the database type this adapter supports
    fn get_db_type_name(&self) -> &'static str;

    /// Map a common data type to a database-specific type string
    fn map_common_data_type(&self, common_type: &DataType) -> Result<String>;

    /// Generate SQL for a column definition
    fn generate_column_definition_sql(&self, column: &ColumnDescriptor) -> Result<String>;

    /// Generate SQL for a constraint
    fn generate_constraint_sql(
        &self,
        constraint: &Constraint,
        table_name: &str,
    ) -> Result<Option<String>>;

    /// Generate SQL for a primary key
    fn generate_primary_key_sql(
        &self,
        pk_columns: &[String],
        table_name: &str,
        constraint_name: Option<&str>,
    ) -> Result<String>;

    /// Generate SQL for a foreign key
    fn generate_foreign_key_sql(&self, fk: &Constraint, table_name: &str) -> Result<String>;

    /// Generate SQL for an index
    fn generate_index_sql(&self, index: &IndexDescriptor, table_name: &str) -> Result<String>;

    /// Generate CREATE TABLE SQL for a model
    fn generate_create_table_sql(&self, model: &ModelDescriptor) -> Result<String>;

    /// Generate DROP TABLE SQL
    fn generate_drop_table_sql(
        &self,
        table_name: &str,
        schema_name: Option<&str>,
    ) -> Result<String>;

    /// Generate SQL to add a column to an existing table
    fn generate_add_column_sql(
        &self,
        table_name: &str,
        column: &ColumnDescriptor,
        schema_name: Option<&str>,
    ) -> Result<String>;

    /// Generate SQL to drop a column from an existing table
    fn generate_drop_column_sql(
        &self,
        table_name: &str,
        column_name: &str,
        schema_name: Option<&str>,
    ) -> Result<String>;

    /// Generate SQL to rename a table
    fn generate_rename_table_sql(
        &self,
        old_table_name: &str,
        new_table_name: &str,
        schema_name: Option<&str>,
    ) -> Result<String>;

    /// Get the character used to quote identifiers at the start
    fn identifier_quote_char_start(&self) -> char;

    /// Get the character used to quote identifiers at the end
    fn identifier_quote_char_end(&self) -> char;

    /// Escape a string literal for SQL
    fn escape_string_literal(&self, value: &str) -> String;

    /// Quote an identifier (table, column name) for the database
    fn quote_identifier(&self, identifier: &str) -> String {
        let start = self.identifier_quote_char_start();
        let end = self.identifier_quote_char_end();
        format!("{}{}{}", start, identifier, end)
    }

    /// Generate SQL statements for enum types used in the model (e.g., CREATE TYPE) for supported databases
    fn generate_enum_types_sql(&self, _model: &ModelDescriptor) -> Result<Vec<String>> {
        // Default implementation: no enum types
        Ok(Vec::new())
    }

    /// Generate SQL to drop a constraint from a table
    fn generate_drop_constraint_sql(
        &self,
        table_name: &str,
        constraint_name: &str,
        schema_name: Option<&str>,
    ) -> Result<String>;

    /// Generate SQL to drop an index
    fn generate_drop_index_sql(
        &self,
        table_name: &str,
        index_name: &str,
        schema_name: Option<&str>,
    ) -> Result<String>;
}
