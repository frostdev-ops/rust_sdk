use crate::cache::{CacheError, CacheResult, CacheService, CacheStats, CacheType};
use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

/// Cache-aside pattern implementation.
///
/// This pattern first attempts to retrieve data from the cache.
/// If not found, it fetches from the source and updates the cache before returning.
pub async fn cache_aside<T, F, Fut>(
    cache: &dyn CacheService,
    key: &str,
    source_fn: F,
    ttl: Option<Duration>,
) -> CacheResult<T>
where
    T: DeserializeOwned + Serialize + Send + Sync,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>>,
{
    // Try to get from cache first
    if let Some(data) = cache.get(key).await? {
        // Special case for String type - avoid JSON serialization overhead
        if std::any::type_name::<T>() == std::any::type_name::<String>() {
            if let Ok(s) = String::from_utf8(data.clone()) {
                // Convert directly by wrapping in a JSON value to satisfy the generic `T`.
                // This avoids any unsafe memory manipulation while still skipping the serde
                // round-trip for typical (non-String) types.
                let value: T =
                    serde_json::from_value(serde_json::Value::String(s)).map_err(|e| {
                        CacheError::Serialization(format!("Failed to deserialize string: {}", e))
                    })?;
                return Ok(value);
            }
        }

        // Standard JSON deserialization for other types
        match serde_json::from_slice(&data) {
            Ok(value) => return Ok(value),
            Err(e) => {
                // Log the error but continue to fetch from source
                tracing::warn!(key = %key, error = %e, "Failed to deserialize cached data");
            }
        }
    }

    // Not in cache or deserialize failed, fetch from source
    let value = source_fn()
        .await
        .map_err(|e| CacheError::Operation(format!("Failed to fetch data from source: {}", e)))?;

    // Special case for String type - store raw bytes
    if std::any::type_name::<T>() == std::any::type_name::<String>() {
        // This is unsafe but controlled - we're checking the type_name above
        let string_value: &String = unsafe { std::mem::transmute(&value) };

        // Set the value in the cache
        if let Err(e) = cache.set(key, string_value.as_bytes(), ttl).await {
            tracing::error!(key = %key, error = %e, "Failed to set data in cache");
        }
    } else {
        // Serialize other types normally
        let serialized = serde_json::to_vec(&value)
            .map_err(|e| CacheError::Serialization(format!("Failed to serialize data: {}", e)))?;

        // Set the value in the cache
        if let Err(e) = cache.set(key, &serialized, ttl).await {
            tracing::error!(key = %key, error = %e, "Failed to set data in cache");
        }
    }

    Ok(value)
}

/// Write-through pattern implementation.
///
/// This pattern updates both the cache and the source simultaneously.
/// It ensures cache and source consistency by updating both in a coordinated manner.
pub async fn write_through<T, F, Fut>(
    cache: &dyn CacheService,
    key: &str,
    value: &T,
    store_fn: F,
    ttl: Option<Duration>,
) -> CacheResult<()>
where
    T: Serialize + Send + Sync,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>>,
{
    // Store in source first
    store_fn()
        .await
        .map_err(|e| CacheError::Operation(format!("Failed to store data in source: {}", e)))?;

    // Special case for String type - store raw bytes
    if std::any::type_name::<T>() == std::any::type_name::<String>() {
        // This is unsafe but controlled - we're checking the type_name above
        let string_value: &String = unsafe { std::mem::transmute(value) };

        // Store in cache
        cache.set(key, string_value.as_bytes(), ttl).await?;
    } else {
        // Serialize the value for other types
        let serialized = serde_json::to_vec(value)
            .map_err(|e| CacheError::Serialization(format!("Failed to serialize data: {}", e)))?;

        // Store in cache
        cache.set(key, &serialized, ttl).await?;
    }

    Ok(())
}

/// Write-behind pattern implementation.
///
/// This pattern updates the cache immediately but writes to the backend source
/// asynchronously in the background.
pub async fn write_behind<T, F, Fut>(
    cache: &dyn CacheService,
    cache_key: &str,
    value: &T,
    backend_write: F,
    ttl: Option<Duration>,
) -> CacheResult<()>
where
    T: serde::Serialize + Send + Sync,
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'static,
{
    // Serialize the value
    let serialized = serde_json::to_vec(value)
        .map_err(|e| CacheError::Serialization(format!("Failed to serialize data: {}", e)))?;

    // Set in cache first
    cache.set(cache_key, &serialized, ttl).await?;

    // Create an owned copy of the key for the async task
    let cache_key = cache_key.to_string();

    // Then write to backend asynchronously - note we don't reference 'cache' inside the spawned task
    tokio::spawn(async move {
        if let Err(e) = backend_write().await {
            tracing::error!(key = %cache_key, error = %e, "Failed to save data to backend");
        }
    });

    Ok(())
}

