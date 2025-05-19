pub mod sqlite_adapter;
mod trait_def;

// Re-export the trait
pub use sqlite_adapter::SqliteAdapter;
pub use trait_def::DatabaseAdapter;

// These will be implemented in future milestones
pub mod mysql_adapter;
pub mod postgres_adapter;

pub use mysql_adapter::MySqlAdapter;
pub use postgres_adapter::PostgresAdapter;
