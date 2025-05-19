#[allow(unused_imports)]
use crate::cache::{
    CacheConfig, CachePolicy, CacheService, CacheType, create_cache_service,
    in_memory::InMemoryCache,
    patterns::{cache_aside, write_through},
};
#[allow(unused_imports)]
use std::collections::HashMap;
#[allow(unused_imports)]
use std::time::Duration;

#[tokio::test]
async fn test_in_memory_cache_basic_operations() {
    // Create a cache config with all required fields
    let config = CacheConfig {
        cache_type: CacheType::InMemory,
        policy: CachePolicy::LRU,
        hosts: vec![],
        port: None,
        connection_timeout_seconds: 5,
        operation_timeout_seconds: 2,
        default_ttl_seconds: 300,
        default_ttl: None,
        username: None,
        password: None,
        pool: Default::default(),
        tls_enabled: false,
        file_path: None,
        max_size_bytes: None,
        namespace: Some("test".to_string()),
        database: None,
        extra_params: HashMap::new(),
    };

    // Create the cache service
    let cache = InMemoryCache::new(&config);

    // Set a value
    let key = "test-key";
    let value = b"test-value";
    cache.set(key, value, None).await.unwrap();

    // Get the value
    let result = cache.get(key).await.unwrap();
    assert_eq!(result, Some(value.to_vec()));

    // Get as string
    let string_result = cache.get_string(key).await.unwrap();
    assert_eq!(string_result, Some("test-value".to_string()));

    // Check if key exists
    let exists = cache.exists(key).await.unwrap();
    assert!(exists);

    // Delete the key
    let deleted = cache.delete(key).await.unwrap();
    assert!(deleted);

    // Verify it's gone
    let result = cache.get(key).await.unwrap();
    assert_eq!(result, None);

    // Test set_nx (only if doesn't exist)
    let key2 = "test-key2";

    // Should succeed as key doesn't exist
    let nx_result = cache.set_nx(key2, b"nx-value", None).await.unwrap();
    assert!(nx_result);

    // Should fail as key now exists
    let nx_result2 = cache.set_nx(key2, b"nx-value2", None).await.unwrap();
    assert!(!nx_result2);

    // Verify we still have the first value
    let result = cache.get(key2).await.unwrap();
    assert_eq!(result, Some(b"nx-value".to_vec()));
}

#[tokio::test]
async fn test_in_memory_cache_expiry() {
    // Create a cache config with a very short TTL
    let config = CacheConfig {
        cache_type: CacheType::InMemory,
        policy: CachePolicy::LRU,
        hosts: vec![],
        port: None,
        connection_timeout_seconds: 5,
        operation_timeout_seconds: 2,
        default_ttl_seconds: 1, // 1 second TTL
        default_ttl: None,
        username: None,
        password: None,
        pool: Default::default(),
        tls_enabled: false,
        file_path: None,
        max_size_bytes: None,
        namespace: Some("test".to_string()),
        database: None,
        extra_params: HashMap::new(),
    };

    // Create the cache service
    let cache = InMemoryCache::new(&config);

    // Set a value
    let key = "expiring-key";
    let value = b"expiring-value";
    cache.set(key, value, None).await.unwrap();

    // Get immediately should succeed
    let result = cache.get(key).await.unwrap();
    assert_eq!(result, Some(value.to_vec()));

    // Wait for expiry
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Get after expiry should be None
    let result = cache.get(key).await.unwrap();
    assert_eq!(result, None);
}

#[tokio::test]
async fn test_in_memory_cache_increment() {
    // Create a cache config
    let config = CacheConfig {
        cache_type: CacheType::InMemory,
        policy: CachePolicy::LRU,
        hosts: vec![],
        port: None,
        connection_timeout_seconds: 5,
        operation_timeout_seconds: 2,
        default_ttl_seconds: 300,
        default_ttl: None,
        username: None,
        password: None,
        pool: Default::default(),
        tls_enabled: false,
        file_path: None,
        max_size_bytes: None,
        namespace: Some("test".to_string()),
        database: None,
        extra_params: HashMap::new(),
    };

    // Create the cache service
    let cache = InMemoryCache::new(&config);

    // Increment a non-existing key
    let key = "counter";
    let result = cache.increment(key, 5).await.unwrap();
    assert_eq!(result, 5);

    // Increment again
    let result = cache.increment(key, 3).await.unwrap();
    assert_eq!(result, 8);

    // Decrement
    let result = cache.decrement(key, 2).await.unwrap();
    assert_eq!(result, 6);
}

