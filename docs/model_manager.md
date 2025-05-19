# PyWatt SDK Model Manager

The Model Manager is a component of the PyWatt SDK that provides a universal database modeling toolkit. It allows developers to define database schemas (models, tables, columns, relationships) in a database-agnostic way and generate the appropriate Data Definition Language (DDL) statements for various SQL databases.

## Features

* **Database-Agnostic Modeling**: Define your database models once and use them with any supported database.
* **Type Safety**: Leverage Rust's type system for defining models.
* **Schema Generation**: Generate DDL statements for creating, altering, and dropping tables.
* **Schema Synchronization**: Add missing tables and columns to match your model definitions.
* **Integration with PyWatt Database API**: Apply models programmatically via the standard `DatabaseConnection` interface.

## Supported Databases

* **SQLite** - Full support
* **MySQL/MariaDB** - Full support
* **PostgreSQL** - Full support

## Usage

### Defining Models

Use the `ModelDescriptor` struct to define database tables:

```rust
use pywatt_sdk::model_manager::{
    definitions::{ModelDescriptor, ColumnDescriptor, DataType, IntegerSize},
};

// Define a user model
let user_model = ModelDescriptor {
    name: "users".to_string(),
    schema: None, // Optional schema name (e.g., "public" for PostgreSQL)
    columns: vec![
        ColumnDescriptor {
            name: "id".to_string(),
            data_type: DataType::Integer(IntegerSize::I32),
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
        ColumnDescriptor {
            name: "created_at".to_string(),
            data_type: DataType::TimestampTz,
            is_nullable: false,
            is_primary_key: false,
            is_unique: false,
            default_value: Some("CURRENT_TIMESTAMP".to_string()),
            auto_increment: false,
            comment: None,
            constraints: vec![],
        },
    ],
    primary_key: None, // Primary key is defined on the column
    indexes: vec![], // Optional indexes
    constraints: vec![], // Optional table-level constraints
    comment: None,
    engine: None, // Database-specific storage engine (e.g., "InnoDB" for MySQL)
    charset: None, // Database-specific character set
    collation: None, // Database-specific collation
    options: Default::default(), // Additional database-specific options
};
```

### Generating SQL DDL

You can use the `ModelGenerator` to generate SQL DDL statements:

```rust
use pywatt_sdk::model_manager::{
    generator::ModelGenerator,
    adapters::SqliteAdapter,
};

// Create an adapter for the target database
let adapter = SqliteAdapter::new();

// Create a model generator with the adapter
let generator = ModelGenerator::new(Box::new(adapter));

// Generate the SQL script for creating the table
let sql = generator.generate_create_table_script(&user_model).unwrap();
println!("{}", sql);

// Generate the SQL script for dropping the table
let drop_sql = generator.generate_drop_table_script("users", None).unwrap();
println!("{}", drop_sql);
```

### Using with Database Connection

The model manager integrates with the PyWatt database API via the `ModelManager` trait, which is automatically implemented for any type that implements `DatabaseConnection`.

```rust
use pywatt_sdk::database::{DatabaseConfig, create_database_connection};
use pywatt_sdk::model_manager::ModelManager;

async fn example() -> Result<(), Box<dyn std::error::Error>> {
    // Create a database connection
    let config = DatabaseConfig {
        db_type: DatabaseType::Sqlite,
        database: "my_database.db".to_string(),
        host: None,
        port: None,
        username: None,
        password: None,
        ssl_mode: None,
        pool: PoolConfig::default(),
        extra_params: std::collections::HashMap::new(),
    };
    let mut conn = create_database_connection(&config).await?;
    
    // Apply a model to the database (create the table)
    conn.apply_model(&user_model).await?;
    
    // Synchronize multiple models with the database
    // This will create missing tables and add missing columns to existing tables
    conn.sync_schema(&[user_model, post_model]).await?;
    
    // Drop a model from the database
    conn.drop_model("temporary_table", None).await?;
    
    Ok(())
}
```

### Schema Synchronization

The `sync_schema` method provides an intelligent way to synchronize your model definitions with the database:

1. For each model, it attempts to create the table
2. If the table already exists, it intelligently detects this from the error message
3. In future versions, it will also detect and add missing columns

This allows you to safely call `sync_schema` at application startup to ensure your database schema matches your model definitions without manual intervention.

