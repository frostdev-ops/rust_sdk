#[cfg(all(test, feature = "database", feature = "sqlite"))]
mod tests {
    use crate::Error;
    use crate::database::{
        DatabaseConfig, DatabaseType, DatabaseValue, PoolConfig, create_database_connection,
    };

    // Test that the database module can be used with the main SDK error type
    #[tokio::test]
    async fn test_database_error_integration() {
        // Create an invalid SQLite configuration
        let config = DatabaseConfig {
            db_type: DatabaseType::Sqlite,
            database: "/nonexistent/path/that/does/not/exist.db".to_string(),
            pool: PoolConfig::default(),
            ..Default::default()
        };

        // Attempt to create a connection - this should fail
        let result = create_database_connection(&config).await;
        assert!(result.is_err(), "Expected connection error but got success");

        // Convert the database error to the SDK's error type
        let sdk_error: Result<(), Error> = result.map(|_| ()).map_err(Error::from);
        assert!(sdk_error.is_err(), "Expected SDK error conversion to work");

        if let Err(Error::Database(_)) = sdk_error {
            // Good, we got the expected error variant
        } else {
            panic!("Expected Error::Database variant, got: {:?}", sdk_error);
        }
    }

    // Test in-memory SQLite database with the SDK
    #[tokio::test]
    async fn test_sqlite_in_memory() -> Result<(), Error> {
        // Create an in-memory SQLite configuration
        let config = DatabaseConfig {
            db_type: DatabaseType::Sqlite,
            database: ":memory:".to_string(),
            pool: PoolConfig::default(),
            ..Default::default()
        };

        // Create connection
        let db = create_database_connection(&config).await?;

        // Create a table
        db.execute("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)", &[])
            .await?;

        // Insert data
        db.execute(
            "INSERT INTO test (id, name) VALUES (?, ?)",
            &[
                DatabaseValue::Integer(1),
                DatabaseValue::Text("Test".to_string()),
            ],
        )
        .await?;

        // Query data
        let row = db
            .query_one(
                "SELECT id, name FROM test WHERE id = ?",
                &[DatabaseValue::Integer(1)],
            )
            .await?
            .expect("Failed to get test row");

        let id = row.get_i64("id")?;
        let name = row.get_string("name")?;

        assert_eq!(id, 1);
        assert_eq!(name, "Test");

        Ok(())
    }
}
