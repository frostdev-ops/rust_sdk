//! CLI tool for managing database models and schemas

use clap::{Parser, Subcommand, ValueEnum};
use pywatt_sdk::model_manager::ModelDescriptor;
use serde::Deserialize;
#[allow(unused_imports)]
use std::fs::File;
#[allow(unused_imports)]
use std::io::{self, Read, Write};
use std::path::PathBuf;

#[cfg(feature = "database")]
use pywatt_sdk::database::{DatabaseConfig, DatabaseType, create_database_connection};
#[cfg(feature = "database")]
use pywatt_sdk::model_manager::adapters::SqliteAdapter;
#[cfg(feature = "database")]
use pywatt_sdk::model_manager::definitions::DataType;
#[cfg(feature = "database")]
use pywatt_sdk::model_manager::{
    ModelManager, config::ModelManagerConfig, generator::ModelGenerator,
};
#[cfg(feature = "database")]
#[cfg(feature = "database")]
#[cfg(feature = "database")]

/// Entry point for the database-tool CLI
#[derive(Parser)]
#[command(name = "database-tool")]
#[command(about = "CLI for generating and applying database schemas from model definitions", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Schema-related commands
    Schema {
        #[command(subcommand)]
        action: SchemaAction,
    },
    /// Model-related commands
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
}

#[derive(Subcommand)]
enum SchemaAction {
    /// Generate SQL DDL script from model definitions
    Generate {
        /// Path to the model definition file (YAML or JSON)
        #[arg(short, long, value_name = "FILE")]
        model_file: PathBuf,
        /// Target database type (sqlite, mysql, postgres)
        #[arg(short = 't', long, value_enum)]
        database_type: DatabaseTypeArg,
        /// Output file for the generated SQL script ('-' for stdout)
        #[arg(short, long, value_name = "OUTPUT", default_value = "-")]
        output: PathBuf,
    },
    /// Apply schema to a database
    Apply {
        /// Path to the model definition file (YAML or JSON)
        #[arg(short, long, value_name = "FILE")]
        model_file: PathBuf,
        /// Path to the database configuration file (TOML)
        #[arg(short, long, value_name = "CONFIG")]
        database_config: PathBuf,
    },
}

#[derive(Subcommand)]
enum ModelAction {
    /// Validate model definitions by generating SQL (using SQLite adapter)
    Validate {
        /// Path to the model definition file (YAML or JSON)
        #[arg(short, long, value_name = "FILE")]
        model_file: PathBuf,
    },
    /// Generate Rust struct code from model definitions
    Generate {
        /// Path to the model definition file (YAML or JSON)
        #[arg(short, long, value_name = "FILE")]
        model_file: PathBuf,
        /// Output file for the generated Rust code ('-' for stdout)
        #[arg(short, long, value_name = "OUTPUT", default_value = "-")]
        output: PathBuf,
    },
    /// Apply model definitions to a database (create or alter tables)
    Apply {
        /// Path to the model definition file (YAML or JSON)
        #[arg(short, long, value_name = "FILE")]
        model_file: PathBuf,
        /// Path to the database configuration file (TOML)
        #[arg(short, long, value_name = "CONFIG")]
        database_config: PathBuf,
    },
    /// Drop a table from the database
    Drop {
        /// Name of the table to drop
        #[arg(short, long, value_name = "TABLE")]
        table: String,
        /// Optional schema name (e.g., "public" for PostgreSQL)
        #[arg(short, long, value_name = "SCHEMA")]
        schema: Option<String>,
        /// Path to the database configuration file (TOML)
        #[arg(short, long, value_name = "CONFIG")]
        database_config: PathBuf,
    },
}

#[derive(ValueEnum, Clone)]
enum DatabaseTypeArg {
    Sqlite,
    Mysql,
    Postgres,
}

#[cfg(feature = "database")]
impl From<DatabaseTypeArg> for DatabaseType {
    fn from(arg: DatabaseTypeArg) -> Self {
        match arg {
            DatabaseTypeArg::Sqlite => DatabaseType::Sqlite,
            DatabaseTypeArg::Mysql => DatabaseType::MySql,
            DatabaseTypeArg::Postgres => DatabaseType::Postgres,
        }
    }
}

#[derive(Deserialize)]
struct ModelsFile {
    #[allow(dead_code)]
    tables: Vec<ModelDescriptor>,
}

