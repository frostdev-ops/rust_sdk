use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::default::Default;

/// Integer size variants for the Integer data type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegerSize {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
}

/// Common database data types that can be mapped to specific databases
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DataType {
    /// Text with optional length
    Text(Option<u32>),
    /// Variable-length character string with specified maximum length
    Varchar(u32),
    /// Fixed-length character string
    Char(u32),
    /// Integer with specific size
    Integer(IntegerSize),
    /// Small integer (typically 16-bit)
    SmallInt,
    /// Big integer (typically 64-bit)
    BigInt,
    /// Boolean true/false
    Boolean,
    /// Single-precision floating point
    Float,
    /// Double-precision floating point
    Double,
    /// Decimal number with specified precision and scale
    Decimal(u8, u8),
    /// Date (without time)
    Date,
    /// Time (without date)
    Time,
    /// Date and time
    DateTime,
    /// Timestamp (often with time zone)
    Timestamp,
    /// Timestamp with time zone
    TimestampTz,
    /// Binary large object
    Blob,
    /// JSON data
    Json,
    /// PostgreSQL's binary JSON
    JsonB,
    /// UUID
    Uuid,
    /// Enumeration type with name and allowed values
    Enum(String, Vec<String>),
    /// Custom or database-specific type
    Custom(String),
}

/// Actions to take on foreign key references
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferentialAction {
    /// No action (default)
    NoAction,
    /// Restrict deletion/update
    Restrict,
    /// Cascade deletion/update
    Cascade,
    /// Set null on deletion/update
    SetNull,
    /// Set default value on deletion/update
    SetDefault,
}

/// Database constraints for columns or tables
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Constraint {
    /// Primary key constraint
    PrimaryKey {
        name: Option<String>,
        columns: Vec<String>,
    },
    /// Unique constraint
    Unique {
        name: Option<String>,
        columns: Vec<String>,
    },
    /// Not null constraint
    NotNull,
    /// Default value constraint
    DefaultValue(String),
    /// Check constraint
    Check {
        name: Option<String>,
        expression: String,
    },
    /// Foreign key constraint
    ForeignKey {
        name: Option<String>,
        columns: Vec<String>,
        references_table: String,
        references_columns: Vec<String>,
        on_delete: Option<ReferentialAction>,
        on_update: Option<ReferentialAction>,
    },
    /// Auto-increment constraint
    AutoIncrement,
}

/// Column definition for a database table
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnDescriptor {
    /// Column name
    pub name: String,
    /// Column data type
    pub data_type: DataType,
    /// Whether the column allows NULL values (default: true)
    #[serde(default = "default_true")]
    pub is_nullable: bool,
    /// Whether the column is a primary key (default: false)
    #[serde(default)]
    pub is_primary_key: bool,
    /// Whether the column has a unique constraint (default: false)
    #[serde(default)]
    pub is_unique: bool,
    /// Default value for the column
    pub default_value: Option<String>,
    /// Whether the column auto-increments (default: false)
    #[serde(default)]
    pub auto_increment: bool,
    /// Optional column comment
    pub comment: Option<String>,
    /// Additional column-specific constraints
    #[serde(default)]
    pub constraints: Vec<Constraint>,
}

/// Index types for different database engines
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexType {
    /// B-Tree index (default for most databases)
    BTree,
    /// Hash index
    Hash,
    /// PostgreSQL GIN index (for array and JSON)
    Gin,
    /// PostgreSQL GiST index (for geometry, full-text)
    Gist,
    /// Spatial index
    Spatial,
}

/// Index definition for a database table
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct IndexDescriptor {
    /// Optional index name (auto-generated if None)
    pub name: Option<String>,
    /// Columns included in the index
    pub columns: Vec<String>,
    /// Whether the index enforces uniqueness (default: false)
    #[serde(default)]
    pub is_unique: bool,
    /// Index type (database-specific)
    pub index_type: Option<IndexType>,
    /// Optional condition for partial indexes
    pub condition: Option<String>,
}

/// Complete table/model definition
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ModelDescriptor {
    /// Table name
    pub name: String,
    /// Schema name (e.g., "public" for PostgreSQL)
    pub schema: Option<String>,
    /// Column definitions
    pub columns: Vec<ColumnDescriptor>,
    /// Composite primary key (overrides is_primary_key on columns)
    pub primary_key: Option<Vec<String>>,
    /// Table indexes
    #[serde(default)]
    pub indexes: Vec<IndexDescriptor>,
    /// Table-level constraints
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    /// Optional table comment
    pub comment: Option<String>,
    /// Storage engine (e.g., "InnoDB" for MySQL)
    pub engine: Option<String>,
    /// Character set (e.g., "utf8mb4" for MySQL)
    pub charset: Option<String>,
    /// Collation (e.g., "utf8mb4_unicode_ci" for MySQL)
    pub collation: Option<String>,
    /// Additional database-specific options
    #[serde(default)]
    pub options: HashMap<String, String>,
}

/// Helper function to provide default true value for serde
fn default_true() -> bool {
    true
}

// Implement Default for model definition types to support `..Default::default()` usage
impl Default for ColumnDescriptor {
    fn default() -> Self {
        ColumnDescriptor {
            name: String::new(),
            data_type: DataType::Text(None),
            is_nullable: default_true(),
            is_primary_key: false,
            is_unique: false,
            default_value: None,
            auto_increment: false,
            comment: None,
            constraints: Vec::new(),
        }
    }
}

// Future implementation for Phase 4
/*
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RelationshipDescriptor {
    OneToOne,
    OneToMany,
    ManyToMany { through_table: String },
}
*/