### Command-Line Interface

The SDK includes a command-line tool for managing database models:

```bash
# Generate SQL script from model definitions
database-tool schema generate --model-file models.yaml --database-type sqlite --output schema.sql

# Apply schema to a database
database-tool schema apply --model-file models.yaml --database-config database.toml

# Validate model definitions
database-tool model validate --model-file models.yaml

# Generate Rust struct code from model definitions
database-tool model generate --model-file models.yaml --output models.rs

# Apply model definitions to a database
database-tool model apply --model-file models.yaml --database-config database.toml

# Drop a table from the database
database-tool model drop --table users --database-config database.toml
```

## YAML Model Definitions

You can define models in YAML:

```yaml
tables:
  - name: users
    schema: public  # Optional
    columns:
      - name: id
        data_type: Integer
        is_primary_key: true
        auto_increment: true
      - name: username
        data_type: Varchar(100)
        is_unique: true
        is_nullable: false
      - name: email
        data_type: Varchar(255)
        is_unique: true
        is_nullable: false
      - name: created_at
        data_type: TimestampTz
        default_value: "CURRENT_TIMESTAMP"
    indexes:
      - columns: [email]
```

## Technical Details

The Model Manager consists of:

1. **Universal Model Representation**: Rust structs and enums to define database models in a generic way.
2. **Database Adapter Trait**: A trait defining the interface for database-specific operations.
3. **Concrete Database Adapters**: Implementations of the adapter trait for each supported database (SQLite, MySQL, PostgreSQL).
4. **Model Generation Engine**: A service that uses the model representation and a chosen adapter to produce DDL.
5. **SDK Integration Adapters**: Extensions for the PyWatt database API to apply models programmatically.

The schema synchronization feature detects existing tables from error messages across all supported databases, with more advanced synchronization features planned for future releases.

## Getting Started

### Adding to Your Cargo.toml

```toml
[dependencies]
pywatt_sdk = { version = "0.2.x", features = ["database"] }
```

### Importing Required Types

```rust
use pywatt_sdk::database::{
    DatabaseConfig, create_database_connection, DatabaseType, PoolConfig
};
use pywatt_sdk::model_manager::{
    definitions::{DataType, IntegerSize, ColumnDescriptor, ModelDescriptor},
    ModelManager, // Extension trait for DatabaseConnection
};
```

## Available Data Types

The `DataType` enum supports a wide range of common database data types that are mapped to appropriate native types in each database:

```rust
// Common data types
DataType::Text(Option<u32>)     // Text with optional length
DataType::Varchar(u32)          // Variable-length string with max length
DataType::Char(u32)             // Fixed-length string
DataType::Integer(IntegerSize)  // Integer with specific size
DataType::SmallInt              // Small integer (16-bit)
DataType::BigInt                // Big integer (64-bit)
DataType::Boolean               // Boolean true/false
DataType::Float                 // Single-precision float
DataType::Double                // Double-precision float
DataType::Decimal(u8, u8)       // Decimal number with precision and scale
DataType::Date                  // Date only
DataType::Time                  // Time only
DataType::DateTime              // Date and time
DataType::Timestamp             // Timestamp
DataType::TimestampTz           // Timestamp with timezone
DataType::Blob                  // Binary data
DataType::Json                  // JSON data
DataType::JsonB                 // Binary JSON (PostgreSQL)
DataType::Uuid                  // UUID
DataType::Enum(String, Vec<String>)  // Enumeration type
DataType::Custom(String)        // Custom database-specific type
```

## Complete Example

See the [model_generation_example.rs](../examples/model_generation_example.rs) file for a complete working example.

## SQL Generation without Database Connection

You can also generate SQL statements without a database connection:

```rust
use pywatt_sdk::database::model_manager::{
    SqliteAdapter, ModelGenerator,
};

// Create an adapter for the target database
let adapter = SqliteAdapter::new();

// Create a model generator
let generator = ModelGenerator::new(Box::new(adapter));

// Generate SQL for the model
let sql = generator.generate_create_table_script(&user_model)?;
println!("{}", sql);
```

## Related Documentation

- [DATABASE_GUIDE.md](DATABASE_GUIDE.md) - General guide to using the database API
- [MODULE_CREATION_GUIDE.md](MODULE_CREATION_GUIDE.md) - Guide to creating PyWatt modules 