#[tokio::test]
async fn test_in_memory_cache_bulk_operations() {
    // Create a cache config
    let config = CacheConfig {
        cache_type: CacheType::InMemory,
        policy: CachePolicy::LRU,
        hosts: vec![],
        port: None,
        connection_timeout_seconds: 5,
        operation_timeout_seconds: 2,
        default_ttl_seconds: 300,
        default_ttl: None,
        username: None,
        password: None,
        pool: Default::default(),
        tls_enabled: false,
        file_path: None,
        max_size_bytes: None,
        namespace: Some("test".to_string()),
        database: None,
        extra_params: HashMap::new(),
    };

    // Create the cache service
    let cache = InMemoryCache::new(&config);

    // Create multiple key-value pairs
    let mut items = HashMap::new();
    items.insert("key1".to_string(), b"value1".to_vec());
    items.insert("key2".to_string(), b"value2".to_vec());
    items.insert("key3".to_string(), b"value3".to_vec());

    // Set multiple values
    cache.set_many(&items, None).await.unwrap();

    // Get multiple values
    let keys = vec![
        "key1".to_string(),
        "key2".to_string(),
        "key3".to_string(),
        "key4".to_string(),
    ];
    let results = cache.get_many(&keys).await.unwrap();

    assert_eq!(results.len(), 3); // key4 doesn't exist
    assert_eq!(results.get("key1").unwrap(), &b"value1".to_vec());
    assert_eq!(results.get("key2").unwrap(), &b"value2".to_vec());
    assert_eq!(results.get("key3").unwrap(), &b"value3".to_vec());

    // Delete multiple keys
    let keys_to_delete = vec!["key1".to_string(), "key2".to_string(), "key4".to_string()];
    let delete_count = cache.delete_many(&keys_to_delete).await.unwrap();

    assert_eq!(delete_count, 2); // key4 doesn't exist

    // Verify key1 and key2 are gone, but key3 remains
    let exists1 = cache.exists("key1").await.unwrap();
    let exists2 = cache.exists("key2").await.unwrap();
    let exists3 = cache.exists("key3").await.unwrap();

    assert!(!exists1);
    assert!(!exists2);
    assert!(exists3);
}

#[tokio::test]
async fn test_in_memory_cache_namespacing() {
    // Create two caches with different namespaces
    let config1 = CacheConfig {
        cache_type: CacheType::InMemory,
        policy: CachePolicy::LRU,
        hosts: vec![],
        port: None,
        connection_timeout_seconds: 5,
        operation_timeout_seconds: 2,
        default_ttl_seconds: 300,
        default_ttl: None,
        username: None,
        password: None,
        pool: Default::default(),
        tls_enabled: false,
        file_path: None,
        max_size_bytes: None,
        namespace: Some("ns1".to_string()),
        database: None,
        extra_params: HashMap::new(),
    };

    let config2 = CacheConfig {
        cache_type: CacheType::InMemory,
        policy: CachePolicy::LRU,
        hosts: vec![],
        port: None,
        connection_timeout_seconds: 5,
        operation_timeout_seconds: 2,
        default_ttl_seconds: 300,
        default_ttl: None,
        username: None,
        password: None,
        pool: Default::default(),
        tls_enabled: false,
        file_path: None,
        max_size_bytes: None,
        namespace: Some("ns2".to_string()),
        database: None,
        extra_params: HashMap::new(),
    };

    let cache1 = InMemoryCache::new(&config1);
    let cache2 = InMemoryCache::new(&config2);

    // Set the same key in both caches
    let key = "shared-key";
    cache1.set(key, b"value1", None).await.unwrap();
    cache2.set(key, b"value2", None).await.unwrap();

    // Get from each cache
    let result1 = cache1.get(key).await.unwrap();
    let result2 = cache2.get(key).await.unwrap();

    // Should be different values due to namespacing
    assert_eq!(result1, Some(b"value1".to_vec()));
    assert_eq!(result2, Some(b"value2".to_vec()));

    // Clear one namespace
    cache1.clear(None).await.unwrap();

    // Cache1 should be empty, cache2 should still have the key
    let result1 = cache1.get(key).await.unwrap();
    let result2 = cache2.get(key).await.unwrap();

    assert_eq!(result1, None);
    assert_eq!(result2, Some(b"value2".to_vec()));
}

#[tokio::test]
async fn test_in_memory_cache_locking() {
    // Create a cache config
    let config = CacheConfig {
        cache_type: CacheType::InMemory,
        policy: CachePolicy::LRU,
        hosts: vec![],
        port: None,
        connection_timeout_seconds: 5,
        operation_timeout_seconds: 2,
        default_ttl_seconds: 300,
        default_ttl: None,
        username: None,
        password: None,
        pool: Default::default(),
        tls_enabled: false,
        file_path: None,
        max_size_bytes: None,
        namespace: Some("test".to_string()),
        database: None,
        extra_params: HashMap::new(),
    };

    // Create the cache service
    let cache = InMemoryCache::new(&config);

    // Acquire a lock
    let lock_key = "my-lock";
    let lock_ttl = Duration::from_secs(10);

    // First lock should succeed
    let lock_token = cache.lock(lock_key, lock_ttl).await.unwrap();
    assert!(lock_token.is_some());
    let token = lock_token.unwrap();

    // Second lock attempt should fail
    let lock_token2 = cache.lock(lock_key, lock_ttl).await.unwrap();
    assert!(lock_token2.is_none());

    // Release lock with wrong token should fail
    let unlock_result = cache.unlock(lock_key, "wrong-token").await.unwrap();
    assert!(!unlock_result);

    // Release lock with correct token should succeed
    let unlock_result = cache.unlock(lock_key, &token).await.unwrap();
    assert!(unlock_result);

    // Should be able to acquire lock again
    let lock_token3 = cache.lock(lock_key, lock_ttl).await.unwrap();
    assert!(lock_token3.is_some());
}

