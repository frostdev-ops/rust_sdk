// MySql adapter implementation

use crate::model_manager::{
    Error, Result,
    adapters::DatabaseAdapter,
    definitions::{
        ColumnDescriptor, Constraint, DataType, IndexDescriptor, IndexType, IntegerSize,
        ModelDescriptor, ReferentialAction,
    },
    errors::{adapter_error, unsupported_feature_error},
};

/// MySQL database adapter for model management
///
/// NOTE: This implementation targets MySQL \>= 5.7 (where JSON is available).
/// It should also work with MariaDB to a certain extent, but some features
/// (e.g., JSON) might not be fully supported.
#[derive(Debug, Default)]
pub struct MySqlAdapter;

impl MySqlAdapter {
    /// Create a new adapter instance
    pub fn new() -> Self {
        Self
    }

    /// Map ReferentialAction to MySQL SQL string
    fn fk_action_sql(action: Option<&ReferentialAction>) -> &'static str {
        match action {
            Some(ReferentialAction::NoAction) | None => "NO ACTION",
            Some(ReferentialAction::Restrict) => "RESTRICT",
            Some(ReferentialAction::Cascade) => "CASCADE",
            Some(ReferentialAction::SetNull) => "SET NULL",
            Some(ReferentialAction::SetDefault) => "SET DEFAULT",
        }
    }

    /// Format a full table name with optional schema (database) qualifier.
    fn format_table_name(&self, table_name: &str, schema: Option<&str>) -> String {
        match schema {
            Some(schema_name) if !schema_name.is_empty() => format!(
                "{}.{table}",
                self.quote_identifier(schema_name),
                table = self.quote_identifier(table_name)
            ),
            _ => self.quote_identifier(table_name),
        }
    }

    /// Map IntegerSize to the concrete MySQL integer type (optionally unsigned)
    fn map_integer_size(&self, size: &IntegerSize) -> &'static str {
        match size {
            IntegerSize::I8 => "TINYINT",
            IntegerSize::U8 => "TINYINT UNSIGNED",
            IntegerSize::I16 => "SMALLINT",
            IntegerSize::U16 => "SMALLINT UNSIGNED",
            IntegerSize::I32 => "INT",
            IntegerSize::U32 => "INT UNSIGNED",
            IntegerSize::I64 => "BIGINT",
            IntegerSize::U64 => "BIGINT UNSIGNED",
        }
    }
}

impl DatabaseAdapter for MySqlAdapter {
    type Error = Error;