#[cfg(feature = "database")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Helper to convert snake_case to PascalCase for struct names
    #[cfg(feature = "database")]
    fn to_pascal_case(s: &str) -> String {
        s.split('_')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect()
    }

    match cli.command {
        Commands::Schema { action } => match action {
            SchemaAction::Generate {
                model_file,
                database_type,
                output,
            } => {
                // Read model definitions
                let mut file = File::open(&model_file)?;
                let mut content = String::new();
                file.read_to_string(&mut content)?;
                let models: ModelsFile =
                    serde_yaml::from_str(&content).or_else(|_| serde_json::from_str(&content))?;

                // Initialize adapter and generator
                let db_type: DatabaseType = database_type.into();
                let config = ModelManagerConfig::new(db_type);
                let adapter = config.get_adapter()?;
                let generator = ModelGenerator::new(adapter);

                // Generate SQL for each model
                let mut script = String::new();
                for model in &models.tables {
                    let sql = generator.generate_create_table_script(model)?;
                    script.push_str(&sql);
                    if !sql.trim_end().ends_with(';') {
                        script.push_str(";\n");
                    }
                    script.push('\n');
                }

                // Write output
                if output.to_string_lossy() == "-" {
                    io::stdout().write_all(script.as_bytes())?;
                } else {
                    let mut out_file = File::create(&output)?;
                    out_file.write_all(script.as_bytes())?;
                }
            }
            SchemaAction::Apply {
                model_file,
                database_config,
            } => {
                // Read model definitions
                let mut mf = File::open(&model_file)?;
                let mut mc = String::new();
                mf.read_to_string(&mut mc)?;
                let models: ModelsFile =
                    serde_yaml::from_str(&mc).or_else(|_| serde_json::from_str(&mc))?;

                // Read database configuration
                let mut df = File::open(&database_config)?;
                let mut dc = String::new();
                df.read_to_string(&mut dc)?;
                let db_config: DatabaseConfig = toml::from_str(&dc)?;

                // Establish connection
                let mut conn = create_database_connection(&db_config).await?;

                // Synchronize schema
                conn.sync_schema(&models.tables).await?;
            }
        },
        Commands::Model { action } => match action {
            ModelAction::Validate { model_file } => {
                let mut file = File::open(&model_file)?;
                let mut content = String::new();
                file.read_to_string(&mut content)?;
                let models: ModelsFile =
                    serde_yaml::from_str(&content).or_else(|_| serde_json::from_str(&content))?;

                // Validate by generating SQL with SQLite adapter
                let adapter = SqliteAdapter::new();
                let generator = ModelGenerator::new(Box::new(adapter));
                for model in &models.tables {
                    generator.generate_create_table_script(model)?;
                }
                println!("Model file '{}' is valid.", model_file.display());
            }
            ModelAction::Generate { model_file, output } => {
                // Read model definitions
                let mut mf = File::open(&model_file)?;
                let mut mc = String::new();
                mf.read_to_string(&mut mc)?;
                let models: ModelsFile =
                    serde_yaml::from_str(&mc).or_else(|_| serde_json::from_str(&mc))?;

                // Generate Rust code for each model
                let mut code = String::new();
                code.push_str("use serde::{Serialize, Deserialize};\n");
                code.push_str("use serde_json::Value;\n\n");
                for model in &models.tables {
                    let struct_name = to_pascal_case(&model.name);
                    code.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
                    code.push_str(&format!("pub struct {} {{\n", struct_name));
                    for col in &model.columns {
                        let rust_ty = match &col.data_type {
                            DataType::Integer(_) => "i64",
                            DataType::SmallInt => "i16",
                            DataType::BigInt => "i64",
                            DataType::Boolean => "bool",
                            DataType::Float | DataType::Double => "f64",
                            DataType::Text(_) | DataType::Varchar(_) | DataType::Char(_) => {
                                "String"
                            }
                            DataType::Blob => "Vec<u8>",
                            DataType::Json | DataType::JsonB => "Value",
                            DataType::Uuid => "String",
                            DataType::Decimal(_, _) => "String",
                            DataType::Date
                            | DataType::Time
                            | DataType::DateTime
                            | DataType::Timestamp
                            | DataType::TimestampTz => "String",
                            DataType::Enum(_, _) | DataType::Custom(_) => "String",
                        };
                        let field_ty = if col.is_nullable {
                            format!("Option<{}>", rust_ty)
                        } else {
                            rust_ty.to_string()
                        };
                        code.push_str(&format!("    pub {}: {},\n", col.name, field_ty));
                    }
                    code.push_str("}\n\n");
                }
                // Write output
                if output.to_string_lossy() == "-" {
                    io::stdout().write_all(code.as_bytes())?;
                } else {
                    let mut out_file = File::create(&output)?;
                    out_file.write_all(code.as_bytes())?;
                }
            }
            ModelAction::Apply {
                model_file,
                database_config,
            } => {
                // Read model definitions
                let mut mf = File::open(&model_file)?;
                let mut mc = String::new();
                mf.read_to_string(&mut mc)?;
                let models: ModelsFile =
                    serde_yaml::from_str(&mc).or_else(|_| serde_json::from_str(&mc))?;

                // Read database configuration
                let mut df = File::open(&database_config)?;
                let mut dc = String::new();
                df.read_to_string(&mut dc)?;
                let db_config: DatabaseConfig = toml::from_str(&dc)?;

                // Establish connection and apply schema
                let mut conn = create_database_connection(&db_config).await?;
                conn.sync_schema(&models.tables).await?;
                println!(
                    "Applied models from '{}' successfully.",
                    model_file.display()
                );
            }
            ModelAction::Drop {
                table,
                schema,
                database_config,
            } => {
                // Read database configuration
                let mut df = File::open(&database_config)?;
                let mut dc = String::new();
                df.read_to_string(&mut dc)?;
                let db_config: DatabaseConfig = toml::from_str(&dc)?;

                // Establish connection and drop table
                let mut conn = create_database_connection(&db_config).await?;
                conn.drop_model(&table, schema.as_deref()).await?;
                println!("Dropped table '{}' successfully.", table);
            }
        },
    }

    Ok(())
}

#[cfg(not(feature = "database"))]
fn main() {
    eprintln!("Error: the 'database' feature must be enabled to use this CLI tool.");
}
