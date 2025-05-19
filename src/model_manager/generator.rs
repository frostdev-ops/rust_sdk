use crate::model_manager::{
    Error, Result,
    adapters::DatabaseAdapter,
    definitions::{Constraint, ModelDescriptor},
    errors::sql_generation_error,
};

/// Service for generating SQL from model definitions
pub struct ModelGenerator {
    /// The database adapter to use for SQL generation
    adapter: Box<dyn DatabaseAdapter<Error = Error>>,
}

impl ModelGenerator {
    /// Create a new ModelGenerator with the given adapter
    pub fn new(adapter: Box<dyn DatabaseAdapter<Error = Error>>) -> Self {
        Self { adapter }
    }

    /// Generate the complete SQL script to create a table
    pub fn generate_create_table_script(&self, model: &ModelDescriptor) -> Result<String> {
        let mut script = String::new();

        // For PostgreSQL, we might need to create enum types first
        // (This will be implemented in the PostgreSQL adapter during milestone 2)

        // Generate the CREATE TABLE statement
        let create_table_sql = self.adapter.generate_create_table_sql(model)?;
        script.push_str(&create_table_sql);
        script.push_str(";\n\n");

        // Generate CREATE INDEX statements for each index
        for index in &model.indexes {
            let index_sql = self.adapter.generate_index_sql(index, &model.name)?;
            script.push_str(&index_sql);
            script.push_str(";\n");
        }

        // PostgreSQL-specific: Generate constraints that couldn't be inline
        // (Will be handled by the adapter in future implementations)

        Ok(script)
    }

    /// Generate the SQL script to drop a table
    pub fn generate_drop_table_script(
        &self,
        table_name: &str,
        schema_name: Option<&str>,
    ) -> Result<String> {
        self.adapter
            .generate_drop_table_sql(table_name, schema_name)
    }

    /// Generate the SQL to add a column to an existing table
    pub fn generate_add_column_script(
        &self,
        table_name: &str,
        column: &crate::model_manager::definitions::ColumnDescriptor,
        schema_name: Option<&str>,
    ) -> Result<String> {
        self.adapter
            .generate_add_column_sql(table_name, column, schema_name)
    }

    /// Generate the SQL to drop a column from an existing table
    pub fn generate_drop_column_script(
        &self,
        table_name: &str,
        column_name: &str,
        schema_name: Option<&str>,
    ) -> Result<String> {
        self.adapter
            .generate_drop_column_sql(table_name, column_name, schema_name)
    }

    /// Generate SQL for a foreign key constraint
    pub fn generate_foreign_key_script(&self, table_name: &str, fk: &Constraint) -> Result<String> {
        match fk {
            Constraint::ForeignKey { .. } => self.adapter.generate_foreign_key_sql(fk, table_name),
            _ => Err(sql_generation_error("Not a foreign key constraint")),
        }
    }

    /// Get the database adapter used by this generator
    pub fn adapter(&self) -> &dyn DatabaseAdapter<Error = Error> {
        &*self.adapter
    }

    /// Get a mutable reference to the database adapter
    pub fn adapter_mut(&mut self) -> &mut dyn DatabaseAdapter<Error = Error> {
        &mut *self.adapter
    }

