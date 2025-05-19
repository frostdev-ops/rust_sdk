use crate::model_manager::{
    Error, Result,
    adapters::DatabaseAdapter,
    definitions::{
        ColumnDescriptor, Constraint, DataType, IndexDescriptor, IndexType, ModelDescriptor,
        ReferentialAction,
    },
    errors::{adapter_error, unsupported_feature_error},
};

/// SQLite database adapter for model management
#[derive(Debug, Default)]
pub struct SqliteAdapter;

impl SqliteAdapter {
    /// Create a new SQLite adapter
    pub fn new() -> Self {
        Self
    }

    /// Helper to format the schema-qualified table name
    fn format_table_name(&self, table_name: &str, schema_name: Option<&str>) -> String {
        // SQLite doesn't traditionally use schemas in the same way as PostgreSQL
        // If a schema is specified, we'll use it as a table prefix
        if let Some(schema) = schema_name {
            format!("{}_{}", schema, table_name)
        } else {
            table_name.to_string()
        }
    }

    /// Helper to handle referential actions
    fn get_referential_action_sql(&self, action: Option<&ReferentialAction>) -> &'static str {
        match action {
            Some(ReferentialAction::NoAction) => "NO ACTION",
            Some(ReferentialAction::Restrict) => "RESTRICT",
            Some(ReferentialAction::Cascade) => "CASCADE",
            Some(ReferentialAction::SetNull) => "SET NULL",
            Some(ReferentialAction::SetDefault) => "SET DEFAULT",
            None => "NO ACTION", // Default
        }
    }
}

impl DatabaseAdapter for SqliteAdapter {
    type Error = Error;