/// Cache Invalidation Strategy Trait
///
/// Defines the interface for cache invalidation strategies.
#[async_trait]
pub trait InvalidationStrategy: Send + Sync {
    /// Generate a cache key for the given entity
    fn generate_key(&self, entity_type: &str, entity_id: &str) -> String;

    /// Invalidate a cached entity
    async fn invalidate(
        &self,
        cache: &dyn CacheService,
        entity_type: &str,
        entity_id: &str,
    ) -> CacheResult<()>;

    /// Invalidate all entities of a certain type
    async fn invalidate_type(&self, cache: &dyn CacheService, entity_type: &str)
    -> CacheResult<()>;
}

/// TTL (Time-To-Live) based invalidation strategy
///
/// This strategy relies on cache entries expiring naturally after a configured TTL.
#[derive(Debug, Clone)]
pub struct TtlInvalidationStrategy {
    /// Key prefix to use
    pub prefix: String,
    /// Default TTL for cache entries
    pub default_ttl: Duration,
}

impl TtlInvalidationStrategy {
    /// Create a new TTL invalidation strategy
    pub fn new(prefix: &str, default_ttl: Duration) -> Self {
        Self {
            prefix: prefix.to_string(),
            default_ttl,
        }
    }
}

#[async_trait]
impl InvalidationStrategy for TtlInvalidationStrategy {
    fn generate_key(&self, entity_type: &str, entity_id: &str) -> String {
        format!("{}:{}:{}", self.prefix, entity_type, entity_id)
    }

    async fn invalidate(
        &self,
        cache: &dyn CacheService,
        entity_type: &str,
        entity_id: &str,
    ) -> CacheResult<()> {
        let key = self.generate_key(entity_type, entity_id);
        let _ = cache.delete(&key).await?;
        Ok(())
    }

    async fn invalidate_type(
        &self,
        cache: &dyn CacheService,
        entity_type: &str,
    ) -> CacheResult<()> {
        // This is a simple strategy that relies on TTL, so we don't need pattern matching
        // for bulk invalidation. For more complex strategies, we would implement pattern matching.
        let pattern = format!("{}:{}:*", self.prefix, entity_type);
        cache.clear(Some(&pattern)).await
    }
}

/// Event-based invalidation strategy
///
/// This strategy explicitly invalidates cache entries in response to specific events.
#[derive(Debug, Clone)]
pub struct EventInvalidationStrategy {
    /// Key prefix to use
    pub prefix: String,
    /// Default TTL for cache entries
    pub default_ttl: Duration,
    /// Whether to use versioned keys
    pub use_versioning: bool,
}

impl EventInvalidationStrategy {
    /// Create a new event-based invalidation strategy
    pub fn new(prefix: &str, default_ttl: Duration, use_versioning: bool) -> Self {
        Self {
            prefix: prefix.to_string(),
            default_ttl,
            use_versioning,
        }
    }

    /// Generate a versioned key if versioning is enabled
    pub async fn versioned_key(
        &self,
        cache: &dyn CacheService,
        entity_type: &str,
    ) -> CacheResult<String> {
        if !self.use_versioning {
            return Ok(self.prefix.clone());
        }

        let version_key = format!("{}:{}_version", self.prefix, entity_type);
        let version = cache.increment(&version_key, 1).await?;
        Ok(format!("{}:v{}", self.prefix, version))
    }
}

#[async_trait]
impl InvalidationStrategy for EventInvalidationStrategy {
    fn generate_key(&self, entity_type: &str, entity_id: &str) -> String {
        if self.use_versioning {
            // This is a placeholder - actual implementation depends on runtime version
            format!("{}:{}:{}", self.prefix, entity_type, entity_id)
        } else {
            format!("{}:{}:{}", self.prefix, entity_type, entity_id)
        }
    }

    async fn invalidate(
        &self,
        cache: &dyn CacheService,
        entity_type: &str,
        entity_id: &str,
    ) -> CacheResult<()> {
        if self.use_versioning {
            // Increment type version to effectively invalidate all entities of this type
            let version_key = format!("{}:{}_version", self.prefix, entity_type);
            let _ = cache.increment(&version_key, 1).await?;
        } else {
            // Direct invalidation
            let key = self.generate_key(entity_type, entity_id);
            let _ = cache.delete(&key).await?;
        }
        Ok(())
    }

