//! Disk-backed search result cache with 6-hour TTL.
//!
//! Used when index misses and a live filesystem search is performed.
//! Cache files live under `~/.isearch/data/search_cache/`.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::core::database;
use crate::core::fs_indexer::FileMetadata;

/// Cache time-to-live: 6 hours
pub const CACHE_TTL_SECS: u64 = 6 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedHit {
    pub path: String,
    pub size: u64,
    pub is_directory: bool,
    pub modified_at: i64,
    pub created_at: i64,
}

impl From<&FileMetadata> for CachedHit {
    fn from(m: &FileMetadata) -> Self {
        Self {
            path: m.path.clone(),
            size: m.size,
            is_directory: m.is_directory,
            modified_at: m.modified_at,
            created_at: m.created_at,
        }
    }
}

impl From<CachedHit> for FileMetadata {
    fn from(h: CachedHit) -> Self {
        FileMetadata {
            path: h.path,
            size: h.size,
            is_directory: h.is_directory,
            modified_at: h.modified_at,
            created_at: h.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheFile {
    /// Normalized query text used as logical key
    query: String,
    /// Unix timestamp when this entry was written
    created_at: u64,
    /// Matching files
    hits: Vec<CachedHit>,
}

pub struct SearchCache {
    dir: PathBuf,
}

impl SearchCache {
    pub fn new() -> Self {
        let dir = database::get_data_dir().join("search_cache");
        Self { dir }
    }

    fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("Failed to create search cache dir {:?}", self.dir))?;
        Ok(())
    }

    fn normalize_query(query: &str) -> String {
        query.trim().to_lowercase()
    }

    fn cache_path(&self, query: &str) -> PathBuf {
        let mut hasher = DefaultHasher::new();
        Self::normalize_query(query).hash(&mut hasher);
        let hash = hasher.finish();
        self.dir.join(format!("{:016x}.json", hash))
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Look up a cached result. Returns `None` on miss or expiry.
    pub fn get(&self, query: &str) -> Option<Vec<CachedHit>> {
        let path = self.cache_path(query);
        if !path.exists() {
            return None;
        }

        let raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to read search cache {:?}: {}", path, e);
                return None;
            }
        };

        let entry: CacheFile = match serde_json::from_str(&raw) {
            Ok(e) => e,
            Err(e) => {
                warn!("Corrupt search cache {:?}: {}", path, e);
                let _ = fs::remove_file(&path);
                return None;
            }
        };

        let age = Self::now_secs().saturating_sub(entry.created_at);
        if age > CACHE_TTL_SECS {
            debug!(
                "Search cache expired for query=\"{}\" age={}s",
                query, age
            );
            let _ = fs::remove_file(&path);
            return None;
        }

        // Guard against hash collisions on different queries
        if entry.query != Self::normalize_query(query) {
            return None;
        }

        info!(
            "[search_cache] HIT query=\"{}\" hits={} age={}s",
            query,
            entry.hits.len(),
            age
        );
        Some(entry.hits)
    }

    /// Persist search results to disk.
    pub fn put(&self, query: &str, hits: &[CachedHit]) -> Result<()> {
        self.ensure_dir()?;
        let path = self.cache_path(query);
        let entry = CacheFile {
            query: Self::normalize_query(query),
            created_at: Self::now_secs(),
            hits: hits.to_vec(),
        };
        let json = serde_json::to_string(&entry)?;
        fs::write(&path, json)
            .with_context(|| format!("Failed to write search cache {:?}", path))?;
        info!(
            "[search_cache] STORE query=\"{}\" hits={} path={:?}",
            query,
            hits.len(),
            path
        );
        Ok(())
    }

    /// Remove expired cache files (best-effort housekeeping).
    pub fn purge_expired(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        let now = Self::now_secs();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(raw) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(cached): Result<CacheFile, _> = serde_json::from_str(&raw) else {
                let _ = fs::remove_file(&path);
                continue;
            };
            if now.saturating_sub(cached.created_at) > CACHE_TTL_SECS {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

impl Default for SearchCache {
    fn default() -> Self {
        Self::new()
    }
}