    /// Generate the SQL migration script to transform one model into another by adding and dropping columns
    pub fn generate_migration_script(
        &self,
        from: &ModelDescriptor,
        to: &ModelDescriptor,
    ) -> Result<String> {
        let mut script = String::new();
        // Add new columns in 'to' that are not in 'from'
        for col in &to.columns {
            if !from.columns.iter().any(|c| c.name == col.name) {
                let stmt =
                    self.adapter
                        .generate_add_column_sql(&to.name, col, to.schema.as_deref())?;
                script.push_str(&stmt);
                script.push_str(";\n");
            }
        }
        // Drop columns in 'from' that are not in 'to'
        for col in &from.columns {
            if !to.columns.iter().any(|c| c.name == col.name) {
                let stmt = self.adapter.generate_drop_column_sql(
                    &from.name,
                    &col.name,
                    from.schema.as_deref(),
                )?;
                script.push_str(&stmt);
                script.push_str(";\n");
            }
        }
        // Add new table-level constraints in 'to' not in 'from'
        for constraint in &to.constraints {
            if !from.constraints.contains(constraint) {
                if let Some(sql) = self.adapter.generate_constraint_sql(constraint, &to.name)? {
                    script.push_str(&sql);
                    script.push_str(";\n");
                }
            }
        }
        // Drop table-level constraints in 'from' not in 'to'
        for constraint in &from.constraints {
            if !to.constraints.contains(constraint) {
                // Determine base name for constraint
                let base_name = match constraint {
                    Constraint::PrimaryKey { name, .. }
                    | Constraint::Unique { name, .. }
                    | Constraint::Check { name, .. }
                    | Constraint::ForeignKey { name, .. } => name.as_deref().unwrap_or_default(),
                    _ => continue,
                };
                let constraint_name = format!("{}_{}", from.name, base_name);
                let stmt = self.adapter.generate_drop_constraint_sql(
                    &from.name,
                    &constraint_name,
                    from.schema.as_deref(),
                )?;
                script.push_str(&stmt);
                script.push_str(";\n");
            }
        }
        // Add new indexes in 'to' not in 'from'
        for index in &to.indexes {
            if !from.indexes.contains(index) {
                let stmt = self.adapter.generate_index_sql(index, &to.name)?;
                script.push_str(&stmt);
                script.push_str(";\n");
            }
        }
        // Drop indexes in 'from' not in 'to'
        for index in &from.indexes {
            if !to.indexes.contains(index) {
                let index_name = index
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("{}_{}_idx", from.name, index.columns.join("_")));
                let stmt = self.adapter.generate_drop_index_sql(
                    &from.name,
                    &index_name,
                    from.schema.as_deref(),
                )?;
                script.push_str(&stmt);
                script.push_str(";\n");
            }
        }
        Ok(script)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_manager::adapters::SqliteAdapter;
    use crate::model_manager::definitions::{
        ColumnDescriptor, Constraint, DataType, IndexDescriptor, IntegerSize, ModelDescriptor,
    };

    #[test]
    fn test_generate_create_table_script_simple() {
        let adapter = Box::new(SqliteAdapter::new());
        let generator = ModelGenerator::new(adapter);
        let model = ModelDescriptor {
            name: "test".to_string(),
            schema: None,
            columns: vec![
                ColumnDescriptor {
                    name: "id".to_string(),
                    data_type: DataType::Integer(IntegerSize::I64),
                    is_nullable: false,
                    is_primary_key: true,
                    is_unique: false,
                    default_value: None,
                    auto_increment: true,
                    comment: None,
                    constraints: vec![],
                },
                ColumnDescriptor {
                    name: "name".to_string(),
                    data_type: DataType::Text(None),
                    is_nullable: true,
                    is_primary_key: false,
                    is_unique: false,
                    default_value: None,
                    auto_increment: false,
                    comment: None,
                    constraints: vec![],
                },
            ],
            primary_key: None,
            indexes: vec![],
            constraints: vec![],
            comment: None,
            engine: None,
            charset: None,
            collation: None,
            options: Default::default(),
        };
        let sql = generator.generate_create_table_script(&model).unwrap();
        assert!(sql.contains("CREATE TABLE \"test\""));
        assert!(sql.contains("\"id\" INTEGER PRIMARY KEY AUTOINCREMENT"));
        assert!(sql.contains("\"name\" TEXT"));
    }

    #[test]
    fn test_generate_drop_table_script_simple() {
        let adapter = Box::new(SqliteAdapter::new());
        let generator = ModelGenerator::new(adapter);
        let sql = generator.generate_drop_table_script("test", None).unwrap();
        assert_eq!(sql, "DROP TABLE IF EXISTS \"test\"");
    }

