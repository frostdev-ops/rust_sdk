use crate::model_manager::{
    Error, Result,
    adapters::DatabaseAdapter,
    definitions::{
        ColumnDescriptor, Constraint, DataType, IndexDescriptor, IndexType, IntegerSize,
        ModelDescriptor, ReferentialAction,
    },
    errors::{adapter_error, unsupported_feature_error},
};

/// PostgreSQL database adapter for model management
/// Implements DataType mapping and DDL generation
#[derive(Debug, Default)]
pub struct PostgresAdapter;

impl PostgresAdapter {
    /// Create a new adapter instance
    pub fn new() -> Self {
        Self
    }

    /// Get SQL for referential actions
    fn ref_action(action: Option<&ReferentialAction>) -> &'static str {
        match action {
            Some(ReferentialAction::NoAction) | None => "NO ACTION",
            Some(ReferentialAction::Restrict) => "RESTRICT",
            Some(ReferentialAction::Cascade) => "CASCADE",
            Some(ReferentialAction::SetNull) => "SET NULL",
            Some(ReferentialAction::SetDefault) => "SET DEFAULT",
        }
    }
}

impl DatabaseAdapter for PostgresAdapter {
    type Error = Error;

    fn get_db_type_name(&self) -> &'static str {
        "postgres"
    }

    fn map_common_data_type(&self, common_type: &DataType) -> Result<String> {
        let ty = match common_type {
            DataType::Text(_) => "TEXT".to_string(),
            DataType::Varchar(len) => format!("VARCHAR({})", len),
            DataType::Char(len) => format!("CHAR({})", len),
            DataType::Integer(size) => match size {
                IntegerSize::I8 | IntegerSize::U8 | IntegerSize::I16 | IntegerSize::U16 => {
                    "SMALLINT".to_string()
                }
                IntegerSize::I32 | IntegerSize::U32 => "INTEGER".to_string(),
                IntegerSize::I64 | IntegerSize::U64 => "BIGINT".to_string(),
            },
            DataType::SmallInt => "SMALLINT".to_string(),
            DataType::BigInt => "BIGINT".to_string(),
            DataType::Boolean => "BOOLEAN".to_string(),
            DataType::Float => "REAL".to_string(),
            DataType::Double => "DOUBLE PRECISION".to_string(),
            DataType::Decimal(p, s) => format!("NUMERIC({}, {})", p, s),
            DataType::Date => "DATE".to_string(),
            DataType::Time => "TIME WITHOUT TIME ZONE".to_string(),
            DataType::DateTime | DataType::Timestamp => "TIMESTAMP WITHOUT TIME ZONE".to_string(),
            DataType::TimestampTz => "TIMESTAMP WITH TIME ZONE".to_string(),
            DataType::Blob => "BYTEA".to_string(),
            DataType::Json => "JSON".to_string(),
            DataType::JsonB => "JSONB".to_string(),
            DataType::Uuid => "UUID".to_string(),
            DataType::Enum(name, _) => name.clone(),
            DataType::Custom(custom) => custom.clone(),
        };
        Ok(ty)
    }

    fn generate_column_definition_sql(&self, column: &ColumnDescriptor) -> Result<String> {
        let name = self.quote_identifier(&column.name);
        let mut sql;

        // Type and auto-increment
        if column.auto_increment {
            // Use SERIAL types
            match &column.data_type {
                DataType::Integer(size) => {
                    let t = match size {
                        IntegerSize::I8 | IntegerSize::U8 | IntegerSize::I16 | IntegerSize::U16 => {
                            "SMALLSERIAL"
                        }
                        IntegerSize::I32 | IntegerSize::U32 => "SERIAL",
                        IntegerSize::I64 | IntegerSize::U64 => "BIGSERIAL",
                    };
                    sql = format!("{} {}", name, t);
                }
                _ => {
                    return Err(adapter_error(
                        "SERIAL types only supported for integer columns",
                    ));
                }
            }
        } else {
            let ty = self.map_common_data_type(&column.data_type)?;
            sql = format!("{} {}", name, ty);
        }

        // Nullability
        if !column.is_nullable {
            sql.push_str(" NOT NULL");
        }

        // Default
        if let Some(def) = &column.default_value {
            sql.push_str(&format!(" DEFAULT {}", def));
        }

        // Unique
        if column.is_unique && !column.is_primary_key {
            sql.push_str(" UNIQUE");
        }

        // Inline CHECK
        for c in &column.constraints {
            if let Constraint::Check { expression, .. } = c {
                sql.push_str(&format!(" CHECK ({})", expression));
            }
        }

        Ok(sql)
    }

    fn generate_constraint_sql(
        &self,
        constraint: &Constraint,
        table: &str,
    ) -> Result<Option<String>> {
        match constraint {
            Constraint::PrimaryKey { name, columns } => {
                let cname = name.as_deref().unwrap_or("pk");
                let qn = self.quote_identifier(&format!("{}_{}", table, cname));
                let cols: Vec<String> = columns.iter().map(|c| self.quote_identifier(c)).collect();
                Ok(Some(format!(
                    "CONSTRAINT {} PRIMARY KEY ({})",
                    qn,
                    cols.join(", ")
                )))
            }
            Constraint::Unique { name, columns } => {
                let cname = name.as_deref().unwrap_or("unique");
                let qn = self.quote_identifier(&format!("{}_{}", table, cname));
                let cols: Vec<String> = columns.iter().map(|c| self.quote_identifier(c)).collect();
                Ok(Some(format!(
                    "CONSTRAINT {} UNIQUE ({})",
                    qn,
                    cols.join(", ")
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
                let qn = self.quote_identifier(&format!("{}_{}", table, cname));
                let cols: Vec<String> = columns.iter().map(|c| self.quote_identifier(c)).collect();
                let rcols: Vec<String> = references_columns
                    .iter()
                    .map(|c| self.quote_identifier(c))
                    .collect();
                let on_del = PostgresAdapter::ref_action(on_delete.as_ref());
                let on_upd = PostgresAdapter::ref_action(on_update.as_ref());
                let rt = self.quote_identifier(references_table);
                Ok(Some(format!(
                    "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {} ON UPDATE {}",
                    qn,
                    cols.join(", "),
                    rt,
                    rcols.join(", "),
                    on_del,
                    on_upd
                )))
            }
            Constraint::Check { name, expression } => {
                let cname = name.as_deref().unwrap_or("check");
                let qn = self.quote_identifier(&format!("{}_{}", table, cname));
                Ok(Some(format!("CONSTRAINT {} CHECK ({})", qn, expression)))
            }
            _ => Ok(None),
        }
    }

    fn generate_primary_key_sql(
        &self,
        pk_columns: &[String],
        table: &str,
        constraint_name: Option<&str>,
    ) -> Result<String> {
        let cname = constraint_name.unwrap_or("pk");
        let qn = self.quote_identifier(&format!("{}_{}", table, cname));
        let cols: Vec<String> = pk_columns
            .iter()
            .map(|c| self.quote_identifier(c))
            .collect();
        Ok(format!(
            "CONSTRAINT {} PRIMARY KEY ({})",
            qn,
            cols.join(", ")
        ))
    }

    fn generate_foreign_key_sql(&self, fk: &Constraint, table: &str) -> Result<String> {
        if let Constraint::ForeignKey { .. } = fk {
            Ok(self.generate_constraint_sql(fk, table)?.unwrap())
        } else {
            Err(adapter_error("Not a foreign key constraint"))
        }
    }

    fn generate_index_sql(&self, index: &IndexDescriptor, table: &str) -> Result<String> {
        let idx = index
            .name
            .clone()
            .unwrap_or_else(|| format!("{}_{}_idx", table, index.columns.join("_")));
        let qn = self.quote_identifier(&idx);
        let qt = self.quote_identifier(table);
        let cols: Vec<String> = index
            .columns
            .iter()
            .map(|c| self.quote_identifier(c))
            .collect();
        let uniq = if index.is_unique { "UNIQUE " } else { "" };
        let method = match &index.index_type {
            Some(IndexType::Gin) => " USING GIN",
            Some(IndexType::Gist) => " USING GIST",
            Some(IndexType::Hash) => " USING HASH",
            Some(IndexType::Spatial) => {
                return Err(unsupported_feature_error("Spatial indexes not supported"));
            }
            _ => "",
        };
        let where_clause = index
            .condition
            .as_ref()
            .map(|c| format!(" WHERE {}", c))
            .unwrap_or_default();
        Ok(format!(
            "CREATE {}INDEX {} ON {}{} ({}){}",
            uniq,
            qn,
            qt,
            method,
            cols.join(", "),
            where_clause
        ))
    }

    fn generate_create_table_sql(&self, model: &ModelDescriptor) -> Result<String> {
        let mut parts = Vec::new();
        for col in &model.columns {
            parts.push(self.generate_column_definition_sql(col)?);
        }
        if let Some(pk) = &model.primary_key {
            parts.push(self.generate_primary_key_sql(pk, &model.name, None)?);
        }
        for c in &model.constraints {
            if let Some(sql) = self.generate_constraint_sql(c, &model.name)? {
                parts.push(sql);
            }
        }
        let full = if let Some(s) = &model.schema {
            format!(
                "{}.{}",
                self.quote_identifier(s),
                self.quote_identifier(&model.name)
            )
        } else {
            self.quote_identifier(&model.name)
        };
        Ok(format!(
            "CREATE TABLE {} (\n  {}\n)",
            full,
            parts.join(",\n  ")
        ))
    }

    fn generate_drop_table_sql(&self, table: &str, schema: Option<&str>) -> Result<String> {
        Ok(match schema {
            Some(s) => format!(
                "DROP TABLE {}.{}",
                self.quote_identifier(s),
                self.quote_identifier(table)
            ),
            None => format!("DROP TABLE {}", self.quote_identifier(table)),
        })
    }

    fn generate_add_column_sql(
        &self,
        table: &str,
        column: &ColumnDescriptor,
        schema: Option<&str>,
    ) -> Result<String> {
        let full = if let Some(s) = schema {
            format!(
                "{}.{}",
                self.quote_identifier(s),
                self.quote_identifier(table)
            )
        } else {
            self.quote_identifier(table)
        };
        let col_sql = self.generate_column_definition_sql(column)?;
        Ok(format!("ALTER TABLE {} ADD COLUMN {}", full, col_sql))
    }

    fn generate_drop_column_sql(
        &self,
        table: &str,
        column: &str,
        schema: Option<&str>,
    ) -> Result<String> {
        let full = if let Some(s) = schema {
            format!(
                "{}.{}",
                self.quote_identifier(s),
                self.quote_identifier(table)
            )
        } else {
            self.quote_identifier(table)
        };
        Ok(format!(
            "ALTER TABLE {} DROP COLUMN {}",
            full,
            self.quote_identifier(column)
        ))
    }

    fn generate_rename_table_sql(
        &self,
        old: &str,
        new: &str,
        schema: Option<&str>,
    ) -> Result<String> {
        let o = if let Some(s) = schema {
            format!(
                "{}.{}",
                self.quote_identifier(s),
                self.quote_identifier(old)
            )
        } else {
            self.quote_identifier(old)
        };
        let n = if let Some(s) = schema {
            format!(
                "{}.{}",
                self.quote_identifier(s),
                self.quote_identifier(new)
            )
        } else {
            self.quote_identifier(new)
        };
        Ok(format!("ALTER TABLE {} RENAME TO {}", o, n))
    }

    fn generate_enum_types_sql(&self, model: &ModelDescriptor) -> Result<Vec<String>> {
        let mut stmts = Vec::new();
        // Generate CREATE TYPE statements for each unique enum in columns
        let mut seen = std::collections::HashSet::new();
        for col in &model.columns {
            if let DataType::Enum(name, variants) = &col.data_type {
                if seen.insert(name.clone()) {
                    // Qualify type name with schema if present
                    let type_name = if let Some(schema) = &model.schema {
                        format!("{}.{}", schema, name)
                    } else {
                        name.clone()
                    };
                    let quoted = self.quote_identifier(&type_name);
                    // Escape and quote each variant
                    let vals: Vec<String> = variants
                        .iter()
                        .map(|v| format!("'{}'", self.escape_string_literal(v)))
                        .collect();
                    let stmt = format!("CREATE TYPE {} AS ENUM ({})", quoted, vals.join(", "));
                    stmts.push(stmt);
                }
            }
        }
        Ok(stmts)
    }

    fn identifier_quote_char_start(&self) -> char {
        '"'
    }
    fn identifier_quote_char_end(&self) -> char {
        '"'
    }
    fn escape_string_literal(&self, v: &str) -> String {
        v.replace('\'', "''")
    }

    /// Generate SQL to drop a constraint from a table
    fn generate_drop_constraint_sql(
        &self,
        table: &str,
        constraint_name: &str,
        schema: Option<&str>,
    ) -> Result<String> {
        let full_table = if let Some(s) = schema {
            format!(
                "{}.{}",
                self.quote_identifier(s),
                self.quote_identifier(table)
            )
        } else {
            self.quote_identifier(table)
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
        _table: &str,
        index_name: &str,
        schema: Option<&str>,
    ) -> Result<String> {
        let full_index = if let Some(s) = schema {
            format!(
                "{}.{}",
                self.quote_identifier(s),
                self.quote_identifier(index_name)
            )
        } else {
            self.quote_identifier(index_name)
        };
        Ok(format!("DROP INDEX {}", full_index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_manager::definitions::DataType;

    #[test]
    fn test_map_common_data_type() {
        let a = PostgresAdapter::new();
        assert_eq!(
            a.map_common_data_type(&DataType::Boolean).unwrap(),
            "BOOLEAN"
        );
        assert_eq!(
            a.map_common_data_type(&DataType::Enum("my_enum".into(), vec!["a".into()]))
                .unwrap(),
            "my_enum"
        );
    }
}