#[tokio::test]
async fn test_cache_factory() {
    // Create a cache config
    let config = CacheConfig {
        cache_type: CacheType::InMemory,
        policy: CachePolicy::LRU,
        hosts: vec![],
        port: None,
        connection_timeout_seconds: 5,
        operation_timeout_seconds: 2,
        default_ttl_seconds: 300,
        default_ttl: None,
        username: None,
        password: None,
        pool: Default::default(),
        tls_enabled: false,
        file_path: None,
        max_size_bytes: None,
        namespace: Some("test".to_string()),
        database: None,
        extra_params: HashMap::new(),
    };

    // Create the cache service using the factory
    let cache = create_cache_service(&config).await.unwrap();

    // Set a value
    let key = "factory-test";
    let value = b"factory-value";
    cache.set(key, value, None).await.unwrap();

    // Get the value
    let result = cache.get(key).await.unwrap();
    assert_eq!(result, Some(value.to_vec()));

    // Check the cache type
    assert_eq!(cache.get_cache_type(), CacheType::InMemory);
}

#[tokio::test]
async fn test_cache_patterns() {
    // Create a cache config
    let config = CacheConfig {
        cache_type: CacheType::InMemory,
        policy: CachePolicy::LRU,
        hosts: vec![],
        port: None,
        connection_timeout_seconds: 5,
        operation_timeout_seconds: 2,
        default_ttl_seconds: 300,
        default_ttl: None,
        username: None,
        password: None,
        pool: Default::default(),
        tls_enabled: false,
        file_path: None,
        max_size_bytes: None,
        namespace: Some("test".to_string()),
        database: None,
        extra_params: HashMap::new(),
    };

    // Create the cache service
    let cache = InMemoryCache::new(&config);

    // Test cache-aside pattern
    let key = "cache-aside-key";

    // First call should fetch from source
    let result: String = cache_aside(
        &cache,
        key,
        || async { Ok("source-value".to_string()) },
        None,
    )
    .await
    .unwrap();

    assert_eq!(result, "source-value");

    // Second call should hit cache
    let result2: String = cache_aside(
        &cache,
        key,
        || async {
            // This should not be called
            Ok("new-source-value".to_string())
        },
        None,
    )
    .await
    .unwrap();

    assert_eq!(result2, "source-value");

    // Test write-through pattern
    let write_key = "write-through-key";
    let value = "write-value".to_string();

    let mut saved_to_source = false;

    // Write to both cache and source
    write_through(
        &cache,
        write_key,
        &value,
        || async {
            saved_to_source = true;
            Ok(())
        },
        None,
    )
    .await
    .unwrap();

    // Verify value was saved to source
    assert!(saved_to_source);

    // Verify value was saved to cache
    let cached_value = cache.get_string(write_key).await.unwrap();
    assert_eq!(cached_value, Some(value));
}

#[tokio::test]
async fn test_cache_stats() {
    // Create a cache config
    let config = CacheConfig {
        cache_type: CacheType::InMemory,
        policy: CachePolicy::LRU,
        hosts: vec![],
        port: None,
        connection_timeout_seconds: 5,
        operation_timeout_seconds: 2,
        default_ttl_seconds: 300,
        default_ttl: None,
        username: None,
        password: None,
        pool: Default::default(),
        tls_enabled: false,
        file_path: None,
        max_size_bytes: None,
        namespace: Some("test".to_string()),
        database: None,
        extra_params: HashMap::new(),
    };

    // Create the cache service
    let cache = InMemoryCache::new(&config);

    // Make some operations to generate stats
    cache.set("key1", b"value1", None).await.unwrap();
    cache.set("key2", b"value2", None).await.unwrap();

    // Some hits
    cache.get("key1").await.unwrap();
    cache.get("key2").await.unwrap();
    cache.get("key2").await.unwrap();

    // Some misses
    cache.get("key3").await.unwrap();
    cache.get("key4").await.unwrap();

    // Get stats
    let stats = cache.stats().await.unwrap();

    assert_eq!(stats.item_count, Some(2));
    assert_eq!(stats.hits, Some(3));
    assert_eq!(stats.misses, Some(2));

    // Memory used should be non-zero
    assert!(stats.memory_used_bytes.unwrap() > 0);
}