    #[test]
    fn test_generate_migration_script_add_and_drop() {
        let adapter = Box::new(SqliteAdapter::new());
        let generator = ModelGenerator::new(adapter);
        // Original model with only 'id'
        let from = ModelDescriptor {
            name: "items".to_string(),
            schema: None,
            columns: vec![ColumnDescriptor {
                name: "id".to_string(),
                data_type: DataType::Integer(IntegerSize::I64),
                is_nullable: false,
                is_primary_key: true,
                is_unique: false,
                default_value: None,
                auto_increment: true,
                comment: None,
                constraints: vec![],
            }],
            primary_key: None,
            indexes: vec![],
            constraints: vec![],
            comment: None,
            engine: None,
            charset: None,
            collation: None,
            options: Default::default(),
        };
        // New model with 'id' and 'name'
        let to = ModelDescriptor {
            name: "items".to_string(),
            schema: None,
            columns: vec![
                ColumnDescriptor {
                    name: "id".to_string(),
                    data_type: DataType::Integer(IntegerSize::I64),
                    is_nullable: false,
                    is_primary_key: true,
                    is_unique: false,
                    default_value: None,
                    auto_increment: true,
                    comment: None,
                    constraints: vec![],
                },
                ColumnDescriptor {
                    name: "name".to_string(),
                    data_type: DataType::Text(None),
                    is_nullable: true,
                    is_primary_key: false,
                    is_unique: false,
                    default_value: None,
                    auto_increment: false,
                    comment: None,
                    constraints: vec![],
                },
            ],
            primary_key: None,
            indexes: vec![],
            constraints: vec![],
            comment: None,
            engine: None,
            charset: None,
            collation: None,
            options: Default::default(),
        };
        let migration_sql = generator.generate_migration_script(&from, &to).unwrap();
        // Expect an ADD COLUMN for 'name'
        assert!(migration_sql.contains("ADD COLUMN \"name\" TEXT"));
        // Expect no DROP COLUMN for 'id'
        assert!(!migration_sql.contains("DROP COLUMN \"id\""));

        // For the reverse migration (dropping 'name' column), SQLite requires table recreation
        // Since SQLite doesn't support DROP COLUMN in older versions, we expect an error
        match generator.generate_migration_script(&to, &from) {
            Ok(_) => panic!("Expected an error for DROP COLUMN with SQLite"),
            Err(err) => {
                assert!(
                    err.to_string()
                        .contains("SQLite doesn't support DROP COLUMN")
                );
            }
        }
    }

    #[test]
    fn test_generate_migration_script_constraints() {
        let adapter = Box::new(SqliteAdapter::new());
        let generator = ModelGenerator::new(adapter);
        // No constraints in 'from'
        let from = ModelDescriptor {
            name: "users".to_string(),
            schema: None,
            columns: vec![],
            primary_key: None,
            indexes: vec![],
            constraints: vec![],
            comment: None,
            engine: None,
            charset: None,
            collation: None,
            options: Default::default(),
        };
        // Add a unique constraint in 'to'
        let to = ModelDescriptor {
            name: "users".to_string(),
            schema: None,
            columns: vec![],
            primary_key: None,
            indexes: vec![],
            constraints: vec![Constraint::Unique {
                name: Some("u1".to_string()),
                columns: vec!["email".to_string()],
            }],
            comment: None,
            engine: None,
            charset: None,
            collation: None,
            options: Default::default(),
        };
        let sql = generator.generate_migration_script(&from, &to).unwrap();
        assert!(sql.contains("CONSTRAINT \"users_u1\" UNIQUE"));

        // Reverse (dropping constraint) should now be handled as an error for SQLite
        match generator.generate_migration_script(&to, &from) {
            // 'to' has constraint, 'from' does not
            Ok(unexpected_sql) => {
                panic!(
                    "Expected an error when trying to generate DROP CONSTRAINT for SQLite, but got SQL: {}",
                    unexpected_sql
                );
            }
            Err(err) => {
                assert!(
                    err.to_string().contains(
                        "SQLite does not support DROP CONSTRAINT. Table must be recreated."
                    ),
                    "Error message mismatch, got: {}",
                    err
                );
            }
        }
    }

    #[test]
    fn test_generate_migration_script_indexes() {
        let adapter = Box::new(SqliteAdapter::new());
        let generator = ModelGenerator::new(adapter);
        // No indexes in 'from'
        let from = ModelDescriptor {
            name: "users".to_string(),
            schema: None,
            columns: vec![],
            primary_key: None,
            indexes: vec![],
            constraints: vec![],
            comment: None,
            engine: None,
            charset: None,
            collation: None,
            options: Default::default(),
        };
        // Add an index in 'to'
        let to = ModelDescriptor {
            name: "users".to_string(),
            schema: None,
            columns: vec![],
            primary_key: None,
            indexes: vec![IndexDescriptor {
                name: None,
                columns: vec!["email".to_string()],
                is_unique: true,
                index_type: None,
                condition: None,
            }],
            constraints: vec![],
            comment: None,
            engine: None,
            charset: None,
            collation: None,
            options: Default::default(),
        };
        let sql = generator.generate_migration_script(&from, &to).unwrap();
        assert!(sql.contains("CREATE UNIQUE INDEX \"users_email_idx\""));
        // Reverse should drop the index
        let reverse_sql = generator.generate_migration_script(&to, &from).unwrap();
        assert!(reverse_sql.contains("DROP INDEX \"users_email_idx\""));
    }
}
