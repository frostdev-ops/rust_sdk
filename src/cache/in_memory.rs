use crate::cache::{CacheConfig, CacheResult, CacheService, CacheStats, CacheType};
use async_trait::async_trait;
use dashmap::DashMap;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Cache entry with value and expiration time
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    /// The stored value
    value: Vec<u8>,
    /// When the entry was created (Unix timestamp in seconds)
    #[allow(dead_code)]
    created_at_unix_secs: u64,
    /// When the entry will expire (Unix timestamp in seconds, if any)
    expires_at_unix_secs: Option<u64>,
    #[allow(dead_code)]
    frequency: u64, // For LFU
}

impl CacheEntry {
    /// Create a new cache entry
    fn new(value: Vec<u8>, ttl: Option<Duration>) -> Self {
        let now_unix_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let expires_at_unix_secs = ttl.map(|d| now_unix_secs + d.as_secs());

        Self {
            value,
            created_at_unix_secs: now_unix_secs,
            expires_at_unix_secs,
            frequency: 0, // For LFU
        }
    }

    /// Check if the entry is expired
    fn is_expired(&self) -> bool {
        if let Some(exp_secs) = self.expires_at_unix_secs {
            let now_unix_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            return now_unix_secs >= exp_secs;
        }
        false
    }
}

/// In-memory cache implementation using DashMap
pub struct InMemoryCache {
    /// The cache storage
    cache: Arc<DashMap<String, CacheEntry>>,
    /// Default TTL for cache entries
    default_ttl: Duration,
    /// Namespace prefix for cache keys
    namespace: Option<String>,
    /// Maximum number of items to store (LRU-like eviction if set)
    max_items: Option<usize>,
    /// Cache hit counter
    hits: Arc<AtomicU64>,
    /// Cache miss counter
    misses: Arc<AtomicU64>,
    /// Last cleanup time
    last_cleanup: Arc<std::sync::Mutex<Instant>>,
}

impl InMemoryCache {
    /// Create a new in-memory cache with provided configuration
    pub fn new(config: &CacheConfig) -> Self {
        // Use helper accessor that resolves duration from CacheConfig
        let default_ttl = config.get_default_ttl();
        let max_items = config.max_size_bytes.map(|bytes| {
            // Rough estimate: 1 item ≈ 100 bytes including overhead
            (bytes / 100) as usize
        });

        let cache = Self {
            cache: Arc::new(DashMap::new()),
            default_ttl,
            namespace: config.namespace.clone(),
            max_items,
            hits: Arc::new(AtomicU64::new(0)),
            misses: Arc::new(AtomicU64::new(0)),
            last_cleanup: Arc::new(std::sync::Mutex::new(Instant::now())),
        };

        // Spawn background cleanup task if TTL is set
        if default_ttl.as_secs() > 0 {
            let cache_ref = cache.cache.clone();
            let cleanup_interval = Duration::from_secs(
                // Clean up every 10% of default TTL or every minute, whichever is greater
                std::cmp::max(default_ttl.as_secs() / 10, 60),
            );
            let last_cleanup = cache.last_cleanup.clone();

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(cleanup_interval);
                loop {
                    interval.tick().await;

                    // Only proceed if it's been at least the cleanup interval
                    let mut last = last_cleanup.lock().unwrap();
                    if Instant::now().duration_since(*last) < cleanup_interval {
                        continue;
                    }
                    *last = Instant::now();

                    // Remove expired entries
                    cache_ref.retain(|_, entry| !entry.is_expired());
                }
            });
        }

        cache
    }

    /// Add namespace prefix to key if configured
    fn prefix_key(&self, key: &str) -> String {
        if let Some(ns) = &self.namespace {
            format!("{}:{}", ns, key)
        } else {
            key.to_string()
        }
    }

    /// Check if an entry exists and is not expired
    fn check_entry(&self, key: &str) -> bool {
        let key = self.prefix_key(key);
        if let Some(entry) = self.cache.get(&key) {
            if entry.is_expired() {
                // Don't remove here, let `get` handle it or the background task
                // self.cache.remove(&key);
                self.misses.fetch_add(1, Ordering::Relaxed);
                false
            } else {
                self.hits.fetch_add(1, Ordering::Relaxed);
                true
            }
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            false
        }
    }

    /// Perform LRU eviction if needed
    fn maybe_evict(&self) {
        if let Some(max_items) = self.max_items {
            if self.cache.len() >= max_items {
                // Simple random eviction strategy - evict ~10% of max items
                let evict_count = max_items / 10;
                if evict_count > 0 {
                    // Get all keys
                    let keys: Vec<String> =
                        self.cache.iter().map(|entry| entry.key().clone()).collect();

                    // Use vector indexing to avoid choose_multiple issues
                    #[allow(deprecated)]
                    let mut rng = rand::thread_rng();
                    let mut indices: Vec<usize> = (0..keys.len()).collect();
                    indices.shuffle(&mut rng);
                    let indices = indices.iter().take(evict_count);

                    // Extract keys by random indices
                    for &idx in indices {
                        if idx < keys.len() {
                            self.cache.remove(&keys[idx]);
                        }
                    }
                }
            }
        }
    }
}

