//! This example demonstrates database module integration with Axum
#![allow(dead_code, unused_imports)]

// This example demonstrates how to use the database protocol in a PyWatt module
#[cfg(feature = "database")]
use secrecy::ExposeSecret;
#[cfg(feature = "database")]
use std::collections::HashMap;
#[cfg(feature = "database")]
use std::sync::Arc;

#[cfg(feature = "database")]
use axum::{
    Json, Router,
    extract::{Extension, Path},
    routing::{get, post},
};
#[allow(unused_imports)]
use pywatt_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

// User data structure for serialization/deserialization
#[derive(Debug, Serialize, Deserialize)]
struct User {
    id: Option<i64>,
    name: String,
    email: String,
}

/* // Temporarily commenting out due to unimplemented proc macro
#[cfg(feature = "proc_macros")]
/// This is a PyWatt module that demonstrates database integration
/// It implements a simple user management API with database storage
#[module(
    secrets = ["DB_PASSWORD"],
    health = "/_health",
    metrics = true,
    version = "v1"
)]
async fn module(state: pywatt_sdk::AppState<()>) -> axum::Router {
    // Use the common implementation
    create_router(state).await
}
*/

// This function contains the actual module implementation, shared between
// the macro-based version and the manual version
#[cfg(feature = "database")]
async fn create_router(state: pywatt_sdk::AppState<()>) -> axum::Router {
    // Initialize the database connection
    let db = initialize_database(&state)
        .await
        .expect("Failed to initialize database");

    // Create the module router
    Router::new()
        .route("/users", get(list_users))
        .route("/users", post(create_user))
        .route("/users/:id", get(get_user))
        .layer(Extension(state))
        .layer(Extension(db))
}

#[cfg(feature = "database")]
/// Initialize the database connection and create tables if needed
async fn initialize_database(
    state: &pywatt_sdk::AppState<()>,
) -> pywatt_sdk::database::DatabaseResult<Arc<Box<dyn pywatt_sdk::database::DatabaseConnection>>> {
    // Get database password from secrets
    let db_password = get_secret(state.secret_client(), "DB_PASSWORD")
        .await
        .expect("Failed to get database password");

    // Create database configuration
    let config = pywatt_sdk::database::DatabaseConfig {
        db_type: pywatt_sdk::database::DatabaseType::Postgres,
        host: Some("localhost".to_string()),
        port: Some(5432),
        database: "pywatt_example".to_string(),
        username: Some("postgres".to_string()),
        password: Some(db_password.expose_secret().to_string()),
        ssl_mode: None,
        pool: pywatt_sdk::database::PoolConfig::default(),
        extra_params: HashMap::new(),
    };

    // Create database connection
    let db = create_database_connection(&config).await?;

    // Create tables if they don't exist
    db.execute(
        "CREATE TABLE IF NOT EXISTS users (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            email TEXT NOT NULL UNIQUE
        )",
        &[],
    )
    .await?;

    // Return the database connection wrapped in an Arc for sharing
    Ok(Arc::new(db))
}

#[cfg(feature = "database")]
/// Handler for listing all users
async fn list_users(
    Extension(db): Extension<Arc<Box<dyn pywatt_sdk::database::DatabaseConnection>>>,
) -> Json<Vec<User>> {
    // Query all users from the database
    let rows = db
        .query("SELECT id, name, email FROM users ORDER BY id", &[])
        .await
        .expect("Failed to query users");

    // Convert the rows to User objects
    let users = rows
        .into_iter()
        .map(|row| User {
            id: Some(row.get_i64("id").unwrap()),
            name: row.get_string("name").unwrap(),
            email: row.get_string("email").unwrap(),
        })
        .collect();

    Json(users)
}

#[cfg(feature = "database")]
/// Handler for creating a new user
async fn create_user(
    Extension(db): Extension<Arc<Box<dyn pywatt_sdk::database::DatabaseConnection>>>,
    Json(user): Json<User>,
) -> Json<User> {
    use pywatt_sdk::database::DatabaseValue;

    // Insert the new user into the database
    let row = db
        .query_one(
            "INSERT INTO users (name, email) VALUES ($1, $2) RETURNING id, name, email",
            &[
                DatabaseValue::Text(user.name),
                DatabaseValue::Text(user.email),
            ],
        )
        .await
        .expect("Failed to create user")
        .expect("No row returned after insert");

    // Return the newly created user with its ID
    Json(User {
        id: Some(row.get_i64("id").unwrap()),
        name: row.get_string("name").unwrap(),
        email: row.get_string("email").unwrap(),
    })
}

#[cfg(feature = "database")]
/// Handler for getting a user by ID
async fn get_user(
    Extension(db): Extension<Arc<Box<dyn pywatt_sdk::database::DatabaseConnection>>>,
    Path(id): Path<i64>,
) -> Json<Option<User>> {
    use pywatt_sdk::database::DatabaseValue;

    // Query the user by ID
    let user = db
        .query_one(
            "SELECT id, name, email FROM users WHERE id = $1",
            &[DatabaseValue::Integer(id)],
        )
        .await
        .expect("Failed to query user")
        .map(|row| User {
            id: Some(row.get_i64("id").unwrap()),
            name: row.get_string("name").unwrap(),
            email: row.get_string("email").unwrap(),
        });

    Json(user)
}

// Provide a main function that prints a helpful message if the required features aren't enabled
fn main() {
    #[cfg(not(all(feature = "database", feature = "proc_macros")))]
    {
        println!("This example requires the 'database' and 'proc_macros' features to be enabled.");
        println!("Please rebuild with: --features database,postgres,proc_macros");
    }

    #[cfg(all(feature = "database", feature = "proc_macros"))]
    {
        println!("This example is meant to be used with the PyWatt orchestrator.");
        println!("The pywatt::module macro handles the main function when used properly.");
    }
}
