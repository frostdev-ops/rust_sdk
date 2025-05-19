// PyWatt SDK - Database Model Manager
//
// This module provides database-agnostic model definition and
// schema generation tools for SQL databases.

pub mod adapters;
#[cfg(feature = "database")]
pub mod config;
pub mod definitions;
pub mod errors;
pub mod generator;
pub mod sdk_integration;

#[cfg(all(test, feature = "integration_tests"))]
pub mod integration_tests;

// Re-export core types for convenience
pub use adapters::DatabaseAdapter;
#[cfg(feature = "database")]
pub use config::ModelManagerConfig;
pub use definitions::{
    ColumnDescriptor, Constraint, DataType, IndexDescriptor, IndexType, IntegerSize,
    ModelDescriptor, ReferentialAction,
};
pub use errors::{Error, Result};
pub use generator::ModelGenerator;
pub use sdk_integration::ModelManager;

// Public API functions
// Will be implemented in future milestone
// pub fn get_adapter_for_database_type(db_type: crate::database::DatabaseType) -> Box<dyn DatabaseAdapter>;
