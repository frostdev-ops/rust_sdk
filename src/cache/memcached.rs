use crate::cache::{CacheConfig, CacheError, CacheResult, CacheService, CacheStats, CacheType};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

/// Memcached cache implementation
#[cfg(feature = "memcached")]
pub struct MemcachedCache {
    /// Memcached client
    #[allow(dead_code)]
    client: Arc<Mutex<memcache::Client>>,
    /// Default TTL for cache entries
    default_ttl: Duration,
    /// Namespace prefix for cache keys
    namespace: Option<String>,
    /// Cache hit counter
    #[allow(dead_code)]
    hits: Arc<AtomicU64>,
    /// Cache miss counter
    #[allow(dead_code)]
    misses: Arc<AtomicU64>,
}

#[cfg(feature = "memcached")]
impl MemcachedCache {
    /// Connect to Memcached using the provided configuration
    pub async fn connect(_config: &CacheConfig) -> CacheResult<Self> {
        #[cfg(feature = "memcached")]
        {
            // Build server list
            let servers = if _config.hosts.is_empty() {
                vec!["127.0.0.1".to_string()]
            } else {
                _config.hosts.clone()
            };

            // Add port if specified
            let servers_with_port: Vec<String> = servers
                .iter()
                .map(|host| {
                    if let Some(port) = _config.port {
                        format!("{}:{}", host, port)
                    } else {
                        // Default memcached port
                        format!("{}:11211", host)
                    }
                })
                .collect();

            // Join server strings for client creation
            let servers_str = servers_with_port.join(",");

            // Create client
            let client = memcache::Client::connect(servers_str).map_err(|e| {
                CacheError::Connection(format!("Failed to connect to Memcached: {}", e))
            })?;

            // Check connection with a ping
            client
                .stats()
                .map_err(|e| CacheError::Connection(format!("Failed to ping Memcached: {}", e)))?;

            Ok(Self {
                client: Arc::new(Mutex::new(client)),
                default_ttl: _config.get_default_ttl(),
                namespace: _config.namespace.clone(),
                hits: Arc::new(AtomicU64::new(0)),
                misses: Arc::new(AtomicU64::new(0)),
            })
        }

        #[cfg(not(feature = "memcached"))]
        {
            Err(CacheError::Connection(
                "Memcached support is not enabled. Recompile with the 'memcached' feature."
                    .to_string(),
            ))
        }
    }

    /// Add namespace prefix to key if configured
    fn prefix_key(&self, key: &str) -> String {
        if let Some(ns) = &self.namespace {
            format!("{}:{}", ns, key)
        } else {
            key.to_string()
        }
    }

    /// Strip namespace prefix from key if needed
    #[allow(dead_code)]
    fn strip_prefix(&self, key: &str) -> String {
        if let Some(ns) = &self.namespace {
            let prefix = format!("{}:", ns);
            if key.starts_with(&prefix) {
                key[prefix.len()..].to_string()
            } else {
                key.to_string()
            }
        } else {
            key.to_string()
        }
    }
}

