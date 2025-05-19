// This example demonstrates how to use the database protocol in a standalone application
// (not running as a PyWatt module)
//
// Run with: cargo run --example standalone_db --features database,sqlite

#[allow(unused_imports)]
use std::f64::consts::PI;

#[cfg(feature = "database")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Import the database types
    use pywatt_sdk::database::{
        DatabaseConfig, DatabaseType, DatabaseValue, PoolConfig, create_database_connection,
    };

    println!("PyWatt SDK Database Example");
    println!("===========================");

    // Create an in-memory SQLite database configuration
    let config = DatabaseConfig {
        db_type: DatabaseType::Sqlite,
        database: ":memory:".to_string(), // Use in-memory database
        pool: PoolConfig::default(),
        host: None,
        port: None,
        username: None,
        password: None,
        ssl_mode: None,
        extra_params: std::collections::HashMap::new(),
    };

    // Create database connection
    println!("Connecting to database...");
    let db = create_database_connection(&config).await?;

    // Create a table
    println!("Creating table...");
    db.execute(
        "CREATE TABLE users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            email TEXT NOT NULL UNIQUE,
            active BOOLEAN NOT NULL DEFAULT 1
        )",
        &[],
    )
    .await?;

    // Insert data
    println!("Inserting data...");
    let users = vec![
        ("Alice", "alice@example.com"),
        ("Bob", "bob@example.com"),
        ("Charlie", "charlie@example.com"),
    ];

    for (name, email) in users {
        db.execute(
            "INSERT INTO users (name, email) VALUES (?, ?)",
            &[
                DatabaseValue::Text(name.to_string()),
                DatabaseValue::Text(email.to_string()),
            ],
        )
        .await?;
    }

    // Deactivate a user with a transaction
    println!("\nDeactivating Bob (using a transaction)...");
    let mut tx = db.begin_transaction().await?;

    tx.execute(
        "UPDATE users SET active = 0 WHERE email = ?",
        &[DatabaseValue::Text("bob@example.com".to_string())],
    )
    .await?;

    println!("Transaction successful, committing...");
    tx.commit().await?;

    // Query data
    println!("\nAll users:");
    let rows = db
        .query("SELECT id, name, email, active FROM users ORDER BY id", &[])
        .await?;

    println!("ID  | Name    | Email             | Active");
    println!("----|---------|-------------------|-------");

    for row in rows {
        let id: i64 = row.get_i64("id")?;
        let name: String = row.get_string("name")?;
        let email: String = row.get_string("email")?;
        let active: bool = row.get_bool("active")?;

        println!(
            "{:<3} | {:<7} | {:<17} | {}",
            id,
            name,
            email,
            if active { "Yes" } else { "No" }
        );
    }

    // Count active users
    println!("\nActive users count:");
    let row = db
        .query_one("SELECT COUNT(*) as count FROM users WHERE active = 1", &[])
        .await?
        .unwrap();

    let count: i64 = row.get_i64("count")?;
    println!("Active users: {}", count);

    // Demonstrate parameter types
    println!("\nDemonstrating parameter types...");
    db.execute(
        "CREATE TABLE demo (
            id INTEGER PRIMARY KEY,
            bool_val BOOLEAN,
            int_val INTEGER,
            float_val REAL,
            text_val TEXT,
            blob_val BLOB,
            null_val TEXT
        )",
        &[],
    )
    .await?;

    db.execute(
        "INSERT INTO demo VALUES (?, ?, ?, ?, ?, ?, ?)",
        &[
            DatabaseValue::Integer(1),                // id
            DatabaseValue::Boolean(true),             // bool_val
            DatabaseValue::Integer(42),               // int_val
            DatabaseValue::Float(PI),                 // float_val
            DatabaseValue::Text("Hello".to_string()), // text_val
            DatabaseValue::Blob(vec![1, 2, 3, 4]),    // blob_val
            DatabaseValue::Null,                      // null_val
        ],
    )
    .await?;

    let demo_row = db.query_one("SELECT * FROM demo", &[]).await?.unwrap();
    let id: i64 = demo_row.get_i64("id")?;
    let bool_val: bool = demo_row.get_bool("bool_val")?;
    let int_val: i64 = demo_row.get_i64("int_val")?;
    let float_val: f64 = demo_row.get_f64("float_val")?;
    let text_val: String = demo_row.get_string("text_val")?;

    // For NULL values, use try_get to get an Option
    let null_val: Option<String> = demo_row.try_get_string("null_val")?;

    println!("Demo row values:");
    println!("ID:      {}", id);
    println!("Boolean: {}", bool_val);
    println!("Integer: {}", int_val);
    println!("Float:   {}", float_val);
    println!("Text:    {}", text_val);
    println!("Null:    {:?}", null_val);

    // Demonstrate error handling with invalid query
    println!("\nDemonstrating error handling (with invalid query)...");
    match db.query("SELECT * FROM non_existent_table", &[]).await {
        Ok(_) => println!("This should not happen!"),
        Err(e) => println!("Expected error: {}", e),
    }

    // Close connection
    println!("\nClosing database connection...");
    db.close().await?;
    println!("Database connection closed successfully");

    Ok(())
}

// If the database feature is not enabled, print a message
#[cfg(not(feature = "database"))]
fn main() {
    println!("This example requires the 'database' feature to be enabled.");
    println!("Please rebuild with: cargo run --example standalone_db --features database,sqlite");
}