    fn get_db_type_name(&self) -> &'static str {
        "mysql"
    }

    fn map_common_data_type(&self, common_type: &DataType) -> Result<String> {
        let ty = match common_type {
            DataType::Text(_) => "TEXT".to_string(),
            DataType::Varchar(len) => format!("VARCHAR({})", len),
            DataType::Char(len) => format!("CHAR({})", len),
            DataType::Integer(size) => self.map_integer_size(size).to_string(),
            DataType::SmallInt => "SMALLINT".to_string(),
            DataType::BigInt => "BIGINT".to_string(),
            DataType::Boolean => "TINYINT(1)".to_string(),
            DataType::Float => "FLOAT".to_string(),
            DataType::Double => "DOUBLE".to_string(),
            DataType::Decimal(p, s) => format!("DECIMAL({}, {})", p, s),
            DataType::Date => "DATE".to_string(),
            DataType::Time => "TIME".to_string(),
            DataType::DateTime => "DATETIME".to_string(),
            DataType::Timestamp | DataType::TimestampTz => "TIMESTAMP".to_string(),
            DataType::Blob => "BLOB".to_string(),
            DataType::Json | DataType::JsonB => "JSON".to_string(),
            DataType::Uuid => "CHAR(36)".to_string(),
            DataType::Enum(_, variants) => {
                // Represent enums inline as ENUM('v1', 'v2', ...)
                let vals: Vec<String> = variants
                    .iter()
                    .map(|v| format!("'{}'", self.escape_string_literal(v)))
                    .collect();
                format!("ENUM({})", vals.join(", "))
            }
            DataType::Custom(custom) => custom.clone(),
        };
        Ok(ty)
    }

    fn generate_column_definition_sql(&self, column: &ColumnDescriptor) -> Result<String> {
        let mut parts: Vec<String> = Vec::new();
        let quoted_name = self.quote_identifier(&column.name);
        parts.push(quoted_name);

        // Data type
        let data_type_sql = self.map_common_data_type(&column.data_type)?;
        parts.push(data_type_sql);

        // Nullability
        if column.is_nullable {
            parts.push("NULL".to_string());
        } else {
            parts.push("NOT NULL".to_string());
        }

        // Default value
        if let Some(default_val) = &column.default_value {
            parts.push(format!("DEFAULT {}", default_val));
        }

        // Auto increment
        if column.auto_increment {
            parts.push("AUTO_INCREMENT".to_string());
        }

        // Primary key (column-level)
        if column.is_primary_key {
            parts.push("PRIMARY KEY".to_string());
        }

        // Unique (column-level)
        if column.is_unique && !column.is_primary_key {
            // PRIMARY KEY implies UNIQUE already
            parts.push("UNIQUE".to_string());
        }

        // Comment
        if let Some(comment) = &column.comment {
            parts.push(format!("COMMENT '{}'", self.escape_string_literal(comment)));
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
                let cname = name.as_deref().unwrap_or("pk");
                let quoted_name = self.quote_identifier(&format!("{}_{}", table_name, cname));
                let quoted_cols: Vec<String> =
                    columns.iter().map(|c| self.quote_identifier(c)).collect();
                Ok(Some(format!(
                    "CONSTRAINT {} PRIMARY KEY ({})",
                    quoted_name,
                    quoted_cols.join(", ")
                )))
            }
            Constraint::Unique { name, columns } => {
                let cname = name.as_deref().unwrap_or("unique");
                let quoted_name = self.quote_identifier(&format!("{}_{}", table_name, cname));
                let quoted_cols: Vec<String> =
                    columns.iter().map(|c| self.quote_identifier(c)).collect();
                Ok(Some(format!(
                    "CONSTRAINT {} UNIQUE ({})",
                    quoted_name,
                    quoted_cols.join(", ")
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
                let cname = name.as_deref().unwrap_or("fk");
                let quoted_name = self.quote_identifier(&format!("{}_{}", table_name, cname));
                let cols: Vec<String> = columns.iter().map(|c| self.quote_identifier(c)).collect();
                let ref_cols: Vec<String> = references_columns
                    .iter()
                    .map(|c| self.quote_identifier(c))
                    .collect();
                let on_del = Self::fk_action_sql(on_delete.as_ref());
                let on_upd = Self::fk_action_sql(on_update.as_ref());

                Ok(Some(format!(
                    "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {} ON UPDATE {}",
                    quoted_name,
                    cols.join(", "),
                    self.quote_identifier(references_table),
                    ref_cols.join(", "),
                    on_del,
                    on_upd
                )))
            }
            Constraint::Check { name, expression } => {
                let cname = name.as_deref().unwrap_or("check");
                let quoted_name = self.quote_identifier(&format!("{}_{}", table_name, cname));
                Ok(Some(format!(
                    "CONSTRAINT {} CHECK ({})",
                    quoted_name, expression
                )))
            }
            // Column-level only
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
        let cname = constraint_name.unwrap_or("pk");
        let quoted_name = self.quote_identifier(&format!("{}_{}", table_name, cname));
        let cols: Vec<String> = pk_columns
            .iter()
            .map(|c| self.quote_identifier(c))
            .collect();
        Ok(format!(
            "CONSTRAINT {} PRIMARY KEY ({})",
            quoted_name,
            cols.join(", ")
        ))
    }

    fn generate_foreign_key_sql(&self, fk: &Constraint, table_name: &str) -> Result<String> {
        if let Constraint::ForeignKey { .. } = fk {
            self.generate_constraint_sql(fk, table_name)
                .map(|opt| opt.expect("FK should generate SQL"))
        } else {
            Err(adapter_error("Not a foreign key constraint"))
        }
    }

    fn generate_index_sql(&self, index: &IndexDescriptor, table_name: &str) -> Result<String> {
        // Determine index name
        let idx_name = index
            .name
            .clone()
            .unwrap_or_else(|| format!("{}_{}_idx", table_name, index.columns.join("_")));
        let quoted_idx = self.quote_identifier(&idx_name);
        let table_name_quoted = self.quote_identifier(table_name);
        let quoted_cols: Vec<String> = index
            .columns
            .iter()
            .map(|c| self.quote_identifier(c))
            .collect();

        // Handle uniqueness
        let uniqueness = if index.is_unique { "UNIQUE " } else { "" };

        // Handle index type
        let using_clause = match &index.index_type {
            None | Some(IndexType::BTree) => "".to_string(),
            Some(IndexType::Hash) => " USING HASH".to_string(),
            Some(unsupported) => {
                return Err(unsupported_feature_error(format!(
                    "MySQL adapter does not support {:?} index type",
                    unsupported
                )));
            }
        };

        // Condition / partial index not supported until MySQL 8.0
        if index.condition.is_some() {
            return Err(unsupported_feature_error(
                "MySQL does not support partial indexes (index conditions) prior to 8.0",
            ));
        }

        Ok(format!(
            "CREATE {}INDEX {} ON {} ({}){}",
            uniqueness,
            quoted_idx,
            table_name_quoted,
            quoted_cols.join(", "),
            using_clause
        ))
    }

    fn generate_create_table_sql(&self, model: &ModelDescriptor) -> Result<String> {
        // Table name with schema.
        let table_full_name = self.format_table_name(&model.name, model.schema.as_deref());

        // Column definitions
        let mut col_defs: Vec<String> = model
            .columns
            .iter()
            .map(|c| self.generate_column_definition_sql(c))
            .collect::<Result<Vec<_>>>()?;

        // Constraints
        let mut constraints: Vec<String> = Vec::new();

        if let Some(pk) = &model.primary_key {
            constraints.push(self.generate_primary_key_sql(pk, &model.name, None)?);
        } else {
            // Implicit primary key from column flags
            let pk_cols: Vec<String> = model
                .columns
                .iter()
                .filter(|c| c.is_primary_key)
                .map(|c| c.name.clone())
                .collect();
            if !pk_cols.is_empty() {
                constraints.push(self.generate_primary_key_sql(&pk_cols, &model.name, None)?);
            }
        }

        for c in &model.constraints {
            if let Some(sql) = self.generate_constraint_sql(c, &model.name)? {
                constraints.push(sql);
            }
        }

        // Combine all definitions
        col_defs.extend(constraints);
        let defs_joined = col_defs.join(",\n  ");

        // Engine & charset & collation
        let mut suffix_opts = String::new();
        if let Some(engine) = &model.engine {
            suffix_opts.push_str(&format!(" ENGINE={}", engine));
        }
        if let Some(charset) = &model.charset {
            suffix_opts.push_str(&format!(" DEFAULT CHARSET={}", charset));
        }
        if let Some(collation) = &model.collation {
            suffix_opts.push_str(&format!(" COLLATE {}", collation));
        }
        if let Some(comment) = &model.comment {
            suffix_opts.push_str(&format!(
                " COMMENT='{}'",
                self.escape_string_literal(comment)
            ));
        }

        Ok(format!(
            "CREATE TABLE {} (\n  {}\n){}",
            table_full_name, defs_joined, suffix_opts
        ))
    }

    fn generate_drop_table_sql(
        &self,
        table_name: &str,
        schema_name: Option<&str>,
    ) -> Result<String> {
        Ok(format!(
            "DROP TABLE IF EXISTS {}",
            self.format_table_name(table_name, schema_name)
        ))
    }

    fn generate_add_column_sql(
        &self,
        table_name: &str,
        column: &ColumnDescriptor,
        schema_name: Option<&str>,
    ) -> Result<String> {
        let tbl = self.format_table_name(table_name, schema_name);
        let column_sql = self.generate_column_definition_sql(column)?;
        Ok(format!("ALTER TABLE {} ADD COLUMN {}", tbl, column_sql))
    }

    fn generate_drop_column_sql(
        &self,
        table_name: &str,
        column_name: &str,
        schema_name: Option<&str>,
    ) -> Result<String> {
        let tbl = self.format_table_name(table_name, schema_name);
        Ok(format!(
            "ALTER TABLE {} DROP COLUMN {}",
            tbl,
            self.quote_identifier(column_name)
        ))
    }

    fn generate_rename_table_sql(
        &self,
        old_table_name: &str,
        new_table_name: &str,
        schema_name: Option<&str>,
    ) -> Result<String> {
        let old_full = self.format_table_name(old_table_name, schema_name);
        let new_full = self.format_table_name(new_table_name, schema_name);
        Ok(format!("RENAME TABLE {} TO {}", old_full, new_full))
    }

    fn identifier_quote_char_start(&self) -> char {
        '`'
    }

    fn identifier_quote_char_end(&self) -> char {
        '`'
    }

    fn escape_string_literal(&self, value: &str) -> String {
        value.replace('\'', "''")
    }

    /// Generate SQL to drop a constraint from a table
    fn generate_drop_constraint_sql(
        &self,
        table_name: &str,
        constraint_name: &str,
        schema_name: Option<&str>,
    ) -> Result<String> {
        // Qualify table with schema if present
        let full_table = if let Some(schema) = schema_name {
            format!(
                "{}.{}",
                self.quote_identifier(schema),
                self.quote_identifier(table_name)
            )
        } else {
            self.quote_identifier(table_name)
        };
        let quoted_constraint = self.quote_identifier(constraint_name);
        Ok(format!(
            "ALTER TABLE {} DROP CONSTRAINT {}",
            full_table, quoted_constraint
        ))
    }

    /// Generate SQL to drop an index
    fn generate_drop_index_sql(
        &self,
        table_name: &str,
        index_name: &str,
        schema_name: Option<&str>,
    ) -> Result<String> {
        // MySQL uses DROP INDEX <index> ON <table>
        let full_table = if let Some(schema) = schema_name {
            format!(
                "{}.{}",
                self.quote_identifier(schema),
                self.quote_identifier(table_name)
            )
        } else {
            self.quote_identifier(table_name)
        };
        let quoted_index = self.quote_identifier(index_name);
        Ok(format!("DROP INDEX {} ON {}", quoted_index, full_table))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integer_mapping() {
        let adapter = MySqlAdapter::new();
        assert_eq!(
            adapter
                .map_common_data_type(&DataType::Integer(IntegerSize::I32))
                .unwrap(),
            "INT"
        );
        assert_eq!(
            adapter
                .map_common_data_type(&DataType::Integer(IntegerSize::U32))
                .unwrap(),
            "INT UNSIGNED"
        );
    }

    #[test]
    fn test_create_table_sql() {
        let adapter = MySqlAdapter::new();
        use crate::model_manager::definitions::*;
        let model = ModelDescriptor {
            name: "users".to_string(),
            schema: None,
            columns: vec![
                ColumnDescriptor {
                    name: "id".to_string(),
                    data_type: DataType::Integer(IntegerSize::I32),
                    is_nullable: false,
                    is_primary_key: true,
                    is_unique: false,
                    default_value: None,
                    auto_increment: true,
                    comment: Some("Primary key".to_string()),
                    constraints: vec![],
                },
                ColumnDescriptor {
                    name: "username".to_string(),
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
            comment: Some("Users table".to_string()),
            engine: Some("InnoDB".to_string()),
            charset: Some("utf8mb4".to_string()),
            collation: Some("utf8mb4_unicode_ci".to_string()),
            options: Default::default(),
        };

        let sql = adapter.generate_create_table_sql(&model).unwrap();
        assert!(sql.contains("CREATE TABLE"));
        assert!(sql.contains("`id` INT NOT NULL AUTO_INCREMENT"));
        assert!(sql.contains("PRIMARY KEY"));
    }
}
