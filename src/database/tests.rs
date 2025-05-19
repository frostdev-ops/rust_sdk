// Unit tests for the database module
// Note: These only test functionality that doesn't require a real database connection

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_database_type_display() {
        assert_eq!(DatabaseType::Postgres.to_string(), "postgres");
        assert_eq!(DatabaseType::MySql.to_string(), "mysql");
        assert_eq!(DatabaseType::Sqlite.to_string(), "sqlite");
    }

    #[test]
    fn test_pool_config_defaults() {
        let config = PoolConfig::default();
        assert_eq!(config.max_connections, 10);
        assert_eq!(config.min_connections, 2);
        assert_eq!(config.idle_timeout_seconds, 300);
        assert_eq!(config.max_lifetime_seconds, 1800);
        assert_eq!(config.acquire_timeout_seconds, 30);
    }

    #[test]
    fn test_database_config_serialization() {
        let mut extra_params = HashMap::new();
        extra_params.insert("mode".to_string(), "readwrite".to_string());

        let config = DatabaseConfig {
            db_type: DatabaseType::Postgres,
            host: Some("localhost".to_string()),
            port: Some(5432),
            database: "testdb".to_string(),
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
            ssl_mode: Some("prefer".to_string()),
            pool: PoolConfig::default(),
            extra_params,
        };

        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: DatabaseConfig = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.db_type, DatabaseType::Postgres);
        assert_eq!(deserialized.host, Some("localhost".to_string()));
        assert_eq!(deserialized.port, Some(5432));
        assert_eq!(deserialized.database, "testdb");
        assert_eq!(deserialized.username, Some("user".to_string()));
        assert_eq!(deserialized.password, Some("pass".to_string()));
        assert_eq!(deserialized.ssl_mode, Some("prefer".to_string()));
        assert_eq!(deserialized.extra_params.get("mode"), Some(&"readwrite".to_string()));
    }

    #[test]
    fn test_database_value_variants() {
        let null = DatabaseValue::Null;
        let boolean = DatabaseValue::Boolean(true);
        let integer = DatabaseValue::Integer(42);
        let float = DatabaseValue::Float(3.14);
        let text = DatabaseValue::Text("test".to_string());
        let blob = DatabaseValue::Blob(vec![1, 2, 3]);
        let array = DatabaseValue::Array(vec![
            DatabaseValue::Integer(1),
            DatabaseValue::Integer(2),
        ]);

        // Just verify they can be created and are different types
        assert!(matches!(null, DatabaseValue::Null));
        assert!(matches!(boolean, DatabaseValue::Boolean(_)));
        assert!(matches!(integer, DatabaseValue::Integer(_)));
        assert!(matches!(float, DatabaseValue::Float(_)));
        assert!(matches!(text, DatabaseValue::Text(_)));
        assert!(matches!(blob, DatabaseValue::Blob(_)));
        assert!(matches!(array, DatabaseValue::Array(_)));
    }
}

// Integration tests (disabled by default, require actual database connections)
#[cfg(all(test, feature = "postgres", feature = "integration_tests"))]
mod postgres_tests {
    use super::*;
    use crate::database::PostgresConnection;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_postgres_connection() {
        let config = DatabaseConfig {
            db_type: DatabaseType::Postgres,
            host: Some("localhost".to_string()),
            port: Some(5432),
            database: "postgres".to_string(), // Default database in PostgreSQL
            username: Some("postgres".to_string()),
            password: Some("postgres".to_string()),
            ssl_mode: None,
            pool: PoolConfig::default(),
            extra_params: HashMap::new(),
        };

        let conn = create_database_connection(&config).await;
        assert!(conn.is_ok(), "Failed to create PostgreSQL connection: {:?}", conn.err());

        let conn = conn.unwrap();
        let result = conn.ping().await;
        assert!(result.is_ok(), "Failed to ping PostgreSQL: {:?}", result.err());

        // Test execute query
        let result = conn.execute("SELECT 1", &[]).await;
        assert!(result.is_ok(), "Failed to execute query: {:?}", result.err());

        // Test query
        let result = conn.query("SELECT 1 as num", &[]).await;
        assert!(result.is_ok(), "Failed to query: {:?}", result.err());
        let rows = result.unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        let value: i32 = row.get("num").unwrap();
        assert_eq!(value, 1);

        // Test transaction
        let tx = conn.begin_transaction().await;
        assert!(tx.is_ok(), "Failed to begin transaction: {:?}", tx.err());
        let tx = tx.unwrap();
        let result = tx.execute("SELECT 1", &[]).await;
        assert!(result.is_ok(), "Failed to execute in transaction: {:?}", result.err());
        let result = tx.commit().await;
        assert!(result.is_ok(), "Failed to commit transaction: {:?}", result.err());

        // Test close
        let result = conn.close().await;
        assert!(result.is_ok(), "Failed to close connection: {:?}", result.err());
    }
}

#[cfg(all(test, feature = "mysql", feature = "integration_tests"))]
mod mysql_tests {
    use super::*;
    use crate::database::MySqlConnection;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_mysql_connection() {
        let config = DatabaseConfig {
            db_type: DatabaseType::MySql,
            host: Some("localhost".to_string()),
            port: Some(3306),
            database: "mysql".to_string(), // Default database in MySQL
            username: Some("root".to_string()),
            password: Some("mysql".to_string()),
            ssl_mode: None,
            pool: PoolConfig::default(),
            extra_params: HashMap::new(),
        };

        let conn = create_database_connection(&config).await;
        assert!(conn.is_ok(), "Failed to create MySQL connection: {:?}", conn.err());

        let conn = conn.unwrap();
        let result = conn.ping().await;
        assert!(result.is_ok(), "Failed to ping MySQL: {:?}", result.err());

        // Test close
        let result = conn.close().await;
        assert!(result.is_ok(), "Failed to close connection: {:?}", result.err());
    }
}

#[cfg(all(test, feature = "sqlite", feature = "integration_tests"))]
mod sqlite_tests {
    use super::*;
    use crate::database::SqliteConnection;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_sqlite_connection() {
        let config = DatabaseConfig {
            db_type: DatabaseType::Sqlite,
            host: None,
            port: None,
            database: ":memory:".to_string(), // In-memory database
            username: None,
            password: None,
            ssl_mode: None,
            pool: PoolConfig::default(),
            extra_params: HashMap::new(),
        };

        let conn = create_database_connection(&config).await;
        assert!(conn.is_ok(), "Failed to create SQLite connection: {:?}", conn.err());

        let conn = conn.unwrap();
        let result = conn.ping().await;
        assert!(result.is_ok(), "Failed to ping SQLite: {:?}", result.err());

        // Test execute query
        let result = conn.execute("CREATE TABLE test (id INTEGER, name TEXT)", &[]).await;
        assert!(result.is_ok(), "Failed to create table: {:?}", result.err());

        let result = conn.execute("INSERT INTO test (id, name) VALUES (1, 'test')", &[]).await;
        assert!(result.is_ok(), "Failed to insert: {:?}", result.err());

        // Test query
        let result = conn.query("SELECT * FROM test", &[]).await;
        assert!(result.is_ok(), "Failed to query: {:?}", result.err());
        let rows = result.unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        let id: i64 = row.get("id").unwrap();
        let name: String = row.get("name").unwrap();
        assert_eq!(id, 1);
        assert_eq!(name, "test");

        // Test close
        let result = conn.close().await;
        assert!(result.is_ok(), "Failed to close connection: {:?}", result.err());
    }
}