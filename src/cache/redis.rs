#[cfg(feature = "redis_cache")]
use crate::cache::{CacheConfig, CacheError, CacheResult, CacheService, CacheStats, CacheType};
#[cfg(feature = "redis_cache")]
use async_trait::async_trait;
#[cfg(feature = "redis_cache")]
use redis::{AsyncCommands, Client, ErrorKind, RedisError, Script, aio::ConnectionManager};
#[cfg(feature = "redis_cache")]
use std::collections::HashMap;
#[cfg(feature = "redis_cache")]
use std::sync::Arc;
#[cfg(feature = "redis_cache")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "redis_cache")]
use std::time::Duration;
#[cfg(feature = "redis_cache")]
use uuid::Uuid;

/// Redis cache implementation
#[cfg(feature = "redis_cache")]
pub struct RedisCache {
    /// Redis connection manager
    connection: Arc<ConnectionManager>,
    /// Default TTL for cache entries
    default_ttl: Duration,
    /// Namespace prefix for cache keys
    namespace: Option<String>,
    /// Cache hit counter
    hits: Arc<AtomicU64>,
    /// Cache miss counter
    misses: Arc<AtomicU64>,
}

#[cfg(not(feature = "redis_cache"))]
pub struct RedisCache;

#[cfg(feature = "redis_cache")]
impl RedisCache {
    /// Connect to Redis using the provided configuration
    pub async fn connect(config: &CacheConfig) -> CacheResult<Self> {
        // Build server list
        let server = if config.hosts.is_empty() {
            "127.0.0.1".to_string()
        } else {
            config.hosts[0].clone()
        };

        // Add port if specified
        let server_with_port = if let Some(port) = config.port {
            format!("{}:{}", server, port)
        } else {
            // Default Redis port
            format!("{}:6379", server)
        };

        // Build Redis URL
        let mut redis_url = format!("redis://{}", server_with_port);

        // Add credentials if provided
        if let (Some(username), Some(password)) = (&config.username, &config.password) {
            redis_url = format!("redis://{}:{}@{}", username, password, server_with_port);
        } else if let Some(password) = &config.password {
            redis_url = format!("redis://:{}@{}", password, server_with_port);
        }

        // Create client
        let client = Client::open(redis_url.clone()).map_err(|e| Self::convert_error(e))?;

        // Create connection manager
        let connection = ConnectionManager::new(client)
            .await
            .map_err(|e| Self::convert_error(e))?;

        Ok(Self {
            connection: Arc::new(connection),
            default_ttl: config.get_default_ttl(),
            namespace: config.namespace.clone(),
            hits: Arc::new(AtomicU64::new(0)),
            misses: Arc::new(AtomicU64::new(0)),
        })
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

    /// Convert Redis errors to CacheError
    fn convert_error(err: RedisError) -> CacheError {
        match err.kind() {
            ErrorKind::IoError => CacheError::Connection(format!("Redis I/O error: {}", err)),
            ErrorKind::AuthenticationFailed => {
                CacheError::Connection(format!("Redis authentication failed: {}", err))
            }
            ErrorKind::BusyLoadingError => {
                CacheError::Connection(format!("Redis busy loading: {}", err))
            }
            ErrorKind::InvalidClientConfig => {
                CacheError::Configuration(format!("Redis invalid client config: {}", err))
            }
            ErrorKind::ClusterDown => {
                CacheError::Connection(format!("Redis cluster down: {}", err))
            }
            ErrorKind::MasterDown => CacheError::Connection(format!("Redis master down: {}", err)),
            _ => CacheError::Operation(format!("Redis error: {}", err)),
        }
    }
}

#[cfg(not(feature = "redis_cache"))]
impl RedisCache {
    /// Connect to Redis using the provided configuration
    pub async fn connect(_config: &crate::cache::CacheConfig) -> crate::cache::CacheResult<Self> {
        Err(crate::cache::CacheError::Connection(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }
}

#[cfg(feature = "redis_cache")]
#[async_trait]
impl CacheService for RedisCache {
    async fn get(&self, key: &str) -> CacheResult<Option<Vec<u8>>> {
        let prefixed_key = self.prefix_key(key);
        let mut conn_mgr = self.connection.as_ref().clone();

        // Execute Redis GET command
        match conn_mgr.get::<_, Option<Vec<u8>>>(&prefixed_key).await {
            Ok(value) => {
                if value.is_some() {
                    self.hits.fetch_add(1, Ordering::Relaxed);
                } else {
                    self.misses.fetch_add(1, Ordering::Relaxed);
                }
                Ok(value)
            }
            Err(e) => {
                // Handle nil responses gracefully
                if e.kind() == ErrorKind::TypeError || e.to_string().contains("nil") {
                    self.misses.fetch_add(1, Ordering::Relaxed);
                    Ok(None)
                } else {
                    Err(Self::convert_error(e))
                }
            }
        }
    }

    async fn set(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> CacheResult<()> {
        let prefixed_key = self.prefix_key(key);
        let mut conn_mgr = self.connection.as_ref().clone();
        let expiry = ttl.unwrap_or(self.default_ttl);

        let _: String = conn_mgr
            .set_ex(&prefixed_key, value, expiry.as_secs())
            .await
            .map_err(Self::convert_error)?;

        Ok(())
    }

    async fn delete(&self, key: &str) -> CacheResult<bool> {
        let key = self.prefix_key(key);
        let mut conn_mgr = self.connection.as_ref().clone();
        let count: i64 = conn_mgr.del(&key).await.map_err(Self::convert_error)?;
        Ok(count > 0)
    }

    async fn exists(&self, key: &str) -> CacheResult<bool> {
        let key = self.prefix_key(key);
        let mut conn_mgr = self.connection.as_ref().clone();
        let count: i64 = conn_mgr.exists(&key).await.map_err(Self::convert_error)?;
        Ok(count > 0)
    }

    async fn set_nx(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> CacheResult<bool> {
        let key = self.prefix_key(key);
        let mut conn_mgr = self.connection.as_ref().clone();
        let acquired: bool = conn_mgr
            .set_nx(&key, value)
            .await
            .map_err(Self::convert_error)?;

        if acquired && ttl.is_some() {
            let _: String = conn_mgr
                .expire(&key, ttl.unwrap().as_secs() as i64)
                .await
                .map_err(Self::convert_error)?;
        }

        Ok(acquired)
    }

    async fn get_set(&self, key: &str, value: &[u8]) -> CacheResult<Option<Vec<u8>>> {
        let key = self.prefix_key(key);
        let mut conn_mgr = self.connection.as_ref().clone();
        // Corrected to use getset and handle potential nil response for old_value
        match conn_mgr
            .getset::<&str, &[u8], Option<Vec<u8>>>(&key, value)
            .await
        {
            Ok(old_value) => Ok(old_value),
            Err(e) => {
                if e.kind() == ErrorKind::TypeError || e.to_string().contains("nil") {
                    Ok(None) // Key didn't exist, so no old value
                } else {
                    Err(Self::convert_error(e))
                }
            }
        }
    }

    async fn increment(&self, key: &str, delta: i64) -> CacheResult<i64> {
        let key = self.prefix_key(key);
        let mut conn_mgr = self.connection.as_ref().clone();
        let result: i64 = if delta >= 0 {
            conn_mgr.incr(&key, delta).await
        } else {
            conn_mgr.decr(&key, -delta).await
        }
        .map_err(Self::convert_error)?;
        Ok(result)
    }

    async fn set_many(
        &self,
        items: &HashMap<String, Vec<u8>>,
        ttl: Option<Duration>,
    ) -> CacheResult<()> {
        if items.is_empty() {
            return Ok(());
        }

        let mut conn_mgr = self.connection.as_ref().clone();
        let mut pipe = redis::pipe();

        for (key, value) in items {
            let prefixed_key = self.prefix_key(key);
            if let Some(t) = ttl {
                pipe.add_command(
                    redis::cmd("SET")
                        .arg(&prefixed_key)
                        .arg(value.as_slice())
                        .arg("EX")
                        .arg(t.as_secs())
                        .clone(),
                );
            } else {
                pipe.add_command(
                    redis::cmd("SET")
                        .arg(&prefixed_key)
                        .arg(value.as_slice())
                        .clone(),
                );
            }
        }

        let _: () = pipe
            .query_async(&mut conn_mgr)
            .await
            .map_err(Self::convert_error)?;
        Ok(())
    }

    async fn get_many(&self, keys: &[String]) -> CacheResult<HashMap<String, Vec<u8>>> {
        if keys.is_empty() {
            return Ok(HashMap::new());
        }

        let prefixed_keys: Vec<String> = keys.iter().map(|k| self.prefix_key(k)).collect();

        let mut conn_mgr = self.connection.as_ref().clone();
        // MGET returns Vec<Option<Vec<u8>>>
        let values: Vec<Option<Vec<u8>>> = conn_mgr
            .get(prefixed_keys.as_slice()) // .get on a slice of keys performs MGET
            .await
            .map_err(Self::convert_error)?;

        let mut result = HashMap::new();
        for (i, value_opt) in values.into_iter().enumerate() {
            if let Some(data) = value_opt {
                if let Some(original_key) = keys.get(i) {
                    // Safely get original key
                    result.insert(original_key.clone(), data);
                }
            }
        }

        Ok(result)
    }

    async fn delete_many(&self, keys: &[String]) -> CacheResult<u64> {
        if keys.is_empty() {
            return Ok(0);
        }

        let prefixed_keys: Vec<String> = keys.iter().map(|k| self.prefix_key(k)).collect();

        let mut conn_mgr = self.connection.as_ref().clone();
        let count: i64 = conn_mgr
            .del(prefixed_keys.as_slice())
            .await
            .map_err(Self::convert_error)?;

        Ok(count as u64)
    }

    async fn clear(&self, namespace: Option<&str>) -> CacheResult<()> {
        let mut conn_mgr = self.connection.as_ref().clone();

        let target_namespace = namespace.or_else(|| self.namespace.as_deref());

        let pattern = if let Some(ns) = target_namespace {
            format!("{}:*", ns)
        } else {
            "*".to_string()
        };

        let keys_to_delete: Vec<String> =
            conn_mgr.keys(&pattern).await.map_err(Self::convert_error)?;

        if !keys_to_delete.is_empty() {
            let _: i64 = conn_mgr
                .del(keys_to_delete.as_slice())
                .await
                .map_err(Self::convert_error)?;
        }

        Ok(())
    }

    async fn lock(&self, key: &str, ttl: Duration) -> CacheResult<Option<String>> {
        let lock_key = self.prefix_key(&format!("lock:{}", key));
        let token = Uuid::new_v4().to_string();
        let mut conn_mgr = self.connection.as_ref().clone();

        let acquired: bool = conn_mgr
            .set_nx(&lock_key, token.as_bytes())
            .await
            .map_err(Self::convert_error)?;

        if acquired {
            let _: String = conn_mgr
                .expire(&lock_key, ttl.as_secs() as i64)
                .await
                .map_err(Self::convert_error)?;
            Ok(Some(token))
        } else {
            Ok(None)
        }
    }

    async fn unlock(&self, key: &str, lock_token: &str) -> CacheResult<bool> {
        let lock_key = self.prefix_key(&format!("lock:{}", key));
        let mut conn_mgr = self.connection.as_ref().clone();

        let script_body = r#"
            if redis.call('get', KEYS[1]) == ARGV[1] then
                return redis.call('del', KEYS[1])
            else
                return 0
            end
        "#;

        let result: i64 = Script::new(script_body)
            .key(&lock_key)
            .arg(lock_token.as_bytes())
            .invoke_async(&mut conn_mgr)
            .await
            .map_err(Self::convert_error)?;

        Ok(result > 0)
    }

    fn get_cache_type(&self) -> CacheType {
        CacheType::Redis
    }

    async fn ping(&self) -> CacheResult<()> {
        let mut conn_mgr = self.connection.as_ref().clone();
        let result: String = conn_mgr.ping().await.map_err(Self::convert_error)?;

        if result == "PONG" {
            Ok(())
        } else {
            Err(CacheError::Connection(format!(
                "Unexpected ping response: {}",
                result
            )))
        }
    }

    async fn close(&self) -> CacheResult<()> {
        Ok(())
    }

    fn get_default_ttl(&self) -> Duration {
        self.default_ttl
    }

    async fn stats(&self) -> CacheResult<CacheStats> {
        let mut conn_mgr = self.connection.as_ref().clone();
        let info_str: String = redis::cmd("INFO")
            .query_async(&mut conn_mgr)
            .await
            .map_err(Self::convert_error)?;

        let mut stats = CacheStats {
            hits: Some(self.hits.load(Ordering::Relaxed)),
            misses: Some(self.misses.load(Ordering::Relaxed)),
            ..Default::default()
        };

        for line in info_str.lines() {
            let line_trimmed = line.trim();
            if line_trimmed.starts_with('#') || line_trimmed.is_empty() {
                continue;
            }

            if let Some(pos) = line_trimmed.find(':') {
                let stat_key = &line_trimmed[0..pos];
                let stat_value = &line_trimmed[pos + 1..];

                match stat_key {
                    "used_memory" => {
                        if let Ok(val) = stat_value.parse::<u64>() {
                            stats.memory_used_bytes = Some(val);
                        }
                    }
                    "db0" => {
                        stat_value.split(',').for_each(|part| {
                            if let Some(kv_pair) = part.split_once('=') {
                                if kv_pair.0 == "keys" {
                                    if let Ok(val) = kv_pair.1.parse::<u64>() {
                                        stats.item_count = Some(val);
                                    }
                                }
                            }
                        });
                    }
                    _ => {
                        stats
                            .additional_metrics
                            .insert(stat_key.to_string(), stat_value.to_string());
                    }
                }
            }
        }

        Ok(stats)
    }

    async fn flush(&self) -> CacheResult<()> {
        let mut conn_mgr = self.connection.as_ref().clone();
        // flushdb returns "OK" which can be mapped to String
        let _: String = conn_mgr.flushdb().await.map_err(Self::convert_error)?;
        Ok(())
    }
}

#[cfg(not(feature = "redis_cache"))]
#[async_trait::async_trait]
impl crate::cache::CacheService for RedisCache {
    async fn get(&self, _key: &str) -> crate::cache::CacheResult<Option<Vec<u8>>> {
        Err(crate::cache::CacheError::Operation(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }

    async fn set(
        &self,
        _key: &str,
        _value: &[u8],
        _ttl: Option<std::time::Duration>,
    ) -> crate::cache::CacheResult<()> {
        Err(crate::cache::CacheError::Operation(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }

    async fn delete(&self, _key: &str) -> crate::cache::CacheResult<bool> {
        Err(crate::cache::CacheError::Operation(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }

    async fn exists(&self, _key: &str) -> crate::cache::CacheResult<bool> {
        Err(crate::cache::CacheError::Operation(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }

    async fn set_nx(
        &self,
        _key: &str,
        _value: &[u8],
        _ttl: Option<std::time::Duration>,
    ) -> crate::cache::CacheResult<bool> {
        Err(crate::cache::CacheError::Operation(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }

    async fn get_set(
        &self,
        _key: &str,
        _value: &[u8],
    ) -> crate::cache::CacheResult<Option<Vec<u8>>> {
        Err(crate::cache::CacheError::Operation(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }

    async fn increment(&self, _key: &str, _delta: i64) -> crate::cache::CacheResult<i64> {
        Err(crate::cache::CacheError::Operation(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }

    async fn set_many(
        &self,
        _items: &std::collections::HashMap<String, Vec<u8>>,
        _ttl: Option<std::time::Duration>,
    ) -> crate::cache::CacheResult<()> {
        Err(crate::cache::CacheError::Operation(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }

    async fn get_many(
        &self,
        _keys: &[String],
    ) -> crate::cache::CacheResult<std::collections::HashMap<String, Vec<u8>>> {
        Err(crate::cache::CacheError::Operation(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }

    async fn delete_many(&self, _keys: &[String]) -> crate::cache::CacheResult<u64> {
        Err(crate::cache::CacheError::Operation(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }

    async fn clear(&self, _namespace: Option<&str>) -> crate::cache::CacheResult<()> {
        Err(crate::cache::CacheError::Operation(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }

    async fn lock(
        &self,
        _key: &str,
        _ttl: std::time::Duration,
    ) -> crate::cache::CacheResult<Option<String>> {
        Err(crate::cache::CacheError::Operation(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }

    async fn unlock(&self, _key: &str, _lock_token: &str) -> crate::cache::CacheResult<bool> {
        Err(crate::cache::CacheError::Operation(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }

    fn get_cache_type(&self) -> crate::cache::CacheType {
        crate::cache::CacheType::Redis
    }

    async fn ping(&self) -> crate::cache::CacheResult<()> {
        Err(crate::cache::CacheError::Operation(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }

    async fn close(&self) -> crate::cache::CacheResult<()> {
        Ok(())
    }

    fn get_default_ttl(&self) -> std::time::Duration {
        std::time::Duration::from_secs(300)
    }

    async fn stats(&self) -> crate::cache::CacheResult<crate::cache::CacheStats> {
        Err(crate::cache::CacheError::Operation(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }

    async fn flush(&self) -> crate::cache::CacheResult<()> {
        Err(crate::cache::CacheError::Operation(
            "Redis support is not enabled. Recompile with the 'redis_cache' feature.".to_string(),
        ))
    }
}