#[cfg(feature = "memcached")]
#[async_trait]
impl CacheService for MemcachedCache {
    async fn get(&self, key: &str) -> CacheResult<Option<Vec<u8>>> {
        let _key = self.prefix_key(key);

        #[cfg(feature = "memcached")]
        {
            let client = self.client.lock().unwrap();
            match client.get::<Vec<u8>>(&_key) {
                Ok(value) => {
                    if value.is_some() {
                        self.hits.fetch_add(1, Ordering::Relaxed);
                    } else {
                        self.misses.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(value)
                }
                Err(e) => {
                    // Handle not found error gracefully
                    if e.to_string().contains("not found") {
                        self.misses.fetch_add(1, Ordering::Relaxed);
                        Ok(None)
                    } else {
                        Err(CacheError::Operation(format!("Memcached get error: {}", e)))
                    }
                }
            }
        }

        #[cfg(not(feature = "memcached"))]
        {
            Err(CacheError::Operation(
                "Memcached support is not enabled. Recompile with the 'memcached' feature."
                    .to_string(),
            ))
        }
    }

    async fn set(&self, key: &str, _value: &[u8], ttl: Option<Duration>) -> CacheResult<()> {
        let _key = self.prefix_key(key);
        let expiry = ttl.unwrap_or(self.default_ttl);
        let _expiry_secs = expiry.as_secs() as u32;

        #[cfg(feature = "memcached")]
        {
            let client = self.client.lock().unwrap();
            client
                .set(&_key, _value, _expiry_secs)
                .map_err(|e| CacheError::Operation(format!("Memcached set error: {}", e)))?;
            Ok(())
        }

        #[cfg(not(feature = "memcached"))]
        {
            Err(CacheError::Operation(
                "Memcached support is not enabled. Recompile with the 'memcached' feature."
                    .to_string(),
            ))
        }
    }

    async fn delete(&self, key: &str) -> CacheResult<bool> {
        let _key = self.prefix_key(key);

        #[cfg(feature = "memcached")]
        {
            let client = self.client.lock().unwrap();
            match client.delete(&_key) {
                Ok(_) => Ok(true),
                Err(e) => {
                    // Handle not found error gracefully
                    if e.to_string().contains("not found") {
                        Ok(false)
                    } else {
                        Err(CacheError::Operation(format!(
                            "Memcached delete error: {}",
                            e
                        )))
                    }
                }
            }
        }

        #[cfg(not(feature = "memcached"))]
        {
            Err(CacheError::Operation(
                "Memcached support is not enabled. Recompile with the 'memcached' feature."
                    .to_string(),
            ))
        }
    }

    async fn exists(&self, key: &str) -> CacheResult<bool> {
        // Memcached doesn't have a direct exists operation, so we use get
        // This will correctly use the cfg-gated get method.
        let result = self.get(key).await?;
        Ok(result.is_some())
    }

    async fn set_nx(&self, key: &str, _value: &[u8], ttl: Option<Duration>) -> CacheResult<bool> {
        let _key = self.prefix_key(key);
        let expiry = ttl.unwrap_or(self.default_ttl);
        let _expiry_secs = expiry.as_secs() as u32;

        #[cfg(feature = "memcached")]
        {
            let client = self.client.lock().unwrap();
            match client.add(&_key, _value, _expiry_secs) {
                Ok(_) => Ok(true),
                Err(e) => {
                    // Handle already exists error gracefully
                    if e.to_string().contains("already exists")
                        || e.to_string().contains("not stored")
                    {
                        Ok(false)
                    } else {
                        Err(CacheError::Operation(format!("Memcached add error: {}", e)))
                    }
                }
            }
        }

        #[cfg(not(feature = "memcached"))]
        {
            Err(CacheError::Operation(
                "Memcached support is not enabled. Recompile with the 'memcached' feature."
                    .to_string(),
            ))
        }
    }

    async fn get_set(&self, key: &str, _value: &[u8]) -> CacheResult<Option<Vec<u8>>> {
        let _key = self.prefix_key(key);

        #[cfg(feature = "memcached")]
        {
            // Memcached doesn't have a direct get-and-set operation
            // So we need to get first, then set
            let client = self.client.lock().unwrap();

            // Get the current value
            let current_value = match client.get::<Vec<u8>>(&_key) {
                Ok(value) => {
                    if value.is_some() {
                        self.hits.fetch_add(1, Ordering::Relaxed);
                    } else {
                        self.misses.fetch_add(1, Ordering::Relaxed);
                    }
                    value
                }
                Err(e) => {
                    // Handle not found error gracefully
                    if e.to_string().contains("not found") {
                        self.misses.fetch_add(1, Ordering::Relaxed);
                        None
                    } else {
                        return Err(CacheError::Operation(format!("Memcached get error: {}", e)));
                    }
                }
            };

            // Set the new value
            let expiry = self.default_ttl.as_secs() as u32;
            client
                .set(&_key, _value, expiry)
                .map_err(|e| CacheError::Operation(format!("Memcached set error: {}", e)))?;

            Ok(current_value)
        }

        #[cfg(not(feature = "memcached"))]
        {
            Err(CacheError::Operation(
                "Memcached support is not enabled. Recompile with the 'memcached' feature."
                    .to_string(),
            ))
        }
    }

    async fn increment(&self, key: &str, _delta: i64) -> CacheResult<i64> {
        let _key = self.prefix_key(key);

        #[cfg(feature = "memcached")]
        {
            let client = self.client.lock().unwrap();

            // Memcached doesn't handle negative increments directly
            let result = if _delta >= 0 {
                client.increment(&_key, _delta as u64)
            } else {
                // For decrement, we need to use the absolute value
                client.decrement(&_key, _delta.unsigned_abs())
            };

            match result {
                Ok(value) => Ok(value as i64),
                Err(e) => {
                    // Handle not found error - initialize with delta if positive, or 0 if negative
                    if e.to_string().contains("not found") {
                        let init_value = if _delta >= 0 { _delta } else { 0 };
                        let expiry = self.default_ttl.as_secs() as u32;
                        client
                            .set(&_key, init_value.to_string().as_bytes(), expiry)
                            .map_err(|e_set| {
                                // Renamed variable to avoid conflict
                                CacheError::Operation(format!(
                                    "Memcached set error during increment: {}",
                                    e_set
                                ))
                            })?;
                        Ok(init_value)
                    } else {
                        Err(CacheError::Operation(format!(
                            "Memcached increment error: {}",
                            e
                        )))
                    }
                }
            }
        }

        #[cfg(not(feature = "memcached"))]
        {
            Err(CacheError::Operation(
                "Memcached support is not enabled. Recompile with the 'memcached' feature."
                    .to_string(),
            ))
        }
    }

    async fn set_many(
        &self,
        items: &HashMap<String, Vec<u8>>,
        ttl: Option<Duration>,
    ) -> CacheResult<()> {
        if items.is_empty() {
            return Ok(());
        }

        let expiry = ttl.unwrap_or(self.default_ttl);
        let expiry_secs = expiry.as_secs() as u32;

        #[cfg(feature = "memcached")]
        {
            let client = self.client.lock().unwrap();

            // Memcached client doesn't have a direct multi-set, so we set each individually
            for (item_key, value) in items {
                // Renamed key to avoid conflict
                let prefixed_item_key = self.prefix_key(item_key); // Use renamed key
                client
                    .set(&prefixed_item_key, value.as_slice(), expiry_secs)
                    .map_err(|e| {
                        CacheError::Operation(format!("Memcached set error in set_many: {}", e))
                    })?;
            }
            Ok(())
        }

        #[cfg(not(feature = "memcached"))]
        {
            Err(CacheError::Operation(
                "Memcached support is not enabled. Recompile with the 'memcached' feature."
                    .to_string(),
            ))
        }
    }

    async fn get_many(&self, keys: &[String]) -> CacheResult<HashMap<String, Vec<u8>>> {
        if keys.is_empty() {
            return Ok(HashMap::new());
        }

        #[cfg(feature = "memcached")]
        {
            let mut result_map = HashMap::new();
            let client = self.client.lock().unwrap();

            for key_str in keys {
                // Renamed key to avoid conflict
                let prefixed_key_str = self.prefix_key(key_str);

                match client.get(&prefixed_key_str) {
                    Ok(Some(value)) => {
                        result_map.insert(self.strip_prefix(key_str), value);
                        self.hits.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(None) => {
                        self.misses.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        if e.to_string().contains("not found") {
                            self.misses.fetch_add(1, Ordering::Relaxed);
                        } else {
                            return Err(CacheError::Operation(format!(
                                "Memcached get error in get_many: {}",
                                e
                            )));
                        }
                    }
                }
            }
            Ok(result_map)
        }

        #[cfg(not(feature = "memcached"))]
        {
            Err(CacheError::Operation(
                "Memcached support is not enabled. Recompile with the 'memcached' feature."
                    .to_string(),
            ))
        }
    }

    async fn delete_many(&self, keys: &[String]) -> CacheResult<u64> {
        if keys.is_empty() {
            return Ok(0);
        }

        #[cfg(feature = "memcached")]
        {
            let client = self.client.lock().unwrap();
            let mut count = 0;

            for key_str in keys {
                // Renamed key to avoid conflict
                let prefixed_key_str = self.prefix_key(key_str);
                match client.delete(&prefixed_key_str) {
                    Ok(_) => count += 1,
                    Err(e) => {
                        if !e.to_string().contains("not found") {
                            return Err(CacheError::Operation(format!(
                                "Memcached delete error in delete_many: {}",
                                e
                            )));
                        }
                    }
                }
            }
            Ok(count)
        }

        #[cfg(not(feature = "memcached"))]
        {
            Err(CacheError::Operation(
                "Memcached support is not enabled. Recompile with the 'memcached' feature."
                    .to_string(),
            ))
        }
    }

    async fn clear(&self, _namespace: Option<&str>) -> CacheResult<()> {
        // For Memcached, flushing a specific namespace isn't directly supported.
        // The `flush_all` command clears the entire cache for the connected server(s).
        // If a namespace is provided, and it matches the configured namespace, we flush.
        // If no namespace is provided, or it doesn't match, this is a no-op to prevent
        // accidental full cache flushes unless the cache is configured without a namespace
        // or the provided namespace matches the global one.
        #[cfg(feature = "memcached")]
        {
            if (_namespace.is_some() && self.namespace.as_deref() == _namespace)
                || (_namespace.is_none() && self.namespace.is_none())
            {
                let client = self.client.lock().unwrap();
                client
                    .flush()
                    .map_err(|e| CacheError::Operation(format!("Memcached flush error: {}", e)))?;
                return Ok(());
            }

            Err(CacheError::Operation(
                "Clearing specific namespace not directly supported by Memcached implementation or namespace mismatch".to_string(),
            ))
        }

        #[cfg(not(feature = "memcached"))]
        {
            Err(CacheError::Operation(
                "Memcached support is not enabled. Recompile with the 'memcached' feature."
                    .to_string(),
            ))
        }
    }

    async fn lock(&self, key: &str, ttl: Duration) -> CacheResult<Option<String>> {
        // This method uses self.set_nx, which is now correctly cfg-gated.
        let lock_key = self.prefix_key(&format!("lock:{}", key));
        let token = Uuid::new_v4().to_string();

        if self.set_nx(&lock_key, token.as_bytes(), Some(ttl)).await? {
            Ok(Some(token))
        } else {
            Ok(None)
        }
    }

    async fn unlock(&self, key: &str, lock_token: &str) -> CacheResult<bool> {
        // This method uses self.get and self.delete, which are now correctly cfg-gated.
        let lock_key = self.prefix_key(&format!("lock:{}", key));

        let current_token_opt = self.get(&lock_key).await?;

        match current_token_opt {
            Some(data) => {
                let current_token = String::from_utf8_lossy(&data).to_string();
                if current_token == lock_token {
                    self.delete(&lock_key).await
                } else {
                    Ok(false) // Token doesn't match
                }
            }
            None => Ok(false), // Lock doesn't exist
        }
    }

    fn get_cache_type(&self) -> CacheType {
        CacheType::Memcached
    }

    async fn ping(&self) -> CacheResult<()> {
        #[cfg(feature = "memcached")]
        {
            let client = self.client.lock().unwrap();
            client
                .stats() // Ping is often done via a lightweight command like stats
                .map_err(|e| CacheError::Connection(format!("Failed to ping Memcached: {}", e)))?;
            Ok(())
        }

        #[cfg(not(feature = "memcached"))]
        {
            Err(CacheError::Connection(
                "Memcached support is not enabled. Recompile with the 'memcached' feature."
                    .to_string(),
            ))
        }
    }

    async fn close(&self) -> CacheResult<()> {
        // No explicit close needed for memcache client
        Ok(())
    }

    fn get_default_ttl(&self) -> Duration {
        self.default_ttl
    }

    async fn stats(&self) -> CacheResult<CacheStats> {
        #[cfg(feature = "memcached")]
        {
            let client = self.client.lock().unwrap();
            match client.stats() {
                Ok(stats_map) => {
                    let mut stats = CacheStats {
                        item_count: None,
                        memory_used_bytes: None,
                        hits: Some(self.hits.load(Ordering::Relaxed)),
                        misses: Some(self.misses.load(Ordering::Relaxed)),
                        sets: None,    // Memcached stats might not directly provide 'sets'
                        deletes: None, // Memcached stats might not directly provide 'deletes'
                        additional_metrics: HashMap::new(),
                        ..Default::default()
                    };

                    for (server, server_stats) in stats_map {
                        for (stat_key, value) in server_stats {
                            // Renamed key to avoid conflict
                            match stat_key.as_str() {
                                "curr_items" => {
                                    if let Ok(count) = value.parse::<u64>() {
                                        stats.item_count =
                                            Some(stats.item_count.unwrap_or(0) + count);
                                    }
                                }
                                "bytes" => {
                                    if let Ok(bytes) = value.parse::<u64>() {
                                        stats.memory_used_bytes =
                                            Some(stats.memory_used_bytes.unwrap_or(0) + bytes);
                                    }
                                }
                                // Capturing more detailed stats per server
                                _ => {
                                    stats
                                        .additional_metrics
                                        .insert(format!("{}.{}", server, stat_key), value);
                                }
                            }
                        }
                    }
                    Ok(stats)
                }
                Err(e) => Err(CacheError::Operation(format!(
                    "Failed to get Memcached stats: {}",
                    e
                ))),
            }
        }

        #[cfg(not(feature = "memcached"))]
        {
            Err(CacheError::Operation(
                "Memcached support is not enabled. Recompile with the 'memcached' feature."
                    .to_string(),
            ))
        }
    }

    async fn flush(&self) -> CacheResult<()> {
        #[cfg(feature = "memcached")]
        {
            let client = self.client.lock().map_err(|e| {
                CacheError::Internal(format!("Memcached client lock poisoned: {}", e))
            })?;
            client
                .flush()
                .map_err(|e| CacheError::Operation(format!("Memcached flush error: {}", e)))?;
            Ok(())
        }

        #[cfg(not(feature = "memcached"))]
        {
            Err(CacheError::Operation(
                "Memcached support is not enabled. Recompile with the 'memcached' feature."
                    .to_string(),
            ))
        }
    }
}