    async fn invalidate_type(
        &self,
        cache: &dyn CacheService,
        entity_type: &str,
    ) -> CacheResult<()> {
        if self.use_versioning {
            // Increment type version to effectively invalidate all entities
            let version_key = format!("{}:{}_version", self.prefix, entity_type);
            let _ = cache.increment(&version_key, 1).await?;
            Ok(())
        } else {
            // Direct pattern invalidation
            let pattern = format!("{}:{}:*", self.prefix, entity_type);
            cache.clear(Some(&pattern)).await
        }
    }
}

/// LRU (Least Recently Used) invalidation helper for in-memory caches
///
/// This is primarily used with the in-memory cache implementation, which
/// manages its own LRU eviction.
#[derive(Debug, Clone)]
pub struct LruInvalidationStrategy {
    /// Key prefix to use
    pub prefix: String,
    /// Maximum entries to keep
    pub max_entries: usize,
}

impl LruInvalidationStrategy {
    /// Create a new LRU invalidation strategy
    pub fn new(prefix: &str, max_entries: usize) -> Self {
        Self {
            prefix: prefix.to_string(),
            max_entries,
        }
    }
}

#[async_trait]
impl InvalidationStrategy for LruInvalidationStrategy {
    fn generate_key(&self, entity_type: &str, entity_id: &str) -> String {
        format!("{}:{}:{}", self.prefix, entity_type, entity_id)
    }

    async fn invalidate(
        &self,
        cache: &dyn CacheService,
        entity_type: &str,
        entity_id: &str,
    ) -> CacheResult<()> {
        let key = self.generate_key(entity_type, entity_id);
        let _ = cache.delete(&key).await?;
        Ok(())
    }

    async fn invalidate_type(
        &self,
        cache: &dyn CacheService,
        entity_type: &str,
    ) -> CacheResult<()> {
        let pattern = format!("{}:{}:*", self.prefix, entity_type);
        cache.clear(Some(&pattern)).await
    }
}

/// Distributed lock implementation with retry capability
pub struct DistributedLock {
    /// Cache service to use for locking
    cache: Arc<dyn CacheService>,
    /// Lock key
    key: String,
    /// Lock token received upon successful acquisition
    token: Option<String>,
    /// Lock TTL
    ttl: Duration,
    /// Retry configuration
    retry_config: RetryConfig,
}

/// Retry configuration for distributed locks
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Delay between retry attempts
    pub retry_delay: Duration,
    /// Whether to use exponential backoff
    pub use_backoff: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            retry_delay: Duration::from_millis(100),
            use_backoff: true,
        }
    }
}

impl DistributedLock {
    /// Create a new distributed lock that takes either a direct Arc<dyn CacheService>
    /// or an Arc<Box<dyn CacheService>> from the factory function
    pub fn new(
        cache: Arc<Box<dyn CacheService>>,
        key: &str,
        ttl: Duration,
        retry_config: Option<RetryConfig>,
    ) -> Self {
        // We need to map from Arc<Box<dyn CacheService>> to Arc<dyn CacheService>
        // We'll create a new wrapper type for this purpose
        let cache_service = Arc::new(CacheServiceWrapper(cache));

        Self {
            cache: cache_service,
            key: format!("lock:{}", key),
            token: None,
            ttl,
            retry_config: retry_config.unwrap_or_default(),
        }
    }