    fn get_db_type_name(&self) -> &'static str {
        "sqlite"
    }

    fn map_common_data_type(&self, common_type: &DataType) -> Result<String> {
        match common_type {
            // SQLite has a flexible type system with only a few storage classes
            DataType::Text(_) | DataType::Varchar(_) | DataType::Char(_) => Ok("TEXT".to_string()),
            DataType::Integer(_) | DataType::SmallInt | DataType::BigInt => {
                Ok("INTEGER".to_string())
            }
            DataType::Boolean => Ok("INTEGER".to_string()), // SQLite uses 0/1 for booleans
            DataType::Float | DataType::Double => Ok("REAL".to_string()),
            DataType::Decimal(_, _) => Ok("NUMERIC".to_string()),
            DataType::Date
            | DataType::Time
            | DataType::DateTime
            | DataType::Timestamp
            | DataType::TimestampTz => {
                // SQLite doesn't have dedicated date/time types
                // Options are TEXT (ISO8601 strings), REAL (Julian day numbers), or INTEGER (Unix timestamp)
                Ok("TEXT".to_string())
            }
            DataType::Blob => Ok("BLOB".to_string()),
            DataType::Json | DataType::JsonB => Ok("TEXT".to_string()), // Store JSON as TEXT
            DataType::Uuid => Ok("TEXT".to_string()),                   // Store UUID as TEXT
            DataType::Enum(_, _) => {
                // SQLite doesn't have a dedicated enum type - we'd use a CHECK constraint
                // but for type mapping only, we'll use TEXT
                Ok("TEXT".to_string())
            }
            DataType::Custom(custom_type) => Ok(custom_type.clone()), // Use as-is
        }
    }

    fn generate_column_definition_sql(&self, column: &ColumnDescriptor) -> Result<String> {
        let quoted_name = self.quote_identifier(&column.name);
        let data_type = self.map_common_data_type(&column.data_type)?;

        let mut parts = vec![quoted_name, data_type];

        // Handle primary key - use rowid alias form for auto-increment
        if column.is_primary_key {
            if column.auto_increment {
                // SQLite special case for auto-increment primary key
                if matches!(
                    column.data_type,
                    DataType::Integer(_) | DataType::SmallInt | DataType::BigInt
                ) {
                    parts.push("PRIMARY KEY AUTOINCREMENT".to_string());
                } else {
                    return Err(adapter_error(
                        "SQLite AUTOINCREMENT can only be used with INTEGER PRIMARY KEY",
                    ));
                }
            } else {
                parts.push("PRIMARY KEY".to_string());
            }
        }

        // NOT NULL constraint
        if !column.is_nullable {
            parts.push("NOT NULL".to_string());
        }

        // Unique constraint (for the column only)
        if column.is_unique {
            parts.push("UNIQUE".to_string());
        }

        // Default value
        if let Some(default_value) = &column.default_value {
            parts.push(format!("DEFAULT {}", default_value));
        }

        // SQLite supports inline CHECK constraints
        for constraint in &column.constraints {
            if let Constraint::Check { expression, .. } = constraint {
                parts.push(format!("CHECK ({})", expression));
            }
        }

        Ok(parts.join(" "))
    }

    fn generate_constraint_sql(
        &self,
        constraint: &Constraint,
        table_name: &str,
    ) -> Result<Option<String>> {
        match constraint {
            Constraint::PrimaryKey { name, columns } => {
                let constraint_name = name.as_deref().unwrap_or("pk");
                let sql =
                    self.generate_primary_key_sql(columns, table_name, Some(constraint_name))?;
                Ok(Some(sql))
            }
            Constraint::Unique { name, columns } => {
                let constraint_name = name.as_deref().unwrap_or("unique");
                let quoted_name =
                    self.quote_identifier(&format!("{}_{}", table_name, constraint_name));
                let quoted_columns: Vec<String> = columns
                    .iter()
                    .map(|col| self.quote_identifier(col))
                    .collect();

                Ok(Some(format!(
                    "CONSTRAINT {} UNIQUE ({})",
                    quoted_name,
                    quoted_columns.join(", ")
                )))
            }
            Constraint::ForeignKey {
                name,
                columns,
                references_table,
                references_columns,
                on_delete,
                on_update,
            } => {
                let constraint_name = name.as_deref().unwrap_or("fk");
                let quoted_name =
                    self.quote_identifier(&format!("{}_{}", table_name, constraint_name));
                let quoted_columns: Vec<String> = columns
                    .iter()
                    .map(|col| self.quote_identifier(col))
                    .collect();
                let quoted_ref_table = self.quote_identifier(references_table);
                let quoted_ref_columns: Vec<String> = references_columns
                    .iter()
                    .map(|col| self.quote_identifier(col))
                    .collect();

                let on_delete_sql = self.get_referential_action_sql(on_delete.as_ref());
                let on_update_sql = self.get_referential_action_sql(on_update.as_ref());

                Ok(Some(format!(
                    "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {} ON UPDATE {}",
                    quoted_name,
                    quoted_columns.join(", "),
                    quoted_ref_table,
                    quoted_ref_columns.join(", "),
                    on_delete_sql,
                    on_update_sql
                )))
            }
            Constraint::Check { name, expression } => {
                let constraint_name = name.as_deref().unwrap_or("check");
                let quoted_name =
                    self.quote_identifier(&format!("{}_{}", table_name, constraint_name));

                Ok(Some(format!(
                    "CONSTRAINT {} CHECK ({})",
                    quoted_name, expression
                )))
            }
            // These are typically handled inline at the column level
            Constraint::NotNull | Constraint::DefaultValue(_) | Constraint::AutoIncrement => {
                Ok(None)
            }
        }
    }

    fn generate_primary_key_sql(
        &self,
        pk_columns: &[String],
        table_name: &str,
        constraint_name: Option<&str>,
    ) -> Result<String> {
        let quoted_columns: Vec<String> = pk_columns
            .iter()
            .map(|col| self.quote_identifier(col))
            .collect();

        let name_part = constraint_name
            .map(|name| {
                format!(
                    "CONSTRAINT {} ",
                    self.quote_identifier(&format!("{}_{}", table_name, name))
                )
            })
            .unwrap_or_default();

        Ok(format!(
            "{}PRIMARY KEY ({})",
            name_part,
            quoted_columns.join(", ")
        ))
    }

    fn generate_foreign_key_sql(&self, fk: &Constraint, table_name: &str) -> Result<String> {
        match fk {
            Constraint::ForeignKey {
                name,
                columns,
                references_table,
                references_columns,
                on_delete,
                on_update,
            } => {
                let constraint_name = name.as_deref().unwrap_or("fk");
                let quoted_name =
                    self.quote_identifier(&format!("{}_{}", table_name, constraint_name));

                let quoted_columns: Vec<String> = columns
                    .iter()
                    .map(|col| self.quote_identifier(col))
                    .collect();

                let quoted_ref_table = self.quote_identifier(references_table);
                let quoted_ref_columns: Vec<String> = references_columns
                    .iter()
                    .map(|col| self.quote_identifier(col))
                    .collect();

                let on_delete_sql = self.get_referential_action_sql(on_delete.as_ref());
                let on_update_sql = self.get_referential_action_sql(on_update.as_ref());

                // SQLite doesn't support ALTER TABLE ADD CONSTRAINT FOREIGN KEY
                // We'll define this as the SQL to be used at table creation time
                Ok(format!(
                    "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {} ON UPDATE {}",
                    quoted_name,
                    quoted_columns.join(", "),
                    quoted_ref_table,
                    quoted_ref_columns.join(", "),
                    on_delete_sql,
                    on_update_sql
                ))
            }
            _ => Err(adapter_error("Not a foreign key constraint")),
        }
    }

    fn generate_index_sql(&self, index: &IndexDescriptor, table_name: &str) -> Result<String> {
        let index_name = match &index.name {
            Some(name) => name.clone(),
            None => format!("{}_{}_idx", table_name, index.columns.join("_")),
        };

        let quoted_name = self.quote_identifier(&index_name);
        let quoted_table = self.quote_identifier(table_name);

        let quoted_columns: Vec<String> = index
            .columns
            .iter()
            .map(|col| self.quote_identifier(col))
            .collect();

        let unique = if index.is_unique { "UNIQUE " } else { "" };

        // SQLite doesn't support most index types - it only has the default btree index
        if let Some(idx_type) = &index.index_type {
            if !matches!(idx_type, IndexType::BTree) {
                return Err(unsupported_feature_error(format!(
                    "SQLite only supports BTree indexes, not {:?}",
                    idx_type
                )));
            }
        }

        let where_clause = index
            .condition
            .as_ref()
            .map(|cond| format!(" WHERE {}", cond))
            .unwrap_or_default();

        Ok(format!(
            "CREATE {}INDEX {} ON {} ({}){}",
            unique,
            quoted_name,
            quoted_table,
            quoted_columns.join(", "),
            where_clause
        ))
    }

    fn generate_create_table_sql(&self, model: &ModelDescriptor) -> Result<String> {
        let table_name = self.format_table_name(&model.name, model.schema.as_deref());
        let quoted_table = self.quote_identifier(&table_name);

        // Generate column definitions
        let mut column_defs = Vec::new();
        for column in &model.columns {
            let column_sql = self.generate_column_definition_sql(column)?;
            column_defs.push(column_sql);
        }

        // Add table-level constraints
        let mut table_constraints = Vec::new();

        // Add primary key if specified at table level
        if let Some(pk_columns) = &model.primary_key {
            let pk_sql = self.generate_primary_key_sql(pk_columns, &table_name, None)?;
            table_constraints.push(pk_sql);
        }

        // Add other table constraints
        for constraint in &model.constraints {
            if let Some(constraint_sql) = self.generate_constraint_sql(constraint, &table_name)? {
                table_constraints.push(constraint_sql);
            }
        }

        // Combine column defs and table constraints
        let mut all_parts = column_defs;
        all_parts.extend(table_constraints);

        // Generate the full CREATE TABLE statement
        let create_table_sql = format!(
            "CREATE TABLE {} (\n  {}\n)",
            quoted_table,
            all_parts.join(",\n  ")
        );

        Ok(create_table_sql)
    }

    fn generate_drop_table_sql(
        &self,
        table_name: &str,
        schema_name: Option<&str>,
    ) -> Result<String> {
        let name = self.format_table_name(table_name, schema_name);
        let quoted_table = self.quote_identifier(&name);

        Ok(format!("DROP TABLE IF EXISTS {}", quoted_table))
    }

    fn generate_add_column_sql(
        &self,
        table_name: &str,
        column: &ColumnDescriptor,
        schema_name: Option<&str>,
    ) -> Result<String> {
        let name = self.format_table_name(table_name, schema_name);
        let quoted_table = self.quote_identifier(&name);
        let column_sql = self.generate_column_definition_sql(column)?;

        // SQLite has limited ALTER TABLE support
        // It can add a column, but with restrictions:
        // - Cannot have PRIMARY KEY or UNIQUE constraints
        // - Cannot have foreign keys
        // - Can have NOT NULL only with a DEFAULT value

        if column.is_primary_key {
            return Err(unsupported_feature_error(
                "SQLite cannot add a PRIMARY KEY column to an existing table",
            ));
        }

        if column.is_unique {
            return Err(unsupported_feature_error(
                "SQLite cannot add a UNIQUE column to an existing table",
            ));
        }

        if !column.is_nullable && column.default_value.is_none() {
            return Err(unsupported_feature_error(
                "SQLite cannot add a NOT NULL column without a DEFAULT value to an existing table",
            ));
        }

        // Check for foreign key constraints
        for constraint in &column.constraints {
            if matches!(constraint, Constraint::ForeignKey { .. }) {
                return Err(unsupported_feature_error(
                    "SQLite cannot add a FOREIGN KEY column to an existing table",
                ));
            }
        }

        Ok(format!(
            "ALTER TABLE {} ADD COLUMN {}",
            quoted_table, column_sql
        ))
    }

    fn generate_drop_column_sql(
        &self,
        _table_name: &str,
        _column_name: &str,
        _schema_name: Option<&str>,
    ) -> Result<String> {
        Err(unsupported_feature_error(
            "SQLite doesn't support DROP COLUMN in versions before 3.35.0. \
            For older versions, you must recreate the table without the column.",
        ))
    }

    fn generate_rename_table_sql(
        &self,
        old_table_name: &str,
        new_table_name: &str,
        schema_name: Option<&str>,
    ) -> Result<String> {
        let old_name = self.format_table_name(old_table_name, schema_name);
        let new_name = self.format_table_name(new_table_name, schema_name);

        let quoted_old_table = self.quote_identifier(&old_name);
        let quoted_new_table = self.quote_identifier(&new_name);

        Ok(format!(
            "ALTER TABLE {} RENAME TO {}",
            quoted_old_table, quoted_new_table
        ))
    }

    fn identifier_quote_char_start(&self) -> char {
        '"'
    }

    fn identifier_quote_char_end(&self) -> char {
        '"'
    }

    fn escape_string_literal(&self, value: &str) -> String {
        // For SQLite, escape ' as ''
        value.replace('\'', "''")
    }

    /// Generate SQL to drop a constraint from a table
    fn generate_drop_constraint_sql(
        &self,
        _table_name: &str,
        _constraint_name: &str,
        _schema_name: Option<&str>,
    ) -> Result<String> {
        // SQLite does not support ALTER TABLE DROP CONSTRAINT.
        // Constraints must be dropped by recreating the table without the constraint.
        Err(unsupported_feature_error(
            "SQLite does not support DROP CONSTRAINT. Table must be recreated.",
        ))
    }

    /// Generate SQL to drop an index
    fn generate_drop_index_sql(
        &self,
        _table_name: &str,
        index_name: &str,
        _schema_name: Option<&str>,
    ) -> Result<String> {
        let quoted_index = self.quote_identifier(index_name);
        Ok(format!("DROP INDEX {}", quoted_index))
    }
}

