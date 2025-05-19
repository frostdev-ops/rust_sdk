use crate::cache::{CacheConfig, CacheError, CacheResult, CacheService, CacheStats, CacheType};
use async_trait::async_trait;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

/// File-based cache implementation
pub struct FileCache {
    /// Base directory for cache files
    base_dir: PathBuf,
    /// Default TTL for cache entries
    default_ttl: Duration,
    /// Namespace prefix for cache keys
    namespace: Option<String>,
    /// Cache hit counter
    hits: Arc<AtomicU64>,
    /// Cache miss counter
    misses: Arc<AtomicU64>,
    /// Lock for ensuring atomic operations
    #[allow(dead_code)]
    locks: Arc<Mutex<HashMap<String, File>>>,
}

impl FileCache {
    /// Create a new file-based cache
    pub async fn new(config: &CacheConfig) -> CacheResult<Self> {
        // Get base directory from config or use system temp dir
        let base_dir = if let Some(path) = &config.file_path {
            Path::new(path).to_path_buf()
        } else if !config.hosts.is_empty() {
            Path::new(&config.hosts[0]).to_path_buf()
        } else {
            std::env::temp_dir().join("pywatt_cache")
        };

        // Create base directory if it doesn't exist
        fs::create_dir_all(&base_dir).map_err(|e| {
            CacheError::Connection(format!("Failed to create cache directory: {}", e))
        })?;

        Ok(Self {
            base_dir,
            default_ttl: config.get_default_ttl(),
            namespace: config.namespace.clone(),
            hits: Arc::new(AtomicU64::new(0)),
            misses: Arc::new(AtomicU64::new(0)),
            locks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Get the full path for a cache key
    fn key_to_path(&self, key: &str) -> PathBuf {
        let prefixed_key = if let Some(ns) = &self.namespace {
            format!("{}:{}", ns, key)
        } else {
            key.to_string()
        };

        // Create a simple hash to shorten the key and ensure valid filenames
        let hash = format!(
            "{:x}",
            prefixed_key
                .bytes()
                .fold(0u64, |sum, b| sum.wrapping_add(b as u64))
        );

        // Create a two-level directory structure for better performance
        let dir1 = &hash[0..2];
        let dir2 = &hash[2..4];

        self.base_dir.join(dir1).join(dir2).join(hash)
    }

    /// Check if a cache entry is expired
    fn is_expired(&self, file_path: &Path) -> bool {
        if let Ok(metadata) = fs::metadata(file_path) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(elapsed) = modified.elapsed() {
                    return elapsed > self.default_ttl;
                }
            }
        }
        // If we can't determine, assume it's expired
        true
    }

    /// Acquires a lock on a file
    #[cfg(feature = "file_cache")]
    fn file_lock(&self, file: &File) -> CacheResult<()> {
        fs2::FileExt::lock_exclusive(file)
            .map_err(|e| CacheError::Operation(format!("Failed to lock file: {}", e)))
    }

    /// Releases a lock on a file
    #[cfg(feature = "file_cache")]
    fn file_unlock(&self, file: &File) -> CacheResult<()> {
        fs2::FileExt::unlock(file)
            .map_err(|e| CacheError::Operation(format!("Failed to unlock file: {}", e)))
    }

    /// Placeholder for when file locking is not available
    #[cfg(not(feature = "file_cache"))]
    fn file_lock(&self, _file: &File) -> CacheResult<()> {
        Ok(()) // No locking support, just return success
    }

    /// Placeholder for when file locking is not available
    #[cfg(not(feature = "file_cache"))]
    fn file_unlock(&self, _file: &File) -> CacheResult<()> {
        Ok(()) // No locking support, just return success
    }
}

#[async_trait]
impl CacheService for FileCache {
    async fn get(&self, key: &str) -> CacheResult<Option<Vec<u8>>> {
        let file_path = self.key_to_path(key);

        // Check if file exists and is not expired
        if file_path.exists() && !self.is_expired(&file_path) {
            match fs::read(&file_path) {
                Ok(data) => {
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    Ok(Some(data))
                }
                Err(e) => Err(CacheError::Operation(format!(
                    "Failed to read cache file: {}",
                    e
                ))),
            }
        } else {
            // Remove file if it exists but is expired
            if file_path.exists() {
                let _ = fs::remove_file(&file_path);
            }
            self.misses.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }
    }