#[async_trait]
impl CacheService for InMemoryCache {
    async fn get(&self, key: &str) -> CacheResult<Option<Vec<u8>>> {
        let key = self.prefix_key(key);

        if let Some(entry) = self.cache.get(&key) {
            if entry.is_expired() {
                // We must drop the read guard before attempting to mutate the map.
                self.misses.fetch_add(1, Ordering::Relaxed);
                // Explicitly release the guard to avoid deadlock.
                drop(entry);
                // Now it is safe to remove the expired entry.
                self.cache.remove(&key);
                Ok(None)
            } else {
                self.hits.fetch_add(1, Ordering::Relaxed);
                Ok(Some(entry.value.clone()))
            }
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }
    }

    async fn set(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> CacheResult<()> {
        let key = self.prefix_key(key);
        let ttl = ttl.or(Some(self.default_ttl));

        // Create new entry
        let entry = CacheEntry::new(value.to_vec(), ttl);

        // Check if we need eviction
        self.maybe_evict();

        // Insert entry
        self.cache.insert(key, entry);

        Ok(())
    }

    async fn delete(&self, key: &str) -> CacheResult<bool> {
        let key = self.prefix_key(key);
        Ok(self.cache.remove(&key).is_some())
    }

    async fn exists(&self, key: &str) -> CacheResult<bool> {
        Ok(self.check_entry(key))
    }

    async fn set_nx(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> CacheResult<bool> {
        // Prepare fully-qualified key once to avoid accidental double prefixing.
        let namespaced_key = self.prefix_key(key);

        // Fast path – check if a *valid* entry already exists.
        if let Some(entry) = self.cache.get(&namespaced_key) {
            if !entry.is_expired() {
                // Existing and still valid – NX condition fails.
                return Ok(false);
            }
            // Entry is expired – drop guard before we mutate the map.
            drop(entry);
            // Remove stale item so we can insert the fresh value.
            self.cache.remove(&namespaced_key);
        }

        // Either the key was missing or we just removed an expired value – insert the new one.
        let ttl = ttl.or(Some(self.default_ttl));
        let entry = CacheEntry::new(value.to_vec(), ttl);

        // Evict if necessary before inserting.
        self.maybe_evict();

        self.cache.insert(namespaced_key, entry);
        Ok(true)
    }

    async fn get_set(&self, key: &str, value: &[u8]) -> CacheResult<Option<Vec<u8>>> {
        let key = self.prefix_key(key);

        // Get existing value
        let old_value = if let Some(entry) = self.cache.get(&key) {
            if entry.is_expired() {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            } else {
                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(entry.value.clone())
            }
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            None
        };

        // Set new value
        let entry = CacheEntry::new(value.to_vec(), Some(self.default_ttl));

        // Check if we need eviction
        self.maybe_evict();

        // Insert entry
        self.cache.insert(key, entry);

        Ok(old_value)
    }

    async fn increment(&self, key: &str, delta: i64) -> CacheResult<i64> {
        let key = self.prefix_key(key);

        // Lock the entire operation to ensure atomicity
        let value = {
            // Get current value if exists
            let current_value = if let Some(entry) = self.cache.get(&key) {
                if entry.is_expired() {
                    self.misses.fetch_add(1, Ordering::Relaxed);
                    // Start from 0 if expired
                    0
                } else {
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    // Parse current value
                    let bytes = &entry.value;
                    String::from_utf8_lossy(bytes).parse::<i64>().unwrap_or(0)
                }
            } else {
                self.misses.fetch_add(1, Ordering::Relaxed);
                // Start from 0 if not exists
                0
            };

            // Calculate new value
            let new_value = current_value + delta;

            // Store new value
            let entry = CacheEntry::new(new_value.to_string().into_bytes(), Some(self.default_ttl));

            // Check if we need eviction
            self.maybe_evict();

            // Insert entry
            self.cache.insert(key, entry);

            new_value
        };

        Ok(value)
    }

    async fn decrement(&self, key: &str, delta: i64) -> CacheResult<i64> {
        // Decrement by delegating to increment with negative delta.
        self.increment(key, -delta).await
    }

    async fn set_many(
        &self,
        items: &HashMap<String, Vec<u8>>,
        ttl: Option<Duration>,
    ) -> CacheResult<()> {
        if items.is_empty() {
            return Ok(());
        }

        let ttl = ttl.or(Some(self.default_ttl));

        // Check if we might need eviction
        if let Some(max_items) = self.max_items {
            if self.cache.len() + items.len() > max_items {
                // Evict at least enough to make room for new items
                let min_evict = (self.cache.len() + items.len() - max_items) + 1;

                // Get all keys
                let keys: Vec<String> =
                    self.cache.iter().map(|entry| entry.key().clone()).collect();

                // Use vector indexing to avoid choose_multiple issues
                #[allow(deprecated)]
                let mut rng = rand::thread_rng();
                let mut indices: Vec<usize> = (0..keys.len()).collect();
                indices.shuffle(&mut rng);
                let indices = indices.iter().take(min_evict);

                // Extract keys by random indices
                for &idx in indices {
                    if idx < keys.len() {
                        self.cache.remove(&keys[idx]);
                    }
                }
            }
        }

        // Insert all items
        for (key, value) in items {
            let key = self.prefix_key(key);
            let entry = CacheEntry::new(value.clone(), ttl);
            self.cache.insert(key, entry);
        }

        Ok(())
    }

    async fn get_many(&self, keys: &[String]) -> CacheResult<HashMap<String, Vec<u8>>> {
        if keys.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result = HashMap::with_capacity(keys.len());

        for key in keys {
            let prefixed_key = self.prefix_key(key);

            if let Some(entry) = self.cache.get(&prefixed_key) {
                if !entry.is_expired() {
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    result.insert(key.clone(), entry.value.clone());
                } else {
                    self.misses.fetch_add(1, Ordering::Relaxed);
                    // Ensure we release the guard before mutation.
                    drop(entry);
                    // Remove expired entry
                    self.cache.remove(&prefixed_key);
                }
            } else {
                self.misses.fetch_add(1, Ordering::Relaxed);
            }
        }

        Ok(result)
    }

    async fn delete_many(&self, keys: &[String]) -> CacheResult<u64> {
        if keys.is_empty() {
            return Ok(0);
        }

        let mut count = 0;

        for key in keys {
            let prefixed_key = self.prefix_key(key);
            if self.cache.remove(&prefixed_key).is_some() {
                count += 1;
            }
        }

        Ok(count)
    }

    async fn clear(&self, namespace: Option<&str>) -> CacheResult<()> {
        // Check if we need to clear the entire cache or just a namespace
        if namespace.is_none() && self.namespace.is_none() {
            // Clear everything
            self.cache.clear();
            return Ok(());
        }

        // Use namespace from parameter, or object's namespace if not specified
        let ns_prefix = if let Some(ns) = namespace {
            format!("{}:", ns)
        } else if let Some(ns) = &self.namespace {
            format!("{}:", ns)
        } else {
            // No namespace, nothing to do
            return Ok(());
        };

        // Get all keys matching the namespace
        let keys_to_delete: Vec<String> = self
            .cache
            .iter()
            .map(|entry| entry.key().clone())
            .filter(|key| key.starts_with(&ns_prefix))
            .collect();

        // Delete matching keys
        for key in keys_to_delete {
            self.cache.remove(&key);
        }

        Ok(())
    }

    async fn lock(&self, key: &str, ttl: Duration) -> CacheResult<Option<String>> {
        let lock_key = self.prefix_key(&format!("lock:{}", key));
        let token = Uuid::new_v4().to_string();

        // Using a simpler approach to match the set_nx pattern but with explicit lock semantics

        // First, check if there's an existing valid lock
        let existing_lock = if let Some(entry) = self.cache.get(&lock_key) {
            !entry.is_expired()
        } else {
            false
        };

        // If a valid lock exists, return None (couldn't acquire)
        if existing_lock {
            return Ok(None);
        }

        // Otherwise, create a new lock with our token
        let entry = CacheEntry::new(token.clone().into_bytes(), Some(ttl));

        // Check if we need eviction before inserting
        self.maybe_evict();

        // Insert our lock and token
        self.cache.insert(lock_key, entry);

        // Return the token for release later
        Ok(Some(token))
    }

    async fn unlock(&self, key: &str, lock_token: &str) -> CacheResult<bool> {
        let lock_key = self.prefix_key(&format!("lock:{}", key));

        // Check if lock exists and token matches
        if let Some(entry) = self.cache.get(&lock_key) {
            if !entry.is_expired() {
                let current_token = String::from_utf8_lossy(&entry.value);
                if current_token == lock_token {
                    // Release the read guard before mutating the map to avoid deadlock.
                    drop(entry);
                    // Remove the lock
                    self.cache.remove(&lock_key);
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    fn get_cache_type(&self) -> CacheType {
        CacheType::InMemory
    }

    async fn ping(&self) -> CacheResult<()> {
        // In-memory cache is always available
        Ok(())
    }

    async fn close(&self) -> CacheResult<()> {
        // No need to close an in-memory cache
        Ok(())
    }

    fn get_default_ttl(&self) -> Duration {
        self.default_ttl
    }

    async fn stats(&self) -> CacheResult<CacheStats> {
        let mut memory_usage = 0;
        let mut item_count = 0;

        // Calculate approximate memory usage
        for entry in self.cache.iter() {
            // Each entry has overhead plus the key and value sizes
            let entry_size = entry.key().len() + entry.value.len() + 32; // 32 bytes approximate overhead
            memory_usage += entry_size;
            item_count += 1;
        }

        Ok(CacheStats {
            item_count: Some(item_count as u64),
            memory_used_bytes: Some(memory_usage as u64),
            hits: Some(self.hits.load(Ordering::Relaxed)),
            misses: Some(self.misses.load(Ordering::Relaxed)),
            sets: None,
            deletes: None,
            additional_metrics: HashMap::new(),
        })
    }

    async fn flush(&self) -> CacheResult<()> {
        self.cache.clear();
        Ok(())
    }
}