/// Test module for SQLite adapter
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_manager::definitions::{IntegerSize, ModelDescriptor};

    #[test]
    fn test_map_data_types() {
        let adapter = SqliteAdapter::new();
        assert_eq!(
            adapter.map_common_data_type(&DataType::Text(None)).unwrap(),
            "TEXT"
        );
        assert_eq!(
            adapter
                .map_common_data_type(&DataType::Varchar(255))
                .unwrap(),
            "TEXT"
        );
        assert_eq!(
            adapter
                .map_common_data_type(&DataType::Integer(IntegerSize::I32))
                .unwrap(),
            "INTEGER"
        );
        assert_eq!(
            adapter.map_common_data_type(&DataType::Boolean).unwrap(),
            "INTEGER"
        );
        assert_eq!(
            adapter.map_common_data_type(&DataType::Float).unwrap(),
            "REAL"
        );
        assert_eq!(
            adapter
                .map_common_data_type(&DataType::Decimal(10, 2))
                .unwrap(),
            "NUMERIC"
        );
        assert_eq!(
            adapter.map_common_data_type(&DataType::Blob).unwrap(),
            "BLOB"
        );
        assert_eq!(
            adapter.map_common_data_type(&DataType::Json).unwrap(),
            "TEXT"
        );
        assert_eq!(
            adapter.map_common_data_type(&DataType::Uuid).unwrap(),
            "TEXT"
        );
    }

    #[test]
    fn test_generate_column_definition() {
        let adapter = SqliteAdapter::new();
        let basic_column = ColumnDescriptor {
            name: "username".to_string(),
            data_type: DataType::Varchar(100),
            is_nullable: false,
            is_primary_key: false,
            is_unique: false,
            default_value: None,
            auto_increment: false,
            comment: None,
            constraints: vec![],
        };
        assert_eq!(
            adapter
                .generate_column_definition_sql(&basic_column)
                .unwrap(),
            "\"username\" TEXT NOT NULL"
        );
        let pk_column = ColumnDescriptor {
            name: "id".to_string(),
            data_type: DataType::Integer(IntegerSize::I64),
            is_nullable: false,
            is_primary_key: true,
            is_unique: false,
            default_value: None,
            auto_increment: true,
            comment: None,
            constraints: vec![],
        };
        assert_eq!(
            adapter.generate_column_definition_sql(&pk_column).unwrap(),
            "\"id\" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL"
        );
    }

    #[test]
    fn test_generate_create_table() {
        let adapter = SqliteAdapter::new();
        let user_model = ModelDescriptor {
            name: "users".to_string(),
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
                    name: "username".to_string(),
                    data_type: DataType::Varchar(100),
                    is_nullable: false,
                    is_primary_key: false,
                    is_unique: true,
                    default_value: None,
                    auto_increment: false,
                    comment: None,
                    constraints: vec![],
                },
                ColumnDescriptor {
                    name: "email".to_string(),
                    data_type: DataType::Varchar(255),
                    is_nullable: false,
                    is_primary_key: false,
                    is_unique: true,
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
        let create_table_sql = adapter.generate_create_table_sql(&user_model).unwrap();
        assert!(create_table_sql.contains("CREATE TABLE \"users\""));
        assert!(create_table_sql.contains("\"id\" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL"));
        assert!(create_table_sql.contains("\"username\" TEXT NOT NULL UNIQUE"));
        assert!(create_table_sql.contains("\"email\" TEXT NOT NULL UNIQUE"));
    }
}