    /// Try to acquire the lock
    pub async fn try_acquire(&mut self) -> CacheResult<bool> {
        // Generate a unique token for this lock - unused since lock() generates the token
        let _token = uuid::Uuid::new_v4().to_string();

        // Try to acquire the lock
        if let Some(token_str) = self.cache.lock(&self.key, self.ttl).await? {
            self.token = Some(token_str);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Acquire the lock, retrying if necessary
    pub async fn acquire(&mut self) -> CacheResult<()> {
        let mut attempts = 0;
        let mut delay = self.retry_config.retry_delay;

        while attempts < self.retry_config.max_attempts {
            if self.try_acquire().await? {
                return Ok(());
            }

            attempts += 1;
            if attempts >= self.retry_config.max_attempts {
                break;
            }

            // Wait before retrying
            tokio::time::sleep(delay).await;

            // Increase delay for next attempt if using backoff
            if self.retry_config.use_backoff {
                delay *= 2;
            }
        }

        Err(CacheError::Internal(format!(
            "Failed to acquire lock '{}' after {} attempts",
            self.key, self.retry_config.max_attempts
        )))
    }

    /// Release the lock
    pub async fn release(&mut self) -> CacheResult<bool> {
        if let Some(token) = &self.token {
            let result = self.cache.unlock(&self.key, token).await?;
            self.token = None;
            Ok(result)
        } else {
            // Not holding the lock
            Ok(false)
        }
    }

    /// Check if we are holding the lock
    pub fn is_acquired(&self) -> bool {
        self.token.is_some()
    }

    /// Execute a function while holding the lock
    pub async fn with_lock<F, Fut, T, E>(&mut self, f: F) -> Result<T, LockError<E>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: std::error::Error + Send + Sync + 'static,
    {
        // Try to acquire the lock
        self.acquire().await.map_err(LockError::Acquisition)?;

        // Execute the function
        let result = f().await;

        // Always release the lock, even if the function failed
        let _ = self.release().await;

        // Return the function result
        result.map_err(LockError::Operation)
    }
}

/// Lock error type
#[derive(Debug)]
pub enum LockError<E> {
    /// Failed to acquire the lock
    Acquisition(CacheError),
    /// Error in the operation executed while holding the lock
    Operation(E),
    /// Failed to release the lock
    Release(CacheError),
}

impl<E: std::fmt::Display> std::fmt::Display for LockError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Acquisition(e) => write!(f, "Failed to acquire lock: {}", e),
            Self::Operation(e) => write!(f, "Operation error: {}", e),
            Self::Release(e) => write!(f, "Failed to release lock: {}", e),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for LockError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Acquisition(e) => Some(e),
            Self::Operation(e) => Some(e),
            Self::Release(e) => Some(e),
        }
    }
}

// Convert from LockError<E> to CacheError
impl<E: std::fmt::Display> From<LockError<E>> for CacheError {
    fn from(error: LockError<E>) -> Self {
        match error {
            LockError::Acquisition(e) => e,
            LockError::Operation(e) => CacheError::Operation(format!("Operation error: {}", e)),
            LockError::Release(e) => e,
        }
    }
}

/// Wrapper to adapt Arc<Box<dyn CacheService>> to Arc<dyn CacheService>
struct CacheServiceWrapper(Arc<Box<dyn CacheService>>);

#[async_trait]
impl CacheService for CacheServiceWrapper {
    async fn get(&self, key: &str) -> CacheResult<Option<Vec<u8>>> {
        self.0.get(key).await
    }

    async fn set(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> CacheResult<()> {
        self.0.set(key, value, ttl).await
    }

    async fn delete(&self, key: &str) -> CacheResult<bool> {
        self.0.delete(key).await
    }

    async fn flush(&self) -> CacheResult<()> {
        self.0.flush().await
    }

    async fn stats(&self) -> CacheResult<CacheStats> {
        self.0.stats().await
    }

    async fn ping(&self) -> CacheResult<()> {
        self.0.ping().await
    }

    async fn close(&self) -> CacheResult<()> {
        self.0.close().await
    }

    async fn exists(&self, key: &str) -> CacheResult<bool> {
        self.0.exists(key).await
    }

    async fn set_nx(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> CacheResult<bool> {
        self.0.set_nx(key, value, ttl).await
    }

    async fn get_set(&self, key: &str, value: &[u8]) -> CacheResult<Option<Vec<u8>>> {
        self.0.get_set(key, value).await
    }

    async fn increment(&self, key: &str, delta: i64) -> CacheResult<i64> {
        self.0.increment(key, delta).await
    }

    async fn decrement(&self, key: &str, delta: i64) -> CacheResult<i64> {
        self.0.decrement(key, delta).await
    }

    async fn set_many(
        &self,
        items: &std::collections::HashMap<String, Vec<u8>>,
        ttl: Option<Duration>,
    ) -> CacheResult<()> {
        self.0.set_many(items, ttl).await
    }

    async fn get_many(
        &self,
        keys: &[String],
    ) -> CacheResult<std::collections::HashMap<String, Vec<u8>>> {
        self.0.get_many(keys).await
    }

    async fn delete_many(&self, keys: &[String]) -> CacheResult<u64> {
        self.0.delete_many(keys).await
    }

    async fn clear(&self, namespace: Option<&str>) -> CacheResult<()> {
        self.0.clear(namespace).await
    }

    async fn lock(&self, key: &str, ttl: Duration) -> CacheResult<Option<String>> {
        // In a real Redis/Memcached implementation, this would use SET NX
        // For InMemory, this could use DashMap entry API or a Mutex
        self.0.lock(key, ttl).await
    }

    async fn unlock(&self, key: &str, token: &str) -> CacheResult<bool> {
        self.0.unlock(key, token).await
    }

    fn get_cache_type(&self) -> CacheType {
        self.0.get_cache_type()
    }

    fn get_default_ttl(&self) -> Duration {
        self.0.get_default_ttl()
    }
}
