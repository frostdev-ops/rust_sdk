// This example demonstrates how to use the cache protocol in a standalone application
// (not running as a PyWatt module)
//
// Run with: cargo run --example cache_example --features cache

#[cfg(feature = "cache")]
use std::collections::HashMap;
#[cfg(feature = "cache")]
use std::sync::Arc;
#[cfg(feature = "cache")]
use std::time::Duration;

#[cfg(feature = "cache")]
use pywatt_sdk::cache::{
    CacheConfig, CachePolicy, CacheType, create_cache_service,
    patterns::{DistributedLock, cache_aside, write_through},
};

#[cfg(feature = "cache")]
use tokio::time::sleep;

#[cfg(feature = "cache")]
#[derive(serde::Serialize, serde::Deserialize)]
struct User {
    id: u64,
    name: String,
    email: String,
}

#[cfg(feature = "cache")]
// Simulates fetching a user from a database
async fn fetch_user_from_db(id: u64) -> Result<User, Box<dyn std::error::Error + Send + Sync>> {
    // In a real application, this would query a database
    println!("Fetching user {} from database", id);

    // Simulate database latency
    sleep(Duration::from_millis(500)).await;

    Ok(User {
        id,
        name: format!("User {}", id),
        email: format!("user{}@example.com", id),
    })
}

#[cfg(feature = "cache")]
// Simulates saving a user to a database
async fn save_user_to_db(user: &User) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // In a real application, this would save to a database
    println!("Saving user {} to database", user.id);

    // Simulate database latency
    sleep(Duration::from_millis(500)).await;

    Ok(())
}

#[cfg(feature = "cache")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an in-memory cache config
    let config = CacheConfig {
        cache_type: CacheType::InMemory,
        policy: CachePolicy::LRU,
        hosts: vec![],
        port: None,
        connection_timeout_seconds: 5,
        operation_timeout_seconds: 2,
        default_ttl_seconds: 60, // 1 minute TTL
        default_ttl: None,
        username: None,
        password: None,
        pool: Default::default(),
        tls_enabled: false,
        file_path: None,
        max_size_bytes: None,
        namespace: Some("example".to_string()),
        database: None,
        extra_params: HashMap::new(),
    };

    // Create the cache service
    let cache = create_cache_service(&config).await?;
    println!("Cache service created: {:?}", cache.get_cache_type());

    // 1. Basic cache operations
    println!("\n=== Basic Cache Operations ===");

    // Set a string value
    cache
        .set_string("greeting", "Hello, world!", Some(Duration::from_secs(60)))
        .await?;
    println!("Set 'greeting' in cache");

    // Get the string value
    let greeting = cache.get_string("greeting").await?;
    println!("Retrieved 'greeting' from cache: {:?}", greeting);

    // Delete the key
    let deleted = cache.delete("greeting").await?;
    println!("Deleted 'greeting' from cache: {}", deleted);

    // Verify it's gone
    let greeting = cache.get_string("greeting").await?;
    println!("After deletion, 'greeting' is: {:?}", greeting);

    // 2. Cache-aside pattern example
    println!("\n=== Cache-Aside Pattern ===");

    let user_id = 42;

    // First call - should fetch from "database"
    let user: User = cache_aside(
        &*cache,
        &format!("user:{}", user_id),
        || fetch_user_from_db(user_id),
        Some(Duration::from_secs(60)),
    )
    .await?;

    println!("First call result: User {} - {}", user.id, user.name);

    // Second call - should use cached value
    let cached_user: User = cache_aside(
        &*cache,
        &format!("user:{}", user_id),
        || {
            println!("This won't be printed if cache hit");
            fetch_user_from_db(user_id)
        },
        Some(Duration::from_secs(60)),
    )
    .await?;

    println!(
        "Second call result: User {} - {}",
        cached_user.id, cached_user.name
    );

    // 3. Write-through pattern example
    println!("\n=== Write-Through Pattern ===");

    let user = User {
        id: 123,
        name: "Jane Doe".to_string(),
        email: "jane@example.com".to_string(),
    };

    // Update user in both cache and "database"
    write_through(
        &*cache,
        &format!("user:{}", user.id),
        &user,
        || save_user_to_db(&user),
        Some(Duration::from_secs(60)),
    )
    .await?;

    println!("User {} saved to cache and database", user.id);

    // Read back from cache
    let cached_user: User = cache_aside(
        &*cache,
        &format!("user:{}", user.id),
        || {
            println!("This won't be printed if write-through worked");
            fetch_user_from_db(user.id)
        },
        None,
    )
    .await?;

    println!(
        "Read from cache: User {} - {}",
        cached_user.id, cached_user.name
    );

    // 4. Distributed locking example
    println!("\n=== Distributed Locking ===");

    // Simple locking using the CacheService trait
    println!("Attempting to acquire lock...");

    if let Some(token) = cache
        .lock("inventory:lock", Duration::from_secs(10))
        .await?
    {
        println!("Lock acquired with token: {}", token);

        // Simulate work
        println!("Performing critical operation...");
        sleep(Duration::from_secs(2)).await;

        // Release the lock
        let released = cache.unlock("inventory:lock", &token).await?;
        println!("Lock released: {}", released);
    } else {
        println!("Failed to acquire lock");
    }

    // Advanced locking using DistributedLock helper
    println!("\nUsing DistributedLock helper...");

    let cache_arc = Arc::new(cache);
    let mut lock = DistributedLock::new(
        Arc::clone(&cache_arc),
        "payment:lock",
        Duration::from_secs(30),
        None,
    );

    let result: Result<
        &'static str,
        pywatt_sdk::cache::patterns::LockError<std::convert::Infallible>,
    > = lock
        .with_lock(|| async {
            println!("Executing critical section with automatic lock management");
            sleep(Duration::from_secs(1)).await;
            Ok("Operation completed successfully")
        })
        .await;

    println!("Operation result: {:?}", result);

    // 5. Get cache statistics
    println!("\n=== Cache Statistics ===");

    let stats = cache_arc.stats().await?;
    println!("Cache stats: {:?}", stats);

    Ok(())
}

// If the cache feature is not enabled, print a message
#[cfg(not(feature = "cache"))]
fn main() {
    println!("This example requires the 'cache' feature to be enabled.");
    println!("Please rebuild with: cargo run --example cache_example --features cache");
}