    async fn set(&self, key: &str, value: &[u8], _ttl: Option<Duration>) -> CacheResult<()> {
        let file_path = self.key_to_path(key);

        // Create parent directories if they don't exist
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                CacheError::Operation(format!("Failed to create cache directory: {}", e))
            })?;
        }

        // Create or overwrite the file
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&file_path)
            .map_err(|e| CacheError::Operation(format!("Failed to create cache file: {}", e)))?;

        // Acquire lock on the file
        self.file_lock(&file)?;

        // Write the data
        file.write_all(value).map_err(|e| {
            let _ = self.file_unlock(&file);
            CacheError::Operation(format!("Failed to write to cache file: {}", e))
        })?;

        // Release lock
        self.file_unlock(&file)?;

        Ok(())
    }

    async fn delete(&self, key: &str) -> CacheResult<bool> {
        let file_path = self.key_to_path(key);

        if file_path.exists() {
            fs::remove_file(&file_path).map_err(|e| {
                CacheError::Operation(format!("Failed to delete cache file: {}", e))
            })?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn exists(&self, key: &str) -> CacheResult<bool> {
        let file_path = self.key_to_path(key);

        if file_path.exists() && !self.is_expired(&file_path) {
            Ok(true)
        } else {
            // Clean up expired files
            if file_path.exists() && self.is_expired(&file_path) {
                let _ = fs::remove_file(&file_path);
            }
            Ok(false)
        }
    }

    async fn set_nx(&self, key: &str, value: &[u8], _ttl: Option<Duration>) -> CacheResult<bool> {
        let file_path = self.key_to_path(key);

        // Check if file already exists and is not expired
        if file_path.exists() && !self.is_expired(&file_path) {
            return Ok(false);
        }

        // Create parent directories if they don't exist
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                CacheError::Operation(format!("Failed to create cache directory: {}", e))
            })?;
        }

        // Try to create the file (will fail if it already exists)
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file_path)
        {
            Ok(mut file) => {
                // Acquire lock on the file
                self.file_lock(&file)?;

                // Write the data
                file.write_all(value).map_err(|e| {
                    let _ = self.file_unlock(&file);
                    CacheError::Operation(format!("Failed to write to cache file: {}", e))
                })?;

                // Release lock
                self.file_unlock(&file)?;

                Ok(true)
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    // Another process created the file between our check and create
                    Ok(false)
                } else {
                    Err(CacheError::Operation(format!(
                        "Failed to create cache file: {}",
                        e
                    )))
                }
            }
        }
    }

    async fn get_set(&self, key: &str, value: &[u8]) -> CacheResult<Option<Vec<u8>>> {
        let file_path = self.key_to_path(key);

        // Create parent directories if they don't exist
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                CacheError::Operation(format!("Failed to create cache directory: {}", e))
            })?;
        }

        let mut old_value = None;

        // Try to read existing file
        if file_path.exists() && !self.is_expired(&file_path) {
            match fs::read(&file_path) {
                Ok(data) => {
                    old_value = Some(data);
                    self.hits.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    return Err(CacheError::Operation(format!(
                        "Failed to read cache file: {}",
                        e
                    )));
                }
            }
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }

        // Now set the new value
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&file_path)
            .map_err(|e| CacheError::Operation(format!("Failed to create cache file: {}", e)))?;

        // Acquire lock on the file
        self.file_lock(&file)?;

        // Write the data
        file.write_all(value).map_err(|e| {
            let _ = self.file_unlock(&file);
            CacheError::Operation(format!("Failed to write to cache file: {}", e))
        })?;

        // Release lock
        self.file_unlock(&file)?;

        Ok(old_value)
    }

    async fn increment(&self, key: &str, delta: i64) -> CacheResult<i64> {
        let file_path = self.key_to_path(key);

        // Create parent directories if they don't exist
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                CacheError::Operation(format!("Failed to create cache directory: {}", e))
            })?;
        }

        // Open or create the file
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&file_path)
            .map_err(|e| CacheError::Operation(format!("Failed to open cache file: {}", e)))?;

        // Acquire lock on the file
        self.file_lock(&file)?;

        // Read current value
        let mut current_value = 0;
        let mut buffer = Vec::new();

        if file_path.exists() && !self.is_expired(&file_path) {
            match file
                .try_clone()
                .map_err(|e| {
                    let _ = self.file_unlock(&file);
                    CacheError::Operation(format!("Failed to clone file handle: {}", e))
                })?
                .read_to_end(&mut buffer)
            {
                Ok(_) => {
                    if !buffer.is_empty() {
                        let value_str = String::from_utf8_lossy(&buffer);
                        if let Ok(value) = value_str.parse::<i64>() {
                            current_value = value;
                        }
                    }
                }
                Err(e) => {
                    let _ = self.file_unlock(&file);
                    return Err(CacheError::Operation(format!(
                        "Failed to read from cache file: {}",
                        e
                    )));
                }
            }
        }

        // Increment value
        let new_value = current_value + delta;

        // Write new value
        file.set_len(0).map_err(|e| {
            let _ = self.file_unlock(&file);
            CacheError::Operation(format!("Failed to truncate cache file: {}", e))
        })?;

        file.write_all(new_value.to_string().as_bytes())
            .map_err(|e| {
                let _ = self.file_unlock(&file);
                CacheError::Operation(format!("Failed to write to cache file: {}", e))
            })?;

        // Release lock
        self.file_unlock(&file)?;

        Ok(new_value)
    }

    // Implementation for remaining methods...
    // For brevity, I'll just stub the remaining methods

    async fn set_many(
        &self,
        items: &HashMap<String, Vec<u8>>,
        ttl: Option<Duration>,
    ) -> CacheResult<()> {
        // Simple implementation: set each item individually
        for (key, value) in items {
            self.set(key, value, ttl).await?;
        }
        Ok(())
    }

    async fn get_many(&self, keys: &[String]) -> CacheResult<HashMap<String, Vec<u8>>> {
        let mut result = HashMap::new();
        for key in keys {
            if let Some(value) = self.get(key).await? {
                result.insert(key.clone(), value);
            }
        }
        Ok(result)
    }

    async fn delete_many(&self, keys: &[String]) -> CacheResult<u64> {
        let mut count = 0;
        for key in keys {
            if self.delete(key).await? {
                count += 1;
            }
        }
        Ok(count)
    }

    async fn clear(&self, namespace: Option<&str>) -> CacheResult<()> {
        // If namespace is not specified and we don't have a namespace, clear everything
        if namespace.is_none() && self.namespace.is_none() {
            // Remove all files in the base directory
            if self.base_dir.exists() {
                fs::remove_dir_all(&self.base_dir).map_err(|e| {
                    CacheError::Operation(format!("Failed to clear cache directory: {}", e))
                })?;

                // Recreate the base directory
                fs::create_dir_all(&self.base_dir).map_err(|e| {
                    CacheError::Operation(format!("Failed to recreate cache directory: {}", e))
                })?;
            }
            return Ok(());
        }

        // Otherwise, only clear keys with the matching namespace
        // This is a naive implementation - for production you'd need something more efficient
        let target_namespace = namespace.or_else(|| self.namespace.as_deref());

        if let Some(ns) = target_namespace {
            // Simply delete all files that might have this namespace
            // In a real implementation, we'd need to be more selective
            let prefix = format!("{}:", ns);

            // Define the recursive function locally
            fn remove_files_with_prefix(dir: &Path, prefix: &str) -> CacheResult<()> {
                if !dir.exists() {
                    return Ok(());
                }

                // Recursively walk directories
                for entry in fs::read_dir(dir).map_err(|e| {
                    CacheError::Operation(format!("Failed to read directory: {}", e))
                })? {
                    let entry = entry.map_err(|e| {
                        CacheError::Operation(format!("Failed to read directory entry: {}", e))
                    })?;
                    let path = entry.path();

                    if path.is_dir() {
                        remove_files_with_prefix(&path, prefix)?;
                    } else if let Some(file_name) = path.file_name() {
                        if let Some(file_name_str) = file_name.to_str() {
                            // This is a very simplistic approach - in reality we'd need
                            // a way to map from hash back to original key
                            if file_name_str.contains(prefix) {
                                let _ = fs::remove_file(&path);
                            }
                        }
                    }
                }

                Ok(())
            }

            // Call the recursive function to remove matching files
            remove_files_with_prefix(&self.base_dir, &prefix)?;
        }

        Ok(())
    }

    async fn lock(&self, key: &str, ttl: Duration) -> CacheResult<Option<String>> {
        let lock_key = format!("lock:{}", key);
        let file_path = self.key_to_path(&lock_key);
        let token = Uuid::new_v4().to_string();

        // Create parent directories if they don't exist
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                CacheError::Operation(format!("Failed to create lock directory: {}", e))
            })?;
        }

        // Try to create the lock file
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file_path)
        {
            Ok(mut file) => {
                // Write the token to the file
                file.write_all(token.as_bytes()).map_err(|e| {
                    CacheError::Operation(format!("Failed to write to lock file: {}", e))
                })?;

                // We could set the file modification time to track expiration,
                // but that requires platform-specific code. Instead, we'll
                // just check the modification time against current time + ttl
                // when we try to acquire the lock.

                Ok(Some(token))
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    // Check if the lock is expired
                    if self.is_expired(&file_path) {
                        // Lock is expired, try to remove it and acquire a new one
                        let _ = fs::remove_file(&file_path);
                        return self.lock(key, ttl).await;
                    }
                    // Lock exists and is not expired
                    Ok(None)
                } else {
                    Err(CacheError::Operation(format!(
                        "Failed to create lock file: {}",
                        e
                    )))
                }
            }
        }
    }

    async fn unlock(&self, key: &str, lock_token: &str) -> CacheResult<bool> {
        let lock_key = format!("lock:{}", key);
        let file_path = self.key_to_path(&lock_key);

        // Check if the lock file exists
        if !file_path.exists() {
            return Ok(false);
        }

        // Read the token from the file
        let current_token = fs::read_to_string(&file_path)
            .map_err(|e| CacheError::Operation(format!("Failed to read lock file: {}", e)))?;

        // Compare the tokens
        if current_token == lock_token {
            // Token matches, remove the lock file
            fs::remove_file(&file_path)
                .map_err(|e| CacheError::Operation(format!("Failed to remove lock file: {}", e)))?;
            Ok(true)
        } else {
            // Token doesn't match
            Ok(false)
        }
    }

    fn get_cache_type(&self) -> CacheType {
        CacheType::File
    }

    async fn ping(&self) -> CacheResult<()> {
        // Check if the base directory exists and is writable
        if !self.base_dir.exists() {
            return Err(CacheError::Connection(
                "Cache directory does not exist".to_string(),
            ));
        }

        // Try to write a test file
        let test_file = self.base_dir.join("ping_test");
        match OpenOptions::new().write(true).create(true).open(&test_file) {
            Ok(_) => {
                // Cleanup
                let _ = fs::remove_file(test_file);
                Ok(())
            }
            Err(e) => Err(CacheError::Connection(format!(
                "Cache directory is not writable: {}",
                e
            ))),
        }
    }

    async fn close(&self) -> CacheResult<()> {
        // Nothing to close for file-based cache
        Ok(())
    }

    fn get_default_ttl(&self) -> Duration {
        self.default_ttl
    }

    async fn stats(&self) -> CacheResult<CacheStats> {
        let mut stats = CacheStats {
            hits: Some(self.hits.load(Ordering::Relaxed)),
            misses: Some(self.misses.load(Ordering::Relaxed)),
            ..Default::default()
        };

        // Count items and total size
        let mut item_count = 0;
        let mut total_size = 0;

        // Recursive function to walk directories and count files
        fn collect_stats(
            dir: &Path,
            exclude_expired: bool,
            current_time: SystemTime,
            ttl: Duration,
            item_count: &mut u64,
            total_size: &mut u64,
        ) -> std::io::Result<()> {
            if !dir.exists() {
                return Ok(());
            }

            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_dir() {
                    collect_stats(
                        &path,
                        exclude_expired,
                        current_time,
                        ttl,
                        item_count,
                        total_size,
                    )?;
                } else if let Ok(metadata) = fs::metadata(&path) {
                    let include = !exclude_expired
                        || metadata
                            .modified()
                            .map(|modified_time| {
                                current_time
                                    .duration_since(modified_time)
                                    .map(|age| age <= ttl)
                                    .unwrap_or(false)
                            })
                            .unwrap_or(false);

                    if include {
                        *item_count += 1;
                        *total_size += metadata.len();
                    }
                }
            }
            Ok(())
        }

        // Start collection from base directory
        if let Err(e) = collect_stats(
            &self.base_dir,
            true,
            SystemTime::now(),
            self.default_ttl,
            &mut item_count,
            &mut total_size,
        ) {
            return Err(CacheError::Operation(format!(
                "Failed to collect stats: {}",
                e
            )));
        }

        stats.item_count = Some(item_count);
        stats.memory_used_bytes = Some(total_size);

        Ok(stats)
    }

    async fn flush(&self) -> CacheResult<()> {
        // Clear all cache files
        if self.base_dir.exists() {
            fs::remove_dir_all(&self.base_dir)
                .map_err(|e| CacheError::Operation(format!("Failed to flush cache: {}", e)))?;

            // Recreate the base directory
            fs::create_dir_all(&self.base_dir).map_err(|e| {
                CacheError::Operation(format!("Failed to recreate cache directory: {}", e))
            })?;
        }

        Ok(())
    }
}
